//! Course outlines from the model: topic courses (and their extensions) and
//! titles for fixed courses. The model proposes; the code decides the shape
//! (number of lessons, roles, word limits, recurrence, unit plans), so the
//! outline is valid whatever the model returns.

use super::fixed::{self, SplitOptions};
use super::model::*;
use super::prompts::{OUTLINE_SYSTEM, OUTLINE_USER};
use super::sizing::{total_minutes, unit_plan, ContentSize};
use super::topics::{interests_for, Interest};
use super::{parse_json, strict_object, CourseError, CourseTemplates, Learner};
use crate::lang::Lang;
use crate::level;
use crate::provider::{CompletionRequest, LlmProvider};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// A topic course is a series of 5 lessons (~one week) by default…
pub const DEFAULT_SERIES: u32 = 5;
/// …or 3 when the topic is too narrow for enough material.
pub const SHORT_SERIES: u32 = 3;
/// Each new word of a topic series appears in at least this many lessons
/// (or every lesson left after it, when fewer remain).
pub const MIN_RECURRENCE: usize = 3;
/// Angles of the article lessons (2–4) of a 5-lesson series.
pub const ANGLES: [&str; 3] = ["technical", "social", "personal life"];
/// Longest page or photo text sent with the request (characters).
pub const MAX_SOURCE_CHARS: usize = 6_000;

/// What the learner asked a topic course to be about.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TopicRequest {
    pub lang: Option<Lang>,
    #[serde(default)]
    pub keywords: Vec<String>,
    /// A page the topic comes from.
    #[serde(default)]
    pub url: String,
    /// Text of that page, or text read from a photo (one source; see
    /// `sources` for several).
    #[serde(default)]
    pub source_text: String,
    /// Pages / photo texts the learner gave, in order. They become the
    /// texts of the article lessons (2, 3, 4) before any news search.
    #[serde(default)]
    pub sources: Vec<SourceText>,
    /// Course name; the model's title when empty.
    #[serde(default)]
    pub name: String,
    /// Lessons in the series; [`DEFAULT_SERIES`] when not set.
    #[serde(default)]
    pub lessons: Option<u32>,
    /// How many news articles were found for the keywords (when the server
    /// searched first): fewer than 3 shortens the series to 3.
    #[serde(default)]
    pub available_articles: Option<usize>,
}

/// Lessons in the series for a request.
pub fn series_length(req: &TopicRequest) -> u32 {
    if let Some(n) = req.lessons {
        return n.clamp(SHORT_SERIES, 10);
    }
    match req.available_articles {
        Some(n) if n < SHORT_SERIES as usize => SHORT_SERIES,
        _ => DEFAULT_SERIES,
    }
}

/// The learner's own sources: `sources`, or `url` + `source_text` when
/// `sources` is empty. Sources without text are left out (the server
/// fetches the page text first, through the robots.txt-checking fetcher).
pub fn given_sources(req: &TopicRequest) -> Vec<SourceText> {
    let mut v: Vec<SourceText> = req.sources.clone();
    if v.is_empty() && !req.source_text.trim().is_empty() {
        v.push(SourceText { url: req.url.trim().to_string(), text: req.source_text.clone(), ..Default::default() });
    }
    v.retain(|s| !s.text.trim().is_empty());
    v
}

/// Does the `i`-th (0-based) lesson of an `n`-lesson series introduce new
/// words? Not the last two (lesson 4 deepens the words of 1–3 from its own
/// angle, lesson 5 reviews them all), so every new word can appear in at
/// least 3 lessons. A 3-lesson series: lessons 1 and 2 introduce words,
/// lesson 3 is the review (lesson-2 words then appear twice).
pub fn introduces_words(i: usize, n: usize) -> bool {
    if n <= SHORT_SERIES as usize { i + 1 < n } else { i + 2 < n }
}

/// Role of the `i`-th (0-based) lesson of an `n`-lesson topic series.
pub fn topic_role(i: usize, n: usize) -> LessonRole {
    match i {
        0 => LessonRole::Background,
        _ if i + 1 == n => LessonRole::Review,
        _ => LessonRole::Article,
    }
}

/// Angles for the article lessons of an `n`-lesson series.
pub fn angles(n: usize) -> Vec<&'static str> {
    let k = n.saturating_sub(2);
    (0..k).map(|i| ANGLES[i % ANGLES.len()]).collect()
}

