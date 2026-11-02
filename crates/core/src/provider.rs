//! Language-model providers behind one trait, so lesson generation (and later
//! conversation and correction) does not care which engine runs it.
//!
//! - [`ClaudeProvider`]: Anthropic Messages API (the engine until the Mac
//!   arrives).
//! - [`LlamaServerProvider`]: llama.cpp `llama-server` (OpenAI-compatible
//!   `/v1/chat/completions`), for Gemma 4 on the Mac. Tested against a mock
//!   server only; **not yet tested on real hardware**.
//! - [`MockProvider`]: canned replies for offline tests.

use async_trait::async_trait;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct CompletionRequest {
    pub system: String,
    pub user: String,
    pub max_tokens: u32,
    /// When set, the reply must be one JSON object matching this schema.
    pub json_schema: Option<Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("尚未設定：{0}")]
    NotConfigured(String),
    #[error("無法連線到 {0}：{1}")]
    Network(String, String),
    #[error("{0} 回傳 {1}：{2}")]
    Api(String, u16, String),
    #[error("{0} 拒絕了這個請求（{1}）")]
    Refused(String, String),
    #[error("{0} 的回覆被截斷（max_tokens）")]
    Truncated(String),
    #[error("{0} 的回覆無法解析：{1}")]
    Parse(String, String),
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Short engine id shown in the UI and stored with each lesson, e.g.
    /// `claude:claude-opus-5-5`.
    fn id(&self) -> String;
    async fn complete(&self, req: &CompletionRequest) -> Result<String, ProviderError>;
}

// ── Claude ───────────────────────────────────────────────

pub const CLAUDE_API_VERSION: &str = "2026-10-21";
pub const CLAUDE_FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

#[derive(Debug, Clone)]
pub struct ClaudeConfig {
    pub base_url: String,
    pub model: String,
    /// `low` | `medium` | `high` | `xhigh` | `max`; `None` = model default.
    pub effort: Option<String>,
    /// Ask the API to re-run a declined request on Anthropic's recommended
    /// fallback model instead of returning the refusal.
    pub fallbacks: bool,
}

pub struct ClaudeProvider {
    http: reqwest::Client,
    cfg: ClaudeConfig,
    api_key: String,
}

impl ClaudeProvider {
    pub fn new(cfg: ClaudeConfig, api_key: String) -> ClaudeProvider {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(600)).build().expect("http client");
        ClaudeProvider { http, cfg, api_key }
    }

    /// The request body; public so tests can check its shape.
    pub fn body(&self, req: &CompletionRequest) -> Value {
        let mut body = json!({
            "model": self.cfg.model,
            "max_tokens": req.max_tokens,
            "system": req.system,
            "messages": [{ "role": "user", "content": req.user }],
        });
        let mut output = serde_json::Map::new();
        if let Some(e) = &self.cfg.effort {
            output.insert("effort".into(), json!(e));
        }
        if let Some(schema) = &req.json_schema {
            output.insert("format".into(), json!({ "type": "json_schema", "schema": schema }));
        }
        if !output.is_empty() {
            body["output_config"] = Value::Object(output);
        }
        if self.cfg.fallbacks {
            body["fallbacks"] = json!("default");
        }
        body
    }
}

#[async_trait]
impl LlmProvider for ClaudeProvider {
    fn id(&self) -> String {
        format!("claude:{}", self.cfg.model)
    }

    async fn complete(&self, req: &CompletionRequest) -> Result<String, ProviderError> {
        let name = "Claude".to_string();
        let mut rb = self
            .http
            .post(format!("{}/v1/messages", self.cfg.base_url.trim_end_matches('/')))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", CLAUDE_API_VERSION)
            .json(&self.body(req));
        if self.cfg.fallbacks {
            rb = rb.header("anthropic-beta", CLAUDE_FALLBACK_BETA);
        }
        let resp = rb.send().await.map_err(|e| ProviderError::Network(name.clone(), e.to_string()))?;
        let status = resp.status();
        let v: Value = resp.json().await.map_err(|e| ProviderError::Parse(name.clone(), e.to_string()))?;
        if !status.is_success() {
            let msg = v["error"]["message"].as_str().unwrap_or("").to_string();
            return Err(ProviderError::Api(name, status.as_u16(), msg));
        }
        // Check why it stopped before reading the content.
        match v["stop_reason"].as_str() {
            Some("refusal") => {
                let why = v["stop_details"]["explanation"]
                    .as_str()
                    .or(v["stop_details"]["category"].as_str())
                    .unwrap_or("未說明原因")
                    .to_string();
                return Err(ProviderError::Refused(name, why));
            }
            Some("max_tokens") => return Err(ProviderError::Truncated(name)),
            _ => {}
        }
        let text: String = v["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        if text.trim().is_empty() {
            return Err(ProviderError::Parse(name, "沒有文字內容".into()));
        }
        Ok(text)
    }
}

// ── llama-server (OpenAI-compatible) ─────────────────────

#[derive(Debug, Clone)]
pub struct LlamaServerConfig {
    /// Base URL ending in `/v1`, e.g. `http://127.0.0.1:8080/v1`.
    pub url: String,
    pub model: String,
}

pub struct LlamaServerProvider {
    http: reqwest::Client,
    cfg: LlamaServerConfig,
}

