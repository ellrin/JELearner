//! JSON API. Every handler is a plain async function over [`Shared`], so the
//! same code can later sit behind Tauri commands.

use crate::{bad, fail, git, load_workspace, provider, resolve_engine, secrets, settings, unavailable, ApiError, Loaded, Resolved, Shared};
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
use jl_core::workspace::{slug, Workspace};
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
        .route("/api/workspace", put(update_workspace))
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
        .route("/api/records/export", post(export_records))
        .route("/api/claude", get(claude_status))
        .route("/api/settings/claude", post(claude_settings))
        .route("/api/settings/engine", post(set_engine))
        .route("/api/claude-code/login", post(claude_code_login))
        .route("/api/engine/test", post(engine_test))
        .route("/api/models", get(models))
        .route("/api/google", get(google_status))
        .route("/api/google/outbox", get(google_outbox))
        .route("/api/google/weekly-report", post(weekly_report))
        .route("/api/google/schedule-reviews", post(schedule_reviews))
        .route("/api/google/book-lesson", post(book_lesson))
        .route("/api/open/{page}", post(open_page))
        .fallback_service(ServeDir::new(web))
}

// ── helpers ─────────────────────────────────────────────

/// The loaded workspace, or 503 with the reason it is not loaded.
async fn ws(s: &Shared) -> Result<RwLockReadGuard<'_, Loaded>, ApiError> {
    let g = s.loaded.read().await;
    if g.is_none() {
        let why = s.load_error.read().await.clone().unwrap_or_else(|| "尚未選擇工作區".into());
        return Err(unavailable(why));
    }
    Ok(RwLockReadGuard::map(g, |o| o.as_ref().unwrap()))
}

#[derive(Deserialize, Default)]
struct LangQuery {
    lang: Option<String>,
    q: Option<String>,
    limit: Option<i64>,
    all: Option<bool>,
    refresh: Option<bool>,
}

fn parse_lang(l: &Option<String>) -> Result<Option<Lang>, ApiError> {
    match l.as_deref().map(str::trim).filter(|s| !s.is_empty() && *s != "all") {
        None => Ok(None),
        Some(s) => s.parse().map(Some).map_err(bad),
    }
}

fn need_lang(l: &Option<String>) -> Result<Lang, ApiError> {
    parse_lang(l)?.ok_or_else(|| bad("請指定語言 lang=en 或 ja"))
}

fn today() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

/// `dir/name.ext`, or `dir/name-2.ext`… if taken. Never overwrites.
fn free_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
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
struct Status {
    workspace: Option<String>,
    demo: bool,
    error: Option<String>,
    db: Option<String>,
    owner: Option<String>,
    languages: Vec<Lang>,
    levels: Value,
    level_options: Value,
    report_email: Option<String>,
    engine: String,
    engine_ready: bool,
    /// 「Claude 訂閱」「Claude API」「本機模型」
    engine_label: String,
    engine_error: Option<String>,
    google: &'static str,
    import: Option<ImportReport>,
    stats: Value,
    prompts_overridden: Vec<String>,
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
    let level_options = json!(Lang::ALL.iter().map(|l| (l.code(), l.levels())).collect::<std::collections::BTreeMap<_, _>>());
    let error = s.load_error.read().await.clone();
    let guard = s.loaded.read().await;
    let Some(l) = guard.as_ref() else {
        return Ok(Json(Status {
            workspace: None,
            demo: false,
            error,
            db: None,
            owner: None,
            languages: Lang::ALL.to_vec(),
            levels: json!({}),
            level_options,
            report_email: None,
            engine,
            engine_ready,
            engine_label,
            engine_error,
            google: s.mailer.mode(),
            import: None,
            stats: json!({}),
            prompts_overridden: vec![],
        }));
    };
    let now = Utc::now();
    let mut stats = serde_json::Map::new();
    for lang in Lang::ALL {
        let st = l.db.stats(Some(lang), now - Duration::days(7), now).await?;
        stats.insert(lang.code().into(), serde_json::to_value(st).unwrap_or_default());
    }
    let levels = json!(Lang::ALL.iter().map(|&x| (x.code(), l.ws.meta.level(x))).collect::<std::collections::BTreeMap<_, _>>());
    Ok(Json(Status {
        workspace: Some(l.ws.root.display().to_string()),
        demo: l.demo,
        error,
        db: Some(l.db_path.display().to_string()),
        owner: l.ws.meta.owner.clone(),
        languages: l.ws.meta.languages.clone(),
        levels,
        level_options,
        report_email: l.ws.meta.report_email.clone(),
        engine,
        engine_ready,
        engine_label,
        engine_error,
        google: s.mailer.mode(),
        import: Some(l.import.clone()),
        stats: Value::Object(stats),
        prompts_overridden: Templates::load(Some(&l.ws.prompts_dir())).overridden,
    }))
}