// ── What the model returns ───────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawOutline {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default = "yes")]
    pub enough_material: bool,
    #[serde(default)]
    pub lessons: Vec<RawLesson>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawLesson {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub angle: String,
    #[serde(default)]
    pub purpose: String,
    #[serde(default)]
    pub search: Vec<String>,
    #[serde(default)]
    pub conversation_topic: String,
    #[serde(default)]
    pub words: Vec<TargetWord>,
    #[serde(default)]
    pub grammar: Vec<TargetGrammar>,
}

pub fn topic_outline_schema() -> Value {
    let s = json!({ "type": "string" });
    let strings = json!({ "type": "array", "items": s });
    strict_object(json!({
        "title": s,
        "summary": s,
        "keywords": strings,
        "enough_material": { "type": "boolean" },
        "lessons": { "type": "array", "items": strict_object(json!({
            "title": s,
            "angle": s,
            "purpose": s,
            "search": strings,
            "conversation_topic": s,
            "words": { "type": "array", "items": strict_object(json!({ "term": s, "reading": s, "meaning": s })) },
            "grammar": { "type": "array", "items": strict_object(json!({ "pattern": s, "meaning": s })) }
        })) }
    }))
}

pub fn fixed_titles_schema() -> Value {
    let s = json!({ "type": "string" });
    strict_object(json!({
        "lessons": { "type": "array", "items": strict_object(json!({
            "number": { "type": "integer" },
            "title": s,
            "angle": s,
            "purpose": s,
            "conversation_topic": s
        })) }
    }))
}

// ── Post-processing (deterministic) ──────────────────────

/// Pick the lessons to keep from the model's list: `n` of them, the first
/// (background) and the last (review) always kept. A list shorter than `n`
/// becomes a short series. Fewer than [`SHORT_SERIES`] lessons is an error.
fn choose_lessons(raw: Vec<RawLesson>, n: usize, enough_material: bool) -> Result<Vec<RawLesson>, CourseError> {
    if raw.len() < SHORT_SERIES as usize {
        return Err(CourseError::Invalid(format!("課程大綱只有 {} 課，至少要 {SHORT_SERIES} 課", raw.len())));
    }
    let n = if raw.len() < n || !enough_material { SHORT_SERIES as usize } else { n };
    if raw.len() == n {
        return Ok(raw);
    }
    // Keep the first n-1 and the last.
    let last = raw.len() - 1;
    Ok(raw.into_iter().enumerate().filter(|(i, _)| *i < n - 1 || *i == last).map(|(_, l)| l).collect())
}

fn norm(term: &str) -> String {
    term.trim().to_lowercase()
}

/// Make every new word appear in at least `min(MIN_RECURRENCE, lessons left
/// from its introduction)` lessons, by adding it to the review list of later
/// lessons. The last lesson of a series is its review, so it gets every
/// word. Spacing: the even-indexed words of a lesson go first to the next
/// lesson, the odd-indexed ones to the one after, so the extra words are
/// spread. Deterministic and idempotent.
pub fn ensure_recurrence(lessons: &mut [LessonPlan]) {
    let n = lessons.len();
    if n == 0 {
        return;
    }
    if lessons[n - 1].role == LessonRole::Review {
        let all: Vec<String> = lessons[..n - 1].iter().flat_map(|l| l.words.iter().map(|w| w.term.clone())).collect();
        for t in all {
            if !lessons[n - 1].uses_word(&t) {
                lessons[n - 1].review.push(t);
            }
        }
    }
    for i in 0..n {
        let terms: Vec<String> = lessons[i].words.iter().map(|w| w.term.clone()).collect();
        for (k, term) in terms.iter().enumerate() {
            let need = MIN_RECURRENCE.min(n - i);
            let mut have = lessons[i..].iter().filter(|l| l.uses_word(term)).count();
            let mut order: Vec<usize> = ((i + 1)..n).collect();
            if k % 2 == 1 && order.len() >= 2 {
                order.swap(0, 1);
            }
            for j in order {
                if have >= need {
                    break;
                }
                if !lessons[j].uses_word(term) {
                    lessons[j].review.push(term.clone());
                    have += 1;
                }
            }
        }
    }
}

/// Count of lessons each new word appears in (for checks and tests).
pub fn recurrence(lessons: &[LessonPlan]) -> Vec<(String, u32, usize)> {
    lessons
        .iter()
        .flat_map(|l| l.words.iter().map(move |w| (w.term.clone(), l.number)))
        .map(|(t, intro)| {
            let c = lessons.iter().filter(|l| l.uses_word(&t)).count();
            (t, intro, c)
        })
        .collect()
}

