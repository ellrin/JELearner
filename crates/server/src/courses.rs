//! Courses in the server (docs/courses.md §10): create a topic or fixed
//! course, list and show courses, open a lesson (content generated on first
//! open), report unit results into the stage A points ledger, extend a topic
//! course, and today's lessons (the daily lesson, docs/plan.md §11). Like
//! `api.rs`, every handler is a plain async function over [`Shared`].

use crate::api::{fetch_article_into_db, need_lang, parse_lang, ws};
use crate::study::{api_err, auto_difficulty, local_today, rules, summary, try_level_up};
use crate::{bad, fail, provider, ApiError, Loaded, Shared};
use axum::extract::{Path as AxPath, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use jl_core::conversation::CourseLink;
use jl_core::course::outline::{self, FixedRequest, TopicRequest};
use jl_core::course::plan::{self, CourseLesson, LessonContext, SourceText};
use jl_core::course::{fixed, sizing, store, topics as ctopics, Course, CourseError, CourseKind, CourseStatus, CourseTemplates, Learner, LessonPlan, LessonRole, LessonStatus, UnitKind};
use jl_core::db::NewMistake;
use jl_core::lesson::{self, MAX_ARTICLE_CHARS};
use jl_core::notebook::{self, NoteBetter, NoteGrammar, NoteMistake, NoteWord, PageData};
use jl_core::points::{self, PointRecord, Skill};
use jl_core::workspace::{slug, Workspace};
use jl_core::Lang;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

type R<T> = Result<Json<T>, ApiError>;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/courses", get(list_courses).post(create_course))
        .route("/api/courses/lists", get(word_lists))
        .route("/api/courses/{id}", get(get_course))
        .route("/api/courses/{id}/extend", post(extend_course))
        .route("/api/courses/{id}/lessons/{n}/open", post(open_lesson))
        .route("/api/courses/{id}/lessons/{n}/units/{u}", post(report_unit))
        .route("/api/daily", post(start_daily))
}

// ── helpers ─────────────────────────────────────────────

fn date() -> String {
    local_today().format("%Y-%m-%d").to_string()
}

fn course_err(e: CourseError) -> ApiError {
    match e {
        CourseError::NotFound(m) => ApiError(StatusCode::NOT_FOUND, format!("找不到：{m}")),
        CourseError::Provider(_) => api_err(e),
        CourseError::Io(_) => fail(e.to_string()),
        _ => bad(e.to_string()),
    }
}

/// The learner for the course engine. `known` = words left out of new words:
/// all the words in their lists for topic courses, only the ones they have
/// really learned (interval ≥ 21 days) for fixed courses, which are made from
/// those very lists.
async fn learner(l: &Loaded, lang: Lang, mastered_only: bool) -> Result<Learner, ApiError> {
    let known: Vec<String> = if mastered_only {
        sqlx::query_scalar(
            "SELECT w.term FROM words w JOIN progress p ON p.item_kind = 'word' AND p.item_id = w.id WHERE w.lang = ? AND p.interval_days >= 21 LIMIT 2000",
        )
        .bind(lang.code())
        .fetch_all(l.db.pool())
        .await?
    } else {
        l.db.list_words(Some(lang), "", 400).await?.into_iter().map(|w| w.term).collect()
    };
    Ok(Learner::from_profile(&l.ws.profile, lang, known))
}

fn templates(ws: &Workspace) -> CourseTemplates {
    CourseTemplates::load(Some(&ws.prompts_dir()))
}

/// Points records keep the stage A skills; a typed conversation is writing
/// (docs/plan.md §20, stage A decisions).
pub(crate) fn points_skill(k: UnitKind) -> Skill {
    use jl_core::course::Skill as C;
    match k {
        UnitKind::Conversation => Skill::Writing,
        _ => match k.skill() {
            C::Listening => Skill::Listening,
            C::Reading => Skill::Reading,
            C::Speaking => Skill::Speaking,
            C::Writing => Skill::Writing,
            C::VocabGrammar => Skill::Vocab,
        },
    }
}

/// When the learner last worked on a course (to find "the course they are doing").
fn activity(c: &Course) -> String {
    c.lessons.iter().flat_map(|l| [l.done.clone(), l.generated.clone()]).flatten().max().unwrap_or_else(|| c.created.clone())
}

fn load_all(ws: &Workspace, lang: Option<Lang>) -> (Vec<Course>, Vec<String>) {
    let (list, skipped) = store::list(ws);
    let courses = list.into_iter().filter(|c| lang.is_none_or(|l| l == c.lang)).filter_map(|c| store::load(ws, &c.id).ok()).collect();
    (courses, skipped)
}

async fn lesson_ids(l: &Loaded, c: &Course) -> Result<Vec<Option<i64>>, ApiError> {
    let mut out = vec![];
    for lp in &c.lessons {
        out.push(if lp.status == LessonStatus::Planned { None } else { l.db.course_lesson_id(&c.id, lp.number).await? });
    }
    Ok(out)
}

fn unit_json(lp: &LessonPlan) -> Vec<Value> {
    lp.units
        .iter()
        .enumerate()
        .map(|(i, u)| json!({ "index": i, "kind": u.kind.code(), "skill": u.skill.code(), "minutes": u.minutes, "items": u.items, "done": lp.units_done.contains(&(i as u32)) }))
        .collect()
}

/// A course for the UI: the outline with every lesson's status, units and
/// database id, plus what to do next.
async fn course_json(l: &Loaded, c: &Course) -> Result<Value, ApiError> {
    let ids = lesson_ids(l, c).await?;
    let lessons: Vec<Value> = c
        .lessons
        .iter()
        .zip(ids)
        .map(|(lp, id)| {
            json!({
                "number": lp.number, "role": lp.role, "title": lp.title, "angle": lp.angle, "purpose": lp.purpose,
                "status": lp.status, "series": lp.series.max(1), "minutes": sizing::total_minutes(&lp.units),
                "words": lp.words, "review": lp.review, "grammar": lp.grammar, "units": unit_json(lp),
                "units_done": lp.units_done.len(), "lesson_id": id, "done": lp.done, "generated": lp.generated,
                "has_article": lp.article.is_some(),
            })
        })
        .collect();
    Ok(json!({
        "id": c.id, "kind": c.kind, "kind_label": c.kind.label(), "name": c.name, "lang": c.lang, "level": c.level,
        "created": c.created, "status": c.status, "summary": c.summary, "keywords": c.keywords, "source_url": c.source_url,
        "vocab": c.vocab, "grammar": c.grammar, "series": c.series.max(1),
        "total": c.lessons.len(), "done": c.done_count(), "next": c.next_lesson().map(|x| x.number),
        "next_title": c.next_lesson().map(|x| x.title.clone()),
        "can_extend": c.can_extend(), "words_total": c.all_words().len(),
        "lessons": lessons,
    }))
}

