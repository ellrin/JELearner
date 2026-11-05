//! Stage A handlers (docs/plan.md §5.4, §7–§10): points, levels, content
//! difficulty, today's tasks, the typed AI conversation and the user's
//! picture. Like `api.rs`, every handler is a plain async function over
//! [`Shared`] so it can become a Tauri command later.

use crate::api::{free_path, need_lang, parse_lang, ws};
use crate::{bad, fail, provider, unavailable, ApiError, Shared};
use axum::extract::{Path as AxPath, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use jl_core::conversation::{self, Conversation, Learner, Speaker, Templates as ConvTemplates, Turn};
use jl_core::db::Db;
use jl_core::lesson;
use jl_core::points::{self, PointRecord, Skill};
use jl_core::progress::{self, Feedback};
use jl_core::rules::Rules;
use jl_core::topics::{self, Topic, TopicKind};
use jl_core::workspace::{slug, Workspace};
use jl_core::{level, Lang};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

type R<T> = Result<Json<T>, ApiError>;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/today", get(today))
        .route("/api/progress", get(progress_view))
        .route("/api/progress/assess", post(assess_now))
        .route("/api/difficulty/feedback", post(difficulty_feedback))
        .route("/api/passive", post(passive))
        .route("/api/lessons/{id}/finish", post(finish_lesson))
        .route("/api/conversations", get(list_conversations).post(start_conversation))
        .route("/api/conversations/next", get(next_topic))
        .route("/api/conversations/{id}", get(get_conversation))
        .route("/api/conversations/{id}/turns", post(conversation_turn))
        .route("/api/conversations/{id}/finish", post(finish_conversation))
        .route(
            "/api/user/avatar",
            post(upload_avatar).delete(delete_avatar).layer(axum::extract::DefaultBodyLimit::max(AVATAR_MAX_BYTES + 1024 * 1024)),
        )
        .route("/api/avatars/{id}", get(avatar))
}

// ── helpers ─────────────────────────────────────────────

fn local_today() -> NaiveDate {
    Local::now().date_naive()
}

/// Start of a local day, in UTC.
fn day_start(d: NaiveDate) -> DateTime<Utc> {
    Local.from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap()).earliest().map(|t| t.with_timezone(&Utc)).unwrap_or_else(Utc::now)
}

fn api_err(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_GATEWAY, e.to_string())
}

async fn rules(s: &Shared) -> Rules {
    let root = s.root.read().await.as_ref().map(|r| r.root.root.clone());
    Rules::load(root.as_deref()).0
}

fn level_since(ws: &Workspace, lang: Lang) -> Option<DateTime<Utc>> {
    ws.profile.lang(lang).level_since.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&Utc))
}

/// Conditions 1–3 for the next step of `lang`.
async fn assessment(ws: &Workspace, db: &Db, lang: Lang, rules: &Rules) -> Result<progress::Assessment, ApiError> {
    let since = level_since(ws, lang);
    let sp = points::skill_points(db, lang, since).await?;
    let recent = points::recent_scores(db, lang, rules.assessment.recent_units, None, None).await?;
    Ok(progress::assess(lang, &ws.profile.lang(lang).level, &sp, &recent, rules))
}

/// Raise the level by one step when conditions 1–3 hold (never down, never
/// across a major level). Returns `{ from, to }` when it went up.
async fn try_level_up(s: &Shared, lang: Lang) -> Result<Option<Value>, ApiError> {
    let r = rules(s).await;
    let a = {
        let l = ws(s).await?;
        assessment(&l.ws, &l.db, lang, &r).await?
    };
    if !a.eligible {
        return Ok(None);
    }
    let Some(next) = a.next.clone() else { return Ok(None) };
    let mut g = s.loaded.write().await;
    let l = g.as_mut().ok_or_else(|| unavailable("請先選擇使用者"))?;
    let mut lp = l.ws.profile.lang(lang);
    if lp.level != a.level {
        return Ok(None); // changed meanwhile
    }
    lp.level = next.clone();
    lp.level_since = Some(Local::now().fixed_offset().to_rfc3339_opts(chrono::SecondsFormat::Secs, false));
    l.ws.profile.lang.insert(lang, lp);
    l.ws.save_profile().map_err(|e| fail(e.to_string()))?;
    Ok(Some(json!({ "from": a.level, "to": next })))
}

