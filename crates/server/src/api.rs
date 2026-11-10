//! JSON API. Every handler is a plain async function over [`Shared`], so the
//! same code can later sit behind Tauri commands.

use crate::{api_key, bad, claude_identity, default_user, enter_user, fail, git, open_workspace, provider, provider_for, resolve_engine, secrets, settings, unavailable, ApiError, Loaded, Resolved, Shared};
use axum::extract::{Path as AxPath, Query, State};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use chrono::{DateTime, Duration, NaiveTime, Utc};
use jl_core::catalog;
use jl_core::db::{ItemKind, NewArticle, NewGrammar, NewMistake, NewWord};
use jl_core::google::CalendarEvent;
use jl_core::import::{self, ImportReport};
use jl_core::lesson::{self, LessonInput, Templates};
use jl_core::news::{self, Source};
use jl_core::report;
use jl_core::level;
use jl_core::workspace::{slug, LangProfile, Layout, Profile, Root, Workspace};
use jl_core::Lang;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio::sync::RwLockReadGuard;
use tower_http::services::ServeDir;

type R<T> = Result<Json<T>, ApiError>;

const BUNDLED_SOURCES: &str = include_str!("../../../workspace-template/sources.toml");

pub fn routes(web: PathBuf) -> Router<Shared> {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/user", put(update_user))
        .route("/api/users", post(create_user))
        .route("/api/users/select", post(select_user))
        .route("/api/workspace/migrate", post(migrate_workspace))
        .route("/api/workspace/reindex", post(reindex))
        .route("/api/setup/workspace", post(setup_workspace))
        .route("/api/setup/new", post(setup_new))
        .route("/api/setup/clone", post(setup_clone))
        .route("/api/git/status", get(git_status))
        .route("/api/git/pull", post(git_pull))
        .route("/api/git/sync", post(git_sync))
        .route("/api/sources", get(sources))
        .route("/api/news", get(news_list))
        .route("/api/articles", get(articles).post(fetch_article))
        .route("/api/articles/{id}", get(article))
        .route("/api/lessons", get(lessons).post(create_lesson))
        .route("/api/lessons/{id}", get(get_lesson))
        .route("/api/lessons/{id}/answers", post(answer_lesson))
        .route("/api/words", get(words).post(add_word))
        .route("/api/words/{id}", delete(delete_word))
        .route("/api/grammar", get(grammar).post(add_grammar))
        .route("/api/grammar/{id}", delete(delete_grammar))
        .route("/api/review", get(review_due).post(review_grade))
        .route("/api/mistakes", get(mistakes).post(add_mistake))
        .route("/api/mistakes/{id}/resolve", post(resolve_mistake))
        .route("/api/materials", get(materials))
        .route("/api/import", post(import_file).layer(axum::extract::DefaultBodyLimit::max(20 * 1024 * 1024)))
        .route("/api/import/file", post(import_material_file).layer(axum::extract::DefaultBodyLimit::max(MATERIAL_FILE_MAX + 1024)))
        .route("/api/records/export", post(export_records))
        .route("/api/claude", get(claude_status))
        .route("/api/settings/claude", post(claude_settings))
        .route("/api/settings/engine", post(set_engine))
        .route("/api/claude-code/login", post(claude_code_login))
        .route("/api/engine/test", post(engine_test))
        .route("/api/models", get(models))
        .route("/api/settings/llama", post(set_llama))
        .route("/api/google", get(google_status))
        .route("/api/google/outbox", get(google_outbox))
        .route("/api/google/weekly-report", post(weekly_report))
        .route("/api/google/schedule-reviews", post(schedule_reviews))
        .route("/api/google/book-lesson", post(book_lesson))
        .route("/api/open/{page}", post(open_page))
        .merge(crate::study::routes())
        .merge(crate::courses::routes())
        .merge(crate::notebook::routes())
        .merge(crate::speech::routes())
        .merge(crate::setup::routes())
        .merge(crate::family::routes())
        .merge(crate::today::routes())
        .merge(crate::interests::routes())
        .merge(crate::prepare::routes())
        .merge(crate::inbox::routes())
        .merge(crate::library::routes())
        .merge(crate::buddies::routes())
        .fallback_service(ServeDir::new(web))
}

// ── helpers ─────────────────────────────────────────────

/// The chosen user's workspace, or 503 with the reason there is none.
pub(crate) async fn ws(s: &Shared) -> Result<RwLockReadGuard<'_, Loaded>, ApiError> {
    let g = s.loaded.read().await;
    if g.is_none() {
        let why = match (s.load_error.read().await.clone(), s.root.read().await.as_ref()) {
            (Some(e), _) => e,
            (None, Some(r)) if r.root.layout() == Layout::V1 => "這個工作區需要先轉成多使用者結構".into(),
            (None, Some(_)) => "請先選擇使用者".into(),
            (None, None) => "尚未選擇工作區".into(),
        };
        return Err(unavailable(why));
    }
    Ok(RwLockReadGuard::map(g, |o| o.as_ref().unwrap()))
}

/// The open workspace folder (with or without a chosen user).
pub(crate) async fn root_path(s: &Shared) -> Result<PathBuf, ApiError> {
    s.root.read().await.as_ref().map(|r| r.root.root.clone()).ok_or_else(|| unavailable("尚未選擇工作區"))
}

#[derive(Deserialize, Default)]
struct LangQuery {
    lang: Option<String>,
    q: Option<String>,
    limit: Option<i64>,
    all: Option<bool>,
    refresh: Option<bool>,
}

pub(crate) fn parse_lang(l: &Option<String>) -> Result<Option<Lang>, ApiError> {
    match l.as_deref().map(str::trim).filter(|s| !s.is_empty() && *s != "all") {
        None => Ok(None),
        Some(s) => s.parse().map(Some).map_err(bad),
    }
}

pub(crate) fn need_lang(l: &Option<String>) -> Result<Lang, ApiError> {
    parse_lang(l)?.ok_or_else(|| bad("請指定語言 lang=en 或 ja"))
}

fn today() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

/// `dir/name.ext`, or `dir/name-2.ext`… if taken. Never overwrites.
pub(crate) fn free_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    p
}

/// Append one row to a workspace CSV, writing the header first if new.
fn append_csv(path: &Path, header: &[&str], row: &[&str]) -> Result<(), ApiError> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| fail(e.to_string()))?;
    }
    let new = !path.exists() || std::fs::metadata(path).map(|m| m.len() == 0).unwrap_or(true);
    // Make sure an existing file ends with a newline before appending.
    if !new {
        let text = std::fs::read(path).map_err(|e| fail(e.to_string()))?;
        if text.last() != Some(&b'\n') {
            std::fs::OpenOptions::new()
                .append(true)
                .open(path)
                .and_then(|mut f| std::io::Write::write_all(&mut f, b"\n"))
                .map_err(|e| fail(e.to_string()))?;
        }
    }
    let file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| fail(e.to_string()))?;
    let mut w = csv::WriterBuilder::new().has_headers(false).from_writer(file);
    if new {
        w.write_record(header).map_err(|e| fail(e.to_string()))?;
    }
    w.write_record(row).map_err(|e| fail(e.to_string()))?;
    w.flush().map_err(|e| fail(e.to_string()))
}

/// Remove rows whose first `key_cols` match from a workspace CSV (only the
/// app's own `my-*.csv` files are edited this way).
fn remove_csv_rows(path: &Path, key: &[&str]) -> Result<usize, ApiError> {
    let Ok(text) = std::fs::read_to_string(path) else { return Ok(0) };
    let mut rdr = csv::ReaderBuilder::new().flexible(true).from_reader(text.as_bytes());
    let headers = rdr.headers().map_err(|e| fail(e.to_string()))?.clone();
    let mut kept = vec![];
    let mut removed = 0;
    for rec in rdr.records().flatten() {
        if key.iter().enumerate().all(|(i, k)| rec.get(i).unwrap_or("").trim() == k.trim()) {
            removed += 1;
        } else {
            kept.push(rec);
        }
    }
    if removed > 0 {
        let mut w = csv::Writer::from_path(path).map_err(|e| fail(e.to_string()))?;
        w.write_record(&headers).map_err(|e| fail(e.to_string()))?;
        for r in kept {
            w.write_record(&r).map_err(|e| fail(e.to_string()))?;
        }
        w.flush().map_err(|e| fail(e.to_string()))?;
    }
    Ok(removed)
}

// ── status & workspace ──────────────────────────────────

#[derive(Serialize)]
struct UserCard {
    id: String,
    name: String,
    color: String,
    /// URL of the user's picture on this computer, if any.
    avatar: Option<String>,
    kind: String,
    admin: bool,
}

