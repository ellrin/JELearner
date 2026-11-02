//! The Anthropic API key. Never in a repository.
//!
//! Looked up in this order: `ANTHROPIC_API_KEY`, the OS keychain (macOS
//! Keychain / Windows Credential Manager / Linux keyutils), then
//! `<config dir>/JELearner/credentials.toml` (`anthropic_api_key = "…"`,
//! written with owner-only permissions when the keychain is unavailable).

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SERVICE: &str = "JELearner";
const ACCOUNT: &str = "anthropic-api-key";

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum KeySource {
    Settings,
    Env,
    Keychain,
    File,
}

/// Keychain reads can prompt on macOS; read at most once per process.
static CACHED: Mutex<Option<Option<String>>> = Mutex::new(None);

fn keychain() -> Option<keyring::Entry> {
    keyring::Entry::new(SERVICE, ACCOUNT).ok()
}

fn credentials_file(app_dir: &Path) -> PathBuf {
    app_dir.join("credentials.toml")
}

fn read_file(app_dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(credentials_file(app_dir)).ok()?;
    let t: toml::Table = raw.parse().ok()?;
    t.get("anthropic_api_key")?.as_str().map(str::to_string).filter(|k| !k.trim().is_empty())
}

pub fn key(explicit: Option<&str>, use_keychain: bool, app_dir: Option<&Path>) -> Option<(String, KeySource)> {
    if let Some(k) = explicit.filter(|k| !k.trim().is_empty()) {
        return Some((k.to_string(), KeySource::Settings));
    }
    if let Ok(k) = std::env::var("ANTHROPIC_API_KEY")
        && !k.trim().is_empty() {
            return Some((k, KeySource::Env));
        }
    if use_keychain {
        let mut c = CACHED.lock().unwrap_or_else(|e| e.into_inner());
        let k = c.get_or_insert_with(|| keychain()?.get_password().ok()).clone();
        if let Some(k) = k {
            return Some((k, KeySource::Keychain));
        }
    }
    app_dir.and_then(read_file).map(|k| (k, KeySource::File))
}

/// Store the key (empty string removes it). Keychain first; the file only
/// when the keychain is not usable on this system.
pub fn save_key(k: &str, use_keychain: bool, app_dir: Option<&Path>) -> Result<KeySource, String> {
    let k = k.trim();
    if use_keychain
        && let Some(entry) = keychain() {
            let r = if k.is_empty() {
                match entry.delete_credential() {
                    Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                    Err(e) => Err(e),
                }
            } else {
                entry.set_password(k)
            };
            if r.is_ok() {
                *CACHED.lock().unwrap_or_else(|e| e.into_inner()) = Some(Some(k.to_string()).filter(|s| !s.is_empty()));
                if k.is_empty()
                    && let Some(d) = app_dir {
                        let _ = std::fs::remove_file(credentials_file(d));
                    }
                return Ok(KeySource::Keychain);
            }
        }
    let dir = app_dir.ok_or("系統沒有設定資料夾，也無法使用金鑰圈")?;
    let path = credentials_file(dir);
    if k.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(KeySource::File);
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
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

    #[test]
    fn mask_shows_only_last_four() {
        assert_eq!(mask("sk-ant-api03-abcdefghijklmnop3f9a"), "sk-ant-••••••••3f9a");
        assert!(!mask("sk-ant-api03-SECRETSECRET1234").contains("SECRET"));
    }

    #[test]
    fn file_fallback_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(save_key("sk-ant-test-1234567890", false, Some(dir.path())).unwrap(), KeySource::File);
        assert_eq!(read_file(dir.path()).as_deref(), Some("sk-ant-test-1234567890"));
        save_key("", false, Some(dir.path())).unwrap();
        assert!(read_file(dir.path()).is_none());
        assert_eq!(key(Some("explicit"), false, Some(dir.path())).unwrap().1, KeySource::Settings);
    }
}
