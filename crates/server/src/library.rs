//! The topic library (docs/decisions.md §22): articles written once from
//! references, lessons made from them at each learner's level.
//!
//! - `GET /api/library` — collections, their topics, which articles exist.
//! - `POST /api/library/build {collection, ids?, limit?}` — write missing
//!   articles in the background (references fetched with robots.txt checked,
//!   the lessons model writes); `GET /api/library/build` shows progress.
//!   Already written articles are never rewritten.
//! - `POST /api/library/lesson {collection, id}` — a one-lesson course from an
//!   article for the active learner; the lesson is written when opened.

use crate::api::ws;
use crate::courses::{course_err, daily_course, date, templates};
use crate::{bad, ApiError, Shared};
use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use jl_core::course::library::{self, Article, SourceDoc, Syllabus, Topic};
use jl_core::course::{store, CourseKind, SourceText};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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

async fn list(State(s): State<Shared>, Query(q): Query<ListQ>) -> R<Value> {
    let l = ws(&s).await?;
    let root = l.ws.root.clone();
    let mut out = vec![];
    for c in library::collections(&root).into_iter().filter(|c| q.collection.as_deref().is_none_or(|x| x == c)) {
        let syl = library::load_syllabus(&root, &c).map_err(course_err)?;
        let topics: Vec<Value> = syl
            .topic
            .iter()
            .map(|t| {
                let p = library::article_path(&root, &c, t);
                let a = std::fs::read_to_string(&p).ok().and_then(|md| library::read_article(&md));
                json!({ "id": t.id, "cat": t.cat, "zh": t.zh, "en": t.en, "written": a.is_some(), "title": a.as_ref().map(|a| a.title.clone()), "words": a.as_ref().map(|a| a.words), "file": a.is_some().then(|| l.ws.relative(&p)) })
            })
            .collect();
        let written = topics.iter().filter(|t| t["written"] == true).count();
        out.push(json!({ "id": c, "name": syl.name, "lang": syl.lang.code(), "level": syl.level, "total": topics.len(), "written": written, "topics": topics }));
    }
    Ok(Json(json!({ "collections": out })))
}

// ── writing articles in the background ──────────────────

#[derive(Debug, Clone, Default, Serialize)]
pub struct Progress {
    pub running: bool,
    pub collection: String,
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
    #[serde(default)]
    ids: Vec<String>,
    /// At most this many new articles this time (default 10).
    limit: Option<usize>,
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
    let todo: Vec<Topic> = syl
        .topic
        .iter()
        .filter(|t| b.ids.is_empty() || b.ids.contains(&t.id))
        .filter(|t| !library::article_path(&root, &b.collection, t).exists())
        .take(b.limit.unwrap_or(10).clamp(1, 300))
        .cloned()
        .collect();
    if todo.is_empty() {
        return Err(bad("這些主題都已經有文章了"));
    }
    {
        let mut p = PROGRESS.lock().expect("lock");
        if p.as_ref().is_some_and(|p| p.running) {
            return Err(bad("已經在產生了，等這一批做完"));
        }
        *p = Some(Progress { running: true, collection: b.collection.clone(), total: todo.len(), ..Default::default() });
    }
    let s2 = s.clone();
    let n = todo.len();
    tokio::spawn(async move {
        for t in todo {
            update(|p| p.current = t.zh.clone());
            match write_one(&s2, &root, &b.collection, &syl, &t, &native).await {
                Ok(title) => update(|p| {
                    p.done += 1;
                    p.made.push(title);
                }),
                Err(e) => update(|p| p.errors.push(format!("{}：{}", t.zh, e.1))),
            }
        }
        update(|p| {
            p.running = false;
            p.current.clear();
        });
    });
    Ok(Json(json!({ "started": n })))
}

