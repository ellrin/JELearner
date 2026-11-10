//! Points (docs/plan.md §8): one point for each active unit completed and
//! passed (+ a bonus for a high score), one point for every 50 minutes of
//! passive listening or reading in the app, nothing for manual logging.
//!
//! Every attempt is a line in `records/points/YYYY-MM-DD.csv` (one file per
//! day, append-only, so several computers never edit the same line). A failed
//! attempt is a line with 0 points, which keeps the recent-performance
//! history rebuildable too. The database (`points`, `units`) is only an index
//! of these files and is rebuilt from them.

use crate::db::{Db, DbResult};
use crate::lang::Lang;
use crate::rules::PointRules;
use crate::workspace::Workspace;
use chrono::{DateTime, FixedOffset, Local, NaiveDate, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const HEADER: &[&str] = &["time", "lang", "unit", "skill", "points", "score", "detail"];
/// Passive units: time spent, not an attempt that can pass or fail.
pub const PASSIVE_UNITS: &[&str] = &["listening", "reading"];

/// Skills points are split into (§8.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Skill {
    Listening,
    Reading,
    Speaking,
    Writing,
    /// Vocabulary and grammar (單字文法).
    Vocab,
}

impl Skill {
    pub const ALL: [Skill; 5] = [Skill::Listening, Skill::Reading, Skill::Speaking, Skill::Writing, Skill::Vocab];

    pub fn code(self) -> &'static str {
        match self {
            Skill::Listening => "listening",
            Skill::Reading => "reading",
            Skill::Speaking => "speaking",
            Skill::Writing => "writing",
            Skill::Vocab => "vocab",
        }
    }

    pub fn parse(s: &str) -> Option<Skill> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "listening" | "聽" => Skill::Listening,
            "reading" | "讀" => Skill::Reading,
            "speaking" | "說" => Skill::Speaking,
            "writing" | "寫" => Skill::Writing,
            "vocab" | "grammar" | "vocab_grammar" | "單字文法" => Skill::Vocab,
            _ => return None,
        })
    }

    pub fn is_output(self) -> bool {
        matches!(self, Skill::Speaking | Skill::Writing)
    }
}

/// Points for one active unit: 0 when not passed, 1 when passed, plus the
/// bonus for a score at or above `bonus_score` or an excellent evaluation.
pub fn award(passed: bool, score: Option<f64>, excellent: bool, r: &PointRules) -> f64 {
    if !passed {
        return 0.0;
    }
    let high = excellent || score.is_some_and(|s| s + 1e-9 >= r.bonus_score);
    if high && r.bonus > 0.0 { 1.0 + r.bonus } else { 1.0 }
}

/// Whether a quiz-like score passes.
pub fn passes(score: f64, r: &PointRules) -> bool {
    score + 1e-9 >= r.pass_score
}

/// Points for passive time: whole blocks of `passive_minutes` in
/// `seconds_since_last_award`.
pub fn passive_points(seconds_since_last_award: i64, r: &PointRules) -> i64 {
    let block = (r.passive_minutes * 60.0).max(60.0) as i64;
    seconds_since_last_award.max(0) / block
}

/// One line of `records/points/*.csv`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointRecord {
    /// RFC 3339 with the computer's offset, e.g. `2026-11-06T09:12:00+08:00`.
    pub time: String,
    pub lang: Lang,
    /// `conversation`, `quiz`, `review`, `listening`, `reading`, …
    pub unit: String,
    pub skill: Skill,
    pub points: f64,
    pub score: Option<f64>,
    /// What it was: a workspace file relative to the profile, or `50 min`.
    pub detail: String,
}

impl PointRecord {
    /// A record stamped now, in this computer's time zone.
    pub fn now(lang: Lang, unit: &str, skill: Skill, points: f64, score: Option<f64>, detail: &str) -> PointRecord {
        PointRecord {
            time: Local::now().fixed_offset().to_rfc3339_opts(SecondsFormat::Secs, false),
            lang,
            unit: unit.into(),
            skill,
            points,
            score: score.map(|s| (s * 100.0).round() / 100.0),
            detail: detail.into(),
        }
    }

