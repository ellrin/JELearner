//! A picture for a lesson text (docs/decisions.md §22).
//!
//! The lead image of a Wikipedia reference page (its `og:image`) when it is on
//! Wikimedia Commons under a free licence; the author and licence come from
//! the file's Commons page and are shown under the picture. The library text
//! keeps only this reference; the picture itself is downloaded and compressed
//! (≤ 800 px wide, JPEG) when a lesson is made, and removed when the lesson is
//! done, so pictures never pile up in the workspace.

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

/// Width of a stored picture.
pub const MAX_WIDTH: u32 = 800;
/// JPEG quality of a stored picture.
pub const QUALITY: u8 = 72;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageRef {
    /// The picture to download (a Wikimedia thumbnail).
    pub url: String,
    /// The file's page (where the licence is).
    pub page: String,
    pub author: String,
    pub licence: String,
    #[serde(default)]
    pub licence_url: String,
}

/// The page's lead picture (`<meta property="og:image">`), resolved against the page.
pub fn og_image(html: &str, page_url: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    let sel = Selector::parse("meta[property='og:image']").ok()?;
    let raw = doc.select(&sel).next()?.value().attr("content")?.trim().to_string();
    let url = url::Url::parse(page_url).ok()?.join(&raw).ok()?;
    // Tracking parameters are not part of the picture.
    let url = url.as_str().split('?').next().unwrap_or("").to_string();
    (url.starts_with("https://") || url.starts_with("http://")).then_some(url)
}

/// A Commons picture's file page; `None` for files outside Commons (often
/// non-free, e.g. `/wikipedia/en/`).
pub fn commons_page(img: &str) -> Option<String> {
    let (origin, path) = {
        let after = img.split_once("://")?.1;
        let (host, path) = after.split_once('/')?;
        (format!("{}://{host}", img.split_once("://")?.0), format!("/{path}"))
    };
    let rest = path.split_once("/wikipedia/commons/")?.1;
    // thumb/x/xy/Name.jpg/960px-Name.jpg  or  x/xy/Name.jpg
    let parts: Vec<&str> = rest.split('/').collect();
    let name = match parts.as_slice() {
        ["thumb", _, _, name, ..] => *name,
        [_, _, name, ..] => *name,
        _ => return None,
    };
    let host = if origin.contains("wikimedia.org") { "https://commons.wikimedia.org".to_string() } else { origin };
    Some(format!("{host}/wiki/File:{name}"))
}

/// The licence and author on a Commons file page.
pub fn parse_commons(html: &str) -> Option<(String, String, String)> {
    let doc = Html::parse_document(html);
    let text = |sel: &str| -> Option<String> {
        let s = Selector::parse(sel).ok()?;
        let t = doc.select(&s).next()?.text().collect::<Vec<_>>().join(" ");
        let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
        (!t.is_empty()).then_some(t)
    };
    let licence = text(".licensetpl_short")?;
    let link = text(".licensetpl_link").unwrap_or_default();
    let author = text("#fileinfotpl_aut + td").unwrap_or_else(|| "Wikimedia Commons".into());
    Some((author, licence, link))
}

/// Free to adapt with attribution: CC BY / BY-SA, CC0, public domain.
pub fn free_licence(l: &str) -> bool {
    let u = l.to_uppercase();
    let free = u.starts_with("CC BY") || u.starts_with("CC0") || u.starts_with("CC-BY") || u.contains("PUBLIC DOMAIN") || u.starts_with("PD");
    free && !u.contains("NC") && !u.contains("ND")
}

/// Shrink to at most [`MAX_WIDTH`] wide and encode as JPEG.
pub fn compress(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("圖片讀不懂：{e}"))?;
    let img = if img.width() > MAX_WIDTH { img.resize(MAX_WIDTH, u32::MAX, image::imageops::FilterType::Lanczos3) } else { img };
    let rgb = img.to_rgb8();
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, QUALITY);
    enc.encode_image(&rgb).map_err(|e| e.to_string())?;
    Ok(out)
}

/// A picture on a reference page that could go with a paragraph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Candidate {
    /// The thumbnail to download (the largest the page offers).
    pub url: String,
    /// The file's Commons page (where the licence is).
    pub page: String,
    /// Its caption, alt text or file name: what it shows.
    pub caption: String,
}

/// A picture placed after a paragraph of a lesson text, with its credit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LessonPicture {
    /// The paragraph (0-based) it follows.
    pub paragraph: usize,
    pub url: String,
    pub page: String,
    pub author: String,
    pub licence: String,
    #[serde(default)]
    pub caption: String,
}

/// File names that are rarely a picture of the topic.
fn decoration(name: &str) -> bool {
    let n = name.to_lowercase();
    ["flag_of", "coat_of_arms", "logo", "icon", "symbol", "wikidata", "commons-logo", "edit-clear", "padlock", "question_book", "ambox", "location_map", "locator"].iter().any(|w| n.contains(w))
}

