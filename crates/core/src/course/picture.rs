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

#[cfg(test)]
mod tests {
    use super::*;

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
