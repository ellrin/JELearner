//! Gmail and Google Calendar behind traits.
//!
//! - [`GmailApi`] / [`GoogleCalendarApi`]: the real REST shapes
//!   (`users.messages.send`, `events.insert` / `events.list`) against a
//!   configurable base URL, with a [`TokenSource`] for the OAuth access token.
//!   OAuth itself comes later (docs/roadmap.md); the tests run them against a
//!   mock server.
//! - [`MemoryMailer`] / [`MemoryCalendar`]: keep everything in memory and show
//!   it in the UI ("模擬寄件匣"), used until Google is connected.

use async_trait::async_trait;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Email {
    pub to: String,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub summary: String,
    #[serde(default)]
    pub description: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// Popup reminder this many minutes before.
    #[serde(default)]
    pub reminder_minutes: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SentEmail {
    pub id: String,
    pub at: DateTime<Utc>,
    pub email: Email,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredEvent {
    pub id: String,
    pub event: CalendarEvent,
}

#[derive(Debug, thiserror::Error)]
pub enum GoogleError {
    #[error("Google 尚未連線：{0}")]
    NotConnected(String),
    #[error("無法連線到 Google：{0}")]
    Network(String),
    #[error("Google 回傳 {0}：{1}")]
    Api(u16, String),
    #[error("{0}")]
    Invalid(String),
}

#[async_trait]
pub trait Mailer: Send + Sync {
    fn mode(&self) -> &'static str;
    async fn send(&self, email: &Email) -> Result<String, GoogleError>;
}

#[async_trait]
pub trait Calendar: Send + Sync {
    fn mode(&self) -> &'static str;
    async fn create_event(&self, ev: &CalendarEvent) -> Result<String, GoogleError>;
    async fn list_events(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<StoredEvent>, GoogleError>;
}

#[async_trait]
pub trait TokenSource: Send + Sync {
    async fn access_token(&self) -> Result<String, GoogleError>;
}

/// A fixed token (tests, or a token pasted by hand during development).
pub struct StaticToken(pub String);

#[async_trait]
impl TokenSource for StaticToken {
    async fn access_token(&self) -> Result<String, GoogleError> {
        if self.0.trim().is_empty() {
            return Err(GoogleError::NotConnected("沒有 access token".into()));
        }
        Ok(self.0.clone())
    }
}

/// RFC 2822 message with UTF-8 subject and base64 body, as Gmail's `raw`.
pub fn rfc2822(email: &Email) -> Result<String, GoogleError> {
    let b64 = base64::engine::general_purpose::STANDARD;
    if email.to.contains(['\r', '\n']) || !email.to.contains('@') {
        return Err(GoogleError::Invalid(format!("收件人格式不正確：{}", email.to)));
    }
    let subject = format!("=?UTF-8?B?{}?=", b64.encode(email.subject.replace(['\r', '\n'], " ")));
    let body = b64.encode(email.body.as_bytes());
    // Wrap base64 at 76 characters, as MIME requires.
    let body = body.as_bytes().chunks(76).map(|c| std::str::from_utf8(c).unwrap_or("")).collect::<Vec<_>>().join("\r\n");
    Ok(format!(
        "To: {}\r\nSubject: {subject}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{body}\r\n",
        email.to.trim()
    ))
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(30)).build().expect("http client")
}

async fn check(resp: reqwest::Response) -> Result<Value, GoogleError> {
    let status = resp.status();
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let msg = v["error"]["message"].as_str().unwrap_or("").to_string();
        return Err(GoogleError::Api(status.as_u16(), msg));
    }
    Ok(v)
}

pub struct GmailApi {
    http: reqwest::Client,
    base: String,
    token: Arc<dyn TokenSource>,
}

impl GmailApi {
    /// `base` is `https://gmail.googleapis.com` in production.
    pub fn new(base: String, token: Arc<dyn TokenSource>) -> GmailApi {
        GmailApi { http: client(), base, token }
    }
}

#[async_trait]
impl Mailer for GmailApi {
    fn mode(&self) -> &'static str {
        "gmail"
    }

    async fn send(&self, email: &Email) -> Result<String, GoogleError> {
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rfc2822(email)?);
        let token = self.token.access_token().await?;
        let resp = self
            .http
            .post(format!("{}/gmail/v1/users/me/messages/send", self.base.trim_end_matches('/')))
            .bearer_auth(token)
            .json(&json!({ "raw": raw }))
            .send()
            .await
            .map_err(|e| GoogleError::Network(e.to_string()))?;
        let v = check(resp).await?;
        Ok(v["id"].as_str().unwrap_or("").to_string())
    }
}

