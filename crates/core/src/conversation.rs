//! AI conversation, typed version (docs/plan.md §10).
//!
//! A conversation has a topic, 3–5 target words, and a turn limit (default
//! 20). Each learner turn goes to the model with the whole transcript; the
//! model answers with a JSON object: a **recast** of what the learner meant
//! (never "you are wrong"), the next **question**, the **target words** the
//! learner used and, for Japanese, sentence **starters**. After the last turn
//! the conversation is locked and evaluated with a second schema (fluency,
//! target words, grammar, better sentences, words to add, keywords).
//!
//! Prompts are files (`prompts/conversation_en.md`, `conversation_ja.md`,
//! `conversation_review.md`, `conversation.md.j2`), overridable by files of
//! the same name in the workspace `prompts/`. Each finished conversation is
//! saved as Markdown with its data embedded (like lessons), so the database
//! can be rebuilt from the workspace.

use crate::db::{Db, DbResult};
use crate::lang::Lang;
use crate::provider::{ChatMessage, ChatRequest, CompletionRequest};
use crate::topics::{Topic, TopicKind};
use chrono::{DateTime, Utc};
use minijinja::Environment;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

pub const EN_FILE: &str = "conversation_en.md";
pub const JA_FILE: &str = "conversation_ja.md";
pub const REVIEW_FILE: &str = "conversation_review.md";
pub const MARKDOWN_FILE: &str = "conversation.md.j2";

const BUNDLED_EN: &str = include_str!("../../../prompts/conversation_en.md");
const BUNDLED_JA: &str = include_str!("../../../prompts/conversation_ja.md");
const BUNDLED_REVIEW: &str = include_str!("../../../prompts/conversation_review.md");
const BUNDLED_MARKDOWN: &str = include_str!("../../../prompts/conversation.md.j2");

/// The first user message: conversations start with the AI, but the
/// messages API wants a user message first.
pub const OPENING_CUE: &str = "(The learner opened the conversation. Greet them and ask the first question.)";
/// Typed turns are limited in length (plan §10.1).
pub const MAX_TURN_CHARS: usize = 300;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub term: String,
    #[serde(default)]
    pub reading: String,
    #[serde(default)]
    pub meaning: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    Ai,
    Me,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub role: Speaker,
    /// AI: the question (or the goodbye on the last turn); learner: what they typed.
    pub text: String,
    /// AI only: the learner's previous message said better.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub recast: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub starters: Vec<String>,
    /// Learner only: target words used in this turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub used: Vec<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Better {
    pub original: String,
    pub suggestion: String,
    #[serde(default)]
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedWord {
    pub term: String,
    #[serde(default)]
    pub reading: String,
    #[serde(default)]
    pub meaning: String,
}

/// The end evaluation (scores 1–5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub fluency: i64,
    pub target_use: i64,
    pub grammar: i64,
    /// `excellent` | `good` | `needs_work`
    pub overall: String,
    pub summary: String,
    pub better: Vec<Better>,
    pub suggested_words: Vec<SuggestedWord>,
    pub keywords: Vec<String>,
}