    pub fn parsed_time(&self) -> Option<DateTime<FixedOffset>> {
        DateTime::parse_from_rfc3339(self.time.trim()).ok()
    }

    /// Local date of the record (its own offset): names the CSV file.
    pub fn day(&self) -> Option<NaiveDate> {
        self.parsed_time().map(|t| t.date_naive())
    }

    pub fn is_passive(&self) -> bool {
        PASSIVE_UNITS.contains(&self.unit.as_str()) && self.score.is_none()
    }
}

fn fmt_num(x: f64) -> String {
    let s = format!("{:.2}", x);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn points_dir(ws: &Workspace) -> PathBuf {
    ws.records_dir().join("points")
}

/// Append one record to its day's file (header first when the file is new).
/// Never rewrites existing lines.
pub fn append(dir: &Path, rec: &PointRecord) -> std::io::Result<PathBuf> {
    let day = rec.day().ok_or_else(|| std::io::Error::other(format!("bad time {}", rec.time)))?;
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.csv", day.format("%Y-%m-%d")));
    let new = std::fs::metadata(&path).map(|m| m.len() == 0).unwrap_or(true);
    if !new {
        let bytes = std::fs::read(&path)?;
        if bytes.last() != Some(&b'\n') {
            use std::io::Write;
            std::fs::OpenOptions::new().append(true).open(&path)?.write_all(b"\n")?;
        }
    }
    let file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
    let mut w = csv::WriterBuilder::new().has_headers(false).from_writer(file);
    let io = |e: csv::Error| std::io::Error::other(e.to_string());
    if new {
        w.write_record(HEADER).map_err(io)?;
    }
    let score = rec.score.map(fmt_num).unwrap_or_default();
    w.write_record([rec.time.as_str(), rec.lang.code(), &rec.unit, rec.skill.code(), &fmt_num(rec.points), &score, &rec.detail])
        .map_err(io)?;
    w.flush()?;
    Ok(path)
}

/// Every record in the folder; bad lines become warnings.
pub fn read_dir(dir: &Path) -> (Vec<PointRecord>, Vec<String>) {
    let mut out = vec![];
    let mut warnings = vec![];
    for path in crate::workspace::files_with_ext(dir, &["csv"]) {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let mut rdr = csv::ReaderBuilder::new().flexible(true).trim(csv::Trim::All).from_reader(text.as_bytes());
        for (i, rec) in rdr.records().enumerate() {
            let line = i + 2;
            let Ok(r) = rec else {
                warnings.push(format!("{name}:{line}: 無法讀取"));
                continue;
            };
            let get = |k: usize| r.get(k).unwrap_or("").trim();
            let parsed = (|| {
                let lang: Lang = get(1).parse().ok()?;
                let skill = Skill::parse(get(3))?;
                let points: f64 = get(4).parse().ok()?;
                let score = if get(5).is_empty() { None } else { Some(get(5).parse::<f64>().ok()?) };
                let rec = PointRecord { time: get(0).into(), lang, unit: get(2).into(), skill, points, score, detail: get(6).into() };
                rec.parsed_time()?;
                Some(rec)
            })();
            match parsed {
                Some(p) => out.push(p),
                None => warnings.push(format!("{name}:{line}: 格式不符")),
            }
        }
    }
    (out, warnings)
}

/// Index one record in the database (idempotent): a `points` row, and a
/// `units` row for an active unit.
pub async fn insert(db: &Db, rec: &PointRecord, started_at: Option<DateTime<Utc>>) -> DbResult<bool> {
    let Some(t) = rec.parsed_time() else { return Ok(false) };
    let at = t.with_timezone(&Utc);
    let day = t.date_naive().format("%Y-%m-%d").to_string();
    let n = sqlx::query(
        "INSERT OR IGNORE INTO points (at, time, day, lang, unit, skill, points, score, detail) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(at)
    .bind(&rec.time)
    .bind(&day)
    .bind(rec.lang.code())
    .bind(&rec.unit)
    .bind(rec.skill.code())
    .bind(rec.points)
    .bind(rec.score)
    .bind(&rec.detail)
    .execute(db.pool())
    .await?
    .rows_affected();
    if !rec.is_passive() {
        let duration = started_at.map(|s| (at - s).num_seconds().max(0));
        sqlx::query(
            "INSERT OR IGNORE INTO units (lang, kind, skill, ref, started_at, ended_at, passed, score, duration_s, points)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(rec.lang.code())
        .bind(&rec.unit)
        .bind(rec.skill.code())
        .bind(&rec.detail)
        .bind(started_at)
        .bind(at)
        .bind(rec.points > 0.0)
        .bind(rec.score)
        .bind(duration)
        .bind(rec.points)
        .execute(db.pool())
        .await?;
    }
    Ok(n > 0)
}

/// Write a record to the workspace, then index it.
pub async fn record(ws: &Workspace, db: &Db, rec: &PointRecord, started_at: Option<DateTime<Utc>>) -> Result<(), String> {
    append(&points_dir(ws), rec).map_err(|e| format!("records/points：{e}"))?;
    insert(db, rec, started_at).await.map_err(|e| format!("資料庫：{e}"))?;
    Ok(())
}

/// Read every points file into the database (new lines only). Run when a
/// user is entered and after a pull, so records from other computers and a
/// fresh database both catch up.
pub async fn sync(ws: &Workspace, db: &Db) -> (usize, Vec<String>) {
    let (recs, mut warnings) = read_dir(&points_dir(ws));
    let mut n = 0;
    for r in &recs {
        match insert(db, r, None).await {
            Ok(true) => n += 1,
            Ok(false) => {}
            Err(e) => warnings.push(format!("points: {e}")),
        }
    }
    (n, warnings)
}

/// Points per skill.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct SkillPoints {
    pub listening: f64,
    pub reading: f64,
    pub speaking: f64,
    pub writing: f64,
    pub vocab: f64,
}

impl SkillPoints {
    pub fn add(&mut self, s: Skill, p: f64) {
        match s {
            Skill::Listening => self.listening += p,
            Skill::Reading => self.reading += p,
            Skill::Speaking => self.speaking += p,
            Skill::Writing => self.writing += p,
            Skill::Vocab => self.vocab += p,
        }
    }
    pub fn total(&self) -> f64 {
        self.listening + self.reading + self.speaking + self.writing + self.vocab
    }
    pub fn output(&self) -> f64 {
        self.speaking + self.writing
    }
    /// Share of speaking + writing (0 when there are no points).
    pub fn output_share(&self) -> f64 {
        let t = self.total();
        if t <= 0.0 { 0.0 } else { self.output() / t }
    }
}

/// Points per skill in a language, optionally only since a moment.
pub async fn skill_points(db: &Db, lang: Lang, since: Option<DateTime<Utc>>) -> DbResult<SkillPoints> {
    let rows: Vec<(String, f64)> = sqlx::query_as(
        "SELECT skill, SUM(points) FROM points WHERE lang = ?1 AND (?2 IS NULL OR at >= ?2) GROUP BY skill",
    )
    .bind(lang.code())
    .bind(since)
    .fetch_all(db.pool())
    .await?;
    let mut sp = SkillPoints::default();
    for (s, p) in rows {
        if let Some(s) = Skill::parse(&s) {
            sp.add(s, p);
        }
    }
    Ok(sp)
}

/// Points per local day for the last `days` days, oldest first (all
/// languages when `lang` is `None`).
pub async fn by_day(db: &Db, lang: Option<Lang>, today: NaiveDate, days: i64) -> DbResult<Vec<(String, f64)>> {
    let from = today - chrono::Duration::days(days - 1);
    let rows: Vec<(String, f64)> = sqlx::query_as(
        "SELECT day, SUM(points) FROM points WHERE (?1 IS NULL OR lang = ?1) AND day >= ?2 GROUP BY day",
    )
    .bind(lang.map(Lang::code))
    .bind(from.format("%Y-%m-%d").to_string())
    .fetch_all(db.pool())
    .await?;
    Ok((0..days)
        .map(|i| {
            let d = (from + chrono::Duration::days(i)).format("%Y-%m-%d").to_string();
            let p = rows.iter().find(|(x, _)| *x == d).map(|(_, p)| *p).unwrap_or(0.0);
            (d, p)
        })
        .collect())
}

/// Points of a language over the last `days` local days, today included
/// (the recent pace of docs/plan.md §9.1).
pub async fn recent_total(db: &Db, lang: Lang, today: NaiveDate, days: i64) -> DbResult<f64> {
    Ok(by_day(db, Some(lang), today, days.max(1)).await?.iter().map(|(_, p)| p).sum())
}

/// Days on which anything was practised, newest first.
pub async fn active_days(db: &Db) -> DbResult<Vec<NaiveDate>> {
    let rows: Vec<String> = sqlx::query_scalar("SELECT DISTINCT day FROM points ORDER BY day DESC LIMIT 400").fetch_all(db.pool()).await?;
    Ok(rows.iter().filter_map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).collect())
}

/// Consecutive practice days ending today (or yesterday, when today has
/// nothing yet). `days` newest first, distinct.
pub fn streak(days: &[NaiveDate], today: NaiveDate) -> u32 {
    let mut expect = match days.first() {
        Some(d) if *d == today => today,
        Some(d) if *d == today.pred_opt().unwrap_or(today) => *d,
        _ => return 0,
    };
    let mut n = 0;
    for d in days {
        if *d == expect {
            n += 1;
            expect = expect.pred_opt().unwrap_or(expect);
        } else if *d < expect {
            break;
        }
    }
    n
}

/// What study buddies see of each other (plan §16): points today and this
/// week, the streak and the last seven days, straight from a member's
/// `records/points/` files — no database needed, so it works for members
/// who study on another device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BuddyStats {
    pub today: f64,
    /// The last seven days, today included.
    pub week: f64,
    pub total: f64,
    pub streak: u32,
    /// Points per day for the last seven days, oldest first (today last).
    pub days: Vec<f64>,
    /// The last day anything was practised (`YYYY-MM-DD`).
    pub last_day: Option<String>,
}

