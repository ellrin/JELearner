//! Lesson plan → lesson content. Runs when the learner opens a lesson, so
//! the news is fresh and the difficulty follows recent results.
//!
//! The reply is a superset of [`crate::lesson::Lesson`] (same vocabulary,
//! grammar and questions, plus the text, output exercises and a
//! conversation plan). The Markdown goes to `lessons/<lang>/` like every
//! other lesson; its front matter adds `course`, `course_lesson`,
//! `course_role` and `units`, and its embedded data still reads as a
//! [`Lesson`], so the database import of lessons works unchanged.

use super::model::*;
use super::prompts::{LESSON_MARKDOWN, LESSON_SYSTEM, LESSON_USER};
use super::{store, strict_object, CourseError, CourseTemplates, Learner};
use crate::lang::Lang;
use crate::lesson::{self, Lesson, LessonError, VocabItem, MAX_ARTICLE_CHARS};
use crate::level;
use crate::provider::{CompletionRequest, LlmProvider};
use crate::workspace::Workspace;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub use super::model::SourceText;

/// What is known when the lesson is opened.
#[derive(Debug, Clone, Default)]
pub struct LessonContext {
    /// Average accuracy of the last units (0–1), to adapt the difficulty.
    pub recent_accuracy: Option<f32>,
    /// A real article containing the lesson's words, when one was found
    /// (see [`pick_article`]); otherwise the model writes a short text.
    /// The lesson's own [`LessonPlan::article`] (a text the learner gave)
    /// wins over this; see [`needs_search`].
    pub article: Option<SourceText>,
    /// The learner's focus theme (plan §19.2.6): the text is set there and
    /// mentions real names from its setting sheet.
    pub focus: Option<super::picker::FocusSheet>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputItem {
    /// `translation` | `sentence-making` | `cloze` | `paraphrase`
    pub kind: String,
    /// What the learner sees (for translation: a sentence in their language).
    pub prompt: String,
    /// A model answer.
    pub answer: String,
    /// Words or grammar to use, or another hint.
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ConversationPlan {
    pub topic: String,
    /// The partner's first line.
    pub opening: String,
    pub target_words: Vec<String>,
}

/// Where a generated lesson belongs (added by us, not by the model).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CourseRef {
    pub id: String,
    pub number: u32,
    pub role: LessonRole,
    pub units: Vec<UnitPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourseLesson {
    #[serde(flatten)]
    pub lesson: Lesson,
    /// Today's text, in the target language. Japanese at N5–N4 marks
    /// readings as `{漢字|かんじ}`.
    pub text: String,
    #[serde(default)]
    pub output: Vec<OutputItem>,
    #[serde(default)]
    pub conversation: ConversationPlan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub course: Option<CourseRef>,
}

/// The lesson schema plus `text`, `output` and `conversation`.
pub fn course_lesson_schema() -> Value {
    let mut schema = lesson::lesson_schema();
    let s = json!({ "type": "string" });
    let extra = json!({
        "text": s,
        "output": { "type": "array", "items": strict_object(json!({
            "kind": { "type": "string", "enum": ["translation", "sentence-making", "cloze", "paraphrase"] },
            "prompt": s, "answer": s, "hint": s
        })) },
        "conversation": strict_object(json!({ "topic": s, "opening": s, "target_words": { "type": "array", "items": s } })),
    });
    if let (Some(props), Some(e)) = (schema["properties"].as_object_mut(), extra.as_object()) {
        props.extend(e.clone());
    }
    let required: Vec<String> = schema["properties"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    schema["required"] = json!(required);
    schema
}

// ── Furigana ─────────────────────────────────────────────

/// `{漢字|かんじ}` → `漢字`.
pub fn strip_furigana(text: &str) -> String {
    map_ruby(text, |base, _| base.to_string())
}

/// `{漢字|かんじ}` → `<ruby>漢字<rt>かんじ</rt></ruby>`.
pub fn ruby_html(text: &str) -> String {
    map_ruby(text, |base, reading| format!("<ruby>{base}<rt>{reading}</rt></ruby>"))
}

/// Like [`ruby_html`] for text that goes into HTML: `&`, `<` and `>` are
/// escaped first (readings and bases too), then `{漢字|かんじ}` becomes ruby.
pub fn ruby_html_escaped(text: &str) -> String {
    let esc = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    ruby_html(&esc)
}

fn map_ruby(text: &str, f: impl Fn(&str, &str) -> String) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) if after[..end].contains('|') && !after[..end].contains('{') => {
                let (base, reading) = after[..end].split_once('|').unwrap_or((&after[..end], ""));
                out.push_str(&f(base, reading));
                rest = &after[end + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

// ── Checks ───────────────────────────────────────────────

fn contains_term(text_lower: &str, term: &str, lang: Lang) -> bool {
    let t = term.trim().to_lowercase();
    if t.is_empty() {
        return true;
    }
    if text_lower.contains(&t) {
        return true;
    }
    // English inflections: "resilient" also matches "resilience",
    // "leverage" matches "leveraging".
    if lang == Lang::En && !t.contains(' ') && t.chars().count() >= 5 {
        let stem: String = t.chars().take(t.chars().count() - 1).collect();
        return text_lower.contains(&stem);
    }
    false
}

/// Target words that do not appear in the text.
pub fn missing_words(text: &str, terms: &[String], lang: Lang) -> Vec<String> {
    let plain = strip_furigana(text).to_lowercase();
    terms.iter().filter(|t| !contains_term(&plain, t, lang)).cloned().collect()
}

/// The candidate article that contains the most target words, when it has
/// at least 2 of them and at least 30 % (fixed courses: texts come from
/// real news when there is a fitting one).
pub fn pick_article(candidates: &[SourceText], terms: &[String], lang: Lang) -> Option<usize> {
    let need = 2.max((terms.len() * 3).div_ceil(10));
    candidates
        .iter()
        .enumerate()
        .map(|(i, a)| (i, terms.len() - missing_words(&a.text, terms, lang).len()))
        .filter(|(_, hits)| *hits >= need && !terms.is_empty())
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(i, _)| i)
}

/// Should the server look for a news article before opening this lesson?
/// Not when the learner gave the lesson's text, and not for review or
/// conversation lessons.
pub fn needs_search(plan: &LessonPlan) -> bool {
    plan.article.is_none() && matches!(plan.role, LessonRole::Regular | LessonRole::Background | LessonRole::Article)
}

// ── Request ──────────────────────────────────────────────

fn units_json(units: &[UnitPlan]) -> Vec<Value> {
    units.iter().map(|u| json!({ "kind": u.kind.code(), "minutes": u.minutes, "items": u.items })).collect()
}

/// The generation request for one planned lesson.
pub fn build_request(t: &CourseTemplates, course: &Course, plan: &LessonPlan, learner: &Learner, ctx: &LessonContext) -> Result<CompletionRequest, CourseError> {
    let lvl = learner.level();
    let size = learner.size();
    let questions = plan.units.iter().find(|u| matches!(u.kind, UnitKind::Reading | UnitKind::Quiz)).map(|u| u.items).unwrap_or(0);
    let output: Vec<&UnitPlan> = plan.units.iter().filter(|u| u.kind.is_output()).collect();
    let turns = plan.units.iter().find(|u| u.kind == UnitKind::Conversation).map(|u| u.items).unwrap_or(0);
    let article = match plan.article.as_ref().or(ctx.article.as_ref()) {
        Some(a) => {
            let n = a.text.chars().count();
            if a.text.trim().is_empty() {
                return Err(LessonError::Empty.into());
            }
            if n > MAX_ARTICLE_CHARS {
                return Err(LessonError::TooLong(n).into());
            }
            json!(a)
        }
        None => Value::Null,
    };
    let c = json!({
        "language": learner.lang.english_name(),
        "lang": learner.lang.code(),
        "level": lvl.label,
        "level_description": level::describe(learner.lang, &lvl),
        "native_language": lesson::native_name(&learner.native_language),
        "course_kind": format!("{:?}", course.kind).to_lowercase(),
        "course_name": course.name,
        "keywords": course.keywords,
        "number": plan.number,
        "total": course.lessons.len(),
        "role": format!("{:?}", plan.role).to_lowercase(),
        "title": plan.title,
        "angle": plan.angle,
        "purpose": plan.purpose,
        "conversation_topic": plan.conversation_topic,
        "new_words": plan.words,
        "review_words": plan.review,
        "grammar": plan.grammar,
        "units": units_json(&plan.units),
        "questions": questions,
        "output": units_json(&output.into_iter().cloned().collect::<Vec<_>>()),
        "turns": turns,
        "text_length": size.text_description(),
        "furigana": size.furigana,
        "article": article,
        "recent_accuracy": ctx.recent_accuracy.map(|a| (a.clamp(0.0, 1.0) * 100.0).round() as u32),
        "known_words": learner.known_words.iter().take(200).collect::<Vec<_>>(),
        "setting": ctx.focus,
        "setting_names": ctx.focus.as_ref().map(|f| f.names()).unwrap_or_default(),
    });
    Ok(CompletionRequest {
        system: t.render(LESSON_SYSTEM, c.clone())?,
        user: t.render(LESSON_USER, c)?,
        max_tokens: 16_000,
        json_schema: Some(course_lesson_schema()),
    })
}

/// Parse and check the reply: the [`Lesson`] checks (choice answers among
/// the options, not empty) plus a non-empty text.
pub fn parse_reply(text: &str) -> Result<CourseLesson, CourseError> {
    lesson::parse_lesson(text)?;
    let cl: CourseLesson = super::parse_json(text)?;
    if cl.text.trim().is_empty() {
        return Err(LessonError::Invalid("沒有課文".into()).into());
    }
    Ok(cl)
}

/// Make sure every new word of the plan is in the vocabulary (the model may
/// leave one out; the plan's reading and meaning are used then).
pub fn complete_vocabulary(cl: &mut CourseLesson, plan: &LessonPlan) {
    for w in &plan.words {
        if !cl.lesson.vocabulary.iter().any(|v| v.term.eq_ignore_ascii_case(&w.term)) {
            cl.lesson.vocabulary.push(VocabItem {
                term: w.term.clone(),
                reading: w.reading.clone(),
                part_of_speech: String::new(),
                meaning: w.meaning.clone(),
                example: String::new(),
                example_translation: String::new(),
            });
        }
    }
}

#[derive(Debug, Clone)]
pub struct MarkdownMeta {
    pub lang: Lang,
    pub level: String,
    pub date: String,
    pub engine: String,
}

pub fn render_markdown(t: &CourseTemplates, cl: &CourseLesson, plan: &LessonPlan, course: &Course, meta: &MarkdownMeta, source: Option<&SourceText>) -> Result<String, CourseError> {
    let furigana = cl.text.contains('|') && meta.lang == Lang::Ja;
    let text_html = if furigana { ruby_html(&cl.text) } else { cl.text.clone() };
    let ctx = json!({
        "lesson": cl.lesson,
        "text": text_html,
        "output": cl.output,
        "conversation": cl.conversation,
        "plan": plan,
        "units": plan.units.iter().map(|u| json!({ "kind": u.kind.code(), "minutes": u.minutes, "items": u.items, "skill": u.skill.code() })).collect::<Vec<_>>(),
        "units_summary": plan.units_summary(),
        "role": format!("{:?}", plan.role).to_lowercase(),
        "course_id": course.id,
        "course_name": course.name,
        "total": course.lessons.len(),
        "lang": meta.lang.code(),
        "language_label": meta.lang.label(),
        "level": meta.level,
        "date": meta.date,
        "engine": meta.engine,
        "url": source.map(|s| s.url.clone()).unwrap_or_default(),
        "source": source.map(|s| s.source.clone()).unwrap_or_default(),
    });
    Ok(t.render(LESSON_MARKDOWN, ctx)?)
}

/// A generated lesson, ready to be saved.
#[derive(Debug, Clone)]
pub struct Generated {
    pub lesson: CourseLesson,
    /// Markdown with the embedded data.
    pub markdown: String,
    /// Target words (new and review) the text does not contain.
    pub missing: Vec<String>,
    pub level: String,
}

/// Prompt, call, parse, check, render.
pub async fn generate(
    provider: &dyn LlmProvider,
    t: &CourseTemplates,
    course: &Course,
    number: u32,
    learner: &Learner,
    ctx: &LessonContext,
    date: &str,
) -> Result<Generated, CourseError> {
    let plan = course.lesson(number).ok_or_else(|| CourseError::NotFound(format!("{} 第 {number} 課", course.id)))?;
    let req = build_request(t, course, plan, learner, ctx)?;
    let mut cl = parse_reply(&provider.complete(&req).await?)?;
    complete_vocabulary(&mut cl, plan);
    cl.course = Some(CourseRef { id: course.id.clone(), number, role: plan.role, units: plan.units.clone() });
    let terms: Vec<String> = plan.words.iter().map(|w| w.term.clone()).chain(plan.review.iter().cloned()).collect();
    let missing = missing_words(&cl.text, &terms, learner.lang);
    let level = learner.level().label;
    let meta = MarkdownMeta { lang: learner.lang, level: level.clone(), date: date.to_string(), engine: provider.id() };
    let md = render_markdown(t, &cl, plan, course, &meta, plan.article.as_ref().or(ctx.article.as_ref()))?;
    let markdown = lesson::embed_data(&md, &cl);
    Ok(Generated { lesson: cl, markdown, missing, level })
}

/// The result of opening a lesson.
#[derive(Debug, Clone)]
pub struct Opened {
    pub course: Course,
    /// Workspace-relative path of the lesson file.
    pub file: String,
    pub markdown: String,
    /// Set when the lesson was generated by this call (the server then adds
    /// it to the database); `None` when it already existed.
    pub generated: Option<Generated>,
}

/// Open a lesson of a saved course: generate it on first open (saving the
/// Markdown under a free name and recording `generated` in the progress
/// file; the outline file is not touched), or read the existing file.
#[allow(clippy::too_many_arguments)]
pub async fn open_lesson(
    provider: &dyn LlmProvider,
    t: &CourseTemplates,
    ws: &Workspace,
    course_id: &str,
    number: u32,
    learner: &Learner,
    ctx: &LessonContext,
    date: &str,
) -> Result<Opened, CourseError> {
    let mut course = store::load(ws, course_id)?;
    let plan = course.lesson(number).ok_or_else(|| CourseError::NotFound(format!("{course_id} 第 {number} 課")))?;
    if let Some(file) = plan.file.clone().filter(|_| plan.status != LessonStatus::Planned) {
        let path = ws.root.join(&file);
        if let Ok(markdown) = std::fs::read_to_string(&path) {
            return Ok(Opened { course, file, markdown, generated: None });
        }
        // The file was removed by hand: generate again.
    }
    let g = generate(provider, t, &course, number, learner, ctx, date).await?;
    let file = store::save_lesson_file(ws, &course, number, &g.markdown, date)?;
    if let Some(p) = course.lesson_mut(number) {
        p.status = LessonStatus::Generated;
        p.file = Some(file.clone());
        p.generated = Some(date.to_string());
    }
    store::save_progress(ws, &course)?;
    Ok(Opened { course, file, markdown: g.markdown.clone(), generated: Some(g) })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn furigana_helpers() {
        let t = "{今日|きょう}は{雨|あめ}です。{not ruby}";
        assert_eq!(strip_furigana(t), "今日は雨です。{not ruby}");
        assert_eq!(ruby_html("{雨|あめ}"), "<ruby>雨<rt>あめ</rt></ruby>");
        assert_eq!(missing_words(t, &["今日".into(), "晴れ".into()], Lang::Ja), vec!["晴れ".to_string()]);
        assert_eq!(ruby_html_escaped("<b>{雨|あめ}</b> & {x|y"), "&lt;b&gt;<ruby>雨<rt>あめ</rt></ruby>&lt;/b&gt; &amp; {x|y");
        assert_eq!(ruby_html("{日本語|にほんご}を{勉強|べんきょう}する"), "<ruby>日本語<rt>にほんご</rt></ruby>を<ruby>勉強<rt>べんきょう</rt></ruby>する");
    }

    #[test]
    fn word_checks_and_article_choice() {
        let text = "Supply chains need resilience. Firms are leveraging data.";
        assert!(missing_words(text, &["resilient".into(), "leverage".into(), "supply chain".into()], Lang::En).is_empty());
        assert_eq!(missing_words(text, &["subsidy".into()], Lang::En), vec!["subsidy".to_string()]);
        let a = |s: &str| SourceText { text: s.into(), ..Default::default() };
        let terms: Vec<String> = ["subsidy", "fab", "yield"].iter().map(|s| s.to_string()).collect();
        let c = vec![a("A subsidy only."), a("The subsidy for the new fab improved yield."), a("A fab and a subsidy.")];
        assert_eq!(pick_article(&c, &terms, Lang::En), Some(1));
        assert_eq!(pick_article(&c[..1], &terms, Lang::En), None, "one word is not enough");
    }

    #[test]
    fn schema_extends_lesson_schema() {
        let s = course_lesson_schema();
        let req = s["required"].as_array().unwrap();
        assert_eq!(req.len(), 8);
        assert!(req.contains(&json!("text")) && req.contains(&json!("vocabulary")));
        assert_eq!(s["properties"]["output"]["items"]["additionalProperties"], false);
    }
}
