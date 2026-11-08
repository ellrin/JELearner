//! A topic library: reading articles prepared ahead (a "pre-RAG").
//!
//! A collection (e.g. `library/uk/`) has a syllabus written by the author
//! (`syllabus.toml`: topics, each with reference pages and news keywords).
//! For each topic the model reads the references and writes one article of
//! 400–600 words at one level (B2), from its history to today, with the
//! sources it used. Articles are Markdown files in the workspace
//! (`library/<collection>/<category>/<id>.md`); lessons are made from them
//! later, at each learner's level, so the references are read only once.
//!
//! References: `w:<title>` = English Wikipedia, `v:<title>` = English
//! Wikivoyage (both CC BY-SA, their ordinary article pages), or a full URL.

use super::prompts::{CourseTemplates, LIBRARY_SYSTEM, LIBRARY_USER};
use super::{parse_json, strict_object, CourseError};
use crate::lang::Lang;
use crate::provider::CompletionRequest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Syllabus {
    pub name: String,
    pub lang: Lang,
    #[serde(default = "b2")]
    pub level: String,
    #[serde(default = "words")]
    pub words: [u32; 2],
    #[serde(default)]
    pub topic: Vec<Topic>,
}
fn b2() -> String {
    "B2".into()
}
fn words() -> [u32; 2] {
    [400, 600]
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Topic {
    pub id: String,
    pub cat: String,
    pub zh: String,
    pub en: String,
    #[serde(default)]
    pub refs: Vec<String>,
    #[serde(default)]
    pub news: Vec<String>,
}

/// A reference page to read for a topic.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RefPage {
    pub url: String,
    /// CSS selector of the article body on that site.
    pub selector: Option<&'static str>,
    pub licence: &'static str,
}

