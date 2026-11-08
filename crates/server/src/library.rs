//! The topic library (docs/decisions.md §22): one thing per lesson, the point
//! is learning the language.
//!
//! - `GET /api/library` — collections, their themes, the planned lessons and
//!   which texts exist.
//! - `POST /api/library/build {collection, ids?, limit?}` — in the background:
//!   read a theme's references (robots.txt checked), split it into lessons if
//!   not done yet (`plans/<theme>.toml`), write the missing lesson texts
//!   (at most `limit` texts); `GET /api/library/build` shows progress. Texts
//!   already written are never rewritten.
//! - `POST /api/library/lesson {collection, id}` — a one-lesson course from a
//!   lesson text for the active learner; written at their level when opened.

use crate::api::ws;
use crate::courses::{course_err, daily_course, date, templates};
use crate::{bad, ApiError, Shared};
use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use jl_core::course::library::{self, Article, LessonFocus, SourceDoc, Syllabus, Topic};
use jl_core::course::{store, CourseKind, SourceText, TargetWord};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Mutex;

type R<T> = Result<Json<T>, ApiError>;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/library", get(list))
        .route("/api/library/build", get(status).post(start))
        .route("/api/library/lesson", post(lesson))
}

/// The reference text sent per source (the start of a page is the overview).
const REF_CHARS: usize = 9_000;

#[derive(Deserialize)]
struct ListQ {
    collection: Option<String>,
}

fn read_text(root: &Path, collection: &str, t: &Topic, l: &LessonFocus) -> Option<Article> {
    std::fs::read_to_string(library::article_path(root, collection, t, &l.id)).ok().and_then(|md| library::read_article(&md))
}

async fn list(State(s): State<Shared>, Query(q): Query<ListQ>) -> R<Value> {
    let l = ws(&s).await?;
    let root = l.ws.root.clone();
    let mut out = vec![];
    for c in library::collections(&root).into_iter().filter(|c| q.collection.as_deref().is_none_or(|x| x == c)) {
        let syl = library::load_syllabus(&root, &c).map_err(course_err)?;
        let mut written = 0;
        let mut planned = 0;
        let themes: Vec<Value> = syl
            .topic
            .iter()
            .map(|t| {
                let plan = library::load_plan(&root, &c, t);
                let lessons: Vec<Value> = plan
                    .iter()
                    .flat_map(|p| p.lesson.iter())
                    .map(|ls| {
                        let a = read_text(&root, &c, t, ls);
                        planned += 1;
                        if a.is_some() {
                            written += 1;
                        }
                        json!({ "id": ls.id, "zh": ls.zh, "en": ls.en, "goal": ls.goal, "written": a.is_some(), "title": a.as_ref().map(|a| a.title.clone()), "words": a.as_ref().map(|a| a.words) })
                    })
                    .collect();
                json!({ "id": t.id, "cat": t.cat, "zh": t.zh, "en": t.en, "split": plan.is_some(), "lessons": lessons })
            })
            .collect();
        out.push(json!({ "id": c, "name": syl.name, "lang": syl.lang.code(), "level": syl.level, "themes": themes.len(), "planned": planned, "written": written, "topics": themes }));
    }
    Ok(Json(json!({ "collections": out })))
}

// ── planning and writing in the background ──────────────

#[derive(Debug, Clone, Default, Serialize)]
pub struct Progress {
    pub running: bool,
    pub collection: String,
    /// Texts to write this time.
    pub total: usize,
    pub done: usize,
    pub current: String,
    pub made: Vec<String>,
    pub errors: Vec<String>,
}

static PROGRESS: Mutex<Option<Progress>> = Mutex::new(None);

fn update(f: impl FnOnce(&mut Progress)) {
    if let Some(p) = PROGRESS.lock().expect("lock").as_mut() {
        f(p);
    }
}

async fn status() -> R<Value> {
    Ok(Json(json!({ "progress": PROGRESS.lock().expect("lock").clone() })))
}

#[derive(Deserialize)]
struct BuildBody {
    collection: String,
    /// Themes (or lesson ids' themes) to work on; empty = in syllabus order.
    #[serde(default)]
    ids: Vec<String>,
    /// At most this many new texts this time (default 10).
    limit: Option<usize>,
}

