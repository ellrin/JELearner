//! Interests from uploads (docs/plan.md §19.2.7, stage F4).
//!
//! - `POST /api/interests/suggest {lang, file | text, name}`: the model reads
//!   an upload (a file the import just stored in the learner's own folder, or
//!   text) and suggests interest keywords; nothing is written.
//! - `POST /api/interests {keywords}`: the learner confirmed (and maybe
//!   edited) them; they are added to `profiles/<id>/interests.csv`.

use crate::api::{need_lang, ws};
use crate::courses::{course_err, read_workspace_list, templates};
use crate::{bad, ApiError, Shared};
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use jl_core::course::picker;
use jl_core::course::topics as ctopics;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

type R<T> = Result<Json<T>, ApiError>;

pub fn routes() -> Router<Shared> {
    Router::new().route("/api/interests", post(add)).route("/api/interests/suggest", post(suggest))
}

#[derive(Deserialize)]
struct SuggestBody {
    lang: Option<String>,
    /// Workspace-relative path of an upload in the learner's own folder.
    file: Option<String>,
    #[serde(default)]
    text: String,
    #[serde(default)]
    name: String,
}

/// Files whose text the model can read here (pictures and PDFs are kept as they are).
fn readable(path: &str) -> bool {
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "md" | "markdown" | "txt" | "csv" | "tsv" | "json")
}

async fn suggest(State(s): State<Shared>, Json(b): Json<SuggestBody>) -> R<Value> {
    let lang = need_lang(&b.lang)?;
    let (req, existing) = {
        let l = ws(&s).await?;
        let (text, name) = match b.file.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
            Some(f) if !readable(f) => return Err(bad("這種檔案還讀不出文字")),
            Some(f) => (read_workspace_list(&l.ws, f)?, Path::new(f).file_name().and_then(|n| n.to_str()).unwrap_or(f).to_string()),
            None => (b.text.clone(), b.name.clone()),
        };
        if text.trim().is_empty() {
            return Err(bad("沒有內容可以讀"));
        }
        let existing: Vec<String> = ctopics::parse_interests(&std::fs::read_to_string(l.ws.interests_file()).unwrap_or_default()).into_iter().map(|i| i.keyword).collect();
        let req = picker::suggest_request(&templates(&l.ws), lang, &l.ws.profile.native_language, &existing, &name, text.trim_start_matches('\u{feff}')).map_err(course_err)?;
        (req, existing)
    };
    let provider = crate::provider(&s).await?;
    let reply = provider.complete(&req).await.map_err(crate::study::api_err)?;
    let g = picker::parse_suggest(&reply, &existing).map_err(course_err)?;
    Ok(Json(json!({ "summary": g.summary, "keywords": g.keywords })))
}

#[derive(Deserialize)]
struct AddBody {
    keywords: Vec<String>,
}

async fn add(State(s): State<Shared>, Json(b): Json<AddBody>) -> R<Value> {
    let keywords: Vec<String> = b.keywords.iter().map(|k| k.trim().to_string()).filter(|k| !k.is_empty() && k.chars().count() <= 30 && !k.contains([',', '\n', '"'])).collect();
    if keywords.is_empty() {
        return Err(bad("沒有要記住的關鍵字"));
    }
    let l = ws(&s).await?;
    let added = crate::api::add_interests(&l.ws, &keywords)?;
    Ok(Json(json!({ "added": added, "file": l.ws.relative(&l.ws.interests_file()) })))
}
