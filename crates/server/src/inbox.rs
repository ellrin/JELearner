//! Results the iPad wrote back (docs/plan.md stage F5, step 2).
//!
//! The iPad writes raw results into `profiles/<id>/ipad/inbox/*.json`; the
//! admin's computer applies them with the same rules as the app itself
//! (points, mistakes, course progress, difficulty, level, conversation
//! records), then moves each file to `inbox/done/` (or `inbox/bad/` with a
//! note when it cannot be read). The rules live only here.
//!
//! An inbox file:
//!
//! ```json
//! { "version": 1, "id": "…", "member": "daidai", "results": [
//!   { "type": "unit", "course": "…", "lesson": 1, "unit": 0, "cards": 12, "score": 0.8 },
//!   { "type": "unit", "course": "…", "lesson": 1, "unit": 2, "answers": [{ "index": 0, "answer": "b" }] },
//!   { "type": "unit", "course": "…", "lesson": 1, "unit": 3, "items": [{ "prompt": "…", "answer": "…", "model": "…", "correct": true }] },
//!   { "type": "conversation", "course": "…", "lesson": 1, "unit": 4, "started_at": "…",
//!     "turns": [{ "role": "ai", "text": "…" }, { "role": "me", "text": "…" }], "review": { …the model's evaluation… } } ] }
//! ```
//!
//! - `POST /api/family/inbox`: apply every member's inbox (admin only); the
//!   workspace sync and the lesson preparation do it too.

use crate::courses::{apply_unit, conversation_unit_done, date, UnitBody};
use crate::study::{auto_difficulty_for, course_start, level_up_for, rules};
use crate::{bad, ApiError, Loaded, Shared};
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Local, Utc};
use jl_core::conversation::{self, CourseLink, Speaker, Templates as ConvTemplates, Turn};
use jl_core::points::{self, PointRecord, Skill};
use jl_core::rules::Rules;
use jl_core::workspace::slug;
use jl_core::Lang;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

type R<T> = Result<Json<T>, ApiError>;

pub fn routes() -> Router<Shared> {
    Router::new().route("/api/family/inbox", post(apply_all))
}

#[derive(Deserialize)]
struct InboxFile {
    #[serde(default)]
    member: String,
    #[serde(default)]
    results: Vec<Item>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Item {
    Unit {
        course: String,
        lesson: u32,
        unit: u32,
        #[serde(flatten)]
        body: UnitBody,
    },
    Conversation {
        course: String,
        lesson: u32,
        unit: u32,
        started_at: Option<DateTime<Utc>>,
        turns: Vec<InTurn>,
        review: Value,
    },
}

#[derive(Deserialize)]
struct InTurn {
    role: Speaker,
    text: String,
    #[serde(default)]
    recast: String,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct Applied {
    pub member: String,
    pub files: usize,
    pub units: usize,
    pub conversations: usize,
    pub points: f64,
    pub errors: Vec<String>,
}

async fn apply_all(State(s): State<Shared>) -> R<Value> {
    {
        let l = crate::api::ws(&s).await?;
        let root = s.root.read().await;
        let open = root.as_ref().ok_or_else(|| bad("尚未選擇工作區"))?;
        if !open.root.is_admin(&l.ws.profile.id) {
            return Err(bad("只有管理員可以收回家人的作答"));
        }
    }
    let applied = process_all(&s).await;
    // Their new points belong on everyone's buddies strip.
    crate::buddies::write_ipad_files(&s).await;
    Ok(Json(json!({ "applied": applied })))
}

/// Every member's inbox, the active user's own too (the admin may study on
/// an iPad as well). Errors are reported per member, never raised.
pub async fn process_all(s: &Shared) -> Vec<Applied> {
    let (members, me) = {
        let root = s.root.read().await;
        let Some(open) = root.as_ref() else { return vec![] };
        let me = s.loaded.read().await.as_ref().map(|l| l.ws.profile.id.clone()).unwrap_or_default();
        let ids: Vec<String> = open.root.list_profiles().into_iter().map(|p| p.id).collect();
        (ids.into_iter().filter(|id| open.root.root.join("profiles").join(id).join("ipad/inbox").is_dir()).collect::<Vec<_>>(), me)
    };
    let mut out = vec![];
    for m in members {
        let res = process_member(s, &m).await;
        // The active user's own results: the open workspace reads its files again.
        if m == me && res.as_ref().is_ok_and(|a| a.files > 0) {
            crate::api::reload_active(s).await;
        }
        match res {
            Ok(a) if a.files > 0 || !a.errors.is_empty() => out.push(a),
            Ok(_) => {}
            Err(e) => out.push(Applied { member: m, errors: vec![e.1], ..Default::default() }),
        }
    }
    out
}

fn inbox_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect()).unwrap_or_default();
    v.sort();
    v
}

fn move_to(file: &Path, sub: &str) -> std::io::Result<()> {
    let dir = file.parent().expect("in a folder").join(sub);
    std::fs::create_dir_all(&dir)?;
    std::fs::rename(file, dir.join(file.file_name().expect("a file")))
}

