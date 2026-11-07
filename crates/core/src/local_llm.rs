//! Local models in the shared folder (docs/decisions.md §15).
//!
//! Every project on the machine keeps model weights and llama.cpp in one
//! folder, `LOCAL_LLM_HOME` (or `[llama] home` in config.toml, since an app
//! opened from Finder does not see shell variables):
//!
//! ```text
//! <home>/gguf/<owner>/<repo>/<file>     what `hf download --local-dir` writes
//! <home>/runtimes/llama.cpp/…/llama-server
//! ```
//!
//! Nothing here downloads: PaperComposer's setup or `hf download` fill the
//! folder. This module finds the files and starts `llama-server` for one
//! catalog model. Tested on an M6 Mac with Qwen3.5 4B (an ignored test).

use crate::catalog::Model;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Child;

/// The shared folder: the configured one, else `LOCAL_LLM_HOME`.
pub fn home(configured: Option<&Path>) -> Option<PathBuf> {
    configured
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("LOCAL_LLM_HOME").filter(|v| !v.is_empty()).map(PathBuf::from))
}

/// Files under `dir` (not in `.cache`, where hf keeps partial downloads)
/// whose path ends with `suffix`, shortest first (the plain file, not a
/// split part or an MTP head in a subfolder).
fn find_under(dir: &Path, suffix: &str) -> Option<PathBuf> {
    let want = suffix.to_lowercase();
    let mut hits = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                if e.file_name() != ".cache" {
                    stack.push(p);
                }
            } else if p.to_string_lossy().to_lowercase().ends_with(&want) {
                hits.push(p);
            }
        }
    }
    hits.sort_by_key(|p| p.as_os_str().len());
    hits.into_iter().next()
}

/// The model's weights in the shared folder, when downloaded.
pub fn find_model(home: &Path, m: &Model) -> Option<PathBuf> {
    if m.repo.is_empty() || m.file_suffix.is_empty() {
        return None;
    }
    find_under(&home.join("gguf").join(&m.repo), &m.file_suffix)
}

/// The vision projector next to the weights, when the catalog names one.
pub fn find_mmproj(home: &Path, m: &Model) -> Option<PathBuf> {
    find_under(&home.join("gguf").join(&m.repo), m.mmproj_suffix.as_deref()?)
}

/// `llama-server` in the shared runtimes folder.
pub fn find_runtime(home: &Path) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    let mut stack = vec![home.join("runtimes/llama.cpp")];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if e.file_name() == exe {
                return Some(p);
            }
        }
    }
    None
}

/// A running `llama-server`.
#[derive(Debug)]
pub struct Running {
    pub child: Child,
    pub model_id: String,
    /// Base URL ending in `/v1`.
    pub url: String,
}

impl Running {
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map(|a| a.port()).unwrap_or(8089)
}

/// Why a model cannot start, in the user's words.
pub fn missing(home: &Path, m: &Model) -> Option<String> {
    if find_runtime(home).is_none() {
        return Some(format!(
            "共用資料夾 {} 裡沒有 llama.cpp：請用 PaperComposer 的「AI 與同步」安裝一次，或把 llama-server 放在 runtimes/llama.cpp",
            home.display()
        ));
    }
    if find_model(home, m).is_none() {
        return Some(format!(
            "還沒下載 {} {}：hf download {} --include \"*{}\" --local-dir \"{}\"",
            m.name,
            m.variant,
            m.repo,
            m.file_suffix,
            home.join("gguf").join(&m.repo).display()
        ));
    }
    None
}