pub fn buddy_stats(recs: &[PointRecord], today: NaiveDate) -> BuddyStats {
    let mut per: std::collections::BTreeMap<NaiveDate, f64> = std::collections::BTreeMap::new();
    for r in recs {
        if let Some(d) = r.day() {
            *per.entry(d).or_default() += r.points;
        }
    }
    let days: Vec<f64> = (0..7).rev().map(|back| per.get(&(today - chrono::Days::new(back))).copied().unwrap_or(0.0)).collect();
    let active: Vec<NaiveDate> = per.keys().rev().copied().collect();
    let round = |x: f64| (x * 100.0).round() / 100.0;
    BuddyStats {
        today: round(*days.last().unwrap_or(&0.0)),
        week: round(days.iter().sum()),
        total: round(per.values().sum()),
        streak: streak(&active, today),
        days: days.into_iter().map(round).collect(),
        last_day: active.first().map(|d| d.format("%Y-%m-%d").to_string()),
    }
}

/// Scores of the most recent scored units, newest first (only `kinds` when
/// given, only after `since` when given).
pub async fn recent_scores(db: &Db, lang: Lang, limit: usize, kinds: Option<&[&str]>, since: Option<DateTime<Utc>>) -> DbResult<Vec<f64>> {
    let rows: Vec<(String, f64)> = sqlx::query_as(
        "SELECT kind, score FROM units WHERE lang = ?1 AND score IS NOT NULL AND (?2 IS NULL OR ended_at > ?2)
         ORDER BY ended_at DESC, id DESC LIMIT 500",
    )
    .bind(lang.code())
    .bind(since)
    .fetch_all(db.pool())
    .await?;
    Ok(rows.into_iter().filter(|(k, _)| kinds.is_none_or(|ks| ks.contains(&k.as_str()))).map(|(_, s)| s).take(limit).collect())
}