#[derive(Serialize)]
struct Status {
    workspace: Option<String>,
    workspace_name: Option<String>,
    demo: bool,
    error: Option<String>,
    /// `v1` (needs migration) or `v2`.
    layout: Option<Layout>,
    users: Vec<UserCard>,
    /// The chosen user (their profile.toml), when there is one.
    user: Option<Value>,
    admin: bool,
    db: Option<String>,
    /// Majors per language with a self-assessment hint.
    level_options: Value,
    engine: String,
    engine_ready: bool,
    /// 「Claude 訂閱」「Claude API」「本機模型」
    engine_label: String,
    engine_error: Option<String>,
    /// Conversations' own engine, when set apart from the lessons' one.
    realtime_engine: Option<String>,
    realtime_label: Option<String>,
    realtime_ready: bool,
    google: &'static str,
    import: Option<ImportReport>,
    stats: Value,
    prompts_overridden: Vec<String>,
    /// Family mode (plan §19.2.5): `{ member, ready }`; never the PIN.
    family: Option<Value>,
}

fn level_options() -> Value {
    json!(Lang::ALL
        .iter()
        .map(|&l| (
            l.code(),
            level::majors(l).iter().map(|m| json!({ "major": m, "hint": level::self_assessment_hint(l, m), "steps": level::steps_in(l, m) })).collect::<Vec<_>>()
        ))
        .collect::<std::collections::BTreeMap<_, _>>())
}

/// profile.toml plus the levels spelled out for the UI.
fn user_json(ws: &Workspace) -> Value {
    let p = &ws.profile;
    let mut v = serde_json::to_value(p).unwrap_or_default();
    let mut langs = serde_json::Map::new();
    for &l in &p.languages {
        let lp = p.lang(l);
        let d = level::from_score(l, lp.difficulty);
        langs.insert(
            l.code().into(),
            json!({ "level": lp.level, "difficulty": lp.difficulty, "difficulty_label": d.label, "major": d.major, "step": d.step, "steps": d.steps, "early_feedback_left": lp.early_feedback_left }),
        );
    }
    v["levels"] = Value::Object(langs);
    // The learner's own interests (interests.csv), heaviest first: the
    // one-tap keywords on the topic course page.
    let mut ints = jl_core::course::topics::parse_interests(&std::fs::read_to_string(ws.interests_file()).unwrap_or_default());
    ints.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal).then(a.keyword.cmp(&b.keyword)));
    v["interests"] = json!(ints.iter().map(|i| json!({ "keyword": i.keyword, "lang": i.lang, "weight": i.weight })).collect::<Vec<_>>());
    v
}

async fn status(State(s): State<Shared>) -> R<Status> {
    let (engine, engine_ready, engine_label, engine_error) = match &s.provider_override {
        Some(p) => (p.id(), true, "測試".to_string(), None),
        None => {
            let st = s.settings.read().await;
            let (model, llama) = (st.claude.model.clone(), st.llama.model.clone());
            drop(st);
            match resolve_engine(&s, false).await {
                Ok(r @ Resolved::LlamaServer) => (format!("llama-server:{llama}"), true, r.label().to_string(), None),
                Ok(r) => (model, true, r.label().to_string(), None),
                Err(e) => (model, false, "未連線".to_string(), Some(e)),
            }
        }
    };
    let rt_kind = s.settings.read().await.realtime_engine;
    let (realtime_engine, realtime_label, realtime_ready) = match (rt_kind, &s.provider_override) {
        (Some(k), None) => match crate::resolve_engine_task(&s, crate::Task::Realtime, false).await {
            Ok(r) => (Some(k.as_str().to_string()), Some(r.label().to_string()), true),
            Err(_) => (Some(k.as_str().to_string()), None, false),
        },
        _ => (None, None, engine_ready),
    };
    let error = s.load_error.read().await.clone();
    let st_now = s.settings.read().await.clone();
    let root = s.root.read().await;
    let (workspace, workspace_name, demo, layout, users) = match root.as_ref() {
        Some(r) => (
            Some(r.root.root.display().to_string()),
            Some(r.root.meta.name.clone()).filter(|n| !n.is_empty()),
            r.demo,
            Some(r.root.layout()),
            r.root
                .list_profiles()
                .into_iter()
                .map(|p| UserCard {
                    kind: r.root.meta.members.iter().find(|m| m.id == p.id).map(|m| format!("{:?}", m.kind).to_lowercase()).unwrap_or_else(|| "household".into()),
                    admin: r.root.is_admin(&p.id),
                    avatar: crate::study::avatar_url(&st_now, &r.root.meta.id, &p.id),
                    id: p.id,
                    name: p.name,
                    color: p.color,
                })
                .collect(),
        ),
        None => (None, None, false, None, vec![]),
    };
    let admin_of = |id: &str| root.as_ref().is_some_and(|r| r.root.is_admin(id));
    let guard = s.loaded.read().await;
    let mut out = Status {
        workspace,
        workspace_name,
        demo,
        error,
        layout,
        users,
        user: None,
        admin: false,
        db: None,
        level_options: level_options(),
        engine,
        engine_ready,
        engine_label,
        engine_error,
        realtime_engine,
        realtime_label,
        realtime_ready,
        google: s.mailer.mode(),
        import: None,
        stats: json!({}),
        prompts_overridden: vec![],
        family: s.settings.read().await.family.as_ref().map(|f| json!({ "member": f.member, "ready": f.member.is_some() && f.pin_hash.is_some() })),
    };
    let Some(l) = guard.as_ref() else { return Ok(Json(out)) };
    let now = Utc::now();
    let mut stats = serde_json::Map::new();
    for lang in Lang::ALL {
        let st = l.db.stats(Some(lang), now - Duration::days(7), now).await?;
        stats.insert(lang.code().into(), serde_json::to_value(st).unwrap_or_default());
    }
    let mut u = user_json(&l.ws);
    u["avatar"] = json!(crate::study::avatar_url(&st_now, &l.ws.meta.id, &l.ws.profile.id));
    out.user = Some(u);
    out.admin = admin_of(&l.ws.profile.id);
    out.db = Some(l.db_path.display().to_string());
    out.import = Some(l.import.clone());
    out.stats = Value::Object(stats);
    out.prompts_overridden = Templates::load(Some(&l.ws.prompts_dir())).overridden;
    Ok(Json(out))
}

// ── users ───────────────────────────────────────────────

/// Enter as a user and remember the choice for this computer.
async fn select_user_inner(s: &Shared, id: &str) -> Result<Value, ApiError> {
    let st = s.settings.read().await.clone();
    let loaded = {
        let root = s.root.read().await;
        let open = root.as_ref().ok_or_else(|| unavailable("尚未選擇工作區"))?;
        if open.root.layout() == Layout::V1 {
            return Err(bad("這個工作區需要先轉成多使用者結構"));
        }
        enter_user(&st, open, id).await.map_err(bad)?
    };
    let ws_id = loaded.ws.meta.id.clone();
    if let Some(dir) = &st.app_dir {
        settings::write_config_value(dir, Some("active_users"), &ws_id, toml::Value::String(id.into())).map_err(fail)?;
    }
    s.settings.write().await.active_users.insert(ws_id, id.into());
    let report = loaded.import.clone();
    let user = user_json(&loaded.ws);
    *s.loaded.write().await = Some(loaded);
    *s.load_error.write().await = None;
    Ok(json!({ "user": user, "import": report }))
}

/// Load the active user again after their files changed outside the open
/// workspace (their own iPad results, lessons prepared for their own iPad).
pub(crate) async fn reload_active(s: &Shared) {
    let me = s.loaded.read().await.as_ref().map(|l| l.ws.profile.id.clone());
    if let Some(me) = me {
        let _ = select_user_inner(s, &me).await;
    }
}

#[derive(Deserialize)]
struct SelectBody {
    id: String,
}

async fn select_user(State(s): State<Shared>, Json(b): Json<SelectBody>) -> R<Value> {
    Ok(Json(select_user_inner(&s, b.id.trim()).await?))
}

#[derive(Deserialize, Default)]
struct UserBody {
    name: Option<String>,
    color: Option<String>,
    languages: Option<Vec<Lang>>,
    /// Self-assessed major level per language, e.g. `{ "en": "B1" }`.
    majors: Option<std::collections::BTreeMap<Lang, String>>,
    daily_minutes: Option<u32>,
    report_email: Option<String>,
    seconds_per_turn: Option<u32>,
    turns: Option<u32>,
    interest_ratio: Option<f32>,
    interests: Option<Vec<String>>,
    /// UI colour theme id (one of `workspace::THEMES`).
    theme: Option<String>,
}