impl Review {
    /// Average of the three scores on 0–1.
    pub fn score(&self) -> f64 {
        let c = |x: i64| x.clamp(1, 5) as f64;
        (c(self.fluency) + c(self.target_use) + c(self.grammar)) / 15.0
    }
    pub fn excellent(&self) -> bool {
        self.overall == "excellent"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub lang: Lang,
    pub topic: Topic,
    /// Content difficulty when it started, e.g. `B1-3`.
    pub level: String,
    pub targets: Vec<Target>,
    pub turns_limit: u32,
    pub turns: Vec<Turn>,
    pub engine: String,
    pub started_at: DateTime<Utc>,
    #[serde(default)]
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub review: Option<Review>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub passed: Option<bool>,
    #[serde(default)]
    pub points: Option<f64>,
}

impl Conversation {
    /// Learner turns so far.
    pub fn turns_done(&self) -> u32 {
        self.turns.iter().filter(|t| t.role == Speaker::Me).count() as u32
    }
    pub fn locked(&self) -> bool {
        self.turns_done() >= self.turns_limit
    }
    /// Target words used at least once.
    pub fn used_targets(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for t in self.turns.iter().filter(|t| t.role == Speaker::Me) {
            for u in &t.used {
                if !out.contains(u) {
                    out.push(u.clone());
                }
            }
        }
        out
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConversationError {
    #[error("模板錯誤（{0}）：{1}")]
    Template(String, String),
    #[error("回覆格式不符：{0}")]
    Invalid(String),
}

#[derive(Debug, Clone)]
pub struct Templates {
    pub en: String,
    pub ja: String,
    pub review: String,
    pub markdown: String,
    pub overridden: Vec<String>,
}

impl Templates {
    pub fn bundled() -> Templates {
        Templates { en: BUNDLED_EN.into(), ja: BUNDLED_JA.into(), review: BUNDLED_REVIEW.into(), markdown: BUNDLED_MARKDOWN.into(), overridden: vec![] }
    }

    pub fn load(workspace_prompts: Option<&Path>) -> Templates {
        let mut t = Templates::bundled();
        if let Some(dir) = workspace_prompts {
            for (name, slot) in [(EN_FILE, &mut t.en), (JA_FILE, &mut t.ja), (REVIEW_FILE, &mut t.review), (MARKDOWN_FILE, &mut t.markdown)] {
                if let Ok(s) = std::fs::read_to_string(dir.join(name))
                    && !s.trim().is_empty()
                {
                    *slot = s;
                    t.overridden.push(name.to_string());
                }
            }
        }
        t
    }
}

fn render(name: &str, template: &str, ctx: Value) -> Result<String, ConversationError> {
    let mut env = Environment::new();
    env.set_trim_blocks(true);
    env.set_lstrip_blocks(true);
    env.add_template(name, template).map_err(|e| ConversationError::Template(name.into(), e.to_string()))?;
    env.get_template(name).and_then(|t| t.render(ctx)).map_err(|e| ConversationError::Template(name.into(), e.to_string()))
}

fn obj(props: Value) -> Value {
    let required: Vec<String> = props.as_object().unwrap().keys().cloned().collect();
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

/// Schema of one AI turn (plan §10.3, plus `starters` for Japanese).
pub fn turn_schema() -> Value {
    let s = json!({ "type": "string" });
    let list = json!({ "type": "array", "items": { "type": "string" } });
    obj(json!({ "recast": s, "question": s, "used_targets": list, "starters": list }))
}

/// Schema of the end evaluation.
pub fn review_schema() -> Value {
    let s = json!({ "type": "string" });
    let n = json!({ "type": "integer" });
    obj(json!({
        "fluency": n, "target_use": n, "grammar": n,
        "overall": { "type": "string", "enum": ["excellent", "good", "needs_work"] },
        "summary": s,
        "better": { "type": "array", "items": obj(json!({ "original": s, "suggestion": s, "why": s })) },
        "suggested_words": { "type": "array", "items": obj(json!({ "term": s, "reading": s, "meaning": s })) },
        "keywords": { "type": "array", "items": s }
    }))
}

pub fn native_name(code: &str) -> &str {
    match code {
        "zh-TW" | "zh-Hant" => "Traditional Chinese (Taiwan)",
        "zh-CN" | "zh-Hans" => "Simplified Chinese",
        "ja" => "Japanese",
        "en" => "English",
        other => other,
    }
}

/// What the prompts know about the learner.
#[derive(Debug, Clone, Serialize)]
pub struct Learner {
    pub native_language: String,
    /// e.g. `CEFR B1 (step 3 of 4 within B1)`
    pub level_description: String,
}

fn context(c: &Conversation, learner: &Learner, turn: u32) -> Value {
    json!({
        "language": c.lang.english_name(),
        "lang": c.lang.code(),
        "level": c.level,
        "level_description": learner.level_description,
        "native_language": native_name(&learner.native_language),
        "topic": c.topic.name,
        "topic_kind": c.topic.kind.code(),
        "targets": c.targets,
        "target_terms": c.targets.iter().map(|t| t.term.clone()).collect::<Vec<_>>(),
        "turn": turn,
        "turns": c.turns_limit,
        "remaining": c.turns_limit.saturating_sub(turn),
        "last": turn >= c.turns_limit,
    })
}

/// The AI turn as it goes back into the transcript.
fn ai_text(t: &Turn) -> String {
    if t.recast.is_empty() { t.text.clone() } else { format!("{}\n{}", t.recast, t.text) }
}

/// The request for the next AI turn: the opening when there are no turns,
/// otherwise the reply to the learner's last turn (already in `c.turns`).
pub fn turn_request(t: &Templates, c: &Conversation, learner: &Learner) -> Result<ChatRequest, ConversationError> {
    let system_t = if c.lang == Lang::Ja { &t.ja } else { &t.en };
    let file = if c.lang == Lang::Ja { JA_FILE } else { EN_FILE };
    let system = render(file, system_t, context(c, learner, c.turns_done()))?;
    let mut messages = vec![ChatMessage::user(OPENING_CUE)];
    for turn in &c.turns {
        messages.push(match turn.role {
            Speaker::Ai => ChatMessage::assistant(ai_text(turn)),
            Speaker::Me => ChatMessage::user(turn.text.clone()),
        });
    }
    Ok(ChatRequest { system, messages, max_tokens: 2_000, json_schema: Some(turn_schema()) })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnReply {
    pub recast: String,
    pub question: String,
    #[serde(default)]
    pub used_targets: Vec<String>,
    #[serde(default)]
    pub starters: Vec<String>,
}

fn strip_fence(text: &str) -> &str {
    let t = text.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
    t.strip_suffix("```").unwrap_or(t).trim()
}

pub fn parse_turn(text: &str) -> Result<TurnReply, ConversationError> {
    let r: TurnReply = serde_json::from_str(strip_fence(text)).map_err(|e| ConversationError::Invalid(e.to_string()))?;
    if r.question.trim().is_empty() && r.recast.trim().is_empty() {
        return Err(ConversationError::Invalid("沒有內容".into()));
    }
    Ok(r)
}

/// The evaluation request: the transcript as text.
pub fn review_request(t: &Templates, c: &Conversation, learner: &Learner) -> Result<CompletionRequest, ConversationError> {
    let system = render(REVIEW_FILE, &t.review, context(c, learner, c.turns_done()))?;
    let mut user = format!("Topic: {}\nTarget words: {}\n\n<conversation>\n", c.topic.name, c.targets.iter().map(|t| t.term.as_str()).collect::<Vec<_>>().join(", "));
    for turn in &c.turns {
        match turn.role {
            Speaker::Ai => user.push_str(&format!("[partner] {}\n", ai_text(turn).replace('\n', " "))),
            Speaker::Me => user.push_str(&format!("[learner] {}\n", turn.text)),
        }
    }
    user.push_str("</conversation>\n\nEvaluate the learner's turns.");
    Ok(CompletionRequest { system, user, max_tokens: 4_000, json_schema: Some(review_schema()) })
}

pub fn parse_review(text: &str) -> Result<Review, ConversationError> {
    serde_json::from_str(strip_fence(text)).map_err(|e| ConversationError::Invalid(e.to_string()))
}

/// Whether a typed turn says something (not empty, not one or two words):
/// English at least 3 words; Japanese (Chinese mixed in is fine) at least 4
/// characters that are not spaces or punctuation.
pub fn is_real(text: &str, lang: Lang) -> bool {
    match lang {
        Lang::En => text.split_whitespace().filter(|w| w.chars().any(|c| c.is_alphabetic())).count() >= 3,
        Lang::Ja => text.chars().filter(|c| c.is_alphanumeric()).count() >= 4,
    }
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// `word` is `base` or a regular inflection of it.
fn inflects(word: &str, base: &str) -> bool {
    if word == base {
        return true;
    }
    let stem = base.strip_suffix('e').unwrap_or(base);
    let y = base.strip_suffix('y').map(|s| format!("{s}i"));
    ["s", "es", "ed", "d", "ing", "er", "ers", "ly"].iter().any(|suf| {
        word == format!("{base}{suf}") || word == format!("{stem}{suf}") || y.as_ref().is_some_and(|y| word == format!("{y}{suf}"))
    }) || (base.len() >= 3 && word.len() == base.len() + 4 && word.starts_with(base) && word.ends_with("ing") && word.as_bytes()[base.len()] == *base.as_bytes().last().unwrap())
}

/// Target words used in a learner's turn. English: whole words with
/// regular inflections (multi-word phrases word by word, the first word
/// inflected); Japanese: the term or its reading anywhere in the text.
pub fn detect_used(text: &str, targets: &[Target], lang: Lang) -> Vec<String> {
    let mut out = vec![];
    match lang {
        Lang::En => {
            let ws = words(text);
            for t in targets {
                let tw = words(&t.term);
                if tw.is_empty() {
                    continue;
                }
                let hit = ws.windows(tw.len()).any(|w| inflects(&w[0], &tw[0]) && w[1..].iter().zip(&tw[1..]).all(|(a, b)| a == b));
                if hit {
                    out.push(t.term.clone());
                }
            }
        }
        Lang::Ja => {
            for t in targets {
                let term = t.term.trim();
                let reading = t.reading.trim();
                if (!term.is_empty() && text.contains(term)) || (reading.chars().count() >= 2 && text.contains(reading)) {
                    out.push(t.term.clone());
                }
            }
        }
    }
    out
}

/// Target words used: what we detect, plus what the model reported (only
/// real targets count).
pub fn merge_used(detected: Vec<String>, reported: &[String], targets: &[Target]) -> Vec<String> {
    let mut out = detected;
    for r in reported {
        if let Some(t) = targets.iter().find(|t| t.term.eq_ignore_ascii_case(r.trim()))
            && !out.contains(&t.term)
        {
            out.push(t.term.clone());
        }
    }
    out
}

/// The Markdown record (front matter, transcript, feedback).
pub fn render_markdown(t: &Templates, c: &Conversation, date: &str) -> Result<String, ConversationError> {
    let used = c.used_targets();
    let unused: Vec<String> = c.targets.iter().map(|t| t.term.clone()).filter(|t| !used.contains(t)).collect();
    let ctx = json!({
        "c": c,
        "lang": c.lang.code(),
        "language_label": c.lang.label(),
        "date": date,
        "turns_done": c.turns_done(),
        "used": used,
        "unused": unused,
        "score_pct": c.score.map(|s| (s * 100.0).round() as i64),
    });
    render(MARKDOWN_FILE, &t.markdown, ctx)
}

const DATA_MARK: &str = "<!-- jelearner:conversation ";

pub fn embed_data(markdown: &str, c: &Conversation) -> String {
    let json = serde_json::to_string(c).unwrap_or_default().replace("-->", "--\\u003e");
    format!("{}\n\n{DATA_MARK}{json} -->\n", markdown.trim_end())
}

pub fn extract_data(markdown: &str) -> Option<Conversation> {
    let start = markdown.rfind(DATA_MARK)? + DATA_MARK.len();
    let end = markdown[start..].find(" -->")? + start;
    serde_json::from_str(&markdown[start..end]).ok()
}

// ── storage ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Row {
    pub id: i64,
    pub lang: String,
    pub topic_id: String,
    pub topic: String,
    pub topic_kind: String,
    /// `active` | `locked` | `done`
    pub status: String,
    pub turns_done: i64,
    pub turns_limit: i64,
    pub score: Option<f64>,
    pub passed: Option<bool>,
    pub points: Option<f64>,
    pub path: String,
    pub data: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

impl Row {
    pub fn conversation(&self) -> Option<Conversation> {
        serde_json::from_str(&self.data).ok()
    }
}

fn status_of(c: &Conversation) -> &'static str {
    if c.review.is_some() {
        "done"
    } else if c.locked() {
        "locked"
    } else {
        "active"
    }
}

pub async fn insert(db: &Db, c: &Conversation, path: &str) -> DbResult<i64> {
    sqlx::query_scalar(
        "INSERT INTO conversations (lang, topic_id, topic, topic_kind, status, turns_done, turns_limit, score, passed, points, path, data, started_at, ended_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(c.lang.code())
    .bind(&c.topic.id)
    .bind(&c.topic.name)
    .bind(c.topic.kind.code())
    .bind(status_of(c))
    .bind(c.turns_done() as i64)
    .bind(c.turns_limit as i64)
    .bind(c.score)
    .bind(c.passed)
    .bind(c.points)
    .bind(path)
    .bind(serde_json::to_string(c).unwrap_or_default())
    .bind(c.started_at)
    .bind(c.ended_at)
    .fetch_one(db.pool())
    .await
}

/// Save a conversation's new state, unless someone else changed it first
/// (`expect_turns` = turns it had when this change started).
pub async fn update(db: &Db, id: i64, c: &Conversation, expect_turns: Option<u32>, path: &str) -> DbResult<bool> {
    let n = sqlx::query(
        "UPDATE conversations SET status = ?, turns_done = ?, score = ?, passed = ?, points = ?, path = ?, data = ?, ended_at = ?
         WHERE id = ? AND (?10 IS NULL OR turns_done = ?10)",
    )
    .bind(status_of(c))
    .bind(c.turns_done() as i64)
    .bind(c.score)
    .bind(c.passed)
    .bind(c.points)
    .bind(path)
    .bind(serde_json::to_string(c).unwrap_or_default())
    .bind(c.ended_at)
    .bind(id)
    .bind(expect_turns.map(|t| t as i64))
    .execute(db.pool())
    .await?
    .rows_affected();
    Ok(n > 0)
}

pub async fn get(db: &Db, id: i64) -> DbResult<Option<Row>> {
    sqlx::query_as::<_, Row>("SELECT * FROM conversations WHERE id = ?").bind(id).fetch_optional(db.pool()).await
}

pub async fn list(db: &Db, lang: Option<Lang>, limit: i64) -> DbResult<Vec<Row>> {
    sqlx::query_as::<_, Row>(
        "SELECT id, lang, topic_id, topic, topic_kind, status, turns_done, turns_limit, score, passed, points, path, '' AS data, started_at, ended_at
         FROM conversations WHERE (?1 IS NULL OR lang = ?1) ORDER BY started_at DESC, id DESC LIMIT ?2",
    )
    .bind(lang.map(Lang::code))
    .bind(limit)
    .fetch_all(db.pool())
    .await
}

/// Recent topics, newest first, for [`crate::topics::pick`].
pub async fn topic_history(db: &Db, lang: Lang) -> DbResult<Vec<(String, TopicKind)>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT topic_id, topic_kind FROM conversations WHERE lang = ? ORDER BY started_at DESC, id DESC LIMIT 40")
            .bind(lang.code())
            .fetch_all(db.pool())
            .await?;
    Ok(rows.into_iter().map(|(id, k)| (id, TopicKind::parse(&k))).collect())
}

/// Target words: words the learner has (preferably reviewed, i.e.
/// recognised) but rarely used, rotating through those picked before.
pub async fn pick_targets(db: &Db, lang: Lang, n: usize) -> DbResult<Vec<Target>> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT w.term, w.reading, w.meaning FROM words w
         LEFT JOIN word_usage u ON u.lang = w.lang AND u.term = w.term
         LEFT JOIN progress p ON p.item_kind = 'word' AND p.item_id = w.id
         WHERE w.lang = ?1 AND length(w.term) BETWEEN 1 AND 40
         ORDER BY COALESCE(u.used, 0), COALESCE(u.targeted, 0), (COALESCE(p.reps, 0) > 0) DESC, w.id DESC
         LIMIT ?2",
    )
    .bind(lang.code())
    .bind(n as i64)
    .fetch_all(db.pool())
    .await?;
    let targets: Vec<Target> = rows.into_iter().map(|(term, reading, meaning)| Target { term, reading, meaning }).collect();
    for t in &targets {
        sqlx::query("INSERT INTO word_usage (lang, term, targeted) VALUES (?, ?, 1) ON CONFLICT (lang, term) DO UPDATE SET targeted = targeted + 1")
            .bind(lang.code())
            .bind(&t.term)
            .execute(db.pool())
            .await?;
    }
    Ok(targets)
}

/// Count an active use of each word (plan §10.2: used words are picked less).
pub async fn record_usage(db: &Db, lang: Lang, used: &[String], at: DateTime<Utc>) -> DbResult<()> {
    for term in used {
        sqlx::query(
            "INSERT INTO word_usage (lang, term, used, last_used_at) VALUES (?, ?, 1, ?)
             ON CONFLICT (lang, term) DO UPDATE SET used = used + 1, last_used_at = excluded.last_used_at",
        )
        .bind(lang.code())
        .bind(term)
        .bind(at)
        .execute(db.pool())
        .await?;
    }
    Ok(())
}

pub async fn record_topic(db: &Db, c: &Conversation) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO topic_stats (lang, topic_id, topic, kind, count, score_sum, scored, last_at) VALUES (?, ?, ?, ?, 1, ?, ?, ?)
         ON CONFLICT (lang, topic_id) DO UPDATE SET count = count + 1, score_sum = score_sum + excluded.score_sum,
           scored = scored + excluded.scored, last_at = excluded.last_at, topic = excluded.topic",
    )
    .bind(c.lang.code())
    .bind(&c.topic.id)
    .bind(&c.topic.name)
    .bind(c.topic.kind.code())
    .bind(c.score.unwrap_or(0.0))
    .bind(c.score.is_some() as i64)
    .bind(c.ended_at.unwrap_or(c.started_at))
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Topic coverage: (topic id, name, kind, count, average score).
pub async fn topic_stats(db: &Db, lang: Lang) -> DbResult<Vec<(String, String, String, i64, Option<f64>)>> {
    let rows: Vec<(String, String, String, i64, f64, i64)> =
        sqlx::query_as("SELECT topic_id, topic, kind, count, score_sum, scored FROM topic_stats WHERE lang = ? ORDER BY last_at DESC")
            .bind(lang.code())
            .fetch_all(db.pool())
            .await?;
    Ok(rows.into_iter().map(|(id, n, k, c, s, sc)| (id, n, k, c, (sc > 0).then(|| s / sc as f64))).collect())
}

/// Read a conversation file into the database when it is not there yet
/// (another computer, or a fresh database). Returns whether it was added.
pub async fn import_file(db: &Db, markdown: &str, rel_path: &str) -> DbResult<bool> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM conversations WHERE path = ?").bind(rel_path).fetch_optional(db.pool()).await?;
    if exists.is_some() {
        return Ok(false);
    }
    let Some(c) = extract_data(markdown) else { return Ok(false) };
    insert(db, &c, rel_path).await?;
    for t in c.turns.iter().filter(|t| t.role == Speaker::Me) {
        record_usage(db, c.lang, &t.used, t.at).await?;
    }
    record_topic(db, &c).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{LlmProvider, MockProvider};

