//! Anthropic API keys, one per JELearner user. Never in a repository.
//!
//! Looked up in this order:
//! 1. a key set in [`crate::Settings`] (tests);
//! 2. `ANTHROPIC_API_KEY` (a developer override that applies to every user
//!    of this process);
//! 3. the OS keychain (macOS Keychain / Windows Credential Manager / Linux
//!    keyutils), account `anthropic-api-key:<workspace id>:<user id>`;
//! 4. `<config dir>/JELearner/credentials/<workspace id>/<user id>.toml`
//!    (`anthropic_api_key = "…"`, owner-only permissions), used when the
//!    keychain is unavailable;
//! 5. for the workspace owner only: the single key of the first version
//!    (keychain account `anthropic-api-key` or `credentials.toml`).
//!
//! A user never falls back to another user's key (docs/plan.md §4).

use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SERVICE: &str = "JELearner";
const LEGACY_ACCOUNT: &str = "anthropic-api-key";

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum KeySource {
    Settings,
    Env,
    Keychain,
    File,
    /// The first version's single key, read for the workspace owner.
    Legacy,
}

/// Whose key.
#[derive(Debug, Clone)]
pub struct Who {
    pub workspace: String,
    pub user: String,
    /// May read the first version's single key (the workspace owner).
    pub legacy: bool,
}

impl Who {
    fn account(&self) -> String {
        format!("{LEGACY_ACCOUNT}:{}:{}", self.workspace, self.user)
    }
}

/// Keychain reads can prompt on macOS; read each account at most once.
static CACHED: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

fn keychain_get(account: &str) -> Option<String> {
    let mut c = CACHED.lock().unwrap_or_else(|e| e.into_inner());
    let map = c.get_or_insert_with(HashMap::new);
    map.entry(account.to_string())
        .or_insert_with(|| keyring::Entry::new(SERVICE, account).ok()?.get_password().ok())
        .clone()
        .filter(|k| !k.trim().is_empty())
}

fn remember(account: &str, k: Option<String>) {
    let mut c = CACHED.lock().unwrap_or_else(|e| e.into_inner());
    c.get_or_insert_with(HashMap::new).insert(account.to_string(), k);
}

fn credentials_file(app_dir: &Path, who: &Who) -> PathBuf {
    app_dir.join("credentials").join(&who.workspace).join(format!("{}.toml", who.user))
}

fn read_file(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let t: toml::Table = raw.parse().ok()?;
    t.get("anthropic_api_key")?.as_str().map(str::to_string).filter(|k| !k.trim().is_empty())
}

pub fn key(explicit: Option<&str>, use_keychain: bool, app_dir: Option<&Path>, who: Option<&Who>) -> Option<(String, KeySource)> {
    if let Some(k) = explicit.filter(|k| !k.trim().is_empty()) {
        return Some((k.to_string(), KeySource::Settings));
    }
    if let Ok(k) = std::env::var("ANTHROPIC_API_KEY")
        && !k.trim().is_empty()
    {
        return Some((k, KeySource::Env));
    }
    // Without a chosen user there is no key to use.
    let who = who?;
    if use_keychain && let Some(k) = keychain_get(&who.account()) {
        return Some((k, KeySource::Keychain));
    }
    if let Some(k) = app_dir.and_then(|d| read_file(&credentials_file(d, who))) {
        return Some((k, KeySource::File));
    }
    if who.legacy {
        if use_keychain && let Some(k) = keychain_get(LEGACY_ACCOUNT) {
            return Some((k, KeySource::Legacy));
        }
        if let Some(k) = app_dir.and_then(|d| read_file(&d.join("credentials.toml"))) {
            return Some((k, KeySource::Legacy));
        }
    }
    None
}

/// Store this user's key (empty string removes it). Keychain first; the file
/// only when the keychain is not usable on this system.
pub fn save_key(k: &str, use_keychain: bool, app_dir: Option<&Path>, who: &Who) -> Result<KeySource, String> {
    let k = k.trim();
    let account = who.account();
    if use_keychain && let Ok(entry) = keyring::Entry::new(SERVICE, &account) {
        let r = if k.is_empty() {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(e),
            }
        } else {
            entry.set_password(k)
        };
        if r.is_ok() {
            remember(&account, Some(k.to_string()).filter(|s| !s.is_empty()));
            if k.is_empty()
                && let Some(d) = app_dir
            {
                let _ = std::fs::remove_file(credentials_file(d, who));
            }
            return Ok(KeySource::Keychain);
        }
    }
    let dir = app_dir.ok_or("系統沒有設定資料夾，也無法使用金鑰圈")?;
    let path = credentials_file(dir, who);
    if k.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(KeySource::File);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut t = toml::Table::new();
    t.insert("anthropic_api_key".into(), toml::Value::String(k.to_string()));
    std::fs::write(&path, toml::to_string(&t).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(KeySource::File)
}

/// `sk-ant-••••••••3f9a`: enough to tell keys apart, nothing more.
pub fn mask(k: &str) -> String {
    let k = k.trim();
    let tail: String = k.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    let head = if k.starts_with("sk-ant-") && k.chars().count() > 15 { "sk-ant-" } else { "" };
    format!("{head}••••••••{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who(user: &str, legacy: bool) -> Who {
        Who { workspace: "ws-1".into(), user: user.into(), legacy }
    }

    #[test]
    fn mask_shows_only_last_four() {
        assert_eq!(mask("sk-ant-api03-abcdefghijklmnop3f9a"), "sk-ant-••••••••3f9a");
        assert!(!mask("sk-ant-api03-SECRETSECRET1234").contains("SECRET"));
    }

    #[test]
    fn keys_are_per_user_with_owner_legacy_fallback() {
        // Only meaningful without a developer override in the environment.
        if std::env::var("ANTHROPIC_API_KEY").is_ok_and(|k| !k.trim().is_empty()) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let d = Some(dir.path());
        assert_eq!(save_key("sk-ant-user-a-0000000000", false, d, &who("a", false)).unwrap(), KeySource::File);
        assert_eq!(key(None, false, d, Some(&who("a", false))).unwrap().0, "sk-ant-user-a-0000000000");
        assert!(key(None, false, d, Some(&who("b", false))).is_none(), "B never gets A's key");
        assert!(key(None, false, d, None).is_none());

        // First-version single key: only the owner may use it.
        std::fs::write(dir.path().join("credentials.toml"), "anthropic_api_key = \"sk-ant-legacy-000000000\"\n").unwrap();
        assert_eq!(key(None, false, d, Some(&who("owner", true))).unwrap().1, KeySource::Legacy);
        assert!(key(None, false, d, Some(&who("b", false))).is_none());

        save_key("", false, d, &who("a", false)).unwrap();
        assert!(key(None, false, d, Some(&who("a", false))).is_none());
        assert_eq!(key(Some("explicit"), false, d, None).unwrap().1, KeySource::Settings);
    }
}