/// Apply a settings change to a profile; validates everything first.
fn apply_user(p: &mut Profile, b: &UserBody) -> Result<(), ApiError> {
    if let Some(n) = &b.name {
        let n = n.trim();
        if n.is_empty() || n.chars().count() > 40 {
            return Err(bad("名字需為 1–40 個字"));
        }
        p.name = n.into();
    }
    if let Some(c) = &b.color {
        let ok = c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit());
        if !ok {
            return Err(bad("顏色格式需為 #rrggbb"));
        }
        p.color = c.clone();
    }
    if let Some(langs) = &b.languages {
        if langs.is_empty() {
            return Err(bad("至少選一種學習語言"));
        }
        let mut l = langs.clone();
        l.sort();
        l.dedup();
        p.languages = l;
    }
    if let Some(majors) = &b.majors {
        for (lang, major) in majors {
            let major = major.trim().to_ascii_uppercase();
            if level::major_start(*lang, &major).is_none() {
                return Err(bad(format!("{} 的程度只能是 {}", lang.label(), level::majors(*lang).join("、"))));
            }
            // A new self-assessment restarts this language at that major
            // level; choosing the same major keeps the progress made.
            if p.lang.get(lang).map(|lp| level::from_score(*lang, lp.difficulty).major.to_string()) != Some(major.clone()) {
                p.lang.insert(*lang, LangProfile::starting_at(*lang, &major));
            }
        }
    }
    for &l in &p.languages.clone() {
        p.lang.entry(l).or_insert_with(|| LangProfile::starting_at(l, l.default_major()));
    }
    if let Some(m) = b.daily_minutes {
        if !(5..=240).contains(&m) {
            return Err(bad("每日目標需為 5–240 分鐘"));
        }
        p.daily_minutes = m;
    }
    if let Some(e) = &b.report_email {
        let e = e.trim();
        if !e.is_empty() && (!e.contains('@') || e.contains(char::is_whitespace)) {
            return Err(bad("email 格式不正確"));
        }
        p.report_email = Some(e.to_string()).filter(|s| !s.is_empty());
    }
    if let Some(sec) = b.seconds_per_turn {
        if !(5..=120).contains(&sec) {
            return Err(bad("每回合秒數需為 5–120"));
        }
        p.conversation.seconds_per_turn = sec;
    }
    if let Some(t) = b.turns {
        if !(5..=40).contains(&t) {
            return Err(bad("對話回合需為 5–40"));
        }
        p.conversation.turns = t;
    }
    if let Some(r) = b.interest_ratio {
        if !(0.0..=1.0).contains(&r) {
            return Err(bad("興趣比例需在 0–1 之間"));
        }
        p.conversation.interest_ratio = r;
    }
    if let Some(t) = &b.theme {
        if !jl_core::workspace::THEMES.contains(&t.as_str()) {
            return Err(bad("未知的主題"));
        }
        p.theme = Some(t.clone());
    }
    Ok(())
}

/// Append keywords to interests.csv (keyword,lang,weight,updated_at).
pub(crate) fn add_interests(ws: &Workspace, keywords: &[String]) -> Result<usize, ApiError> {
    let file = ws.interests_file();
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    let have: Vec<String> = existing.lines().skip(1).filter_map(|l| l.split(',').next()).map(|k| k.trim().trim_matches('"').to_lowercase()).collect();
    let now = Utc::now().to_rfc3339();
    let mut n = 0;
    for k in keywords {
        let k = k.trim();
        if k.is_empty() || have.contains(&k.to_lowercase()) {
            continue;
        }
        append_csv(&file, &["keyword", "lang", "weight", "updated_at"], &[k, "", "1", &now])?;
        n += 1;
    }
    Ok(n)
}

/// Add a user to the workspace (anyone at this computer may; the workspace
/// is shared by trust) and enter as them.
async fn create_user(State(s): State<Shared>, Json(b): Json<UserBody>) -> R<Value> {
    let name = b.name.clone().unwrap_or_default();
    let mut p = Profile::new("", name.trim());
    apply_user(&mut p, &b)?;
    let template = s.settings.read().await.paths.template.clone();
    let ws = {
        let mut root = s.root.write().await;
        let open = root.as_mut().ok_or_else(|| unavailable("尚未選擇工作區"))?;
        if open.root.layout() == Layout::V1 {
            return Err(bad("這個工作區需要先轉成多使用者結構"));
        }
        open.root.create_profile(&template, p).map_err(|e| bad(e.to_string()))?
    };
    if let Some(k) = &b.interests {
        add_interests(&ws, k)?;
    }
    Ok(Json(select_user_inner(&s, &ws.profile.id).await?))
}

/// Change the chosen user's own settings.
async fn update_user(State(s): State<Shared>, Json(b): Json<UserBody>) -> R<Value> {
    let mut g = s.loaded.write().await;
    let l = g.as_mut().ok_or_else(|| unavailable("請先選擇使用者"))?;
    let mut p = l.ws.profile.clone();
    apply_user(&mut p, &b)?;
    l.ws.profile = p;
    l.ws.save_profile().map_err(|e| fail(e.to_string()))?;
    if let Some(k) = &b.interests {
        add_interests(&l.ws, k)?;
    }
    // Keep the member list's display name in step.
    if b.name.is_some() {
        let mut root = s.root.write().await;
        if let Some(open) = root.as_mut()
            && let Some(m) = open.root.meta.members.iter_mut().find(|m| m.id == l.ws.profile.id)
        {
            m.name = l.ws.profile.name.clone();
            open.root.save().map_err(|e| fail(e.to_string()))?;
        }
    }
    Ok(Json(json!({ "user": user_json(&l.ws) })))
}

#[derive(Deserialize)]
struct MigrateBody {
    name: String,
}

/// Turn a first-version workspace into the multi-user layout; the person
/// running it becomes its owner.
async fn migrate_workspace(State(s): State<Shared>, Json(b): Json<MigrateBody>) -> R<Value> {
    if b.name.trim().is_empty() {
        return Err(bad("請輸入名字"));
    }
    let template = s.settings.read().await.paths.template.clone();
    let m = {
        let mut root = s.root.write().await;
        let open = root.as_mut().ok_or_else(|| unavailable("尚未選擇工作區"))?;
        open.root.migrate_v1(&template, b.name.trim(), None).map_err(|e| bad(e.to_string()))?
    };
    let mut out = select_user_inner(&s, &m.profile).await?;
    out["migration"] = json!(m);
    Ok(Json(out))
}

async fn reindex(State(s): State<Shared>) -> R<ImportReport> {
    let l = ws(&s).await?;
    Ok(Json(import::sync_workspace(&l.ws, &l.db).await))
}

#[derive(Deserialize)]
struct WorkspaceBody {
    path: String,
    #[serde(default)]
    init: bool,
}

/// Switch to a workspace folder and remember it in config.toml; enter the
/// user this computer used there last (or the only one).
pub(crate) async fn use_workspace(s: &Shared, path: PathBuf, init: bool) -> Result<Value, ApiError> {
    let st = s.settings.read().await.clone();
    let open = open_workspace(&st, Some(path.clone()), init).map_err(bad)?;
    if let Some(dir) = &st.app_dir {
        settings::write_config_value(dir, None, "workspace_path", toml::Value::String(path.display().to_string())).map_err(fail)?;
    }
    s.settings.write().await.workspace = Some(path.clone());
    let layout = open.root.layout();
    let next = if layout == Layout::V2 { default_user(&st, &open.root) } else { None };
    let users = open.root.list_profiles().len();
    *s.root.write().await = Some(open);
    *s.loaded.write().await = None;
    *s.load_error.write().await = None;
    let mut out = json!({ "workspace": path.display().to_string(), "layout": layout, "users": users });
    if let Some(id) = next {
        let entered = select_user_inner(s, &id).await?;
        out["user"] = entered["user"].clone();
        out["import"] = entered["import"].clone();
    }
    Ok(out)
}

/// A folder the user typed for a workspace: `~` expanded, and refused when
/// empty, relative (it would land wherever the app happens to run), the home
/// folder itself, or inside the JELearner program folder (the public repo,
/// where personal files must never go).
pub(crate) fn user_path(s: &settings::Settings, raw: &str) -> Result<PathBuf, ApiError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(bad("請輸入資料夾位置"));
    }
    let home = dirs::home_dir();
    let path = if raw == "~" {
        home.clone().ok_or_else(|| bad("找不到使用者資料夾"))?
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home.clone().ok_or_else(|| bad("找不到使用者資料夾"))?.join(rest)
    } else {
        PathBuf::from(raw)
    };
    if !path.is_absolute() {
        return Err(bad("請輸入完整的位置，例如 ~/JELearner-private"));
    }
    if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(bad("位置不能包含 ..，請輸入完整的位置"));
    }
    if home.as_deref() == Some(path.as_path()) {
        return Err(bad("請在使用者資料夾裡另建一個資料夾，例如 ~/JELearner-private"));
    }
    let program = s.paths.web.parent().map(Path::to_path_buf);
    if let Some(program) = program
        && let (Ok(prog), Some(target)) = (program.canonicalize(), existing_ancestor(&path))
        && target.starts_with(&prog)
    {
        return Err(bad("工作區不能放在 JELearner 程式的資料夾裡（那是公開的 repo），請選別的位置"));
    }
    Ok(path)
}

/// The path, or the nearest folder above it that exists, made canonical.
fn existing_ancestor(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|a| a.exists()).and_then(|a| a.canonicalize().ok())
}

/// Refuse a workspace inside another Git repository (it would be committed
/// into that repo, or nest one clone in another; plan §19.2.3).
async fn not_nested(path: &Path) -> Result<(), ApiError> {
    match crate::setup::enclosing_repo(path).await {
        Some(top) => Err(bad(format!("這個位置在另一個 Git repository（{}）裡面，請換一個位置", top.display()))),
        None => Ok(()),
    }
}

async fn setup_workspace(State(s): State<Shared>, Json(b): Json<WorkspaceBody>) -> R<Value> {
    let path = user_path(&*s.settings.read().await, &b.path)?;
    not_nested(&path).await?;
    Ok(Json(use_workspace(&s, path, b.init).await?))
}