/// Move the content difficulty and log why. Returns `{ from, to, label }`.
async fn change_difficulty(s: &Shared, lang: Lang, delta: i32, reason: &str, use_feedback: bool) -> Result<Option<Value>, ApiError> {
    let mut g = s.loaded.write().await;
    let l = g.as_mut().ok_or_else(|| unavailable("請先選擇使用者"))?;
    let mut lp = l.ws.profile.lang(lang);
    let from = lp.difficulty;
    let to = progress::apply(lang, from, delta);
    if use_feedback {
        lp.early_feedback_left = lp.early_feedback_left.saturating_sub(1);
    }
    if to == from && !use_feedback {
        return Ok(None);
    }
    lp.difficulty = to;
    l.ws.profile.lang.insert(lang, lp);
    l.ws.save_profile().map_err(|e| fail(e.to_string()))?;
    sqlx::query("INSERT INTO difficulty_log (lang, from_score, to_score, reason, at) VALUES (?, ?, ?, ?, ?)")
        .bind(lang.code())
        .bind(from as i64)
        .bind(to as i64)
        .bind(reason)
        .bind(Utc::now())
        .execute(l.db.pool())
        .await?;
    Ok(Some(json!({ "from": from, "to": to, "label": level::from_score(lang, to).label })))
}

async fn last_difficulty_change(db: &Db, lang: Lang) -> Result<Option<DateTime<Utc>>, ApiError> {
    Ok(sqlx::query_scalar::<_, Option<DateTime<Utc>>>("SELECT MAX(at) FROM difficulty_log WHERE lang = ?").bind(lang.code()).fetch_one(db.pool()).await?)
}

/// After the first lessons: adjust from recent quiz scores.
async fn auto_difficulty(s: &Shared, lang: Lang) -> Result<Option<Value>, ApiError> {
    let r = rules(s).await;
    let delta = {
        let l = ws(s).await?;
        if l.ws.profile.lang(lang).early_feedback_left > 0 {
            return Ok(None);
        }
        let since = last_difficulty_change(&l.db, lang).await?;
        let scores = points::recent_scores(&l.db, lang, r.difficulty.window, Some(&["quiz"]), since).await?;
        progress::accuracy_delta(&scores, &r.difficulty)
    };
    if delta == 0 {
        return Ok(None);
    }
    change_difficulty(s, lang, delta, if delta > 0 { "accuracy:up" } else { "accuracy:down" }, false).await
}

async fn points_on(db: &Db, day: NaiveDate) -> Result<(f64, Vec<(String, f64)>), ApiError> {
    let rows: Vec<(String, f64)> = sqlx::query_as("SELECT lang, SUM(points) FROM points WHERE day = ? GROUP BY lang")
        .bind(day.format("%Y-%m-%d").to_string())
        .fetch_all(db.pool())
        .await?;
    Ok((rows.iter().map(|(_, p)| p).sum(), rows))
}

async fn total_points(db: &Db, lang: Option<Lang>) -> Result<f64, ApiError> {
    Ok(sqlx::query_scalar::<_, Option<f64>>("SELECT SUM(points) FROM points WHERE (?1 IS NULL OR lang = ?1)").bind(lang.map(Lang::code)).fetch_one(db.pool()).await?.unwrap_or(0.0))
}

/// The summary shown on the profile card and the today page.
async fn summary(ws: &Workspace, db: &Db, lang: Lang, r: &Rules) -> Result<Value, ApiError> {
    let today = local_today();
    let (today_all, per) = points_on(db, today).await?;
    let today_lang = per.iter().find(|(l, _)| l == lang.code()).map(|(_, p)| *p).unwrap_or(0.0);
    let a = assessment(ws, db, lang, r).await?;
    let days = points::active_days(db).await?;
    let lp = ws.profile.lang(lang);
    Ok(json!({
        "lang": lang.code(),
        "level": lp.level,
        "next": a.next,
        "major_boundary": a.major_boundary,
        "points_since": a.points_since,
        "step_points": a.step_points,
        "eligible": a.eligible,
        "difficulty": level::from_score(lang, lp.difficulty).label,
        "early_feedback_left": lp.early_feedback_left,
        "today": today_all,
        "today_lang": today_lang,
        "goal": r.daily_goal(ws.profile.daily_minutes),
        "total": total_points(db, None).await?,
        "total_lang": total_points(db, Some(lang)).await?,
        "streak": points::streak(&days, today),
    }))
}

#[derive(Deserialize, Default)]
struct LangQ {
    lang: Option<String>,
    skip: Option<String>,
}

fn lang_or_first(q: &Option<String>, ws: &Workspace) -> Result<Lang, ApiError> {
    Ok(parse_lang(q)?.unwrap_or_else(|| ws.profile.languages.first().copied().unwrap_or(Lang::En)))
}

// ── today & progress ────────────────────────────────────

