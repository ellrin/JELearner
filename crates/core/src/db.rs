//! SQLite: words, grammar, review progress, mistakes, lessons, articles and
//! imported materials.
//!
//! One database per private workspace, stored outside every repository
//! (`<config dir>/JELearner/data/<workspace id>/learner.sqlite`). Words and
//! grammar written in the workspace files are imported idempotently, so the
//! database can always be rebuilt; progress and mistakes are exported to
//! `records/` for backup and for a second machine.

use crate::lang::Lang;
use crate::srs::{self, Card};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use std::str::FromStr;

pub type DbResult<T> = Result<T, sqlx::Error>;

/// Schema versions, applied in order and tracked with `PRAGMA user_version`.
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE words (
    id          INTEGER PRIMARY KEY,
    lang        TEXT NOT NULL,
    term        TEXT NOT NULL,
    reading     TEXT NOT NULL DEFAULT '',
    meaning     TEXT NOT NULL DEFAULT '',
    pos         TEXT NOT NULL DEFAULT '',
    example     TEXT NOT NULL DEFAULT '',
    note        TEXT NOT NULL DEFAULT '',
    tags        TEXT NOT NULL DEFAULT '',
    source      TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    UNIQUE (lang, term, reading)
);
CREATE TABLE grammar (
    id          INTEGER PRIMARY KEY,
    lang        TEXT NOT NULL,
    pattern     TEXT NOT NULL,
    meaning     TEXT NOT NULL DEFAULT '',
    explanation TEXT NOT NULL DEFAULT '',
    example     TEXT NOT NULL DEFAULT '',
    level       TEXT NOT NULL DEFAULT '',
    tags        TEXT NOT NULL DEFAULT '',
    source      TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    UNIQUE (lang, pattern)
);
CREATE TABLE progress (
    item_kind        TEXT NOT NULL CHECK (item_kind IN ('word', 'grammar')),
    item_id          INTEGER NOT NULL,
    ease             REAL NOT NULL DEFAULT 2.5,
    interval_days    INTEGER NOT NULL DEFAULT 0,
    reps             INTEGER NOT NULL DEFAULT 0,
    lapses           INTEGER NOT NULL DEFAULT 0,
    due_at           TEXT NOT NULL,
    last_reviewed_at TEXT,
    PRIMARY KEY (item_kind, item_id)
);
CREATE TABLE reviews (
    id          INTEGER PRIMARY KEY,
    item_kind   TEXT NOT NULL,
    item_id     INTEGER NOT NULL,
    grade       INTEGER NOT NULL,
    reviewed_at TEXT NOT NULL
);
CREATE TABLE articles (
    id           INTEGER PRIMARY KEY,
    lang         TEXT NOT NULL,
    source       TEXT NOT NULL DEFAULT '',
    url          TEXT NOT NULL UNIQUE,
    title        TEXT NOT NULL DEFAULT '',
    summary      TEXT NOT NULL DEFAULT '',
    body         TEXT NOT NULL DEFAULT '',
    published_at TEXT,
    fetched_at   TEXT NOT NULL
);
CREATE TABLE lessons (
    id          INTEGER PRIMARY KEY,
    lang        TEXT NOT NULL,
    title       TEXT NOT NULL,
    level       TEXT NOT NULL DEFAULT '',
    article_id  INTEGER REFERENCES articles(id) ON DELETE SET NULL,
    source_url  TEXT NOT NULL DEFAULT '',
    path        TEXT NOT NULL DEFAULT '',
    markdown    TEXT NOT NULL,
    data        TEXT NOT NULL,
    engine      TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL
);
CREATE TABLE mistakes (
    id             INTEGER PRIMARY KEY,
    lang           TEXT NOT NULL,
    item_kind      TEXT NOT NULL DEFAULT '',
    item_id        INTEGER,
    lesson_id      INTEGER REFERENCES lessons(id) ON DELETE SET NULL,
    question       TEXT NOT NULL,
    user_answer    TEXT NOT NULL DEFAULT '',
    correct_answer TEXT NOT NULL DEFAULT '',
    explanation    TEXT NOT NULL DEFAULT '',
    created_at     TEXT NOT NULL,
    resolved_at    TEXT
);
CREATE TABLE materials (
    id          INTEGER PRIMARY KEY,
    week        TEXT NOT NULL,
    lang        TEXT NOT NULL,
    title       TEXT NOT NULL DEFAULT '',
    path        TEXT NOT NULL UNIQUE,
    items       INTEGER NOT NULL DEFAULT 0,
    data        TEXT NOT NULL,
    imported_at TEXT NOT NULL
);
CREATE INDEX progress_due ON progress (due_at);
CREATE INDEX mistakes_open ON mistakes (lang, resolved_at);
"#, r#"
-- Stage A (docs/plan.md §17): points, units, conversations, word usage,
-- topic stats, difficulty changes, passive time.
CREATE TABLE points (
    id      INTEGER PRIMARY KEY,
    at      TEXT NOT NULL,            -- UTC, for comparisons
    time    TEXT NOT NULL,            -- as written in records/points (with offset)
    day     TEXT NOT NULL,            -- local date of `time`
    lang    TEXT NOT NULL,
    unit    TEXT NOT NULL,
    skill   TEXT NOT NULL,
    points  REAL NOT NULL,
    score   REAL,
    detail  TEXT NOT NULL DEFAULT '',
    UNIQUE (time, lang, unit, skill, detail)
);
CREATE INDEX points_lang_at ON points (lang, at);
CREATE TABLE units (
    id         INTEGER PRIMARY KEY,
    lang       TEXT NOT NULL,
    kind       TEXT NOT NULL,
    skill      TEXT NOT NULL,
    ref        TEXT NOT NULL DEFAULT '',
    started_at TEXT,
    ended_at   TEXT NOT NULL,
    passed     INTEGER NOT NULL,
    score      REAL,
    duration_s INTEGER,
    points     REAL NOT NULL DEFAULT 0,
    UNIQUE (ended_at, lang, kind, ref)
);
CREATE INDEX units_lang_end ON units (lang, ended_at);
CREATE TABLE conversations (
    id          INTEGER PRIMARY KEY,
    lang        TEXT NOT NULL,
    topic_id    TEXT NOT NULL DEFAULT '',
    topic       TEXT NOT NULL DEFAULT '',
    topic_kind  TEXT NOT NULL DEFAULT 'general',
    status      TEXT NOT NULL DEFAULT 'active',
    turns_done  INTEGER NOT NULL DEFAULT 0,
    turns_limit INTEGER NOT NULL,
    score       REAL,
    passed      INTEGER,
    points      REAL,
    path        TEXT NOT NULL DEFAULT '',
    data        TEXT NOT NULL,
    started_at  TEXT NOT NULL,
    ended_at    TEXT
);
CREATE TABLE word_usage (
    lang         TEXT NOT NULL,
    term         TEXT NOT NULL,
    used         INTEGER NOT NULL DEFAULT 0,
    targeted     INTEGER NOT NULL DEFAULT 0,
    last_used_at TEXT,
    PRIMARY KEY (lang, term)
);
CREATE TABLE topic_stats (
    lang      TEXT NOT NULL,
    topic_id  TEXT NOT NULL,
    topic     TEXT NOT NULL DEFAULT '',
    kind      TEXT NOT NULL DEFAULT 'general',
    count     INTEGER NOT NULL DEFAULT 0,
    score_sum REAL NOT NULL DEFAULT 0,
    scored    INTEGER NOT NULL DEFAULT 0,
    last_at   TEXT,
    PRIMARY KEY (lang, topic_id)
);
CREATE TABLE difficulty_log (
    id         INTEGER PRIMARY KEY,
    lang       TEXT NOT NULL,
    from_score INTEGER NOT NULL,
    to_score   INTEGER NOT NULL,
    reason     TEXT NOT NULL,
    at         TEXT NOT NULL
);
CREATE TABLE listening_time (
    id      INTEGER PRIMARY KEY,
    lang    TEXT NOT NULL,
    kind    TEXT NOT NULL,
    seconds INTEGER NOT NULL,
    at      TEXT NOT NULL
);
ALTER TABLE lessons ADD COLUMN topic TEXT NOT NULL DEFAULT '';
ALTER TABLE lessons ADD COLUMN difficulty INTEGER;
ALTER TABLE lessons ADD COLUMN recipe TEXT NOT NULL DEFAULT '';
"#];