    fn targets() -> Vec<Target> {
        vec![
            Target { term: "mitigate".into(), reading: "".into(), meaning: "減輕".into() },
            Target { term: "ramp up".into(), reading: "".into(), meaning: "逐步提高".into() },
            Target { term: "study".into(), reading: "".into(), meaning: "讀書".into() },
        ]
    }

    fn conv(lang: Lang, limit: u32) -> Conversation {
        Conversation {
            lang,
            topic: Topic { id: "school".into(), name: "校園與社團".into(), kind: TopicKind::General, group: "生活".into() },
            level: if lang == Lang::Ja { "N5-2".into() } else { "B1-3".into() },
            targets: targets(),
            turns_limit: limit,
            turns: vec![],
            engine: "mock".into(),
            started_at: Utc::now(),
            ended_at: None,
            review: None,
            score: None,
            passed: None,
            points: None,
        }
    }

    fn learner() -> Learner {
        Learner { native_language: "zh-TW".into(), level_description: "CEFR B1 (step 3 of 4 within B1)".into() }
    }

    #[test]
    fn real_turns_and_target_detection() {
        assert!(!is_real("yes", Lang::En));
        assert!(!is_real("ok sure", Lang::En));
        assert!(is_real("I like coffee", Lang::En));
        assert!(!is_real("はい。", Lang::Ja));
        assert!(is_real("コーヒーが好き", Lang::Ja));
        assert!(is_real("我喜歡 coffee", Lang::Ja), "mixing Chinese is fine in Japanese mode");
        let used = detect_used("We tried to mitigated it, then ramped up studying.", &targets(), Lang::En);
        assert_eq!(used, vec!["mitigate".to_string(), "ramp up".into(), "study".into()]);
        assert!(detect_used("the student", &targets(), Lang::En).is_empty(), "no partial-word matches");
        assert_eq!(detect_used("I studied", &targets(), Lang::En), vec!["study".to_string()]);
        let ja = vec![Target { term: "確保".into(), reading: "かくほ".into(), meaning: "".into() }];
        assert_eq!(detect_used("じかんをかくほしたい", &ja, Lang::Ja), vec!["確保".to_string()]);
        assert_eq!(merge_used(vec![], &["Mitigate".into(), "unknown".into()], &targets()), vec!["mitigate".to_string()]);
    }

