//! Importing the user's own material: vocabulary and grammar CSV files and
//! weekly material JSON files, plus exporting / restoring progress records.
//!
//! CSV headers are matched loosely (case-insensitive, several aliases), so a
//! list exported from a spreadsheet or a flashcard app usually works as is.
//! See docs/data-formats.md.

use crate::db::{Db, ItemKind, NewArticle, NewGrammar, NewWord};
use crate::lang::Lang;
use crate::workspace::{files_with_ext, Workspace};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Default, Clone, Serialize)]
pub struct ImportReport {
    pub words: usize,
    pub grammar: usize,
    pub articles: usize,
    pub materials: usize,
    pub lessons: usize,
    pub restored: usize,
    /// Problems that did not stop the import: `file:line: message`.
    pub warnings: Vec<String>,
}

impl ImportReport {
    pub fn merge(&mut self, o: ImportReport) {
        self.words += o.words;
        self.grammar += o.grammar;
        self.articles += o.articles;
        self.materials += o.materials;
        self.lessons += o.lessons;
        self.restored += o.restored;
        self.warnings.extend(o.warnings);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("CSV：{0}")]
    Csv(#[from] csv::Error),
    #[error("JSON：{0}")]
    Json(#[from] serde_json::Error),
    #[error("資料庫：{0}")]
    Db(#[from] sqlx::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
}

const WORD_ALIASES: &[(&str, &[&str])] = &[
    ("lang", &["lang", "language", "語言"]),
    ("term", &["term", "word", "vocab", "vocabulary", "front", "expression", "單字", "單詞", "単語", "語彙"]),
    ("reading", &["reading", "kana", "furigana", "pronunciation", "ipa", "phonetic", "讀音", "読み", "よみ", "發音"]),
    ("meaning", &["meaning", "translation", "definition", "back", "gloss", "中文", "意思", "解釋", "意味"]),
    ("pos", &["pos", "part_of_speech", "part of speech", "詞性", "品詞"]),
    ("example", &["example", "sentence", "例句", "例文"]),
    ("note", &["note", "notes", "memo", "備註", "メモ"]),
    ("tags", &["tags", "tag", "標籤", "タグ"]),
];

const GRAMMAR_ALIASES: &[(&str, &[&str])] = &[
    ("lang", &["lang", "language", "語言"]),
    ("pattern", &["pattern", "grammar", "point", "structure", "文法", "句型", "文型"]),
    ("meaning", &["meaning", "translation", "意思", "意味"]),
    ("explanation", &["explanation", "usage", "description", "說明", "解説", "用法"]),
    ("example", &["example", "sentence", "例句", "例文"]),
    ("level", &["level", "jlpt", "cefr", "程度", "級"]),
    ("tags", &["tags", "tag", "標籤", "タグ"]),
];

/// Map canonical field name -> column index, from the header row.
fn header_map(headers: &csv::StringRecord, aliases: &[(&'static str, &[&str])]) -> HashMap<&'static str, usize> {
    let mut map = HashMap::new();
    for (i, h) in headers.iter().enumerate() {
        let h = h.trim().trim_start_matches('\u{feff}').to_lowercase();
        for (canon, names) in aliases {
            if names.iter().any(|n| *n == h) && !map.contains_key(canon) {
                map.insert(*canon, i);
            }
        }
    }
    map
}

fn field(rec: &csv::StringRecord, map: &HashMap<&'static str, usize>, key: &str) -> String {
    map.get(key).and_then(|&i| rec.get(i)).unwrap_or("").trim().to_string()
}

fn reader(text: &str) -> csv::Reader<&[u8]> {
    // Tab-separated files (common from Anki / spreadsheets) are detected by
    // the header line.
    let first = text.lines().next().unwrap_or("");
    let delim = if first.matches('\t').count() > first.matches(',').count() { b'\t' } else { b',' };
    csv::ReaderBuilder::new().delimiter(delim).flexible(true).trim(csv::Trim::All).from_reader(text.as_bytes())
}

fn row_lang(rec: &csv::StringRecord, map: &HashMap<&'static str, usize>, default: Lang, origin: &str, line: u64, warnings: &mut Vec<String>) -> Lang {
    let raw = field(rec, map, "lang");
    if raw.is_empty() {
        return default;
    }
    raw.parse().unwrap_or_else(|e| {
        warnings.push(format!("{origin}:{line}: {e}，改用 {default}"));
        default
    })
}

pub async fn import_vocab_csv(db: &Db, text: &str, default: Lang, origin: &str) -> Result<ImportReport, ImportError> {
    let mut rdr = reader(text);
    let map = header_map(rdr.headers()?, WORD_ALIASES);
    if !map.contains_key("term") {
        return Err(ImportError::Invalid(format!("{origin}：找不到單字欄位（標題需有 term / word / 單字 其中之一）")));
    }
    let mut rep = ImportReport::default();
    for (i, rec) in rdr.records().enumerate() {
        let line = i as u64 + 2;
        let rec = match rec {
            Ok(r) => r,
            Err(e) => {
                rep.warnings.push(format!("{origin}:{line}: {e}"));
                continue;
            }
        };
        let term = field(&rec, &map, "term");
        if term.is_empty() || term.starts_with('#') {
            continue;
        }
        let lang = row_lang(&rec, &map, default, origin, line, &mut rep.warnings);
        let w = NewWord {
            lang: Some(lang),
            term,
            reading: field(&rec, &map, "reading"),
            meaning: field(&rec, &map, "meaning"),
            pos: field(&rec, &map, "pos"),
            example: field(&rec, &map, "example"),
            note: field(&rec, &map, "note"),
            tags: field(&rec, &map, "tags"),
            source: origin.to_string(),
        };
        db.upsert_word(&w, lang).await?;
        rep.words += 1;
    }
    Ok(rep)
}

pub async fn import_grammar_csv(db: &Db, text: &str, default: Lang, origin: &str) -> Result<ImportReport, ImportError> {
    let mut rdr = reader(text);
    let map = header_map(rdr.headers()?, GRAMMAR_ALIASES);
    if !map.contains_key("pattern") {
        return Err(ImportError::Invalid(format!("{origin}：找不到文法欄位（標題需有 pattern / grammar / 文法 其中之一）")));
    }
    let mut rep = ImportReport::default();
    for (i, rec) in rdr.records().enumerate() {
        let line = i as u64 + 2;
        let rec = match rec {
            Ok(r) => r,
            Err(e) => {
                rep.warnings.push(format!("{origin}:{line}: {e}"));
                continue;
            }
        };
        let pattern = field(&rec, &map, "pattern");
        if pattern.is_empty() || pattern.starts_with('#') {
            continue;
        }
        let lang = row_lang(&rec, &map, default, origin, line, &mut rep.warnings);
        let g = NewGrammar {
            lang: Some(lang),
            pattern,
            meaning: field(&rec, &map, "meaning"),
            explanation: field(&rec, &map, "explanation"),
            example: field(&rec, &map, "example"),
            level: field(&rec, &map, "level"),
            tags: field(&rec, &map, "tags"),
            source: origin.to_string(),
        };
        db.upsert_grammar(&g, lang).await?;
        rep.grammar += 1;
    }
    Ok(rep)
}

/// A week's worth of study material, collected by the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Material {
    /// ISO week, e.g. `2026-W45`.
    pub week: String,
    #[serde(default)]
    pub title: String,
    pub language: Lang,
    #[serde(default)]
    pub items: Vec<MaterialItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum MaterialItem {
    Word {
        term: String,
        #[serde(default)]
        reading: String,
        #[serde(default)]
        meaning: String,
        #[serde(default)]
        example: String,
        #[serde(default)]
        tags: String,
    },
    Grammar {
        pattern: String,
        #[serde(default)]
        meaning: String,
        #[serde(default)]
        explanation: String,
        #[serde(default)]
        example: String,
    },
    Article {
        title: String,
        #[serde(default)]
        url: String,
        #[serde(default)]
        text: String,
    },
    Note {
        text: String,
    },
}

pub fn parse_material(text: &str) -> Result<Material, ImportError> {
    let m: Material = serde_json::from_str(text)?;
    let ok_week = m.week.len() == 8 && m.week.as_bytes()[4] == b'-' && m.week.as_bytes()[5] == b'W';
    if !ok_week {
        return Err(ImportError::Invalid(format!("week 需為 ISO 週，例如 2026-W45（收到 {:?}）", m.week)));
    }
    Ok(m)
}

pub async fn import_material(db: &Db, text: &str, path: &str) -> Result<ImportReport, ImportError> {
    let m = parse_material(text)?;
    let lang = m.language;
    let mut rep = ImportReport::default();
    for (i, item) in m.items.iter().enumerate() {
        match item {
            MaterialItem::Word { term, reading, meaning, example, tags } => {
                let w = NewWord {
                    lang: Some(lang),
                    term: term.clone(),
                    reading: reading.clone(),
                    meaning: meaning.clone(),
                    example: example.clone(),
                    tags: tags.clone(),
                    source: path.to_string(),
                    ..Default::default()
                };
                db.upsert_word(&w, lang).await?;
                rep.words += 1;
            }
            MaterialItem::Grammar { pattern, meaning, explanation, example } => {
                let g = NewGrammar {
                    lang: Some(lang),
                    pattern: pattern.clone(),
                    meaning: meaning.clone(),
                    explanation: explanation.clone(),
                    example: example.clone(),
                    source: path.to_string(),
                    ..Default::default()
                };
                db.upsert_grammar(&g, lang).await?;
                rep.grammar += 1;
            }
            MaterialItem::Article { title, url, text } => {
                // Articles without a URL get a stable local one so they can
                // still be turned into lessons.
                let url = if url.trim().is_empty() { format!("material://{path}#{i}") } else { url.clone() };
                let a = NewArticle {
                    lang: Some(lang),
                    source: format!("materials {}", m.week),
                    url,
                    title: title.clone(),
                    body: text.clone(),
                    ..Default::default()
                };
                db.upsert_article(&a, lang).await?;
                rep.articles += 1;
            }
            MaterialItem::Note { .. } => {}
        }
    }
    db.upsert_material(&m.week, lang, &m.title, path, m.items.len() as i64, text).await?;
    rep.materials = 1;
    Ok(rep)
}

/// Default language for a vocab / grammar file: from its name
/// (`*-ja.csv`, `ja/…`) or the workspace's first language.
pub fn lang_from_path(path: &Path, fallback: Lang) -> Lang {
    let s = path.to_string_lossy().to_lowercase();
    let has = |code: &str| {
        s.contains(&format!("/{code}/")) || s.contains(&format!("-{code}.")) || s.contains(&format!("_{code}.")) || s.ends_with(&format!("/{code}.csv"))
    };
    if has("ja") || s.contains("japanese") {
        Lang::Ja
    } else if has("en") || s.contains("english") {
        Lang::En
    } else {
        fallback
    }
}

/// Re-read every file in the workspace into the database (idempotent).
pub async fn sync_workspace(ws: &Workspace, db: &Db) -> ImportReport {
    let mut rep = ImportReport::default();
    let fallback = ws.profile.languages.first().copied().unwrap_or(Lang::En);
    for path in files_with_ext(&ws.vocab_dir(), &["csv", "tsv"]) {
        let origin = ws.relative(&path);
        let r = match std::fs::read_to_string(&path) {
            Ok(t) => import_vocab_csv(db, &t, lang_from_path(&path, fallback), &origin).await,
            Err(e) => Err(e.into()),
        };
        absorb(&mut rep, r, &origin);
    }
    for path in files_with_ext(&ws.grammar_dir(), &["csv", "tsv"]) {
        let origin = ws.relative(&path);
        let r = match std::fs::read_to_string(&path) {
            Ok(t) => import_grammar_csv(db, &t, lang_from_path(&path, fallback), &origin).await,
            Err(e) => Err(e.into()),
        };
        absorb(&mut rep, r, &origin);
    }
    for path in files_with_ext(&ws.materials_dir(), &["json"]) {
        let origin = ws.relative(&path);
        let r = match std::fs::read_to_string(&path) {
            Ok(t) => import_material(db, &t, &origin).await,
            Err(e) => Err(e.into()),
        };
        absorb(&mut rep, r, &origin);
    }
    for lang in Lang::ALL {
        for path in files_with_ext(&ws.lessons_dir(lang), &["md"]) {
            let origin = ws.relative(&path);
            absorb(&mut rep, import_lesson_file(db, &path, &origin, lang).await, &origin);
        }
    }
    rep
}

/// A lesson Markdown file written by JELearner (with embedded data) that the
/// database does not know yet, e.g. generated on another machine.
async fn import_lesson_file(db: &Db, path: &Path, origin: &str, lang: Lang) -> Result<ImportReport, ImportError> {
    use crate::lesson::{extract_data, front_matter};
    let mut rep = ImportReport::default();
    if db.lesson_by_path(origin).await?.is_some() {
        return Ok(rep);
    }
    let md = std::fs::read_to_string(path)?;
    let Some(lesson) = extract_data(&md) else { return Ok(rep) };
    let level = front_matter(&md, "level").unwrap_or_default();
    let url = front_matter(&md, "source").unwrap_or_default();
    let engine = front_matter(&md, "engine").unwrap_or_default();
    let data = serde_json::to_value(&lesson)?;
    db.insert_lesson(lang, &lesson.title, &level, None, &url, origin, &md, &data, &engine).await?;
    rep.lessons += 1;
    Ok(rep)
}

fn absorb(rep: &mut ImportReport, r: Result<ImportReport, ImportError>, origin: &str) {
    match r {
        Ok(r) => rep.merge(r),
        Err(e) => rep.warnings.push(format!("{origin}: {e}")),
    }
}

// ── Records: progress and mistakes as CSV ────────────────

#[derive(Debug, Serialize, Deserialize)]
struct ProgressRecord {
    kind: String,
    lang: String,
    key: String,
    reading: String,
    ease: f64,
    interval_days: i64,
    reps: i64,
    lapses: i64,
    due_at: String,
    last_reviewed_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct MistakeRecord {
    lang: String,
    question: String,
    user_answer: String,
    correct_answer: String,
    explanation: String,
    created_at: String,
    resolved_at: String,
}

/// Write `records/progress.csv` and `records/mistakes.csv`. Items are keyed
/// by their text (term + reading, or pattern), not by database id, so the
/// files restore into any machine's database.
pub async fn export_records(ws: &Workspace, db: &Db) -> Result<(usize, usize), ImportError> {
    use sqlx::Row;
    std::fs::create_dir_all(ws.records_dir())?;
    let rows = sqlx::query(
        "SELECT p.item_kind AS kind, COALESCE(w.lang, g.lang) AS lang, COALESCE(w.term, g.pattern) AS key,
                COALESCE(w.reading, '') AS reading, p.ease, p.interval_days, p.reps, p.lapses,
                p.due_at, COALESCE(p.last_reviewed_at, '') AS last
         FROM progress p
         LEFT JOIN words w ON p.item_kind = 'word' AND p.item_id = w.id
         LEFT JOIN grammar g ON p.item_kind = 'grammar' AND p.item_id = g.id
         WHERE p.reps > 0 OR p.lapses > 0
         ORDER BY kind, lang, key",
    )
    .fetch_all(db.pool())
    .await?;
    let mut w = csv::Writer::from_path(ws.records_dir().join("progress.csv"))?;
    for r in &rows {
        w.serialize(ProgressRecord {
            kind: r.get("kind"),
            lang: r.get("lang"),
            key: r.get("key"),
            reading: r.get("reading"),
            ease: r.get("ease"),
            interval_days: r.get("interval_days"),
            reps: r.get("reps"),
            lapses: r.get("lapses"),
            due_at: r.get("due_at"),
            last_reviewed_at: r.get("last"),
        })?;
    }
    w.flush()?;
    let mistakes = db.list_mistakes(None, true).await?;
    let mut m = csv::Writer::from_path(ws.records_dir().join("mistakes.csv"))?;
    for x in &mistakes {
        m.serialize(MistakeRecord {
            lang: x.lang.clone(),
            question: x.question.clone(),
            user_answer: x.user_answer.clone(),
            correct_answer: x.correct_answer.clone(),
            explanation: x.explanation.clone(),
            created_at: x.created_at.to_rfc3339(),
            resolved_at: x.resolved_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
        })?;
    }
    m.flush()?;
    Ok((rows.len(), mistakes.len()))
}

/// Restore exported records into a database that has none yet (a fresh
/// machine). Does nothing when progress or mistakes already exist.
pub async fn restore_records(ws: &Workspace, db: &Db) -> Result<ImportReport, ImportError> {
    let mut rep = ImportReport::default();
    let has_progress: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM progress WHERE reps > 0 OR lapses > 0").fetch_one(db.pool()).await?;
    let progress = ws.records_dir().join("progress.csv");
    if has_progress == 0 && progress.exists() {
        let mut rdr = csv::Reader::from_path(&progress)?;
        for rec in rdr.deserialize::<ProgressRecord>() {
            let Ok(r) = rec else { continue };
            let id: Option<i64> = if r.kind == ItemKind::Word.as_str() {
                sqlx::query_scalar("SELECT id FROM words WHERE lang = ? AND term = ? AND reading = ?")
                    .bind(&r.lang)
                    .bind(&r.key)
                    .bind(&r.reading)
                    .fetch_optional(db.pool())
                    .await?
            } else {
                sqlx::query_scalar("SELECT id FROM grammar WHERE lang = ? AND pattern = ?").bind(&r.lang).bind(&r.key).fetch_optional(db.pool()).await?
            };
            let Some(id) = id else { continue };
            sqlx::query(
                "UPDATE progress SET ease = ?, interval_days = ?, reps = ?, lapses = ?, due_at = ?,
                 last_reviewed_at = NULLIF(?, '') WHERE item_kind = ? AND item_id = ?",
            )
            .bind(r.ease)
            .bind(r.interval_days)
            .bind(r.reps)
            .bind(r.lapses)
            .bind(&r.due_at)
            .bind(&r.last_reviewed_at)
            .bind(&r.kind)
            .bind(id)
            .execute(db.pool())
            .await?;
            rep.restored += 1;
        }
    }
    let has_mistakes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mistakes").fetch_one(db.pool()).await?;
    let mistakes = ws.records_dir().join("mistakes.csv");
    if has_mistakes == 0 && mistakes.exists() {
        let mut rdr = csv::Reader::from_path(&mistakes)?;
        for rec in rdr.deserialize::<MistakeRecord>() {
            let Ok(r) = rec else { continue };
            sqlx::query(
                "INSERT INTO mistakes (lang, question, user_answer, correct_answer, explanation, created_at, resolved_at)
                 VALUES (?, ?, ?, ?, ?, ?, NULLIF(?, ''))",
            )
            .bind(&r.lang)
            .bind(&r.question)
            .bind(&r.user_answer)
            .bind(&r.correct_answer)
            .bind(&r.explanation)
            .bind(&r.created_at)
            .bind(&r.resolved_at)
            .execute(db.pool())
            .await?;
            rep.restored += 1;
        }
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn vocab_csv_with_aliases_and_tabs() {
        let db = Db::memory().await.unwrap();
        let csv = "Word\tReading\t中文\ttags\n挑戦\tちょうせん\t挑戰\tN3\n\n# comment\t\t\t\n";
        let rep = import_vocab_csv(&db, csv, Lang::Ja, "vocab/mine.tsv").await.unwrap();
        assert_eq!(rep.words, 1);
        let w = &db.list_words(Some(Lang::Ja), "", 10).await.unwrap()[0];
        assert_eq!((w.term.as_str(), w.reading.as_str(), w.meaning.as_str(), w.tags.as_str()), ("挑戦", "ちょうせん", "挑戰", "N3"));
    }

    #[tokio::test]
    async fn vocab_csv_per_row_language() {
        let db = Db::memory().await.unwrap();
        let csv = "lang,term,meaning\nen,resilient,有韌性的\nja,粘り強い,有韌性的\nfr,bonjour,你好\n";
        let rep = import_vocab_csv(&db, csv, Lang::En, "x.csv").await.unwrap();
        assert_eq!(rep.words, 3);
        assert_eq!(rep.warnings.len(), 1, "unknown language is reported, not fatal");
        assert_eq!(db.list_words(Some(Lang::Ja), "", 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn missing_term_column_is_an_error() {
        let db = Db::memory().await.unwrap();
        assert!(import_vocab_csv(&db, "a,b\n1,2\n", Lang::En, "x.csv").await.is_err());
    }

    #[tokio::test]
    async fn material_json_imports_everything() {
        let db = Db::memory().await.unwrap();
        let json = r#"{"week":"2026-W45","title":"AI 新聞週","language":"ja","items":[
            {"kind":"word","term":"半導体","reading":"はんどうたい","meaning":"半導體"},
            {"kind":"grammar","pattern":"〜に伴って","meaning":"隨著"},
            {"kind":"article","title":"メモ","text":"本文です。"},
            {"kind":"note","text":"自分用"}]}"#;
        let rep = import_material(&db, json, "materials/2026-W45/ai.json").await.unwrap();
        assert_eq!((rep.words, rep.grammar, rep.articles, rep.materials), (1, 1, 1, 1));
        assert_eq!(db.list_materials().await.unwrap()[0].items, 4);
        assert!(parse_material(r#"{"week":"45","language":"ja"}"#).is_err());
    }

    #[test]
    fn language_from_file_name() {
        assert_eq!(lang_from_path(Path::new("vocab/jlpt-ja.csv"), Lang::En), Lang::Ja);
        assert_eq!(lang_from_path(Path::new("vocab/en/toeic.csv"), Lang::Ja), Lang::En);
        assert_eq!(lang_from_path(Path::new("vocab/misc.csv"), Lang::Ja), Lang::Ja);
    }
}