/// A theme with work left: not split yet, or a planned lesson without its text.
fn has_work(root: &Path, collection: &str, t: &Topic) -> bool {
    match library::load_plan(root, collection, t) {
        None => true,
        Some(p) => p.lesson.iter().any(|l| !library::article_path(root, collection, t, &l.id).exists()),
    }
}

async fn start(State(s): State<Shared>, Json(b): Json<BuildBody>) -> R<Value> {
    let (root, native) = {
        let l = ws(&s).await?;
        let root_r = s.root.read().await;
        let open = root_r.as_ref().ok_or_else(|| bad("尚未選擇工作區"))?;
        if !open.root.is_admin(&l.ws.profile.id) {
            return Err(bad("只有管理員可以產生主題庫"));
        }
        (l.ws.root.clone(), l.ws.profile.native_language.clone())
    };
    let syl = library::load_syllabus(&root, &b.collection).map_err(course_err)?;
    let wanted = |t: &Topic| b.ids.is_empty() || b.ids.iter().any(|i| *i == t.id || i.rsplit_once('-').is_some_and(|(th, _)| th == t.id));
    let themes: Vec<Topic> = syl.topic.iter().filter(|t| wanted(t) && has_work(&root, &b.collection, t)).cloned().collect();
    if themes.is_empty() {
        return Err(bad("這些主題的課都已經寫好了"));
    }
    let limit = b.limit.unwrap_or(10).clamp(1, 500);
    {
        let mut p = PROGRESS.lock().expect("lock");
        if p.as_ref().is_some_and(|p| p.running) {
            return Err(bad("已經在產生了，等這一批做完"));
        }
        *p = Some(Progress { running: true, collection: b.collection.clone(), total: limit, ..Default::default() });
    }
    let s2 = s.clone();
    tokio::spawn(async move {
        let mut written = 0;
        for t in themes {
            if written >= limit {
                break;
            }
            match work_on_theme(&s2, &root, &b.collection, &syl, &t, &native, limit - written).await {
                Ok(n) => written += n,
                Err(e) => update(|p| p.errors.push(format!("{}：{}", t.zh, e.1))),
            }
        }
        update(|p| {
            p.running = false;
            p.current.clear();
            p.total = p.done;
        });
    });
    Ok(Json(json!({ "started": true, "limit": limit })))
}

/// Read a theme's references once; split it if needed; write up to `room` texts.
async fn work_on_theme(s: &Shared, root: &Path, collection: &str, syl: &Syllabus, t: &Topic, native: &str, room: usize) -> Result<usize, ApiError> {
    update(|p| p.current = format!("讀參考資料：{}", t.zh));
    let mut docs: Vec<(String, SourceDoc)> = vec![];
    let mut problems = vec![];
    for r in &t.refs {
        let Some(page) = library::ref_page(r) else { continue };
        match jl_core::news::fetch_article(&s.fetcher, &page.url, page.selector).await {
            Ok(a) if !a.paragraphs.is_empty() => {
                let text = library::clean_reference_text(&a.paragraphs, REF_CHARS);
                let title = a.title.trim_end_matches(" - Wikipedia").trim_end_matches(" – Travel guide at Wikivoyage").to_string();
                docs.push((r.clone(), SourceDoc { title, url: page.url.clone(), licence: page.licence.into(), text }));
            }
            Ok(_) => problems.push(format!("{r}：沒有內文")),
            Err(e) => problems.push(format!("{r}：{e}")),
        }
    }
    if docs.is_empty() {
        return Err(bad(format!("一個參考資料都讀不到（{}）", problems.join("；"))));
    }
    let tpl = { templates(&ws(s).await?.ws) };
    let provider = crate::provider(s).await?;
    // 1. Split, once per theme.
    let plan = match library::load_plan(root, collection, t) {
        Some(p) => p,
        None => {
            update(|p| p.current = format!("拆課：{}", t.zh));
            let all: Vec<SourceDoc> = docs.iter().map(|(_, d)| d.clone()).collect();
            let req = library::split_request(&tpl, syl, t, native, &all).map_err(course_err)?;
            let reply = provider.complete(&req).await.map_err(crate::study::api_err)?;
            let mut plan = library::parse_split(&reply, syl, t, all.len()).map_err(course_err)?;
            for l in plan.lesson.iter_mut() {
                l.refs = l.sources.iter().filter_map(|i| docs.get(i - 1).map(|(r, _)| r.clone())).collect();
            }
            library::save_plan(root, collection, t, &plan).map_err(course_err)?;
            plan
        }
    };
    // 2. One text per lesson, from that lesson's references.
    let mut n = 0;
    for ls in plan.lesson.iter().filter(|l| !library::article_path(root, collection, t, &l.id).exists()) {
        if n >= room {
            break;
        }
        update(|p| p.current = format!("{}：{}", t.zh, ls.zh));
        let mine: Vec<SourceDoc> = docs.iter().filter(|(r, _)| ls.refs.is_empty() || ls.refs.contains(r)).map(|(_, d)| d.clone()).collect();
        let sources = if mine.is_empty() { docs.iter().map(|(_, d)| d.clone()).collect() } else { mine };
        let res: Result<String, ApiError> = async {
            let req = library::article_request(&tpl, syl, t, ls, native, &sources).map_err(course_err)?;
            let reply = provider.complete(&req).await.map_err(crate::study::api_err)?;
            let a = library::parse_article(&reply, syl, &sources).map_err(course_err)?;
            let md = library::render_article(syl, collection, t, ls, &a, &sources, &provider.id(), &date());
            let path = library::article_path(root, collection, t, &ls.id);
            std::fs::create_dir_all(path.parent().expect("has a parent")).map_err(|e| bad(e.to_string()))?;
            // Never overwrite: a text written meanwhile (or by hand) stays.
            if !path.exists() {
                std::fs::write(&path, md).map_err(|e| bad(e.to_string()))?;
            }
            Ok(a.title)
        }
        .await;
        match res {
            Ok(title) => {
                n += 1;
                update(|p| {
                    p.done += 1;
                    p.made.push(format!("{}・{}", ls.zh, title));
                });
            }
            Err(e) => update(|p| p.errors.push(format!("{}・{}：{}", t.zh, ls.zh, e.1))),
        }
    }
    Ok(n)
}