/// Start `llama-server` for `m` and wait until it answers (a large model
/// takes a while to load the first time). No thinking: Gemma 4 and Qwen
/// otherwise reason for a thousand tokens before every JSON answer.
pub async fn start(home: &Path, m: &Model, wait: Duration) -> Result<Running, String> {
    if let Some(why) = missing(home, m) {
        return Err(why);
    }
    let bin = find_runtime(home).expect("checked");
    let weights = find_model(home, m).expect("checked");
    let port = free_port();
    let mut cmd = tokio::process::Command::new(&bin);
    cmd.arg("-m")
        .arg(&weights)
        .args(["--host", "127.0.0.1", "--port", &port.to_string(), "-ngl", "999", "--jinja", "--reasoning-budget", "0"])
        .args(["-c", &m.context.unwrap_or(16384).to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if let Some(mm) = find_mmproj(home, m) {
        cmd.arg("--mmproj").arg(mm);
    }
    if let Some(dir) = bin.parent() {
        cmd.current_dir(dir); // its shared libraries sit next to it
    }
    let mut child = cmd.spawn().map_err(|e| format!("無法啟動 llama-server：{e}"))?;
    let url = format!("http://127.0.0.1:{port}/v1");
    let http = reqwest::Client::new();
    let deadline = tokio::time::Instant::now() + wait;
    while tokio::time::Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() {
                use tokio::io::AsyncReadExt;
                let _ = e.read_to_string(&mut err).await;
            }
            let tail: Vec<&str> = err.lines().rev().take(3).collect();
            return Err(format!("llama-server 結束了（{status}）：{}", tail.into_iter().rev().collect::<Vec<_>>().join(" ")));
        }
        if http.get(format!("{url}/models")).timeout(Duration::from_secs(2)).send().await.is_ok_and(|r| r.status().is_success()) {
            return Ok(Running { child, model_id: m.id.clone(), url });
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let _ = child.kill().await;
    Err(format!("{} 啟動逾時", m.name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    #[test]
    fn finds_files_in_the_hf_layout() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let cat = Catalog::bundled();
        let m = cat.get("qwen3.5-4b-q8").expect("Qwen 4B in the catalog");
        assert!(find_model(home, m).is_none());
        assert!(missing(home, m).unwrap().contains("llama.cpp"));

        touch(&home.join("runtimes/llama.cpp/llama-b1/llama-server"));
        let repo = home.join("gguf").join(&m.repo);
        // A partial download in hf's .cache does not count; the plain file wins over an MTP head.
        touch(&repo.join(".cache/huggingface/download/Qwen3.5-4B-Q8_0.gguf"));
        assert!(missing(home, m).unwrap().contains("hf download"));
        touch(&repo.join("MTP/mtp-Qwen3.5-4B-Q8_0.gguf"));
        touch(&repo.join("Qwen3.5-4B-Q8_0.gguf"));
        touch(&repo.join("mmproj-BF16.gguf"));
        assert_eq!(find_model(home, m).unwrap(), repo.join("Qwen3.5-4B-Q8_0.gguf"));
        assert_eq!(find_mmproj(home, m).unwrap(), repo.join("mmproj-BF16.gguf"));
        assert_eq!(find_runtime(home).unwrap(), home.join("runtimes/llama.cpp/llama-b1/llama-server"));
        assert!(missing(home, m).is_none());
    }

    /// Real hardware: llama-server and Qwen3.5 4B from the shared folder,
    /// one JSON answer. Run: `cargo test -p jl-core real_local -- --ignored`.
    #[tokio::test]
    #[ignore = "needs LOCAL_LLM_HOME with llama.cpp and Qwen3.5 4B"]
    async fn real_local_model_answers() {
        use crate::provider::{CompletionRequest, LlamaServerConfig, LlamaServerProvider, LlmProvider};
        let home = home(None).expect("LOCAL_LLM_HOME");
        let cat = Catalog::bundled();
        let m = cat.get("qwen3.5-4b-q8").unwrap();
        let mut run = start(&home, m, Duration::from_secs(180)).await.unwrap();
        let p = LlamaServerProvider::new(LlamaServerConfig { url: run.url.clone(), model: m.id.clone() });
        let schema = serde_json::json!({"type":"object","properties":{"word":{"type":"string"}},"required":["word"]});
        let t = std::time::Instant::now();
        let out = p
            .complete(&CompletionRequest {
                system: "Reply with JSON only.".into(),
                user: "Give one English word that means 'happy'.".into(),
                max_tokens: 200,
                json_schema: Some(schema),
            })
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v["word"].as_str().is_some_and(|w| !w.is_empty()), "{out}");
        assert!(t.elapsed() < Duration::from_secs(30), "no thinking: {:?}", t.elapsed());
        assert!(run.alive());
        run.child.kill().await.unwrap();
    }
}