/// Today's tasks (3–4) and the numbers on top. The client highlights the
/// first unfinished task as the one primary action.
async fn today(State(s): State<Shared>, Query(q): Query<LangQ>) -> R<Value> {
    let r = rules(&s).await;
    let l = ws(&s).await?;
    let lang = lang_or_first(&q.lang, &l.ws)?;
    let db = &l.db;
    let day = local_today();
    let start = day_start(day);
    let now = Utc::now();
    let stats = db.stats(Some(lang), start, now).await?;

    // Review: due cards, and progress in the current round.
    let last_round: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT MAX(at) FROM points WHERE lang = ? AND unit = 'review'").bind(lang.code()).fetch_one(db.pool()).await?;
    let in_round = reviews_since(db, lang, last_round).await?.len() as i64;
    let review_done = points::units_on(db, lang, "review", day).await? > 0 || (stats.due == 0);

    // Conversation: one finished today; or one to continue.
    let conv_done = points::units_on(db, lang, "conversation", day).await? > 0;
    let open: Option<(i64, i64, i64)> = sqlx::query_as(
        "SELECT id, turns_done, turns_limit FROM conversations WHERE lang = ? AND status != 'done' ORDER BY started_at DESC LIMIT 1",
    )
    .bind(lang.code())
    .fetch_optional(db.pool())
    .await?;

    // Lesson quiz: one finished today; the newest lesson without a passed quiz.
    let quiz_done = points::units_on(db, lang, "quiz", day).await? > 0;
    let lessons = db.list_lessons(Some(lang), 30).await?;
    let mut todo_lesson = None;
    for row in &lessons {
        let rel = l.ws.profile_relative(std::path::Path::new(&row.path));
        if !points::already_passed(db, lang, "quiz", &rel).await? {
            todo_lesson = Some(json!({ "id": row.id, "title": row.title }));
            break;
        }
    }

    let mut tasks = vec![
        json!({ "id": "review", "done": review_done, "due": stats.due, "progress": [in_round.min(r.points.review_round), r.points.review_round], "reviews_today": stats.reviews }),
        json!({ "id": "conversation", "done": conv_done, "open": open.map(|(id, d, t)| json!({ "id": id, "turns_done": d, "turns_limit": t })), "needs_engine": true }),
        json!({ "id": "lesson", "done": quiz_done, "lesson": todo_lesson, "needs_engine": todo_lesson.is_none() }),
    ];
    if stats.open_mistakes > 0 {
        tasks.push(json!({ "id": "mistakes", "done": false, "open": stats.open_mistakes }));
    }
    let next = tasks.iter().position(|t| t["done"] != true);
    Ok(Json(json!({ "summary": summary(&l.ws, db, lang, &r).await?, "tasks": tasks, "next": next })))
}

/// The progress page: level and conditions, skills, recent days, topics.
async fn progress_view(State(s): State<Shared>, Query(q): Query<LangQ>) -> R<Value> {
    let r = rules(&s).await;
    let l = ws(&s).await?;
    let lang = lang_or_first(&q.lang, &l.ws)?;
    let db = &l.db;
    let a = assessment(&l.ws, db, lang, &r).await?;
    let all = points::skill_points(db, lang, None).await?;
    let since = points::skill_points(db, lang, level_since(&l.ws, lang)).await?;
    let days = points::by_day(db, Some(lang), local_today(), 14).await?;
    let (general, topics_warning) = topics::load_general(&l.ws.topics_file());
    let stats = conversation::topic_stats(db, lang).await?;
    let topic_list: Vec<Value> = general
        .iter()
        .map(|t| {
            let st = stats.iter().find(|x| x.0 == t.id);
            json!({ "id": t.id, "name": t.name, "group": t.group, "count": st.map(|x| x.3).unwrap_or(0), "score": st.and_then(|x| x.4) })
        })
        .collect();
    let interests: Vec<Value> = stats.iter().filter(|x| x.2 == "interest").map(|x| json!({ "id": x.0, "name": x.1, "count": x.3, "score": x.4 })).collect();
    let recent: Vec<(String, String, String, f64, Option<f64>, String)> = sqlx::query_as(
        "SELECT time, unit, skill, points, score, detail FROM points WHERE lang = ? ORDER BY at DESC, id DESC LIMIT 20",
    )
    .bind(lang.code())
    .fetch_all(db.pool())
    .await?;
    Ok(Json(json!({
        "summary": summary(&l.ws, db, lang, &r).await?,
        "assessment": a,
        "skills": all,
        "skills_level": since,
        "days": days,
        "topics": topic_list,
        "interests": interests,
        "topics_warning": topics_warning,
        "recent": recent.into_iter().map(|(time, unit, skill, p, score, detail)| json!({ "time": time, "unit": unit, "skill": skill, "points": p, "score": score, "detail": detail })).collect::<Vec<_>>(),
        "rules": { "output_share": r.assessment.output_share, "recent_units": r.assessment.recent_units, "recent_score": r.assessment.recent_score },
    })))
}

#[derive(Deserialize)]
struct LangBody {
    lang: Option<String>,
}

