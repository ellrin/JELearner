//! App settings: where things are, which engine to use, how to reach Google.
//!
//! Read from `<config dir>/JELearner/config.toml` (outside every repository;
//! macOS: `~/Library/Application Support/JELearner/`), with environment
//! variables taking precedence. Tests build a [`Settings`] directly.

use jl_core::catalog::Catalog;
use jl_core::provider::{ClaudeConfig, LlamaServerConfig};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Bundled files: the web UI and the workspace template.
#[derive(Debug, Clone)]
pub struct Paths {
    pub web: PathBuf,
    pub template: PathBuf,
}

impl Paths {
    pub fn from_repo() -> Paths {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = root.canonicalize().unwrap_or(root);
        Paths { web: root.join("web"), template: root.join("workspace-template") }
    }
}

/// Which engine generates lessons. Same choices as PaperComposer, plus the
/// local model for the Mac.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    /// Claude subscription when Claude Code is signed in, else the API key.
    Auto,
    /// The user's own Claude subscription through the Claude Code CLI.
    Subscription,
    /// Anthropic API key (usage billing).
    Api,
    /// llama-server on this machine (the Mac).
    LlamaServer,
}

impl EngineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EngineKind::Auto => "auto",
            EngineKind::Subscription => "subscription",
            EngineKind::Api => "api",
            EngineKind::LlamaServer => "llama-server",
        }
    }

    pub fn parse(s: &str) -> Option<EngineKind> {
        match s.trim() {
            "auto" | "" => Some(EngineKind::Auto),
            "subscription" | "claude-code" => Some(EngineKind::Subscription),
            "api" | "claude" | "claude-api" => Some(EngineKind::Api),
            "llama-server" | "llama" | "local" => Some(EngineKind::LlamaServer),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoogleMode {
    /// In-memory outbox and calendar shown in the UI.
    Mock,
    /// Real REST endpoints (or a mock server in tests) with a bearer token.
    Api,
}

#[derive(Debug, Clone)]
pub struct GoogleSettings {
    pub mode: GoogleMode,
    pub gmail_base: String,
    pub calendar_base: String,
    pub calendar_id: String,
    /// OAuth access token. Until OAuth is built, only for development.
    pub token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub paths: Paths,
    /// `<config dir>/JELearner`: config.toml, credentials, databases, demo
    /// workspace. `None` only when the OS has no config folder.
    pub app_dir: Option<PathBuf>,
    /// The private workspace, when chosen.
    pub workspace: Option<PathBuf>,
    pub engine: EngineKind,
    pub claude: ClaudeConfig,
    /// Set by tests; otherwise the key is looked up (env, keychain, file).
    pub claude_api_key: Option<String>,
    pub use_keychain: bool,
    /// Path of the Claude Code CLI when it is not on the usual paths
    /// (`JELEARNER_CLAUDE_BIN` or `[claude] bin`); tests point it at a fake.
    pub claude_bin: Option<PathBuf>,
    /// The user this computer last entered, per workspace id.
    pub active_users: std::collections::BTreeMap<String, String>,
    pub llama: LlamaServerConfig,
    pub google: GoogleSettings,
    pub crawler_delay_ms: u64,
    pub user_agent: Option<String>,
    pub catalog: Catalog,
}

#[derive(Deserialize, Default)]
struct ConfigFile {
    workspace_path: Option<PathBuf>,
    #[serde(default)]
    active_users: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    engine: EngineSection,
    #[serde(default)]
    claude: ClaudeSection,
    #[serde(default)]
    llama: LlamaSection,
    #[serde(default)]
    google: GoogleSection,
    #[serde(default)]
    crawler: CrawlerSection,
}

#[derive(Deserialize, Default)]
struct EngineSection {
    kind: Option<String>,
}
#[derive(Deserialize, Default)]
struct ClaudeSection {
    model: Option<String>,
    effort: Option<String>,
    fallbacks: Option<bool>,
    bin: Option<PathBuf>,
}
#[derive(Deserialize, Default)]
struct LlamaSection {
    url: Option<String>,
    model: Option<String>,
}
#[derive(Deserialize, Default)]
struct GoogleSection {
    mode: Option<String>,
    calendar_id: Option<String>,
}
#[derive(Deserialize, Default)]
struct CrawlerSection {
    min_delay_ms: Option<u64>,
    user_agent: Option<String>,
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.trim().is_empty())
}

pub fn config_path(app_dir: &Path) -> PathBuf {
    app_dir.join("config.toml")
}

fn read_config(app_dir: Option<&Path>) -> ConfigFile {
    app_dir
        .map(config_path)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|raw| toml::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Set `key = value` at the top level (`table = None`) or in `[table]`,
/// keeping everything else in config.toml.
pub fn write_config_value(app_dir: &Path, table: Option<&str>, key: &str, value: toml::Value) -> Result<(), String> {
    let path = config_path(app_dir);
    let mut doc: toml::Table = std::fs::read_to_string(&path).ok().and_then(|raw| raw.parse().ok()).unwrap_or_default();
    let target = match table {
        None => &mut doc,
        Some(t) => match doc.entry(t.to_string()).or_insert_with(|| toml::Value::Table(Default::default())) {
            toml::Value::Table(t) => t,
            _ => return Err(format!("config.toml: [{t}] is not a table")),
        },
    };
    target.insert(key.to_string(), value);
    std::fs::create_dir_all(app_dir).map_err(|e| e.to_string())?;
    std::fs::write(&path, toml::to_string(&doc).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

impl Settings {
    /// Production settings: config.toml, then environment overrides.
    pub fn from_env(paths: Paths) -> Settings {
        let app_dir = env("JELEARNER_HOME").map(PathBuf::from).or_else(|| dirs::config_dir().map(|d| d.join("JELearner")));
        let cfg = read_config(app_dir.as_deref());
        let catalog = Catalog::load(app_dir.as_ref().map(|d| d.join("models/catalog.json")).as_deref());
        let engine = env("JELEARNER_ENGINE").or(cfg.engine.kind).and_then(|k| EngineKind::parse(&k)).unwrap_or(EngineKind::Auto);
        let google_mode = match env("JELEARNER_GOOGLE_MODE").or(cfg.google.mode).as_deref() {
            Some("api") | Some("google") => GoogleMode::Api,
            _ => GoogleMode::Mock,
        };
        Settings {
            paths,
            workspace: env("JELEARNER_WORKSPACE").map(PathBuf::from).or(cfg.workspace_path),
            engine,
            claude: ClaudeConfig {
                base_url: env("JELEARNER_CLAUDE_BASE_URL").unwrap_or_else(|| "https://api.anthropic.com".into()),
                model: env("JELEARNER_CLAUDE_MODEL").or(cfg.claude.model).unwrap_or_else(|| catalog.cloud.default.clone()),
                effort: env("JELEARNER_CLAUDE_EFFORT").or(cfg.claude.effort).or_else(|| catalog.cloud.effort.clone()),
                fallbacks: cfg.claude.fallbacks.unwrap_or(true),
            },
            claude_api_key: None,
            use_keychain: env("JELEARNER_NO_KEYCHAIN").is_none(),
            claude_bin: env("JELEARNER_CLAUDE_BIN").map(PathBuf::from).or(cfg.claude.bin),
            active_users: cfg.active_users,
            llama: LlamaServerConfig {
                url: env("JELEARNER_LLAMA_URL").or(cfg.llama.url).unwrap_or_else(|| "http://127.0.0.1:8080/v1".into()),
                model: env("JELEARNER_LLAMA_MODEL").or(cfg.llama.model).unwrap_or_else(|| catalog.defaults.brain.clone()),
            },
            google: GoogleSettings {
                mode: google_mode,
                gmail_base: env("JELEARNER_GMAIL_BASE_URL").unwrap_or_else(|| "https://gmail.googleapis.com".into()),
                calendar_base: env("JELEARNER_CALENDAR_BASE_URL").unwrap_or_else(|| "https://www.googleapis.com".into()),
                calendar_id: cfg.google.calendar_id.unwrap_or_else(|| "primary".into()),
                token: env("JELEARNER_GOOGLE_TOKEN"),
            },
            crawler_delay_ms: env("JELEARNER_CRAWL_DELAY_MS").and_then(|v| v.parse().ok()).or(cfg.crawler.min_delay_ms).unwrap_or(1500),
            user_agent: cfg.crawler.user_agent,
            catalog,
            app_dir,
        }
    }

    /// Settings for tests: everything under `app_dir`, mocks everywhere.
    /// Like production, the workspace chosen earlier is read from
    /// `app_dir/config.toml`.
    pub fn for_tests(paths: Paths, app_dir: PathBuf) -> Settings {
        let catalog = Catalog::bundled();
        let cfg = read_config(Some(&app_dir));
        let workspace = cfg.workspace_path;
        Settings {
            paths,
            app_dir: Some(app_dir),
            workspace,
            engine: EngineKind::Auto,
            claude: ClaudeConfig {
                base_url: "http://127.0.0.1:9".into(),
                model: catalog.cloud.default.clone(),
                effort: catalog.cloud.effort.clone(),
                fallbacks: true,
            },
            claude_api_key: None,
            use_keychain: false,
            // Never the real CLI in tests; a test that wants one sets a fake.
            claude_bin: Some(PathBuf::from("/nonexistent/claude")),
            active_users: cfg.active_users,
            llama: LlamaServerConfig { url: "http://127.0.0.1:9/v1".into(), model: catalog.defaults.brain.clone() },
            google: GoogleSettings {
                mode: GoogleMode::Mock,
                gmail_base: "http://127.0.0.1:9".into(),
                calendar_base: "http://127.0.0.1:9".into(),
                calendar_id: "primary".into(),
                token: None,
            },
            crawler_delay_ms: 0,
            user_agent: None,
            catalog,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_config_keeps_other_values() {
        let dir = tempfile::tempdir().unwrap();
        write_config_value(dir.path(), Some("claude"), "model", "m1".into()).unwrap();
        write_config_value(dir.path(), None, "workspace_path", "/w".into()).unwrap();
        write_config_value(dir.path(), Some("claude"), "effort", "low".into()).unwrap();
        let cfg = read_config(Some(dir.path()));
        assert_eq!(cfg.claude.model.as_deref(), Some("m1"));
        assert_eq!(cfg.claude.effort.as_deref(), Some("low"));
        assert_eq!(cfg.workspace_path, Some(PathBuf::from("/w")));
    }
}