impl LlamaServerProvider {
    pub fn new(cfg: LlamaServerConfig) -> LlamaServerProvider {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(600)).build().expect("http client");
        LlamaServerProvider { http, cfg }
    }
}

#[async_trait]
impl LlmProvider for LlamaServerProvider {
    fn id(&self) -> String {
        format!("llama-server:{}", self.cfg.model)
    }

    async fn complete(&self, req: &CompletionRequest) -> Result<String, ProviderError> {
        let name = "llama-server".to_string();
        let mut body = json!({
            "model": self.cfg.model,
            "max_tokens": req.max_tokens,
            "messages": [
                { "role": "system", "content": req.system },
                { "role": "user", "content": req.user },
            ],
        });
        if let Some(schema) = &req.json_schema {
            body["response_format"] = json!({ "type": "json_schema", "json_schema": { "name": "reply", "schema": schema } });
        }
        let resp = self
            .http
            .post(format!("{}/chat/completions", self.cfg.url.trim_end_matches('/')))
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Network(name.clone(), e.to_string()))?;
        let status = resp.status();
        let v: Value = resp.json().await.map_err(|e| ProviderError::Parse(name.clone(), e.to_string()))?;
        if !status.is_success() {
            let msg = v["error"]["message"].as_str().unwrap_or("").to_string();
            return Err(ProviderError::Api(name, status.as_u16(), msg));
        }
        if v["choices"][0]["finish_reason"] == "length" {
            return Err(ProviderError::Truncated(name));
        }
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| ProviderError::Parse(name, "沒有 choices[0].message.content".into()))
    }
}

// ── Mock ─────────────────────────────────────────────────

/// Replies with queued answers (or the last one forever) and records requests.
pub struct MockProvider {
    replies: Mutex<Vec<String>>,
    pub seen: Mutex<Vec<CompletionRequest>>,
}

impl MockProvider {
    pub fn new(replies: Vec<String>) -> MockProvider {
        MockProvider { replies: Mutex::new(replies), seen: Mutex::new(vec![]) }
    }
}

#[async_trait]
impl LlmProvider for MockProvider {
    fn id(&self) -> String {
        "mock".into()
    }

    async fn complete(&self, req: &CompletionRequest) -> Result<String, ProviderError> {
        self.seen.lock().unwrap().push(req.clone());
        let mut r = self.replies.lock().unwrap();
        match r.len() {
            0 => Err(ProviderError::NotConfigured("mock 沒有回覆".into())),
            1 => Ok(r[0].clone()),
            _ => Ok(r.remove(0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Json, Router};

    async fn serve(app: Router) -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        format!("http://{addr}")
    }

    fn req() -> CompletionRequest {
        CompletionRequest { system: "s".into(), user: "u".into(), max_tokens: 100, json_schema: Some(json!({"type":"object"})) }
    }

    fn claude(base: String) -> ClaudeProvider {
        ClaudeProvider::new(
            ClaudeConfig { base_url: base, model: "claude-opus-5-5".into(), effort: Some("medium".into()), fallbacks: true },
            "test-key".into(),
        )
    }

    #[test]
    fn claude_body_has_no_sampling_params() {
        let b = claude("http://x".into()).body(&req());
        assert_eq!(b["output_config"]["format"]["type"], "json_schema");
        assert_eq!(b["output_config"]["effort"], "medium");
        assert_eq!(b["fallbacks"], "default");
        assert!(b.get("temperature").is_none(), "sampling params are rejected by current models");
    }

    #[tokio::test]
    async fn claude_reads_text_and_reports_refusal() {
        let app = Router::new().route(
            "/v1/messages",
            post(|headers: axum::http::HeaderMap, Json(b): Json<Value>| async move {
                assert_eq!(headers["x-api-key"], "test-key");
                assert_eq!(headers["anthropic-version"], CLAUDE_API_VERSION);
                if b["system"] == "refuse" {
                    return Json(json!({"content": [], "stop_reason": "refusal", "stop_details": {"type":"refusal","category":"cyber","explanation":"nope"}}));
                }
                Json(json!({"content": [{"type":"thinking","thinking":""},{"type":"text","text":"{\"ok\":"},{"type":"text","text":"true}"}], "stop_reason": "end_turn"}))
            }),
        );
        let base = serve(app).await;
        let p = claude(base);
        assert_eq!(p.complete(&req()).await.unwrap(), "{\"ok\":true}");
        let mut r = req();
        r.system = "refuse".into();
        assert!(matches!(p.complete(&r).await, Err(ProviderError::Refused(_, w)) if w == "nope"));
    }

    #[tokio::test]
    async fn llama_server_uses_chat_completions() {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|Json(b): Json<Value>| async move {
                assert_eq!(b["response_format"]["type"], "json_schema");
                assert_eq!(b["messages"][0]["role"], "system");
                Json(json!({"choices": [{"message": {"content": "hi"}, "finish_reason": "stop"}]}))
            }),
        );
        let base = serve(app).await;
        let p = LlamaServerProvider::new(LlamaServerConfig { url: format!("{base}/v1"), model: "gemma-4-12b".into() });
        assert_eq!(p.complete(&req()).await.unwrap(), "hi");
    }
}