/// Whether a unit with this kind and reference has already been passed
/// (a lesson's quiz is rewarded once).
pub async fn already_passed(db: &Db, lang: Lang, kind: &str, reference: &str) -> DbResult<bool> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM units WHERE lang = ? AND kind = ? AND ref = ? AND passed = 1")
        .bind(lang.code())
        .bind(kind)
        .bind(reference)
        .fetch_one(db.pool())
        .await?;
    Ok(n > 0)
}

/// Units of a kind finished on a local day.
pub async fn units_on(db: &Db, lang: Lang, kind: &str, day: NaiveDate) -> DbResult<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM points WHERE lang = ? AND unit = ? AND day = ?")
        .bind(lang.code())
        .bind(kind)
        .bind(day.format("%Y-%m-%d").to_string())
        .fetch_one(db.pool())
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Rules;

    fn rules() -> PointRules {
        Rules::bundled().points
    }

    #[test]
    fn buddy_stats_from_records() {
        let rec = |time: &str, points: f64| PointRecord { time: time.into(), lang: Lang::En, unit: "review".into(), skill: Skill::Vocab, points, score: Some(1.0), detail: String::new() };
        let today = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let recs = vec![
            rec("2026-11-10T09:00:00+08:00", 1.5),
            rec("2026-11-10T21:00:00+08:00", 1.0),
            rec("2026-11-09T08:00:00+08:00", 2.0),
            rec("2026-11-07T08:00:00+08:00", 1.0),
            rec("2026-10-01T08:00:00+08:00", 4.0),
        ];
        let b = buddy_stats(&recs, today);
        assert_eq!(b.today, 2.5);
        assert_eq!(b.week, 5.5, "the last seven days only");
        assert_eq!(b.total, 9.5);
        assert_eq!(b.streak, 2, "the 8th was missed");
        assert_eq!(b.days, vec![0.0, 0.0, 0.0, 1.0, 0.0, 2.0, 2.5]);
        assert_eq!(b.last_day.as_deref(), Some("2026-11-10"));
        let none = buddy_stats(&[], today);
        assert_eq!((none.today, none.streak, none.last_day.clone()), (0.0, 0, None));
        assert_eq!(none.days.len(), 7);
    }

    #[test]
    fn active_units_score_one_plus_bonus() {
        let r = rules();
        assert_eq!(award(false, Some(0.99), true, &r), 0.0, "not passed = 0");
        assert_eq!(award(true, Some(0.8), false, &r), 1.0);
        assert_eq!(award(true, Some(0.95), false, &r), 1.5, "95% earns the bonus");
        assert_eq!(award(true, None, true, &r), 1.5, "excellent conversation earns the bonus");
        let mut off = r.clone();
        off.bonus = 0.0;
        assert_eq!(award(true, Some(1.0), true, &off), 1.0, "bonus can be turned off");
        assert!(passes(0.7, &r) && !passes(0.69, &r));
    }

    #[test]
    fn passive_time_in_fifty_minute_blocks() {
        let r = rules();
        assert_eq!(passive_points(49 * 60, &r), 0);
        assert_eq!(passive_points(50 * 60, &r), 1);
        assert_eq!(passive_points(101 * 60, &r), 2);
        assert_eq!(passive_points(-5, &r), 0);
    }

    #[test]
    fn streak_counts_back_from_today_or_yesterday() {
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let today = d("2026-11-06");
        assert_eq!(streak(&[d("2026-11-06"), d("2026-11-05"), d("2026-11-04"), d("2026-11-01")], today), 3);
        assert_eq!(streak(&[d("2026-11-05"), d("2026-11-04")], today), 2, "today not done yet");
        assert_eq!(streak(&[d("2026-11-03")], today), 0);
        assert_eq!(streak(&[], today), 0);
    }

    #[test]
    fn csv_round_trip_and_append_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = PointRecord::now(Lang::En, "conversation", Skill::Writing, 1.5, Some(0.956), "conversations/en/x.md");
        a.time = "2026-11-06T09:12:00+08:00".into();
        let mut b = a.clone();
        b.time = "2026-11-06T10:00:00+08:00".into();
        b.unit = "listening".into();
        b.skill = Skill::Listening;
        b.points = 1.0;
        b.score = None;
        b.detail = "50 min".into();
        let p = append(dir.path(), &a).unwrap();
        append(dir.path(), &b).unwrap();
        assert!(p.ends_with("2026-11-06.csv"), "the record's own date names the file");
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("time,lang,unit,skill,points,score,detail\n"));
        assert!(text.contains("2026-11-06T09:12:00+08:00,en,conversation,writing,1.5,0.96,conversations/en/x.md"));
        assert!(text.contains(",listening,listening,1,,50 min"));
        let (recs, w) = read_dir(dir.path());
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].score, Some(0.96));
        assert!(recs[1].is_passive());
        std::fs::write(dir.path().join("2026-11-07.csv"), "time,lang,unit,skill,points,score,detail\nnope,xx,a,b,c,,\n").unwrap();
        let (_, w) = read_dir(dir.path());
        assert_eq!(w.len(), 1);
    }

    #[tokio::test]
    async fn database_index_is_idempotent_and_sums_skills() {
        let db = Db::memory().await.unwrap();
        let mut a = PointRecord::now(Lang::En, "quiz", Skill::Reading, 1.0, Some(0.8), "lessons/en/a.md");
        a.time = "2026-11-06T09:00:00+08:00".into();
        let mut b = PointRecord::now(Lang::En, "conversation", Skill::Writing, 1.5, Some(0.96), "conversations/en/b.md");
        b.time = "2026-11-06T09:30:00+08:00".into();
        let mut c = PointRecord::now(Lang::En, "reading", Skill::Reading, 1.0, None, "50 min");
        c.time = "2026-11-06T10:30:00+08:00".into();
        for r in [&a, &b, &c, &a] {
            insert(&db, r, None).await.unwrap();
        }
        let sp = skill_points(&db, Lang::En, None).await.unwrap();
        assert_eq!(sp.total(), 3.5);
        assert!((sp.output_share() - 1.5 / 3.5).abs() < 1e-9);
        let units: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM units").fetch_one(db.pool()).await.unwrap();
        assert_eq!(units, 2, "passive time is not a unit");
        assert_eq!(recent_scores(&db, Lang::En, 5, None, None).await.unwrap(), vec![0.96, 0.8]);
        assert_eq!(recent_scores(&db, Lang::En, 5, Some(&["quiz"]), None).await.unwrap(), vec![0.8]);
        assert!(already_passed(&db, Lang::En, "quiz", "lessons/en/a.md").await.unwrap());
        let days = by_day(&db, Some(Lang::En), NaiveDate::from_ymd_opt(2026, 11, 7).unwrap(), 3).await.unwrap();
        assert_eq!(days, vec![("2026-11-05".into(), 0.0), ("2026-11-06".into(), 3.5), ("2026-11-07".into(), 0.0)]);
    }

    #[tokio::test]
    async fn recent_pace_from_point_records() {
        // Records written to records/points/ and read back (as after a pull):
        // only the last 28 days of this language count.
        let dir = tempfile::tempdir().unwrap();
        let rec = |time: &str, lang: Lang, points: f64| {
            let mut r = PointRecord::now(lang, "quiz", Skill::Reading, points, Some(0.8), "lessons/x.md");
            r.time = time.into();
            r
        };
        for r in [
            rec("2026-10-01T09:00:00+08:00", Lang::En, 30.0), // older than 28 days
            rec("2026-10-10T09:00:00+08:00", Lang::En, 25.0),
            rec("2026-11-01T09:00:00+08:00", Lang::En, 10.5),
            rec("2026-11-06T09:00:00+08:00", Lang::En, 4.5),
            rec("2026-11-06T10:00:00+08:00", Lang::Ja, 9.0),
        ] {
            append(dir.path(), &r).unwrap();
        }
        let (recs, w) = read_dir(dir.path());
        assert!(w.is_empty(), "{w:?}");
        let db = Db::memory().await.unwrap();
        for r in &recs {
            insert(&db, r, None).await.unwrap();
        }
        let today = NaiveDate::from_ymd_opt(2026, 11, 6).unwrap();
        let rules = Rules::bundled();
        let recent = recent_total(&db, Lang::En, today, rules.milestones.pace_days as i64).await.unwrap();
        assert_eq!(recent, 40.0);
        let m = crate::progress::milestones(315.0, 375.0, recent, &rules.milestones);
        assert_eq!((m.pace_per_week, m.weeks_to_next_step), (10.0, Some(6)));
    }
}