/// Check the level now (also done after every unit).
async fn assess_now(State(s): State<Shared>, Json(b): Json<LangBody>) -> R<Value> {
    let lang = need_lang(&b.lang)?;
    let up = try_level_up(&s, lang).await?;
    let r = rules(&s).await;
    let l = ws(&s).await?;
    let a = assessment(&l.ws, &l.db, lang, &r).await?;
    Ok(Json(json!({ "level_up": up, "assessment": a })))
}

#[derive(Deserialize)]
struct FeedbackBody {
    lang: Option<String>,
    feedback: Feedback,
}

/// "Too easy / just right / too hard" after one of the first lessons.
async fn difficulty_feedback(State(s): State<Shared>, Json(b): Json<FeedbackBody>) -> R<Value> {
    let lang = need_lang(&b.lang)?;
    let left = ws(&s).await?.ws.profile.lang(lang).early_feedback_left;
    if left == 0 {
        return Ok(Json(json!({ "ignored": true, "early_feedback_left": 0 })));
    }
    let r = rules(&s).await;
    let delta = progress::early_delta(b.feedback, &r.difficulty);
    let reason = format!("feedback:{}", serde_json::to_value(b.feedback).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default());
    let change = change_difficulty(&s, lang, delta, &reason, true).await?;
    let l = ws(&s).await?;
    let lp = l.ws.profile.lang(lang);
    Ok(Json(json!({ "change": change, "difficulty": level::from_score(lang, lp.difficulty).label, "early_feedback_left": lp.early_feedback_left })))
}

#[derive(Deserialize)]
struct PassiveBody {
    lang: Option<String>,
    /// `reading` | `listening`
    kind: String,
    seconds: i64,
}

/// Time spent reading (or, later, listening) in the app, sent by the page
/// while it is visible and the learner is active. Every 50 minutes = 1
/// point. Nothing can be logged by hand.
async fn passive(State(s): State<Shared>, Json(b): Json<PassiveBody>) -> R<Value> {
    let lang = need_lang(&b.lang)?;
    let skill = match b.kind.as_str() {
        "reading" => Skill::Reading,
        "listening" => Skill::Listening,
        _ => return Err(bad("kind 只能是 reading 或 listening")),
    };
    let secs = b.seconds.clamp(0, 120);
    let r = rules(&s).await;
    let l = ws(&s).await?;
    if secs > 0 {
        sqlx::query("INSERT INTO listening_time (lang, kind, seconds, at) VALUES (?, ?, ?, ?)")
            .bind(lang.code())
            .bind(&b.kind)
            .bind(secs)
            .bind(Utc::now())
            .execute(l.db.pool())
            .await?;
    }
    let last: Option<DateTime<Utc>> = sqlx::query_scalar("SELECT MAX(at) FROM points WHERE lang = ? AND unit = ? AND score IS NULL")
        .bind(lang.code())
        .bind(&b.kind)
        .fetch_one(l.db.pool())
        .await?;
    let since: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(seconds), 0) FROM listening_time WHERE lang = ?1 AND kind = ?2 AND (?3 IS NULL OR at > ?3)")
        .bind(lang.code())
        .bind(&b.kind)
        .bind(last)
        .fetch_one(l.db.pool())
        .await?;
    let n = points::passive_points(since, &r.points);
    for _ in 0..n {
        let rec = PointRecord::now(lang, &b.kind, skill, 1.0, None, &format!("{} min", r.points.passive_minutes));
        points::record(&l.ws, &l.db, &rec, None).await.map_err(fail)?;
    }
    let block = (r.points.passive_minutes * 60.0) as i64;
    Ok(Json(json!({ "awarded": n, "seconds": since - n * block, "block": block })))
}

/// Reviews since a moment for a language: grades.
async fn reviews_since(db: &Db, lang: Lang, since: Option<DateTime<Utc>>) -> Result<Vec<i64>, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT r.grade FROM reviews r
         LEFT JOIN words w ON r.item_kind = 'word' AND r.item_id = w.id
         LEFT JOIN grammar g ON r.item_kind = 'grammar' AND r.item_id = g.id
         WHERE COALESCE(w.lang, g.lang) = ?1 AND (?2 IS NULL OR r.reviewed_at > ?2)",
    )
    .bind(lang.code())
    .bind(since)
    .fetch_all(db.pool())
    .await?)
}

/// After a review: a full round of cards is a unit (1 point, + bonus at
/// 95% remembered). Called by the review handler.
pub async fn review_round(s: &Shared, lang: Lang) -> Result<Value, ApiError> {
    let r = rules(s).await;
    let l = ws(s).await?;
    let last: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT MAX(at) FROM points WHERE lang = ? AND unit = 'review'").bind(lang.code()).fetch_one(l.db.pool()).await?;
    let grades = reviews_since(&l.db, lang, last).await?;
    let size = r.points.review_round.max(1);
    let done = grades.len() as i64;
    if done < size {
        return Ok(json!({ "round": { "done": done, "size": size } }));
    }
    let score = grades.iter().filter(|g| **g >= 3).count() as f64 / grades.len() as f64;
    let pts = points::award(true, Some(score), false, &r.points);
    let rec = PointRecord::now(lang, "review", Skill::Vocab, pts, Some(score), &format!("{done} 張"));
    points::record(&l.ws, &l.db, &rec, None).await.map_err(fail)?;
    drop(l);
    let up = try_level_up(s, lang).await?;
    Ok(json!({ "round": { "done": 0, "size": size }, "award": { "points": pts, "score": score, "cards": done }, "level_up": up }))
}

