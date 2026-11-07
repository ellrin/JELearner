//! Setup wizard checks (docs/plan.md §19.2.3, stage F1).
//!
//! The wizard asks one question at a time and, before it creates or
//! downloads anything, runs the checks for that path here. Each check says
//! in one sentence what is wrong and how to fix it, so the wizard can stop
//! on that step instead of failing half way:
//!
//! - `git`: Git is installed.
//! - `identity`: Git knows the author (needed for the first commit).
//! - `location`: a usable folder, and **not inside another Git repository**
//!   (the author once cloned the workspace into an older clone of itself).
//! - `reach`: the repo answers (`git ls-remote`, never prompting).
//! - `private`: a GitHub repo is private (personal data must not be public).
//! - `empty`: creating a new workspace needs an empty repo.

use crate::api::user_path;
use crate::{bad, fail, ApiError, Shared};
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

type R<T> = Result<Json<T>, ApiError>;

pub fn routes() -> Router<Shared> {
    Router::new().route("/api/setup/check", post(check)).route("/api/setup/git-identity", post(set_identity))
}

#[derive(Debug, Serialize, Clone)]
pub struct Check {
    pub id: &'static str,
    pub ok: bool,
    /// What was found, one sentence.
    pub msg: String,
    /// How to fix it, one sentence (only when not ok).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

fn ok(id: &'static str, msg: impl Into<String>) -> Check {
    Check { id, ok: true, msg: msg.into(), fix: None }
}
fn no(id: &'static str, msg: impl Into<String>, fix: impl Into<String>) -> Check {
    Check { id, ok: false, msg: msg.into(), fix: Some(fix.into()) }
}

/// Run git without ever waiting for a password; None when git is missing.
async fn git(dir: Option<&Path>, args: &[&str], secs: u64) -> Option<Result<String, String>> {
    let mut c = Command::new("git");
    c.args(args).env("GIT_TERMINAL_PROMPT", "0").env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes").env("LC_ALL", "C").kill_on_drop(true);
    if let Some(d) = dir {
        c.current_dir(d);
    }
    let out = match tokio::time::timeout(Duration::from_secs(secs), c.output()).await {
        Err(_) => return Some(Err("逾時".into())),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Ok(Err(e)) => return Some(Err(e.to_string())),
        Ok(Ok(o)) => o,
    };
    Some(if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    })
}

