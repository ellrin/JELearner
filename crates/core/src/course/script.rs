//! The teacher's script for a lesson taught out loud (docs/plan.md §19.4.2).
//!
//! Written once, in advance, by the lessons model: every line in the language
//! studied, in words the learner's level knows. The lesson page plays it in
//! order (small talk, intro, each paragraph: read aloud, the explained words
//! and grammar, a sentence of the learner's own; then a spoken quiz) and only
//! asks a model live for short replies. A script whose paragraphs do not
//! match the text is mended with plain lines, never thrown away.

use super::prompts::{CourseTemplates, SCRIPT_SYSTEM, SCRIPT_USER};
use super::{parse_json, strict_object, CourseError};
use crate::lang::Lang;
use crate::provider::CompletionRequest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Script {
    #[serde(default)]
    pub lang: String,
    pub small_talk: SmallTalk,
    pub intro: Intro,
    pub paragraphs: Vec<ParagraphScript>,
    pub quiz: Vec<QuizLine>,
    pub goodbye: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SmallTalk {
    pub opening: String,
    pub follow_up: String,
    pub praise: Vec<String>,
    pub to_lesson: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Intro {
    pub lines: Vec<String>,
    pub ready: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ParagraphScript {
    pub read: String,
    pub after_read: String,
    pub explain: Vec<Explain>,
    pub sentence: SentenceTask,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Explain {
    /// `word` or `grammar`.
    pub kind: String,
    pub term: String,
    pub line: String,
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SentenceTask {
    pub prompt: String,
    pub target: String,
    pub example: String,
    pub praise: String,
    pub retry: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct QuizLine {
    pub question: String,
    pub answer: String,
    pub keywords: Vec<String>,
    pub hint: String,
    pub reveal: String,
}

/// What the script is written from.
pub struct ScriptInput<'a> {
    pub lang: Lang,
    pub level_description: String,
    pub native_language: String,
    pub title: &'a str,
    pub paragraphs: &'a [String],
    /// `{term, meaning}`
    pub words: Vec<Value>,
    /// `{pattern, meaning, example}`
    pub grammar: Vec<Value>,
    /// `{question, answer}`
    pub questions: Vec<Value>,
    pub minutes: u32,
}

pub fn schema() -> Value {
    let s = || json!({ "type": "string" });
    let list = |v: Value| json!({ "type": "array", "items": v });
    strict_object(json!({
        "small_talk": strict_object(json!({ "opening": s(), "follow_up": s(), "praise": list(s()), "to_lesson": s() })),
        "intro": strict_object(json!({ "lines": list(s()), "ready": s() })),
        "paragraphs": list(strict_object(json!({
            "read": s(), "after_read": s(),
            "explain": list(strict_object(json!({ "kind": { "type": "string", "enum": ["word", "grammar"] }, "term": s(), "line": s(), "optional": { "type": "boolean" } }))),
            "sentence": strict_object(json!({ "prompt": s(), "target": s(), "example": s(), "praise": s(), "retry": s() })),
        }))),
        "quiz": list(strict_object(json!({ "question": s(), "answer": s(), "keywords": list(s()), "hint": s(), "reveal": s() }))),
        "goodbye": s(),
    }))
}

pub fn script_request(t: &CourseTemplates, i: &ScriptInput) -> Result<CompletionRequest, CourseError> {
    let c = json!({
        "language": i.lang.english_name(), "lang": i.lang.code(), "level_description": i.level_description,
        "native_language": crate::lesson::native_name(&i.native_language), "title": i.title, "paragraphs": i.paragraphs,
        "words": i.words, "grammar": i.grammar, "questions": i.questions, "minutes": i.minutes,
    });
    Ok(CompletionRequest { system: t.render(SCRIPT_SYSTEM, c.clone())?, user: t.render(SCRIPT_USER, c)?, max_tokens: 8000, json_schema: Some(schema()) })
}

/// Plain lines for whatever the model left out (a paragraph it skipped, an empty field).
fn plain(lang: Lang) -> (ParagraphScript, Script) {
    match lang {
        Lang::Ja => (
            ParagraphScript { read: "ここを声に出して読んでみましょう。".into(), after_read: "よくできました。".into(), explain: vec![], sentence: SentenceTask::default() },
            Script {
                lang: "ja".into(),
                small_talk: SmallTalk { opening: "こんにちは。今日はどうですか。".into(), follow_up: "そうですか。もう少し教えてください。".into(), praise: vec!["いいですね。".into(), "すてきですね。".into(), "なるほど。".into()], to_lesson: "では、今日のレッスンを始めましょう。".into() },
                intro: Intro { lines: vec![], ready: "準備はいいですか。".into() },
                paragraphs: vec![],
                quiz: vec![],
                goodbye: "今日もよくがんばりました。またね。".into(),
            },
        ),
        _ => (
            ParagraphScript { read: "Please read this part out loud.".into(), after_read: "Nice reading!".into(), explain: vec![], sentence: SentenceTask::default() },
            Script {
                lang: "en".into(),
                small_talk: SmallTalk { opening: "Hi! How is your day going?".into(), follow_up: "I see. Can you tell me a little more?".into(), praise: vec!["That sounds nice!".into(), "Great!".into(), "I see, thanks for telling me.".into()], to_lesson: "Now, let's start today's lesson.".into() },
                intro: Intro { lines: vec![], ready: "Are you ready?".into() },
                paragraphs: vec![],
                quiz: vec![],
                goodbye: "Great work today. See you next time!".into(),
            },
        ),
    }
}

/// The model's script, mended to fit the text: one entry per paragraph,
/// no empty line left where the lesson plays one.
pub fn parse_script(text: &str, paragraphs: usize, lang: Lang) -> Result<Script, CourseError> {
    let mut s: Script = parse_json(text)?;
    mend(&mut s, paragraphs, lang);
    Ok(s)
}

pub fn mend(s: &mut Script, paragraphs: usize, lang: Lang) {
    let (para, base) = plain(lang);
    s.lang = lang.code().into();
    let fill = |v: &mut String, d: &str| {
        if v.trim().is_empty() {
            *v = d.to_string();
        }
    };
    fill(&mut s.small_talk.opening, &base.small_talk.opening);
    fill(&mut s.small_talk.follow_up, &base.small_talk.follow_up);
    fill(&mut s.small_talk.to_lesson, &base.small_talk.to_lesson);
    s.small_talk.praise.retain(|p| !p.trim().is_empty());
    if s.small_talk.praise.is_empty() {
        s.small_talk.praise = base.small_talk.praise.clone();
    }
    s.intro.lines.retain(|l| !l.trim().is_empty());
    fill(&mut s.intro.ready, &base.intro.ready);
    fill(&mut s.goodbye, &base.goodbye);
    s.paragraphs.truncate(paragraphs);
    while s.paragraphs.len() < paragraphs {
        s.paragraphs.push(para.clone());
    }
    for p in s.paragraphs.iter_mut() {
        fill(&mut p.read, &para.read);
        fill(&mut p.after_read, &para.after_read);
        p.explain.retain(|e| !e.line.trim().is_empty());
        if p.sentence.target.trim().is_empty() || p.sentence.prompt.trim().is_empty() {
            p.sentence = SentenceTask::default();
        }
    }
    s.quiz.retain(|q| !q.question.trim().is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_is_mended_to_fit_the_text() {
        let reply = json!({
            "small_talk": { "opening": "Hi! How was your day?", "follow_up": "", "praise": ["Nice!"], "to_lesson": "Let's start." },
            "intro": { "lines": ["Today we learn to order a dram.", ""], "ready": "Ready?" },
            "paragraphs": [
                { "read": "Read this one, please.", "after_read": "Great!", "explain": [{ "kind": "word", "term": "a dram", "line": "A dram is a small glass of whisky.", "optional": false }, { "kind": "word", "term": "x", "line": " ", "optional": true }],
                  "sentence": { "prompt": "Make a sentence with a dram.", "target": "a dram", "example": "I'd like a dram.", "praise": "Perfect!", "retry": "Try again with a dram." } },
                { "read": "", "after_read": "", "explain": [], "sentence": { "prompt": "", "target": "", "example": "", "praise": "", "retry": "" } },
                { "read": "extra", "after_read": "extra", "explain": [], "sentence": { "prompt": "", "target": "", "example": "", "praise": "", "retry": "" } }
            ],
            "quiz": [{ "question": "What is a dram?", "answer": "a small whisky", "keywords": ["small", "whisky"], "hint": "Think of size.", "reveal": "A dram is a small whisky." }, { "question": " ", "answer": "", "keywords": [], "hint": "", "reveal": "" }],
            "goodbye": ""
        });
        let s = parse_script(&reply.to_string(), 2, Lang::En).unwrap();
        assert_eq!(s.lang, "en");
        assert_eq!(s.paragraphs.len(), 2, "one per paragraph");
        assert_eq!(s.paragraphs[0].explain.len(), 1, "an empty explanation is dropped");
        assert_eq!(s.paragraphs[1].read, "Please read this part out loud.", "a plain line where the model left none");
        assert!(s.paragraphs[1].sentence.target.is_empty());
        assert_eq!(s.small_talk.follow_up, "I see. Can you tell me a little more?");
        assert_eq!(s.intro.lines.len(), 1);
        assert_eq!(s.quiz.len(), 1);
        assert_eq!(s.goodbye, "Great work today. See you next time!");
        let short = parse_script(&json!({ "small_talk": { "opening": "", "follow_up": "", "praise": [], "to_lesson": "" }, "intro": { "lines": [], "ready": "" }, "paragraphs": [], "quiz": [], "goodbye": "" }).to_string(), 3, Lang::Ja).unwrap();
        assert_eq!(short.paragraphs.len(), 3);
        assert_eq!(short.paragraphs[0].read, "ここを声に出して読んでみましょう。");
        assert!(parse_script("not json", 1, Lang::En).is_err());
    }

    #[test]
    fn the_request_names_the_paragraphs_and_words() {
        let t = CourseTemplates::bundled();
        let paras = vec!["Ask for a dram.".to_string(), "It is aged in oak.".to_string()];
        let r = script_request(&t, &ScriptInput { lang: Lang::En, level_description: "B1".into(), native_language: "zh-TW".into(), title: "A Dram", paragraphs: &paras, words: vec![json!({ "term": "a dram", "meaning": "一小杯威士忌" })], grammar: vec![], questions: vec![json!({ "question": "What is a dram?", "answer": "a small whisky" })], minutes: 15 }).unwrap();
        assert!(r.user.contains("[1] Ask for a dram.") && r.user.contains("[2] It is aged in oak.") && r.user.contains("- a dram: 一小杯威士忌"), "{}", r.user);
        assert!(r.system.contains("English only") || r.system.contains("in English"), "{}", r.system);
        assert!(r.json_schema.is_some());
    }
}