// ── list, show ──────────────────────────────────────────

#[derive(Deserialize, Default)]
struct LangQ {
    lang: Option<String>,
}

async fn list_courses(State(s): State<Shared>, Query(q): Query<LangQ>) -> R<Value> {
    let l = ws(&s).await?;
    let lang = parse_lang(&q.lang)?;
    let (courses, skipped) = load_all(&l.ws, lang);
    let mut out = vec![];
    for c in courses.iter().filter(|c| c.kind != CourseKind::Daily) {
        let next = c.next_lesson();
        out.push(json!({
            "id": c.id, "kind": c.kind, "kind_label": c.kind.label(), "name": c.name, "lang": c.lang, "status": c.status,
            "created": c.created, "summary": c.summary, "keywords": c.keywords, "total": c.lessons.len(), "done": c.done_count(),
            "next": next.map(|x| x.number), "next_title": next.map(|x| x.title.clone()), "can_extend": c.can_extend(),
            "activity": activity(c), "words": c.all_words().len(),
        }));
    }
    Ok(Json(json!({ "courses": out, "skipped": skipped })))
}

async fn get_course(State(s): State<Shared>, AxPath(id): AxPath<String>) -> R<Value> {
    let l = ws(&s).await?;
    let c = store::load(&l.ws, &id).map_err(course_err)?;
    Ok(Json(course_json(&l, &c).await?))
}

/// Word and grammar lists in the learner's workspace, for a fixed course.
async fn word_lists(State(s): State<Shared>, Query(q): Query<LangQ>) -> R<Value> {
    let l = ws(&s).await?;
    let lang = parse_lang(&q.lang)?;
    let mut out = vec![];
    // `course_only`: uploaded for a course (`recipes/lists/`), not in review.
    let lists = store::lists_dir(&l.ws);
    for (kind, dir, course_only) in [("vocab", l.ws.vocab_dir(), false), ("grammar", l.ws.grammar_dir(), false), ("vocab", lists.join("vocab"), true), ("grammar", lists.join("grammar"), true)] {
        for path in jl_core::workspace::files_with_ext(&dir, &["csv", "tsv"]) {
            let flang = jl_core::import::lang_from_path(&path, l.ws.profile.languages.first().copied().unwrap_or(Lang::En));
            if lang.is_some_and(|x| x != flang) {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let n = if kind == "vocab" { fixed::read_word_list(&text).len() } else { fixed::read_grammar_list(&text).len() };
            out.push(json!({ "kind": kind, "path": l.ws.relative(&path), "name": path.file_name().map(|f| f.to_string_lossy().to_string()), "items": n, "lang": flang, "course_only": course_only }));
        }
    }
    Ok(Json(json!(out)))
}

// ── create ──────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct SourceIn {
    #[serde(default)]
    url: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    title: String,
}

#[derive(Deserialize)]
struct CreateBody {
    /// `topic` | `fixed`
    kind: String,
    lang: Option<String>,
    #[serde(default)]
    name: String,
    // topic
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    sources: Vec<SourceIn>,
    lessons: Option<u32>,
    // fixed: lists already in the workspace (workspace-relative paths) …
    #[serde(default)]
    vocab: Vec<String>,
    #[serde(default)]
    grammar: Vec<String>,
    // … or uploaded now (saved into the workspace first, never overwriting)
    vocab_text: Option<String>,
    grammar_text: Option<String>,
    file_name: Option<String>,
    #[serde(default)]
    new_per_lesson: u32,
    /// Let the model name the fixed course's lessons (default: when an engine is ready).
    describe: Option<bool>,
}

/// Stored news below this many articles says little about a topic (a
/// handful of feeds), so the series length is left to the model's
/// `enough_material` then.
const NEWS_FOR_COUNTING: i64 = 30;

/// Count stored news articles that mention a keyword (only when there is
/// enough news to tell, so a small or fresh workspace keeps the full series).
async fn available_articles(l: &Loaded, lang: Lang, keywords: &[String]) -> Result<Option<usize>, ApiError> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM articles WHERE lang = ?").bind(lang.code()).fetch_one(l.db.pool()).await?;
    if total < NEWS_FOR_COUNTING || keywords.is_empty() {
        return Ok(None);
    }
    let mut ids = std::collections::BTreeSet::new();
    for k in keywords {
        let pat = format!("%{}%", k.trim());
        let found: Vec<i64> = sqlx::query_scalar("SELECT id FROM articles WHERE lang = ?1 AND (title LIKE ?2 OR summary LIKE ?2 OR body LIKE ?2)")
            .bind(lang.code())
            .bind(&pat)
            .fetch_all(l.db.pool())
            .await?;
        ids.extend(found);
    }
    Ok(Some(ids.len()))
}

/// Fetch a page the learner gave (robots.txt checked) into a source text.
async fn fetch_source(s: &Shared, url: &str, lang: Lang) -> Result<SourceText, ApiError> {
    let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let id = fetch_article_into_db(s, url, lang, &host).await?;
    let l = ws(s).await?;
    let a = l.db.get_article(id).await?.ok_or_else(|| fail("文章消失了"))?;
    Ok(SourceText { title: a.title, url: a.url, source: a.source, text: a.body.chars().take(MAX_ARTICLE_CHARS).collect() })
}

