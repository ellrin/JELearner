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
pub mod courses;
pub mod git;
pub mod notebook;
pub mod secrets;
pub mod settings;
pub mod study;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use jl_core::crawler::Fetcher;
use jl_core::db::Db;
use jl_core::google::{Calendar, GmailApi, GoogleCalendarApi, Mailer, MemoryCalendar, MemoryMailer, StaticToken};
use jl_core::import::{self, ImportReport};
use jl_core::claude_code::{self, ClaudeCodeProvider};
use jl_core::provider::{ClaudeProvider, LlamaServerProvider, LlmProvider};
use jl_core::workspace::{Layout, MemberKind, Profile, Root, Workspace};
pub use settings::{EngineKind, GoogleMode, Paths, Settings};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// The open workspace, before or after a user is chosen.
pub struct OpenRoot {
    pub root: Root,
    pub demo: bool,
}

/// A user inside the open workspace: their files and their database.
pub struct Loaded {
    pub ws: Workspace,
    pub db: Db,
    pub demo: bool,
    pub db_path: PathBuf,
    pub import: ImportReport,
}

pub struct AppState {
    pub settings: RwLock<Settings>,
    pub root: RwLock<Option<OpenRoot>>,
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
    /// Claude Code CLI status (installed / signed in), refreshed at most once
    /// a minute.
    pub cc: claude_code::Cache,
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

/// Database file for a workspace and user.
pub fn db_path(settings: &Settings, ws_id: &str, user: &str) -> PathBuf {
    let base = settings.app_dir.clone().unwrap_or_else(std::env::temp_dir);
    base.join("data").join(ws_id).join(user).join("learner.sqlite")
}

/// This user's own Claude Code sign-in folder (`CLAUDE_CONFIG_DIR`).
pub fn claude_dir(settings: &Settings, ws_id: &str, user: &str) -> PathBuf {
    let base = settings.app_dir.clone().unwrap_or_else(std::env::temp_dir);
    base.join("claude").join(ws_id).join(user)
}

pub const DEMO_USER: &str = "demo";

/// Open (or with `init`, create or complete) a workspace. Without a path,
/// the configured workspace or the demo workspace.
pub fn open_workspace(settings: &Settings, path: Option<PathBuf>, init: bool) -> Result<OpenRoot, String> {
    let (path, demo) = match path.or_else(|| settings.workspace.clone()) {
        Some(r) => (r, false),
        None => {
            let base = settings.app_dir.clone().ok_or("系統沒有設定資料夾")?;
            (base.join("demo-workspace"), true)
        }
    };
    let template = &settings.paths.template;
    let mut root = if init || demo { Root::init(&path, template) } else { Root::open(&path) }.map_err(|e| e.to_string())?;
    if demo && root.list_profiles().is_empty() && root.layout() == Layout::V2 {
        let mut p = Profile::new(DEMO_USER, "示範使用者");
        p.languages = vec![jl_core::Lang::En, jl_core::Lang::Ja];
        root.create_profile(template, p).map_err(|e| e.to_string())?;
    }
    Ok(OpenRoot { root, demo })
}

/// Enter as a user: open their database and read their files in.
pub async fn enter_user(settings: &Settings, open: &OpenRoot, id: &str) -> Result<Loaded, String> {
    let ws = open.root.user(id).map_err(|e| e.to_string())?;
    let path = db_path(settings, &ws.meta.id, id);
    // First version: one database per workspace. The owner keeps it.
    if !path.exists() && is_owner(&open.root, id) {
        let base = settings.app_dir.clone().unwrap_or_else(std::env::temp_dir);
        let legacy = base.join("data").join(&ws.meta.id).join("learner.sqlite");
        if legacy.exists() {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            for ext in ["", "-wal", "-shm"] {
                let from = PathBuf::from(format!("{}{ext}", legacy.display()));
                if from.exists() {
                    std::fs::rename(&from, format!("{}{ext}", path.display())).map_err(|e| e.to_string())?;
                }
            }
        }
    }
    let db = Db::open(&path).await.map_err(|e| format!("無法開啟資料庫 {}：{e}", path.display()))?;
    let mut rep = import::sync_workspace(&ws, &db).await;
    match import::restore_records(&ws, &db).await {
        Ok(r) => rep.merge(r),
        Err(e) => rep.warnings.push(format!("records: {e}")),
    }
    Ok(Loaded { ws, db, demo: open.demo, db_path: path, import: rep })
}

/// The workspace owner (or first admin) may inherit first-version settings
/// such as the single API key and database.
pub fn is_owner(root: &Root, id: &str) -> bool {
    root.meta.members.iter().any(|m| m.id == id && m.kind == MemberKind::Owner) || root.meta.admins.first().is_some_and(|a| a == id)
}

/// The user to enter automatically: the one this computer used last in this
/// workspace, or the only user there is.
pub fn default_user(settings: &Settings, root: &Root) -> Option<String> {
    let users = root.list_profiles();
    settings
        .active_users
        .get(&root.meta.id)
        .filter(|id| users.iter().any(|p| &p.id == *id))
        .cloned()
        .or_else(|| (users.len() == 1).then(|| users[0].id.clone()))
}

/// Whose Claude: the chosen user's key and sign-in folder.
pub async fn claude_identity(s: &Shared) -> (Option<secrets::Who>, claude_code::Cli) {
    let st = s.settings.read().await;
    let bin = st.claude_bin.clone();
    let root = s.root.read().await;
    let loaded = s.loaded.read().await;
    match (root.as_ref(), loaded.as_ref()) {
        (Some(r), Some(l)) => {
            let ws_id = l.ws.meta.id.clone();
            let user = l.ws.profile.id.clone();
            let cli = claude_code::Cli { bin, config_dir: Some(claude_dir(&st, &ws_id, &user)) };
            let legacy = is_owner(&r.root, &user);
            (Some(secrets::Who { workspace: ws_id, user, legacy }), cli)
        }
        // No user chosen: nobody's sign-in may be used.
        _ => (None, claude_code::Cli { bin, config_dir: st.app_dir.as_ref().map(|d| d.join("claude").join("_nobody")) }),
    }
}

pub async fn api_key(s: &Shared) -> Option<(String, secrets::KeySource)> {
    let (who, _) = claude_identity(s).await;
    let st = s.settings.read().await;
    secrets::key(st.claude_api_key.as_deref(), st.use_keychain, st.app_dir.as_deref(), who.as_ref())
}

/// The engine a request will actually use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Resolved {
    Subscription,
    Api,
    LlamaServer,
}

