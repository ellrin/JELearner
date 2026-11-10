//! Today's topic cards and the focus theme (docs/plan.md §19.2.6, stage F3).
//!
//! "開始今天的課": recent headlines, the learner's interests and the general
//! topics go to the model, which offers 3–5 cards; the learner picks one or
//! asks for another batch (the cards already shown are excluded). A news
//! card carries its article, so the lesson is written from it.
//!
//! A focus theme (for example 「蘇格蘭／英國旅遊」) turns every category into
//! its local version. When it is set, the model first writes a **setting
//! sheet** of real names (cities, streets, shop chains, lines, dishes,
//! landmarks…) in `profiles/<id>/focus/<lang>.toml`, which the learner may
//! edit; cards and lesson texts take their places from it, so lessons
//! connect without an invented main character.
//!
//! An upload (stage F4) also tells something about the learner: the model
//! reads it and suggests interest keywords, which the learner confirms or
//! edits before they go into `interests.csv`.

use super::prompts::{CourseTemplates, FOCUS_SYSTEM, FOCUS_USER, INTERESTS_SYSTEM, INTERESTS_USER, TOPICS_SYSTEM, TOPICS_USER};
use super::{parse_json, strict_object, CourseError};
use crate::lang::Lang;
use crate::provider::CompletionRequest;
use crate::workspace::Workspace;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

/// A recent headline offered to the model (from the articles table).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Headline {
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub source: String,
}

/// One topic card.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Card {
    /// In the native language.
    pub title: String,
    /// One sentence, native language.
    pub why: String,
    /// `news`, `interest` or `general`.
    pub kind: String,
    /// The headline this card is about (`news`).
    #[serde(default)]
    pub article_id: Option<i64>,
    /// Search words in the target language.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// The interest or general topic it comes from.
    #[serde(default)]
    pub category: String,
    /// A topic-library lesson (`kind` "library"): its lesson id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
}

pub struct CardsInput<'a> {
    pub lang: Lang,
    pub level_description: String,
    pub native_language: String,
    pub interests: &'a [String],
    pub general: &'a [String],
    /// 0–1: the share of cards from interests.
    pub interest_ratio: f32,
    pub headlines: &'a [Headline],
    /// Titles already shown today.
    pub exclude: &'a [String],
    pub focus: Option<&'a FocusSheet>,
    pub count: usize,
}

pub fn cards_schema() -> Value {
    let s = json!({ "type": "string" });
    strict_object(json!({
        "cards": { "type": "array", "items": strict_object(json!({
            "title": s, "why": s,
            "kind": { "type": "string", "enum": ["news", "interest", "general"] },
            "article_id": { "type": ["integer", "null"] },
            "keywords": { "type": "array", "items": s },
            "category": s
        })) }
    }))
}

pub fn cards_request(t: &CourseTemplates, i: &CardsInput) -> Result<CompletionRequest, CourseError> {
    let c = json!({
        "language": i.lang.english_name(),
        "level_description": i.level_description,
        "native_language": crate::lesson::native_name(&i.native_language),
        "count": i.count,
        "interest_share": (i.interest_ratio.clamp(0.0, 1.0) * 100.0).round() as u32,
        "interests": i.interests,
        "general": i.general,
        "headlines": i.headlines,
        "exclude": i.exclude,
        "focus": i.focus,
        "focus_names": i.focus.map(FocusSheet::names).unwrap_or_default(),
    });
    Ok(CompletionRequest { system: t.render(TOPICS_SYSTEM, c.clone())?, user: t.render(TOPICS_USER, c)?, max_tokens: 3000, json_schema: Some(cards_schema()) })
}