/// `w:Title` / `v:Title` / a URL → the page to fetch.
pub fn ref_page(r: &str) -> Option<RefPage> {
    let r = r.trim();
    let wiki = |host: &str, title: &str| format!("https://{host}/wiki/{}", urlencode(&title.trim().replace(' ', "_")));
    if let Some(t) = r.strip_prefix("w:") {
        return Some(RefPage { url: wiki("en.wikipedia.org", t), selector: Some("#mw-content-text .mw-parser-output"), licence: "CC BY-SA 4.0 (Wikipedia)" });
    }
    if let Some(t) = r.strip_prefix("v:") {
        return Some(RefPage { url: wiki("en.wikivoyage.org", t), selector: Some("#mw-content-text .mw-parser-output"), licence: "CC BY-SA 4.0 (Wikivoyage)" });
    }
    (r.starts_with("https://") || r.starts_with("http://")).then(|| RefPage { url: r.to_string(), selector: None, licence: "see source" })
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~(),'!*:".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The library folder of a workspace and its collections (folders with a syllabus).
pub fn library_dir(root: &Path) -> PathBuf {
    root.join("library")
}

pub fn collections(root: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(library_dir(root))
        .map(|d| d.flatten().filter(|e| e.path().join("syllabus.toml").is_file()).map(|e| e.file_name().to_string_lossy().to_string()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

pub fn load_syllabus(root: &Path, collection: &str) -> Result<Syllabus, CourseError> {
    if collection.is_empty() || collection.contains(['/', '\\', '.']) {
        return Err(CourseError::Invalid("主題庫名稱不對".into()));
    }
    let p = library_dir(root).join(collection).join("syllabus.toml");
    let text = std::fs::read_to_string(&p).map_err(|_| CourseError::NotFound(format!("{}", p.display())))?;
    let s: Syllabus = toml::from_str(&text).map_err(|e| CourseError::Invalid(format!("syllabus.toml：{e}")))?;
    Ok(s)
}

/// `library/<collection>/<category>/<id>.md`.
pub fn article_path(root: &Path, collection: &str, t: &Topic) -> PathBuf {
    library_dir(root).join(collection).join(&t.cat).join(format!("{}.md", t.id))
}

/// A reference as read: what goes to the model and into the article's sources.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceDoc {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub licence: String,
    #[serde(skip_serializing, default)]
    pub text: String,
}

/// Wikipedia's citation marks (`[12]`, `[a]`, `[citation needed]`) out of a paragraph.
pub fn clean_reference_text(paragraphs: &[String], max_chars: usize) -> String {
    let mut out = String::new();
    for p in paragraphs {
        let mut s = String::with_capacity(p.len());
        let mut depth = 0;
        let mut buf = String::new();
        for c in p.chars() {
            match c {
                '[' => {
                    depth += 1;
                    buf.clear();
                }
                ']' if depth > 0 => {
                    depth -= 1;
                    let inner = buf.trim().to_lowercase();
                    let mark = inner.len() <= 3 || inner.contains("citation") || inner.contains("note") || inner.chars().all(|c| c.is_ascii_digit());
                    if !mark {
                        s.push('[');
                        s.push_str(&buf);
                        s.push(']');
                    }
                    buf.clear();
                }
                _ if depth > 0 => buf.push(c),
                _ => s.push(c),
            }
        }
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if s.is_empty() {
            continue;
        }
        if out.chars().count() + s.chars().count() > max_chars {
            break;
        }
        out.push_str(&s);
        out.push_str("\n\n");
    }
    out.trim_end().to_string()
}

pub fn article_schema() -> Value {
    let s = json!({ "type": "string" });
    strict_object(json!({
        "title": s,
        "text": s,
        "key_terms": { "type": "array", "items": strict_object(json!({ "term": s, "meaning": s })) },
        "used_sources": { "type": "array", "items": { "type": "integer" } },
        "summary": s,
    }))
}

pub fn article_request(t: &CourseTemplates, syl: &Syllabus, topic: &Topic, native_language: &str, sources: &[SourceDoc]) -> Result<CompletionRequest, CourseError> {
    let c = json!({
        "collection": syl.name,
        "language": syl.lang.english_name(),
        "native_language": crate::lesson::native_name(native_language),
        "level": syl.level,
        "min_words": syl.words[0],
        "max_words": syl.words[1],
        "topic_en": topic.en,
        "topic_zh": topic.zh,
        "category": topic.cat,
        "sources": sources.iter().map(|s| json!({ "title": s.title, "url": s.url, "text": s.text })).collect::<Vec<_>>(),
    });
    Ok(CompletionRequest { system: t.render(LIBRARY_SYSTEM, c.clone())?, user: t.render(LIBRARY_USER, c)?, max_tokens: 4000, json_schema: Some(article_schema()) })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyTerm {
    pub term: String,
    pub meaning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArticleOut {
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub key_terms: Vec<KeyTerm>,
    #[serde(default)]
    pub used_sources: Vec<usize>,
    #[serde(default)]
    pub summary: String,
}

pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// The model's article, checked: a text of about the right length, at least
/// one real source used (an article without a source is not kept).
pub fn parse_article(text: &str, syl: &Syllabus, sources: &[SourceDoc]) -> Result<ArticleOut, CourseError> {
    let mut a: ArticleOut = parse_json(text)?;
    a.used_sources.retain(|i| *i >= 1 && *i <= sources.len());
    a.used_sources.sort();
    a.used_sources.dedup();
    if a.used_sources.is_empty() {
        return Err(CourseError::Invalid("文章沒有標出用了哪個參考資料，不收".into()));
    }
    let n = word_count(&a.text);
    // A little slack around the asked length.
    let (lo, hi) = (syl.words[0] * 3 / 4, syl.words[1] * 5 / 4);
    if (n as u32) < lo || (n as u32) > hi {
        return Err(CourseError::Invalid(format!("文章 {n} 字，不在 {lo}–{hi} 字之間")));
    }
    a.title = a.title.trim().to_string();
    if a.title.is_empty() {
        a.title = String::from("Untitled");
    }
    Ok(a)
}

/// The article file: `key: <json>` lines (like lesson files), the text,
/// key terms and references.
pub fn render_article(syl: &Syllabus, collection: &str, topic: &Topic, a: &ArticleOut, sources: &[SourceDoc], engine: &str, date: &str) -> String {
    let used: Vec<&SourceDoc> = a.used_sources.iter().filter_map(|i| sources.get(i - 1)).collect();
    let mut s = String::from("---\n");
    let line = |k: &str, v: Value| format!("{k}: {v}\n");
    s += &line("id", json!(topic.id));
    s += &line("collection", json!(collection));
    s += &line("title", json!(a.title));
    s += &line("topic", json!(topic.en));
    s += &line("topic_zh", json!(topic.zh));
    s += &line("category", json!(topic.cat));
    s += &line("lang", json!(syl.lang.code()));
    s += &line("level", json!(syl.level));
    s += &line("words", json!(word_count(&a.text)));
    s += &line("news", json!(topic.news));
    s += &line("summary", json!(a.summary));
    s += &line("key_terms", json!(a.key_terms));
    s += &line("sources", json!(used));
    s += &line("engine", json!(engine));
    s += &line("created", json!(date));
    s += "---\n\n";
    s += &format!("# {}\n\n{}\n\n## Key terms\n\n", a.title, a.text.trim());
    for k in &a.key_terms {
        s += &format!("- **{}** — {}\n", k.term, k.meaning);
    }
    s += "\n## References\n\n";
    for (i, src) in used.iter().enumerate() {
        s += &format!("{}. {} — <{}> ({})\n", i + 1, src.title, src.url, src.licence);
    }
    s
}

/// An article file read back: its front matter values and the text alone.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Article {
    pub id: String,
    pub title: String,
    pub category: String,
    pub topic_zh: String,
    pub level: String,
    pub words: usize,
    pub text: String,
    pub news: Vec<String>,
    pub sources: Vec<SourceDoc>,
    pub key_terms: Vec<KeyTerm>,
}

pub fn read_article(md: &str) -> Option<Article> {
    let fm = |k: &str| -> Option<Value> {
        let rest = md.strip_prefix("---\n")?;
        let head = &rest[..rest.find("\n---")?];
        head.lines().find_map(|l| l.strip_prefix(&format!("{k}: ")).and_then(|v| serde_json::from_str(v).ok()))
    };
    let s = |k: &str| fm(k).and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    let body_start = md.find("\n---\n").map(|i| i + 5)?;
    let body = &md[body_start..];
    let after_title = body.trim_start().split_once('\n').map(|(_, r)| r).unwrap_or("");
    let text = after_title.split("\n## Key terms").next().unwrap_or("").trim().to_string();
    Some(Article {
        id: s("id"),
        title: s("title"),
        category: s("category"),
        topic_zh: s("topic_zh"),
        level: s("level"),
        words: fm("words").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
        news: fm("news").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
        sources: fm("sources").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
        key_terms: fm("key_terms").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn syl() -> Syllabus {
        toml::from_str(
            r#"
name = "蘇格蘭與英國"
lang = "en"
topic = [
  { id = "haggis", cat = "飲食", zh = "哈吉斯", en = "Haggis", refs = ["w:Haggis", "v:Edinburgh", "https://example.org/x"], news = ["haggis"] },
]
"#,
        )
        .unwrap()
    }

    #[test]
    fn syllabus_and_references() {
        let s = syl();
        assert_eq!((s.level.as_str(), s.words), ("B2", [400, 600]));
        let t = &s.topic[0];
        let r: Vec<RefPage> = t.refs.iter().filter_map(|r| ref_page(r)).collect();
        assert_eq!(r[0].url, "https://en.wikipedia.org/wiki/Haggis");
        assert_eq!(r[1].url, "https://en.wikivoyage.org/wiki/Edinburgh");
        assert_eq!(ref_page("w:King's Cross station").unwrap().url, "https://en.wikipedia.org/wiki/King's_Cross_station");
        assert_eq!(ref_page("w:Platform 9¾").unwrap().url, "https://en.wikipedia.org/wiki/Platform_9%C2%BE");
        assert_eq!(r[2].selector, None);
        assert!(ref_page("nothing").is_none());
    }

    #[test]
    fn citation_marks_are_removed() {
        let p = vec!["Haggis is a savoury pudding.[1][2] It is eaten on Burns Night.[citation needed] Rules [b] apply.".to_string(), "  ".into(), "Second [note 3] paragraph about the [Scottish Parliament] debate.".into()];
        let t = clean_reference_text(&p, 10_000);
        assert_eq!(t, "Haggis is a savoury pudding. It is eaten on Burns Night. Rules apply.\n\nSecond paragraph about the [Scottish Parliament] debate.");
        assert_eq!(clean_reference_text(&p, 20), "", "the limit stops before a paragraph that does not fit");
    }

    #[test]
    fn articles_need_a_source_and_the_right_length_and_round_trip() {
        let s = syl();
        let src = vec![SourceDoc { title: "Haggis".into(), url: "https://en.wikipedia.org/wiki/Haggis".into(), licence: "CC BY-SA 4.0 (Wikipedia)".into(), text: "x".into() }];
        let text = std::iter::repeat_n("word", 480).collect::<Vec<_>>().join(" ");
        let ok = json!({ "title": "A Pudding", "text": text, "key_terms": [{ "term": "savoury", "meaning": "鹹的" }], "used_sources": [1, 1, 7], "summary": "哈吉斯的歷史" }).to_string();
        let a = parse_article(&ok, &s, &src).unwrap();
        assert_eq!(a.used_sources, vec![1]);
        let none = json!({ "title": "x", "text": text, "key_terms": [], "used_sources": [5], "summary": "" }).to_string();
        assert!(parse_article(&none, &s, &src).is_err(), "no real source: not kept");
        let short = json!({ "title": "x", "text": "too short", "key_terms": [], "used_sources": [1], "summary": "" }).to_string();
        assert!(parse_article(&short, &s, &src).is_err());

        let md = render_article(&s, "uk", &s.topic[0], &a, &src, "claude:sonnet", "2026-11-08");
        assert!(md.contains("## References") && md.contains("CC BY-SA"));
        let back = read_article(&md).unwrap();
        assert_eq!((back.id.as_str(), back.title.as_str(), back.category.as_str(), back.words), ("haggis", "A Pudding", "飲食", 480));
        assert_eq!(back.text, text);
        assert_eq!(back.sources[0].url, "https://en.wikipedia.org/wiki/Haggis");
        assert_eq!(back.key_terms[0].meaning, "鹹的");
        assert_eq!(back.news, vec!["haggis".to_string()]);
    }

    #[test]
    fn request_carries_the_sources_and_rules() {
        let s = syl();
        let t = CourseTemplates::bundled();
        let src = vec![SourceDoc { title: "Haggis".into(), url: "https://en.wikipedia.org/wiki/Haggis".into(), licence: String::new(), text: "Haggis is a savoury pudding.".into() }];
        let req = article_request(&t, &s, &s.topic[0], "zh-TW", &src).unwrap();
        assert!(req.system.contains("B2") && req.system.contains("400–600") && req.system.contains("Do not invent"));
        assert!(req.user.contains("[1] Haggis (https://en.wikipedia.org/wiki/Haggis)") && req.user.contains("savoury pudding"));
    }
}