/// Save an uploaded word or grammar list for a fixed course under a free
/// name in `recipes/lists/<vocab|grammar>/`. It is **not** imported: the
/// course's words enter review only when their lesson is opened, so a
/// 300-word list does not put 300 cards on today (docs/courses.md §6).
/// Returns the workspace-relative path.
fn save_list(l: &Loaded, kind: &str, text: &str, name: &str, lang: Lang) -> Result<String, ApiError> {
    let dir = store::lists_dir(&l.ws).join(if kind == "vocab" { "vocab" } else { "grammar" });
    std::fs::create_dir_all(&dir).map_err(|e| fail(e.to_string()))?;
    let stem = format!("{}-{}", list_stem(name, kind), lang.code());
    let path = crate::api::free_path(&dir, &stem, "csv");
    std::fs::write(&path, text).map_err(|e| fail(e.to_string()))?;
    Ok(l.ws.relative(&path))
}

/// A list's display stem from a file name, without a trailing language
/// code (`toeic-en.csv` → `toeic`).
pub(crate) fn list_stem(name: &str, kind: &str) -> String {
    let stem_src = Path::new(name).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    let base = slug(if stem_src.is_empty() { if kind == "vocab" { "course-words" } else { "course-grammar" } } else { &stem_src }, 40);
    match base.strip_suffix("-en").or_else(|| base.strip_suffix("-ja")) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => base,
    }
}

fn read_workspace_list(ws: &Workspace, rel: &str) -> Result<String, ApiError> {
    let p = ws.root.join(rel);
    let canon = p.canonicalize().map_err(|_| bad(format!("找不到 {rel}")))?;
    let root = ws.profile_dir().canonicalize().map_err(|e| fail(e.to_string()))?;
    if !canon.starts_with(&root) {
        return Err(bad("只能使用自己工作區裡的清單"));
    }
    std::fs::read_to_string(&canon).map_err(|e| fail(e.to_string()))
}

async fn create_course(State(s): State<Shared>, Json(b): Json<CreateBody>) -> R<Value> {
    let lang = need_lang(&b.lang)?;
    {
        let l = ws(&s).await?;
        if !l.ws.profile.languages.contains(&lang) {
            return Err(bad(format!("{} 不在你的學習語言中", lang.label())));
        }
    }
    let today = date();
    let mut warnings: Vec<String> = vec![];
    let mut course = match b.kind.as_str() {
        "topic" => {
            let keywords: Vec<String> = b.keywords.iter().flat_map(|k| k.split([',', '，', '、', '\n'])).map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect();
            let mut sources = vec![];
            for src in &b.sources {
                let url = src.url.trim();
                if !url.is_empty() {
                    match fetch_source(&s, url, lang).await {
                        Ok(t) => sources.push(t),
                        // A site that does not allow fetching: say what to do
                        // instead (the page offers a paste box), not the URL again.
                        Err(e) if e.1.contains("robots.txt") => warnings.push(ROBOTS_BLOCKED.to_string()),
                        Err(e) => warnings.push(format!("{url}：{}", e.1)),
                    }
                } else if !src.text.trim().is_empty() {
                    let title = if src.title.trim().is_empty() { src.text.trim().chars().take(30).collect() } else { src.title.trim().to_string() };
                    sources.push(SourceText { title, url: String::new(), source: "貼上".into(), text: src.text.trim().chars().take(MAX_ARTICLE_CHARS).collect() });
                }
            }
            warnings.dedup();
            if keywords.is_empty() && sources.is_empty() && !b.sources.is_empty() {
                return Err(bad(warnings.join("；")));
            }
            let (lr, t, interests, available) = {
                let l = ws(&s).await?;
                let interests = ctopics::parse_interests(&std::fs::read_to_string(l.ws.interests_file()).unwrap_or_default());
                (learner(&l, lang, false).await?, templates(&l.ws), interests, available_articles(&l, lang, &keywords).await?)
            };
            let req = TopicRequest { lang: Some(lang), keywords, sources, name: b.name.clone(), lessons: b.lessons, available_articles: available, ..Default::default() };
            let p = provider(&s).await?;
            outline::create_topic_course(p.as_ref(), &t, &lr, &req, &interests, &today).await.map_err(course_err)?
        }
        "fixed" => {
            let (lr, t, words, grammar, vocab_files, grammar_files) = {
                let l = ws(&s).await?;
                let mut vocab_files = b.vocab.clone();
                let mut grammar_files = b.grammar.clone();
                let name = b.file_name.clone().unwrap_or_default();
                if let Some(text) = b.vocab_text.as_deref().map(|t| t.trim_start_matches('\u{feff}')).filter(|t| !t.trim().is_empty()) {
                    vocab_files.push(save_list(&l, "vocab", text, &name, lang)?);
                }
                if let Some(text) = b.grammar_text.as_deref().map(|t| t.trim_start_matches('\u{feff}')).filter(|t| !t.trim().is_empty()) {
                    grammar_files.push(save_list(&l, "grammar", text, &name, lang)?);
                }
                let mut words = vec![];
                for f in &vocab_files {
                    words.extend(fixed::read_word_list(&read_workspace_list(&l.ws, f)?));
                }
                let mut grammar = vec![];
                for f in &grammar_files {
                    grammar.extend(fixed::read_grammar_list(&read_workspace_list(&l.ws, f)?));
                }
                (learner(&l, lang, true).await?, templates(&l.ws), words, grammar, vocab_files, grammar_files)
            };
            let name = if b.name.trim().is_empty() {
                vocab_files.iter().chain(&grammar_files).next().map(|f| list_stem(f, "vocab")).unwrap_or_default()
            } else {
                b.name.clone()
            };
            let req = FixedRequest { name, words, grammar, vocab_files, grammar_files, new_per_lesson: b.new_per_lesson, review_every: None };
            let mut course = outline::create_fixed_course(&lr, &req, &today).map_err(course_err)?;
            let want = b.describe.unwrap_or(true);
            if want && let Ok(p) = provider(&s).await {
                match outline::describe_fixed(p.as_ref(), &t, &lr, &mut course).await {
                    Ok(_) => {}
                    Err(e) => warnings.push(format!("課名：{e}")),
                }
            }
            course
        }
        other => return Err(bad(format!("不支援的課程種類：{other}（topic、fixed）"))),
    };
    let l = ws(&s).await?;
    store::save_new(&l.ws, &mut course).map_err(course_err)?;
    let mut out = course_json(&l, &course).await?;
    out["warnings"] = json!(warnings);
    Ok(Json(out))
}

