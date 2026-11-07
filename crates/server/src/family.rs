//! Family mode (docs/plan.md §19.2.4–19.2.5, stage F2).
//!
//! A family member may not know any programming and must never meet Git,
//! sync or connection settings. The admin sets their computer up once:
//!
//! 1. `POST /api/family/key {url}`: a deploy key for this computer
//!    (`<app dir>/deploy/<owner>__<repo>/id_ed25519`, never in the
//!    workspace) and where on GitHub to add it (write access).
//! 2. `POST /api/family/test {url}`: does the key work yet?
//! 3. `POST /api/family/clone {url, path}`: download with that key; the
//!    clone remembers it (`core.sshCommand`), so later git calls need nothing.
//! 4. The usual user, language and Claude steps (the admin signs the member
//!    in to the member's own Claude).
//! 5. `POST /api/family/finish {member, pin}`: this computer now belongs to
//!    that member; the admin's PIN guards admin settings (salted hash in
//!    config.toml); the setup changes are pushed.
//!
//! Afterwards `POST /api/family/sync` runs on its own (the page calls it on
//! open, after changes and on close): fetch, commit **only**
//! `profiles/<member>/`, replay on top of the admin's work, push. Shared
//! files are never committed from here, so the member cannot clash with the
//! admin. Failures come back as `offline` (retry later, quietly), `auth`
//! (the key was removed) or `conflict` (kept local; for the admin to look at).
//!
//! The PIN is a household lock, not security: everything runs on the
//! member's own computer.

use crate::api::user_path;
use crate::settings::{self, FamilySettings};
use crate::{bad, fail, ApiError, Shared};
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

type R<T> = Result<Json<T>, ApiError>;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/family/key", post(make_key))
        .route("/api/family/test", post(test_key))
        .route("/api/family/open-key", post(open_key_page))
        .route("/api/family/clone", post(clone))
        .route("/api/family/finish", post(finish))
        .route("/api/family/unlock", post(unlock))
        .route("/api/family/leave", post(leave))
        .route("/api/family/sync", post(sync))
}

// ── URLs and keys ───────────────────────────────────────

/// A GitHub repo URL in the SSH form deploy keys need, plus owner/repo.
/// Other URLs (another host, a local path in tests) pass through unchanged.
pub fn ssh_url(url: &str) -> (String, Option<(String, String)>) {
    let u = url.trim();
    let rest = u.strip_prefix("https://github.com/").or_else(|| u.strip_prefix("git@github.com:")).or_else(|| u.strip_prefix("ssh://git@github.com/"));
    if let Some(rest) = rest {
        let mut it = rest.trim_end_matches('/').trim_end_matches(".git").splitn(3, '/');
        if let (Some(o), Some(r)) = (it.next(), it.next())
            && !o.is_empty()
            && !r.is_empty()
        {
            return (format!("git@github.com:{o}/{r}.git"), Some((o.to_string(), r.to_string())));
        }
    }
    (u.to_string(), None)
}

fn key_path(app_dir: &Path, owner: &str, repo: &str) -> PathBuf {
    let safe = |s: &str| s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect::<String>();
    app_dir.join("deploy").join(format!("{}__{}", safe(owner), safe(repo))).join("id_ed25519")
}

/// The ssh command that uses only this key. Forward slashes work on Windows too.
pub fn ssh_command(key: &Path) -> String {
    let k = key.display().to_string().replace('\\', "/");
    format!("ssh -i \"{k}\" -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new -o BatchMode=yes")
}

fn host_name() -> String {
    sysinfo::System::host_name().unwrap_or_else(|| "這台電腦".into())
}