    #[test]
    fn schemas_require_everything() {
        let t = turn_schema();
        assert_eq!(t["required"].as_array().unwrap().len(), 4);
        assert_eq!(t["additionalProperties"], false);
        let r = review_schema();
        assert_eq!(r["required"].as_array().unwrap().len(), 8);
        assert_eq!(r["properties"]["better"]["items"]["additionalProperties"], false);
    }

    #[tokio::test]
    async fn turns_carry_the_whole_transcript_until_the_limit() {
        let t = Templates::bundled();
        let mut c = conv(Lang::En, 2);
        let reply = |q: &str| json!({ "recast": "So you study design.", "question": q, "used_targets": [], "starters": [] }).to_string();
        let p = MockProvider::new(vec![
            json!({ "recast": "", "question": "Hi! What are you studying?", "used_targets": [], "starters": [] }).to_string(),
            reply("What do you like about it?"),
            reply("Nice talking!"),
        ]);
        let open = turn_request(&t, &c, &learner()).unwrap();
        assert_eq!(open.messages.len(), 1, "opening: only the cue");
        assert!(open.system.contains("mitigate") && open.system.contains("校園與社團") && open.system.contains("B1-3"));
        let r = parse_turn(&p.chat(&open).await.unwrap()).unwrap();
        c.turns.push(Turn { role: Speaker::Ai, text: r.question, recast: r.recast, starters: r.starters, used: vec![], at: Utc::now() });
        for (i, msg) in ["I study design at university.", "I like to mitigate stress by drawing."].iter().enumerate() {
            let used = detect_used(msg, &c.targets, c.lang);
            c.turns.push(Turn { role: Speaker::Me, text: msg.to_string(), recast: String::new(), starters: vec![], used, at: Utc::now() });
            let req = turn_request(&t, &c, &learner()).unwrap();
            assert_eq!(req.messages.len(), 2 + 2 * i + 1, "cue + every turn so far");
            assert_eq!(req.messages.last().unwrap().content, *msg);
            if i == 1 {
                assert!(req.system.contains("last turn"), "the model is told it is the last turn:\n{}", req.system);
            }
            let r = parse_turn(&p.chat(&req).await.unwrap()).unwrap();
            c.turns.push(Turn { role: Speaker::Ai, text: r.question, recast: r.recast, starters: r.starters, used: vec![], at: Utc::now() });
        }
        assert!(c.locked());
        assert_eq!(c.used_targets(), vec!["study".to_string(), "mitigate".into()]);
        let seen = p.seen_chats.lock().unwrap();
        assert_eq!(seen[2].messages[1].content, "Hi! What are you studying?");
        assert!(seen[2].messages[2].content.contains("design"));
    }