// ── lessons ─────────────────────────────────────────────

#[derive(Deserialize)]
struct FinishAnswer {
    index: usize,
    #[serde(default)]
    answer: String,
    correct: Option<bool>,
}

#[derive(Deserialize)]
struct FinishBody {
    answers: Vec<FinishAnswer>,
}

/// The end of a lesson's quiz: grade all answers (choices here, short
/// answers as the learner marked them), award the unit once, and adjust
/// difficulty and level. Mistakes were already recorded per question.
async fn finish_lesson(State(s): State<Shared>, AxPath(id): AxPath<i64>, Json(b): Json<FinishBody>) -> R<Value> {
    let r = rules(&s).await;
    let (lang, result) = {
        let l = ws(&s).await?;
        let row = l.db.get_lesson(id).await?.ok_or_else(|| bad("找不到教材"))?;
        let lesson: lesson::Lesson = serde_json::from_str(&row.data).map_err(|e| fail(e.to_string()))?;
        let lang: Lang = row.lang.parse().map_err(bad)?;
        let total = lesson.questions.len().max(1);
        let mut correct = 0;
        for (i, q) in lesson.questions.iter().enumerate() {
            let a = b.answers.iter().find(|a| a.index == i);
            let ok = a.and_then(|a| a.correct.or_else(|| lesson::grade(q, &a.answer))).unwrap_or(false);
            if ok {
                correct += 1;
            }
        }
        let score = correct as f64 / total as f64;
        let passed = points::passes(score, &r.points);
        let detail = l.ws.profile_relative(std::path::Path::new(&row.path));
        let already = points::already_passed(&l.db, lang, "quiz", &detail).await?;
        let pts = if already { 0.0 } else { points::award(passed, Some(score), false, &r.points) };
        if !already {
            let rec = PointRecord::now(lang, "quiz", Skill::Reading, pts, Some(score), &detail);
            points::record(&l.ws, &l.db, &rec, None).await.map_err(fail)?;
        }
        (lang, json!({ "score": score, "correct": correct, "total": lesson.questions.len(), "passed": passed, "points": pts, "already": already }))
    };
    let difficulty_change = auto_difficulty(&s, lang).await?;
    let level_up = try_level_up(&s, lang).await?;
    let l = ws(&s).await?;
    let mut out = result;
    out["ask_difficulty"] = json!(l.ws.profile.lang(lang).early_feedback_left > 0);
    out["difficulty_change"] = json!(difficulty_change);
    out["level_up"] = json!(level_up);
    out["summary"] = summary(&l.ws, &l.db, lang, &r).await?;
    Ok(Json(out))
}

// ── conversations ───────────────────────────────────────

fn learner(ws: &Workspace, lang: Lang, level_label: &str) -> Learner {
    let lv = level::parse(lang, level_label).unwrap_or_else(|| ws.profile.difficulty(lang));
    Learner { native_language: ws.profile.native_language.clone(), level_description: level::describe(lang, &lv) }
}

fn view(id: i64, row_status: &str, path: &str, c: &Conversation, seconds_per_turn: u32) -> Value {
    json!({
        "id": id,
        "status": row_status,
        "path": path,
        "conversation": c,
        "turns_done": c.turns_done(),
        "turns_limit": c.turns_limit,
        "used": c.used_targets(),
        "seconds_per_turn": seconds_per_turn,
        "max_chars": conversation::MAX_TURN_CHARS,
    })
}

async fn list_conversations(State(s): State<Shared>, Query(q): Query<LangQ>) -> R<Value> {
    let l = ws(&s).await?;
    Ok(Json(json!(conversation::list(&l.db, parse_lang(&q.lang)?, 50).await?)))
}

