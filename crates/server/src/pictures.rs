//! Pictures between the paragraphs of a lesson (docs/plan.md §19.4.1).
//!
//! Never generated: the pictures the lesson's reference pages show
//! (Wikipedia, Wikivoyage), only files on Wikimedia Commons under a free
//! licence, with their author and licence. Nothing goes into the private
//! repository: the lesson keeps the pictures' addresses and credits, and each
//! device downloads and keeps them itself. Here that is
//! `<config dir>/JELearner/cache/pictures/`, shrunk to a small JPEG, and
//! everything older than two weeks is deleted.
//!
//! - `GET /api/pictures?u=<address>` — a lesson's picture (downloaded,
//!   shrunk and kept on first use; only addresses chosen for a lesson).

use crate::{ApiError, Shared};
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use jl_core::course::picture::{self, Candidate, LessonPicture};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Pictures are kept this long, then deleted.
pub const KEEP: Duration = Duration::from_secs(14 * 24 * 3600);
/// At most this many pictures in one lesson.
const MAX_PICTURES: usize = 4;
/// At most this many reference pages are read for one lesson.
const MAX_PAGES: usize = 3;

pub fn routes() -> Router<Shared> {
    Router::new().route("/api/pictures", get(file))
}

pub(crate) fn dir(app_dir: &Path) -> PathBuf {
    app_dir.join("cache").join("pictures")
}

async fn cache_dir(s: &Shared) -> Option<PathBuf> {
    s.settings.read().await.app_dir.as_deref().map(dir)
}

/// Delete everything older than `KEEP` (at `now`); returns how many files went.
pub fn prune(dir: &Path, now: SystemTime) -> usize {
    let mut n = 0;
    for sub in ["", "lessons", "courses"] {
        let Ok(rd) = std::fs::read_dir(dir.join(sub)) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| now.duration_since(t).ok()).is_some_and(|age| age > KEEP);
            if p.is_file() && old && std::fs::remove_file(&p).is_ok() {
                n += 1;
            }
        }
    }
    n
}

