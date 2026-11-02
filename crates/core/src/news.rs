//! News sources (RSS / Atom feeds listed in the workspace's `sources.toml`),
//! feed parsing and article text extraction.

use crate::crawler::{CrawlError, Fetcher};
use crate::lang::Lang;
use chrono::{DateTime, Utc};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub lang: Lang,
    /// RSS or Atom feed URL.
    pub feed: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Optional CSS selector for the article body on this site.
    #[serde(default)]
    pub body_selector: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SourcesFile {
    #[serde(default, rename = "source")]
    pub sources: Vec<Source>,
}

/// Sources from the workspace file, or the bundled defaults when it is
/// missing or broken (the error is returned alongside for the UI).
pub fn load_sources(path: &Path, bundled: &str) -> (Vec<Source>, Option<String>) {
    let parse = |raw: &str| toml::from_str::<SourcesFile>(raw).map(|f| f.sources);
    match std::fs::read_to_string(path) {
        Ok(raw) => match parse(&raw) {
            Ok(s) => (s, None),
            Err(e) => (parse(bundled).unwrap_or_default(), Some(format!("sources.toml：{e}"))),
        },
        Err(_) => (parse(bundled).unwrap_or_default(), None),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedItem {
    pub source: String,
    pub lang: Lang,
    pub title: String,
    pub url: String,
    pub summary: String,
    pub published_at: Option<DateTime<Utc>>,
}

pub fn parse_feed(bytes: &[u8], source: &Source) -> Result<Vec<FeedItem>, String> {
    let feed = feed_rs::parser::parse(bytes).map_err(|e| format!("{}：feed 無法解析（{e}）", source.name))?;
    Ok(feed
        .entries
        .into_iter()
        .filter_map(|e| {
            let url = e.links.first()?.href.clone();
            let title = e.title.map(|t| clean(&t.content)).unwrap_or_default();
            let summary = e.summary.map(|s| text_of_html(&s.content)).unwrap_or_default();
            Some(FeedItem {
                source: source.name.clone(),
                lang: source.lang,
                title,
                url,
                summary,
                published_at: e.published.or(e.updated),
            })
        })
        .collect())
}

pub async fn fetch_feed(fetcher: &Fetcher, source: &Source) -> Result<Vec<FeedItem>, String> {
    let text = fetcher.get_text(&source.feed).await.map_err(|e| format!("{}：{e}", source.name))?;
    parse_feed(text.as_bytes(), source)
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtractedArticle {
    pub title: String,
    pub paragraphs: Vec<String>,
}

impl ExtractedArticle {
    pub fn body(&self) -> String {
        self.paragraphs.join("\n\n")
    }
}

fn clean(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Plain text from an HTML fragment (feed summaries often contain markup).
pub fn text_of_html(html: &str) -> String {
    let doc = Html::parse_fragment(html);
    clean(&doc.root_element().text().collect::<Vec<_>>().join(" "))
}

/// Pull the readable text out of an article page: the site-specific selector
/// when given, else `<article>`, `<main>`, or the whole body; paragraphs
/// shorter than a sentence (menus, captions) are dropped.
pub fn extract_article(html: &str, body_selector: Option<&str>) -> ExtractedArticle {
    let doc = Html::parse_document(html);
    let sel = |s: &str| Selector::parse(s).ok();
    let title = sel("meta[property='og:title']")
        .and_then(|s| doc.select(&s).next())
        .and_then(|n| n.value().attr("content").map(clean))
        .or_else(|| sel("h1").and_then(|s| doc.select(&s).next()).map(|n| clean(&n.text().collect::<String>())))
        .or_else(|| sel("title").and_then(|s| doc.select(&s).next()).map(|n| clean(&n.text().collect::<String>())))
        .unwrap_or_default();

    let containers = [body_selector.unwrap_or(""), "article", "main", "[role=main]", "body"];
    let p = sel("p").unwrap();
    for c in containers.iter().filter(|c| !c.is_empty()) {
        let Some(cs) = sel(c) else { continue };
        let paragraphs: Vec<String> = doc
            .select(&cs)
            .flat_map(|root| root.select(&p).map(|n| clean(&n.text().collect::<String>())).collect::<Vec<_>>())
            .filter(|t| t.chars().count() >= 25 || (t.chars().any(|c| ('\u{3040}'..='\u{9fff}').contains(&c)) && t.chars().count() >= 12))
            .collect();
        if !paragraphs.is_empty() {
            let mut seen = std::collections::HashSet::new();
            let paragraphs = paragraphs.into_iter().filter(|t| seen.insert(t.clone())).collect();
            return ExtractedArticle { title, paragraphs };
        }
    }
    ExtractedArticle { title, paragraphs: vec![] }
}

pub async fn fetch_article(fetcher: &Fetcher, url: &str, body_selector: Option<&str>) -> Result<ExtractedArticle, CrawlError> {
    let html = fetcher.get_text(url).await?;
    Ok(extract_article(&html, body_selector))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src() -> Source {
        Source { id: "t".into(), name: "Test".into(), lang: Lang::En, feed: "http://x/feed".into(), enabled: true, body_selector: None }
    }

    #[test]
    fn parses_rss_and_atom() {
        let rss = r#"<?xml version="1.0"?><rss version="2.0"><channel><title>T</title>
          <item><title>Chips  are back</title><link>http://x/a</link><description>&lt;p&gt;Short &lt;b&gt;summary&lt;/b&gt;&lt;/p&gt;</description>
          <pubDate>Mon, 02 Nov 2026 08:00:00 GMT</pubDate></item></channel></rss>"#;
        let items = parse_feed(rss.as_bytes(), &src()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Chips are back");
        assert_eq!(items[0].summary, "Short summary");
        assert!(items[0].published_at.is_some());

        let atom = r#"<?xml version="1.0"?><feed xmlns="http://www.w3.org/2005/Atom"><title>A</title><id>x</id><updated>2026-11-01T00:00:00Z</updated>
          <entry><title>生成AIの規制</title><link href="http://x/b"/><id>b</id><updated>2026-11-01T00:00:00Z</updated></entry></feed>"#;
        let items = parse_feed(atom.as_bytes(), &src()).unwrap();
        assert_eq!(items[0].url, "http://x/b");
    }

    #[test]
    fn extracts_article_paragraphs() {
        let html = r#"<html><head><title>Site</title><meta property="og:title" content="Robots  learn to fold laundry"></head>
          <body><nav><p>Home</p></nav><article><h1>ignored</h1>
          <p>Researchers announced a robot that can fold laundry reliably.</p>
          <p>Short.</p>
          <p>The team says the system learned from thousands of videos.</p></article></body></html>"#;
        let a = extract_article(html, None);
        assert_eq!(a.title, "Robots learn to fold laundry");
        assert_eq!(a.paragraphs.len(), 2);
        let ja = "<html><body><main><p>政府は新しい規制案を発表した。</p><p>メニュー</p></main></body></html>";
        assert_eq!(extract_article(ja, None).paragraphs, vec!["政府は新しい規制案を発表した。"]);
    }

    #[test]
    fn broken_sources_file_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sources.toml");
        std::fs::write(&p, "[[source]]\nid = 1").unwrap();
        let bundled = "[[source]]\nid='a'\nname='A'\nlang='ja'\nfeed='http://a/rss'\n";
        let (s, err) = load_sources(&p, bundled);
        assert_eq!(s.len(), 1);
        assert!(err.is_some());
    }
}