/// Topics available for a language, and the one that would be picked next.
async fn choose_topic(l: &crate::Loaded, lang: Lang, wanted: Option<&str>, skip: &[String]) -> Result<Topic, ApiError> {
    let (general, _) = topics::load_general(&l.ws.topics_file());
    let interests = topics::load_interests(&l.ws.interests_file());
    if let Some(id) = wanted.filter(|w| !w.is_empty()) {
        let found = general.iter().find(|t| t.id == id).cloned().or_else(|| {
            interests.iter().filter(|i| i.lang.is_none_or(|x| x == lang)).map(topics::interest_topic).find(|t| t.id == id)
        });
        return found.ok_or_else(|| bad("找不到這個主題"));
    }
    let mut history = conversation::topic_history(&l.db, lang).await?;
    // "Another one": topics just shown count as the most recent.
    for id in skip.iter().rev() {
        let kind = if id.starts_with("interest:") { TopicKind::Interest } else { TopicKind::General };
        history.insert(0, (id.clone(), kind));
    }
    let mut pick = topics::pick(&general, &interests, lang, l.ws.profile.conversation.interest_ratio, &history);
    // Everything of that kind was skipped: any general topic not skipped.
    if let Some(p) = &pick
        && skip.contains(&p.id)
    {
        pick = general.iter().find(|t| !skip.contains(&t.id)).cloned();
    }
    pick.ok_or_else(|| bad("沒有可用的主題：請在 topics.toml 或興趣中加入主題"))
}

/// The topic the next conversation would have (no model call).
async fn next_topic(State(s): State<Shared>, Query(q): Query<LangQ>) -> R<Value> {
    let l = ws(&s).await?;
    let lang = lang_or_first(&q.lang, &l.ws)?;
    let skip: Vec<String> = q.skip.as_deref().unwrap_or("").split(',').map(str::trim).filter(|x| !x.is_empty()).map(String::from).collect();
    let topic = choose_topic(&l, lang, None, &skip).await?;
    let c = &l.ws.profile.conversation;
    Ok(Json(json!({ "topic": topic, "turns": c.turns, "seconds_per_turn": c.seconds_per_turn, "level": l.ws.profile.difficulty(lang).label })))
}

#[derive(Deserialize)]
struct StartBody {
    lang: Option<String>,
    topic_id: Option<String>,
}

/// Start a conversation: topic, target words, and the AI's opening.
async fn start_conversation(State(s): State<Shared>, Json(b): Json<StartBody>) -> R<Value> {
    let lang = need_lang(&b.lang)?;
    let (mut c, learner, templates, secs) = {
        let l = ws(&s).await?;
        if !l.ws.profile.languages.contains(&lang) {
            return Err(bad(format!("{} 不在你的學習語言中", lang.label())));
        }
        let topic = choose_topic(&l, lang, b.topic_id.as_deref(), &[]).await?;
        let targets = conversation::pick_targets(&l.db, lang, if lang == Lang::En { 4 } else { 3 }).await?;
        let level = l.ws.profile.difficulty(lang).label;
        let c = Conversation {
            lang,
            topic,
            level: level.clone(),
            targets,
            turns_limit: l.ws.profile.conversation.turns,
            turns: vec![],
            engine: String::new(),
            started_at: Utc::now(),
            ended_at: None,
            review: None,
            score: None,
            passed: None,
            points: None,
        };
        (c, learner(&l.ws, lang, &level), ConvTemplates::load(Some(&l.ws.prompts_dir())), l.ws.profile.conversation.seconds_per_turn)
    };
    let p = provider(&s).await?;
    c.engine = p.id();
    let req = conversation::turn_request(&templates, &c, &learner).map_err(|e| bad(e.to_string()))?;
    let reply = conversation::parse_turn(&p.chat(&req).await.map_err(api_err)?).map_err(api_err)?;
    c.turns.push(Turn { role: Speaker::Ai, text: reply.question, recast: String::new(), starters: reply.starters, used: vec![], at: Utc::now() });
    let l = ws(&s).await?;
    let id = conversation::insert(&l.db, &c, "").await?;
    Ok(Json(view(id, "active", "", &c, secs)))
}

async fn load_conversation(l: &crate::Loaded, id: i64) -> Result<(conversation::Row, Conversation), ApiError> {
    let row = conversation::get(&l.db, id).await?.ok_or_else(|| bad("找不到這段對話"))?;
    let c = row.conversation().ok_or_else(|| fail("對話資料無法讀取"))?;
    Ok((row, c))
}

async fn get_conversation(State(s): State<Shared>, AxPath(id): AxPath<i64>) -> R<Value> {
    let l = ws(&s).await?;
    let (row, c) = load_conversation(&l, id).await?;
    Ok(Json(view(id, &row.status, &row.path, &c, l.ws.profile.conversation.seconds_per_turn)))
}

#[derive(Deserialize)]
struct TurnBody {
    text: String,
}