/// The message for a URL whose site does not allow automatic fetching.
pub(crate) const ROBOTS_BLOCKED: &str = "這個網站不允許自動抓取，直接貼上文章文字吧";

// ── extend ──────────────────────────────────────────────

async fn extend_course(State(s): State<Shared>, AxPath(id): AxPath<String>) -> R<Value> {
    let (mut course, lr, t) = {
        let l = ws(&s).await?;
        let c = store::load(&l.ws, &id).map_err(course_err)?;
        if !c.can_extend() {
            return Err(bad("全部課完成後才能延伸"));
        }
        let lang = c.lang;
        (c, learner(&l, lang, false).await?, templates(&l.ws))
    };
    let last = course.lessons.iter().map(|x| x.number).max().unwrap_or(0);
    let p = provider(&s).await?;
    outline::extend_topic_course(p.as_ref(), &t, &lr, &mut course, &TopicRequest::default()).await.map_err(course_err)?;
    let l = ws(&s).await?;
    store::append_lessons(&l.ws, &course, last + 1, &date()).map_err(course_err)?;
    store::save_progress(&l.ws, &course).map_err(course_err)?;
    let c = store::load(&l.ws, &id).map_err(course_err)?;
    Ok(Json(course_json(&l, &c).await?))
}

// ── open a lesson ───────────────────────────────────────

/// A news article from the database for a lesson that needs one: stored
/// articles matching the lesson's search keywords, fetched when needed
/// (robots.txt checked), the one with the most target words; an article
/// lesson of a topic course takes the best keyword match even without them.
async fn find_article(s: &Shared, lang: Lang, lp: &LessonPlan) -> Option<SourceText> {
    let terms: Vec<String> = lp.words.iter().map(|w| w.term.clone()).chain(lp.review.iter().cloned()).collect();
    let mut rows: Vec<(i64, String, String, String, String)> = vec![];
    {
        let l = ws(s).await.ok()?;
        for k in lp.search.iter().take(3) {
            let pat = format!("%{}%", k.trim());
            let found: Vec<(i64, String, String, String, String)> = sqlx::query_as(
                "SELECT id, title, url, source, body FROM articles WHERE lang = ?1 AND (title LIKE ?2 OR summary LIKE ?2 OR body LIKE ?2)
                 ORDER BY COALESCE(published_at, fetched_at) DESC LIMIT 4",
            )
            .bind(lang.code())
            .bind(&pat)
            .fetch_all(l.db.pool())
            .await
            .ok()?;
            for f in found {
                if !rows.iter().any(|r| r.0 == f.0) {
                    rows.push(f);
                }
            }
        }
    }
    let mut cands = vec![];
    let mut fetched = 0;
    for (id, title, url, source, body) in rows.into_iter().take(5) {
        let body = if body.trim().is_empty() && url.starts_with("http") && fetched < 3 {
            fetched += 1;
            match fetch_article_into_db(s, &url, lang, &source).await {
                Ok(_) => ws(s).await.ok()?.db.get_article(id).await.ok().flatten().map(|a| a.body).unwrap_or_default(),
                Err(_) => continue,
            }
        } else {
            body
        };
        if body.trim().is_empty() {
            continue;
        }
        cands.push(SourceText { title, url, source, text: body.chars().take(MAX_ARTICLE_CHARS).collect() });
    }
    if let Some(i) = plan::pick_article(&cands, &terms, lang) {
        return Some(cands.swap_remove(i));
    }
    (lp.role != LessonRole::Regular).then(|| cands.into_iter().next()).flatten()
}

async fn recent_accuracy(l: &Loaded, lang: Lang) -> Result<Option<f32>, ApiError> {
    let scores = points::recent_scores(&l.db, lang, 10, None, None).await?;
    Ok((!scores.is_empty()).then(|| (scores.iter().sum::<f64>() / scores.len() as f64) as f32))
}

/// Add a generated course lesson to the database like `api.rs` does for a
/// single lesson: the lesson row (marked with its course) and its new words
/// and grammar for review.
async fn index_lesson(l: &Loaded, course: &Course, number: u32, file: &str, markdown: &str, engine: &str) -> Result<i64, ApiError> {
    let raw = lesson::extract_raw(markdown).ok_or_else(|| fail("教材檔沒有內嵌資料"))?;
    let cl: CourseLesson = serde_json::from_value(raw.clone()).map_err(|e| fail(e.to_string()))?;
    let level = lesson::front_matter(markdown, "level").unwrap_or_else(|| course.level.clone());
    let url = lesson::front_matter(markdown, "source").unwrap_or_default();
    let lang = course.lang;
    let id = l.db.insert_lesson(lang, &cl.lesson.title, &level, None, &url, file, markdown, &raw, engine).await?;
    l.db.set_lesson_course(id, &course.id, number).await?;
    jl_core::import::index_lesson_items(&l.db, &cl.lesson, lang, &level, file).await.map_err(|e| fail(e.to_string()))?;
    Ok(id)
}