#[derive(Deserialize)]
struct NewBody {
    path: String,
    remote: Option<String>,
}

/// New workspace from the template; `git init`, and push to the user's own
/// empty private repo when a remote is given. Users are added next.
async fn setup_new(State(s): State<Shared>, Json(b): Json<NewBody>) -> R<Value> {
    let path = user_path(&*s.settings.read().await, &b.path)?;
    if path.exists() && std::fs::read_dir(&path).map(|mut d| d.next().is_some()).unwrap_or(false) {
        return Err(bad("資料夾不是空的；要使用現有資料夾請選「開啟資料夾」"));
    }
    not_nested(&path).await?;
    let template = s.settings.read().await.paths.template.clone();
    Root::init(&path, &template).map_err(|e| bad(e.to_string()))?;
    let mut out = use_workspace(&s, path.clone(), true).await?;
    match git::init_and_push(&path, b.remote.as_deref().map(str::trim).filter(|r| !r.is_empty())).await {
        Ok(msg) => out["git"] = json!(msg),
        Err(e) => out["git_error"] = json!(e),
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
struct CloneBody {
    url: String,
    /// The folder that holds the clone (`<parent>/<repo name>`)…
    #[serde(default)]
    parent: String,
    /// …or the clone's own folder (the setup wizard), see `setup::target_for`.
    #[serde(default)]
    path: String,
}

async fn setup_clone(State(s): State<Shared>, Json(b): Json<CloneBody>) -> R<Value> {
    let url = b.url.trim();
    let name = url.trim_end_matches('/').trim_end_matches(".git").rsplit(['/', ':']).next().unwrap_or("").to_string();
    if name.is_empty() {
        return Err(bad("repo 網址無效"));
    }
    let settings = s.settings.read().await.clone();
    let target = if b.path.trim().is_empty() {
        let parent = if b.parent.trim().is_empty() { "~" } else { b.parent.trim() };
        user_path(&settings, parent)?.join(&name)
    } else {
        crate::setup::target_for("clone", &user_path(&settings, &b.path)?, url)
    };
    not_nested(&target).await?;
    git::clone(url, &target).await.map_err(bad)?;
    Ok(Json(use_workspace(&s, target, true).await?))
}

async fn git_status(State(s): State<Shared>) -> R<git::GitStatus> {
    let root = root_path(&s).await?;
    Ok(Json(git::status(&root).await))
}

async fn git_pull(State(s): State<Shared>) -> R<Value> {
    let root = root_path(&s).await?;
    let pulled = git::pull(&root).await.map_err(bad)?;
    if pulled {
        // Another device may have added users or changed workspace.toml.
        if let Some(open) = s.root.write().await.as_mut()
            && let Ok(fresh) = Root::open(&open.root.root)
        {
            open.root = fresh;
        }
        let rep = match ws(&s).await {
            Ok(l) => Some(import::sync_workspace(&l.ws, &l.db).await),
            Err(_) => None,
        };
        return Ok(Json(json!({ "pulled": true, "import": rep })));
    }
    Ok(Json(json!({ "pulled": false })))
}

#[derive(Deserialize, Default)]
struct SyncBody {
    message: Option<String>,
}

async fn git_sync(State(s): State<Shared>, body: Option<Json<SyncBody>>) -> R<Value> {
    let root = root_path(&s).await?;
    let msg = body.and_then(|b| b.0.message);
    let first = git::sync(&root, msg).await.map_err(bad)?;
    // Family members' iPad results came down with the pull: apply them on
    // the admin's computer and send the outcome back up (plan F5).
    let is_admin = {
        let me = s.loaded.read().await.as_ref().map(|l| l.ws.profile.id.clone()).unwrap_or_default();
        s.root.read().await.as_ref().is_some_and(|o| o.root.is_admin(&me))
    };
    let applied = if is_admin { crate::inbox::process_all(&s).await } else { vec![] };
    // Everyone's points for the members' iPads (the buddies strip).
    let buddies = if is_admin { crate::buddies::write_ipad_files(&s).await } else { 0 };
    let result = if applied.iter().any(|a| a.files > 0) || buddies > 0 {
        git::sync(&root, Some("Apply iPad results".into())).await.unwrap_or(first)
    } else {
        first
    };
    let mut out = serde_json::to_value(&result).map_err(|e| fail(e.to_string()))?;
    out["inbox"] = json!(applied);
    Ok(Json(out))
}

// ── news & articles ─────────────────────────────────────

fn load_sources(l: &Loaded) -> (Vec<Source>, Option<String>) {
    news::load_sources(&l.ws.sources_file(), BUNDLED_SOURCES)
}

async fn sources(State(s): State<Shared>) -> R<Value> {
    let l = ws(&s).await?;
    let (sources, error) = load_sources(&l);
    Ok(Json(json!({ "sources": sources, "error": error })))
}

/// Article list for a language; with `refresh=true` the enabled feeds are
/// fetched first (robots.txt and rate limits apply).
/// Fetch the enabled feeds (for one language or all) into the articles
/// table; returns how many items came in and the feed errors.
pub(crate) async fn refresh_news(s: &Shared, lang: Option<Lang>) -> Result<(usize, Vec<String>), ApiError> {
    let mut errors = vec![];
    let mut fetched = 0;
    let (sources, err) = {
        let l = ws(&s).await?;
        load_sources(&l)
    };
    errors.extend(err);
    for src in sources.iter().filter(|x| x.enabled && lang.is_none_or(|l| l == x.lang)) {
        match news::fetch_feed(&s.fetcher, src).await {
            Ok(items) => {
                let l = ws(&s).await?;
                for it in items.iter().take(30) {
                    let a = NewArticle {
                        lang: Some(it.lang),
                        source: it.source.clone(),
                        url: it.url.clone(),
                        title: it.title.clone(),
                        summary: it.summary.clone(),
                        body: String::new(),
                        published_at: it.published_at,
                    };
                    l.db.upsert_article(&a, it.lang).await?;
                    fetched += 1;
                }
            }
            Err(e) => errors.push(e),
        }
    }

    Ok((fetched, errors))
}

async fn news_list(State(s): State<Shared>, Query(q): Query<LangQuery>) -> R<Value> {
    let lang = parse_lang(&q.lang)?;
    let (fetched, errors) = if q.refresh.unwrap_or(false) { refresh_news(&s, lang).await? } else { (0, vec![]) };
    let list = ws(&s).await?.db.list_articles(lang, q.limit.unwrap_or(60)).await?;
    // Pages robots.txt does not let us fetch are shown but cannot be picked
    // (docs/plan.md §19.1); the answer is cached per site.
    let mut out = Vec::with_capacity(list.len());
    for a in list {
        let blocked = a.body.trim().is_empty() && a.url.starts_with("http") && !s.fetcher.allowed(&a.url).await.unwrap_or(false);
        let mut v = json!(a);
        v["blocked"] = json!(blocked);
        out.push(v);
    }
    // The ones that can be used first; blocked pages last (stable order).
    out.sort_by_key(|v| v["blocked"] == json!(true));
    Ok(Json(json!({ "articles": out, "fetched": fetched, "errors": errors })))
}

async fn articles(State(s): State<Shared>, Query(q): Query<LangQuery>) -> R<Value> {
    let l = ws(&s).await?;
    Ok(Json(json!(l.db.list_articles(parse_lang(&q.lang)?, q.limit.unwrap_or(60)).await?)))
}

async fn article(State(s): State<Shared>, AxPath(id): AxPath<i64>) -> R<Value> {
    let l = ws(&s).await?;
    let a = l.db.get_article(id).await?.ok_or_else(|| bad("找不到文章"))?;
    Ok(Json(json!(a)))
}

#[derive(Deserialize)]
struct FetchBody {
    url: String,
    lang: Option<String>,
}

/// Fetch one article page (robots.txt checked) and store its text.
pub(crate) async fn fetch_article_into_db(s: &Shared, url: &str, lang: Lang, source: &str) -> Result<i64, ApiError> {
    let selector = {
        let l = ws(s).await?;
        let (sources, _) = load_sources(&l);
        let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string));
        sources
            .into_iter()
            .find(|x| url::Url::parse(&x.feed).ok().and_then(|u| u.host_str().map(str::to_string)) == host)
            .and_then(|x| x.body_selector)
    };
    let art = news::fetch_article(&s.fetcher, url, selector.as_deref()).await.map_err(|e| bad(e.to_string()))?;
    if art.paragraphs.is_empty() {
        return Err(bad("讀不到文章內文。可以改用「貼上文字」"));
    }
    let l = ws(s).await?;
    let a = NewArticle { lang: Some(lang), source: source.to_string(), url: url.to_string(), title: art.title.clone(), body: art.body(), ..Default::default() };
    Ok(l.db.upsert_article(&a, lang).await?)
}

async fn fetch_article(State(s): State<Shared>, Json(b): Json<FetchBody>) -> R<Value> {
    let lang = need_lang(&b.lang)?;
    let host = url::Url::parse(b.url.trim()).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let id = fetch_article_into_db(&s, b.url.trim(), lang, &host).await?;
    let l = ws(&s).await?;
    Ok(Json(json!(l.db.get_article(id).await?)))
}

