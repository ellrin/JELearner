//! Study buddies (docs/plan.md §16.4, the shared workspace): the members of
//! a workspace see each other's points and streak.
//!
//! Everything comes from the files every member already syncs
//! (`profiles/<id>/records/points/*.csv`, `profile.toml`), so no extra
//! service is needed and nothing new leaves the private repository.
//!
//! - `GET /api/buddies` — one card per member: today, the last seven days,
//!   the streak, the level, and (for the admin) how far a member's iPad
//!   lessons are prepared.
//! - `profiles/<id>/ipad/buddies.json` — the same cards for a member's iPad,
//!   written by the admin's computer before it syncs (`write_ipad_files`).

use crate::{ApiError, Shared};
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use jl_core::points::{self, BuddyStats};
use jl_core::workspace::Root;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

type R<T> = Result<Json<T>, ApiError>;

pub fn routes() -> Router<Shared> {
    Router::new().route("/api/buddies", get(list))
}

/// Lessons waiting on a member's iPad (`ipad/index.json`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Prepared {
    /// The last day with a lesson (`YYYY-MM-DD`), today or later.
    pub until: Option<String>,
    /// Days with lessons from today on.
    pub days: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Buddy {
    pub id: String,
    pub name: String,
    pub color: String,
    /// Level per language studied (`en` → `B1-2`).
    pub levels: BTreeMap<String, String>,
    /// Daily goal in points.
    pub goal: f64,
    #[serde(flatten)]
    pub stats: BuddyStats,
    /// Only for members whose lessons are prepared for an iPad.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepared: Option<Prepared>,
}

fn prepared(root: &Root, id: &str, today: &str) -> Option<Prepared> {
    let text = std::fs::read_to_string(root.profiles_dir().join(id).join("ipad").join("index.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let days: Vec<&String> = v["days"].as_object()?.iter().filter(|(k, list)| k.as_str() >= today && list.as_array().is_some_and(|l| !l.is_empty())).map(|(k, _)| k).collect();
    Some(Prepared { until: days.iter().max().map(|d| d.to_string()), days: days.len() })
}

/// Every member's card, in the workspace's member order.
pub fn cards(root: &Root, rules: &jl_core::rules::Rules, today: chrono::NaiveDate) -> Vec<Buddy> {
    let day = today.format("%Y-%m-%d").to_string();
    root.list_profiles()
        .into_iter()
        .map(|p| {
            let (recs, _) = points::read_dir(&root.profiles_dir().join(&p.id).join("records").join("points"));
            Buddy {
                levels: p.languages.iter().map(|l| (l.code().to_string(), p.lang(*l).level)).collect(),
                goal: rules.daily_goal(p.daily_minutes),
                stats: points::buddy_stats(&recs, today),
                prepared: prepared(root, &p.id, &day),
                id: p.id,
                name: p.name,
                color: p.color,
            }
        })
        .collect()
}

async fn list(State(s): State<Shared>) -> R<Value> {
    let rules = crate::study::rules(&s).await;
    let me = s.loaded.read().await.as_ref().map(|l| l.ws.profile.id.clone()).unwrap_or_default();
    let settings = s.settings.read().await.clone();
    let root = s.root.read().await;
    let Some(open) = root.as_ref() else { return Ok(Json(json!({ "buddies": [] }))) };
    let admin = open.root.is_admin(&me);
    let list: Vec<Value> = cards(&open.root, &rules, crate::study::local_today())
        .into_iter()
        .map(|b| {
            let mut v = serde_json::to_value(&b).unwrap_or(Value::Null);
            v["me"] = json!(b.id == me);
            v["avatar"] = json!(crate::study::avatar_url(&settings, &open.root.meta.id, &b.id));
            // How far the iPad lessons are prepared is the admin's business.
            if !admin {
                v.as_object_mut().map(|o| o.remove("prepared"));
            }
            v
        })
        .collect();
    Ok(Json(json!({ "buddies": list, "admin": admin })))
}

/// The cards for each iPad member (`profiles/<id>/ipad/buddies.json`), so the
/// iPad shows them without the computer. Written only when the numbers
/// changed; returns how many files were written.
pub async fn write_ipad_files(s: &Shared) -> usize {
    let rules = crate::study::rules(s).await;
    let root = s.root.read().await;
    let Some(open) = root.as_ref() else { return 0 };
    let today = crate::study::local_today();
    let all = cards(&open.root, &rules, today);
    if all.len() < 2 {
        return 0;
    }
    let mut written = 0;
    for b in &all {
        let dir = open.root.profiles_dir().join(&b.id).join("ipad");
        if !dir.join("index.json").is_file() {
            continue;
        }
        let list: Vec<Value> = all
            .iter()
            .map(|x| {
                let mut v = serde_json::to_value(x).unwrap_or(Value::Null);
                v["me"] = json!(x.id == b.id);
                v.as_object_mut().map(|o| o.remove("prepared"));
                v
            })
            .collect();
        let text = serde_json::to_string_pretty(&json!({ "version": 1, "as_of": today.format("%Y-%m-%d").to_string(), "buddies": list })).unwrap_or_default();
        let path = dir.join("buddies.json");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) && std::fs::write(&path, text).is_ok() {
            written += 1;
        }
    }
    written
}