/// Cards for the lesson's review round: due cards first, then the lesson's
/// review words and new words (as stored in the database, so grading them
/// moves them in the review schedule), at most `n`.
async fn review_cards(l: &Loaded, lang: Lang, lp: &LessonPlan, cl: Option<&CourseLesson>, n: u32) -> Result<Vec<Value>, ApiError> {
    let n = n.max(1) as usize;
    let mut out: Vec<Value> = vec![];
    let mut seen = std::collections::HashSet::new();
    for d in l.db.due_items(Some(lang), Utc::now(), n as i64).await? {
        seen.insert(d.front.to_lowercase());
        out.push(json!({ "item_kind": d.item_kind, "item_id": d.item_id, "front": d.front, "reading": d.reading, "back": d.back, "example": d.example }));
    }
    let terms: Vec<String> = lp.review.iter().cloned().chain(lp.words.iter().map(|w| w.term.clone())).collect();
    for t in terms {
        if out.len() >= n {
            break;
        }
        if !seen.insert(t.to_lowercase()) {
            continue;
        }
        let row: Option<(i64, String, String, String)> = sqlx::query_as("SELECT id, term, reading, meaning FROM words WHERE lang = ? AND term = ? COLLATE NOCASE LIMIT 1")
            .bind(lang.code())
            .bind(&t)
            .fetch_optional(l.db.pool())
            .await?;
        match row {
            Some((id, term, reading, meaning)) => out.push(json!({ "item_kind": "word", "item_id": id, "front": term, "reading": reading, "back": meaning, "example": "" })),
            None => {
                let v = cl.and_then(|c| c.lesson.vocabulary.iter().find(|v| v.term.eq_ignore_ascii_case(&t)));
                let w = lp.words.iter().find(|w| w.term.eq_ignore_ascii_case(&t));
                let (reading, meaning) = v.map(|v| (v.reading.clone(), v.meaning.clone())).or_else(|| w.map(|w| (w.reading.clone(), w.meaning.clone()))).unwrap_or_default();
                out.push(json!({ "item_kind": "word", "item_id": null, "front": t, "reading": reading, "back": meaning, "example": "" }));
            }
        }
    }
    out.truncate(n);
    Ok(out)
}

async fn open_lesson(State(s): State<Shared>, AxPath((id, n)): AxPath<(String, u32)>) -> R<Value> {
    let (course, lp, lr, t, mut ctx) = {
        let l = ws(&s).await?;
        let c = store::load(&l.ws, &id).map_err(course_err)?;
        let lp = c.lesson(n).cloned().ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("沒有第 {n} 課")))?;
        let lang = c.lang;
        let ctx = LessonContext { recent_accuracy: recent_accuracy(&l, lang).await?, article: None };
        (c, lp, learner(&l, lang, false).await?, templates(&l.ws), ctx)
    };
    let root = ws(&s).await?.ws.root.clone();
    let existing = lp.status != LessonStatus::Planned && lp.file.as_ref().is_some_and(|f| root.join(f).is_file());
    // Only a lesson that will be generated needs a provider and maybe news.
    let p = if existing { None } else { Some(provider(&s).await?) };
    if !existing && plan::needs_search(&lp) {
        ctx.article = find_article(&s, course.lang, &lp).await;
    }
    let mock_p;
    let prov: &dyn jl_core::provider::LlmProvider = match &p {
        Some(p) => p.as_ref(),
        None => {
            mock_p = jl_core::provider::MockProvider::new(vec![]);
            &mock_p
        }
    };
    let opened = {
        let l = ws(&s).await?;
        plan::open_lesson(prov, &t, &l.ws, &id, n, &lr, &ctx, &date()).await.map_err(course_err)?
    };
    let l = ws(&s).await?;
    let lesson_id = match l.db.course_lesson_id(&id, n).await? {
        Some(x) if opened.generated.is_none() => x,
        _ => {
            let engine = lesson::front_matter(&opened.markdown, "engine").unwrap_or_else(|| prov.id());
            index_lesson(&l, &opened.course, n, &opened.file, &opened.markdown, &engine).await?
        }
    };
    let row = l.db.get_lesson(lesson_id).await?.ok_or_else(|| fail("教材消失了"))?;
    let data: Value = serde_json::from_str(&row.data).unwrap_or(Value::Null);
    let cl: Option<CourseLesson> = serde_json::from_value(data.clone()).ok();
    let lp = opened.course.lesson(n).cloned().unwrap_or(lp);
    let review_items = lp.units.iter().find(|u| u.kind == UnitKind::Review).map(|u| u.items).unwrap_or(0);
    let cards = if review_items > 0 { review_cards(&l, course.lang, &lp, cl.as_ref(), review_items).await? } else { vec![] };
    let page = notebook::find_page(&l.ws, course.lang, &row.path).and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()));
    Ok(Json(json!({
        "course": course_json(&l, &opened.course).await?,
        "number": n,
        "lesson_id": lesson_id,
        "file": opened.file,
        "lesson": data,
        "row": { "title": row.title, "level": row.level, "lang": row.lang, "path": row.path, "engine": row.engine, "created_at": row.created_at, "source_url": row.source_url },
        "plan": { "number": lp.number, "role": lp.role, "title": lp.title, "angle": lp.angle, "purpose": lp.purpose, "units": unit_json(&lp), "words": lp.words, "review": lp.review, "status": lp.status },
        "review_cards": cards,
        "generated": opened.generated.is_some(),
        "missing": opened.generated.as_ref().map(|g| g.missing.clone()).unwrap_or_default(),
        "notebook": page,
    })))
}

// ── unit results ────────────────────────────────────────

#[derive(Deserialize)]
struct QuizAnswer {
    index: usize,
    #[serde(default)]
    answer: String,
    correct: Option<bool>,
}

#[derive(Deserialize)]
struct OutputAnswer {
    #[serde(default)]
    prompt: String,
    #[serde(default)]
    answer: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    hint: String,
    correct: bool,
}

#[derive(Deserialize, Default)]
struct UnitBody {
    /// Review: the share of cards remembered (0–1); others are computed.
    score: Option<f64>,
    /// Review: how many cards were done.
    cards: Option<u32>,
    /// Reading / quiz: the answers to the lesson's questions.
    answers: Option<Vec<QuizAnswer>>,
    /// Output exercises, as the learner marked them against the model answer.
    items: Option<Vec<OutputAnswer>>,
}

/// What comes after a unit: the next unit of the lesson, else the next lesson.
fn next_step(c: &Course, n: u32) -> Value {
    if let Some(lp) = c.lesson(n)
        && let Some(i) = (0..lp.units.len() as u32).find(|i| !lp.units_done.contains(i))
    {
        return json!({ "lesson": n, "unit": i, "kind": lp.units[i as usize].kind.code() });
    }
    match c.next_lesson() {
        Some(x) => json!({ "lesson": x.number, "unit": null, "title": x.title }),
        None => json!({ "lesson": null, "can_extend": c.can_extend() }),
    }
}