pub async fn process_member(s: &Shared, member: &str) -> Result<Applied, ApiError> {
    let dir = {
        let root = s.root.read().await;
        let open = root.as_ref().ok_or_else(|| bad("尚未選擇工作區"))?;
        open.root.root.join("profiles").join(member).join("ipad/inbox")
    };
    let mut a = Applied { member: member.to_string(), ..Default::default() };
    let files = inbox_files(&dir);
    if files.is_empty() {
        return Ok(a);
    }
    let mut m = crate::prepare::member_loaded(s, member).await?;
    let r = rules(s).await;
    let mut langs: Vec<Lang> = vec![];
    for f in files {
        let parsed: Result<InboxFile, String> = std::fs::read_to_string(&f).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()));
        let parsed = parsed.and_then(|x| if x.member.is_empty() || x.member == member { Ok(x) } else { Err(format!("這是 {} 的檔案", x.member)) });
        let inbox = match parsed {
            Ok(x) => x,
            Err(e) => {
                a.errors.push(format!("{}：讀不懂（{e}）", f.file_name().unwrap_or_default().to_string_lossy()));
                let _ = std::fs::write(f.with_extension("error.txt"), &e);
                let _ = move_to(&f, "bad");
                let _ = move_to(&f.with_extension("error.txt"), "bad");
                continue;
            }
        };
        for item in inbox.results {
            match apply(&m, &r, item).await {
                Ok((lang, pts, conv)) => {
                    if !langs.contains(&lang) {
                        langs.push(lang);
                    }
                    a.points += pts;
                    if conv { a.conversations += 1 } else { a.units += 1 }
                }
                Err(e) => a.errors.push(format!("{}：{}", f.file_name().unwrap_or_default().to_string_lossy(), e.1)),
            }
        }
        move_to(&f, "done").map_err(|e| bad(e.to_string()))?;
        a.files += 1;
    }
    // Difficulty and level, as after a unit in the app.
    for lang in langs {
        auto_difficulty_for(&mut m, lang, &r).await?;
        level_up_for(&mut m, lang, &r).await?;
    }
    Ok(a)
}

/// One result: `(lang, points, is_conversation)`.
async fn apply(m: &Loaded, r: &Rules, item: Item) -> Result<(Lang, f64, bool), ApiError> {
    match item {
        Item::Unit { course, lesson, unit, body } => {
            let (lang, _, _, _, pts, _, _) = apply_unit(m, r, &course, lesson, unit, body, &date()).await?;
            Ok((lang, pts, false))
        }
        Item::Conversation { course, lesson, unit, started_at, turns, review } => {
            let link = CourseLink { course: course.clone(), lesson, unit };
            let lang = jl_core::course::store::load(&m.ws, &course).map_err(|e| bad(e.to_string()))?.lang;
            // Already recorded (the file came twice): nothing more.
            if crate::study::course_conversation_done(&m.db, &link).await? {
                return Ok((lang, 0.0, true));
            }
            let (topic, targets, limit, _) = course_start(m, lang, &link).await?;
            let level = m.ws.profile.difficulty(lang).label;
            let now = Utc::now();
            let mut c = conversation::Conversation {
                lang,
                topic,
                level: level.clone(),
                targets,
                turns_limit: limit,
                turns: turns
                    .into_iter()
                    .map(|t| {
                        Turn { role: t.role, text: t.text, recast: t.recast, starters: vec![], used: vec![], at: now }
                    })
                    .collect(),
                engine: "ipad".into(),
                started_at: started_at.unwrap_or(now),
                ended_at: Some(now),
                review: None,
                score: None,
                passed: None,
                points: None,
                course: Some(link.clone()),
            };
            // Target words the learner used, as the app detects them.
            for t in c.turns.iter_mut().filter(|t| t.role == Speaker::Me) {
                t.used = conversation::detect_used(&t.text, &c.targets, lang);
            }
            let review = conversation::parse_review(&review.to_string()).map_err(|e| bad(format!("對話評分讀不懂：{e}")))?;
            let score = review.score();
            let passed = c.locked() && score + 1e-9 >= r.points.conversation_pass;
            let pts = points::award(passed, Some(score), review.excellent(), &r.points);
            c.score = Some((score * 100.0).round() / 100.0);
            c.passed = Some(passed);
            c.points = Some(pts);
            c.review = Some(review);
            let t = ConvTemplates::load(Some(&m.ws.prompts_dir()));
            let day = Local::now().format("%Y-%m-%d").to_string();
            let dir = m.ws.conversations_dir(lang);
            std::fs::create_dir_all(&dir).map_err(|e| bad(e.to_string()))?;
            let path = crate::api::free_path(&dir, &format!("{day}-{}", slug(&c.topic.name, 40)), "md");
            let md = conversation::render_markdown(&t, &c, &day).map_err(|e| bad(e.to_string()))?;
            std::fs::write(&path, conversation::embed_data(&md, &c)).map_err(|e| bad(e.to_string()))?;
            let rel = m.ws.relative(&path);
            let rec = PointRecord::now(lang, "conversation", Skill::Writing, pts, c.score, &m.ws.profile_relative(&path));
            points::record(&m.ws, &m.db, &rec, Some(c.started_at)).await.map_err(|e| bad(e.to_string()))?;
            conversation::insert(&m.db, &c, &rel).await?;
            conversation::record_topic(&m.db, &c).await?;
            conversation_unit_done(m, &link, passed, c.score).await?;
            Ok((lang, pts, true))
        }
    }
}