#[derive(Deserialize)]
struct WorkspaceUpdate {
    owner: Option<String>,
    report_email: Option<String>,
    levels: Option<std::collections::BTreeMap<Lang, String>>,
}

async fn update_workspace(State(s): State<Shared>, Json(b): Json<WorkspaceUpdate>) -> R<Value> {
    let mut g = s.loaded.write().await;
    let l = g.as_mut().ok_or_else(|| unavailable("尚未選擇工作區"))?;
    if let Some(o) = b.owner {
        l.ws.meta.owner = Some(o.trim().to_string()).filter(|s| !s.is_empty());
    }
    if let Some(e) = b.report_email {
        let e = e.trim().to_string();
        if !e.is_empty() && (!e.contains('@') || e.contains(char::is_whitespace)) {
            return Err(bad("email 格式不正確"));
        }
        l.ws.meta.report_email = Some(e).filter(|s| !s.is_empty());
    }
    if let Some(levels) = b.levels {
        for (lang, lv) in levels {
            if !lang.levels().contains(&lv.as_str()) {
                return Err(bad(format!("{} 的程度只能是 {}", lang.label(), lang.levels().join("、"))));
            }
            l.ws.meta.levels.insert(lang, lv);
        }
    }
    l.ws.save_meta().map_err(|e| fail(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
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

/// Switch to a workspace folder and remember it in config.toml.
async fn use_workspace(s: &Shared, path: PathBuf, init: bool) -> Result<Value, ApiError> {
    let st = s.settings.read().await.clone();
    let loaded = load_workspace(&st, Some(path.clone()), init).await.map_err(bad)?;
    if let Some(dir) = &st.app_dir {
        settings::write_config_value(dir, None, "workspace_path", toml::Value::String(path.display().to_string())).map_err(fail)?;
    }
    s.settings.write().await.workspace = Some(path.clone());
    let report = loaded.import.clone();
    *s.loaded.write().await = Some(loaded);
    *s.load_error.write().await = None;
    Ok(json!({ "workspace": path.display().to_string(), "import": report }))
}

fn expand_home(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().map(|h| h.join(rest)).unwrap_or_else(|| PathBuf::from(p)),
        None => PathBuf::from(p),
    }
}

async fn setup_workspace(State(s): State<Shared>, Json(b): Json<WorkspaceBody>) -> R<Value> {
    if b.path.trim().is_empty() {
        return Err(bad("請輸入資料夾位置"));
    }
    Ok(Json(use_workspace(&s, expand_home(b.path.trim()), b.init).await?))
}

#[derive(Deserialize)]
struct NewBody {
    path: String,
    remote: Option<String>,
}

/// New workspace from the template; `git init`, and push to the user's own
/// empty private repo when a remote is given.
async fn setup_new(State(s): State<Shared>, Json(b): Json<NewBody>) -> R<Value> {
    let path = expand_home(b.path.trim());
    if path.exists() && std::fs::read_dir(&path).map(|mut d| d.next().is_some()).unwrap_or(false) {
        return Err(bad("資料夾不是空的；要使用現有資料夾請選「開啟資料夾」"));
    }
    let template = s.settings.read().await.paths.template.clone();
    Workspace::init(&path, &template).map_err(|e| bad(e.to_string()))?;
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
    parent: String,
}

async fn setup_clone(State(s): State<Shared>, Json(b): Json<CloneBody>) -> R<Value> {
    let url = b.url.trim();
    let name = url.trim_end_matches('/').trim_end_matches(".git").rsplit(['/', ':']).next().unwrap_or("").to_string();
    if name.is_empty() {
        return Err(bad("repo 網址無效"));
    }
    let target = expand_home(b.parent.trim()).join(&name);
    git::clone(url, &target).await.map_err(bad)?;
    Ok(Json(use_workspace(&s, target, true).await?))
}

async fn git_status(State(s): State<Shared>) -> R<git::GitStatus> {
    let root = ws(&s).await?.ws.root.clone();
    Ok(Json(git::status(&root).await))
}

async fn git_pull(State(s): State<Shared>) -> R<Value> {
    let root = ws(&s).await?.ws.root.clone();
    let pulled = git::pull(&root).await.map_err(bad)?;
    if pulled {
        let l = ws(&s).await?;
        let rep = import::sync_workspace(&l.ws, &l.db).await;
        return Ok(Json(json!({ "pulled": true, "import": rep })));
    }
    Ok(Json(json!({ "pulled": false })))
}

#[derive(Deserialize, Default)]
struct SyncBody {
    message: Option<String>,
}

async fn git_sync(State(s): State<Shared>, body: Option<Json<SyncBody>>) -> R<git::SyncResult> {
    let root = ws(&s).await?.ws.root.clone();
    let msg = body.and_then(|b| b.0.message);
    Ok(Json(git::sync(&root, msg).await.map_err(bad)?))
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
async fn news_list(State(s): State<Shared>, Query(q): Query<LangQuery>) -> R<Value> {
    let lang = parse_lang(&q.lang)?;
    let mut errors = vec![];
    let mut fetched = 0;
    if q.refresh.unwrap_or(false) {
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
    }
    let l = ws(&s).await?;
    let list = l.db.list_articles(lang, q.limit.unwrap_or(60)).await?;
    Ok(Json(json!({ "articles": list, "fetched": fetched, "errors": errors })))
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
async fn fetch_article_into_db(s: &Shared, url: &str, lang: Lang, source: &str) -> Result<i64, ApiError> {
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
        let level = b.level.clone().filter(|v| lang.levels().contains(&v.as_str())).unwrap_or_else(|| l.ws.meta.level(lang));
        let known: Vec<String> = l.db.list_words(Some(lang), "", 400).await?.into_iter().map(|w| w.term).collect();
        let url = if a.url.starts_with("manual://") || a.url.starts_with("material://") { String::new() } else { a.url.clone() };
        (
            LessonInput { lang, level, native_language: l.ws.meta.native_language.clone(), title: a.title, url, source: a.source, text: a.body, known_words: known },
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
}

async fn review_grade(State(s): State<Shared>, Json(g): Json<Grade>) -> R<Value> {
    if !(1..=5).contains(&g.grade) {
        return Err(bad("grade 需為 1–5"));
    }
    let l = ws(&s).await?;
    let card = l.db.record_review(g.kind, g.id, g.grade, Utc::now()).await?;
    Ok(Json(json!(card)))
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
    let fallback = l.ws.meta.languages.first().copied().unwrap_or(Lang::En);
    let raw_name = q.name.clone().unwrap_or_default();
    let stem_src = Path::new(&raw_name).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    let lang = parse_lang(&q.lang)?;
    let (path, report) = match q.kind.as_str() {
        "vocab" | "grammar" => {
            let lang = lang.unwrap_or_else(|| import::lang_from_path(Path::new(&raw_name), fallback));
            let ext = if raw_name.to_lowercase().ends_with(".tsv") { "tsv" } else { "csv" };
            let dir = if q.kind == "vocab" { l.ws.vocab_dir() } else { l.ws.grammar_dir() };
            let stem = format!("{}-{}", slug(if stem_src.is_empty() { "imported" } else { &stem_src }, 40), lang.code());
            let stem = if stem_src.ends_with(&format!("-{}", lang.code())) { slug(&stem_src, 44) } else { stem };
            std::fs::create_dir_all(&dir).map_err(|e| fail(e.to_string()))?;
            let path = free_path(&dir, &stem, ext);
            let origin = l.ws.relative(&path);
            // Validate before writing, so a wrong file leaves no trace.
            let check = jl_core::db::Db::memory().await?;
            let r = if q.kind == "vocab" {
                import::import_vocab_csv(&check, &body, lang, &origin).await
            } else {
                import::import_grammar_csv(&check, &body, lang, &origin).await
            };
            r.map_err(|e| bad(e.to_string()))?;
            std::fs::write(&path, &body).map_err(|e| fail(e.to_string()))?;
            let r = if q.kind == "vocab" {
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
        other => return Err(bad(format!("不支援的匯入類型：{other}（vocab、grammar、materials）"))),
    };
    Ok(Json(json!({ "file": l.ws.relative(&path), "report": report })))
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
    let (k, model, effort, models, engine, bin) = {
        let st = s.settings.read().await;
        (
            secrets::key(st.claude_api_key.as_deref(), st.use_keychain, st.app_dir.as_deref()),
            st.claude.model.clone(),
            st.claude.effort.clone(),
            st.catalog.cloud.models.clone(),
            st.engine,
            st.claude_bin.clone(),
        )
    };
    let sub = s.cc.get(bin.as_deref(), q.fresh.unwrap_or(false)).await;
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
        "models": models,
    })))
}

#[derive(Deserialize)]
struct EngineBody {
    kind: String,
}

/// Choose the engine: auto, subscription, api or llama-server.
async fn set_engine(State(s): State<Shared>, Json(b): Json<EngineBody>) -> R<Value> {
    let kind = settings::EngineKind::parse(&b.kind).ok_or_else(|| bad("engine 只能是 auto、subscription、api、llama-server"))?;
    let mut st = s.settings.write().await;
    if let Some(dir) = &st.app_dir {
        settings::write_config_value(dir, Some("engine"), "kind", toml::Value::String(kind.as_str().into())).map_err(fail)?;
    }
    st.engine = kind;
    Ok(Json(json!({ "engine": kind.as_str() })))
}

/// `claude auth login --claudeai`: the browser opens on Anthropic's sign-in
/// page; the UI polls `/api/claude?fresh=true` until it lands.
async fn claude_code_login(State(s): State<Shared>) -> R<Value> {
    let bin = s.settings.read().await.claude_bin.clone();
    jl_core::claude_code::start_login(bin.as_deref()).map_err(bad)?;
    Ok(Json(json!({ "started": true })))
}

#[derive(Deserialize)]
struct ClaudeSettingsBody {
    key: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}

async fn claude_settings(State(s): State<Shared>, Json(b): Json<ClaudeSettingsBody>) -> R<Value> {
    let mut st = s.settings.write().await;
    let mut saved = None;
    if let Some(k) = &b.key {
        saved = Some(secrets::save_key(k, st.use_keychain, st.app_dir.as_deref()).map_err(fail)?);
    }
    if let Some(m) = b.model.filter(|m| !m.trim().is_empty()) {
        let m = m.trim().to_string();
        if let Some(dir) = &st.app_dir {
            settings::write_config_value(dir, Some("claude"), "model", toml::Value::String(m.clone())).map_err(fail)?;
        }
        st.claude.model = m;
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
            let st = s.settings.read().await;
            match w {
                "subscription" => (
                    std::sync::Arc::new(jl_core::claude_code::ClaudeCodeProvider::new(st.claude_bin.clone(), st.claude.model.clone(), st.claude.effort.clone())),
                    Resolved::Subscription.label(),
                ),
                "api" => {
                    let (key, _) = secrets::key(st.claude_api_key.as_deref(), st.use_keychain, st.app_dir.as_deref()).ok_or_else(|| unavailable("尚未設定 API key"))?;
                    (std::sync::Arc::new(jl_core::provider::ClaudeProvider::new(st.claude.clone(), key)), Resolved::Api.label())
                }
                _ => return Err(bad("engine 只能是 subscription 或 api")),
            }
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
    Ok(Json(json!({
        "catalog": c,
        "memory_gb": (memory_gb * 10.0).round() / 10.0,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "plan_here": catalog::plan(c, &defaults, memory_gb),
        "plan_mac32": catalog::plan(c, &defaults, 32.0),
        "note": "本機模型等 Mac 到貨後啟用；目前只顯示預估，尚未實機測試。",
    })))
}

// ── Google (mock until OAuth) ───────────────────────────

async fn google_status(State(s): State<Shared>) -> R<Value> {
    let email = s.loaded.read().await.as_ref().and_then(|l| l.ws.meta.report_email.clone());
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
        .or(ws(s).await?.ws.meta.report_email.clone())
        .ok_or_else(|| bad("請先在「設定 → 個人」填寫週報信箱"))
}

async fn weekly_report(State(s): State<Shared>, body: Option<Json<ReportBody>>) -> R<Value> {
    let to = recipient(&s, body.and_then(|b| b.0.to)).await?;
    let (email, langs) = {
        let l = ws(&s).await?;
        let now = Utc::now();
        let mut per = vec![];
        for lang in l.ws.meta.languages.clone() {
            per.push((lang, l.db.stats(Some(lang), now - Duration::days(7), now).await?));
        }
        let week = report::iso_week(now.date_naive());
        (report::weekly_report(&to, l.ws.meta.owner.as_deref(), &per, &week), per.len())
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
    let tz = ws(&s).await?.ws.meta.timezone.clone();
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