/// The pictures a Wikipedia or Wikivoyage page shows (figures and infobox
/// images), in page order: only files on Wikimedia Commons (files kept on the
/// wiki itself are often not free), no flags, logos or icons, no tiny ones.
pub fn page_images(html: &str, page_url: &str) -> Vec<Candidate> {
    let doc = Html::parse_document(html);
    let Ok(base) = url::Url::parse(page_url) else { return vec![] };
    let link_sel = Selector::parse("a[href*='/wiki/File:']").expect("selector");
    let img_sel = Selector::parse("img").expect("selector");
    let mut out: Vec<Candidate> = vec![];
    for a in doc.select(&link_sel) {
        let Some(img) = a.select(&img_sel).next() else { continue };
        let v = img.value();
        if v.attr("width").and_then(|w| w.parse::<u32>().ok()).is_some_and(|w| w < 120) {
            continue;
        }
        // The largest size the page offers (srcset "… 2x"), else src.
        let src = v.attr("srcset").and_then(|ss| ss.split(',').map(|x| x.trim()).filter(|x| !x.is_empty()).last().and_then(|x| x.split_whitespace().next())).or_else(|| v.attr("src"));
        let Some(src) = src else { continue };
        let Ok(url) = base.join(src) else { continue };
        let url = url.as_str().split('?').next().unwrap_or("").to_string();
        let Some(page) = commons_page(&url) else { continue };
        let name = page.rsplit("File:").next().unwrap_or("").to_string();
        if decoration(&name) || out.iter().any(|c| c.page == page) {
            continue;
        }
        // The figure's caption, else the image's alt text, else the file name.
        let caption = a
            .ancestors()
            .filter_map(scraper::ElementRef::wrap)
            .find(|e| e.value().name() == "figure" || e.value().classes().any(|c| c == "thumb"))
            .and_then(|f| f.select(&Selector::parse("figcaption, .thumbcaption").expect("selector")).next())
            .map(|c| c.text().collect::<Vec<_>>().join(" "))
            .or_else(|| v.attr("alt").map(str::to_string))
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| name.rsplit_once('.').map(|(n, _)| n).unwrap_or(&name).replace(['_', '-'], " "));
        let caption = caption.split_whitespace().collect::<Vec<_>>().join(" ");
        out.push(Candidate { url, page, caption });
    }
    out
}

fn words(t: &str) -> std::collections::HashSet<String> {
    const STOP: &[&str] = &["this", "that", "with", "from", "have", "were", "they", "their", "there", "which", "about", "into", "when", "what", "your", "also", "than", "then", "them", "been", "will", "more", "most", "some", "such", "only", "very", "many"];
    t.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 4 && !STOP.contains(w))
        .map(|w| w.trim_end_matches('s').to_string())
        .collect()
}