/// One learner turn: recast + next question. The last turn locks the
/// conversation; nothing more can be said in it.
async fn conversation_turn(State(s): State<Shared>, AxPath(id): AxPath<i64>, Json(b): Json<TurnBody>) -> R<Value> {
    let text = b.text.trim().to_string();
    let (mut c, before, learner, templates, secs) = {
        let l = ws(&s).await?;
        let (row, c) = load_conversation(&l, id).await?;
        if row.status != "active" || c.locked() {
            return Err(ApiError(StatusCode::CONFLICT, "這段對話已經完成".into()));
        }
        if text.chars().count() > conversation::MAX_TURN_CHARS {
            return Err(bad(format!("一回合最多 {} 個字", conversation::MAX_TURN_CHARS)));
        }
        if !conversation::is_real(&text, c.lang) {
            return Err(bad("多寫幾個字吧"));
        }
        let before = c.turns_done();
        let lr = learner(&l.ws, c.lang, &c.level);
        (c, before, lr, ConvTemplates::load(Some(&l.ws.prompts_dir())), l.ws.profile.conversation.seconds_per_turn)
    };
    let detected = conversation::detect_used(&text, &c.targets, c.lang);
    c.turns.push(Turn { role: Speaker::Me, text, recast: String::new(), starters: vec![], used: detected.clone(), at: Utc::now() });
    let req = conversation::turn_request(&templates, &c, &learner).map_err(|e| bad(e.to_string()))?;
    let p = provider(&s).await?;
    let reply = conversation::parse_turn(&p.chat(&req).await.map_err(api_err)?).map_err(api_err)?;
    let used = conversation::merge_used(detected, &reply.used_targets, &c.targets);
    if let Some(me) = c.turns.last_mut() {
        me.used = used.clone();
    }
    let last = c.locked();
    let starters = if last { vec![] } else { reply.starters.clone() };
    c.turns.push(Turn { role: Speaker::Ai, text: reply.question.clone(), recast: reply.recast.clone(), starters, used: vec![], at: Utc::now() });
    let l = ws(&s).await?;
    if !conversation::update(&l.db, id, &c, Some(before), "").await? {
        return Err(ApiError(StatusCode::CONFLICT, "這一回合已經送出過了".into()));
    }
    conversation::record_usage(&l.db, c.lang, &used, Utc::now()).await?;
    let status = if last { "locked" } else { "active" };
    let mut out = view(id, status, "", &c, secs);
    out["locked"] = json!(last);
    out["used_now"] = json!(used);
    Ok(Json(out))
}

/// After the last turn: evaluation, points, the Markdown record.
async fn finish_conversation(State(s): State<Shared>, AxPath(id): AxPath<i64>) -> R<Value> {
    let r = rules(&s).await;
    let (mut c, learner, templates) = {
        let l = ws(&s).await?;
        let (row, c) = load_conversation(&l, id).await?;
        if row.status == "done" {
            let mut out = view(id, "done", &row.path, &c, l.ws.profile.conversation.seconds_per_turn);
            out["result"] = result_json(&c, &row.path, None);
            return Ok(Json(out));
        }
        if !c.locked() {
            return Err(bad(format!("還有 {} 回合", c.turns_limit - c.turns_done())));
        }
        let lr = learner(&l.ws, c.lang, &c.level);
        (c, lr, ConvTemplates::load(Some(&l.ws.prompts_dir())))
    };
    let req = conversation::review_request(&templates, &c, &learner).map_err(|e| bad(e.to_string()))?;
    let p = provider(&s).await?;
    let review = conversation::parse_review(&p.complete(&req).await.map_err(api_err)?).map_err(api_err)?;
    let score = review.score();
    // Every counted turn was checked for real content when it was sent.
    let passed = c.locked() && score + 1e-9 >= r.points.conversation_pass;
    let pts = points::award(passed, Some(score), review.excellent(), &r.points);
    c.score = Some((score * 100.0).round() / 100.0);
    c.passed = Some(passed);
    c.points = Some(pts);
    c.review = Some(review);
    c.ended_at = Some(Utc::now());
    let lang = c.lang;
    let rel = {
        let l = ws(&s).await?;
        // Someone may have finished it meanwhile.
        let (row, _) = load_conversation(&l, id).await?;
        if row.status == "done" {
            return Err(ApiError(StatusCode::CONFLICT, "這段對話已經結算過了".into()));
        }
        let date = Local::now().format("%Y-%m-%d").to_string();
        let dir = l.ws.conversations_dir(lang);
        std::fs::create_dir_all(&dir).map_err(|e| fail(e.to_string()))?;
        let path = free_path(&dir, &format!("{date}-{}", slug(&c.topic.name, 40)), "md");
        let md = conversation::render_markdown(&templates, &c, &date).map_err(|e| fail(e.to_string()))?;
        std::fs::write(&path, conversation::embed_data(&md, &c)).map_err(|e| fail(e.to_string()))?;
        let rel = l.ws.relative(&path);
        let rec = PointRecord::now(lang, "conversation", Skill::Writing, pts, c.score, &l.ws.profile_relative(&path));
        points::record(&l.ws, &l.db, &rec, Some(c.started_at)).await.map_err(fail)?;
        conversation::update(&l.db, id, &c, None, &rel).await?;
        conversation::record_topic(&l.db, &c).await?;
        rel
    };
    let level_up = try_level_up(&s, lang).await?;
    let l = ws(&s).await?;
    let mut out = view(id, "done", &rel, &c, l.ws.profile.conversation.seconds_per_turn);
    out["result"] = result_json(&c, &rel, level_up);
    out["summary"] = summary(&l.ws, &l.db, lang, &r).await?;
    Ok(Json(out))
}