    #[test]
    fn japanese_prompt_asks_for_starters_and_short_sentences() {
        let t = Templates::bundled();
        let c = conv(Lang::Ja, 20);
        let r = turn_request(&t, &c, &learner()).unwrap();
        assert!(r.system.contains("starters"));
        assert!(r.system.contains("N5-2"));
    }

    #[test]
    fn review_and_markdown_round_trip() {
        let t = Templates::bundled();
        let mut c = conv(Lang::En, 1);
        c.turns.push(Turn { role: Speaker::Ai, text: "What do you study?".into(), recast: String::new(), starters: vec![], used: vec![], at: Utc::now() });
        c.turns.push(Turn { role: Speaker::Me, text: "I study design -->".into(), recast: String::new(), starters: vec![], used: vec!["study".into()], at: Utc::now() });
        c.turns.push(Turn { role: Speaker::Ai, text: "Great talking!".into(), recast: "So you're a design student.".into(), starters: vec![], used: vec![], at: Utc::now() });
        let req = review_request(&t, &c, &learner()).unwrap();
        assert!(req.user.contains("[learner] I study design"));
        assert!(req.json_schema.is_some());
        let review = parse_review(
            &json!({"fluency": 4, "target_use": 3, "grammar": 5, "overall": "good", "summary": "很好", "better": [{"original": "I study design", "suggestion": "I'm studying design.", "why": "進行式"}], "suggested_words": [{"term": "major", "reading": "", "meaning": "主修"}], "keywords": ["design"]}).to_string(),
        )
        .unwrap();
        assert!((review.score() - 0.8).abs() < 1e-9);
        c.review = Some(review);
        c.score = Some(0.8);
        let md = render_markdown(&t, &c, "2026-11-06").unwrap();
        assert!(md.contains("↺ So you're a design student."), "{md}");
        assert!(md.contains("I'm studying design."));
        assert!(md.contains("mitigate"), "unused targets listed");
        let full = embed_data(&md, &c);
        let back = extract_data(&full).unwrap();
        assert_eq!(back.turns[1].text, "I study design -->");
        assert_eq!(back.review.unwrap().grammar, 5);
    }

