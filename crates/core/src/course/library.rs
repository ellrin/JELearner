//! A topic library: reading texts prepared ahead (a "pre-RAG").
//!
//! A collection (e.g. `library/uk/`) has a syllabus written by the author
//! (`syllabus.toml`): themes, each with reference pages and news keywords.
//! The point is learning the language, one thing per lesson:
//!
//! 1. **Split**: the model reads a theme's references and plans 3–6 lessons,
//!    each about one thing, with an English goal (a situation or a group of
//!    words) and the vocabulary to use (`plans/<theme>.toml`, editable).
//! 2. **Write**: for each lesson, a 300–450-word text at one level (B2) that
//!    stays on that one thing and uses the vocabulary, with the sources used
//!    (`<category>/<lesson id>.md`).
//! 3. Lessons are made from a text later, at each learner's level; the
//!    references are read only once.
//!
//! References: `w:<title>` = English Wikipedia, `v:<title>` = English
//! Wikivoyage (both CC BY-SA, their ordinary article pages), or a full URL.

use super::prompts::{CourseTemplates, LIBRARY_SPLIT_SYSTEM, LIBRARY_SPLIT_USER, LIBRARY_SYSTEM, LIBRARY_USER};
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
    /// Words per lesson text.
    #[serde(default = "words")]
    pub words: [u32; 2],
    /// Lessons per theme.
    #[serde(default = "lessons")]
    pub lessons: [u32; 2],
    #[serde(default)]
    pub topic: Vec<Topic>,
}
fn b2() -> String {
    "B2".into()
}
fn words() -> [u32; 2] {
    [300, 450]
}
fn lessons() -> [u32; 2] {
    [3, 6]
}

/// A theme of the syllabus.
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

/// A reference page to read for a theme.
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

fn check_name(name: &str) -> Result<(), CourseError> {
    if name.is_empty() || name.contains(['/', '\\', '.']) {
        return Err(CourseError::Invalid("名稱不對".into()));
    }
    Ok(())
}

pub fn load_syllabus(root: &Path, collection: &str) -> Result<Syllabus, CourseError> {
    check_name(collection)?;
    let p = library_dir(root).join(collection).join("syllabus.toml");
    let text = std::fs::read_to_string(&p).map_err(|_| CourseError::NotFound(format!("{}", p.display())))?;
    let s: Syllabus = toml::from_str(&text).map_err(|e| CourseError::Invalid(format!("syllabus.toml：{e}")))?;
    Ok(s)
}

// ── step 1: a theme split into lessons ──────────────────

/// One lesson of a theme: one thing, with an English goal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LessonFocus {
    pub id: String,
    pub zh: String,
    pub en: String,
    pub focus: String,
    pub goal: String,
    #[serde(default)]
    pub vocabulary: Vec<String>,
    /// Numbers of the sources given when the plan was made (1-based).
    #[serde(default)]
    pub sources: Vec<usize>,
    /// Those sources as the theme's references (`w:…`), which stay valid when
    /// a page cannot be read another time.
    #[serde(default)]
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ThemePlan {
    pub theme: String,
    #[serde(default)]
    pub lesson: Vec<LessonFocus>,
}

/// `library/<collection>/plans/<theme>.toml` (the author may edit it).
pub fn plan_path(root: &Path, collection: &str, t: &Topic) -> PathBuf {
    library_dir(root).join(collection).join("plans").join(format!("{}.toml", t.id))
}

pub fn load_plan(root: &Path, collection: &str, t: &Topic) -> Option<ThemePlan> {
    toml::from_str(&std::fs::read_to_string(plan_path(root, collection, t)).ok()?).ok().filter(|p: &ThemePlan| !p.lesson.is_empty())
}

pub fn save_plan(root: &Path, collection: &str, t: &Topic, plan: &ThemePlan) -> Result<(), CourseError> {
    let p = plan_path(root, collection, t);
    std::fs::create_dir_all(p.parent().expect("has a parent"))?;
    let body = toml::to_string(plan).map_err(|e| CourseError::Invalid(e.to_string()))?;
    let head = format!("# {}（{}）：拆成幾堂課，每堂只講一件事。可以改 focus、goal、vocabulary，或刪掉一堂；\n# 已經寫好的文章不會因為這裡改了而重寫（刪掉那篇 .md 才會重寫）。\n\n", t.zh, t.en);
    std::fs::write(p, format!("{head}{body}"))?;
    Ok(())
}