// ── a lesson from a text ────────────────────────────────

#[derive(Deserialize)]
struct LessonBody {
    collection: String,
    /// A lesson id (`<theme>-<n>`).
    id: String,
}

/// The text as a lesson source (title, text and where it came from).
pub(crate) fn source_of(a: &Article) -> SourceText {
    let first = a.sources.first().map(|s| s.url.clone()).unwrap_or_default();
    SourceText { title: a.title.clone(), url: first, source: format!("主題庫：{}", a.lesson_zh), text: a.text.clone() }
}

async fn lesson(State(s): State<Shared>, Json(b): Json<LessonBody>) -> R<Value> {
    let l = ws(&s).await?;
    let root = l.ws.root.clone();
    let syl = library::load_syllabus(&root, &b.collection).map_err(course_err)?;
    if !l.ws.profile.languages.contains(&syl.lang) {
        return Err(bad(format!("{} 不在你的學習語言中", syl.lang.label())));
    }
    let (t, ls) = syl
        .topic
        .iter()
        .find_map(|t| library::load_plan(&root, &b.collection, t).and_then(|p| p.lesson.into_iter().find(|x| x.id == b.id)).map(|ls| (t.clone(), ls)))
        .ok_or_else(|| bad("找不到這一課"))?;
    let a = read_text(&root, &b.collection, &t, &ls).ok_or_else(|| bad("這一課的文章還沒寫"))?;
    let keywords: Vec<String> = a.key_terms.iter().take(3).map(|k| k.term.clone()).chain(t.news.iter().take(2).cloned()).collect();
    let mut course = daily_course(&l, syl.lang, &a.title, keywords, Some(source_of(&a)), String::new(), &date()).await?;
    // The words to learn are the text's own key terms (not words due elsewhere).
    let n = course.lessons.first().map(|lp| lp.words.len()).unwrap_or(0).clamp(4, 8);
    if let Some(lp) = course.lessons.first_mut() {
        lp.words = a.key_terms.iter().take(n).map(|k| TargetWord { term: k.term.clone(), reading: String::new(), meaning: k.meaning.clone() }).collect();
        lp.purpose = ls.goal.clone();
    }
    course.kind = CourseKind::Single;
    course.name = format!("{}：{}", t.zh, ls.zh);
    course.summary = ls.goal.clone();
    store::save_new(&l.ws, &mut course).map_err(course_err)?;
    Ok(Json(json!({ "id": course.id, "next": 1, "lessons": course.lessons.len() })))
}
