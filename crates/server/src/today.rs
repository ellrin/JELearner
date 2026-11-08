//! 開始今天的課 (docs/plan.md §19.2.6, stage F3): one way in.
//!
//! - `POST /api/today/topics {lang, exclude, refresh}`: fresh headlines, the
//!   learner's interests and the general topics → 3–5 topic cards from the
//!   model; `exclude` = titles already shown ("換一批").
//! - `POST /api/today/start {lang, card}`: today's lesson on that card (a
//!   news card brings its article); only today's lesson is written, when the
//!   learner opens it.
//! - `GET / POST / DELETE /api/focus`: the focus theme and its setting
//!   sheet (`profiles/<id>/focus/<lang>.toml`).

use crate::api::{need_lang, ws};
use crate::courses::{course_err, daily_course, date, general_topics, templates};
use crate::{bad, ApiError, Shared};
use axum::extract::{Query, State};
use axum::routing::post;
use axum::{Json, Router};
use jl_core::course::picker::{self, Card, CardsInput, FocusSheet, Headline};
use jl_core::course::topics as ctopics;
use jl_core::course::{store, CourseKind, SourceText};
use jl_core::Lang;
use serde::Deserialize;
use serde_json::{json, Value};

type R<T> = Result<Json<T>, ApiError>;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/today/topics", post(topics))
        .route("/api/today/start", post(start))
        .route("/api/focus", axum::routing::get(get_focus).post(set_focus).delete(clear_focus))
}

#[derive(Deserialize)]
struct TopicsBody {
    lang: Option<String>,
    #[serde(default)]
    exclude: Vec<String>,
    /// Fetch the feeds first (the first batch of the day).
    #[serde(default)]
    refresh: bool,
}

/// Recent headlines for the cards; with a focus, the ones about it first.
pub(crate) async fn headlines(s: &Shared, lang: Lang, focus: Option<&FocusSheet>) -> Result<Vec<Headline>, ApiError> {
    let l = ws(s).await?;
    let mut out: Vec<Headline> = vec![];
    if let Some(f) = focus {
        for k in f.news_keywords.iter().chain(f.cities.iter()).take(6) {
            let rows: Vec<(i64, String, String, String)> = sqlx::query_as(
                "SELECT id, title, summary, source FROM articles WHERE lang = ?1 AND (title LIKE ?2 OR summary LIKE ?2)
                 ORDER BY COALESCE(published_at, fetched_at) DESC LIMIT 4",
            )
            .bind(lang.code())
            .bind(format!("%{}%", k.trim()))
            .fetch_all(l.db.pool())
            .await?;
            for (id, title, summary, source) in rows {
                if !out.iter().any(|h| h.id == id) {
                    out.push(Headline { id, title, summary, source });
                }
            }
        }
    }
    for a in l.db.list_articles(Some(lang), 40).await? {
        if out.len() >= 20 {
            break;
        }
        if !out.iter().any(|h| h.id == a.id) {
            out.push(Headline { id: a.id, title: a.title, summary: a.summary.chars().take(160).collect(), source: a.source });
        }
    }
    Ok(out)
}

async fn topics(State(s): State<Shared>, Json(b): Json<TopicsBody>) -> R<Value> {
    let lang = need_lang(&b.lang)?;
    // The feeds may be down or offline: the cards still come from interests and general topics.
    let news_errors = if b.refresh { crate::api::refresh_news(&s, Some(lang)).await.map(|(_, e)| e).unwrap_or_else(|e| vec![e.1]) } else { vec![] };
    let (cards, heads, focus) = {
        let l = ws(&s).await?;
        let (parts, focus, t) = card_inputs(&l, lang);
        drop(l);
        let heads = headlines(&s, lang, focus.as_ref()).await?;
        let cards = ask_cards(&s, lang, parts, focus.as_ref(), &t, &heads, &b.exclude, 4, 5).await?;
        (cards, heads, focus)
    };
    // A news card shows where its article is from.
    let cards: Vec<Value> = cards
        .into_iter()
        .map(|c| {
            let mut v = json!(c);
            if let Some(h) = c.article_id.and_then(|id| heads.iter().find(|h| h.id == id)) {
                v["article"] = json!({ "title": h.title, "source": h.source });
            }
            v
        })
        .collect();
    Ok(Json(json!({ "cards": cards, "focus": focus.map(|f| f.name), "news_errors": news_errors })))
}

type CardParts = (Vec<String>, Vec<String>, String, String, f32);

/// What the cards are made from, for one learner (the active user, or a
/// family member when the admin prepares lessons for them).
pub(crate) fn card_inputs(l: &crate::Loaded, lang: Lang) -> (CardParts, Option<FocusSheet>, jl_core::course::prompts::CourseTemplates) {
    let interests: Vec<String> = ctopics::parse_interests(&std::fs::read_to_string(l.ws.interests_file()).unwrap_or_default())
        .into_iter()
        .filter(|i| i.lang.is_none_or(|x| x == lang))
        .map(|i| i.keyword)
        .collect();
    let general: Vec<String> = general_topics(&l.ws).into_iter().map(|g| g.name).collect();
    let level = jl_core::level::from_score(lang, l.ws.profile.lang(lang).difficulty);
    let parts = (interests, general, jl_core::level::describe(lang, &level), l.ws.profile.native_language.clone(), l.ws.profile.conversation.interest_ratio);
    (parts, picker::load_focus(&l.ws, lang), templates(&l.ws))
}

