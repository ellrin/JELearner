//! Lessons prepared for a family member's iPad (docs/plan.md stage F5, step 2).
//!
//! The admin's computer makes a few days of daily lessons for a member with
//! the admin's own engine (the lessons model), from the member's level,
//! interests and focus theme, with the news this computer already has. They
//! are written into the member's folder like any lesson, and for each lesson
//! a package the iPad reads as it is:
//!
//! - `profiles/<id>/ipad/<date>/<course>-<nn>.json`: the plan and its units,
//!   the whole lesson data, the review cards due when it was prepared, and
//!   for the conversation unit everything needed to hold it without the app
//!   (prompts rendered here, the JSON schemas, the opening line).
//! - `profiles/<id>/ipad/index.json`: the lessons to choose from, per day.
//!
//! The next sync takes them to the shared repository.
//!
//! - `POST /api/family/prepare {member | members, lang, days, choices}` starts
//!   the job in the background; `GET /api/family/prepare` shows its progress.
//!   Several members are prepared one after another, each at their own level;
//!   with the topic library they get the same lessons as long as they are
//!   prepared together. The admin can be one of them (their own iPad).

use crate::api::need_lang;
use crate::courses::{course_err, daily_course, find_article, index_lesson, learner, recent_accuracy, review_cards, templates, unit_json};
use crate::{bad, ApiError, Loaded, Shared};
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use jl_core::conversation::{self, CourseLink, Templates as ConvTemplates};
use jl_core::course::plan::{self, CourseLesson, LessonContext};
use jl_core::course::{picker, store, UnitKind};
use jl_core::{lesson, Lang};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Mutex;

type R<T> = Result<Json<T>, ApiError>;

