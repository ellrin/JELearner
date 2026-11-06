//! News article -> structured lesson (vocabulary, grammar, questions) ->
//! Markdown.
//!
//! The model is asked for one JSON object matching [`lesson_schema`]; the
//! Markdown is rendered by us from a template, so the look of a lesson can be
//! changed without touching code or re-running the model. All three
//! templates are files: the bundled ones in `prompts/`, overridable per user
//! by files of the same name in the workspace's `prompts/` folder.

use crate::lang::Lang;
use crate::provider::{CompletionRequest, LlmProvider, ProviderError};
use minijinja::Environment;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

pub const SYSTEM_FILE: &str = "lesson_system.md";
pub const USER_FILE: &str = "lesson_user.md";
pub const MARKDOWN_FILE: &str = "lesson.md.j2";

const BUNDLED_SYSTEM: &str = include_str!("../../../prompts/lesson_system.md");
const BUNDLED_USER: &str = include_str!("../../../prompts/lesson_user.md");
const BUNDLED_MARKDOWN: &str = include_str!("../../../prompts/lesson.md.j2");

/// Longest article we send as is (characters). Longer text is refused with a
/// clear message rather than cut silently.
pub const MAX_ARTICLE_CHARS: usize = 24_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lesson {
    pub title: String,
    /// One-paragraph summary in the learner's language.
    pub summary: String,
    pub vocabulary: Vec<VocabItem>,
    pub grammar: Vec<GrammarItem>,
    pub questions: Vec<Question>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VocabItem {
    pub term: String,
    pub reading: String,
    pub part_of_speech: String,
    pub meaning: String,
    pub example: String,
    pub example_translation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrammarItem {
    pub pattern: String,
    pub meaning: String,
    pub explanation: String,
    pub example: String,
    pub example_translation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    /// `choice` (one of `options`) or `short` (free answer, self-checked).
    pub kind: String,
    pub question: String,
    pub options: Vec<String>,
    pub answer: String,
    pub explanation: String,
}

/// JSON Schema for structured output (all fields required, no extras).
pub fn lesson_schema() -> Value {
    let obj = |props: Value| {
        let required: Vec<String> = props.as_object().unwrap().keys().cloned().collect();
        json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
    };
    let s = json!({ "type": "string" });
    obj(json!({
        "title": s,
        "summary": s,
        "vocabulary": { "type": "array", "items": obj(json!({
            "term": s, "reading": s, "part_of_speech": s, "meaning": s, "example": s, "example_translation": s
        })) },
        "grammar": { "type": "array", "items": obj(json!({
            "pattern": s, "meaning": s, "explanation": s, "example": s, "example_translation": s
        })) },
        "questions": { "type": "array", "items": obj(json!({
            "kind": { "type": "string", "enum": ["choice", "short"] },
            "question": s,
            "options": { "type": "array", "items": s },
            "answer": s,
            "explanation": s
        })) }
    }))
}

#[derive(Debug, Clone)]
pub struct Templates {
    pub system: String,
    pub user: String,
    pub markdown: String,
    /// Which files came from the workspace (for the UI).
    pub overridden: Vec<String>,
}

impl Templates {
    pub fn bundled() -> Templates {
        Templates { system: BUNDLED_SYSTEM.into(), user: BUNDLED_USER.into(), markdown: BUNDLED_MARKDOWN.into(), overridden: vec![] }
    }

    /// Bundled templates, each replaced by the workspace's file when present.
    pub fn load(workspace_prompts: Option<&Path>) -> Templates {
        let mut t = Templates::bundled();
        if let Some(dir) = workspace_prompts {
            for (name, slot) in [(SYSTEM_FILE, &mut t.system), (USER_FILE, &mut t.user), (MARKDOWN_FILE, &mut t.markdown)] {
                if let Ok(s) = std::fs::read_to_string(dir.join(name))
                    && !s.trim().is_empty() {
                        *slot = s;
                        t.overridden.push(name.to_string());
                    }
            }
        }
        t
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LessonInput {
    pub lang: Lang,
    pub level: String,
    pub native_language: String,
    pub title: String,
    pub url: String,
    pub source: String,
    pub text: String,
    /// Words the learner already has, so the model can skip them.
    pub known_words: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LessonError {
    #[error("文章是空的，沒有可以做成教材的內容")]
    Empty,
    #[error("文章太長（{0} 字，上限 {MAX_ARTICLE_CHARS} 字）。請貼上想學的段落")]
    TooLong(usize),
    #[error("模板錯誤（{0}）：{1}")]
    Template(String, String),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("教材格式不符：{0}")]
    Invalid(String),
}

pub(crate) fn native_name(code: &str) -> &str {
    match code {
        "zh-TW" | "zh-Hant" => "Traditional Chinese (Taiwan)",
        "zh-CN" | "zh-Hans" => "Simplified Chinese",
        "ja" => "Japanese",
        "en" => "English",
        other => other,
    }
}

pub(crate) fn render(name: &str, template: &str, ctx: Value) -> Result<String, LessonError> {
    let mut env = Environment::new();
    env.set_trim_blocks(true);
    env.set_lstrip_blocks(true);
    env.add_template(name, template).map_err(|e| LessonError::Template(name.into(), e.to_string()))?;
    env.get_template(name)
        .and_then(|t| t.render(ctx))
        .map_err(|e| LessonError::Template(name.into(), e.to_string()))
}

pub fn build_request(t: &Templates, input: &LessonInput) -> Result<CompletionRequest, LessonError> {
    let n = input.text.chars().count();
    if input.text.trim().is_empty() {
        return Err(LessonError::Empty);
    }
    if n > MAX_ARTICLE_CHARS {
        return Err(LessonError::TooLong(n));
    }
    let ctx = json!({
        "language": input.lang.english_name(),
        "lang": input.lang.code(),
        "level": input.level,
        "native_language": native_name(&input.native_language),
        "title": input.title,
        "url": input.url,
        "source": input.source,
        "text": input.text,
        "known_words": input.known_words,
    });
    Ok(CompletionRequest {
        system: render(SYSTEM_FILE, &t.system, ctx.clone())?,
        user: render(USER_FILE, &t.user, ctx)?,
        max_tokens: 16_000,
        json_schema: Some(lesson_schema()),
    })
}

/// Parse the model's reply. Tolerates a ```json fence (local models).
pub fn parse_lesson(text: &str) -> Result<Lesson, LessonError> {
    let t = text.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    let lesson: Lesson = serde_json::from_str(t).map_err(|e| LessonError::Invalid(e.to_string()))?;
    if lesson.vocabulary.is_empty() && lesson.grammar.is_empty() {
        return Err(LessonError::Invalid("沒有單字也沒有文法".into()));
    }
    for (i, q) in lesson.questions.iter().enumerate() {
        if q.kind == "choice" && !q.options.iter().any(|o| o == &q.answer) {
            return Err(LessonError::Invalid(format!("第 {} 題的答案不在選項中", i + 1)));
        }
    }
    Ok(lesson)
}

#[derive(Debug, Clone, Serialize)]
pub struct MarkdownMeta {
    pub lang: Lang,
    pub level: String,
    pub url: String,
    pub source: String,
    pub date: String,
    pub engine: String,
}

pub fn render_markdown(t: &Templates, lesson: &Lesson, meta: &MarkdownMeta) -> Result<String, LessonError> {
    let ctx = json!({
        "lesson": lesson,
        "lang": meta.lang.code(),
        "language_label": meta.lang.label(),
        "level": meta.level,
        "url": meta.url,
        "source": meta.source,
        "date": meta.date,
        "engine": meta.engine,
    });
    render(MARKDOWN_FILE, &t.markdown, ctx)
}

/// The whole step: prompt, call, parse, render.
pub async fn generate(
    provider: &dyn LlmProvider,
    t: &Templates,
    input: &LessonInput,
    date: &str,
) -> Result<(Lesson, String), LessonError> {
    let req = build_request(t, input)?;
    let reply = provider.complete(&req).await?;
    let lesson = parse_lesson(&reply)?;
    let meta = MarkdownMeta {
        lang: input.lang,
        level: input.level.clone(),
        url: input.url.clone(),
        source: input.source.clone(),
        date: date.to_string(),
        engine: provider.id(),
    };
    let md = render_markdown(t, &lesson, &meta)?;
    Ok((lesson, md))
}

const DATA_MARK: &str = "<!-- jelearner:lesson ";

/// Append the lesson's data to its Markdown as an HTML comment (invisible
/// when rendered), so the database can be rebuilt from the workspace files
/// on another machine. `data` is a [`Lesson`] or a superset of it (a course
/// lesson adds fields; [`extract_data`] ignores what it does not know).
pub fn embed_data<T: Serialize>(markdown: &str, data: &T) -> String {
    let json = serde_json::to_string(data).unwrap_or_default().replace("-->", "--\\u003e");
    format!("{}\n\n{DATA_MARK}{json} -->\n", markdown.trim_end())
}

/// The embedded data as JSON, with every field (a course lesson's text,
/// output exercises and conversation plan too).
pub fn extract_raw(markdown: &str) -> Option<serde_json::Value> {
    let start = markdown.rfind(DATA_MARK)? + DATA_MARK.len();
    let end = markdown[start..].find(" -->")? + start;
    serde_json::from_str(&markdown[start..end]).ok()
}

pub fn extract_data(markdown: &str) -> Option<Lesson> {
    let start = markdown.rfind(DATA_MARK)? + DATA_MARK.len();
    let end = markdown[start..].find(" -->")? + start;
    serde_json::from_str(&markdown[start..end]).ok()
}

/// A `key: value` line from the YAML front matter (no YAML parser needed for
/// the few scalar fields we write).
pub fn front_matter(markdown: &str, key: &str) -> Option<String> {
    let body = markdown.strip_prefix("---")?;
    let end = body.find("\n---")?;
    body[..end].lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

/// Grade a choice question; short answers compare loosely (case, spaces,
/// full-width punctuation) and otherwise are left to the learner.
pub fn grade(q: &Question, answer: &str) -> Option<bool> {
    let norm = |s: &str| {
        s.trim()
            .to_lowercase()
            .chars()
            .filter(|c| !c.is_whitespace() && !"。、．，,.!?！？「」『』\"'".contains(*c))
            .collect::<String>()
    };
    if q.kind == "choice" {
        Some(answer.trim() == q.answer.trim())
    } else if norm(answer) == norm(&q.answer) {
        Some(true)
    } else {
        None
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::provider::MockProvider;

    pub fn sample_lesson_json() -> String {
        json!({
            "title": "半導体の新工場",
            "summary": "日本政府宣布補助新的半導體工廠。",
            "vocabulary": [{"term":"半導体","reading":"はんどうたい","part_of_speech":"名詞","meaning":"半導體","example":"半導体の需要が高まっている。","example_translation":"半導體的需求正在增加。"}],
            "grammar": [{"pattern":"〜に伴って","meaning":"隨著","explanation":"表示一方變化另一方也跟著變化。","example":"需要の増加に伴って、価格も上がった。","example_translation":"隨著需求增加，價格也上漲了。"}],
            "questions": [
                {"kind":"choice","question":"「半導体」的讀音？","options":["はんどうたい","はんとうたい"],"answer":"はんどうたい","explanation":"濁音。"},
                {"kind":"short","question":"用「〜に伴って」造句。","options":[],"answer":"技術の進歩に伴って、生活が便利になった。","explanation":"前接名詞。"}
            ]
        })
        .to_string()
    }

    fn input() -> LessonInput {
        LessonInput {
            lang: Lang::Ja,
            level: "N3".into(),
            native_language: "zh-TW".into(),
            title: "新工場".into(),
            url: "https://example.com/a".into(),
            source: "Example".into(),
            text: "政府は新しい半導体工場を支援する。".into(),
            known_words: vec!["政府".into()],
        }
    }

    #[test]
    fn schema_requires_every_field() {
        let s = lesson_schema();
        assert_eq!(s["additionalProperties"], false);
        assert_eq!(s["required"].as_array().unwrap().len(), 5);
        assert_eq!(s["properties"]["questions"]["items"]["required"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn prompt_includes_article_and_level() {
        let r = build_request(&Templates::bundled(), &input()).unwrap();
        assert!(r.user.contains("半導体工場"));
        assert!(r.system.contains("N3") || r.user.contains("N3"));
        assert!(r.user.contains("政府"), "known words are listed");
        let mut long = input();
        long.text = "あ".repeat(MAX_ARTICLE_CHARS + 1);
        assert!(matches!(build_request(&Templates::bundled(), &long), Err(LessonError::TooLong(_))));
    }

    #[test]
    fn workspace_template_overrides_bundled() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MARKDOWN_FILE), "# {{ lesson.title }} ({{ level }})").unwrap();
        let t = Templates::load(Some(dir.path()));
        assert_eq!(t.overridden, vec![MARKDOWN_FILE.to_string()]);
        let l = parse_lesson(&sample_lesson_json()).unwrap();
        let meta = MarkdownMeta { lang: Lang::Ja, level: "N3".into(), url: "".into(), source: "".into(), date: "2026-11-02".into(), engine: "mock".into() };
        assert_eq!(render_markdown(&t, &l, &meta).unwrap(), "# 半導体の新工場 (N3)");
    }

    #[tokio::test]
    async fn generate_renders_markdown() {
        let p = MockProvider::new(vec![format!("```json\n{}\n```", sample_lesson_json())]);
        let (lesson, md) = generate(&p, &Templates::bundled(), &input(), "2026-11-02").await.unwrap();
        assert_eq!(lesson.vocabulary.len(), 1);
        assert!(md.contains("# 半導体の新工場"));
        assert!(md.contains("はんどうたい"));
        assert!(md.contains("〜に伴って"));
        assert!(md.contains("https://example.com/a"));
        assert!(md.contains("・Example\n\n原文："), "source line is its own paragraph:\n{md}");
        assert_eq!(front_matter(&md, "source").as_deref(), Some("https://example.com/a"));
        assert_eq!(front_matter(&md, "title").as_deref(), Some("半導体の新工場"));
        assert!(p.seen.lock().unwrap()[0].json_schema.is_some());
    }

    #[test]
    fn rejects_choice_answer_not_in_options() {
        let mut v: Value = serde_json::from_str(&sample_lesson_json()).unwrap();
        v["questions"][0]["answer"] = json!("x");
        assert!(parse_lesson(&v.to_string()).is_err());
    }

    #[test]
    fn data_round_trips_through_markdown() {
        let mut l = parse_lesson(&sample_lesson_json()).unwrap();
        l.summary = "contains --> marker".into();
        let md = embed_data("---\nlevel: N3\nsource: \"https://x\"\n---\n# T", &l);
        let back = extract_data(&md).unwrap();
        assert_eq!(back.summary, "contains --> marker");
        assert_eq!(front_matter(&md, "level").as_deref(), Some("N3"));
        assert_eq!(front_matter(&md, "source").as_deref(), Some("https://x"));
    }

    #[test]
    fn grading() {
        let l = parse_lesson(&sample_lesson_json()).unwrap();
        assert_eq!(grade(&l.questions[0], "はんどうたい"), Some(true));
        assert_eq!(grade(&l.questions[0], "はんとうたい"), Some(false));
        assert_eq!(grade(&l.questions[1], "技術の進歩に伴って 生活が便利になった"), Some(true));
        assert_eq!(grade(&l.questions[1], "別の文"), None);
    }
}