#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NewWord {
    pub lang: Option<Lang>,
    pub term: String,
    #[serde(default)]
    pub reading: String,
    #[serde(default)]
    pub meaning: String,
    #[serde(default)]
    pub pos: String,
    #[serde(default)]
    pub example: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Word {
    pub id: i64,
    pub lang: String,
    pub term: String,
    pub reading: String,
    pub meaning: String,
    pub pos: String,
    pub example: String,
    pub note: String,
    pub tags: String,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub due_at: Option<DateTime<Utc>>,
    pub reps: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NewGrammar {
    pub lang: Option<Lang>,
    pub pattern: String,
    #[serde(default)]
    pub meaning: String,
    #[serde(default)]
    pub explanation: String,
    #[serde(default)]
    pub example: String,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Grammar {
    pub id: i64,
    pub lang: String,
    pub pattern: String,
    pub meaning: String,
    pub explanation: String,
    pub example: String,
    pub level: String,
    pub tags: String,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub due_at: Option<DateTime<Utc>>,
    pub reps: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Word,
    Grammar,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Word => "word",
            ItemKind::Grammar => "grammar",
        }
    }
}

/// A card due for review, with what to show on its front and back.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct DueItem {
    pub item_kind: String,
    pub item_id: i64,
    pub lang: String,
    pub front: String,
    pub reading: String,
    pub back: String,
    pub example: String,
    pub due_at: DateTime<Utc>,
    pub reps: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NewMistake {
    pub lang: Option<Lang>,
    #[serde(default)]
    pub item_kind: String,
    pub item_id: Option<i64>,
    pub lesson_id: Option<i64>,
    pub question: String,
    #[serde(default)]
    pub user_answer: String,
    #[serde(default)]
    pub correct_answer: String,
    #[serde(default)]
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Mistake {
    pub id: i64,
    pub lang: String,
    pub item_kind: String,
    pub item_id: Option<i64>,
    pub lesson_id: Option<i64>,
    pub question: String,
    pub user_answer: String,
    pub correct_answer: String,
    pub explanation: String,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NewArticle {
    pub lang: Option<Lang>,
    #[serde(default)]
    pub source: String,
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub body: String,
    pub published_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Article {
    pub id: i64,
    pub lang: String,
    pub source: String,
    pub url: String,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub published_at: Option<DateTime<Utc>>,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct LessonRow {
    pub id: i64,
    pub lang: String,
    pub title: String,
    pub level: String,
    pub article_id: Option<i64>,
    pub source_url: String,
    pub path: String,
    pub markdown: String,
    pub data: String,
    pub engine: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct MaterialRow {
    pub id: i64,
    pub week: String,
    pub lang: String,
    pub title: String,
    pub path: String,
    pub items: i64,
    pub imported_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Stats {
    pub words: i64,
    pub grammar: i64,
    pub due: i64,
    pub lessons: i64,
    pub open_mistakes: i64,
    /// Since the `since` argument (e.g. the last 7 days).
    pub new_words: i64,
    pub new_grammar: i64,
    pub reviews: i64,
    pub reviews_correct: i64,
    pub new_lessons: i64,
    pub new_mistakes: i64,
}

fn lang_str(l: Option<Lang>, fallback: Lang) -> &'static str {
    l.unwrap_or(fallback).code()
}

impl Db {
    /// Open (or create) the database file and bring the schema up to date.
    pub async fn open(path: &Path) -> DbResult<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(sqlx::Error::Io)?;
        }
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new().max_connections(4).connect_with(opts).await?;
        let db = Db { pool };
        db.migrate().await?;
        Ok(db)
    }

    /// In-memory database for tests (one connection, so it stays the same DB).
    pub async fn memory() -> DbResult<Db> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?;
        let db = Db { pool };
        db.migrate().await?;
        Ok(db)
    }

    async fn migrate(&self) -> DbResult<()> {
        let version: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(&self.pool).await?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            // One script per version, in its own transaction.
            let script = format!("BEGIN;\n{sql}\nPRAGMA user_version = {};\nCOMMIT;", i + 1);
            sqlx::raw_sql(&script).execute(&self.pool).await?;
        }
        Ok(())
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    // ── Words ───────────────────────────────────────────

    /// Insert or update by (lang, term, reading). Empty fields never erase
    /// existing ones, so re-importing a sparse CSV keeps richer data.
    pub async fn upsert_word(&self, w: &NewWord, fallback: Lang) -> DbResult<i64> {
        let now = Utc::now();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO words (lang, term, reading, meaning, pos, example, note, tags, source, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (lang, term, reading) DO UPDATE SET
               meaning = CASE WHEN excluded.meaning <> '' THEN excluded.meaning ELSE words.meaning END,
               pos     = CASE WHEN excluded.pos     <> '' THEN excluded.pos     ELSE words.pos END,
               example = CASE WHEN excluded.example <> '' THEN excluded.example ELSE words.example END,
               note    = CASE WHEN excluded.note    <> '' THEN excluded.note    ELSE words.note END,
               tags    = CASE WHEN excluded.tags    <> '' THEN excluded.tags    ELSE words.tags END
             RETURNING id",
        )
        .bind(lang_str(w.lang, fallback))
        .bind(w.term.trim())
        .bind(w.reading.trim())
        .bind(w.meaning.trim())
        .bind(w.pos.trim())
        .bind(w.example.trim())
        .bind(w.note.trim())
        .bind(w.tags.trim())
        .bind(w.source.trim())
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        self.ensure_card(ItemKind::Word, id, now).await?;
        Ok(id)
    }

    pub async fn list_words(&self, lang: Option<Lang>, query: &str, limit: i64) -> DbResult<Vec<Word>> {
        let like = format!("%{}%", query.trim());
        sqlx::query_as::<_, Word>(
            "SELECT w.*, p.due_at, p.reps FROM words w
             LEFT JOIN progress p ON p.item_kind = 'word' AND p.item_id = w.id
             WHERE (?1 IS NULL OR w.lang = ?1)
               AND (?2 = '%%' OR w.term LIKE ?2 OR w.reading LIKE ?2 OR w.meaning LIKE ?2 OR w.tags LIKE ?2)
             ORDER BY w.created_at DESC, w.id DESC LIMIT ?3",
        )
        .bind(lang.map(Lang::code))
        .bind(like)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn delete_word(&self, id: i64) -> DbResult<bool> {
        sqlx::query("DELETE FROM progress WHERE item_kind = 'word' AND item_id = ?").bind(id).execute(&self.pool).await?;
        Ok(sqlx::query("DELETE FROM words WHERE id = ?").bind(id).execute(&self.pool).await?.rows_affected() > 0)
    }

    // ── Grammar ─────────────────────────────────────────

    pub async fn upsert_grammar(&self, g: &NewGrammar, fallback: Lang) -> DbResult<i64> {
        let now = Utc::now();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO grammar (lang, pattern, meaning, explanation, example, level, tags, source, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (lang, pattern) DO UPDATE SET
               meaning     = CASE WHEN excluded.meaning     <> '' THEN excluded.meaning     ELSE grammar.meaning END,
               explanation = CASE WHEN excluded.explanation <> '' THEN excluded.explanation ELSE grammar.explanation END,
               example     = CASE WHEN excluded.example     <> '' THEN excluded.example     ELSE grammar.example END,
               level       = CASE WHEN excluded.level       <> '' THEN excluded.level       ELSE grammar.level END,
               tags        = CASE WHEN excluded.tags        <> '' THEN excluded.tags        ELSE grammar.tags END
             RETURNING id",
        )
        .bind(lang_str(g.lang, fallback))
        .bind(g.pattern.trim())
        .bind(g.meaning.trim())
        .bind(g.explanation.trim())
        .bind(g.example.trim())
        .bind(g.level.trim())
        .bind(g.tags.trim())
        .bind(g.source.trim())
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        self.ensure_card(ItemKind::Grammar, id, now).await?;
        Ok(id)
    }

    pub async fn list_grammar(&self, lang: Option<Lang>, query: &str, limit: i64) -> DbResult<Vec<Grammar>> {
        let like = format!("%{}%", query.trim());
        sqlx::query_as::<_, Grammar>(
            "SELECT g.*, p.due_at, p.reps FROM grammar g
             LEFT JOIN progress p ON p.item_kind = 'grammar' AND p.item_id = g.id
             WHERE (?1 IS NULL OR g.lang = ?1)
               AND (?2 = '%%' OR g.pattern LIKE ?2 OR g.meaning LIKE ?2 OR g.explanation LIKE ?2 OR g.tags LIKE ?2)
             ORDER BY g.created_at DESC, g.id DESC LIMIT ?3",
        )
        .bind(lang.map(Lang::code))
        .bind(like)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn delete_grammar(&self, id: i64) -> DbResult<bool> {
        sqlx::query("DELETE FROM progress WHERE item_kind = 'grammar' AND item_id = ?").bind(id).execute(&self.pool).await?;
        Ok(sqlx::query("DELETE FROM grammar WHERE id = ?").bind(id).execute(&self.pool).await?.rows_affected() > 0)
    }

    // ── Review (spaced repetition) ──────────────────────

    async fn ensure_card(&self, kind: ItemKind, id: i64, now: DateTime<Utc>) -> DbResult<()> {
        sqlx::query("INSERT OR IGNORE INTO progress (item_kind, item_id, due_at) VALUES (?, ?, ?)")
            .bind(kind.as_str())
            .bind(id)
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn due_items(&self, lang: Option<Lang>, now: DateTime<Utc>, limit: i64) -> DbResult<Vec<DueItem>> {
        sqlx::query_as::<_, DueItem>(
            "SELECT * FROM (
               SELECT 'word' AS item_kind, w.id AS item_id, w.lang, w.term AS front, w.reading,
                      w.meaning AS back, w.example, p.due_at, p.reps
               FROM progress p JOIN words w ON p.item_kind = 'word' AND p.item_id = w.id
               UNION ALL
               SELECT 'grammar', g.id, g.lang, g.pattern, '', g.meaning || CASE WHEN g.explanation <> '' THEN '｜' || g.explanation ELSE '' END,
                      g.example, p.due_at, p.reps
               FROM progress p JOIN grammar g ON p.item_kind = 'grammar' AND p.item_id = g.id
             ) WHERE (?1 IS NULL OR lang = ?1) AND due_at <= ?2
             ORDER BY due_at, item_id LIMIT ?3",
        )
        .bind(lang.map(Lang::code))
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    /// Due counts per calendar day (UTC) for the next `days` days, for
    /// scheduling review sessions on the calendar.
    pub async fn due_by_day(&self, lang: Option<Lang>, now: DateTime<Utc>, days: i64) -> DbResult<Vec<(String, i64)>> {
        let until = now + Duration::days(days);
        let rows = sqlx::query(
            "SELECT substr(p.due_at, 1, 10) AS day, COUNT(*) AS n
             FROM progress p
             LEFT JOIN words w ON p.item_kind = 'word' AND p.item_id = w.id
             LEFT JOIN grammar g ON p.item_kind = 'grammar' AND p.item_id = g.id
             WHERE p.due_at <= ?2 AND (?1 IS NULL OR COALESCE(w.lang, g.lang) = ?1)
             GROUP BY day ORDER BY day",
        )
        .bind(lang.map(Lang::code))
        .bind(until)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| (r.get::<String, _>("day"), r.get::<i64, _>("n"))).collect())
    }

    pub async fn record_review(&self, kind: ItemKind, id: i64, grade: u8, now: DateTime<Utc>) -> DbResult<Card> {
        self.ensure_card(kind, id, now).await?;
        let row = sqlx::query("SELECT ease, interval_days, reps, lapses FROM progress WHERE item_kind = ? AND item_id = ?")
            .bind(kind.as_str())
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        let card = Card {
            ease: row.get("ease"),
            interval_days: row.get("interval_days"),
            reps: row.get("reps"),
            lapses: row.get("lapses"),
        };
        let next = srs::review(card, grade);
        let due = now + Duration::days(next.interval_days.max(0));
        sqlx::query(
            "UPDATE progress SET ease = ?, interval_days = ?, reps = ?, lapses = ?, due_at = ?, last_reviewed_at = ?
             WHERE item_kind = ? AND item_id = ?",
        )
        .bind(next.ease)
        .bind(next.interval_days)
        .bind(next.reps)
        .bind(next.lapses)
        .bind(due)
        .bind(now)
        .bind(kind.as_str())
        .bind(id)
        .execute(&self.pool)
        .await?;
        sqlx::query("INSERT INTO reviews (item_kind, item_id, grade, reviewed_at) VALUES (?, ?, ?, ?)")
            .bind(kind.as_str())
            .bind(id)
            .bind(grade as i64)
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(next)
    }

    // ── Mistakes ────────────────────────────────────────

    pub async fn add_mistake(&self, m: &NewMistake, fallback: Lang) -> DbResult<i64> {
        sqlx::query_scalar(
            "INSERT INTO mistakes (lang, item_kind, item_id, lesson_id, question, user_answer, correct_answer, explanation, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(lang_str(m.lang, fallback))
        .bind(&m.item_kind)
        .bind(m.item_id)
        .bind(m.lesson_id)
        .bind(&m.question)
        .bind(&m.user_answer)
        .bind(&m.correct_answer)
        .bind(&m.explanation)
        .bind(Utc::now())
        .fetch_one(&self.pool)
        .await
    }

    pub async fn list_mistakes(&self, lang: Option<Lang>, include_resolved: bool) -> DbResult<Vec<Mistake>> {
        sqlx::query_as::<_, Mistake>(
            "SELECT * FROM mistakes WHERE (?1 IS NULL OR lang = ?1) AND (?2 OR resolved_at IS NULL)
             ORDER BY created_at DESC, id DESC",
        )
        .bind(lang.map(Lang::code))
        .bind(include_resolved)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn resolve_mistake(&self, id: i64) -> DbResult<bool> {
        Ok(sqlx::query("UPDATE mistakes SET resolved_at = ? WHERE id = ? AND resolved_at IS NULL")
            .bind(Utc::now())
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected()
            > 0)
    }

    // ── Articles ────────────────────────────────────────

    /// Insert or refresh by URL; an empty body never replaces a fetched one.
    pub async fn upsert_article(&self, a: &NewArticle, fallback: Lang) -> DbResult<i64> {
        sqlx::query_scalar(
            "INSERT INTO articles (lang, source, url, title, summary, body, published_at, fetched_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (url) DO UPDATE SET
               title   = CASE WHEN excluded.title   <> '' THEN excluded.title   ELSE articles.title END,
               summary = CASE WHEN excluded.summary <> '' THEN excluded.summary ELSE articles.summary END,
               body    = CASE WHEN excluded.body    <> '' THEN excluded.body    ELSE articles.body END,
               published_at = COALESCE(excluded.published_at, articles.published_at)
             RETURNING id",
        )
        .bind(lang_str(a.lang, fallback))
        .bind(&a.source)
        .bind(&a.url)
        .bind(&a.title)
        .bind(&a.summary)
        .bind(&a.body)
        .bind(a.published_at)
        .bind(Utc::now())
        .fetch_one(&self.pool)
        .await
    }

    pub async fn list_articles(&self, lang: Option<Lang>, limit: i64) -> DbResult<Vec<Article>> {
        sqlx::query_as::<_, Article>(
            "SELECT id, lang, source, url, title, summary, '' AS body, published_at, fetched_at FROM articles
             WHERE (?1 IS NULL OR lang = ?1)
             ORDER BY COALESCE(published_at, fetched_at) DESC, id DESC LIMIT ?2",
        )
        .bind(lang.map(Lang::code))
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_article(&self, id: i64) -> DbResult<Option<Article>> {
        sqlx::query_as::<_, Article>("SELECT * FROM articles WHERE id = ?").bind(id).fetch_optional(&self.pool).await
    }

    // ── Lessons ─────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_lesson(
        &self,
        lang: Lang,
        title: &str,
        level: &str,
        article_id: Option<i64>,
        source_url: &str,
        path: &str,
        markdown: &str,
        data: &serde_json::Value,
        engine: &str,
    ) -> DbResult<i64> {
        sqlx::query_scalar(
            "INSERT INTO lessons (lang, title, level, article_id, source_url, path, markdown, data, engine, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(lang.code())
        .bind(title)
        .bind(level)
        .bind(article_id)
        .bind(source_url)
        .bind(path)
        .bind(markdown)
        .bind(data.to_string())
        .bind(engine)
        .bind(Utc::now())
        .fetch_one(&self.pool)
        .await
    }

    pub async fn list_lessons(&self, lang: Option<Lang>, limit: i64) -> DbResult<Vec<LessonRow>> {
        sqlx::query_as::<_, LessonRow>(
            "SELECT id, lang, title, level, article_id, source_url, path, '' AS markdown, '' AS data, engine, created_at
             FROM lessons WHERE (?1 IS NULL OR lang = ?1) ORDER BY created_at DESC, id DESC LIMIT ?2",
        )
        .bind(lang.map(Lang::code))
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_lesson(&self, id: i64) -> DbResult<Option<LessonRow>> {
        sqlx::query_as::<_, LessonRow>("SELECT * FROM lessons WHERE id = ?").bind(id).fetch_optional(&self.pool).await
    }

    pub async fn lesson_by_path(&self, path: &str) -> DbResult<Option<i64>> {
        sqlx::query_scalar("SELECT id FROM lessons WHERE path = ?").bind(path).fetch_optional(&self.pool).await
    }

    // ── Materials ───────────────────────────────────────

    pub async fn upsert_material(&self, week: &str, lang: Lang, title: &str, path: &str, items: i64, data: &str) -> DbResult<i64> {
        sqlx::query_scalar(
            "INSERT INTO materials (week, lang, title, path, items, data, imported_at) VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (path) DO UPDATE SET week = excluded.week, lang = excluded.lang, title = excluded.title,
               items = excluded.items, data = excluded.data, imported_at = excluded.imported_at
             RETURNING id",
        )
        .bind(week)
        .bind(lang.code())
        .bind(title)
        .bind(path)
        .bind(items)
        .bind(data)
        .bind(Utc::now())
        .fetch_one(&self.pool)
        .await
    }

    pub async fn list_materials(&self) -> DbResult<Vec<MaterialRow>> {
        sqlx::query_as::<_, MaterialRow>(
            "SELECT id, week, lang, title, path, items, imported_at FROM materials ORDER BY week DESC, path",
        )
        .fetch_all(&self.pool)
        .await
    }

    // ── Stats ───────────────────────────────────────────

    pub async fn stats(&self, lang: Option<Lang>, since: DateTime<Utc>, now: DateTime<Utc>) -> DbResult<Stats> {
        let l = lang.map(Lang::code);
        let count = |sql: &'static str| {
            let pool = self.pool.clone();
            async move { sqlx::query_scalar::<_, i64>(sql).bind(l).bind(since).bind(now).fetch_one(&pool).await }
        };
        Ok(Stats {
            words: count("SELECT COUNT(*) FROM words WHERE (?1 IS NULL OR lang = ?1)").await?,
            grammar: count("SELECT COUNT(*) FROM grammar WHERE (?1 IS NULL OR lang = ?1)").await?,
            due: count(
                "SELECT COUNT(*) FROM progress p
                 LEFT JOIN words w ON p.item_kind = 'word' AND p.item_id = w.id
                 LEFT JOIN grammar g ON p.item_kind = 'grammar' AND p.item_id = g.id
                 WHERE p.due_at <= ?3 AND (?1 IS NULL OR COALESCE(w.lang, g.lang) = ?1)",
            )
            .await?,
            lessons: count("SELECT COUNT(*) FROM lessons WHERE (?1 IS NULL OR lang = ?1)").await?,
            open_mistakes: count("SELECT COUNT(*) FROM mistakes WHERE (?1 IS NULL OR lang = ?1) AND resolved_at IS NULL").await?,
            new_words: count("SELECT COUNT(*) FROM words WHERE (?1 IS NULL OR lang = ?1) AND created_at >= ?2").await?,
            new_grammar: count("SELECT COUNT(*) FROM grammar WHERE (?1 IS NULL OR lang = ?1) AND created_at >= ?2").await?,
            reviews: count(
                "SELECT COUNT(*) FROM reviews r
                 LEFT JOIN words w ON r.item_kind = 'word' AND r.item_id = w.id
                 LEFT JOIN grammar g ON r.item_kind = 'grammar' AND r.item_id = g.id
                 WHERE r.reviewed_at >= ?2 AND (?1 IS NULL OR COALESCE(w.lang, g.lang) = ?1)",
            )
            .await?,
            reviews_correct: count(
                "SELECT COUNT(*) FROM reviews r
                 LEFT JOIN words w ON r.item_kind = 'word' AND r.item_id = w.id
                 LEFT JOIN grammar g ON r.item_kind = 'grammar' AND r.item_id = g.id
                 WHERE r.reviewed_at >= ?2 AND r.grade >= 3 AND (?1 IS NULL OR COALESCE(w.lang, g.lang) = ?1)",
            )
            .await?,
            new_lessons: count("SELECT COUNT(*) FROM lessons WHERE (?1 IS NULL OR lang = ?1) AND created_at >= ?2").await?,
            new_mistakes: count("SELECT COUNT(*) FROM mistakes WHERE (?1 IS NULL OR lang = ?1) AND created_at >= ?2").await?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(term: &str, meaning: &str) -> NewWord {
        NewWord { lang: Some(Lang::Ja), term: term.into(), reading: "よみ".into(), meaning: meaning.into(), ..Default::default() }
    }

    #[tokio::test]
    async fn upsert_keeps_richer_fields() {
        let db = Db::memory().await.unwrap();
        let a = db.upsert_word(&word("読み", "閱讀"), Lang::En).await.unwrap();
        let b = db.upsert_word(&word("読み", ""), Lang::En).await.unwrap();
        assert_eq!(a, b);
        let ws = db.list_words(Some(Lang::Ja), "", 10).await.unwrap();
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].meaning, "閱讀");
        assert!(ws[0].due_at.is_some(), "new words are due right away");
        assert!(db.list_words(Some(Lang::En), "", 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn review_moves_due_date_and_counts() {
        let db = Db::memory().await.unwrap();
        let id = db.upsert_word(&word("試験", "考試"), Lang::Ja).await.unwrap();
        let now = Utc::now();
        assert_eq!(db.due_items(Some(Lang::Ja), now, 10).await.unwrap().len(), 1);
        let card = db.record_review(ItemKind::Word, id, 4, now).await.unwrap();
        assert_eq!(card.reps, 1);
        assert!(db.due_items(Some(Lang::Ja), now, 10).await.unwrap().is_empty());
        let s = db.stats(Some(Lang::Ja), now - Duration::days(7), now).await.unwrap();
        assert_eq!((s.words, s.reviews, s.reviews_correct, s.due), (1, 1, 1, 0));
    }

    #[tokio::test]
    async fn mistakes_resolve() {
        let db = Db::memory().await.unwrap();
        let id = db
            .add_mistake(&NewMistake { question: "Q".into(), correct_answer: "A".into(), ..Default::default() }, Lang::En)
            .await
            .unwrap();
        assert_eq!(db.list_mistakes(Some(Lang::En), false).await.unwrap().len(), 1);
        assert!(db.resolve_mistake(id).await.unwrap());
        assert!(db.list_mistakes(Some(Lang::En), false).await.unwrap().is_empty());
        assert_eq!(db.list_mistakes(None, true).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn file_database_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x/learner.sqlite");
        {
            let db = Db::open(&path).await.unwrap();
            db.upsert_grammar(&NewGrammar { pattern: "〜ざるを得ない".into(), ..Default::default() }, Lang::Ja).await.unwrap();
        }
        let db = Db::open(&path).await.unwrap();
        assert_eq!(db.list_grammar(None, "", 10).await.unwrap().len(), 1);
    }
}
