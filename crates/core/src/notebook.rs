//! Review notebook (復習筆記本, docs/plan.md §11.1): one page per lesson and
//! one per conversation, in `profiles/<id>/notebook/<lang>/<stem>.md`.
//!
//! A page has two parts kept apart so that regenerating never touches what
//! the learner wrote:
//!
//! - the **app section** between `<!-- jelearner:auto -->` and
//!   `<!-- /jelearner:auto -->` (wrong answers with the right answer and the
//!   explanation, the lesson's new words and grammar, the conversation's
//!   "could say better" sentences), rewritten every time the lesson or
//!   conversation changes;
//! - the learner's **notes** under `## 我的筆記`, written only when the
//!   learner saves them in the app (or by hand in any editor).
//!
//! Pictures are not in the workspace: a note refers to one as
//! `![](jl-image:<id>)` and the file stays on the computer it was added on
//! (`<config dir>/JELearner/notebook-images/…`, see the server).
//! The page ends with the app data as an HTML comment, like lessons.

use crate::course::plan::ruby_html_escaped;
use crate::lang::Lang;
use crate::workspace::{slug, Workspace};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const AUTO_START: &str = "<!-- jelearner:auto 這一段由 App 產生，重新產生時會整段取代；自己的筆記請寫在「我的筆記」下面 -->";
const AUTO_START_PREFIX: &str = "<!-- jelearner:auto";
pub const AUTO_END: &str = "<!-- /jelearner:auto -->";
pub const NOTES_HEADING: &str = "## 我的筆記";
const DATA_MARK: &str = "<!-- jelearner:notebook ";
/// How a note refers to a picture kept on this computer.
pub const IMAGE_SCHEME: &str = "jl-image:";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NoteMistake {
    pub question: String,
    #[serde(default)]
    pub your_answer: String,
    pub answer: String,
    #[serde(default)]
    pub explanation: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NoteWord {
    pub term: String,
    #[serde(default)]
    pub reading: String,
    #[serde(default)]
    pub meaning: String,
    #[serde(default)]
    pub example: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NoteGrammar {
    pub pattern: String,
    #[serde(default)]
    pub meaning: String,
    #[serde(default)]
    pub explanation: String,
    #[serde(default)]
    pub example: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NoteBetter {
    pub original: String,
    pub suggestion: String,
    #[serde(default)]
    pub why: String,
}

/// What the app writes on a page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageData {
    /// `lesson` | `conversation`
    pub kind: String,
    pub title: String,
    pub lang: Lang,
    /// `YYYY-MM-DD` of the lesson or conversation.
    pub date: String,
    /// Workspace-relative path of the lesson or conversation file.
    pub source: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub course: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub mistakes: Vec<NoteMistake>,
    #[serde(default)]
    pub words: Vec<NoteWord>,
    #[serde(default)]
    pub grammar: Vec<NoteGrammar>,
    #[serde(default)]
    pub better: Vec<NoteBetter>,
}

#[derive(Debug, thiserror::Error)]
pub enum NotebookError {
    #[error("找不到這一頁筆記")]
    NotFound,
    #[error("筆記頁 {0} 的 App 區段標記被移除了，App 不會改寫這個檔案")]
    NoMarkers(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

pub fn dir(ws: &Workspace, lang: Lang) -> PathBuf {
    ws.profile_dir().join("notebook").join(lang.code())
}

/// A page stem is a plain file name (no folders, no dots at the start).
pub fn valid_stem(stem: &str) -> bool {
    !stem.is_empty() && !stem.starts_with('.') && stem.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.')) && !stem.contains("..")
}

// ── Markdown ─────────────────────────────────────────────

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// Text from the model or the learner, for HTML inside Markdown: escaped,
/// Japanese `{漢字|かんじ}` readings as `<ruby>`.
fn text(s: &str, lang: Lang) -> String {
    if lang == Lang::Ja { ruby_html_escaped(s) } else { ruby_html_escaped(&crate::course::plan::strip_furigana(s)) }
}

fn term_html(w: &NoteWord, lang: Lang) -> String {
    let kana = |r: &str| !r.is_empty() && r.chars().all(|c| ('\u{3040}'..='\u{30ff}').contains(&c) || c == 'ー' || c.is_whitespace());
    if lang == Lang::Ja && kana(&w.reading) && w.term.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
        format!("<ruby>{}<rt>{}</rt></ruby>", ruby_html_escaped(&w.term), ruby_html_escaped(&w.reading))
    } else {
        ruby_html_escaped(&w.term)
    }
}

/// The app section (between the markers, markers included).
pub fn render_auto(d: &PageData) -> String {
    let l = d.lang;
    let mut out = format!("{AUTO_START}\n\n");
    if !d.summary.trim().is_empty() {
        out.push_str(&format!("> {}\n\n", text(d.summary.trim(), l).replace('\n', " ")));
    }
    if !d.mistakes.is_empty() {
        out.push_str(&format!("## 錯題（{}）\n\n", d.mistakes.len()));
        for (i, m) in d.mistakes.iter().enumerate() {
            out.push_str(&format!("{}. {}\n", i + 1, text(&m.question, l)));
            out.push_str(&format!("   - 你的答案：{}\n", if m.your_answer.trim().is_empty() { "（空白）".into() } else { text(&m.your_answer, l) }));
            out.push_str(&format!("   - 正確答案：**{}**\n", text(&m.answer, l)));
            if !m.explanation.trim().is_empty() {
                out.push_str(&format!("   - {}\n", text(&m.explanation, l)));
            }
        }
        out.push('\n');
    }
    if !d.better.is_empty() {
        out.push_str("## 可以這樣說\n\n");
        for b in &d.better {
            out.push_str(&format!("- {} → **{}**", text(&b.original, l), text(&b.suggestion, l)));
            if !b.why.trim().is_empty() {
                out.push_str(&format!("（{}）", text(&b.why, l)));
            }
            out.push('\n');
        }
        out.push('\n');
    }
    if !d.words.is_empty() {
        out.push_str(&format!("## 新單字（{}）\n\n| 單字 | 讀音 | 意思 | 例句 |\n| --- | --- | --- | --- |\n", d.words.len()));
        for w in &d.words {
            out.push_str(&format!("| {} | {} | {} | {} |\n", cell(&term_html(w, l)), cell(&ruby_html_escaped(&w.reading)), cell(&text(&w.meaning, l)), cell(&text(&w.example, l))));
        }
        out.push('\n');
    }
    if !d.grammar.is_empty() {
        out.push_str(&format!("## 新文法（{}）\n\n", d.grammar.len()));
        for g in &d.grammar {
            out.push_str(&format!("### {}", text(&g.pattern, l)));
            if !g.meaning.trim().is_empty() {
                out.push_str(&format!("：{}", text(&g.meaning, l)));
            }
            out.push_str("\n\n");
            if !g.explanation.trim().is_empty() {
                out.push_str(&format!("{}\n\n", text(&g.explanation, l)));
            }
            if !g.example.trim().is_empty() {
                out.push_str(&format!("- {}\n\n", text(&g.example, l)));
            }
        }
    }
    if d.mistakes.is_empty() && d.better.is_empty() && d.words.is_empty() && d.grammar.is_empty() {
        out.push_str("（這一課沒有錯題，也沒有新的字。）\n\n");
    }
    out.push_str(AUTO_END);
    out
}

fn data_comment(d: &PageData) -> String {
    let json = serde_json::to_string(d).unwrap_or_default().replace("-->", "--\\u003e");
    format!("{DATA_MARK}{json} -->")
}

/// A new page.
pub fn render_page(d: &PageData, notes: &str) -> String {
    let kind = if d.kind == "conversation" { "對話" } else { "課" };
    format!(
        "---\ntitle: {}\nkind: {}\nlang: {}\ndate: {}\nsource: {}\n{}---\n\n# {}\n\n> 復習筆記・{}・{}\n\n{}\n\n{NOTES_HEADING}\n\n{}\n\n{}\n",
        serde_json::to_string(&d.title).unwrap_or_default(),
        d.kind,
        d.lang.code(),
        d.date,
        serde_json::to_string(&d.source).unwrap_or_default(),
        if d.course.is_empty() { String::new() } else { format!("course: {}\n", serde_json::to_string(&d.course).unwrap_or_default()) },
        d.title.replace('\n', " "),
        kind,
        d.date,
        render_auto(d),
        notes.trim(),
        data_comment(d),
    )
}

/// Positions of the app section: (start of the start marker, end of the end
/// marker).
fn auto_span(text: &str) -> Option<(usize, usize)> {
    let s = text.find(AUTO_START_PREFIX)?;
    let e = text[s..].find(AUTO_END)? + s + AUTO_END.len();
    Some((s, e))
}

/// Replace the app section and the data comment; everything else (the
/// learner's notes, front matter, anything they added) stays as it is.
/// `None` when the markers are gone (then the file is left alone).
pub fn update_page(existing: &str, d: &PageData) -> Option<String> {
    let (s, e) = auto_span(existing)?;
    let mut out = format!("{}{}{}", &existing[..s], render_auto(d), &existing[e..]);
    match out.rfind(DATA_MARK) {
        Some(ds) => {
            let de = out[ds..].find("-->").map(|x| ds + x + 3).unwrap_or(out.len());
            out.replace_range(ds..de, &data_comment(d));
        }
        None => {
            out = format!("{}\n\n{}\n", out.trim_end(), data_comment(d));
        }
    }
    Some(out)
}

/// The learner's notes: the text under `## 我的筆記` (or, if that heading
/// was removed, everything after the app section), without the data.
pub fn notes(text: &str) -> String {
    let after = auto_span(text).map(|(_, e)| e).unwrap_or(0);
    let mut body = &text[after..];
    if let Some(d) = body.rfind(DATA_MARK) {
        body = &body[..d];
    }
    let body = body.trim_start();
    let body = body.strip_prefix(NOTES_HEADING).unwrap_or(body);
    body.trim().to_string()
}

/// Write the learner's notes (the app section is not touched).
pub fn set_notes(text: &str, new_notes: &str) -> Option<String> {
    let (_, e) = auto_span(text)?;
    let tail = &text[e..];
    let data = tail.rfind(DATA_MARK).map(|d| tail[d..].trim_end().to_string()).unwrap_or_default();
    Some(format!("{}\n\n{NOTES_HEADING}\n\n{}\n\n{}\n", &text[..e], new_notes.trim(), data).replace("\n\n\n\n", "\n\n"))
}

pub fn extract(text: &str) -> Option<PageData> {
    let start = text.rfind(DATA_MARK)? + DATA_MARK.len();
    let end = text[start..].find(" -->")? + start;
    serde_json::from_str(&text[start..end]).ok()
}

/// Picture ids a note refers to, in order.
pub fn image_ids(notes: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = notes;
    while let Some(i) = rest.find(IMAGE_SCHEME) {
        let after = &rest[i + IMAGE_SCHEME.len()..];
        let id: String = after.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        if !id.is_empty() && !out.contains(&id) {
            out.push(id.clone());
        }
        rest = &after[id.len()..];
    }
    out
}

// ── Files ────────────────────────────────────────────────

fn md_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "md")).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// The page whose `source` is this lesson or conversation file.
pub fn find_page(ws: &Workspace, lang: Lang, source: &str) -> Option<PathBuf> {
    md_files(&dir(ws, lang)).into_iter().find(|p| {
        std::fs::read_to_string(p).ok().and_then(|t| crate::lesson::front_matter(&t, "source")).is_some_and(|s| s == source)
    })
}

/// Create or refresh the page of a lesson or conversation. Returns the
/// workspace-relative path and whether it was created now.
pub fn save(ws: &Workspace, d: &PageData) -> Result<(String, bool), NotebookError> {
    let dir = dir(ws, d.lang);
    std::fs::create_dir_all(&dir)?;
    if let Some(path) = find_page(ws, d.lang, &d.source) {
        let old = std::fs::read_to_string(&path)?;
        let rel = ws.relative(&path);
        let new = update_page(&old, d).ok_or_else(|| NotebookError::NoMarkers(rel.clone()))?;
        if new != old {
            std::fs::write(&path, new)?;
        }
        return Ok((rel, false));
    }
    let stem = Path::new(&d.source).file_stem().and_then(|s| s.to_str()).map(|s| s.to_string()).unwrap_or_else(|| format!("{}-{}", d.date, slug(&d.title, 40)));
    let path = crate::course::store::free_path(&dir, &stem, "md");
    std::fs::write(&path, render_page(d, ""))?;
    Ok((ws.relative(&path), true))
}

pub fn page_path(ws: &Workspace, lang: Lang, stem: &str) -> Result<PathBuf, NotebookError> {
    if !valid_stem(stem) {
        return Err(NotebookError::NotFound);
    }
    let p = dir(ws, lang).join(format!("{stem}.md"));
    if p.is_file() { Ok(p) } else { Err(NotebookError::NotFound) }
}

/// One page, for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub lang: Lang,
    pub stem: String,
    pub path: String,
    pub title: String,
    pub kind: String,
    pub date: String,
    pub source: String,
    pub data: Option<PageData>,
    pub notes: String,
    pub images: Vec<String>,
    /// The app section's markers were removed by hand: the app no longer
    /// refreshes it.
    pub frozen: bool,
}