// ── lessons ─────────────────────────────────────────────

#[derive(Deserialize)]
struct LessonBody {
    lang: Option<String>,
    article_id: Option<i64>,
    url: Option<String>,
    title: Option<String>,
    text: Option<String>,
    level: Option<String>,
}

async fn create_lesson(State(s): State<Shared>, Json(b): Json<LessonBody>) -> R<Value> {
    // 1. The article: stored, fetched from a URL, or pasted text.
    let article_id = if let Some(id) = b.article_id {
        let a = ws(&s).await?.db.get_article(id).await?.ok_or_else(|| bad("找不到文章"))?;
        if a.body.trim().is_empty() {
            let lang: Lang = a.lang.parse().map_err(bad)?;
            fetch_article_into_db(&s, &a.url, lang, &a.source).await?
        } else {
            id
        }
    } else if let Some(url) = b.url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
        let lang = need_lang(&b.lang)?;
        let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
        fetch_article_into_db(&s, url, lang, &host).await?
    } else if let Some(text) = b.text.as_deref().filter(|t| !t.trim().is_empty()) {
        let lang = need_lang(&b.lang)?;
        let title = b.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| text.chars().take(30).collect());
        let a = NewArticle {
            lang: Some(lang),
            source: "貼上".into(),
            url: format!("manual://{}", Utc::now().timestamp_millis()),
            title,
            body: text.to_string(),
            ..Default::default()
        };
        ws(&s).await?.db.upsert_article(&a, lang).await?
    } else {
        return Err(bad("請選一篇新聞、貼上網址或貼上文字"));
    };

    let (input, templates) = {
        let l = ws(&s).await?;
        let a = l.db.get_article(article_id).await?.ok_or_else(|| fail("文章消失了"))?;
        let lang: Lang = a.lang.parse().map_err(bad)?;
        // An explicit level (e.g. `B1-3` or `B1`) wins; otherwise the
        // learner's current content difficulty.
        let level = b.level.as_deref().and_then(|v| level::parse(lang, v)).unwrap_or_else(|| l.ws.profile.difficulty(lang)).label;
        let known: Vec<String> = l.db.list_words(Some(lang), "", 400).await?.into_iter().map(|w| w.term).collect();
        let url = if a.url.starts_with("manual://") || a.url.starts_with("material://") { String::new() } else { a.url.clone() };
        (
            LessonInput { lang, level, native_language: l.ws.profile.native_language.clone(), title: a.title, url, source: a.source, text: a.body, known_words: known },
            Templates::load(Some(&l.ws.prompts_dir())),
        )
    };

    // 2. The model (no lock held while it runs).
    let p = provider(&s).await?;
    let (lesson, markdown) = lesson::generate(p.as_ref(), &templates, &input, &today()).await.map_err(|e| match e {
        lesson::LessonError::Provider(_) => ApiError(axum::http::StatusCode::BAD_GATEWAY, e.to_string()),
        _ => bad(e.to_string()),
    })?;

    // 3. Save: Markdown file in the workspace, rows in the database.
    let l = ws(&s).await?;
    let dir = l.ws.lessons_dir(input.lang);
    std::fs::create_dir_all(&dir).map_err(|e| fail(e.to_string()))?;
    let path = free_path(&dir, &format!("{}-{}", today(), slug(&lesson.title, 40)), "md");
    let full = lesson::embed_data(&markdown, &lesson);
    std::fs::write(&path, &full).map_err(|e| fail(e.to_string()))?;
    let rel = l.ws.relative(&path);
    let data = serde_json::to_value(&lesson).map_err(|e| fail(e.to_string()))?;
    let id = l.db.insert_lesson(input.lang, &lesson.title, &input.level, Some(article_id), &input.url, &rel, &full, &data, &p.id()).await?;
    let mut words = 0;
    for v in &lesson.vocabulary {
        let w = NewWord {
            lang: Some(input.lang),
            term: v.term.clone(),
            reading: v.reading.clone(),
            meaning: v.meaning.clone(),
            pos: v.part_of_speech.clone(),
            example: format!("{}｜{}", v.example, v.example_translation),
            source: rel.clone(),
            ..Default::default()
        };
        l.db.upsert_word(&w, input.lang).await?;
        words += 1;
    }
    let mut grammar = 0;
    for g in &lesson.grammar {
        let x = NewGrammar {
            lang: Some(input.lang),
            pattern: g.pattern.clone(),
            meaning: g.meaning.clone(),
            explanation: g.explanation.clone(),
            example: format!("{}｜{}", g.example, g.example_translation),
            level: input.level.clone(),
            source: rel.clone(),
            ..Default::default()
        };
        l.db.upsert_grammar(&x, input.lang).await?;
        grammar += 1;
    }
    Ok(Json(json!({
        "id": id, "path": rel, "engine": p.id(), "lesson": lesson, "markdown": markdown,
        "added": { "words": words, "grammar": grammar }
    })))
}

async fn lessons(State(s): State<Shared>, Query(q): Query<LangQuery>) -> R<Value> {
    let l = ws(&s).await?;
    Ok(Json(json!(l.db.list_lessons(parse_lang(&q.lang)?, q.limit.unwrap_or(100)).await?)))
}

async fn get_lesson(State(s): State<Shared>, AxPath(id): AxPath<i64>) -> R<Value> {
    let l = ws(&s).await?;
    let row = l.db.get_lesson(id).await?.ok_or_else(|| bad("找不到教材"))?;
    let data: Value = serde_json::from_str(&row.data).unwrap_or(Value::Null);
    Ok(Json(json!({ "lesson": data, "row": row })))
}

#[derive(Deserialize)]
struct Answer {
    index: usize,
    answer: String,
    /// For short answers the learner marks themselves after seeing the model answer.
    correct: Option<bool>,
}

#[derive(Deserialize)]
struct AnswersBody {
    answers: Vec<Answer>,
}

async fn answer_lesson(State(s): State<Shared>, AxPath(id): AxPath<i64>, Json(b): Json<AnswersBody>) -> R<Value> {
    let l = ws(&s).await?;
    let row = l.db.get_lesson(id).await?.ok_or_else(|| bad("找不到教材"))?;
    let lesson: lesson::Lesson = serde_json::from_str(&row.data).map_err(|e| fail(e.to_string()))?;
    let lang: Lang = row.lang.parse().map_err(bad)?;
    let mut results = vec![];
    for a in b.answers {
        let q = lesson.questions.get(a.index).ok_or_else(|| bad(format!("沒有第 {} 題", a.index + 1)))?;
        let verdict = a.correct.or_else(|| lesson::grade(q, &a.answer));
        let mut mistake_id = None;
        if verdict == Some(false) {
            let m = NewMistake {
                lang: Some(lang),
                item_kind: "question".into(),
                item_id: None,
                lesson_id: Some(id),
                question: q.question.clone(),
                user_answer: a.answer.clone(),
                correct_answer: q.answer.clone(),
                explanation: q.explanation.clone(),
            };
            mistake_id = Some(l.db.add_mistake(&m, lang).await?);
        }
        results.push(json!({
            "index": a.index, "correct": verdict, "answer": q.answer, "explanation": q.explanation, "mistake_id": mistake_id
        }));
    }
    Ok(Json(json!({ "results": results })))
}

// ── words & grammar ─────────────────────────────────────

async fn words(State(s): State<Shared>, Query(q): Query<LangQuery>) -> R<Value> {
    let l = ws(&s).await?;
    Ok(Json(json!(l.db.list_words(parse_lang(&q.lang)?, q.q.as_deref().unwrap_or(""), q.limit.unwrap_or(500)).await?)))
}

const WORD_HEADER: &[&str] = &["term", "reading", "meaning", "pos", "example", "tags", "note"];
const GRAMMAR_HEADER: &[&str] = &["pattern", "meaning", "explanation", "example", "level", "tags"];

/// Added by hand: goes into the database *and* `vocab/my-words-<lang>.csv`,
/// so it is part of the private workspace and syncs.
async fn add_word(State(s): State<Shared>, Json(w): Json<NewWord>) -> R<Value> {
    let lang = w.lang.ok_or_else(|| bad("請指定語言"))?;
    if w.term.trim().is_empty() {
        return Err(bad("單字不能是空的"));
    }
    let l = ws(&s).await?;
    let file = l.ws.vocab_dir().join(format!("my-words-{}.csv", lang.code()));
    append_csv(&file, WORD_HEADER, &[&w.term, &w.reading, &w.meaning, &w.pos, &w.example, &w.tags, &w.note])?;
    let w = NewWord { source: l.ws.relative(&file), ..w };
    let id = l.db.upsert_word(&w, lang).await?;
    Ok(Json(json!({ "id": id, "file": l.ws.relative(&file) })))
}