/// The repository that already contains `path` (or the folder above it that
/// exists), when that is not `path` itself.
pub async fn enclosing_repo(path: &Path) -> Option<PathBuf> {
    let anc = path.ancestors().find(|a| a.exists())?.to_path_buf();
    let dir = if anc.is_dir() { anc } else { anc.parent()?.to_path_buf() };
    let top = git(Some(&dir), &["rev-parse", "--show-toplevel"], 10).await?.ok()?;
    let top = PathBuf::from(top);
    let same = match (top.canonicalize(), path.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    (!same).then_some(top)
}

/// `owner/repo` of a GitHub URL (https or ssh), else None.
fn github_repo(url: &str) -> Option<(String, String)> {
    let rest = url.trim().strip_prefix("https://github.com/").or_else(|| url.trim().strip_prefix("git@github.com:"))?;
    let mut it = rest.trim_end_matches('/').trim_end_matches(".git").splitn(3, '/');
    let (owner, repo) = (it.next()?, it.next()?);
    (!owner.is_empty() && !repo.is_empty()).then(|| (owner.to_string(), repo.to_string()))
}

fn gh_api() -> String {
    std::env::var("JELEARNER_GH_API").unwrap_or_else(|_| "https://api.github.com".into())
}

/// Without signing in, GitHub shows a public repo and answers 404 for a
/// private one; `reach` already proved the user can read it.
async fn private_check(url: &str) -> Check {
    let Some((owner, repo)) = github_repo(url) else {
        return ok("private", "不是 GitHub 的 repo，請自己確認它不是公開的");
    };
    let r = reqwest::Client::new()
        .get(format!("{}/repos/{owner}/{repo}", gh_api()))
        .header("User-Agent", "JELearner")
        .timeout(Duration::from_secs(15))
        .send()
        .await;
    match r {
        Ok(r) if r.status().as_u16() == 404 => ok("private", "是 private repo"),
        Ok(r) if r.status().is_success() => no(
            "private",
            "這個 repo 是公開的，任何人都看得到你的學習資料",
            format!("到 GitHub 的 {owner}/{repo} → Settings → General 最下方，改成 Private，再檢查一次"),
        ),
        _ => ok("private", "暫時無法向 GitHub 確認是否為 private，請自己確認"),
    }
}

#[derive(Deserialize)]
struct CheckBody {
    /// `clone` (used it before), `create` (new, synced), `local` (this computer only).
    plan: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    path: String,
}

/// The folder the workspace will be in: for `clone`, `<path>/<repo name>`
/// when `path` is a parent; for the others, `path` itself.
pub fn target_for(plan: &str, path: &Path, url: &str) -> PathBuf {
    if plan == "clone" {
        let name = url.trim().trim_end_matches('/').trim_end_matches(".git").rsplit(['/', ':']).next().unwrap_or("").to_string();
        if !name.is_empty() && path.file_name().is_none_or(|f| f != name.as_str()) && path.exists() {
            return path.join(name);
        }
    }
    path.to_path_buf()
}

async fn check(State(s): State<Shared>, Json(b): Json<CheckBody>) -> R<Value> {
    let plan = b.plan.as_str();
    if !["clone", "create", "local"].contains(&plan) {
        return Err(bad("plan 只能是 clone、create、local"));
    }
    let mut out: Vec<Check> = vec![];

    // Git
    let version = git(None, &["--version"], 10).await;
    match &version {
        // This computer only: the workspace works without Git (no history, no sync until it is installed).
        None | Some(Err(_)) if plan == "local" => out.push(ok("git", "沒有 Git：先只存在這台電腦，之後要同步再安裝")),
        None | Some(Err(_)) => out.push(no(
            "git",
            "這台電腦沒有 Git",
            if cfg!(target_os = "macos") { "在「終端機」執行 xcode-select --install，裝好後再檢查一次" } else { "到 git-scm.com 下載安裝 Git，裝好後再檢查一次" },
        )),
        Some(Ok(v)) => out.push(ok("git", v.replace("git version", "Git").trim().to_string())),
    }
    let has_git = matches!(version, Some(Ok(_)));

    // Author, for the first commit of a new workspace.
    if plan != "clone" && has_git {
        let name = git(None, &["config", "--global", "user.name"], 10).await.and_then(Result::ok).unwrap_or_default();
        let email = git(None, &["config", "--global", "user.email"], 10).await.and_then(Result::ok).unwrap_or_default();
        out.push(if name.is_empty() || email.is_empty() {
            no("identity", "Git 還不知道作者是誰", "填上名字與 email（只記在這台電腦的 Git 設定）")
        } else {
            ok("identity", format!("作者 {name}"))
        });
    }

    // Location
    let settings = s.settings.read().await.clone();
    match user_path(&settings, &b.path) {
        Err(e) => out.push(no("location", e.1, "換一個位置")),
        Ok(path) => {
            let target = target_for(plan, &path, &b.url);
            if let Some(top) = enclosing_repo(&target).await {
                out.push(no(
                    "location",
                    format!("這個位置在另一個 Git repository（{}）裡面", top.display()),
                    "換一個不在任何 repo 裡的位置，例如 ~/JELearner-private",
                ));
            } else if target.exists() && std::fs::read_dir(&target).map(|mut d| d.next().is_some()).unwrap_or(false) {
                out.push(no(
                    "location",
                    format!("{} 已經有東西了", target.display()),
                    if plan == "clone" { "換一個位置，或刪掉那個資料夾" } else { "換一個空的位置；要用現有的工作區請選「開啟資料夾」" },
                ));
            } else {
                out.push(ok("location", format!("會放在 {}", target.display())));
            }
        }
    }

    // The repo
    if plan != "local" {
        let url = b.url.trim();
        if url.is_empty() {
            out.push(no("reach", "還沒有 repo 網址", "貼上 GitHub repo 的網址"));
        } else if has_git {
            match git(None, &["ls-remote", "--heads", url], 30).await {
                Some(Ok(refs)) => {
                    out.push(ok("reach", "連得到這個 repo"));
                    out.push(private_check(url).await);
                    if plan == "create" {
                        out.push(if refs.trim().is_empty() {
                            ok("empty", "repo 是空的，可以建立新的工作區")
                        } else {
                            no("empty", "這個 repo 已經有內容", "如果是你在別台電腦建立的工作區，改成下載它")
                        });
                    } else if refs.trim().is_empty() {
                        out.push(no("empty", "這個 repo 是空的，還沒有工作區", "改成在這個 repo 建立新的工作區"));
                    }
                }
                _ => out.push(no(
                    "reach",
                    "連不到這個 repo",
                    "確認網址正確，並確認這台電腦登入過 GitHub（例如用 GitHub Desktop 或 git 登入一次）",
                )),
            }
        }
    }

    let ready = out.iter().all(|c| c.ok);
    Ok(Json(json!({ "ready": ready, "checks": out })))
}

#[derive(Deserialize)]
struct IdentityBody {
    name: String,
    email: String,
}

/// Set the global Git author (the `identity` check's fix).
async fn set_identity(Json(b): Json<IdentityBody>) -> R<Value> {
    let (name, email) = (b.name.trim(), b.email.trim());
    if name.is_empty() || !email.contains('@') {
        return Err(bad("請填名字與 email"));
    }
    for (k, v) in [("user.name", name), ("user.email", email)] {
        match git(None, &["config", "--global", k, v], 10).await {
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(fail(e)),
            None => return Err(bad("這台電腦沒有 Git")),
        }
    }
    Ok(Json(json!({ "name": name, "email": email })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_github_urls() {
        assert_eq!(github_repo("https://github.com/ellrin/JELearner-private.git"), Some(("ellrin".into(), "JELearner-private".into())));
        assert_eq!(github_repo("git@github.com:ellrin/JELearner-private.git"), Some(("ellrin".into(), "JELearner-private".into())));
        assert_eq!(github_repo("https://github.com/ellrin/x/"), Some(("ellrin".into(), "x".into())));
        assert_eq!(github_repo("https://gitlab.com/a/b.git"), None);
        assert_eq!(github_repo("https://github.com/"), None);
    }

    #[tokio::test]
    async fn finds_the_repo_around_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let outer = dir.path().join("outer");
        std::fs::create_dir_all(&outer).unwrap();
        assert!(git(Some(&outer), &["init", "-q"], 10).await.unwrap().is_ok());
        // A new folder inside the outer repo: refused.
        assert!(enclosing_repo(&outer.join("JELearner-private")).await.is_some());
        // The repo itself (opening an existing workspace): fine.
        assert!(enclosing_repo(&outer).await.is_none());
        // Next to it: fine.
        assert!(enclosing_repo(&dir.path().join("elsewhere")).await.is_none());
    }

    #[test]
    fn clone_goes_into_a_folder_named_after_the_repo() {
        let dir = tempfile::tempdir().unwrap();
        let url = "https://github.com/ellrin/JELearner-private.git";
        assert_eq!(target_for("clone", dir.path(), url), dir.path().join("JELearner-private"));
        assert_eq!(target_for("create", dir.path(), url), dir.path());
        let named = dir.path().join("JELearner-private");
        assert_eq!(target_for("clone", &named, url), named, "already the repo's name");
    }
}