/// Where a series goes in its course and what it starts from.
#[derive(Debug, Clone)]
pub struct SeriesOptions {
    /// Number of the series' first lesson (continues an extended course).
    pub first_number: u32,
    /// 1 for the first series, 2 for the first extension…
    pub series: u32,
    /// Words of an earlier series reviewed in the first and last lesson.
    pub carried: Vec<String>,
    pub keywords: Vec<String>,
    /// The learner's own texts, for the article lessons in order.
    pub sources: Vec<SourceText>,
}

impl Default for SeriesOptions {
    fn default() -> Self {
        SeriesOptions { first_number: 1, series: 1, carried: vec![], keywords: vec![], sources: vec![] }
    }
}

/// Turn the model's outline into a valid series: exactly `n` (or 3)
/// lessons, roles by position, new words only in the lessons that
/// [`introduces_words`], unique, unknown to the learner and within the
/// level's limit, grammar within its limit, recurrence ensured, the
/// learner's sources on the article lessons, unit plans by lesson length.
pub fn fix_topic_outline(raw: &RawOutline, n: u32, size: &ContentSize, learner: &Learner, opt: &SeriesOptions) -> Result<Vec<LessonPlan>, CourseError> {
    let (first_number, carried, keywords) = (opt.first_number, &opt.carried, &opt.keywords);
    let mut sources = opt.sources.iter();
    let chosen = choose_lessons(raw.lessons.clone(), n as usize, raw.enough_material)?;
    let n = chosen.len();
    let known: Vec<String> = learner.known_words.iter().map(|k| norm(k)).collect();
    let mut seen: Vec<String> = carried.iter().map(|c| norm(c)).collect();
    let mut grammar_seen: Vec<TargetGrammar> = Vec::new();
    let angle_list = angles(n);
    let mut out: Vec<LessonPlan> = Vec::new();
    for (i, r) in chosen.into_iter().enumerate() {
        let role = topic_role(i, n);
        let number = first_number + i as u32;
        let mut words: Vec<TargetWord> = Vec::new();
        if introduces_words(i, n) {
            for w in r.words {
                let t = norm(&w.term);
                if t.is_empty() || known.contains(&t) || seen.contains(&t) {
                    continue;
                }
                if words.len() < size.topic_new_words.max as usize {
                    seen.push(t);
                    words.push(TargetWord { term: w.term.trim().to_string(), reading: w.reading.trim().into(), meaning: w.meaning.trim().into() });
                }
            }
        }
        let grammar: Vec<TargetGrammar> = if role == LessonRole::Review {
            grammar_seen.clone()
        } else {
            let mut g: Vec<TargetGrammar> = Vec::new();
            for x in r.grammar {
                if !x.pattern.trim().is_empty() && !grammar_seen.iter().any(|s| s.pattern == x.pattern) && g.len() < size.grammar.max as usize {
                    g.push(x);
                }
            }
            grammar_seen.extend(g.iter().cloned());
            g
        };
        let angle = if !r.angle.trim().is_empty() {
            r.angle
        } else if role == LessonRole::Article {
            angle_list.get(i - 1).copied().unwrap_or("").to_string()
        } else {
            String::new()
        };
        let units = unit_plan(role, learner.lesson_minutes(), number);
        let title = if r.title.trim().is_empty() { format!("第 {number} 課") } else { r.title };
        let article = if role == LessonRole::Article { sources.next().cloned() } else { None };
        out.push(LessonPlan {
            number,
            role,
            conversation_topic: if r.conversation_topic.trim().is_empty() { title.clone() } else { r.conversation_topic },
            title,
            angle,
            purpose: r.purpose,
            search: if r.search.is_empty() { keywords.to_vec() } else { r.search },
            series: opt.series,
            article,
            minutes: total_minutes(&units),
            words,
            grammar,
            units,
            ..Default::default()
        });
    }
    if let (Some(first), false) = (out.first_mut(), carried.is_empty()) {
        first.review.extend(carried.iter().cloned());
    }
    if let (Some(last), false) = (out.last_mut(), carried.is_empty()) {
        for c in carried.iter() {
            if !last.uses_word(c) {
                last.review.push(c.clone());
            }
        }
    }
    ensure_recurrence(&mut out);
    Ok(out)
}

// ── Requests ─────────────────────────────────────────────

fn native(code: &str) -> &str {
    crate::lesson::native_name(code)
}

