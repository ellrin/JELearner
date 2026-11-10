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
use jl_core::course::picture::{self, ImageRef};
use std::collections::HashMap;
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
        .route("/api/courses/{id}/picture", get(picture_file))
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
struct BuildBody {
    collection: String,
    /// Themes (or lesson ids' themes) to work on; empty = in syllabus order.
    #[serde(default)]
    ids: Vec<String>,
    /// At most this many new texts this time (default 10).
    limit: Option<usize>,
}

/// In turns over the categories (syllabus order within each), so a batch
/// of new lessons is not all of one kind.
pub(crate) fn spread<T>(items: Vec<T>, cat: impl Fn(&T) -> String) -> Vec<T> {
    let mut cats: Vec<String> = vec![];
    let mut queues: Vec<std::collections::VecDeque<T>> = vec![];
    for x in items {
        let c = cat(&x);
        match cats.iter().position(|k| *k == c) {
            Some(i) => queues[i].push_back(x),
            None => {
                cats.push(c);
                queues.push(std::collections::VecDeque::from([x]));
            }
        }
    }
    let mut out = vec![];
    while queues.iter().any(|q| !q.is_empty()) {
        for q in queues.iter_mut() {
            if let Some(x) = q.pop_front() {
                out.push(x);
            }
        }
    }
    out
}

/// Library lessons a learner has had (made into a course here or prepared
/// for their iPad): `profiles/<id>/library-used.txt`, one id per line, in
/// the private repository like the rest of their files.
pub(crate) fn used(ws: &jl_core::workspace::Workspace) -> Vec<String> {
    let read = |p: std::path::PathBuf| std::fs::read_to_string(p).unwrap_or_default().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect::<Vec<_>>();
    let mut v = read(ws.profile_dir().join("library-used.txt"));
    // Before 2026-11-11 the iPad's list lived in ipad/.
    for id in read(ws.profile_dir().join("ipad").join("library-used.txt")) {
        if !v.contains(&id) {
            v.push(id);
        }
    }
    v
}

pub(crate) fn mark_used(ws: &jl_core::workspace::Workspace, id: &str) -> std::io::Result<()> {
    if used(ws).iter().any(|x| x == id) {
        return Ok(());
    }
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(ws.profile_dir().join("library-used.txt"))?;
    writeln!(f, "{id}")
}

/// Up to `n` written lessons this learner has not had, in turns over the
/// categories; `skip` = lesson titles (中文) already on screen.
pub(crate) fn pick_unused(ws: &jl_core::workspace::Workspace, collection: &str, n: usize, skip: &[String]) -> Vec<(Topic, LessonFocus)> {
    let had = used(ws);
    let avail: Vec<(Topic, LessonFocus)> = written_lessons(&ws.root, collection).into_iter().filter(|(_, l)| !had.contains(&l.id) && !skip.iter().any(|s| s.trim() == l.zh.trim())).collect();
    spread(avail, |(t, _)| t.cat.clone()).into_iter().take(n).collect()
}

/// Write up to `n` more lesson texts (themes in turns over the categories),
/// for a run that found too few; returns how many were written.
pub(crate) async fn write_more(s: &Shared, root: &Path, collection: &str, native: &str, n: usize) -> Result<usize, ApiError> {
    let syl = library::load_syllabus(root, collection).map_err(course_err)?;
    let themes = spread(syl.topic.iter().filter(|t| has_work(root, collection, t)).cloned().collect(), |t| t.cat.clone());
    let mut written = 0;
    // One lesson per theme each pass, so the new ones are spread too.
    for t in themes {
        if written >= n {
            break;
        }
        written += work_on_theme(s, root, collection, &syl, &t, native, 1).await.unwrap_or(0);
    }
    Ok(written)
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
    let themes: Vec<Topic> = spread(syl.topic.iter().filter(|t| wanted(t) && has_work(&root, &b.collection, t)).cloned().collect(), |t| t.cat.clone());
    if themes.is_empty() {
        return Err(bad("這些主題的課都已經寫好了"));
    }
    let limit = b.limit.unwrap_or(10).clamp(1, 500);
    if current(&s).is_some_and(|p| p.running) {
        return Err(bad("已經在產生了，等這一批做完"));
    }
    set(&s, Progress { running: true, collection: b.collection.clone(), total: limit, ..Default::default() });
    let s2 = s.clone();
    tokio::spawn(async move {
        let mut written = 0;
        for t in themes {
            if written >= limit {
                break;
            }
            match work_on_theme(&s2, &root, &b.collection, &syl, &t, &native, limit - written).await {
                Ok(n) => written += n,
                Err(e) => update(&s2, |p| p.errors.push(format!("{}：{}", t.zh, e.1))),
            }
        }
        update(&s2, |p| {
            p.running = false;
            p.current.clear();
            p.total = p.done;
        });
    });
    Ok(Json(json!({ "started": true, "limit": limit })))
}