async fn delete_word(State(s): State<Shared>, AxPath(id): AxPath<i64>) -> R<Value> {
    let l = ws(&s).await?;
    let found = l.db.list_words(None, "", i64::MAX).await?.into_iter().find(|w| w.id == id);
    let mut removed_from = None;
    if let Some(w) = &found {
        let file = l.ws.vocab_dir().join(format!("my-words-{}.csv", w.lang));
        if remove_csv_rows(&file, &[&w.term, &w.reading])? > 0 {
            removed_from = Some(l.ws.relative(&file));
        }
    }
    let deleted = l.db.delete_word(id).await?;
    Ok(Json(json!({ "deleted": deleted, "removed_from": removed_from, "source": found.map(|w| w.source) })))
}

async fn grammar(State(s): State<Shared>, Query(q): Query<LangQuery>) -> R<Value> {
    let l = ws(&s).await?;
    Ok(Json(json!(l.db.list_grammar(parse_lang(&q.lang)?, q.q.as_deref().unwrap_or(""), q.limit.unwrap_or(500)).await?)))
}

async fn add_grammar(State(s): State<Shared>, Json(g): Json<NewGrammar>) -> R<Value> {
    let lang = g.lang.ok_or_else(|| bad("請指定語言"))?;
    if g.pattern.trim().is_empty() {
        return Err(bad("文法不能是空的"));
    }
    let l = ws(&s).await?;
    let file = l.ws.grammar_dir().join(format!("my-grammar-{}.csv", lang.code()));
    append_csv(&file, GRAMMAR_HEADER, &[&g.pattern, &g.meaning, &g.explanation, &g.example, &g.level, &g.tags])?;
    let g = NewGrammar { source: l.ws.relative(&file), ..g };
    let id = l.db.upsert_grammar(&g, lang).await?;
    Ok(Json(json!({ "id": id, "file": l.ws.relative(&file) })))
}

async fn delete_grammar(State(s): State<Shared>, AxPath(id): AxPath<i64>) -> R<Value> {
    let l = ws(&s).await?;
    let found = l.db.list_grammar(None, "", i64::MAX).await?.into_iter().find(|g| g.id == id);
    let mut removed_from = None;
    if let Some(g) = &found {
        let file = l.ws.grammar_dir().join(format!("my-grammar-{}.csv", g.lang));
        if remove_csv_rows(&file, &[&g.pattern])? > 0 {
            removed_from = Some(l.ws.relative(&file));
        }
    }
    let deleted = l.db.delete_grammar(id).await?;
    Ok(Json(json!({ "deleted": deleted, "removed_from": removed_from, "source": found.map(|g| g.source) })))
}

// ── review & mistakes ───────────────────────────────────

async fn review_due(State(s): State<Shared>, Query(q): Query<LangQuery>) -> R<Value> {
    let l = ws(&s).await?;
    Ok(Json(json!(l.db.due_items(parse_lang(&q.lang)?, Utc::now(), q.limit.unwrap_or(50)).await?)))
}

#[derive(Deserialize)]
struct Grade {
    kind: ItemKind,
    id: i64,
    grade: u8,
    /// Part of a course lesson's review unit: the course unit carries the
    /// point, so no separate review round is counted.
    #[serde(default)]
    in_course: bool,
}

async fn review_grade(State(s): State<Shared>, Json(g): Json<Grade>) -> R<Value> {
    if !(1..=5).contains(&g.grade) {
        return Err(bad("grade 需為 1–5"));
    }
    let lang = {
        let l = ws(&s).await?;
        l.db.record_review(g.kind, g.id, g.grade, Utc::now()).await?;
        let table = if g.kind == ItemKind::Word { "words" } else { "grammar" };
        let lang: Option<String> = sqlx::query_scalar(&format!("SELECT lang FROM {table} WHERE id = ?")).bind(g.id).fetch_optional(l.db.pool()).await?;
        lang.and_then(|l| l.parse::<Lang>().ok())
    };
    // A full round of cards is a unit worth a point.
    let round = match lang {
        Some(_) if g.in_course => json!({}),
        Some(lang) => crate::study::review_round(&s, lang).await?,
        None => json!({}),
    };
    Ok(Json(round))
}

async fn mistakes(State(s): State<Shared>, Query(q): Query<LangQuery>) -> R<Value> {
    let l = ws(&s).await?;
    Ok(Json(json!(l.db.list_mistakes(parse_lang(&q.lang)?, q.all.unwrap_or(false)).await?)))
}

async fn add_mistake(State(s): State<Shared>, Json(m): Json<NewMistake>) -> R<Value> {
    let lang = m.lang.ok_or_else(|| bad("請指定語言"))?;
    if m.question.trim().is_empty() {
        return Err(bad("題目不能是空的"));
    }
    let l = ws(&s).await?;
    Ok(Json(json!({ "id": l.db.add_mistake(&m, lang).await? })))
}

async fn resolve_mistake(State(s): State<Shared>, AxPath(id): AxPath<i64>) -> R<Value> {
    let l = ws(&s).await?;
    Ok(Json(json!({ "resolved": l.db.resolve_mistake(id).await? })))
}

// ── materials & import ──────────────────────────────────

async fn materials(State(s): State<Shared>) -> R<Value> {
    let l = ws(&s).await?;
    Ok(Json(json!(l.db.list_materials().await?)))
}

#[derive(Deserialize)]
struct ImportQuery {
    kind: String,
    name: Option<String>,
    lang: Option<String>,
}

/// Upload a CSV (vocab / grammar) or a weekly JSON (materials). The file is
/// saved into the workspace first (never overwriting), then imported.
async fn import_file(State(s): State<Shared>, Query(q): Query<ImportQuery>, body: String) -> R<Value> {
    let body = body.trim_start_matches('\u{feff}').to_string();
    if body.trim().is_empty() {
        return Err(bad("檔案是空的"));
    }
    let l = ws(&s).await?;
    let fallback = l.ws.profile.languages.first().copied().unwrap_or(Lang::En);
    let raw_name = q.name.clone().unwrap_or_default();
    let stem_src = Path::new(&raw_name).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    let lang = parse_lang(&q.lang)?;
    // Drag and drop: find out what the file is.
    let kind = if q.kind == "auto" {
        import::detect_kind(&raw_name, &body).ok_or_else(|| bad("看不出這是什麼檔案：單字 CSV 需要 term／word／單字 欄，文法 CSV 需要 pattern／文法 欄，每週素材是 JSON"))?.to_string()
    } else {
        q.kind.clone()
    };
    let (path, report) = match kind.as_str() {
        "vocab" | "grammar" => {
            let lang = lang.unwrap_or_else(|| import::lang_from_path(Path::new(&raw_name), fallback));
            let ext = if raw_name.to_lowercase().ends_with(".tsv") { "tsv" } else { "csv" };
            let dir = if kind == "vocab" { l.ws.vocab_dir() } else { l.ws.grammar_dir() };
            let stem = format!("{}-{}", slug(if stem_src.is_empty() { "imported" } else { &stem_src }, 40), lang.code());
            let stem = if stem_src.ends_with(&format!("-{}", lang.code())) { slug(&stem_src, 44) } else { stem };
            std::fs::create_dir_all(&dir).map_err(|e| fail(e.to_string()))?;
            let path = free_path(&dir, &stem, ext);
            let origin = l.ws.relative(&path);
            // Validate before writing, so a wrong file leaves no trace.
            let check = jl_core::db::Db::memory().await?;
            let r = if kind == "vocab" {
                import::import_vocab_csv(&check, &body, lang, &origin).await
            } else {
                import::import_grammar_csv(&check, &body, lang, &origin).await
            };
            r.map_err(|e| bad(e.to_string()))?;
            std::fs::write(&path, &body).map_err(|e| fail(e.to_string()))?;
            let r = if kind == "vocab" {
                import::import_vocab_csv(&l.db, &body, lang, &origin).await
            } else {
                import::import_grammar_csv(&l.db, &body, lang, &origin).await
            };
            (path, r.map_err(|e| bad(e.to_string()))?)
        }
        "materials" => {
            let m = import::parse_material(&body).map_err(|e| bad(e.to_string()))?;
            let dir = l.ws.materials_dir().join(&m.week);
            std::fs::create_dir_all(&dir).map_err(|e| fail(e.to_string()))?;
            let stem = slug(if stem_src.is_empty() { &m.title } else { &stem_src }, 40);
            let path = free_path(&dir, &stem, "json");
            std::fs::write(&path, &body).map_err(|e| fail(e.to_string()))?;
            let r = import::import_material(&l.db, &body, &l.ws.relative(&path)).await.map_err(|e| bad(e.to_string()))?;
            (path, r)
        }
        other => return Err(bad(format!("不支援的匯入類型：{other}（vocab、grammar、materials、auto）"))),
    };
    Ok(Json(json!({ "file": l.ws.relative(&path), "kind": kind, "report": report })))
}

/// Upper limit of a file kept as material (PDF; images and text are smaller).
const MATERIAL_FILE_MAX: usize = 20 * 1024 * 1024;

#[derive(Deserialize)]
struct FileQuery {
    name: String,
}