async fn report_unit(State(s): State<Shared>, AxPath((id, n, u)): AxPath<(String, u32, u32)>, body: Option<Json<UnitBody>>) -> R<Value> {
    let b = body.map(|b| b.0).unwrap_or_default();
    let r = rules(&s).await;
    let today = date();
    let (lang, kind, passed, score, pts, res, course) = {
        let l = ws(&s).await?;
        let mut course = store::load(&l.ws, &id).map_err(course_err)?;
        let lp = course.lesson(n).cloned().ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("沒有第 {n} 課")))?;
        let unit = lp.units.get(u as usize).cloned().ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("第 {n} 課沒有第 {} 個單元", u + 1)))?;
        let lesson_id = l.db.course_lesson_id(&id, n).await?.ok_or_else(|| bad("請先打開這一課"))?;
        let row = l.db.get_lesson(lesson_id).await?.ok_or_else(|| bad("找不到教材"))?;
        let lang = course.lang;
        let (passed, score) = match unit.kind {
            UnitKind::Conversation => return Err(bad("對話單元在對話完成時自動記錄")),
            UnitKind::Reading | UnitKind::Quiz => {
                let data: lesson::Lesson = serde_json::from_str(&row.data).map_err(|e| fail(e.to_string()))?;
                let answers = b.answers.unwrap_or_default();
                let total = data.questions.len().max(1);
                let correct = data
                    .questions
                    .iter()
                    .enumerate()
                    .filter(|(i, q)| answers.iter().find(|a| a.index == *i).and_then(|a| a.correct.or_else(|| lesson::grade(q, &a.answer))).unwrap_or(false))
                    .count();
                let sc = correct as f64 / total as f64;
                (points::passes(sc, &r.points), Some(sc))
            }
            UnitKind::Review => {
                let cards = b.cards.unwrap_or(0);
                if cards == 0 {
                    return Err(bad("還沒有複習任何卡片"));
                }
                (true, b.score.map(|x| x.clamp(0.0, 1.0)))
            }
            _ => {
                let items = b.items.unwrap_or_default();
                if items.is_empty() {
                    return Err(bad("還沒有作答"));
                }
                for it in items.iter().filter(|i| !i.correct) {
                    let m = NewMistake {
                        lang: Some(lang),
                        item_kind: "output".into(),
                        item_id: None,
                        lesson_id: Some(lesson_id),
                        question: plan::strip_furigana(&it.prompt),
                        user_answer: it.answer.clone(),
                        correct_answer: plan::strip_furigana(&it.model),
                        explanation: it.hint.clone(),
                    };
                    l.db.add_mistake(&m, lang).await?;
                }
                let sc = items.iter().filter(|i| i.correct).count() as f64 / items.len() as f64;
                (points::passes(sc, &r.points), Some(sc))
            }
        };
        let was_done = lp.units_done.contains(&u);
        let res = store::mark_unit(&mut course, n, u, passed, score.map(|x| x as f32), &today).map_err(course_err)?;
        store::save_progress(&l.ws, &course).map_err(course_err)?;
        // A repeat of a passed unit earns nothing and is not written again.
        let pts = if res.points > 0.0 { points::award(true, score, false, &r.points) } else { 0.0 };
        if !(was_done && passed) {
            let detail = format!("{}#{u}", l.ws.profile_relative(Path::new(&row.path)));
            let rec = PointRecord::now(lang, unit.kind.code(), points_skill(unit.kind), pts, score, &detail);
            points::record(&l.ws, &l.db, &rec, None).await.map_err(fail)?;
            l.db.link_course_units().await?;
        }
        refresh_lesson_page(&l, lesson_id).await;
        (lang, unit.kind, passed, score, pts, res, course)
    };
    let difficulty_change = if matches!(kind, UnitKind::Reading | UnitKind::Quiz) { auto_difficulty(&s, lang).await? } else { None };
    let level_up = try_level_up(&s, lang).await?;
    let l = ws(&s).await?;
    let lesson_id = l.db.course_lesson_id(&id, n).await?;
    let page = match lesson_id {
        Some(lid) => match l.db.get_lesson(lid).await? {
            Some(row) => notebook::find_page(&l.ws, lang, &row.path).and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string())),
            None => None,
        },
        None => None,
    };
    Ok(Json(json!({
        "passed": passed, "score": score, "points": pts, "already": res.points == 0.0 && passed,
        "lesson_done": res.lesson_done, "course_finished": res.course_finished,
        "next": next_step(&course, n),
        "course": course_json(&l, &course).await?,
        "difficulty_change": difficulty_change, "level_up": level_up,
        "ask_difficulty": res.lesson_done && l.ws.profile.lang(lang).early_feedback_left > 0,
        "notebook": page,
        "summary": summary(&l.ws, &l.db, lang, &r).await?,
    })))
}

/// The conversation unit of a course lesson is done when its conversation
/// is finished: the conversation already wrote its points line, so only
/// the course progress changes here.
pub(crate) async fn conversation_unit_done(l: &Loaded, link: &CourseLink, passed: bool, score: Option<f64>) -> Result<Value, ApiError> {
    let mut course = store::load(&l.ws, &link.course).map_err(course_err)?;
    let res = store::mark_unit(&mut course, link.lesson, link.unit, passed, score.map(|x| x as f32), &date()).map_err(course_err)?;
    store::save_progress(&l.ws, &course).map_err(course_err)?;
    if let Some(lid) = l.db.course_lesson_id(&link.course, link.lesson).await? {
        refresh_lesson_page(l, lid).await;
    }
    Ok(json!({ "id": course.id, "name": course.name, "lesson": link.lesson, "lesson_done": res.lesson_done, "course_finished": res.course_finished, "next": next_step(&course, link.lesson) }))
}

// ── notebook pages ──────────────────────────────────────