fn base_context(learner: &Learner) -> Value {
    let lvl = learner.level();
    let size = learner.size();
    json!({
        "language": learner.lang.english_name(),
        "lang": learner.lang.code(),
        "level": lvl.label,
        "level_description": level::describe(learner.lang, &lvl),
        "native_language": native(&learner.native_language),
        "topic_words_min": size.topic_new_words.min,
        "topic_words_max": size.topic_new_words.max,
        "grammar_min": size.grammar.min,
        "grammar_max": size.grammar.max,
        "text_length": size.text_description(),
        "known_words": learner.known_words.iter().take(300).collect::<Vec<_>>(),
    })
}

/// The outline request for a topic series.
pub fn topic_request(
    t: &CourseTemplates,
    learner: &Learner,
    keywords: &[String],
    interests: &[String],
    req: &TopicRequest,
    n: u32,
    previous_words: &[String],
) -> Result<CompletionRequest, CourseError> {
    let mut ctx = base_context(learner);
    let mut given = given_sources(req);
    given.truncate((n as usize).saturating_sub(2));
    let per_source = MAX_SOURCE_CHARS / given.len().max(1);
    let sources: Vec<Value> = given
        .iter()
        .map(|s| json!({ "title": s.title, "url": s.url, "text": s.text.chars().take(per_source).collect::<String>() }))
        .collect();
    let word_lessons = (0..n as usize).filter(|&i| introduces_words(i, n as usize)).count();
    let extra = json!({
        "kind": "topic",
        "lessons": n,
        "word_lessons": word_lessons,
        "angles": angles(n as usize),
        "keywords": keywords,
        "interests": interests,
        "url": req.url,
        "sources": sources,
        "previous_words": previous_words,
    });
    if let (Some(c), Some(e)) = (ctx.as_object_mut(), extra.as_object()) {
        c.extend(e.clone());
    }
    Ok(CompletionRequest {
        system: t.render(OUTLINE_SYSTEM, ctx.clone())?,
        user: t.render(OUTLINE_USER, ctx)?,
        max_tokens: 12_000,
        json_schema: Some(topic_outline_schema()),
    })
}

/// Create a topic course outline (not saved; see [`super::store::save_new`]).
/// Keywords come from the request; when it has none (and no URL or text),
/// from the learner's heaviest interests.
pub async fn create_topic_course(
    provider: &dyn LlmProvider,
    t: &CourseTemplates,
    learner: &Learner,
    req: &TopicRequest,
    interests: &[Interest],
    date: &str,
) -> Result<Course, CourseError> {
    let own: Vec<String> = req.keywords.iter().map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect();
    let sources = given_sources(req);
    let has_source = !req.url.trim().is_empty() || !sources.is_empty();
    let mine = interests_for(interests, learner.lang);
    let keywords: Vec<String> = if !own.is_empty() || has_source { own } else { mine.iter().take(3).map(|i| i.keyword.clone()).collect() };
    if keywords.is_empty() && !has_source {
        return Err(CourseError::Invalid("請輸入關鍵字、網址或照片文字，或先在興趣裡加入關鍵字".into()));
    }
    let interest_words: Vec<String> = mine.iter().take(10).map(|i| i.keyword.clone()).collect();
    let n = series_length(req);
    let request = topic_request(t, learner, &keywords, &interest_words, req, n, &[])?;
    let raw: RawOutline = parse_json(&provider.complete(&request).await?)?;
    let keywords = if keywords.is_empty() { raw.keywords.clone() } else { keywords };
    let opt = SeriesOptions { keywords: keywords.clone(), sources: sources.clone(), ..Default::default() };
    let lessons = fix_topic_outline(&raw, n, &learner.size(), learner, &opt)?;
    let name = [req.name.trim(), raw.title.trim(), keywords.first().map(String::as_str).unwrap_or("")]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or("主題課程")
        .to_string();
    Ok(Course {
        id: String::new(),
        kind: CourseKind::Topic,
        name,
        lang: learner.lang,
        level: learner.level().label,
        created: date.to_string(),
        status: CourseStatus::Active,
        daily_minutes: learner.daily_minutes,
        summary: raw.summary,
        keywords,
        source_url: if req.url.trim().is_empty() { sources.first().map(|s| s.url.clone()).unwrap_or_default() } else { req.url.trim().to_string() },
        vocab: vec![],
        grammar: vec![],
        new_per_lesson: 0,
        review_every: 0,
        series: 1,
        engine: provider.id(),
        lessons,
    })
}