pub struct GoogleCalendarApi {
    http: reqwest::Client,
    base: String,
    calendar_id: String,
    time_zone: String,
    token: Arc<dyn TokenSource>,
}

impl GoogleCalendarApi {
    /// `base` is `https://www.googleapis.com` in production.
    pub fn new(base: String, calendar_id: String, time_zone: String, token: Arc<dyn TokenSource>) -> GoogleCalendarApi {
        GoogleCalendarApi { http: client(), base, calendar_id, time_zone, token }
    }

    fn events_url(&self) -> String {
        let id: String = url::form_urlencoded::byte_serialize(self.calendar_id.as_bytes()).collect();
        format!("{}/calendar/v3/calendars/{id}/events", self.base.trim_end_matches('/'))
    }
}

#[async_trait]
impl Calendar for GoogleCalendarApi {
    fn mode(&self) -> &'static str {
        "google"
    }

    async fn create_event(&self, ev: &CalendarEvent) -> Result<String, GoogleError> {
        let mut body = json!({
            "summary": ev.summary,
            "description": ev.description,
            "start": { "dateTime": ev.start.to_rfc3339(), "timeZone": self.time_zone },
            "end": { "dateTime": ev.end.to_rfc3339(), "timeZone": self.time_zone },
        });
        if let Some(m) = ev.reminder_minutes {
            body["reminders"] = json!({ "useDefault": false, "overrides": [{ "method": "popup", "minutes": m }] });
        }
        let token = self.token.access_token().await?;
        let resp = self
            .http
            .post(self.events_url())
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|e| GoogleError::Network(e.to_string()))?;
        let v = check(resp).await?;
        Ok(v["id"].as_str().unwrap_or("").to_string())
    }

    async fn list_events(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<StoredEvent>, GoogleError> {
        let token = self.token.access_token().await?;
        let resp = self
            .http
            .get(self.events_url())
            .bearer_auth(token)
            .query(&[
                ("timeMin", from.to_rfc3339()),
                ("timeMax", to.to_rfc3339()),
                ("singleEvents", "true".into()),
                ("orderBy", "startTime".into()),
            ])
            .send()
            .await
            .map_err(|e| GoogleError::Network(e.to_string()))?;
        let v = check(resp).await?;
        let parse_time = |t: &Value| {
            t["dateTime"].as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.with_timezone(&Utc))
        };
        Ok(v["items"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|i| {
                        Some(StoredEvent {
                            id: i["id"].as_str()?.to_string(),
                            event: CalendarEvent {
                                summary: i["summary"].as_str().unwrap_or("").to_string(),
                                description: i["description"].as_str().unwrap_or("").to_string(),
                                start: parse_time(&i["start"])?,
                                end: parse_time(&i["end"])?,
                                reminder_minutes: None,
                            },
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}

/// Keeps sent mail in memory; the UI shows it as the simulated outbox.
#[derive(Default)]
pub struct MemoryMailer {
    pub sent: Mutex<Vec<SentEmail>>,
}

#[async_trait]
impl Mailer for MemoryMailer {
    fn mode(&self) -> &'static str {
        "mock"
    }

    async fn send(&self, email: &Email) -> Result<String, GoogleError> {
        rfc2822(email)?; // same validation as the real one
        let mut sent = self.sent.lock().unwrap();
        let id = format!("mock-mail-{}", sent.len() + 1);
        sent.push(SentEmail { id: id.clone(), at: Utc::now(), email: email.clone() });
        Ok(id)
    }
}

#[derive(Default)]
pub struct MemoryCalendar {
    pub events: Mutex<Vec<StoredEvent>>,
}

#[async_trait]
impl Calendar for MemoryCalendar {
    fn mode(&self) -> &'static str {
        "mock"
    }

    async fn create_event(&self, ev: &CalendarEvent) -> Result<String, GoogleError> {
        if ev.end <= ev.start {
            return Err(GoogleError::Invalid("結束時間必須晚於開始時間".into()));
        }
        let mut events = self.events.lock().unwrap();
        let id = format!("mock-event-{}", events.len() + 1);
        events.push(StoredEvent { id: id.clone(), event: ev.clone() });
        Ok(id)
    }

    async fn list_events(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<StoredEvent>, GoogleError> {
        let mut v: Vec<StoredEvent> =
            self.events.lock().unwrap().iter().filter(|e| e.event.start < to && e.event.end > from).cloned().collect();
        v.sort_by_key(|e| e.event.start);
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::Query,
        http::HeaderMap,
        routing::{get, post},
        Json, Router,
    };
    use chrono::Duration as D;
    use std::collections::HashMap;

    async fn serve(app: Router) -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        format!("http://{addr}")
    }

    fn mail() -> Email {
        Email { to: "me@example.com".into(), subject: "本週學習週報".into(), body: "新單字 12 個".into() }
    }

    #[test]
    fn rfc2822_encodes_utf8_and_rejects_header_injection() {
        let raw = rfc2822(&mail()).unwrap();
        assert!(raw.contains("Subject: =?UTF-8?B?"));
        assert!(raw.contains("charset=UTF-8"));
        let bad = Email { to: "a@b.c\r\nBcc: x@y.z".into(), ..mail() };
        assert!(rfc2822(&bad).is_err());
    }

    #[tokio::test]
    async fn gmail_sends_raw_message_with_bearer_token() {
        let app = Router::new().route(
            "/gmail/v1/users/me/messages/send",
            post(|h: HeaderMap, Json(b): Json<Value>| async move {
                assert_eq!(h["authorization"], "Bearer tok");
                let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(b["raw"].as_str().unwrap()).unwrap();
                assert!(String::from_utf8(raw).unwrap().starts_with("To: me@example.com"));
                Json(json!({"id": "m1"}))
            }),
        );
        let base = serve(app).await;
        let g = GmailApi::new(base, Arc::new(StaticToken("tok".into())));
        assert_eq!(g.send(&mail()).await.unwrap(), "m1");
        let none = GmailApi::new("http://127.0.0.1:9".into(), Arc::new(StaticToken(String::new())));
        assert!(matches!(none.send(&mail()).await, Err(GoogleError::NotConnected(_))));
    }

    #[tokio::test]
    async fn calendar_insert_and_list() {
        let store: Arc<Mutex<Vec<Value>>> = Arc::default();
        let s1 = store.clone();
        let s2 = store.clone();
        let app = Router::new().route(
            "/calendar/v3/calendars/primary/events",
            post(move |Json(b): Json<Value>| {
                let s = s1.clone();
                async move {
                    let mut v = s.lock().unwrap();
                    let mut b = b;
                    b["id"] = json!(format!("e{}", v.len() + 1));
                    v.push(b.clone());
                    Json(b)
                }
            })
            .merge(get(move |Query(q): Query<HashMap<String, String>>| {
                let s = s2.clone();
                async move {
                    assert!(q.contains_key("timeMin"));
                    Json(json!({"items": s.lock().unwrap().clone()}))
                }
            })),
        );
        let base = serve(app).await;
        let cal = GoogleCalendarApi::new(base, "primary".into(), "Asia/Taipei".into(), Arc::new(StaticToken("t".into())));
        let start = Utc::now();
        let ev = CalendarEvent { summary: "日文課".into(), description: "".into(), start, end: start + D::minutes(30), reminder_minutes: Some(10) };
        assert_eq!(cal.create_event(&ev).await.unwrap(), "e1");
        let listed = cal.list_events(start - D::days(1), start + D::days(1)).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].event.summary, "日文課");
        assert_eq!(store.lock().unwrap()[0]["reminders"]["overrides"][0]["minutes"], 10);
    }

    #[tokio::test]
    async fn memory_implementations() {
        let m = MemoryMailer::default();
        m.send(&mail()).await.unwrap();
        assert_eq!(m.sent.lock().unwrap().len(), 1);
        let c = MemoryCalendar::default();
        let t = Utc::now();
        assert!(c.create_event(&CalendarEvent { summary: "x".into(), description: "".into(), start: t, end: t, reminder_minutes: None }).await.is_err());
    }
}