fn result_json(c: &Conversation, path: &str, level_up: Option<Value>) -> Value {
    let used = c.used_targets();
    let unused: Vec<&String> = c.targets.iter().map(|t| &t.term).filter(|t| !used.contains(t)).collect();
    json!({
        "score": c.score, "passed": c.passed, "points": c.points, "review": c.review,
        "excellent": c.review.as_ref().map(|r| r.excellent()),
        "used": used, "unused": unused, "path": path, "level_up": level_up,
    })
}

// ── the user's picture ──────────────────────────────────

/// Largest picture accepted (after the browser has resized it).
pub const AVATAR_MAX_BYTES: usize = 2 * 1024 * 1024;

/// Pictures stay on this computer, next to the user's database, never in
/// the workspace repository: `<app dir>/avatars/<workspace id>/<user id>.<ext>`.
fn avatar_dir(st: &crate::Settings, ws_id: &str) -> PathBuf {
    st.app_dir.clone().unwrap_or_else(std::env::temp_dir).join("avatars").join(ws_id)
}

const AVATAR_EXTS: &[(&str, &str)] = &[("jpg", "image/jpeg"), ("png", "image/png"), ("webp", "image/webp")];

fn image_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("png")
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// The picture file of a user, if any.
pub fn avatar_file(st: &crate::Settings, ws_id: &str, user: &str) -> Option<PathBuf> {
    let ok = !user.is_empty() && user.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return None;
    }
    let dir = avatar_dir(st, ws_id);
    AVATAR_EXTS.iter().map(|(e, _)| dir.join(format!("{user}.{e}"))).find(|p| p.is_file())
}

/// URL for the UI (with the file time, so a new picture shows at once).
pub fn avatar_url(st: &crate::Settings, ws_id: &str, user: &str) -> Option<String> {
    let f = avatar_file(st, ws_id, user)?;
    let v = std::fs::metadata(&f).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis()).unwrap_or(0);
    Some(format!("/api/avatars/{user}?v={v}"))
}

async fn upload_avatar(State(s): State<Shared>, body: axum::body::Bytes) -> R<Value> {
    if body.len() > AVATAR_MAX_BYTES {
        return Err(bad("圖片太大（上限 2 MB）"));
    }
    let ext = image_ext(&body).ok_or_else(|| bad("只接受 JPEG、PNG 或 WebP 圖片"))?;
    let (ws_id, user) = {
        let l = ws(&s).await?;
        (l.ws.meta.id.clone(), l.ws.profile.id.clone())
    };
    let st = s.settings.read().await.clone();
    let dir = avatar_dir(&st, &ws_id);
    std::fs::create_dir_all(&dir).map_err(|e| fail(e.to_string()))?;
    for (e, _) in AVATAR_EXTS {
        let _ = std::fs::remove_file(dir.join(format!("{user}.{e}")));
    }
    std::fs::write(dir.join(format!("{user}.{ext}")), &body).map_err(|e| fail(e.to_string()))?;
    Ok(Json(json!({ "avatar": avatar_url(&st, &ws_id, &user), "bytes": body.len() })))
}

async fn delete_avatar(State(s): State<Shared>) -> R<Value> {
    let (ws_id, user) = {
        let l = ws(&s).await?;
        (l.ws.meta.id.clone(), l.ws.profile.id.clone())
    };
    let st = s.settings.read().await.clone();
    let mut removed = false;
    while let Some(f) = avatar_file(&st, &ws_id, &user) {
        std::fs::remove_file(f).map_err(|e| fail(e.to_string()))?;
        removed = true;
    }
    Ok(Json(json!({ "removed": removed })))
}

/// Any user of the open workspace (for the user menu and person cards).
async fn avatar(State(s): State<Shared>, AxPath(id): AxPath<String>) -> Response {
    let ws_id = match s.root.read().await.as_ref() {
        Some(r) => r.root.meta.id.clone(),
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    let st = s.settings.read().await.clone();
    let Some(f) = avatar_file(&st, &ws_id, &id) else { return StatusCode::NOT_FOUND.into_response() };
    let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
    let ct = AVATAR_EXTS.iter().find(|(e, _)| *e == ext).map(|(_, c)| *c).unwrap_or("application/octet-stream");
    match std::fs::read(&f) {
        Ok(bytes) => ([(header::CONTENT_TYPE, ct), (header::CACHE_CONTROL, "no-cache")], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