impl Resolved {
    pub fn label(self) -> &'static str {
        match self {
            Resolved::Subscription => "Claude 訂閱",
            Resolved::Api => "Claude API",
            Resolved::LlamaServer => "本機模型",
        }
    }
}

const NO_CLAUDE: &str = "Claude 還沒連上：到「設定 → Claude」用瀏覽器登入你的 Claude 帳號（訂閱），或貼上 API key";

/// Pick the engine from the settings: `auto` prefers the subscription when
/// Claude Code is signed in, then an API key.
pub async fn resolve_engine(s: &Shared, fresh: bool) -> Result<Resolved, String> {
    let kind = s.settings.read().await.engine;
    let has_key = api_key(s).await.is_some();
    let (who, cli) = claude_identity(s).await;
    if who.is_none() && kind != EngineKind::LlamaServer && s.settings.read().await.claude_api_key.is_none() && !has_key {
        return Err("請先選擇使用者".into());
    }
    let sub_ready = || async { who.is_some() && s.cc.get(&cli, fresh).await.ready };
    match kind {
        EngineKind::LlamaServer => Ok(Resolved::LlamaServer),
        EngineKind::Api if has_key => Ok(Resolved::Api),
        EngineKind::Api => Err("Claude API 尚未設定 key：請到「設定 → Claude」貼上，或設定環境變數 ANTHROPIC_API_KEY".into()),
        EngineKind::Subscription if sub_ready().await => Ok(Resolved::Subscription),
        EngineKind::Subscription => Err("Claude Code 沒有安裝或還沒登入：請到「設定 → Claude」用瀏覽器登入".into()),
        EngineKind::Auto if sub_ready().await => Ok(Resolved::Subscription),
        EngineKind::Auto if has_key => Ok(Resolved::Api),
        EngineKind::Auto => Err(NO_CLAUDE.into()),
    }
}

/// The engine for this request, from the current settings.
pub async fn provider(s: &Shared) -> Result<Arc<dyn LlmProvider>, ApiError> {
    if let Some(p) = &s.provider_override {
        return Ok(p.clone());
    }
    let which = resolve_engine(s, false).await.map_err(unavailable)?;
    provider_for(s, which).await
}

/// A provider for one specific engine, as the chosen user.
pub async fn provider_for(s: &Shared, which: Resolved) -> Result<Arc<dyn LlmProvider>, ApiError> {
    let (_, cli) = claude_identity(s).await;
    let key = api_key(s).await;
    let st = s.settings.read().await;
    Ok(match which {
        Resolved::LlamaServer => Arc::new(LlamaServerProvider::new(st.llama.clone())),
        Resolved::Subscription => Arc::new(ClaudeCodeProvider::new(cli, st.claude.model.clone(), st.claude.effort.clone())),
        Resolved::Api => {
            let (key, _) = key.ok_or_else(|| unavailable(NO_CLAUDE))?;
            Arc::new(ClaudeProvider::new(st.claude.clone(), key))
        }
    })
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
    let (root, loaded, err) = match open_workspace(&settings, None, false) {
        Ok(open) => {
            let mut loaded = None;
            let mut err = None;
            if open.root.layout() == Layout::V2
                && let Some(id) = default_user(&settings, &open.root)
            {
                match enter_user(&settings, &open, &id).await {
                    Ok(l) => loaded = Some(l),
                    Err(e) => err = Some(e),
                }
            }
            (Some(open), loaded, err)
        }
        Err(e) => (None, None, Some(e)),
    };
    Arc::new(AppState {
        settings: RwLock::new(settings),
        root: RwLock::new(root),
        loaded: RwLock::new(loaded),
        load_error: RwLock::new(err),
        fetcher,
        mailer,
        calendar,
        mock_mail,
        mock_calendar,
        provider_override: opts.provider,
        cc: Default::default(),
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