/// What a material file is, from its extension and its first bytes:
/// (stored extension, kind, size limit). `None` for anything else.
pub(crate) fn material_file_kind(name: &str, body: &[u8]) -> Option<(&'static str, &'static str, usize)> {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let starts = |m: &[u8]| body.starts_with(m);
    match ext.as_str() {
        "md" | "markdown" | "txt" => {
            let text = std::str::from_utf8(body).ok()?;
            (!text.trim().is_empty() && !text.contains('\0')).then_some((if ext == "txt" { "txt" } else { "md" }, "text", 2 * 1024 * 1024))
        }
        "pdf" if starts(b"%PDF-") => Some(("pdf", "pdf", MATERIAL_FILE_MAX)),
        "png" if starts(b"\x89PNG\r\n\x1a\n") => Some(("png", "image", 10 * 1024 * 1024)),
        "jpg" | "jpeg" if starts(&[0xFF, 0xD8, 0xFF]) => Some(("jpg", "image", 10 * 1024 * 1024)),
        "webp" if body.len() > 12 && starts(b"RIFF") && &body[8..12] == b"WEBP" => Some(("webp", "image", 10 * 1024 * 1024)),
        "gif" if starts(b"GIF87a") || starts(b"GIF89a") => Some(("gif", "image", 10 * 1024 * 1024)),
        _ => None,
    }
}

/// Keep a Markdown / text note, a PDF or a picture as material for later
/// (stage C reads PDFs and pictures): saved into the profile's `materials/`
/// under a safe, free name (never overwriting). Text notes are kept as they
/// are; nothing is imported into review.
async fn import_material_file(State(s): State<Shared>, Query(q): Query<FileQuery>, body: axum::body::Bytes) -> R<Value> {
    let (ext, kind, max) = material_file_kind(&q.name, &body).ok_or_else(|| bad("只收 CSV、TSV、JSON、Markdown、文字檔、PDF 與圖片（PNG、JPEG、WebP、GIF）"))?;
    if body.len() > max {
        return Err(bad(format!("檔案太大（上限 {} MB）", max / 1024 / 1024)));
    }
    let l = ws(&s).await?;
    let dir = l.ws.materials_dir();
    std::fs::create_dir_all(&dir).map_err(|e| fail(e.to_string()))?;
    let stem_src = Path::new(&q.name).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    let stem = slug(if stem_src.is_empty() { "material" } else { &stem_src }, 40);
    let stem = if stem.is_empty() { "material".to_string() } else { stem };
    let path = free_path(&dir, &stem, ext);
    std::fs::write(&path, &body).map_err(|e| fail(e.to_string()))?;
    Ok(Json(json!({ "file": l.ws.relative(&path), "kind": kind, "bytes": body.len() })))
}

async fn export_records(State(s): State<Shared>) -> R<Value> {
    let l = ws(&s).await?;
    let (progress, mistakes) = import::export_records(&l.ws, &l.db).await.map_err(|e| fail(e.to_string()))?;
    Ok(Json(json!({ "progress": progress, "mistakes": mistakes, "dir": l.ws.relative(&l.ws.records_dir()) })))
}

// ── engines & models ────────────────────────────────────

#[derive(Deserialize, Default)]
struct FreshQuery {
    fresh: Option<bool>,
}

async fn claude_status(State(s): State<Shared>, Query(q): Query<FreshQuery>) -> R<Value> {
    let (model, effort, models, engine) = {
        let st = s.settings.read().await;
        (st.claude.model.clone(), st.claude.effort.clone(), st.catalog.cloud.models.clone(), st.engine)
    };
    let (realtime, defaults) = {
        let st = s.settings.read().await;
        let c = &st.catalog.cloud;
        (st.realtime.clone(), json!({ "model": c.default, "realtime": c.realtime.clone().unwrap_or_else(|| c.default.clone()) }))
    };
    let k = api_key(&s).await;
    let (who, cli) = claude_identity(&s).await;
    // Without a chosen user nobody's sign-in is shown.
    let sub = if who.is_some() { s.cc.get(&cli, q.fresh.unwrap_or(false)).await } else { Default::default() };
    let active = resolve_engine(&s, false).await;
    Ok(Json(json!({
        "engine": engine.as_str(),
        "active": active.as_ref().ok(),
        "active_label": active.as_ref().map(|r| r.label()).ok(),
        "active_error": active.err(),
        "api": {
            "configured": k.is_some(),
            "source": k.as_ref().map(|(_, src)| *src),
            "masked": k.as_ref().map(|(k, _)| secrets::mask(k)),
        },
        "subscription": sub,
        "install": jl_core::claude_code::INSTALL_HINT,
        "model": model,
        "effort": effort,
        "realtime_model": realtime.model,
        "realtime_effort": realtime.effort,
        "defaults": defaults,
        "models": models,
    })))
}

#[derive(Deserialize)]
struct EngineBody {
    /// The engine for lessons and courses (and conversations, unless set apart).
    kind: Option<String>,
    /// Conversations: an engine, or "same" for the lessons' one.
    realtime: Option<String>,
}

/// Choose the engine: auto, subscription, api or llama-server.
async fn set_engine(State(s): State<Shared>, Json(b): Json<EngineBody>) -> R<Value> {
    let mut st = s.settings.write().await;
    if let Some(k) = b.kind.as_deref() {
        let kind = settings::EngineKind::parse(k).ok_or_else(|| bad("engine 只能是 auto、subscription、api、llama-server"))?;
        if let Some(dir) = &st.app_dir {
            settings::write_config_value(dir, Some("engine"), "kind", toml::Value::String(kind.as_str().into())).map_err(fail)?;
        }
        st.engine = kind;
    }
    if let Some(r) = b.realtime.as_deref() {
        let rt = if r == "same" || r.is_empty() { None } else { Some(settings::EngineKind::parse(r).ok_or_else(|| bad("對話的引擎只能是 same、auto、subscription、api、llama-server"))?) };
        if let Some(dir) = &st.app_dir {
            // "same" leaves the key out, so the file says only what was chosen.
            let v = rt.map(|k| toml::Value::String(k.as_str().into())).unwrap_or_else(|| toml::Value::String("same".into()));
            settings::write_config_value(dir, Some("engine"), "realtime", v).map_err(fail)?;
        }
        st.realtime_engine = rt;
    }
    Ok(Json(json!({ "engine": st.engine.as_str(), "realtime": st.realtime_engine.map(|k| k.as_str()).unwrap_or("same") })))
}

/// `claude auth login --claudeai`: the browser opens on Anthropic's sign-in
/// page; the UI polls `/api/claude?fresh=true` until it lands.
async fn claude_code_login(State(s): State<Shared>) -> R<Value> {
    let (who, cli) = claude_identity(&s).await;
    if who.is_none() {
        return Err(bad("請先選擇使用者，登入的會是這位使用者自己的 Claude 帳號"));
    }
    jl_core::claude_code::start_login(&cli).map_err(bad)?;
    Ok(Json(json!({ "started": true })))
}

#[derive(Deserialize)]
struct ClaudeSettingsBody {
    key: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    realtime_model: Option<String>,
}

async fn claude_settings(State(s): State<Shared>, Json(b): Json<ClaudeSettingsBody>) -> R<Value> {
    let mut saved = None;
    if let Some(k) = &b.key {
        let (who, _) = claude_identity(&s).await;
        let who = who.ok_or_else(|| bad("請先選擇使用者，key 會存給這位使用者"))?;
        let st = s.settings.read().await;
        saved = Some(secrets::save_key(k, st.use_keychain, st.app_dir.as_deref(), &who).map_err(fail)?);
    }
    let mut st = s.settings.write().await;
    if let Some(m) = b.model.filter(|m| !m.trim().is_empty()) {
        let m = m.trim().to_string();
        if let Some(dir) = &st.app_dir {
            settings::write_config_value(dir, Some("claude"), "model", toml::Value::String(m.clone())).map_err(fail)?;
        }
        st.claude.model = m;
    }
    if let Some(m) = b.realtime_model.filter(|m| !m.trim().is_empty()) {
        let m = m.trim().to_string();
        if let Some(dir) = &st.app_dir {
            settings::write_config_value(dir, Some("claude"), "realtime_model", toml::Value::String(m.clone())).map_err(fail)?;
        }
        st.realtime.model = m;
    }
    if let Some(e) = b.effort {
        let e = e.trim().to_string();
        if !["", "low", "medium", "high", "xhigh", "max"].contains(&e.as_str()) {
            return Err(bad("effort 只能是 low、medium、high、xhigh、max"));
        }
        if let Some(dir) = &st.app_dir {
            settings::write_config_value(dir, Some("claude"), "effort", toml::Value::String(e.clone())).map_err(fail)?;
        }
        st.claude.effort = Some(e).filter(|e| !e.is_empty());
    }
    Ok(Json(json!({ "ok": true, "stored_in": saved })))
}

#[derive(Deserialize, Default)]
struct TestBody {
    /// Test this path instead of the current choice: subscription | api.
    engine: Option<String>,
}