/// Create or refresh the notebook page of a lesson (wrong answers, new
/// words and grammar). Never fails the request it is part of.
pub(crate) async fn refresh_lesson_page(l: &Loaded, lesson_id: i64) -> Option<String> {
    let row = l.db.get_lesson(lesson_id).await.ok()??;
    let data: lesson::Lesson = serde_json::from_str(&row.data).ok()?;
    let lang: Lang = row.lang.parse().ok()?;
    let mistakes: Vec<(String, String, String, String)> =
        sqlx::query_as("SELECT question, user_answer, correct_answer, explanation FROM mistakes WHERE lesson_id = ? ORDER BY id")
            .bind(lesson_id)
            .fetch_all(l.db.pool())
            .await
            .ok()?;
    let course = sqlx::query_scalar::<_, String>("SELECT course FROM lessons WHERE id = ?").bind(lesson_id).fetch_one(l.db.pool()).await.unwrap_or_default();
    let d = PageData {
        kind: "lesson".into(),
        title: plan::strip_furigana(&data.title),
        lang,
        date: row.created_at.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string(),
        source: row.path.clone(),
        course,
        summary: data.summary.clone(),
        mistakes: mistakes.into_iter().map(|(q, y, a, e)| NoteMistake { question: q, your_answer: y, answer: a, explanation: e }).collect(),
        words: data.vocabulary.iter().map(|v| NoteWord { term: v.term.clone(), reading: v.reading.clone(), meaning: v.meaning.clone(), example: v.example.clone() }).collect(),
        grammar: data.grammar.iter().map(|g| NoteGrammar { pattern: g.pattern.clone(), meaning: g.meaning.clone(), explanation: g.explanation.clone(), example: g.example.clone() }).collect(),
        better: vec![],
    };
    notebook::save(&l.ws, &d).ok().map(|(p, _)| p)
}

/// The notebook page of a finished conversation ("could say better").
pub(crate) fn conversation_page(l: &Loaded, c: &jl_core::conversation::Conversation, rel: &str) -> Option<String> {
    let review = c.review.as_ref()?;
    let d = PageData {
        kind: "conversation".into(),
        title: c.topic.name.clone(),
        lang: c.lang,
        date: c.started_at.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string(),
        source: rel.to_string(),
        course: c.course.as_ref().map(|x| x.course.clone()).unwrap_or_default(),
        summary: review.summary.clone(),
        mistakes: vec![],
        words: review.suggested_words.iter().map(|w| NoteWord { term: w.term.clone(), reading: w.reading.clone(), meaning: w.meaning.clone(), example: String::new() }).collect(),
        grammar: vec![],
        better: review.better.iter().map(|b| NoteBetter { original: b.original.clone(), suggestion: b.suggestion.clone(), why: b.why.clone() }).collect(),
    };
    notebook::save(&l.ws, &d).ok().map(|(p, _)| p)
}

// ── today's lessons and the daily lesson ────────────────

fn general_topics(ws: &Workspace) -> Vec<ctopics::GeneralTopic> {
    let (general, _) = jl_core::topics::load_general(&ws.topics_file());
    general.into_iter().map(|t| ctopics::GeneralTopic { id: t.id, name: t.name, group: t.group }).collect()
}

/// A per-learner offset so two learners do not get the same topic each day.
fn seed(id: &str) -> u64 {
    id.bytes().fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(b as u64)) % 97
}

fn daily_topic(ws: &Workspace, lang: Lang) -> Option<(String, String, &'static str)> {
    let interests = ctopics::parse_interests(&std::fs::read_to_string(ws.interests_file()).unwrap_or_default());
    let day = ctopics::day_number(local_today()) + seed(&ws.profile.id);
    match ctopics::pick_topic(day, ws.profile.conversation.interest_ratio, &interests, &general_topics(ws), lang)? {
        ctopics::TopicPick::Interest { keyword } => Some((keyword.clone(), keyword, "interest")),
        ctopics::TopicPick::General { id, name } => Some((name, id, "general")),
    }
}

/// Points earned by each unit of a course lesson (index → best points), from
/// the unit attempts linked to the course and the lesson's conversation.
async fn unit_points(l: &Loaded, course: &str, number: u32) -> Result<std::collections::HashMap<u32, f64>, ApiError> {
    let mut out = std::collections::HashMap::new();
    let rows: Vec<(i64, f64)> = sqlx::query_as("SELECT unit_index, MAX(points) FROM units WHERE course = ? AND course_lesson = ? AND unit_index IS NOT NULL GROUP BY unit_index")
        .bind(course)
        .bind(number)
        .fetch_all(l.db.pool())
        .await?;
    for (i, p) in rows {
        out.insert(i as u32, p);
    }
    let conv: Vec<(i64, Option<f64>)> = sqlx::query_as(
        "SELECT json_extract(data, '$.course.unit'), points FROM conversations
         WHERE status = 'done' AND json_extract(data, '$.course.course') = ? AND json_extract(data, '$.course.lesson') = ?",
    )
    .bind(course)
    .bind(number)
    .fetch_all(l.db.pool())
    .await
    .unwrap_or_default();
    for (i, p) in conv {
        let e = out.entry(i as u32).or_insert(0.0);
        *e = e.max(p.unwrap_or(0.0));
    }
    Ok(out)
}

async fn lesson_entry(l: &Loaded, c: &Course, lp: &LessonPlan, lesson_id: Option<i64>, today: &str) -> Result<Value, ApiError> {
    let pts = unit_points(l, &c.id, lp.number).await?;
    let mut units = unit_json(lp);
    for u in units.iter_mut() {
        let i = u["index"].as_u64().unwrap_or(0) as u32;
        u["points"] = json!(pts.get(&i).copied());
    }
    Ok(json!({
        "course": c.id, "course_name": c.name, "kind": c.kind, "kind_label": c.kind.label(), "number": lp.number,
        "total": c.lessons.len(), "title": lp.title, "status": lp.status, "lesson_id": lesson_id,
        "units": units, "minutes": sizing::total_minutes(&lp.units), "done_today": lp.done.as_deref() == Some(today),
    }))
}