    #[tokio::test]
    async fn storage_targets_usage_and_import() {
        let db = Db::memory().await.unwrap();
        for term in ["alpha", "beta", "gamma", "delta"] {
            db.upsert_word(&crate::db::NewWord { lang: Some(Lang::En), term: term.into(), ..Default::default() }, Lang::En).await.unwrap();
        }
        let a = pick_targets(&db, Lang::En, 2).await.unwrap();
        let b = pick_targets(&db, Lang::En, 2).await.unwrap();
        assert!(a.iter().all(|t| !b.contains(t)), "targets rotate: {a:?} {b:?}");
        record_usage(&db, Lang::En, &[a[0].term.clone()], Utc::now()).await.unwrap();
        let c = pick_targets(&db, Lang::En, 4).await.unwrap();
        assert_eq!(c.last().unwrap().term, a[0].term, "a used word is picked last");

        let mut cv = conv(Lang::En, 1);
        cv.turns.push(Turn { role: Speaker::Me, text: "x".into(), recast: String::new(), starters: vec![], used: vec!["study".into()], at: Utc::now() });
        let md = embed_data("# x", &cv);
        assert!(import_file(&db, &md, "profiles/ellrin/conversations/en/a.md").await.unwrap());
        assert!(!import_file(&db, &md, "profiles/ellrin/conversations/en/a.md").await.unwrap(), "only once");
        assert_eq!(topic_history(&db, Lang::En).await.unwrap()[0].0, "school");
        let row = &list(&db, Some(Lang::En), 10).await.unwrap()[0];
        assert_eq!(row.status, "locked");
    }
}