/// One topic: read its references, have the model write the article, save it.
async fn write_one(s: &Shared, root: &std::path::Path, collection: &str, syl: &Syllabus, t: &Topic, native: &str) -> Result<String, ApiError> {
    let mut sources = vec![];
    let mut problems = vec![];
    for r in &t.refs {
        let Some(page) = library::ref_page(r) else { continue };
        match jl_core::news::fetch_article(&s.fetcher, &page.url, page.selector).await {
            Ok(a) if !a.paragraphs.is_empty() => {
                let text = library::clean_reference_text(&a.paragraphs, REF_CHARS);
                let title = a.title.trim_end_matches(" - Wikipedia").trim_end_matches(" – Travel guide at Wikivoyage").to_string();
                sources.push(SourceDoc { title, url: page.url.clone(), licence: page.licence.into(), text });
            }
            Ok(_) => problems.push(format!("{r}：沒有內文")),
            Err(e) => problems.push(format!("{r}：{e}")),
        }
    }
    if sources.is_empty() {
        return Err(bad(format!("一個參考資料都讀不到（{}）", problems.join("；"))));
    }
    let t_ = { templates(&ws(s).await?.ws) };
    let req = library::article_request(&t_, syl, t, native, &sources).map_err(course_err)?;
    let provider = crate::provider(s).await?;
    let reply = provider.complete(&req).await.map_err(crate::study::api_err)?;
    let a = library::parse_article(&reply, syl, &sources).map_err(course_err)?;
    let md = library::render_article(syl, collection, t, &a, &sources, &provider.id(), &date());
    let path = library::article_path(root, collection, t);
    std::fs::create_dir_all(path.parent().expect("has a parent")).map_err(|e| bad(e.to_string()))?;
    // Never overwrite: an article written meanwhile (or by hand) stays.
    if path.exists() {
        return Ok(a.title);
    }
    std::fs::write(&path, md).map_err(|e| bad(e.to_string()))?;
    Ok(a.title)
}

// ── a lesson from an article ────────────────────────────

#[derive(Deserialize)]
struct LessonBody {
    collection: String,
    id: String,
}

/// The article as a lesson source (title, text and where it came from).
pub(crate) fn source_of(a: &Article) -> SourceText {
    let refs: Vec<String> = a.sources.iter().map(|s| s.url.clone()).collect();
    SourceText { title: a.title.clone(), url: refs.first().cloned().unwrap_or_default(), source: format!("主題庫：{}", a.topic_zh), text: a.text.clone() }
}

async fn lesson(State(s): State<Shared>, Json(b): Json<LessonBody>) -> R<Value> {
    let l = ws(&s).await?;
    let root = l.ws.root.clone();
    let syl = library::load_syllabus(&root, &b.collection).map_err(course_err)?;
    if !l.ws.profile.languages.contains(&syl.lang) {
        return Err(bad(format!("{} 不在你的學習語言中", syl.lang.label())));
    }
    let t = syl.topic.iter().find(|t| t.id == b.id).ok_or_else(|| bad("找不到這個主題"))?;
    let md = std::fs::read_to_string(library::article_path(&root, &b.collection, t)).map_err(|_| bad("這個主題還沒有文章"))?;
    let a = library::read_article(&md).ok_or_else(|| bad("文章讀不懂"))?;
    let keywords: Vec<String> = a.key_terms.iter().take(3).map(|k| k.term.clone()).chain(t.news.iter().take(2).cloned()).collect();
    let mut course = daily_course(&l, syl.lang, &a.title, keywords, Some(source_of(&a)), String::new(), &date()).await?;
    // The words to learn are the article's own key terms (not words due elsewhere).
    let n = course.lessons.first().map(|lp| lp.words.len()).unwrap_or(0).clamp(4, 8);
    if let Some(lp) = course.lessons.first_mut() {
        lp.words = a.key_terms.iter().take(n).map(|k| jl_core::course::TargetWord { term: k.term.clone(), reading: String::new(), meaning: k.meaning.clone() }).collect();
    }
    course.kind = CourseKind::Single;
    course.name = format!("{}：{}", t.cat, t.zh);
    course.summary = a.title.clone();
    store::save_new(&l.ws, &mut course).map_err(course_err)?;
    Ok(Json(json!({ "id": course.id, "next": 1, "lessons": course.lessons.len() })))
}
