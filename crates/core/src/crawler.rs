//! Polite HTTP fetching for the news crawler.
//!
//! Every request: checks the site's robots.txt for our user agent (cached
//! per origin), waits between requests to the same host (at least
//! `min_delay`, or the site's Crawl-delay), sends an honest User-Agent and
//! caps the response size.

use crate::robots::Robots;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use url::Url;

pub const DEFAULT_USER_AGENT: &str = "JELearnerBot/0.1 (+https://github.com/ellrin/JELearner; personal language study)";
const MAX_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum CrawlError {
    #[error("網址無效：{0}")]
    BadUrl(String),
    #[error("robots.txt 不允許抓取：{0}")]
    Disallowed(String),
    #[error("無法連線：{0}")]
    Http(String),
    #[error("伺服器回應 {0}")]
    Status(u16),
    #[error("內容過大（超過 4 MB）")]
    TooLarge,
}

pub struct Fetcher {
    http: reqwest::Client,
    pub user_agent: String,
    pub min_delay: Duration,
    robots: Mutex<HashMap<String, Robots>>,
    last: Mutex<HashMap<String, Instant>>,
}

impl Fetcher {
    pub fn new(user_agent: Option<String>, min_delay: Duration) -> Fetcher {
        let user_agent = user_agent.unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());
        let http = reqwest::Client::builder()
            .user_agent(user_agent.clone())
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .expect("http client");
        Fetcher { http, user_agent, min_delay, robots: Mutex::new(HashMap::new()), last: Mutex::new(HashMap::new()) }
    }

    fn origin(url: &Url) -> String {
        let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
        format!("{}://{}{}", url.scheme(), url.host_str().unwrap_or(""), port)
    }

    async fn robots_for(&self, url: &Url) -> Robots {
        let origin = Self::origin(url);
        if let Some(r) = self.robots.lock().await.get(&origin) {
            return r.clone();
        }
        let robots = match self.http.get(format!("{origin}/robots.txt")).send().await {
            Ok(resp) if resp.status().is_success() => Robots::parse(&resp.text().await.unwrap_or_default()),
            // No robots.txt (4xx): crawling is allowed.
            Ok(resp) if resp.status().is_client_error() => Robots::allow_all(),
            // Server error or unreachable: assume we may not crawl.
            _ => Robots::deny_all(),
        };
        self.robots.lock().await.insert(origin, robots.clone());
        robots
    }

    /// May we fetch this URL? (Also used by the UI to explain a refusal.)
    pub async fn allowed(&self, url: &str) -> Result<bool, CrawlError> {
        let u = Url::parse(url).map_err(|e| CrawlError::BadUrl(e.to_string()))?;
        let path = match u.query() {
            Some(q) => format!("{}?{q}", u.path()),
            None => u.path().to_string(),
        };
        Ok(self.robots_for(&u).await.allowed(&self.user_agent, &path))
    }

    /// GET a URL as text, after the robots.txt check and the per-host delay.
    pub async fn get_text(&self, url: &str) -> Result<String, CrawlError> {
        Ok(decode(&self.get_bytes(url).await?))
    }

    /// GET a URL as bytes (a picture), with the same checks as [`Self::get_text`].
    pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, CrawlError> {
        let u = Url::parse(url).map_err(|e| CrawlError::BadUrl(e.to_string()))?;
        if !matches!(u.scheme(), "http" | "https") {
            return Err(CrawlError::BadUrl(format!("只支援 http / https：{url}")));
        }
        if !self.allowed(url).await? {
            return Err(CrawlError::Disallowed(url.to_string()));
        }
        let robots = self.robots_for(&u).await;
        let delay = robots
            .delay_for(&self.user_agent)
            .map(|s| Duration::from_secs_f64(s.clamp(0.0, 30.0)))
            .unwrap_or_default()
            .max(self.min_delay);
        self.wait_turn(&Self::origin(&u), delay).await;

        let resp = self.http.get(u).send().await.map_err(|e| CrawlError::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(CrawlError::Status(resp.status().as_u16()));
        }
        if resp.content_length().is_some_and(|n| n as usize > MAX_BYTES) {
            return Err(CrawlError::TooLarge);
        }
        let bytes = resp.bytes().await.map_err(|e| CrawlError::Http(e.to_string()))?;
        if bytes.len() > MAX_BYTES {
            return Err(CrawlError::TooLarge);
        }
        Ok(bytes.to_vec())
    }

    async fn wait_turn(&self, origin: &str, delay: Duration) {
        loop {
            let wait = {
                let mut last = self.last.lock().await;
                let now = Instant::now();
                match last.get(origin) {
                    Some(t) if now.duration_since(*t) < delay => delay - now.duration_since(*t),
                    _ => {
                        last.insert(origin.to_string(), now);
                        return;
                    }
                }
            };
            tokio::time::sleep(wait).await;
        }
    }
}

/// Bytes to text: UTF-8, or Shift_JIS / EUC-JP declared by older Japanese
/// sites would need a decoder; we keep UTF-8 (lossy) for now.
fn decode(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