/// Read a theme's references once; split it if needed; write up to `room` texts.
async fn work_on_theme(s: &Shared, root: &Path, collection: &str, syl: &Syllabus, t: &Topic, native: &str, room: usize) -> Result<usize, ApiError> {
    update(s, |p| p.current = format!("讀參考資料：{}", t.zh));
    let mut docs: Vec<(String, SourceDoc)> = vec![];
    // Each reference page's lead picture (by its URL).
    let mut lead: HashMap<String, String> = HashMap::new();
    let mut problems = vec![];
    for r in &t.refs {
        let Some(page) = library::ref_page(r) else { continue };
        match s.fetcher.get_text(&page.url).await {
            Ok(html) => {
                let a = jl_core::news::extract_article(&html, page.selector);
                if a.paragraphs.is_empty() {
                    problems.push(format!("{r}：沒有內文"));
                    continue;
                }
                if let Some(img) = picture::og_image(&html, &page.url) {
                    lead.insert(page.url.clone(), img);
                }
                let text = library::clean_reference_text(&a.paragraphs, REF_CHARS);
                let title = a.title.trim_end_matches(" - Wikipedia").trim_end_matches(" – Travel guide at Wikivoyage").to_string();
                docs.push((r.clone(), SourceDoc { title, url: page.url.clone(), licence: page.licence.into(), text }));
            }
            Err(e) => problems.push(format!("{r}：{e}")),
        }
    }
    let mut licences: HashMap<String, Option<ImageRef>> = HashMap::new();
    if docs.is_empty() {
        return Err(bad(format!("一個參考資料都讀不到（{}）", problems.join("；"))));
    }
    let tpl = { templates(&ws(s).await?.ws) };
    let provider = crate::provider(s).await?;
    // 1. Split, once per theme.
    let plan = match library::load_plan(root, collection, t) {
        Some(p) => p,
        None => {
            update(s, |p| p.current = format!("拆課：{}", t.zh));
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
        update(s, |p| p.current = format!("{}：{}", t.zh, ls.zh));
        let mine: Vec<SourceDoc> = docs.iter().filter(|(r, _)| ls.refs.is_empty() || ls.refs.contains(r)).map(|(_, d)| d.clone()).collect();
        let sources = if mine.is_empty() { docs.iter().map(|(_, d)| d.clone()).collect() } else { mine };
        let res: Result<String, ApiError> = async {
            let req = library::article_request(&tpl, syl, t, ls, native, &sources).map_err(course_err)?;
            let reply = provider.complete(&req).await.map_err(crate::study::api_err)?;
            let a = library::parse_article(&reply, syl, &sources).map_err(course_err)?;
            // A free picture from the first used source that has one.
            let mut image = None;
            for i in &a.used_sources {
                let Some(img) = sources.get(i - 1).and_then(|d| lead.get(&d.url)) else { continue };
                if !licences.contains_key(img) {
                    let found = free_picture(s, img).await;
                    licences.insert(img.clone(), found);
                }
                if let Some(Some(r)) = licences.get(img) {
                    image = Some(r.clone());
                    break;
                }
            }
            let md = library::render_article(syl, collection, t, ls, &a, &sources, image.as_ref(), &provider.id(), &date());
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
                update(s, |p| {
                    p.done += 1;
                    p.made.push(format!("{}・{}", ls.zh, title));
                });
            }
            Err(e) => update(s, |p| p.errors.push(format!("{}・{}：{}", t.zh, ls.zh, e.1))),
        }
    }
    Ok(n)
}

/// A Commons picture with its author and a free licence, or nothing.
async fn free_picture(s: &Shared, img: &str) -> Option<ImageRef> {
    let page = picture::commons_page(img)?;
    let html = s.fetcher.get_text(&page).await.ok()?;
    let (author, licence, licence_url) = picture::parse_commons(&html)?;
    picture::free_licence(&licence).then(|| ImageRef { url: img.to_string(), page, author, licence, licence_url })
}

// ── a lesson's picture: stored while the lesson is open ─

fn picture_paths(ws: &jl_core::workspace::Workspace, lang: jl_core::Lang, course: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = ws.lessons_dir(lang).join("images");
    (dir.join(format!("{course}.jpg")), dir.join(format!("{course}.json")))
}

/// The picture of a course's lesson, as the lesson page shows it.
pub(crate) fn picture_info(ws: &jl_core::workspace::Workspace, lang: jl_core::Lang, course: &str) -> Option<Value> {
    let (jpg, meta) = picture_paths(ws, lang, course);
    if !jpg.is_file() {
        return None;
    }
    let r: ImageRef = serde_json::from_str(&std::fs::read_to_string(meta).ok()?).ok()?;
    Some(json!({ "src": format!("/api/courses/{}/picture", urlencoding(course)), "author": r.author, "licence": r.licence, "licence_url": r.licence_url, "page": r.page }))
}