pub fn read(ws: &Workspace, lang: Lang, stem: &str) -> Result<Page, NotebookError> {
    let path = page_path(ws, lang, stem)?;
    let text = std::fs::read_to_string(&path)?;
    Ok(page_from(ws, lang, stem, &path, &text))
}

fn page_from(ws: &Workspace, lang: Lang, stem: &str, path: &Path, text: &str) -> Page {
    use crate::lesson::front_matter;
    let data = extract(text);
    let n = notes(text);
    Page {
        lang,
        stem: stem.to_string(),
        path: ws.relative(path),
        title: front_matter(text, "title").or_else(|| data.as_ref().map(|d| d.title.clone())).unwrap_or_else(|| stem.to_string()),
        kind: front_matter(text, "kind").unwrap_or_else(|| "lesson".into()),
        date: front_matter(text, "date").unwrap_or_default(),
        source: front_matter(text, "source").unwrap_or_default(),
        images: image_ids(&n),
        notes: n,
        frozen: auto_span(text).is_none(),
        data,
    }
}

/// Write the learner's notes on a page.
pub fn write_notes(ws: &Workspace, lang: Lang, stem: &str, new_notes: &str) -> Result<Page, NotebookError> {
    let path = page_path(ws, lang, stem)?;
    let text = std::fs::read_to_string(&path)?;
    let new = match set_notes(&text, new_notes) {
        Some(t) => t,
        // No app section: the notes are everything after the front matter heading.
        None => format!("{}\n\n{NOTES_HEADING}\n\n{}\n", text.split(NOTES_HEADING).next().unwrap_or(&text).trim_end(), new_notes.trim()),
    };
    std::fs::write(&path, &new)?;
    Ok(page_from(ws, lang, stem, &path, &new))
}