/// The cards, checked: a news card must name a headline that was offered
/// (otherwise it becomes a plain topic), no empty or repeated titles, none
/// already shown, at most `max`.
pub fn parse_cards(text: &str, headlines: &[Headline], exclude: &[String], max: usize) -> Result<Vec<Card>, CourseError> {
    #[derive(Deserialize)]
    struct Reply {
        cards: Vec<Card>,
    }
    let r: Reply = parse_json(text)?;
    let norm = |s: &str| s.trim().to_lowercase();
    let mut out: Vec<Card> = vec![];
    for mut c in r.cards {
        c.title = c.title.trim().to_string();
        if c.title.is_empty() || exclude.iter().any(|e| norm(e) == norm(&c.title)) || out.iter().any(|o| norm(&o.title) == norm(&c.title)) {
            continue;
        }
        if c.article_id.is_some_and(|id| !headlines.iter().any(|h| h.id == id)) {
            c.article_id = None;
        }
        if c.article_id.is_none() && c.kind == "news" {
            c.kind = "general".into();
        }
        c.keywords.retain(|k| !k.trim().is_empty());
        c.keywords.truncate(3);
        out.push(c);
        if out.len() == max {
            break;
        }
    }
    if out.is_empty() {
        return Err(CourseError::Invalid("沒有新的主題，請再換一批".into()));
    }
    Ok(out)
}

// ── focus theme ─────────────────────────────────────────

/// Real names for one place, reused by every card and lesson.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct FocusSheet {
    /// What the learner typed (the model's reply has no name; it is set after).
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub cities: Vec<String>,
    #[serde(default)]
    pub streets: Vec<String>,
    #[serde(default)]
    pub shops: Vec<String>,
    #[serde(default)]
    pub transport: Vec<String>,
    #[serde(default)]
    pub foods: Vec<String>,
    #[serde(default)]
    pub drinks: Vec<String>,
    #[serde(default)]
    pub landmarks: Vec<String>,
    #[serde(default)]
    pub daily_life: Vec<String>,
    #[serde(default)]
    pub news_keywords: Vec<String>,
}