fn hash(s: &str) -> String {
    // FNV-1a: stable across runs and Rust versions (file names in the cache).
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn safe(id: &str) -> String {
    id.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

/// The reference pages a course's text was written from (a library course
/// records them when it is made), for finding its pictures later.
pub(crate) async fn remember_sources(s: &Shared, course: &str, urls: &[String]) {
    if let Some(d) = cache_dir(s).await {
        let f = d.join("courses").join(format!("{}.json", safe(course)));
        if std::fs::create_dir_all(f.parent().expect("has a parent")).is_ok() {
            let _ = std::fs::write(f, serde_json::to_string(urls).unwrap_or_default());
        }
    }
}

/// The paragraphs of a lesson text, as the pages split them (one per line).
pub fn paragraphs(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// The pictures for a lesson's text: chosen once (reference pages read,
/// robots.txt respected, each file's licence checked) and kept with the
/// cache, so opening the lesson again reads no page. `fallback` is the
/// lesson's own article address when the course recorded no pages.
pub(crate) async fn for_lesson(s: &Shared, course: &str, n: u32, text: &str, fallback: Option<&str>) -> Vec<LessonPicture> {
    let Some(d) = cache_dir(s).await else { return vec![] };
    let file = d.join("lessons").join(format!("{}-{n}.json", safe(course)));
    if let Some(v) = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str::<Vec<LessonPicture>>(&t).ok()) {
        return v;
    }
    let mut urls: Vec<String> = std::fs::read_to_string(d.join("courses").join(format!("{}.json", safe(course)))).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    if urls.is_empty() {
        urls.extend(fallback.filter(|u| u.starts_with("http")).map(str::to_string));
    }
    let mut cands: Vec<Candidate> = vec![];
    for u in urls.iter().take(MAX_PAGES) {
        if let Ok(html) = s.fetcher.get_text(u).await {
            for c in picture::page_images(&html, u) {
                if !cands.iter().any(|x| x.page == c.page) {
                    cands.push(c);
                }
            }
        }
    }
    let paras = paragraphs(text);
    let mut out = vec![];
    // A picture whose licence is not free (or unreadable) is passed over; the
    // paragraph then takes the next match.
    let mut skip: Vec<usize> = vec![];
    loop {
        let rest: Vec<Candidate> = cands.iter().enumerate().filter(|(k, _)| !skip.contains(k)).map(|(_, c)| c.clone()).collect();
        let idx: Vec<usize> = cands.iter().enumerate().filter(|(k, _)| !skip.contains(k)).map(|(k, _)| k).collect();
        let taken: Vec<usize> = out.iter().map(|p: &LessonPicture| p.paragraph).collect();
        let free_paras: Vec<(usize, String)> = paras.iter().cloned().enumerate().filter(|(i, _)| !taken.contains(i)).collect();
        let pick = picture::match_paragraphs(&free_paras.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>(), &rest, 1);
        let Some(&(pi, ck)) = pick.first() else { break };
        if out.len() >= MAX_PICTURES {
            break;
        }
        let k = idx[ck];
        skip.push(k);
        let c = &cands[k];
        let credit = s.fetcher.get_text(&c.page).await.ok().and_then(|h| picture::parse_commons(&h)).filter(|(_, l, _)| picture::free_licence(l));
        if let Some((author, licence, _)) = credit {
            out.push(LessonPicture { paragraph: free_paras[pi].0, url: c.url.clone(), page: c.page.clone(), author, licence, caption: c.caption.clone() });
        }
    }
    out.sort_by_key(|p| p.paragraph);
    if std::fs::create_dir_all(file.parent().expect("has a parent")).is_ok() {
        let _ = std::fs::write(&file, serde_json::to_string_pretty(&out).unwrap_or_default());
    }
    out
}

/// A picture as the lesson page uses it (through this computer's cache).
pub(crate) fn for_page(p: &LessonPicture) -> serde_json::Value {
    serde_json::json!({ "paragraph": p.paragraph, "src": format!("/api/pictures?u={}", urlencode(&p.url)), "author": p.author, "licence": p.licence, "page": p.page, "caption": p.caption })
}

fn urlencode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

#[derive(Deserialize)]
struct FileQ {
    u: String,
}

/// Only an address chosen for a lesson on this computer is fetched.
fn chosen(d: &Path, url: &str) -> bool {
    let Ok(rd) = std::fs::read_dir(d.join("lessons")) else { return false };
    rd.flatten().any(|e| std::fs::read_to_string(e.path()).ok().and_then(|t| serde_json::from_str::<Vec<LessonPicture>>(&t).ok()).is_some_and(|v| v.iter().any(|p| p.url == url)))
}

async fn file(State(s): State<Shared>, Query(q): Query<FileQ>) -> Result<Response, ApiError> {
    let not_found = || ApiError(StatusCode::NOT_FOUND, "沒有這張圖".into());
    let d = cache_dir(&s).await.ok_or_else(not_found)?;
    let f = d.join(format!("{}.jpg", hash(&q.u)));
    let bytes = match std::fs::read(&f) {
        Ok(b) => b,
        Err(_) => {
            if !chosen(&d, &q.u) {
                return Err(not_found());
            }
            let raw = s.fetcher.get_bytes(&q.u).await.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;
            // Kept small: at most 800 px wide, JPEG.
            let jpg = picture::compress(&raw).map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
            let _ = std::fs::create_dir_all(&d).and_then(|_| std::fs::write(&f, &jpg));
            prune(&d, SystemTime::now());
            jpg
        }
    };
    Ok(([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "private, max-age=86400")], bytes).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_pictures_are_deleted_after_two_weeks() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("lessons")).unwrap();
        std::fs::write(d.path().join("a.jpg"), b"x").unwrap();
        std::fs::write(d.path().join("lessons/c-1.json"), b"[]").unwrap();
        let now = SystemTime::now();
        assert_eq!(prune(d.path(), now + Duration::from_secs(13 * 24 * 3600)), 0, "kept for two weeks");
        assert_eq!(prune(d.path(), now + Duration::from_secs(15 * 24 * 3600)), 2, "then deleted");
        assert!(!d.path().join("a.jpg").exists());
    }
}