pub fn split_schema() -> Value {
    let s = json!({ "type": "string" });
    strict_object(json!({
        "lessons": { "type": "array", "items": strict_object(json!({
            "zh": s, "en": s, "focus": s, "goal": s,
            "vocabulary": { "type": "array", "items": s },
            "sources": { "type": "array", "items": { "type": "integer" } },
        })) }
    }))
}

fn base_context(syl: &Syllabus, topic: &Topic, native_language: &str, sources: &[SourceDoc]) -> Value {
    json!({
        "collection": syl.name,
        "language": syl.lang.english_name(),
        "native_language": crate::lesson::native_name(native_language),
        "level": syl.level,
        "min_words": syl.words[0],
        "max_words": syl.words[1],
        "min_lessons": syl.lessons[0],
        "max_lessons": syl.lessons[1],
        "topic_en": topic.en,
        "topic_zh": topic.zh,
        "category": topic.cat,
        "sources": sources.iter().map(|s| json!({ "title": s.title, "url": s.url, "text": s.text })).collect::<Vec<_>>(),
    })
}

pub fn split_request(t: &CourseTemplates, syl: &Syllabus, topic: &Topic, native_language: &str, sources: &[SourceDoc]) -> Result<CompletionRequest, CourseError> {
    let c = base_context(syl, topic, native_language, sources);
    Ok(CompletionRequest { system: t.render(LIBRARY_SPLIT_SYSTEM, c.clone())?, user: t.render(LIBRARY_SPLIT_USER, c)?, max_tokens: 3000, json_schema: Some(split_schema()) })
}

#[derive(Deserialize)]
struct SplitOut {
    lessons: Vec<SplitLesson>,
}
#[derive(Deserialize)]
struct SplitLesson {
    #[serde(default)]
    zh: String,
    #[serde(default)]
    en: String,
    #[serde(default)]
    focus: String,
    #[serde(default)]
    goal: String,
    #[serde(default)]
    vocabulary: Vec<String>,
    #[serde(default)]
    sources: Vec<usize>,
}

/// The planned lessons, checked: each has a focus, a goal and a real source;
/// at most the syllabus' number; ids `<theme>-1`, `<theme>-2`, …
pub fn parse_split(text: &str, syl: &Syllabus, topic: &Topic, n_sources: usize) -> Result<ThemePlan, CourseError> {
    let out: SplitOut = parse_json(text)?;
    let mut lesson = vec![];
    for l in out.lessons {
        let mut sources: Vec<usize> = l.sources.into_iter().filter(|i| *i >= 1 && *i <= n_sources).collect();
        sources.sort();
        sources.dedup();
        if l.focus.trim().is_empty() || l.goal.trim().is_empty() || l.en.trim().is_empty() || sources.is_empty() {
            continue;
        }
        let vocabulary: Vec<String> = l.vocabulary.into_iter().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).take(10).collect();
        lesson.push(LessonFocus { id: String::new(), zh: l.zh.trim().to_string(), en: l.en.trim().to_string(), focus: l.focus.trim().to_string(), goal: l.goal.trim().to_string(), vocabulary, sources, refs: vec![] });
        if lesson.len() as u32 == syl.lessons[1] {
            break;
        }
    }
    if lesson.is_empty() {
        return Err(CourseError::Invalid("拆不出可用的課（每堂要有重點、英文目標與出處）".into()));
    }
    for (i, l) in lesson.iter_mut().enumerate() {
        l.id = format!("{}-{}", topic.id, i + 1);
    }
    Ok(ThemePlan { theme: topic.id.clone(), lesson })
}

// ── step 2: one text per lesson ─────────────────────────