/// Make the key once; later calls return the same public key.
async fn ensure_key(path: &Path) -> Result<String, String> {
    let public = path.with_extension("pub");
    if !path.exists() {
        std::fs::create_dir_all(path.parent().ok_or("位置無效")?).map_err(|e| e.to_string())?;
        let comment = format!("JELearner {}", host_name());
        let out = Command::new("ssh-keygen")
            .args(["-t", "ed25519", "-N", "", "-q", "-C", &comment, "-f"])
            .arg(path)
            .output()
            .await
            .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { "這台電腦沒有 ssh-keygen（Windows：設定 → 選用功能 → OpenSSH 用戶端）".to_string() } else { e.to_string() })?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
    }
    std::fs::read_to_string(&public).map(|s| s.trim().to_string()).map_err(|e| e.to_string())
}

/// Run git without prompts; `ssh` = the ssh command for this call (before
/// the clone has it in its config).
async fn git(dir: &Path, args: &[&str], ssh: Option<&str>, secs: u64) -> Result<String, String> {
    let mut c = Command::new("git");
    c.args(args).current_dir(dir).env("GIT_TERMINAL_PROMPT", "0").env("LC_ALL", "C").kill_on_drop(true);
    if let Some(s) = ssh {
        c.env("GIT_SSH_COMMAND", s);
    }
    let out = tokio::time::timeout(Duration::from_secs(secs), c.output())
        .await
        .map_err(|_| "timed out".to_string())?
        .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { "找不到 git".to_string() } else { e.to_string() })?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// What kind of trouble a git error is, in words the member never sees
/// (the page shows one sentence; this goes into "給幫忙的人看").
pub fn classify(err: &str) -> &'static str {
    let e = err.to_lowercase();
    if ["could not resolve host", "network is unreachable", "timed out", "connection refused", "operation timed out", "no route to host"].iter().any(|x| e.contains(x)) {
        "offline"
    } else if ["permission denied", "repository not found", "authentication failed", "could not read from remote", "access denied", "key is already in use"].iter().any(|x| e.contains(x)) {
        "auth"
    } else {
        "other"
    }
}

fn app_dir(s: &settings::Settings) -> Result<PathBuf, ApiError> {
    s.app_dir.clone().ok_or_else(|| fail("沒有設定資料夾"))
}

// ── setup (the admin, on the member's computer) ─────────

#[derive(Deserialize)]
struct UrlBody {
    url: String,
    #[serde(default)]
    path: String,
}

async fn make_key(State(s): State<Shared>, Json(b): Json<UrlBody>) -> R<Value> {
    let (url, gh) = ssh_url(&b.url);
    let Some((owner, repo)) = gh else {
        // Not GitHub (a local or self-hosted repo): the system's own git sign-in is used.
        return Ok(Json(json!({ "url": url, "public_key": null })));
    };
    let dir = app_dir(&*s.settings.read().await)?;
    let key = key_path(&dir, &owner, &repo);
    let public = ensure_key(&key).await.map_err(fail)?;
    Ok(Json(json!({
        "url": url,
        "public_key": public,
        "title": format!("JELearner – {}", host_name()),
        "add_url": format!("https://github.com/{owner}/{repo}/settings/keys/new"),
    })))
}

/// Open the repo's "add deploy key" page in the browser. Only that page of a
/// GitHub repo, built here from owner/repo: the local API must not open
/// arbitrary links.
async fn open_key_page(Json(b): Json<UrlBody>) -> R<Value> {
    let (_, gh) = ssh_url(&b.url);
    let (owner, repo) = gh.ok_or_else(|| bad("只支援 GitHub 的 repo"))?;
    let ok = |s: &str| s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    if !ok(&owner) || !ok(&repo) {
        return Err(bad("repo 網址無效"));
    }
    let page = format!("https://github.com/{owner}/{repo}/settings/keys/new");
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(&page).spawn().map_err(|e| fail(format!("無法開啟瀏覽器：{e}")))?;
    Ok(Json(json!({ "opened": page })))
}

async fn key_for(s: &Shared, url: &str) -> Result<(String, Option<PathBuf>), ApiError> {
    let (url, gh) = ssh_url(url);
    let key = match gh {
        Some((o, r)) => Some(key_path(&app_dir(&*s.settings.read().await)?, &o, &r)),
        None => None,
    };
    Ok((url, key.filter(|k| k.exists())))
}