/// Extend a topic course by another series (default 5 lessons) on the same
/// keywords. Words of the earlier series that appeared in fewer than
/// [`MIN_RECURRENCE`] lessons are carried into the new series as review.
/// The new lessons are appended to `course.lessons`; save them with
/// [`super::store::append_lessons`].
pub async fn extend_topic_course(
    provider: &dyn LlmProvider,
    t: &CourseTemplates,
    learner: &Learner,
    course: &mut Course,
    req: &TopicRequest,
) -> Result<(), CourseError> {
    if course.kind != CourseKind::Topic {
        return Err(CourseError::Invalid("只有主題課程可以延伸".into()));
    }
    let n = series_length(req);
    let previous: Vec<String> = course.all_words().iter().map(|w| w.term.clone()).collect();
    let carried: Vec<String> = recurrence(&course.lessons).into_iter().filter(|(_, _, c)| *c < MIN_RECURRENCE).map(|(t, _, _)| t).collect();
    let mut inner = learner.clone();
    inner.known_words.extend(previous.iter().cloned());
    let mut r = req.clone();
    if r.url.is_empty() {
        r.url = course.source_url.clone();
    }
    let request = topic_request(t, &inner, &course.keywords, &[], &r, n, &previous)?;
    let raw: RawOutline = parse_json(&provider.complete(&request).await?)?;
    let first = course.lessons.iter().map(|l| l.number).max().unwrap_or(0) + 1;
    let series = course.series.max(1) + 1;
    let opt = SeriesOptions { first_number: first, series, carried, keywords: course.keywords.clone(), sources: given_sources(req) };
    let lessons = fix_topic_outline(&raw, n, &learner.size(), &inner, &opt)?;
    course.lessons.extend(lessons);
    course.series = series;
    course.status = CourseStatus::Active;
    Ok(())
}

/// What a fixed course is made from.
#[derive(Debug, Clone, Default)]
pub struct FixedRequest {
    pub name: String,
    pub words: Vec<TargetWord>,
    pub grammar: Vec<TargetGrammar>,
    /// Workspace-relative files the lists came from (stored in the course).
    pub vocab_files: Vec<String>,
    pub grammar_files: Vec<String>,
    /// 0 = from the level table.
    pub new_per_lesson: u32,
    /// `None` = every 5th lesson; `Some(0)` = no review lessons.
    pub review_every: Option<u32>,
}

/// Create a fixed course outline by splitting the lists; no model needed.
pub fn create_fixed_course(learner: &Learner, req: &FixedRequest, date: &str) -> Result<Course, CourseError> {
    let size = learner.size();
    let review_every = req.review_every.unwrap_or(fixed::REVIEW_EVERY);
    let opt = SplitOptions { new_per_lesson: req.new_per_lesson, review_every, daily_minutes: learner.lesson_minutes(), known: learner.known_words.clone() };
    let lessons = fixed::split(&req.words, &req.grammar, &size, &opt);
    if lessons.is_empty() {
        return Err(CourseError::Invalid("單字表與文法表都是空的（或都已經學過）".into()));
    }
    Ok(Course {
        id: String::new(),
        kind: CourseKind::Fixed,
        name: if req.name.trim().is_empty() { "固定課程".into() } else { req.name.trim().into() },
        lang: learner.lang,
        level: learner.level().label,
        created: date.to_string(),
        status: CourseStatus::Active,
        daily_minutes: learner.daily_minutes,
        summary: String::new(),
        keywords: vec![],
        source_url: String::new(),
        vocab: req.vocab_files.clone(),
        grammar: req.grammar_files.clone(),
        new_per_lesson: if req.new_per_lesson == 0 { size.fixed_new_words } else { req.new_per_lesson },
        review_every,
        series: 1,
        engine: String::new(),
        lessons,
    })
}

#[derive(Debug, Deserialize)]
struct RawTitles {
    #[serde(default)]
    lessons: Vec<RawTitle>,
}

#[derive(Debug, Deserialize)]
struct RawTitle {
    number: u32,
    #[serde(default)]
    title: String,
    #[serde(default)]
    angle: String,
    #[serde(default)]
    purpose: String,
    #[serde(default)]
    conversation_topic: String,
}

