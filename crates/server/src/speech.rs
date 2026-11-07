//! Speech handlers (docs/plan.md §15, docs/decisions.md §14): on-device
//! recognition through [`MacSpeech`] when this computer has it.
//!
//! - `GET /api/speech`: the engine (`macos` or none) and, per language,
//!   whether its recognition model is downloaded.
//! - `POST /api/speech/install` `{lang}`: download a language's model (the
//!   system keeps it; takes a few minutes the first time).
//! - `POST /api/speech/transcribe?lang=en`: body = a WAV recording (the page
//!   records 16 kHz mono, at most 15 s). 409 when the model is missing.
//!
//! Speaking is not here: the page's `speechSynthesis` uses the system
//! voices. The recording is written to a temporary file for the helper and
//! deleted right after; nothing leaves the computer.

use crate::api::need_lang;
use crate::{bad, unavailable, ApiError, Shared};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use jl_core::speech::{Audio, MacSpeech, SpeechError, SpeechProvider};
use serde::Deserialize;
use serde_json::{json, Value};

type R<T> = Result<Json<T>, ApiError>;

/// Largest recording accepted: 15 s of 16 kHz mono 16-bit WAV is about
/// 480 KB; this leaves room for a 44.1 kHz recording from another browser.
pub const AUDIO_MAX_BYTES: usize = 2 * 1024 * 1024;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/speech", get(status))
        .route("/api/speech/install", post(install))
        .route("/api/speech/transcribe", post(transcribe).layer(DefaultBodyLimit::max(AUDIO_MAX_BYTES)))
}

async fn engine(s: &Shared) -> Option<MacSpeech> {
    MacSpeech::find(s.settings.read().await.speech_bin.as_deref()).map(MacSpeech::new)
}

fn speech_error(e: SpeechError) -> ApiError {
    match e {
        SpeechError::NotInstalled(_) => ApiError(StatusCode::CONFLICT, e.to_string()),
        SpeechError::Unavailable(_) => unavailable(e.to_string()),
        SpeechError::Failed(_) => ApiError(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
    }
}

async fn status(State(s): State<Shared>) -> Json<Value> {
    let Some(mac) = engine(&s).await else {
        return Json(json!({ "engine": null, "langs": [] }));
    };
    match mac.status().await {
        Ok(langs) => Json(json!({ "engine": mac.id(), "langs": langs })),
        // The helper is configured but cannot run: no local engine.
        Err(e) => Json(json!({ "engine": null, "langs": [], "error": e.to_string() })),
    }
}

#[derive(Deserialize)]
struct LangBody {
    lang: Option<String>,
}

async fn install(State(s): State<Shared>, Json(b): Json<LangBody>) -> R<Value> {
    let lang = need_lang(&b.lang)?;
    let mac = engine(&s).await.ok_or_else(|| unavailable("這台電腦沒有本機語音辨識"))?;
    // One download at a time: a second click waits for the first.
    static ONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _one = ONE.lock().await;
    mac.install(lang).await.map_err(speech_error)?;
    let langs = mac.status().await.map_err(speech_error)?;
    Ok(Json(json!({ "engine": mac.id(), "langs": langs })))
}

#[derive(Deserialize)]
struct LangQuery {
    lang: Option<String>,
}

async fn transcribe(State(s): State<Shared>, Query(q): Query<LangQuery>, body: Bytes) -> R<Value> {
    let lang = need_lang(&q.lang)?;
    if body.is_empty() {
        return Err(bad("沒有錄到聲音"));
    }
    if body.len() < 12 || &body[0..4] != b"RIFF" || &body[8..12] != b"WAVE" {
        return Err(bad("錄音格式不對（需要 WAV）"));
    }
    let mac = engine(&s).await.ok_or_else(|| unavailable("這台電腦沒有本機語音辨識"))?;
    let t = mac.transcribe(&Audio { mime: "audio/wav".into(), bytes: body.to_vec() }, lang).await.map_err(speech_error)?;
    Ok(Json(json!({ "text": t.text, "confidence": t.confidence })))
}