fn urlencoding(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// A finished lesson does not keep its picture (the credit stays in the text).
pub(crate) fn remove_picture(ws: &jl_core::workspace::Workspace, lang: jl_core::Lang, course: &str) {
    let (jpg, meta) = picture_paths(ws, lang, course);
    let _ = std::fs::remove_file(jpg);
    let _ = std::fs::remove_file(meta);
}

async fn picture_file(State(s): State<Shared>, axum::extract::Path(id): axum::extract::Path<String>) -> Result<axum::response::Response, ApiError> {
    use axum::response::IntoResponse;
    let l = ws(&s).await?;
    let c = store::load(&l.ws, &id).map_err(course_err)?;
    let (jpg, _) = picture_paths(&l.ws, c.lang, &c.id);
    let bytes = std::fs::read(jpg).map_err(|_| ApiError(axum::http::StatusCode::NOT_FOUND, "沒有圖片".into()))?;
    Ok(([(axum::http::header::CONTENT_TYPE, "image/jpeg"), (axum::http::header::CACHE_CONTROL, "private, max-age=3600")], bytes).into_response())
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
    let course = course_from_text(&s, &l, &b.collection, &b.id, &date()).await?;
    let _ = mark_used(&l.ws, &b.id);
    let picture = picture_paths(&l.ws, course.lang, &course.id).0.is_file();
    Ok(Json(json!({ "id": course.id, "next": 1, "lessons": course.lessons.len(), "picture": picture })))
}

/// The collection for a language (the first one), if the workspace has one.
pub(crate) fn collection_for(root: &Path, lang: jl_core::Lang) -> Option<String> {
    library::collections(root).into_iter().find(|c| library::load_syllabus(root, c).is_ok_and(|s| s.lang == lang))
}

/// Every planned lesson whose text is written, in syllabus order.
pub(crate) fn written_lessons(root: &Path, collection: &str) -> Vec<(Topic, LessonFocus)> {
    let Ok(syl) = library::load_syllabus(root, collection) else { return vec![] };
    syl.topic
        .iter()
        .flat_map(|t| library::load_plan(root, collection, t).map(|p| p.lesson).unwrap_or_default().into_iter().map(move |l| (t.clone(), l)))
        .filter(|(t, l)| library::article_path(root, collection, t, &l.id).exists())
        .collect()
}

/// The picture of a lesson for the iPad: its path in the member's folder and its credit.
pub(crate) fn picture_for_package(ws: &jl_core::workspace::Workspace, lang: jl_core::Lang, course: &str) -> Value {
    let (jpg, meta) = picture_paths(ws, lang, course);
    let Ok(r) = std::fs::read_to_string(meta).and_then(|t| serde_json::from_str::<ImageRef>(&t).map_err(std::io::Error::other)) else { return Value::Null };
    if !jpg.is_file() {
        return Value::Null;
    }
    let rel = jpg.strip_prefix(ws.profile_dir()).map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    json!({ "path": rel, "author": r.author, "licence": r.licence, "page": r.page })
}

/// A one-lesson course from a library text for `l` (the active learner or a
/// family member), dated `created`, with its picture downloaded and shrunk.
pub(crate) async fn course_from_text(s: &Shared, l: &crate::Loaded, collection: &str, id: &str, created: &str) -> Result<jl_core::course::Course, ApiError> {
    let b = LessonBody { collection: collection.to_string(), id: id.to_string() };
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
    let mut course = daily_course(l, syl.lang, &a.title, keywords, Some(source_of(&a)), String::new(), created).await?;
    // The words to learn are the text's own key terms (not words due elsewhere).
    let n = course.lessons.first().map(|lp| lp.words.len()).unwrap_or(0).clamp(4, 8);
    if let Some(lp) = course.lessons.first_mut() {
        lp.words = a.key_terms.iter().take(n).map(|k| TargetWord { term: k.term.clone(), reading: String::new(), meaning: k.meaning.clone() }).collect();
        lp.purpose = ls.goal.clone();
    }
    course.kind = CourseKind::Single;
    // The lesson's own name (the theme made it too long to read on a card).
    course.name = ls.zh.clone();
    course.summary = ls.goal.clone();
    store::save_new(&l.ws, &mut course).map_err(course_err)?;
    // The picture: downloaded and shrunk now, removed when the lesson is done.
    if let Some(img) = &a.image
        && let Ok(bytes) = s.fetcher.get_bytes(&img.url).await
        && let Ok(jpg) = picture::compress(&bytes)
    {
        let (p, meta) = picture_paths(&l.ws, syl.lang, &course.id);
        if std::fs::create_dir_all(p.parent().expect("has a parent")).is_ok() && std::fs::write(&p, jpg).is_ok() {
            let _ = std::fs::write(meta, serde_json::to_string_pretty(img).unwrap_or_default());
        }
    }
    Ok(course)
}