/// `library/<collection>/<category>/<lesson id>.md`.
pub fn article_path(root: &Path, collection: &str, t: &Topic, lesson_id: &str) -> PathBuf {
    library_dir(root).join(collection).join(&t.cat).join(format!("{lesson_id}.md"))
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

/// The text of one lesson; `sources` are the ones the plan gave this lesson.
pub fn article_request(t: &CourseTemplates, syl: &Syllabus, topic: &Topic, lesson: &LessonFocus, native_language: &str, sources: &[SourceDoc]) -> Result<CompletionRequest, CourseError> {
    let mut c = base_context(syl, topic, native_language, sources);
    c["lesson_en"] = json!(lesson.en);
    c["lesson_zh"] = json!(lesson.zh);
    c["focus"] = json!(lesson.focus);
    c["goal"] = json!(lesson.goal);
    c["vocabulary"] = json!(lesson.vocabulary);
    Ok(CompletionRequest { system: t.render(LIBRARY_SYSTEM, c.clone())?, user: t.render(LIBRARY_USER, c)?, max_tokens: 3000, json_schema: Some(article_schema()) })
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

/// The model's text, checked: about the right length, at least one real
/// source used (a text without a source is not kept).
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

/// The text file: `key: <json>` lines (like lesson files), the text, key
/// terms and references.
#[allow(clippy::too_many_arguments)]
pub fn render_article(syl: &Syllabus, collection: &str, topic: &Topic, lesson: &LessonFocus, a: &ArticleOut, sources: &[SourceDoc], image: Option<&super::picture::ImageRef>, engine: &str, date: &str) -> String {
    let used: Vec<&SourceDoc> = a.used_sources.iter().filter_map(|i| sources.get(i - 1)).collect();
    let mut s = String::from("---\n");
    let line = |k: &str, v: Value| format!("{k}: {v}\n");
    s += &line("id", json!(lesson.id));
    s += &line("theme", json!(topic.id));
    s += &line("collection", json!(collection));
    s += &line("title", json!(a.title));
    s += &line("lesson", json!(lesson.en));
    s += &line("lesson_zh", json!(lesson.zh));
    s += &line("focus", json!(lesson.focus));
    s += &line("goal", json!(lesson.goal));
    s += &line("theme_zh", json!(topic.zh));
    s += &line("category", json!(topic.cat));
    s += &line("lang", json!(syl.lang.code()));
    s += &line("level", json!(syl.level));
    s += &line("words", json!(word_count(&a.text)));
    s += &line("news", json!(topic.news));
    s += &line("summary", json!(a.summary));
    s += &line("key_terms", json!(a.key_terms));
    s += &line("sources", json!(used));
    if let Some(img) = image {
        s += &line("image", json!(img));
    }
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
    if let Some(img) = image {
        s += &format!("\nPicture: {} — {} (<{}>)\n", img.author, img.licence, img.page);
    }
    s
}

/// A text file read back: its front matter values and the text alone.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Article {
    pub id: String,
    pub theme: String,
    pub title: String,
    pub category: String,
    pub lesson_zh: String,
    pub goal: String,
    pub level: String,
    pub words: usize,
    pub text: String,
    pub news: Vec<String>,
    pub sources: Vec<SourceDoc>,
    pub key_terms: Vec<KeyTerm>,
    /// A free picture for the text (downloaded only when a lesson is made).
    pub image: Option<super::picture::ImageRef>,
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
        theme: s("theme"),
        title: s("title"),
        category: s("category"),
        lesson_zh: s("lesson_zh"),
        goal: s("goal"),
        level: s("level"),
        words: fm("words").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
        news: fm("news").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
        sources: fm("sources").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
        key_terms: fm("key_terms").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
        image: fm("image").and_then(|v| serde_json::from_value(v).ok()),
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
  { id = "whisky", cat = "飲食", zh = "威士忌", en = "Scotch whisky", refs = ["w:Scotch whisky", "v:Edinburgh", "https://example.org/x"], news = ["whisky"] },
]
"#,
        )
        .unwrap()
    }
    fn src() -> Vec<SourceDoc> {
        vec![
            SourceDoc { title: "Scotch whisky".into(), url: "https://en.wikipedia.org/wiki/Scotch_whisky".into(), licence: "CC BY-SA 4.0 (Wikipedia)".into(), text: "Whisky is aged three years.".into() },
            SourceDoc { title: "Edinburgh".into(), url: "https://en.wikivoyage.org/wiki/Edinburgh".into(), licence: "CC BY-SA 4.0 (Wikivoyage)".into(), text: "Pubs on the Royal Mile.".into() },
        ]
    }
    fn focus() -> LessonFocus {
        LessonFocus { id: "whisky-1".into(), zh: "點一杯威士忌".into(), en: "Ordering a dram".into(), focus: "How to order whisky in a Scottish pub.".into(), goal: "能在酒吧點威士忌".into(), vocabulary: vec!["dram".into(), "neat".into()], sources: vec![1, 2], refs: vec![] }
    }

    #[test]
    fn syllabus_defaults_and_references() {
        let s = syl();
        assert_eq!((s.level.as_str(), s.words, s.lessons), ("B2", [300, 450], [3, 6]));
        let r: Vec<RefPage> = s.topic[0].refs.iter().filter_map(|r| ref_page(r)).collect();
        assert_eq!(r[0].url, "https://en.wikipedia.org/wiki/Scotch_whisky");
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
    fn a_theme_is_split_into_one_thing_lessons() {
        let s = syl();
        let t = CourseTemplates::bundled();
        let req = split_request(&t, &s, &s.topic[0], "zh-TW", &src()).unwrap();
        assert!(req.system.contains("One lesson = one thing") && req.system.contains("Language first") && req.system.contains("3–6"));
        assert!(req.user.contains("[2] Edinburgh"));
        let reply = json!({ "lessons": [
            { "zh": "點一杯威士忌", "en": "Ordering a dram", "focus": "How to order.", "goal": "能點酒", "vocabulary": ["dram", " neat ", ""], "sources": [2, 2, 9] },
            { "zh": "沒出處", "en": "No source", "focus": "x", "goal": "y", "vocabulary": [], "sources": [7] },
            { "zh": "沒重點", "en": "No focus", "focus": " ", "goal": "y", "vocabulary": [], "sources": [1] },
            { "zh": "三年規定", "en": "Three years in oak", "focus": "The ageing rule.", "goal": "能說明熟成", "vocabulary": ["cask"], "sources": [1] },
        ] }).to_string();
        let plan = parse_split(&reply, &s, &s.topic[0], 2).unwrap();
        assert_eq!(plan.lesson.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), vec!["whisky-1", "whisky-2"]);
        assert_eq!(plan.lesson[0].vocabulary, vec!["dram", "neat"]);
        assert_eq!(plan.lesson[0].sources, vec![2]);
        assert!(parse_split(&json!({ "lessons": [] }).to_string(), &s, &s.topic[0], 2).is_err());
        // Saved and read back (the author may edit it).
        let dir = tempfile::tempdir().unwrap();
        save_plan(dir.path(), "uk", &s.topic[0], &plan).unwrap();
        assert_eq!(load_plan(dir.path(), "uk", &s.topic[0]).unwrap(), plan);
    }

    #[test]
    fn texts_need_a_source_and_the_right_length_and_round_trip() {
        let s = syl();
        let t = CourseTemplates::bundled();
        let req = article_request(&t, &s, &s.topic[0], &focus(), "zh-TW", &src()).unwrap();
        assert!(req.system.contains("one thing only") && req.system.contains("dram, neat") && req.system.contains("300–450") && req.system.contains("Do not invent"));
        let text = std::iter::repeat_n("word", 360).collect::<Vec<_>>().join(" ");
        let ok = json!({ "title": "A Dram, Please", "text": text, "key_terms": [{ "term": "dram", "meaning": "一小杯" }], "used_sources": [1, 1, 7], "summary": "點酒" }).to_string();
        let a = parse_article(&ok, &s, &src()).unwrap();
        assert_eq!(a.used_sources, vec![1]);
        assert!(parse_article(&json!({ "title": "x", "text": text, "key_terms": [], "used_sources": [5], "summary": "" }).to_string(), &s, &src()).is_err(), "no real source");
        assert!(parse_article(&json!({ "title": "x", "text": "too short", "key_terms": [], "used_sources": [1], "summary": "" }).to_string(), &s, &src()).is_err());
        let img = super::super::picture::ImageRef { url: "https://thumb.wikimedia.org/x/800px-a.jpg".into(), page: "https://commons.wikimedia.org/wiki/File:a.jpg".into(), author: "D. K.".into(), licence: "CC BY-SA 3.0".into(), licence_url: String::new() };
        let md = render_article(&s, "uk", &s.topic[0], &focus(), &a, &src(), Some(&img), "claude:sonnet", "2026-11-08");
        assert!(md.contains("Picture: D. K. — CC BY-SA 3.0"));
        assert!(md.contains("## References") && md.contains("CC BY-SA"));
        let back = read_article(&md).unwrap();
        assert_eq!((back.id.as_str(), back.theme.as_str(), back.title.as_str(), back.words), ("whisky-1", "whisky", "A Dram, Please", 360));
        assert_eq!((back.lesson_zh.as_str(), back.goal.as_str()), ("點一杯威士忌", "能在酒吧點威士忌"));
        assert_eq!(back.text, text);
        assert_eq!(back.key_terms[0].meaning, "一小杯");
        assert_eq!(back.image, Some(img));
        assert_eq!(article_path(Path::new("/w"), "uk", &s.topic[0], "whisky-1"), PathBuf::from("/w/library/uk/飲食/whisky-1.md"));
    }
}