pub fn routes() -> Router<Shared> {
    Router::new().route("/api/family/prepare", get(status).post(start))
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Progress {
    pub running: bool,
    /// The member being prepared now.
    pub member: String,
    /// Everyone this run prepares for.
    pub members: Vec<String>,
    /// Lessons planned for the members still to come.
    #[serde(skip)]
    pub rest: usize,
    pub lang: String,
    pub total: usize,
    pub done: usize,
    /// What is being made now (a topic title).
    pub current: String,
    pub made: Vec<String>,
    pub errors: Vec<String>,
}

/// Progress per server (an app has one; tests run several side by side).
static PROGRESS: Mutex<Option<std::collections::HashMap<usize, Progress>>> = Mutex::new(None);

fn key(s: &Shared) -> usize {
    std::sync::Arc::as_ptr(s) as *const () as usize
}

fn current(s: &Shared) -> Option<Progress> {
    PROGRESS.lock().expect("lock").as_ref().and_then(|m| m.get(&key(s)).cloned())
}

fn set(s: &Shared, p: Progress) {
    PROGRESS.lock().expect("lock").get_or_insert_with(Default::default).insert(key(s), p);
}

fn update(s: &Shared, f: impl FnOnce(&mut Progress)) {
    if let Some(p) = PROGRESS.lock().expect("lock").as_mut().and_then(|m| m.get_mut(&key(s))) {
        f(p);
    }
}

async fn status(State(s): State<Shared>) -> R<Value> {
    Ok(Json(json!({ "progress": current(&s) })))
}

#[derive(Deserialize)]
struct StartBody {
    #[serde(default)]
    member: String,
    /// Several members at once (instead of `member`).
    #[serde(default)]
    members: Vec<String>,
    lang: Option<String>,
    /// `library` (default when the language has a topic library), `news`, or
    /// `mix`: each day one topic from the news and the rest from the library.
    source: Option<String>,
    #[serde(default = "three")]
    days: u32,
    #[serde(default = "three")]
    choices: u32,
}
fn three() -> u32 {
    3
}

async fn start(State(s): State<Shared>, Json(b): Json<StartBody>) -> R<Value> {
    let lang = need_lang(&b.lang)?;
    if !(1..=7).contains(&b.days) || !(1..=4).contains(&b.choices) {
        return Err(bad("一次備 1 到 7 天、每天 1 到 4 堂"));
    }
    let mut members: Vec<String> = if b.members.is_empty() { vec![b.member.clone()] } else { b.members.clone() };
    members.retain(|m| !m.trim().is_empty());
    members.dedup();
    if members.is_empty() {
        return Err(bad("要替誰備課？"));
    }
    // Only an admin prepares lessons (for the others, or for their own iPad).
    {
        let l = crate::api::ws(&s).await?;
        let root = s.root.read().await;
        let open = root.as_ref().ok_or_else(|| bad("尚未選擇工作區"))?;
        if !open.root.is_admin(&l.ws.profile.id) {
            return Err(bad("只有管理員可以替家人備課"));
        }
        for m in &members {
            open.root.user(m).map_err(|_| bad(format!("找不到使用者 {m}")))?;
        }
    }
    if current(&s).is_some_and(|p| p.running) {
        return Err(bad("已經在備課了，等這一批做完"));
    }
    let per = (b.days * b.choices) as usize;
    set(&s, Progress { running: true, member: members[0].clone(), members: members.clone(), lang: lang.code().into(), total: per * members.len(), ..Default::default() });
    let s2 = s.clone();
    tokio::spawn(async move {
        for (i, m) in members.iter().enumerate() {
            // The members still to come count towards the total.
            update(&s2, |p| {
                p.member = m.clone();
                p.rest = per * (members.len() - i - 1);
            });
            if let Err(e) = run(&s2, m, lang, b.days, b.choices, b.source.as_deref()).await {
                update(&s2, |p| p.errors.push(if members.len() > 1 { format!("{m}：{}", e.1) } else { e.1 }));
            }
        }
        // The active user's own files may have changed (their own iPad lessons).
        crate::api::reload_active(&s2).await;
        // The iPad also shows everyone's points.
        crate::buddies::write_ipad_files(&s2).await;
        update(&s2, |p| {
            p.running = false;
            p.current.clear();
        });
    });
    Ok(Json(json!({ "started": true })))
}

/// The member as loaded on this computer: their own database, their files read in.
pub(crate) async fn member_loaded(s: &Shared, member: &str) -> Result<Loaded, ApiError> {
    let settings = s.settings.read().await.clone();
    let root = s.root.read().await;
    let open = root.as_ref().ok_or_else(|| bad("尚未選擇工作區"))?;
    crate::enter_user(&settings, open, member).await.map_err(bad)
}

fn day(offset: u32) -> String {
    (chrono::Local::now().date_naive() + chrono::Days::new(offset as u64)).format("%Y-%m-%d").to_string()
}

async fn run(s: &Shared, member: &str, lang: Lang, days: u32, choices: u32, source: Option<&str>) -> Result<(), ApiError> {
    // Results from the iPad first: the new lessons follow her latest level.
    let _ = crate::inbox::process_member(s, member).await;
    let m = member_loaded(s, member).await?;
    if !m.ws.profile.languages.contains(&lang) {
        return Err(bad(format!("{member} 沒有在學 {}", lang.label())));
    }
    // Topics already prepared are not offered again.
    let mut index = load_index(&m);
    // The iPad answers in conversations with the real-time model (decisions §19).
    index["realtime_model"] = json!(s.settings.read().await.realtime.model.clone());
    index["realtime_effort"] = json!(s.settings.read().await.realtime.effort.clone());
    // The topic library first, when the language has one (and it was not turned off).
    let mut choices = choices;
    if source != Some("news")
        && let Some(coll) = crate::library::collection_for(&m.ws.root, lang)
    {
        if source != Some("mix") || choices < 2 {
            return run_library(s, &m, lang, days, choices, &coll, index).await;
        }
        // Mixed: the library fills all but one topic a day; a used-up library
        // is not an error here, the news takes the whole day.
        update(s, |p| p.rest += days as usize); // the news topics still to come
        let lib = run_library(s, &m, lang, days, choices - 1, &coll, index).await;
        update(s, |p| p.rest = p.rest.saturating_sub(days as usize));
        if lib.is_ok() {
            choices = 1;
        }
        index = load_index(&m);
    }
    let mut shown: Vec<String> = index["days"].as_object().map(|d| d.values().flat_map(|v| v.as_array().cloned().unwrap_or_default()).filter_map(|x| x["title"].as_str().map(str::to_string)).collect()).unwrap_or_default();
    let (parts, focus, t) = crate::today::card_inputs(&m, lang);
    let heads = crate::today::headlines(s, lang, focus.as_ref()).await?;
    let want = (days * choices) as usize;
    let mut cards = crate::today::ask_cards(s, lang, parts.clone(), focus.as_ref(), &t, &heads, &shown, want, want).await?;
    if cards.len() < want {
        shown.extend(cards.iter().map(|c| c.title.clone()));
        let more = crate::today::ask_cards(s, lang, parts, focus.as_ref(), &t, &heads, &shown, want - cards.len(), want - cards.len()).await.unwrap_or_default();
        cards.extend(more);
    }
    let n = cards.len().min(want);
    update(s, |p| p.total = p.done + n + p.rest);
    let provider = crate::provider(s).await?;
    for (i, card) in cards.into_iter().take(want).enumerate() {
        let date = day(i as u32 / choices);
        update(s, |p| p.current = card.title.clone());
        match prepare_one(s, &m, provider.as_ref(), lang, &card, &date).await {
            Ok(entries) => {
                let list = index["days"][&date].as_array().cloned().unwrap_or_default();
                let mut list = list;
                list.extend(entries);
                index["days"][&date] = Value::Array(list);
                save_index(&m, &index).map_err(|e| bad(e.to_string()))?;
                update(s, |p| {
                    p.done += 1;
                    p.made.push(card.title.clone());
                });
            }
            Err(e) => update(s, |p| p.errors.push(format!("{}：{}", card.title, e.1))),
        }
    }
    Ok(())
}

/// One topic: the daily course on `date`, its lessons written, a package each.
async fn prepare_one(s: &Shared, m: &Loaded, provider: &dyn jl_core::provider::LlmProvider, lang: Lang, card: &picker::Card, date: &str) -> Result<Vec<Value>, ApiError> {
    // A news card brings its article (from this computer's news).
    let mut article = None;
    if let Some(id) = card.article_id {
        let a = crate::api::ws(s).await?.db.get_article(id).await?;
        if let Some(a) = a.filter(|a| !a.body.trim().is_empty()) {
            article = Some(jl_core::course::SourceText { title: a.title, url: a.url, source: a.source, text: a.body.chars().take(jl_core::lesson::MAX_ARTICLE_CHARS).collect() });
        }
    }
    let focus_name = picker::load_focus(&m.ws, lang).map(|f| f.name).unwrap_or_default();
    let mut course = daily_course(m, lang, card.title.trim(), card.keywords.clone(), article, focus_name, date).await?;
    store::save_new(&m.ws, &mut course).map_err(course_err)?;
    let card = json!({ "title": card.title, "why": card.why, "kind": card.kind, "category": card.category });
    package_course(s, m, provider, lang, &course, &card, date).await
}

/// Write a saved course's lessons and a package each for the iPad.
async fn package_course(s: &Shared, m: &Loaded, provider: &dyn jl_core::provider::LlmProvider, lang: Lang, course: &jl_core::course::Course, card: &Value, date: &str) -> Result<Vec<Value>, ApiError> {
    let t = templates(&m.ws);
    let lr = learner(m, lang, false).await?;
    let focus = picker::load_focus(&m.ws, lang);
    let mut out = vec![];
    for n in course.lessons.iter().map(|l| l.number).collect::<Vec<_>>() {
        let lp = course.lesson(n).cloned().ok_or_else(|| bad("課程消失了"))?;
        let mut ctx = LessonContext { recent_accuracy: recent_accuracy(m, lang).await?, article: None, focus: focus.clone() };
        if plan::needs_search(&lp) {
            ctx.article = find_article(s, lang, &lp).await;
        }
        let opened = plan::open_lesson(provider, &t, &m.ws, &course.id, n, &lr, &ctx, date).await.map_err(course_err)?;
        let engine = lesson::front_matter(&opened.markdown, "engine").unwrap_or_else(|| provider.id());
        let lesson_id = index_lesson(m, &opened.course, n, &opened.file, &opened.markdown, &engine).await?;
        let data = lesson::extract_raw(&opened.markdown).unwrap_or(Value::Null);
        let lp = opened.course.lesson(n).cloned().unwrap_or(lp);
        let cl: Option<CourseLesson> = serde_json::from_value(data.clone()).ok();
        let review_items = lp.units.iter().find(|u| u.kind == UnitKind::Review).map(|u| u.items).unwrap_or(0);
        let cards = if review_items > 0 { review_cards(m, lang, &lp, cl.as_ref(), review_items).await? } else { vec![] };
        let mut conv = Value::Null;
        if let Some(ui) = lp.units.iter().position(|u| u.kind == UnitKind::Conversation) {
            let link = CourseLink { course: course.id.clone(), lesson: n, unit: ui as u32 };
            let (topic, targets, turns, opening) = crate::study::course_start(m, lang, &link).await?;
            let level = m.ws.profile.difficulty(lang).label;
            let c = conversation::Conversation {
                lang,
                topic,
                level: level.clone(),
                targets,
                turns_limit: turns,
                turns: vec![],
                engine: String::new(),
                started_at: chrono::Utc::now(),
                ended_at: None,
                review: None,
                score: None,
                passed: None,
                points: None,
                course: Some(link),
            };
            let ct = ConvTemplates::load(Some(&m.ws.prompts_dir()));
            let mut pkg = conversation::offline_package(&ct, &c, &crate::study::conv_learner(&m.ws, lang, &level)).map_err(|e| bad(e.to_string()))?;
            pkg["opening"] = json!(opening);
            pkg["unit"] = json!(ui);
            conv = pkg;
        }
        let rel = format!("ipad/{date}/{}-{n:02}.json", course.id);
        let package = json!({
            "version": 1,
            "date": date,
            "lang": lang.code(),
            "member": m.ws.profile.id,
            "course": course.id,
            "lesson": n,
            "lesson_id": lesson_id,
            "card": card,
            // A library lesson's picture (in the member's folder while the lesson is open).
            "picture": crate::library::picture_for_package(&m.ws, lang, &course.id),
            "plan": { "title": lp.title, "purpose": lp.purpose, "minutes": lp.minutes, "units": unit_json(&lp), "words": lp.words },
            "data": data,
            "review_cards": cards,
            "conversation": conv,
            "file": opened.file,
        });
        let path = m.ws.profile_dir().join(&rel);
        std::fs::create_dir_all(path.parent().expect("has a parent")).map_err(|e| bad(e.to_string()))?;
        std::fs::write(&path, serde_json::to_string_pretty(&package).unwrap_or_default()).map_err(|e| bad(e.to_string()))?;
        out.push(json!({ "title": card["title"], "why": card["why"], "kind": card["kind"], "category": card["category"], "course": course.id, "lesson": n, "file": rel, "minutes": lp.minutes }));
    }
    Ok(out)
}

/// Library lessons for the iPad: texts not used for this member yet, each
/// day's choices from different categories.
async fn run_library(s: &Shared, m: &Loaded, lang: Lang, days: u32, choices: u32, coll: &str, mut index: Value) -> Result<(), ApiError> {
    let used_path = m.ws.profile_dir().join("ipad").join("library-used.txt");
    let used: Vec<String> = std::fs::read_to_string(&used_path).unwrap_or_default().lines().map(str::to_string).collect();
    let avail: Vec<(jl_core::course::library::Topic, jl_core::course::library::LessonFocus)> =
        crate::library::written_lessons(&m.ws.root, coll).into_iter().filter(|(_, l)| !used.contains(&l.id)).collect();
    let want = (days * choices) as usize;
    // Round-robin over categories, in syllabus order.
    let mut cats: Vec<String> = vec![];
    for (t, _) in &avail {
        if !cats.contains(&t.cat) {
            cats.push(t.cat.clone());
        }
    }
    let mut queues: Vec<std::collections::VecDeque<(jl_core::course::library::Topic, jl_core::course::library::LessonFocus)>> =
        cats.iter().map(|c| avail.iter().filter(|(t, _)| &t.cat == c).cloned().collect()).collect();
    let mut picked = vec![];
    while picked.len() < want && queues.iter().any(|q| !q.is_empty()) {
        for q in queues.iter_mut() {
            if picked.len() == want {
                break;
            }
            if let Some(x) = q.pop_front() {
                picked.push(x);
            }
        }
    }
    if picked.is_empty() {
        return Err(bad("主題庫裡沒有還沒用過的課了：先在主題庫「寫更多課」"));
    }
    let n = picked.len();
    update(s, |p| p.total = p.done + n + p.rest);
    let provider = crate::provider(s).await?;
    for (i, (t, l)) in picked.into_iter().enumerate() {
        let date = day(i as u32 / choices);
        update(s, |p| p.current = l.zh.clone());
        let res: Result<Vec<Value>, ApiError> = async {
            let made = crate::library::course_from_text(s, m, coll, &l.id, &date).await?;
            let card = json!({ "title": l.zh, "why": l.goal, "kind": "library", "category": t.cat });
            package_course(s, m, provider.as_ref(), lang, &made, &card, &date).await
        }
        .await;
        match res {
            Ok(entries) => {
                let mut list = index["days"][&date].as_array().cloned().unwrap_or_default();
                list.extend(entries);
                index["days"][&date] = Value::Array(list);
                save_index(m, &index).map_err(|e| bad(e.to_string()))?;
                let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&used_path).map_err(|e| bad(e.to_string()))?;
                use std::io::Write;
                writeln!(f, "{}", l.id).map_err(|e| bad(e.to_string()))?;
                update(s, |p| {
                    p.done += 1;
                    p.made.push(l.zh.clone());
                });
            }
            Err(e) => update(s, |p| p.errors.push(format!("{}：{}", l.zh, e.1))),
        }
    }
    Ok(())
}

fn index_path(m: &Loaded) -> std::path::PathBuf {
    m.ws.profile_dir().join("ipad").join("index.json")
}

fn load_index(m: &Loaded) -> Value {
    std::fs::read_to_string(index_path(m)).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(|v: &Value| v["days"].is_object()).unwrap_or_else(|| json!({ "version": 1, "days": {} }))
}

/// Past days drop out of the index (their packages stay until the iPad reports them).
fn save_index(m: &Loaded, index: &Value) -> std::io::Result<()> {
    let today = day(0);
    let mut v = index.clone();
    if let Some(d) = v["days"].as_object_mut() {
        d.retain(|k, _| k.as_str() >= today.as_str());
    }
    v["updated"] = json!(chrono::Utc::now().to_rfc3339());
    let p = index_path(m);
    std::fs::create_dir_all(p.parent().expect("has a parent"))?;
    std::fs::write(p, serde_json::to_string_pretty(&v).unwrap_or_default())
}