/// Every page (newest first), optionally of one language.
pub fn list(ws: &Workspace, lang: Option<Lang>) -> Vec<Page> {
    let mut out = vec![];
    for l in Lang::ALL {
        if lang.is_some_and(|x| x != l) {
            continue;
        }
        for p in md_files(&dir(ws, l)) {
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            out.push(page_from(ws, l, &stem, &p, &text));
        }
    }
    out.sort_by(|a, b| b.date.cmp(&a.date).then(b.stem.cmp(&a.stem)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> PageData {
        PageData {
            kind: "lesson".into(),
            title: "{今日|きょう}の天気".into(),
            lang: Lang::Ja,
            date: "2026-11-06".into(),
            source: "profiles/ellrin/lessons/ja/2026-11-06-weather.md".into(),
            course: String::new(),
            summary: "天氣".into(),
            mistakes: vec![NoteMistake { question: "「{雨|あめ}」の意味は？".into(), your_answer: "雪".into(), answer: "雨".into(), explanation: "a <b> tag | pipe".into() }],
            words: vec![NoteWord { term: "天気".into(), reading: "てんき".into(), meaning: "天氣".into(), example: "{今日|きょう}はいい{天気|てんき}です。".into() }],
            grammar: vec![],
            better: vec![],
        }
    }

    #[test]
    fn page_keeps_notes_apart_from_the_app_section() {
        let d = data();
        let page = render_page(&d, "");
        assert!(page.contains("<ruby>天気<rt>てんき</rt></ruby>"), "{page}");
        assert!(page.contains("<ruby>雨<rt>あめ</rt></ruby>"));
        assert!(page.contains("a &lt;b&gt; tag"), "model text is escaped");
        assert_eq!(notes(&page), "");
        let mine = "自分のメモ：雨と雪をまちがえた。\n\n![](jl-image:abc123)";
        let page = set_notes(&page, mine).unwrap();
        assert_eq!(notes(&page), mine);
        assert_eq!(image_ids(&page), vec!["abc123"]);
        // Regenerate with another mistake: the notes survive, the app section changes.
        let mut d2 = d.clone();
        d2.mistakes.push(NoteMistake { question: "Q2".into(), answer: "A2".into(), ..Default::default() });
        let page2 = update_page(&page, &d2).unwrap();
        assert_eq!(notes(&page2), mine);
        assert!(page2.contains("## 錯題（2）"));
        assert_eq!(extract(&page2).unwrap().mistakes.len(), 2);
        assert_eq!(update_page(&page2, &d2).unwrap(), page2, "idempotent");
        // A hand edit outside the app section stays too.
        let edited = page2.replace("# {今日|きょう}の天気", "# 天気（手で直したタイトル）");
        assert!(update_page(&edited, &d).unwrap().contains("手で直したタイトル"));
        // Markers removed by hand: the app leaves the file alone.
        let frozen = page2.replace(AUTO_END, "");
        assert!(update_page(&frozen, &d).is_none());
    }

    #[test]
    fn english_pages_strip_reading_marks() {
        let mut d = data();
        d.lang = Lang::En;
        d.words[0] = NoteWord { term: "subsidy".into(), reading: "/ˈsʌbsədi/".into(), meaning: "補助".into(), example: String::new() };
        let page = render_page(&d, "note");
        assert!(page.contains("| subsidy | /ˈsʌbsədi/ | 補助 |"), "{page}");
        assert!(!page.contains("<ruby>天気"));
        assert!(valid_stem("2026-11-06-weather") && !valid_stem("../x") && !valid_stem(".hidden") && !valid_stem("a/b"));
    }
}
