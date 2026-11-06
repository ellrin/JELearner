//! Review notebook handlers (docs/plan.md §11.1): pages by date, one page,
//! the learner's notes, and pictures.
//!
//! Pages are Markdown in the workspace (`profiles/<id>/notebook/<lang>/`) and
//! sync with it. Pictures stay on this computer, next to the databases:
//! `<app dir>/notebook-images/<workspace id>/<user id>/<image id>.<ext>`,
//! never in the workspace repository. The browser compresses a picture
//! before sending it (longest side ≤ 1600 px, JPEG or WebP about 0.8).

use crate::api::{parse_lang, ws};
use crate::study::image_ext;
use crate::{bad, fail, ApiError, Shared};
use axum::extract::{Path as AxPath, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use jl_core::notebook;
use jl_core::Lang;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

type R<T> = Result<Json<T>, ApiError>;

/// Largest picture accepted (after the browser has compressed it).
pub const IMAGE_MAX_BYTES: usize = 3 * 1024 * 1024;
const IMAGE_EXTS: &[(&str, &str)] = &[("jpg", "image/jpeg"), ("png", "image/png"), ("webp", "image/webp")];

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/notebook", get(list))
        .route("/api/notebook/images", post(upload_image).layer(axum::extract::DefaultBodyLimit::max(IMAGE_MAX_BYTES + 1024 * 1024)))
        .route("/api/notebook/images/{id}", get(image).delete(delete_image))
        .route("/api/notebook/{lang}/{stem}", get(page).put(save_notes))
}

fn nb_err(e: notebook::NotebookError) -> ApiError {
    match e {
        notebook::NotebookError::NotFound => ApiError(StatusCode::NOT_FOUND, e.to_string()),
        notebook::NotebookError::NoMarkers(_) => bad(e.to_string()),
        notebook::NotebookError::Io(_) => fail(e.to_string()),
    }
}

fn valid_image_id(id: &str) -> bool {
    (8..=40).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric())
}

async fn image_dir(s: &Shared) -> Result<PathBuf, ApiError> {
    let (ws_id, user) = {
        let l = ws(s).await?;
        (l.ws.meta.id.clone(), l.ws.profile.id.clone())
    };
    let st = s.settings.read().await;
    Ok(st.app_dir.clone().unwrap_or_else(std::env::temp_dir).join("notebook-images").join(ws_id).join(user))
}

fn image_file(dir: &std::path::Path, id: &str) -> Option<PathBuf> {
    if !valid_image_id(id) {
        return None;
    }
    IMAGE_EXTS.iter().map(|(e, _)| dir.join(format!("{id}.{e}"))).find(|p| p.is_file())
}

fn page_summary(p: &notebook::Page) -> Value {
    let d = p.data.as_ref();
    json!({
        "lang": p.lang, "stem": p.stem, "title": p.title, "kind": p.kind, "date": p.date, "source": p.source,
        "mistakes": d.map(|d| d.mistakes.len()).unwrap_or(0), "words": d.map(|d| d.words.len()).unwrap_or(0),
        "grammar": d.map(|d| d.grammar.len()).unwrap_or(0), "better": d.map(|d| d.better.len()).unwrap_or(0),
        "has_notes": !p.notes.trim().is_empty(), "images": p.images.len(),
    })
}

#[derive(Deserialize, Default)]
struct LangQ {
    lang: Option<String>,
}

async fn list(State(s): State<Shared>, Query(q): Query<LangQ>) -> R<Value> {
    let l = ws(&s).await?;
    let pages = notebook::list(&l.ws, parse_lang(&q.lang)?);
    Ok(Json(json!(pages.iter().map(page_summary).collect::<Vec<_>>())))
}

