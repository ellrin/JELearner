//! JELearner local server. `cargo run -p jl-server` serves the web UI and the
//! JSON API on 127.0.0.1 only. When the app is packaged with Tauri the same
//! handlers move behind Tauri commands (docs/roadmap.md).
//!
//! The private workspace is found in this order:
//! 1. `JELEARNER_WORKSPACE`
//! 2. `workspace_path` in `<config dir>/JELearner/config.toml`
//! 3. a demo copy of `workspace-template/` in `<config dir>/JELearner/demo-workspace`
//!
//! Its database lives at `<config dir>/JELearner/data/<workspace id>/learner.sqlite`,
//! so every user (and every workspace) has its own private database.

pub mod api;
pub mod git;
pub mod secrets;
pub mod settings;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use jl_core::crawler::Fetcher;
use jl_core::db::Db;
use jl_core::google::{Calendar, GmailApi, GoogleCalendarApi, Mailer, MemoryCalendar, MemoryMailer, StaticToken};
use jl_core::import::{self, ImportReport};
use jl_core::provider::{ClaudeProvider, LlamaServerProvider, LlmProvider};
use jl_core::workspace::Workspace;
pub use settings::{EngineKind, GoogleMode, Paths, Settings};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

pub struct Loaded {
    pub ws: Workspace,
    pub db: Db,
    pub demo: bool,
    pub db_path: PathBuf,
    pub import: ImportReport,
}

pub struct AppState {
    pub settings: RwLock<Settings>,
    pub loaded: RwLock<Option<Loaded>>,
    pub load_error: RwLock<Option<String>>,
    pub fetcher: Fetcher,
    pub mailer: Arc<dyn Mailer>,
    pub calendar: Arc<dyn Calendar>,
    /// Present in mock mode, for the simulated outbox in the UI.
    pub mock_mail: Option<Arc<MemoryMailer>>,
    pub mock_calendar: Option<Arc<MemoryCalendar>>,
    /// Tests may inject a provider; otherwise one is built per request from
    /// the current settings.
    pub provider_override: Option<Arc<dyn LlmProvider>>,
}

pub type Shared = Arc<AppState>;

#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

pub fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}
pub fn fail(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, msg.into())
}
pub fn unavailable(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::SERVICE_UNAVAILABLE, msg.into())
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        fail(format!("資料庫：{e}"))
    }
}

/// Database file for a workspace id.
pub fn db_path(settings: &Settings, id: &str) -> PathBuf {
    let base = settings.app_dir.clone().unwrap_or_else(std::env::temp_dir);
    base.join("data").join(id).join("learner.sqlite")
}

/// Open a workspace and its database, then read the workspace files in.
pub async fn load_workspace(settings: &Settings, root: Option<PathBuf>, init: bool) -> Result<Loaded, String> {
    let (root, demo) = match root.or_else(|| settings.workspace.clone()) {
        Some(r) => (r, false),
        None => {
            let base = settings.app_dir.clone().ok_or("系統沒有設定資料夾")?;
            (base.join("demo-workspace"), true)
        }
    };
    let ws = if init || demo {
        Workspace::init(&root, &settings.paths.template)
    } else {
        Workspace::open(&root)
    }
    .map_err(|e| e.to_string())?;
    let path = db_path(settings, &ws.meta.id);
    let db = Db::open(&path).await.map_err(|e| format!("無法開啟資料庫 {}：{e}", path.display()))?;
    let mut rep = import::sync_workspace(&ws, &db).await;
    match import::restore_records(&ws, &db).await {
        Ok(r) => rep.merge(r),
        Err(e) => rep.warnings.push(format!("records: {e}")),
    }
    Ok(Loaded { ws, db, demo, db_path: path, import: rep })
}

/// The engine for this request, from the current settings.
pub async fn provider(s: &Shared) -> Result<Arc<dyn LlmProvider>, ApiError> {
    if let Some(p) = &s.provider_override {
        return Ok(p.clone());
    }
    let st = s.settings.read().await;
    match st.engine {
        EngineKind::LlamaServer => Ok(Arc::new(LlamaServerProvider::new(st.llama.clone()))),
        EngineKind::Claude => {
            let (key, _) = secrets::key(st.claude_api_key.as_deref(), st.use_keychain, st.app_dir.as_deref())
                .ok_or_else(|| unavailable("Claude 尚未設定 API key：請到「設定 → Claude」貼上，或設定環境變數 ANTHROPIC_API_KEY"))?;
            Ok(Arc::new(ClaudeProvider::new(st.claude.clone(), key)))
        }
    }
}

pub struct Options {
    pub provider: Option<Arc<dyn LlmProvider>>,
}

pub async fn build_state(settings: Settings, opts: Options) -> Shared {
    let (mailer, calendar, mock_mail, mock_calendar): (Arc<dyn Mailer>, Arc<dyn Calendar>, _, _) = match settings.google.mode {
        GoogleMode::Mock => {
            let m = Arc::new(MemoryMailer::default());
            let c = Arc::new(MemoryCalendar::default());
            (m.clone(), c.clone(), Some(m), Some(c))
        }
        GoogleMode::Api => {
            let token = Arc::new(StaticToken(settings.google.token.clone().unwrap_or_default()));
            let tz = "UTC".to_string();
            (
                Arc::new(GmailApi::new(settings.google.gmail_base.clone(), token.clone())),
                Arc::new(GoogleCalendarApi::new(settings.google.calendar_base.clone(), settings.google.calendar_id.clone(), tz, token)),
                None,
                None,
            )
        }
    };
    let fetcher = Fetcher::new(settings.user_agent.clone(), Duration::from_millis(settings.crawler_delay_ms));
    let (loaded, err) = match load_workspace(&settings, None, false).await {
        Ok(l) => (Some(l), None),
        Err(e) => (None, Some(e)),
    };
    Arc::new(AppState {
        settings: RwLock::new(settings),
        loaded: RwLock::new(loaded),
        load_error: RwLock::new(err),
        fetcher,
        mailer,
        calendar,
        mock_mail,
        mock_calendar,
        provider_override: opts.provider,
    })
}

pub async fn router(settings: Settings, opts: Options) -> axum::Router {
    let web = settings.paths.web.clone();
    let state = build_state(settings, opts).await;
    api::routes(web).with_state(state)
}

/// Bind on 127.0.0.1 (port 0 = any free port) and return the address and a
/// future that serves until dropped.
pub async fn bind(
    settings: Settings,
    port: u16,
) -> std::io::Result<(SocketAddr, impl std::future::Future<Output = std::io::Result<()>>)> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    let addr = listener.local_addr()?;
    let app = router(settings, Options { provider: None }).await;
    Ok((addr, async move { axum::serve(listener, app).await }))
}