async fn test_key(State(s): State<Shared>, Json(b): Json<UrlBody>) -> R<Value> {
    let (url, key) = key_for(&s, &b.url).await?;
    let ssh = key.as_deref().map(ssh_command);
    let cwd = std::env::temp_dir();
    match git(&cwd, &["ls-remote", "--heads", &url], ssh.as_deref(), 30).await {
        Ok(refs) => Ok(Json(json!({ "ok": true, "empty": refs.trim().is_empty() }))),
        Err(e) => Ok(Json(json!({ "ok": false, "reason": classify(&e), "detail": e }))),
    }
}

async fn clone(State(s): State<Shared>, Json(b): Json<UrlBody>) -> R<Value> {
    let (url, key) = key_for(&s, &b.url).await?;
    let settings = s.settings.read().await.clone();
    let path = user_path(&settings, if b.path.trim().is_empty() { "~/JELearner-private" } else { &b.path })?;
    let target = crate::setup::target_for("clone", &path, &url);
    if let Some(top) = crate::setup::enclosing_repo(&target).await {
        return Err(bad(format!("這個位置在另一個 Git repository（{}）裡面，請換一個位置", top.display())));
    }
    if target.exists() && std::fs::read_dir(&target).map(|mut d| d.next().is_some()).unwrap_or(false) {
        return Err(bad(format!("{} 已經有東西了，請換一個位置", target.display())));
    }
    let parent = target.parent().ok_or_else(|| bad("位置無效"))?;
    std::fs::create_dir_all(parent).map_err(|e| fail(e.to_string()))?;
    let ssh = key.as_deref().map(ssh_command);
    git(parent, &["clone", "--quiet", &url, &target.to_string_lossy()], ssh.as_deref(), 300)
        .await
        .map_err(|e| ApiError(axum::http::StatusCode::BAD_GATEWAY, format!("下載失敗：{e}")))?;
    if let Some(cmd) = &ssh {
        git(&target, &["config", "core.sshCommand", cmd], None, 10).await.map_err(fail)?;
    }
    // Remember the key; the member comes at the end of the setup.
    let fam = FamilySettings { key, ..Default::default() };
    save(&s, Some(fam)).await?;
    let out = crate::api::use_workspace(&s, target, false).await?;
    Ok(Json(out))
}

async fn save(s: &Shared, fam: Option<FamilySettings>) -> Result<(), ApiError> {
    let dir = app_dir(&*s.settings.read().await)?;
    let table = match &fam {
        Some(f) => Some(toml::Table::try_from(f).map_err(|e| fail(e.to_string()))?),
        None => None,
    };
    settings::write_config_table(&dir, "family", table).map_err(fail)?;
    s.settings.write().await.family = fam;
    Ok(())
}