async fn engine_test(State(s): State<Shared>, body: Option<Json<TestBody>>) -> R<Value> {
    let want = body.and_then(|b| b.0.engine);
    let (p, label): (std::sync::Arc<dyn jl_core::provider::LlmProvider>, &str) = match want.as_deref() {
        None | Some("") => {
            let label = resolve_engine(&s, true).await.map(|r| r.label()).unwrap_or("");
            (provider(&s).await?, label)
        }
        Some(w) => {
            if claude_identity(&s).await.0.is_none() && s.settings.read().await.claude_api_key.is_none() {
                return Err(bad("請先選擇使用者"));
            }
            let which = match w {
                "subscription" => Resolved::Subscription,
                "api" => {
                    if api_key(&s).await.is_none() {
                        return Err(unavailable("尚未設定 API key"));
                    }
                    Resolved::Api
                }
                _ => return Err(bad("engine 只能是 subscription 或 api")),
            };
            (provider_for(&s, which).await?, which.label())
        }
    };
    let req = jl_core::provider::CompletionRequest {
        system: "Reply with exactly: OK".into(),
        user: "Say OK.".into(),
        max_tokens: 1024,
        json_schema: None,
    };
    let t0 = std::time::Instant::now();
    let reply = p.complete(&req).await.map_err(|e| ApiError(axum::http::StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(json!({ "engine": p.id(), "via": label, "reply": reply.chars().take(80).collect::<String>(), "ms": t0.elapsed().as_millis() })))
}

async fn models(State(s): State<Shared>) -> R<Value> {
    let st = s.settings.read().await;
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let memory_gb = sys.total_memory() as f64 / 1024f64.powi(3);
    let c = &st.catalog;
    let defaults = [c.defaults.brain.as_str(), c.defaults.ears.as_str(), c.defaults.mouth.as_str()];
    let running = match s.local_llm.lock().await.as_mut() {
        Some(r) => r.alive().then(|| r.model_id.clone()),
        None => None,
    };
    // Per brain: downloaded into the shared folder, running, chosen.
    let brains: Vec<Value> = c
        .models
        .iter()
        .filter(|m| m.role == "brain")
        .map(|m| {
            json!({
                "id": m.id,
                "installed": st.llama_home.as_deref().is_some_and(|h| jl_core::local_llm::find_model(h, m).is_some()),
                "running": running.as_deref() == Some(m.id.as_str()),
                "chosen": st.llama.model == m.id,
            })
        })
        .collect();
    Ok(Json(json!({
        "catalog": c,
        "memory_gb": (memory_gb * 10.0).round() / 10.0,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "plan_here": catalog::plan(c, &defaults, memory_gb),
        "plan_mac32": catalog::plan(c, &defaults, 32.0),
        "home": st.llama_home,
        "runtime": st.llama_home.as_deref().is_some_and(|h| jl_core::local_llm::find_runtime(h).is_some()),
        "external": st.llama_external.then(|| st.llama.url.clone()),
        "engine": st.engine.as_str(),
        "brains": brains,
    })))
}

#[derive(Deserialize)]
struct LlamaBody {
    model: String,
}

/// Use a local model: remember it, switch the engine to it and start it now
/// (so the first lesson does not wait for the load).
async fn set_llama(State(s): State<Shared>, Json(b): Json<LlamaBody>) -> R<Value> {
    {
        let mut st = s.settings.write().await;
        if !st.catalog.get(&b.model).is_some_and(|m| m.role == "brain") {
            return Err(bad(format!("模型目錄裡沒有「{}」", b.model)));
        }
        if let Some(dir) = st.app_dir.clone() {
            settings::write_config_value(&dir, Some("llama"), "model", toml::Value::String(b.model.clone())).map_err(fail)?;
            settings::write_config_value(&dir, Some("engine"), "kind", toml::Value::String("llama-server".into())).map_err(fail)?;
        }
        st.llama.model = b.model.clone();
        st.engine = settings::EngineKind::LlamaServer;
    }
    let t = std::time::Instant::now();
    crate::local_llama(&s).await?;
    Ok(Json(json!({ "model": b.model, "engine": "llama-server", "ms": t.elapsed().as_millis() as u64 })))
}

// ── Google (mock until OAuth) ───────────────────────────

async fn google_status(State(s): State<Shared>) -> R<Value> {
    let email = s.loaded.read().await.as_ref().and_then(|l| l.ws.profile.report_email.clone());
    Ok(Json(json!({ "mail": s.mailer.mode(), "calendar": s.calendar.mode(), "report_email": email })))
}

async fn google_outbox(State(s): State<Shared>) -> R<Value> {
    let mail = s.mock_mail.as_ref().map(|m| m.sent.lock().unwrap().clone()).unwrap_or_default();
    let events = s.mock_calendar.as_ref().map(|c| c.events.lock().unwrap().clone()).unwrap_or_default();
    Ok(Json(json!({ "mail": mail, "events": events })))
}

#[derive(Deserialize, Default)]
struct ReportBody {
    to: Option<String>,
}

async fn recipient(s: &Shared, to: Option<String>) -> Result<String, ApiError> {
    to.filter(|t| !t.trim().is_empty())
        .or(ws(s).await?.ws.profile.report_email.clone())
        .ok_or_else(|| bad("請先在「設定 → 個人」填寫週報信箱"))
}

async fn weekly_report(State(s): State<Shared>, body: Option<Json<ReportBody>>) -> R<Value> {
    let to = recipient(&s, body.and_then(|b| b.0.to)).await?;
    let (email, langs) = {
        let l = ws(&s).await?;
        let now = Utc::now();
        let mut per = vec![];
        for lang in l.ws.profile.languages.clone() {
            per.push((lang, l.db.stats(Some(lang), now - Duration::days(7), now).await?));
        }
        let week = report::iso_week(now.date_naive());
        (report::weekly_report(&to, Some(&l.ws.profile.name), &per, &week), per.len())
    };
    let id = s.mailer.send(&email).await.map_err(|e| bad(e.to_string()))?;
    Ok(Json(json!({ "id": id, "mode": s.mailer.mode(), "languages": langs, "email": email })))
}

#[derive(Deserialize)]
struct ScheduleBody {
    days: Option<i64>,
    /// Time of day (UTC) for review sessions, "HH:MM".
    at_utc: Option<String>,
    minutes: Option<i64>,
    lang: Option<String>,
}

async fn schedule_reviews(State(s): State<Shared>, Json(b): Json<ScheduleBody>) -> R<Value> {
    let at = NaiveTime::parse_from_str(b.at_utc.as_deref().unwrap_or("12:00"), "%H:%M").map_err(|_| bad("時間格式需為 HH:MM"))?;
    let events = {
        let l = ws(&s).await?;
        let now = Utc::now();
        let due = l.db.due_by_day(parse_lang(&b.lang)?, now, b.days.unwrap_or(7).clamp(1, 31)).await?;
        report::review_events(&due, now.date_naive(), at, b.minutes.unwrap_or(20))
    };
    let mut ids = vec![];
    for ev in &events {
        ids.push(s.calendar.create_event(ev).await.map_err(|e| bad(e.to_string()))?);
    }
    Ok(Json(json!({ "created": ids, "events": events, "mode": s.calendar.mode() })))
}

#[derive(Deserialize)]
struct BookBody {
    start: DateTime<Utc>,
    minutes: Option<i64>,
    topic: String,
    lang: Option<String>,
    to: Option<String>,
}

async fn book_lesson(State(s): State<Shared>, Json(b): Json<BookBody>) -> R<Value> {
    if b.topic.trim().is_empty() {
        return Err(bad("請輸入課程主題"));
    }
    let lang = parse_lang(&b.lang)?;
    let label = lang.map(|l| l.label()).unwrap_or("語言");
    let ev = CalendarEvent {
        summary: format!("{label}課：{}", b.topic.trim()),
        description: "JELearner 排的課。開始前打開 App，教材會在「教材」頁。".into(),
        start: b.start,
        end: b.start + Duration::minutes(b.minutes.unwrap_or(30).clamp(10, 240)),
        reminder_minutes: Some(15),
    };
    let event_id = s.calendar.create_event(&ev).await.map_err(|e| bad(e.to_string()))?;
    let tz = ws(&s).await?.ws.profile.timezone.clone();
    let mut out = json!({ "event_id": event_id, "event": ev, "mode": s.calendar.mode() });
    match recipient(&s, b.to).await {
        Ok(to) => {
            let email = report::lesson_confirmation(&to, &ev, &tz);
            out["email_id"] = json!(s.mailer.send(&email).await.map_err(|e| bad(e.to_string()))?);
        }
        Err(_) => out["email_skipped"] = json!("沒有設定週報信箱，未寄確認信"),
    }
    Ok(Json(out))
}

// ── misc ────────────────────────────────────────────────

/// Fixed pages only: the local API must not open arbitrary links.
async fn open_page(AxPath(page): AxPath<String>) -> R<Value> {
    let url = match page.as_str() {
        "anthropic-keys" => "https://console.anthropic.com/settings/keys",
        "github-new-repo" => "https://github.com/new?visibility=private",
        "claude-code-setup" => jl_core::claude_code::SETUP_PAGE,
        // System Settings → Accessibility → Spoken Content, where the free
        // Premium voices are downloaded (Manage Voices).
        "voices" if cfg!(target_os = "macos") => "x-apple.systempreferences:com.apple.Accessibility-Settings.extension?SpokenContent",
        _ => return Err(bad("unknown page")),
    };
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(url).spawn().map_err(|e| fail(format!("無法開啟瀏覽器：{e}")))?;
    Ok(Json(json!({ "opened": url })))
}