/// Today's lessons (one, or two when the daily goal is over 60 minutes):
/// lessons finished today, then the next lessons of the course the learner
/// is doing (the active course worked on most recently; a second lesson
/// comes from another active course when there is one, else the next
/// lesson of the same course). Without any course, today's daily lesson,
/// or the topic it would have.
pub(crate) async fn today_lessons(l: &Loaded, lang: Lang) -> Result<Value, ApiError> {
    let today = date();
    let lr = Learner::from_profile(&l.ws.profile, lang, vec![]);
    let per_day = lr.lessons_per_day();
    let (all, _) = load_all(&l.ws, Some(lang));
    let mut entries: Vec<Value> = vec![];
    for c in &all {
        for lp in c.lessons.iter().filter(|x| x.done.as_deref() == Some(today.as_str())) {
            entries.push(lesson_entry(l, c, lp, l.db.course_lesson_id(&c.id, lp.number).await?, &today).await?);
        }
    }
    let mut active: Vec<&Course> = all.iter().filter(|c| c.status == CourseStatus::Active && c.next_lesson().is_some() && c.kind != CourseKind::Daily).collect();
    active.sort_by_key(|c| std::cmp::Reverse(activity(c)));
    let daily: Vec<&Course> = all.iter().filter(|c| c.kind == CourseKind::Daily && c.created == today && c.next_lesson().is_some()).collect();
    let sources: Vec<&Course> = if active.is_empty() { daily } else { active };
    let mut picked: Vec<(&Course, &LessonPlan)> = vec![];
    // One lesson from each course in turn, then more from the first.
    for c in &sources {
        if entries.len() + picked.len() >= per_day {
            break;
        }
        if let Some(lp) = c.next_lesson() {
            picked.push((c, lp));
        }
    }
    if let Some(c) = sources.first() {
        for lp in c.lessons.iter().filter(|x| x.status != LessonStatus::Done) {
            if entries.len() + picked.len() >= per_day {
                break;
            }
            if !picked.iter().any(|(pc, p)| pc.id == c.id && p.number == lp.number) {
                picked.push((c, lp));
            }
        }
    }
    for (c, lp) in picked {
        entries.push(lesson_entry(l, c, lp, l.db.course_lesson_id(&c.id, lp.number).await?, &today).await?);
    }
    let mut suggestion = Value::Null;
    if entries.is_empty() && sources.is_empty() {
        let minutes = lr.lesson_minutes();
        let units = sizing::unit_plan(LessonRole::Regular, minutes, ctopics::day_number(local_today()) as u32);
        let topic = daily_topic(&l.ws, lang);
        let lp = LessonPlan { number: 1, title: topic.as_ref().map(|t| t.0.clone()).unwrap_or_else(|| "每日課程".into()), units, minutes, ..Default::default() };
        suggestion = json!({ "topic": topic.as_ref().map(|t| json!({ "name": t.0, "id": t.1, "kind": t.2 })), "units": unit_json(&lp), "minutes": sizing::total_minutes(&lp.units), "lessons": per_day });
    }
    Ok(json!({ "lessons": entries, "per_day": per_day, "daily": suggestion }))
}

#[derive(Deserialize)]
struct DailyBody {
    lang: Option<String>,
}

/// Create today's daily lesson (or return it): a one-day course of kind
/// `daily` with one lesson (two when the daily goal is over 60 minutes) on
/// the day's topic (interests / general topics by the learner's ratio). Its
/// words come from the learner's own list, the ones practised least.
async fn start_daily(State(s): State<Shared>, Json(b): Json<DailyBody>) -> R<Value> {
    let lang = need_lang(&b.lang)?;
    let today = date();
    let l = ws(&s).await?;
    let (all, _) = load_all(&l.ws, Some(lang));
    if let Some(c) = all.iter().find(|c| c.kind == CourseKind::Daily && c.created == today) {
        return Ok(Json(json!({ "id": c.id, "next": c.next_lesson().map(|x| x.number), "created": false })));
    }
    let lr = learner(&l, lang, false).await?;
    let (name, _id, _kind) = daily_topic(&l.ws, lang).ok_or_else(|| bad("沒有可用的主題：請在興趣或 topics.toml 加入主題"))?;
    let size = lr.size();
    let targets = jl_core::conversation::pick_targets(&l.db, lang, size.topic_new_words.min.max(3) as usize * lr.lessons_per_day()).await?;
    let per = lr.lessons_per_day();
    let chunk = targets.len().div_ceil(per.max(1)).max(1);
    let day = ctopics::day_number(local_today()) as u32;
    let lessons: Vec<LessonPlan> = (0..per)
        .map(|i| {
            let words = targets.iter().skip(i * chunk).take(chunk).map(|t| jl_core::course::TargetWord { term: t.term.clone(), reading: t.reading.clone(), meaning: t.meaning.clone() }).collect();
            let minutes = sizing::daily_sessions(lr.daily_minutes)[i.min(sizing::daily_sessions(lr.daily_minutes).len() - 1)];
            LessonPlan {
                number: i as u32 + 1,
                role: LessonRole::Regular,
                title: if per > 1 { format!("{name}（{}）", i + 1) } else { name.clone() },
                angle: if per > 1 && i == 1 { "personal life".into() } else { String::new() },
                purpose: "用自己的單字讀今天的主題，再說出來".into(),
                search: vec![name.clone()],
                conversation_topic: name.clone(),
                minutes,
                words,
                units: sizing::unit_plan(LessonRole::Regular, minutes, day + i as u32),
                ..Default::default()
            }
        })
        .collect();
    let mut course = Course {
        id: String::new(),
        kind: CourseKind::Daily,
        name: name.clone(),
        lang,
        level: lr.level().label,
        created: today,
        status: CourseStatus::Active,
        daily_minutes: lr.daily_minutes,
        summary: name.clone(),
        keywords: vec![name.clone()],
        source_url: String::new(),
        vocab: vec![],
        grammar: vec![],
        new_per_lesson: 0,
        review_every: 0,
        series: 1,
        engine: String::new(),
        lessons,
    };
    store::save_new(&l.ws, &mut course).map_err(course_err)?;
    Ok(Json(json!({ "id": course.id, "next": 1, "created": true })))
}