fn hash_pin(salt: &str, pin: &str) -> String {
    let mut h = Sha256::new();
    h.update(salt.as_bytes());
    h.update(b":");
    h.update(pin.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn valid_pin(pin: &str) -> bool {
    (4..=6).contains(&pin.len()) && pin.chars().all(|c| c.is_ascii_digit())
}

#[derive(Deserialize)]
struct FinishBody {
    member: String,
    pin: String,
}

async fn finish(State(s): State<Shared>, Json(b): Json<FinishBody>) -> R<Value> {
    if !valid_pin(&b.pin) {
        return Err(bad("PIN 要 4 到 6 位數字"));
    }
    let root = crate::api::root_path(&s).await?;
    let known: Vec<String> = s.root.read().await.as_ref().map(|r| r.root.list_profiles().into_iter().map(|p| p.id).collect()).unwrap_or_default();
    if !known.iter().any(|m| m == &b.member) {
        return Err(bad("工作區裡沒有這位使用者"));
    }
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(|e| fail(e.to_string()))?;
    let salt: String = salt.iter().map(|b| format!("{b:02x}")).collect();
    let mut fam = s.settings.read().await.family.clone().unwrap_or_default();
    fam.member = Some(b.member.clone());
    fam.pin_hash = Some(hash_pin(&salt, &b.pin));
    fam.pin_salt = Some(salt);
    save(&s, Some(fam)).await?;
    // This repo's commits are the member's (their computer usually has no Git author at all).
    for (k, v) in [("user.name", b.member.clone()), ("user.email", format!("{}@jelearner.local", b.member))] {
        git(&root, &["config", k, &v], None, 10).await.map_err(fail)?;
    }
    // The admin is here: the new member and their settings go up now.
    let pushed = crate::git::sync(&root, Some(format!("Add {} (family member's computer)", b.member))).await;
    Ok(Json(json!({ "member": b.member, "synced": pushed.is_ok(), "detail": pushed.err() })))
}

#[derive(Deserialize)]
struct PinBody {
    pin: String,
}

async fn check_pin(s: &Shared, pin: &str) -> Result<(), ApiError> {
    let fam = s.settings.read().await.family.clone().ok_or_else(|| bad("這台電腦不是家人模式"))?;
    match (fam.pin_salt, fam.pin_hash) {
        (Some(salt), Some(hash)) if hash_pin(&salt, pin.trim()) == hash => Ok(()),
        (Some(_), Some(_)) => Err(ApiError(axum::http::StatusCode::FORBIDDEN, "PIN 不對".into())),
        _ => Ok(()), // still being set up: no PIN yet
    }
}

async fn unlock(State(s): State<Shared>, Json(b): Json<PinBody>) -> R<Value> {
    check_pin(&s, &b.pin).await?;
    Ok(Json(json!({ "ok": true })))
}

/// The admin turns family mode off on this computer (the key stays, so a
/// new setup can reuse it).
async fn leave(State(s): State<Shared>, Json(b): Json<PinBody>) -> R<Value> {
    check_pin(&s, &b.pin).await?;
    save(&s, None).await?;
    Ok(Json(json!({ "ok": true })))
}

// ── sync (on its own, the member never sees it) ─────────

/// One sync at a time: the page may ask on open, after changes and on close.
static ONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Commit the member's own folder, bring in the admin's work, push.
pub async fn sync_member(root: &Path, member: &str) -> Result<Value, (String, String)> {
    let err = |e: String| (classify(&e).to_string(), e);
    let mine = format!("profiles/{member}");
    // 1. Commit only my folder.
    if root.join(&mine).exists() {
        git(root, &["add", "-A", "--", &mine], None, 30).await.map_err(err)?;
        let staged = git(root, &["diff", "--cached", "--name-only"], None, 30).await.map_err(err)?;
        if !staged.trim().is_empty() {
            let diff = git(root, &["diff", "--cached", "-U0", "--no-color"], None, 30).await.map_err(err)?;
            if let Some(kind) = diff.lines().filter(|l| l.starts_with('+') && !l.starts_with("+++")).find_map(jl_core::vcs::secret_like) {
                let _ = git(root, &["reset", "-q"], None, 30).await;
                return Err(("other".into(), format!("看起來含有 {kind}，沒有 commit")));
            }
            let names: Vec<String> = staged.lines().map(str::to_string).collect();
            let msg = jl_core::vcs::commit_message(&names);
            git(root, &["-c", &format!("user.name={member}"), "-c", &format!("user.email={member}@jelearner.local"), "commit", "-q", "-m", &msg], None, 30)
                .await
                .map_err(err)?;
        }
    }
    // 2. The admin's work, with mine replayed on top (other local edits are set aside and put back).
    git(root, &["fetch", "--quiet", "origin"], None, 60).await.map_err(err)?;
    let upstream = git(root, &["rev-parse", "--abbrev-ref", "@{u}"], None, 10).await.is_ok();
    let mut pulled = false;
    if upstream {
        let counts = git(root, &["rev-list", "--left-right", "--count", "HEAD...@{u}"], None, 10).await.map_err(err)?;
        let behind: u32 = counts.split_whitespace().nth(1).and_then(|n| n.parse().ok()).unwrap_or(0);
        if behind > 0 {
            if let Err(e) = git(root, &["-c", &format!("user.name={member}"), "-c", &format!("user.email={member}@jelearner.local"), "pull", "--rebase", "--autostash", "-q"], None, 120).await {
                let _ = git(root, &["rebase", "--abort"], None, 30).await;
                return Err(("conflict".into(), e));
            }
            pulled = true;
        }
    }
    // 3. Up.
    let ahead = if upstream {
        git(root, &["rev-list", "--count", "@{u}..HEAD"], None, 10).await.map_err(err)?.trim().parse::<u32>().unwrap_or(0)
    } else {
        1
    };
    let pushed = if ahead > 0 {
        let args: &[&str] = if upstream { &["push", "-q"] } else { &["push", "-q", "-u", "origin", "HEAD"] };
        git(root, args, None, 120).await.map_err(err)?;
        true
    } else {
        false
    };
    Ok(json!({ "ok": true, "pulled": pulled, "pushed": pushed }))
}

async fn sync(State(s): State<Shared>) -> R<Value> {
    let Some(member) = s.settings.read().await.family.as_ref().and_then(|f| f.member.clone()) else {
        return Err(bad("這台電腦不是家人模式"));
    };
    let root = crate::api::root_path(&s).await?;
    let _one = ONE.lock().await;
    // Progress records first (they live in my folder too).
    if let Ok(l) = crate::api::ws(&s).await {
        let _ = jl_core::import::export_records(&l.ws, &l.db).await;
    }
    match sync_member(&root, &member).await {
        Ok(v) => Ok(Json(v)),
        Err((reason, detail)) => Ok(Json(json!({ "ok": false, "reason": reason, "detail": detail }))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploy_keys_need_the_ssh_url() {
        assert_eq!(ssh_url("https://github.com/ellrin/JELearner-private").0, "git@github.com:ellrin/JELearner-private.git");
        assert_eq!(ssh_url("https://github.com/ellrin/JELearner-private.git/").1, Some(("ellrin".into(), "JELearner-private".into())));
        assert_eq!(ssh_url("git@github.com:ellrin/x.git").0, "git@github.com:ellrin/x.git");
        assert_eq!(ssh_url("/tmp/bare.git"), ("/tmp/bare.git".into(), None));
    }

    #[test]
    fn errors_are_sorted_for_the_member() {
        assert_eq!(classify("ssh: Could not resolve host: github.com"), "offline");
        assert_eq!(classify("git@github.com: Permission denied (publickey)."), "auth");
        assert_eq!(classify("ERROR: Repository not found."), "auth");
        assert_eq!(classify("something else"), "other");
    }

    #[test]
    fn pin_hash_and_rules() {
        assert!(valid_pin("1234") && valid_pin("123456"));
        assert!(!valid_pin("123") && !valid_pin("1234567") && !valid_pin("12a4"));
        assert_ne!(hash_pin("a", "1234"), hash_pin("b", "1234"), "salted");
        assert_eq!(hash_pin("a", "1234"), hash_pin("a", "1234"));
    }

    #[tokio::test]
    async fn makes_one_key_per_repo_and_reuses_it() {
        let dir = tempfile::tempdir().unwrap();
        let key = key_path(dir.path(), "ellrin", "JELearner-private");
        let a = ensure_key(&key).await.unwrap();
        assert!(a.starts_with("ssh-ed25519 "), "{a}");
        assert_eq!(ensure_key(&key).await.unwrap(), a, "same key the second time");
        assert!(ssh_command(&key).contains("IdentitiesOnly=yes"));
    }
}