/// Ask the model for `count` cards (at most `max` kept).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn ask_cards(
    s: &Shared,
    lang: Lang,
    parts: CardParts,
    focus: Option<&FocusSheet>,
    t: &jl_core::course::prompts::CourseTemplates,
    heads: &[Headline],
    exclude: &[String],
    count: usize,
    max: usize,
) -> Result<Vec<Card>, ApiError> {
    let (interests, general, level_description, native_language, interest_ratio) = parts;
    let req = picker::cards_request(
        t,
        &CardsInput { lang, level_description, native_language, interests: &interests, general: &general, interest_ratio, headlines: heads, exclude, focus, count },
    )
    .map_err(course_err)?;
    let provider = crate::provider(s).await?;
    let reply = provider.complete(&req).await.map_err(crate::study::api_err)?;
    picker::parse_cards(&reply, heads, exclude, max).map_err(course_err)
}

#[derive(Deserialize)]
struct StartBody {
    lang: Option<String>,
    card: Card,
}

async fn start(State(s): State<Shared>, Json(b): Json<StartBody>) -> R<Value> {
    let lang = need_lang(&b.lang)?;
    if b.card.title.trim().is_empty() {
        return Err(bad("沒有選主題"));
    }
    {
        let l = ws(&s).await?;
        let (all, _) = crate::courses::load_all(&l.ws, Some(lang));
        if let Some(c) = all.iter().find(|c| c.kind == CourseKind::Daily && c.created == date()) {
            return Ok(Json(json!({ "id": c.id, "next": c.next_lesson().map(|x| x.number), "created": false })));
        }
    }
    // A news card: its article, fetched now if only the headline is stored
    // (robots.txt is respected; a refusal just means a written text instead).
    let mut article: Option<SourceText> = None;
    if let Some(id) = b.card.article_id {
        let a = ws(&s).await?.db.get_article(id).await?;
        if let Some(a) = a {
            let body = if a.body.trim().is_empty() && a.url.starts_with("http") {
                match crate::api::fetch_article_into_db(&s, &a.url, lang, &a.source).await {
                    Ok(_) => ws(&s).await?.db.get_article(id).await?.map(|x| x.body).unwrap_or_default(),
                    Err(_) => String::new(),
                }
            } else {
                a.body.clone()
            };
            if !body.trim().is_empty() {
                article = Some(SourceText { title: a.title, url: a.url, source: a.source, text: body.chars().take(jl_core::lesson::MAX_ARTICLE_CHARS).collect() });
            }
        }
    }
    let l = ws(&s).await?;
    let focus = picker::load_focus(&l.ws, lang).map(|f| f.name).unwrap_or_default();
    let mut course = daily_course(&l, lang, b.card.title.trim(), b.card.keywords.clone(), article, focus, &date()).await?;
    store::save_new(&l.ws, &mut course).map_err(course_err)?;
    Ok(Json(json!({ "id": course.id, "next": 1, "created": true })))
}

// ── focus theme ─────────────────────────────────────────

#[derive(Deserialize)]
struct LangQ {
    lang: Option<String>,
}

async fn get_focus(State(s): State<Shared>, Query(q): Query<LangQ>) -> R<Value> {
    let lang = need_lang(&q.lang)?;
    let l = ws(&s).await?;
    Ok(Json(json!({ "focus": picker::load_focus(&l.ws, lang), "file": l.ws.relative(&picker::focus_path(&l.ws, lang)) })))
}

#[derive(Deserialize)]
struct FocusBody {
    lang: Option<String>,
    name: String,
}

/// Set a focus theme: the model writes its setting sheet once (the learner
/// may edit the file afterwards).
async fn set_focus(State(s): State<Shared>, Json(b): Json<FocusBody>) -> R<Value> {
    let lang = need_lang(&b.lang)?;
    let name = b.name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err(bad("重點主題寫 1 到 40 個字，例如「蘇格蘭／英國旅遊」"));
    }
    let req = {
        let l = ws(&s).await?;
        picker::focus_request(&templates(&l.ws), lang, &l.ws.profile.native_language, name).map_err(course_err)?
    };
    let provider = crate::provider(&s).await?;
    let reply = provider.complete(&req).await.map_err(crate::study::api_err)?;
    let sheet = picker::parse_focus(&reply, name).map_err(course_err)?;
    let l = ws(&s).await?;
    picker::save_focus(&l.ws, lang, &sheet).map_err(course_err)?;
    Ok(Json(json!({ "focus": sheet, "file": l.ws.relative(&picker::focus_path(&l.ws, lang)) })))
}

async fn clear_focus(State(s): State<Shared>, Query(q): Query<LangQ>) -> R<Value> {
    let lang = need_lang(&q.lang)?;
    let l = ws(&s).await?;
    let removed = picker::remove_focus(&l.ws, lang).map_err(course_err)?;
    Ok(Json(json!({ "removed": removed })))
}