impl FocusSheet {
    /// All the names, for prompts: "Edinburgh, Glasgow, Princes Street, Tesco…".
    pub fn names(&self) -> String {
        [&self.cities, &self.streets, &self.shops, &self.transport, &self.foods, &self.drinks, &self.landmarks]
            .iter()
            .flat_map(|v| v.iter())
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub fn focus_schema() -> Value {
    let list = json!({ "type": "array", "items": { "type": "string" } });
    strict_object(json!({
        "summary": { "type": "string" },
        "cities": list, "streets": list, "shops": list, "transport": list, "foods": list,
        "drinks": list, "landmarks": list, "daily_life": list, "news_keywords": list
    }))
}

pub fn focus_request(t: &CourseTemplates, lang: Lang, native_language: &str, name: &str) -> Result<CompletionRequest, CourseError> {
    let c = json!({ "language": lang.english_name(), "native_language": crate::lesson::native_name(native_language), "name": name });
    Ok(CompletionRequest { system: t.render(FOCUS_SYSTEM, c.clone())?, user: t.render(FOCUS_USER, c)?, max_tokens: 3000, json_schema: Some(focus_schema()) })
}

pub fn parse_focus(text: &str, name: &str) -> Result<FocusSheet, CourseError> {
    let mut f: FocusSheet = parse_json(text)?;
    f.name = name.trim().to_string();
    if f.names().is_empty() {
        return Err(CourseError::Invalid("設定集是空的，請換個說法再試一次".into()));
    }
    Ok(f)
}

/// `profiles/<id>/focus/<lang>.toml`: one focus per study language.
pub fn focus_path(ws: &Workspace, lang: Lang) -> PathBuf {
    ws.profile_dir().join("focus").join(format!("{}.toml", lang.code()))
}

pub fn load_focus(ws: &Workspace, lang: Lang) -> Option<FocusSheet> {
    let text = std::fs::read_to_string(focus_path(ws, lang)).ok()?;
    toml::from_str(&text).ok().filter(|f: &FocusSheet| !f.name.trim().is_empty())
}

pub fn save_focus(ws: &Workspace, lang: Lang, f: &FocusSheet) -> Result<(), CourseError> {
    let path = focus_path(ws, lang);
    std::fs::create_dir_all(path.parent().expect("has a parent"))?;
    let body = toml::to_string(f).map_err(|e| CourseError::Invalid(e.to_string()))?;
    let head = "# 重點主題的在地設定集（docs/plan.md §19.2.6）。可以自己改：增刪地名、店名、路線，\n# 之後的主題卡與課文都從這裡取材。刪掉這個檔案＝取消重點主題。\n\n";
    std::fs::write(&path, format!("{head}{body}"))?;
    Ok(())
}

pub fn remove_focus(ws: &Workspace, lang: Lang) -> Result<bool, CourseError> {
    let path = focus_path(ws, lang);
    if path.exists() {
        std::fs::remove_file(path)?;
        return Ok(true);
    }
    Ok(false)
}

// ── interests from an upload (stage F4, plan §19.2.7) ──

/// What an upload is about and the interest keywords it suggests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct InterestGuess {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub keywords: Vec<String>,
}

/// The upload text sent to the model (the start is enough to tell the topic).
pub const SUGGEST_MAX_CHARS: usize = 6000;

pub fn suggest_schema() -> Value {
    strict_object(json!({
        "summary": { "type": "string" },
        "keywords": { "type": "array", "items": { "type": "string" } }
    }))
}

pub fn suggest_request(t: &CourseTemplates, lang: Lang, native_language: &str, existing: &[String], file_name: &str, text: &str) -> Result<CompletionRequest, CourseError> {
    let text: String = text.chars().take(SUGGEST_MAX_CHARS).collect();
    let c = json!({ "language": lang.english_name(), "native_language": crate::lesson::native_name(native_language), "existing": existing, "file_name": file_name, "text": text });
    Ok(CompletionRequest { system: t.render(INTERESTS_SYSTEM, c.clone())?, user: t.render(INTERESTS_USER, c)?, max_tokens: 600, json_schema: Some(suggest_schema()) })
}

/// At most 4 keywords, trimmed, none empty, too long, repeated or already known.
pub fn parse_suggest(text: &str, existing: &[String]) -> Result<InterestGuess, CourseError> {
    let mut g: InterestGuess = parse_json(text)?;
    let norm = |s: &str| s.trim().to_lowercase();
    let mut out: Vec<String> = vec![];
    for k in g.keywords.drain(..) {
        let k = k.trim().trim_matches(|c| c == '"' || c == '#').trim().to_string();
        if k.is_empty() || k.chars().count() > 30 || k.contains(',') || existing.iter().any(|e| norm(e) == norm(&k)) || out.iter().any(|o| norm(o) == norm(&k)) {
            continue;
        }
        out.push(k);
        if out.len() == 4 {
            break;
        }
    }
    g.keywords = out;
    g.summary = g.summary.trim().chars().take(40).collect();
    Ok(g)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heads() -> Vec<Headline> {
        vec![Headline { id: 7, title: "ScotRail fares rise".into(), summary: String::new(), source: "BBC".into() }]
    }

    #[test]
    fn cards_are_checked() {
        let reply = json!({ "cards": [
            { "title": "蘇格蘭鐵路漲價", "why": "通勤會用到", "kind": "news", "article_id": 7, "keywords": ["ScotRail", "fares"], "category": "交通" },
            { "title": "不存在的新聞", "why": "x", "kind": "news", "article_id": 99, "keywords": [], "category": "新聞" },
            { "title": "蘇格蘭鐵路漲價", "why": "重複", "kind": "general", "article_id": null, "keywords": [], "category": "交通" },
            { "title": "昨天看過的", "why": "x", "kind": "general", "article_id": null, "keywords": [], "category": "飲食" },
            { "title": "  ", "why": "x", "kind": "general", "article_id": null, "keywords": [], "category": "" },
            { "title": "哈吉斯與早餐", "why": "道地早餐", "kind": "general", "article_id": null, "keywords": ["haggis", "a", "b", "c"], "category": "飲食" }
        ]}).to_string();
        let cards = parse_cards(&reply, &heads(), &["昨天看過的".into()], 5).unwrap();
        let titles: Vec<&str> = cards.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, vec!["蘇格蘭鐵路漲價", "不存在的新聞", "哈吉斯與早餐"]);
        assert_eq!(cards[0].article_id, Some(7));
        assert_eq!((cards[1].article_id, cards[1].kind.as_str()), (None, "general"), "an unknown headline is dropped");
        assert_eq!(cards[2].keywords.len(), 3);
        assert_eq!(parse_cards(&reply, &heads(), &[], 2).unwrap().len(), 2);
        let none = json!({ "cards": [{ "title": "昨天看過的", "why": "", "kind": "general", "article_id": null, "keywords": [], "category": "" }] }).to_string();
        assert!(parse_cards(&none, &[], &["昨天看過的".into()], 5).is_err());
    }