/// Which picture goes after which paragraph: each paragraph takes the unused
/// picture whose caption shares the most words with it, or the next one in
/// page order when none does (they all come from the topic's own pages);
/// at most `max` pictures, each used once. Returns (paragraph, candidate).
pub fn match_paragraphs(paragraphs: &[String], candidates: &[Candidate], max: usize) -> Vec<(usize, usize)> {
    let caps: Vec<_> = candidates.iter().map(|c| words(&c.caption)).collect();
    let mut used = vec![false; candidates.len()];
    let mut out = vec![];
    for (i, p) in paragraphs.iter().enumerate() {
        if out.len() >= max || used.iter().all(|u| *u) {
            break;
        }
        let pw = words(p);
        let best = (0..candidates.len()).filter(|k| !used[*k]).max_by_key(|k| (caps[*k].intersection(&pw).count(), std::cmp::Reverse(*k)));
        if let Some(k) = best {
            used[k] = true;
            out.push((i, k));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_pictures_and_paragraph_matching() {
        let html = r#"<div id="mw-content-text">
          <table class="infobox"><tr><td><a href="/wiki/File:Flag_of_Scotland.svg"><img src="//upload.wikimedia.org/wikipedia/commons/thumb/1/10/Flag_of_Scotland.svg/250px-Flag_of_Scotland.svg.png" width="250"></a></td></tr>
          <tr><td><a href="/wiki/File:Glenlivet_distillery.jpg"><img alt="The Glenlivet distillery in Speyside" src="//upload.wikimedia.org/wikipedia/commons/thumb/a/aa/Glenlivet_distillery.jpg/250px-Glenlivet_distillery.jpg" srcset="//upload.wikimedia.org/wikipedia/commons/thumb/a/aa/Glenlivet_distillery.jpg/375px-Glenlivet_distillery.jpg 1.5x, //upload.wikimedia.org/wikipedia/commons/thumb/a/aa/Glenlivet_distillery.jpg/500px-Glenlivet_distillery.jpg 2x" width="250"></a></td></tr></table>
          <figure><a href="/wiki/File:Whisky_barrels.jpg"><img src="//upload.wikimedia.org/wikipedia/commons/thumb/b/bb/Whisky_barrels.jpg/220px-Whisky_barrels.jpg" width="220"></a><figcaption>Oak barrels where the whisky is aged</figcaption></figure>
          <figure><a href="/wiki/File:Poster.jpg"><img src="//upload.wikimedia.org/wikipedia/en/c/cc/Poster.jpg" width="220"></a><figcaption>A poster (not free)</figcaption></figure>
          <a href="/wiki/File:Tiny.png"><img src="//upload.wikimedia.org/wikipedia/commons/x/xy/Tiny.png" width="20"></a>
        </div>"#;
        let c = page_images(html, "https://en.wikipedia.org/wiki/Scotch_whisky");
        let names: Vec<&str> = c.iter().map(|x| x.page.rsplit("File:").next().unwrap()).collect();
        assert_eq!(names, vec!["Glenlivet_distillery.jpg", "Whisky_barrels.jpg"], "no flag, no non-Commons file, no icon");
        assert_eq!(c[0].url, "https://upload.wikimedia.org/wikipedia/commons/thumb/a/aa/Glenlivet_distillery.jpg/500px-Glenlivet_distillery.jpg", "the largest offered");
        assert_eq!(c[0].caption, "The Glenlivet distillery in Speyside");
        assert_eq!(c[1].caption, "Oak barrels where the whisky is aged");
        let paras = vec!["Scotch must be aged in oak barrels for three years.".to_string(), "The Glenlivet distillery opened in Speyside in 1824.".to_string(), "Ask for a dram.".to_string()];
        assert_eq!(match_paragraphs(&paras, &c, 5), vec![(0, 1), (1, 0)], "by shared words, each picture once");
        assert_eq!(match_paragraphs(&paras, &c, 1), vec![(0, 1)]);
        assert!(match_paragraphs(&paras, &[], 5).is_empty());
    }

    #[test]
    fn lead_picture_and_its_commons_page() {
        let html = r#"<html><head><meta property="og:image" content="https://thumb.wikimedia.org/wikipedia/commons/thumb/b/bb/Edinburgh_Castle.jpg/1280px-Edinburgh_Castle.jpg?utm_source=en.wikipedia.org&amp;utm_campaign=index"></head></html>"#;
        let img = og_image(html, "https://en.wikipedia.org/wiki/Edinburgh_Castle").unwrap();
        assert_eq!(img, "https://thumb.wikimedia.org/wikipedia/commons/thumb/b/bb/Edinburgh_Castle.jpg/1280px-Edinburgh_Castle.jpg");
        assert_eq!(commons_page(&img).unwrap(), "https://commons.wikimedia.org/wiki/File:Edinburgh_Castle.jpg");
        assert_eq!(commons_page("https://upload.wikimedia.org/wikipedia/commons/a/ab/Haggis.jpg").unwrap(), "https://commons.wikimedia.org/wiki/File:Haggis.jpg");
        assert!(commons_page("https://upload.wikimedia.org/wikipedia/en/a/ab/Poster.jpg").is_none(), "not on Commons (often non-free)");
        assert_eq!(commons_page("http://127.0.0.1:9/wikipedia/commons/thumb/a/ab/T.png/960px-T.png").unwrap(), "http://127.0.0.1:9/wiki/File:T.png", "tests use their own host");
        assert!(og_image("<html></html>", "https://x.org/").is_none());
        assert_eq!(og_image(r#"<meta property="og:image" content="/wikipedia/commons/thumb/a/ab/T.png/960px-T.png?x=1">"#, "http://127.0.0.1:9/wiki/T").unwrap(), "http://127.0.0.1:9/wikipedia/commons/thumb/a/ab/T.png/960px-T.png");
    }

    #[test]
    fn licence_and_author_from_a_commons_page() {
        // Commons writes the underscores of these class names as entities.
        let html = r#"<table><tr><td id="fileinfotpl&#95;aut">Author</td><td><a href="/wiki/User:D">Daniel Kraft</a></td></tr></table>
            <div class="licensetpl"><span class="licensetpl&#95;short" style="display:none">CC BY-SA 3.0</span><span class="licensetpl&#95;link" style="display:none">https://creativecommons.org/licenses/by-sa/3.0</span></div>"#;
        assert_eq!(parse_commons(html).unwrap(), ("Daniel Kraft".into(), "CC BY-SA 3.0".into(), "https://creativecommons.org/licenses/by-sa/3.0".into()));
        assert!(parse_commons("<p>no licence</p>").is_none());
        assert!(free_licence("CC BY-SA 3.0") && free_licence("CC0") && free_licence("Public domain") && free_licence("CC BY 2.0"));
        assert!(!free_licence("CC BY-NC-SA 2.0") && !free_licence("CC BY-ND 4.0") && !free_licence("Fair use"));
    }

    #[test]
    fn pictures_are_shrunk_to_a_small_jpeg() {
        let big = image::RgbImage::from_fn(1600, 900, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 128]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(big).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let jpg = compress(&png).unwrap();
        let back = image::load_from_memory(&jpg).unwrap();
        assert_eq!((back.width(), back.height()), (800, 450));
        assert!(jpg.len() < 120_000, "a small file: {} bytes", jpg.len());
        assert!(compress(b"not a picture").is_err());
    }
}