/// Optional: let the model give the fixed course's lessons titles, angles
/// and conversation topics (one call). Lessons it leaves out keep their
/// plain titles; words, grammar and units are never changed.
pub async fn describe_fixed(provider: &dyn LlmProvider, t: &CourseTemplates, learner: &Learner, course: &mut Course) -> Result<usize, CourseError> {
    let mut ctx = base_context(learner);
    let lessons: Vec<Value> = course
        .lessons
        .iter()
        .map(|l| {
            json!({
                "number": l.number,
                "role": if l.role == LessonRole::Review { "review" } else { "regular" },
                "words": l.words.iter().map(|w| w.term.clone()).collect::<Vec<_>>(),
                "grammar": l.grammar.iter().map(|g| g.pattern.clone()).collect::<Vec<_>>(),
            })
        })
        .collect();
    if let Some(c) = ctx.as_object_mut() {
        c.insert("kind".into(), json!("fixed"));
        c.insert("name".into(), json!(course.name));
        c.insert("plan".into(), json!(lessons));
        c.insert("lessons".into(), json!(course.lessons.len()));
    }
    let request = CompletionRequest {
        system: t.render(OUTLINE_SYSTEM, ctx.clone())?,
        user: t.render(OUTLINE_USER, ctx)?,
        max_tokens: 12_000,
        json_schema: Some(fixed_titles_schema()),
    };
    let raw: RawTitles = parse_json(&provider.complete(&request).await?)?;
    let mut n = 0;
    for r in raw.lessons {
        if let Some(l) = course.lesson_mut(r.number) {
            if !r.title.trim().is_empty() {
                l.title = format!("第 {} 課：{}", l.number, r.title.trim());
                n += 1;
            }
            if !r.angle.trim().is_empty() {
                l.angle = r.angle;
            }
            if !r.purpose.trim().is_empty() {
                l.purpose = r.purpose;
            }
            if !r.conversation_topic.trim().is_empty() {
                l.conversation_topic = r.conversation_topic;
            }
        }
    }
    course.engine = provider.id();
    Ok(n)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::provider::MockProvider;

    pub fn learner(lang: Lang) -> Learner {
        let difficulty = if lang == Lang::En { 220 } else { 10 };
        Learner { lang, native_language: "zh-TW".into(), difficulty, daily_minutes: 30, known_words: vec!["chip".into()] }
    }

    fn w(t: &str) -> Value {
        json!({ "term": t, "reading": "", "meaning": format!("{t} 的意思") })
    }

    /// A model outline where words never reappear (the fixer must do it).
    pub fn sample_outline(lessons: usize) -> String {
        let ls: Vec<Value> = (0..lessons)
            .map(|i| {
                let words: Vec<Value> = (0..8).map(|k| w(&format!("term{i}{k}"))).collect();
                json!({
                    "title": format!("Lesson {}", i + 1),
                    "angle": "",
                    "purpose": "了解主題",
                    "search": ["AI chips"],
                    "conversation_topic": "",
                    "words": words,
                    "grammar": [{ "pattern": format!("pattern {i}"), "meaning": "" }, { "pattern": format!("pattern {i}b"), "meaning": "" }, { "pattern": "extra", "meaning": "" }]
                })
            })
            .collect();
        json!({ "title": "AI Chips", "summary": "一週認識 AI 晶片", "keywords": ["AI chips"], "enough_material": true, "lessons": ls }).to_string()
    }

    #[test]
    fn series_length_and_roles() {
        assert_eq!(series_length(&TopicRequest::default()), 5);
        assert_eq!(series_length(&TopicRequest { available_articles: Some(2), ..Default::default() }), 3);
        assert_eq!(series_length(&TopicRequest { available_articles: Some(9), ..Default::default() }), 5);
        assert_eq!(series_length(&TopicRequest { lessons: Some(1), ..Default::default() }), 3);
        let roles: Vec<LessonRole> = (0..5).map(|i| topic_role(i, 5)).collect();
        assert_eq!(roles, vec![LessonRole::Background, LessonRole::Article, LessonRole::Article, LessonRole::Article, LessonRole::Review]);
        assert_eq!(angles(5), vec!["technical", "social", "personal life"]);
        assert_eq!(angles(3), vec!["technical"]);
        assert_eq!((0..5).map(|i| introduces_words(i, 5)).collect::<Vec<_>>(), vec![true, true, true, false, false]);
        assert_eq!((0..3).map(|i| introduces_words(i, 3)).collect::<Vec<_>>(), vec![true, true, false]);
        assert_eq!((0..7).filter(|&i| introduces_words(i, 7)).count(), 5);
    }

    #[test]
    fn fixes_outline_shape_and_recurrence() {
        let mut raw: RawOutline = parse_json(&sample_outline(6)).unwrap();
        // A duplicate across lessons and a known word.
        raw.lessons[2].words.push(serde_json::from_value(w("term00")).unwrap());
        raw.lessons[1].words.insert(0, serde_json::from_value(w("Chip")).unwrap());
        let l = learner(Lang::En);
        let opt = SeriesOptions { keywords: vec!["AI chips".into()], ..Default::default() };
        let plans = fix_topic_outline(&raw, 5, &l.size(), &l, &opt).unwrap();
        assert_eq!(plans.len(), 5, "exactly 5 lessons (6th dropped, last kept as review)");
        assert_eq!(plans[4].title, "Lesson 6");
        assert_eq!(plans.iter().map(|p| p.number).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5]);
        assert!(plans[4].words.is_empty(), "review lesson has no new words");
        assert!(plans[3].words.is_empty(), "lesson 4 deepens earlier words");
        assert_eq!(plans[3].role, LessonRole::Article);
        assert!(!plans[3].review.is_empty());
        assert!(plans.iter().all(|p| p.series == 1));
        assert!(plans.iter().filter(|p| p.role != LessonRole::Review).all(|p| p.words.len() <= 10 && p.grammar.len() <= 2));
        assert!(!plans[1].words.iter().any(|w| w.term == "Chip"), "known words are dropped");
        assert_eq!(plans[2].words.iter().filter(|w| w.term == "term00").count(), 0, "a word is new only once");
        assert_eq!(plans[1].angle, "technical");
        assert_eq!(plans[3].angle, "personal life");
        assert_eq!(plans[4].grammar.len(), 8, "review lesson practises the series' grammar");
        assert!(plans.iter().all(|p| p.minutes == 30 && p.units.len() == 4));
        let rec = recurrence(&plans);
        assert_eq!(rec.len(), 24, "8 words in each of lessons 1–3");
        for (term, intro, count) in rec {
            assert!(count >= MIN_RECURRENCE, "{term} from lesson {intro} appears in {count} lessons");
        }
        // Lesson-1 words appear in exactly 3 lessons, spread over lessons 2 and 3.
        assert!(plans[1].review.contains(&"term00".to_string()) && plans[2].review.contains(&"term01".to_string()));
        assert!(!plans[1].review.contains(&"term01".to_string()));
        // Idempotent.
        let mut again = plans.clone();
        ensure_recurrence(&mut again);
        assert_eq!(again, plans);
    }

    #[test]
    fn narrow_topic_becomes_three_lessons() {
        let mut raw: RawOutline = parse_json(&sample_outline(5)).unwrap();
        raw.enough_material = false;
        let l = learner(Lang::Ja);
        let plans = fix_topic_outline(&raw, 5, &l.size(), &l, &SeriesOptions::default()).unwrap();
        assert_eq!(plans.len(), 3);
        assert_eq!(plans[0].title, "Lesson 1");
        assert_eq!(plans[2].title, "Lesson 5", "the review lesson is kept");
        assert!(plans[0].words.len() <= 6, "Japanese N5: at most 6 new words in a topic lesson");
        assert!(plans[0].grammar.len() <= 1);
        assert!(!plans[1].words.is_empty() && plans[2].words.is_empty(), "lessons 1 and 2 introduce words, 3 reviews");
        for (term, intro, count) in recurrence(&plans) {
            // Lesson-1 words: 3 lessons; lesson-2 words can only reach 2 in a short series.
            assert_eq!(count, if intro == 1 { 3 } else { 2 }, "{term}");
        }
        raw.lessons.truncate(2);
        assert!(fix_topic_outline(&raw, 5, &l.size(), &l, &SeriesOptions::default()).is_err());
    }

    #[tokio::test]
    async fn creates_and_extends_topic_course() {
        let p = MockProvider::new(vec![sample_outline(5), sample_outline(5)]);
        let t = CourseTemplates::bundled();
        let l = learner(Lang::En);
        let interests = super::super::topics::parse_interests("keyword,lang,weight,updated_at\nAI chips,en,2,\n");
        assert!(create_topic_course(&p, &t, &l, &TopicRequest::default(), &[], "2026-11-05").await.is_err());
        let mut c = create_topic_course(&p, &t, &l, &TopicRequest::default(), &interests, "2026-11-05").await.unwrap();
        assert_eq!(c.keywords, vec!["AI chips"], "keywords from interests when the request has none");
        assert_eq!(c.name, "AI Chips");
        assert_eq!(c.lessons.len(), 5);
        assert_eq!(c.level, "B1-1");
        let seen = p.seen.lock().unwrap()[0].clone();
        assert!(seen.user.contains("AI chips") && seen.user.contains("5"));
        assert!(seen.system.contains("B1"));
        assert!(seen.json_schema.is_some());
        assert!(seen.user.contains("technical"), "angles are in the prompt:\n{}", seen.user);

        for lp in c.lessons.iter_mut() {
            lp.status = LessonStatus::Done;
        }
        assert!(c.can_extend());
        extend_topic_course(&p, &t, &l, &mut c, &TopicRequest::default()).await.unwrap();
        assert_eq!(c.lessons.len(), 10);
        assert_eq!(c.series, 2);
        assert_eq!(c.lessons[5].number, 6);
        assert!(c.lessons[5..].iter().all(|l| l.series == 2));
        // The same words from the mock are known now, so the new series has no new words.
        assert!(c.lessons[5..].iter().all(|l| l.words.is_empty()));
        // Every first-series word reached 3 lessons, so nothing had to be carried.
        assert!(recurrence(&c.lessons[..5]).iter().all(|(_, _, n)| *n >= MIN_RECURRENCE));
        assert!(c.lessons[5].review.is_empty());
        assert!(p.seen.lock().unwrap()[1].user.contains("term00"), "previous words are in the prompt");
    }

    #[tokio::test]
    async fn learner_sources_become_the_article_lessons() {
        let p = MockProvider::new(vec![sample_outline(5)]);
        let src = |t: &str, u: &str| SourceText { title: t.into(), url: u.into(), source: String::new(), text: format!("{t} body text.") };
        let req = TopicRequest {
            keywords: vec!["AI chips".into()],
            sources: vec![src("Fab news", "https://example.com/fab"), src("Photo of a poster", ""), src("", "https://example.com/empty")],
            ..Default::default()
        };
        // The third source has text too; one without text is left out.
        let mut req2 = req.clone();
        req2.sources[2].text = String::new();
        let c = create_topic_course(&p, &CourseTemplates::bundled(), &learner(Lang::En), &req2, &[], "2026-11-05").await.unwrap();
        assert_eq!(c.lessons[1].article.as_ref().unwrap().url, "https://example.com/fab");
        assert_eq!(c.lessons[2].article.as_ref().unwrap().title, "Photo of a poster");
        assert!(c.lessons[3].article.is_none(), "lesson 4 looks for another angle");
        assert!(c.lessons[0].article.is_none() && c.lessons[4].article.is_none());
        assert_eq!(c.source_url, "https://example.com/fab");
        let seen = p.seen.lock().unwrap()[0].clone();
        assert!(seen.user.contains("Fab news body text.") && seen.user.contains("<source lesson=\"3\""), "{}", seen.user);
        assert!(seen.system.contains("lesson 2"));
        assert!(!plan_needs_search(&c, 2) && !plan_needs_search(&c, 3) && plan_needs_search(&c, 4));

        // A single page given as url + text (the older fields) goes to lesson 2.
        let p = MockProvider::new(vec![sample_outline(5)]);
        let one = TopicRequest { url: "https://example.com/a".into(), source_text: "Page text.".into(), ..Default::default() };
        let c = create_topic_course(&p, &CourseTemplates::bundled(), &learner(Lang::En), &one, &[], "2026-11-05").await.unwrap();
        assert_eq!(c.lessons[1].article.as_ref().unwrap().text, "Page text.");
        assert!(c.lessons[2].article.is_none());
    }

    fn plan_needs_search(c: &Course, n: u32) -> bool {
        super::super::plan::needs_search(c.lesson(n).unwrap())
    }

    #[tokio::test]
    async fn fixed_course_split_and_titles() {
        let l = learner(Lang::En);
        let words: Vec<TargetWord> = (0..25).map(|i| TargetWord { term: format!("w{i}"), ..Default::default() }).collect();
        let mut c = create_fixed_course(&l, &FixedRequest { name: "TOEIC".into(), words, ..Default::default() }, "2026-11-05").unwrap();
        assert_eq!(c.lessons.iter().filter(|l| l.role == LessonRole::Regular).count(), 3);
        assert_eq!(c.lessons.len(), 4, "3 word lessons and a final review");
        assert_eq!(c.new_per_lesson, 10);
        let reply = json!({ "lessons": [{ "number": 1, "title": "Office life", "angle": "work", "purpose": "職場用語", "conversation_topic": "Your first day" }, { "number": 99, "title": "x", "angle": "", "purpose": "", "conversation_topic": "" }] });
        let p = MockProvider::new(vec![reply.to_string()]);
        let n = describe_fixed(&p, &CourseTemplates::bundled(), &l, &mut c).await.unwrap();
        assert_eq!(n, 1);
        assert_eq!(c.lessons[0].title, "第 1 課：Office life");
        assert_eq!(c.lessons[1].title, "第 2 課：w10、w11、w12");
        assert!(p.seen.lock().unwrap()[0].user.contains("w10"));
        assert!(create_fixed_course(&l, &FixedRequest::default(), "2026-11-05").is_err());
    }
}