    #[test]
    fn prompts_carry_the_focus() {
        let t = CourseTemplates::bundled();
        let focus = FocusSheet { name: "蘇格蘭／英國旅遊".into(), cities: vec!["Edinburgh".into()], shops: vec!["Tesco".into()], ..Default::default() };
        let i = CardsInput {
            lang: Lang::En,
            level_description: "B2".into(),
            native_language: "zh-TW".into(),
            interests: &["AI".into()],
            general: &["交通".into(), "飲食".into()],
            interest_ratio: 0.6,
            headlines: &heads(),
            exclude: &["昨天看過的".into()],
            focus: Some(&focus),
            count: 4,
        };
        let r = cards_request(&t, &i).unwrap();
        assert!(r.system.contains("exactly 4") && r.system.contains("60 %"));
        assert!(r.system.contains("蘇格蘭／英國旅遊") && r.system.contains("Edinburgh, Tesco"), "{}", r.system);
        assert!(r.user.contains("[7] ScotRail fares rise") && r.user.contains("昨天看過的"));
        let plain = cards_request(&t, &CardsInput { focus: None, ..i }).unwrap();
        assert!(!plain.system.contains("Focus theme"));
        let f = focus_request(&t, Lang::En, "zh-TW", "蘇格蘭／英國旅遊").unwrap();
        assert!(f.system.contains("never invent") && f.user.contains("蘇格蘭／英國旅遊"));
    }

    #[test]
    fn focus_sheet_round_trip() {
        let reply = json!({ "summary": "蘇格蘭", "cities": ["Edinburgh", "Glasgow"], "streets": ["Princes Street"], "shops": ["Tesco"], "transport": ["ScotRail"],
            "foods": ["haggis"], "drinks": ["whisky"], "landmarks": ["Edinburgh Castle"], "daily_life": ["pounds"], "news_keywords": ["Holyrood"] }).to_string();
        let f = parse_focus(&reply, " 蘇格蘭／英國旅遊 ").unwrap();
        assert_eq!(f.name, "蘇格蘭／英國旅遊");
        assert!(f.names().starts_with("Edinburgh, Glasgow, Princes Street, Tesco"));
        let empty = json!({ "summary": "", "cities": [], "streets": [], "shops": [], "transport": [], "foods": [], "drinks": [], "landmarks": [], "daily_life": [], "news_keywords": [] }).to_string();
        assert!(parse_focus(&empty, "x").is_err());
        // The file the learner may edit reads back.
        let text = toml::to_string(&f).unwrap();
        assert_eq!(toml::from_str::<FocusSheet>(&text).unwrap(), f);
    }

    #[test]
    fn interest_suggestions_are_cleaned() {
        let existing = vec!["AI 晶片".to_string()];
        let t = CourseTemplates::bundled();
        let req = suggest_request(&t, Lang::En, "zh-TW", &existing, "chips.md", &"x".repeat(9000)).unwrap();
        assert!(req.system.contains("AI 晶片"), "known interests are left out by the prompt too");
        assert!(req.user.contains("chips.md") && req.user.len() < 6200);
        let reply = json!({ "summary": "一篇關於晶片工廠的新聞", "keywords": [" 半導體 ", "ai 晶片", "半導體", "", "a,b", "#供應鏈", "電力", "工廠", "太多了"] }).to_string();
        let g = parse_suggest(&reply, &existing).unwrap();
        assert_eq!(g.keywords, vec!["半導體", "供應鏈", "電力", "工廠"]);
        assert_eq!(g.summary, "一篇關於晶片工廠的新聞");
        let none = parse_suggest(&json!({ "summary": "TOEIC 單字", "keywords": [] }).to_string(), &[]).unwrap();
        assert!(none.keywords.is_empty());
    }
}