async fn page_json(s: &Shared, p: notebook::Page) -> Result<Value, ApiError> {
    let dir = image_dir(s).await?;
    let l = ws(s).await?;
    let local: Vec<&String> = p.images.iter().filter(|id| image_file(&dir, id).is_some()).collect();
    let lesson_id = if p.kind == "lesson" { l.db.lesson_by_path(&p.source).await? } else { None };
    let conversation_id: Option<i64> =
        if p.kind == "conversation" { sqlx::query_scalar("SELECT id FROM conversations WHERE path = ?").bind(&p.source).fetch_optional(l.db.pool()).await? } else { None };
    let course: Option<(String, Option<i64>)> = match lesson_id {
        Some(id) => sqlx::query_as("SELECT course, course_lesson FROM lessons WHERE id = ? AND course != ''").bind(id).fetch_optional(l.db.pool()).await?,
        None => None,
    };
    Ok(json!({
        "page": p, "local_images": local, "lesson_id": lesson_id, "conversation_id": conversation_id,
        "course": course.map(|(c, n)| json!({ "id": c, "lesson": n })),
    }))
}

async fn page(State(s): State<Shared>, AxPath((lang, stem)): AxPath<(String, String)>) -> R<Value> {
    let lang: Lang = lang.parse().map_err(bad)?;
    let p = {
        let l = ws(&s).await?;
        notebook::read(&l.ws, lang, &stem).map_err(nb_err)?
    };
    Ok(Json(page_json(&s, p).await?))
}

#[derive(Deserialize)]
struct NotesBody {
    notes: String,
}

/// Save the learner's notes; the app section is never touched.
async fn save_notes(State(s): State<Shared>, AxPath((lang, stem)): AxPath<(String, String)>, Json(b): Json<NotesBody>) -> R<Value> {
    let lang: Lang = lang.parse().map_err(bad)?;
    if b.notes.chars().count() > 50_000 {
        return Err(bad("筆記太長了（上限 5 萬字）"));
    }
    let p = {
        let l = ws(&s).await?;
        notebook::write_notes(&l.ws, lang, &stem, &b.notes).map_err(nb_err)?
    };
    Ok(Json(page_json(&s, p).await?))
}

fn new_image_id(bytes: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    let a = h.finish();
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    t.hash(&mut h);
    format!("{:016x}{:08x}", a ^ h.finish(), (t as u64) & 0xffff_ffff)
}

async fn upload_image(State(s): State<Shared>, body: axum::body::Bytes) -> R<Value> {
    if body.len() > IMAGE_MAX_BYTES {
        return Err(bad("圖片太大（上限 3 MB）"));
    }
    let ext = image_ext(&body).ok_or_else(|| bad("只接受 JPEG、PNG 或 WebP 圖片"))?;
    let dir = image_dir(&s).await?;
    std::fs::create_dir_all(&dir).map_err(|e| fail(e.to_string()))?;
    let id = new_image_id(&body);
    std::fs::write(dir.join(format!("{id}.{ext}")), &body).map_err(|e| fail(e.to_string()))?;
    Ok(Json(json!({ "id": id, "ref": format!("{}{id}", notebook::IMAGE_SCHEME), "url": format!("/api/notebook/images/{id}"), "bytes": body.len() })))
}

async fn image(State(s): State<Shared>, AxPath(id): AxPath<String>) -> Response {
    let Ok(dir) = image_dir(&s).await else { return StatusCode::NOT_FOUND.into_response() };
    let Some(f) = image_file(&dir, &id) else { return StatusCode::NOT_FOUND.into_response() };
    let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
    let ct = IMAGE_EXTS.iter().find(|(e, _)| *e == ext).map(|(_, c)| *c).unwrap_or("application/octet-stream");
    match std::fs::read(&f) {
        Ok(bytes) => ([(header::CONTENT_TYPE, ct), (header::CACHE_CONTROL, "private, max-age=86400")], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn delete_image(State(s): State<Shared>, AxPath(id): AxPath<String>) -> R<Value> {
    let dir = image_dir(&s).await?;
    let mut removed = false;
    while let Some(f) = image_file(&dir, &id) {
        std::fs::remove_file(f).map_err(|e| fail(e.to_string()))?;
        removed = true;
    }
    Ok(Json(json!({ "removed": removed })))
}
