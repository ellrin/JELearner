//! Speech: text to speech and speech to text behind one trait
//! (docs/plan.md §15), like [`crate::provider::LlmProvider`] for text.
//!
//! Engines, so upper layers never name one:
//!
//! - [`MacSpeech`] (stage D step 2, docs/decisions.md §14): recognition with
//!   Apple's on-device `SpeechTranscriber` through the `jl-speech` helper
//!   (`tools/macos-speech/`). Speaking stays in the browser
//!   (`web/speech.js`), whose `speechSynthesis` already uses the system
//!   voices, Premium ones included. Tested on a Mac (M6, macOS 27) with
//!   speech made by `say`; **microphone recordings are tested by hand**.
//! - Windows built-in speech (`Windows.Media.SpeechRecognition`; Japanese
//!   needs the language pack) on daidai's computer: not written yet, the
//!   browser's recognition stands in.
//! - optionally a cloud pronunciation score, only if it becomes cheap and
//!   convenient (not planned now).
//!
//! [`MockSpeech`] and [`fake`] (a stand-in `jl-speech`) are for tests.

use crate::lang::Lang;
use async_trait::async_trait;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum SpeechError {
    #[error("語音引擎無法使用：{0}")]
    Unavailable(String),
    /// The engine works but the language's model is not downloaded yet.
    #[error("還沒下載{0}的語音辨識模型")]
    NotInstalled(String),
    #[error("語音處理失敗：{0}")]
    Failed(String),
}

/// Synthesised speech.
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    /// e.g. `audio/wav`.
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// A recognised utterance.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Transcript {
    pub text: String,
    /// 0–1 when the engine reports it.
    pub confidence: Option<f64>,
}

#[async_trait]
pub trait SpeechProvider: Send + Sync {
    /// Short engine id, e.g. `macos`, `windows`, `whisper+kokoro`, `mock`.
    fn id(&self) -> String;
    /// Speak `text` in `lang`; `voice` is an engine-specific voice name,
    /// `rate` 1.0 = normal speed.
    async fn synthesize(&self, text: &str, lang: Lang, voice: Option<&str>, rate: f32) -> Result<Audio, SpeechError>;
    /// Recognise speech in `lang` from `audio`.
    async fn transcribe(&self, audio: &Audio, lang: Lang) -> Result<Transcript, SpeechError>;
}

/// A test engine: "audio" is the UTF-8 text itself, so a round trip
/// returns the text that was spoken.
#[derive(Debug, Default)]
pub struct MockSpeech;

#[async_trait]
impl SpeechProvider for MockSpeech {
    fn id(&self) -> String {
        "mock".into()
    }

    async fn synthesize(&self, text: &str, lang: Lang, _voice: Option<&str>, rate: f32) -> Result<Audio, SpeechError> {
        if text.trim().is_empty() {
            return Err(SpeechError::Failed("沒有文字".into()));
        }
        if !(0.25..=4.0).contains(&rate) {
            return Err(SpeechError::Failed(format!("速度 {rate}")));
        }
        Ok(Audio { mime: format!("text/plain; lang={}", lang.code()), bytes: text.as_bytes().to_vec() })
    }

    async fn transcribe(&self, audio: &Audio, _lang: Lang) -> Result<Transcript, SpeechError> {
        let text = String::from_utf8(audio.bytes.clone()).map_err(|e| SpeechError::Failed(e.to_string()))?;
        Ok(Transcript { text, confidence: Some(1.0) })
    }
}

// ── macOS: Apple's on-device recognition through jl-speech ─────────

/// Full locale per language: a bare `en` can resolve to a regional variant
/// nobody asked for (Apple picked en-IE before en-US was installed).
pub fn locale(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "en-US",
        Lang::Ja => "ja-JP",
    }
}

/// Recognition model state for one language.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LocaleStatus {
    pub lang: Lang,
    pub locale: String,
    pub supported: bool,
    pub installed: bool,
}

/// Apple's `SpeechTranscriber` via the `jl-speech` helper (see
/// `tools/macos-speech/`). Recognition only: speaking is done by the page.
#[derive(Debug, Clone)]
pub struct MacSpeech {
    pub bin: std::path::PathBuf,
}

impl MacSpeech {
    pub fn new(bin: impl Into<std::path::PathBuf>) -> MacSpeech {
        MacSpeech { bin: bin.into() }
    }

    /// Where `jl-speech` is: the configured path, else next to the running
    /// program (the packaged app), else this checkout's release build
    /// (`swift build -c release` in `tools/macos-speech/`). None off macOS.
    pub fn find(configured: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
        if let Some(p) = configured {
            return Some(p.to_path_buf()); // as configured, even when missing: status then says why
        }
        if !cfg!(target_os = "macos") {
            return None;
        }
        let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("jl-speech")));
        let dev = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/macos-speech/.build/release/jl-speech");
        beside.into_iter().chain([dev]).find(|p| p.is_file())
    }

    async fn run(&self, args: &[&str]) -> Result<serde_json::Value, SpeechError> {
        let out = tokio::process::Command::new(&self.bin)
            .args(args)
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| SpeechError::Unavailable(format!("{}：{e}", self.bin.display())))?;
        let v: serde_json::Value = serde_json::from_slice(&out.stdout)
            .map_err(|_| SpeechError::Failed(format!("jl-speech 的回覆無法解析：{}", String::from_utf8_lossy(&out.stdout).trim())))?;
        if let Some(e) = v["error"].as_str() {
            if let Some(loc) = e.strip_prefix("not_installed: ") {
                return Err(SpeechError::NotInstalled(loc.to_string()));
            }
            return Err(SpeechError::Failed(e.to_string()));
        }
        Ok(v)
    }

    /// Whether each language's recognition model is supported and downloaded.
    pub async fn status(&self) -> Result<Vec<LocaleStatus>, SpeechError> {
        let mut args = vec!["status"];
        args.extend(Lang::ALL.iter().map(|l| locale(*l)));
        let v = self.run(&args).await?;
        let list = v["locales"].as_array().cloned().unwrap_or_default();
        Ok(Lang::ALL
            .iter()
            .map(|&lang| {
                let row = list.iter().find(|r| r["asked"] == locale(lang));
                LocaleStatus {
                    lang,
                    locale: row.and_then(|r| r["locale"].as_str()).unwrap_or(locale(lang)).to_string(),
                    supported: row.is_some_and(|r| r["supported"] == true),
                    installed: row.is_some_and(|r| r["installed"] == true),
                }
            })
            .collect())
    }

    /// Download one language's model (the system keeps it afterwards).
    pub async fn install(&self, lang: Lang) -> Result<(), SpeechError> {
        self.run(&["install", locale(lang)]).await.map(|_| ())
    }
}

#[async_trait]
impl SpeechProvider for MacSpeech {
    fn id(&self) -> String {
        "macos".into()
    }

    async fn synthesize(&self, _text: &str, _lang: Lang, _voice: Option<&str>, _rate: f32) -> Result<Audio, SpeechError> {
        Err(SpeechError::Unavailable("朗讀由網頁使用系統聲音".into()))
    }

    async fn transcribe(&self, audio: &Audio, lang: Lang) -> Result<Transcript, SpeechError> {
        if audio.bytes.is_empty() {
            return Err(SpeechError::Failed("沒有錄到聲音".into()));
        }
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let ext = if audio.mime.contains("wav") { "wav" } else { "audio" };
        let path = std::env::temp_dir().join(format!("jl-speech-{}-{n}.{ext}", std::process::id()));
        tokio::fs::write(&path, &audio.bytes).await.map_err(|e| SpeechError::Failed(e.to_string()))?;
        let r = self.run(&["transcribe", locale(lang), &path.to_string_lossy()]).await;
        let _ = tokio::fs::remove_file(&path).await;
        let v = r?;
        Ok(Transcript { text: v["text"].as_str().unwrap_or("").to_string(), confidence: v["confidence"].as_f64() })
    }
}

/// A stand-in `jl-speech` for tests (a shell script), like
/// [`crate::claude_code::fake`]: `status` reports en-US installed and ja-JP
/// not until `install ja-JP` runs; `transcribe` answers with the contents
/// of the audio file, after the 44-byte header when it is a WAV (so tests
/// send text as "audio"), and leaves the arguments in `last-args`. Never
/// calls Apple's engine.
#[cfg(unix)]
pub mod fake {
    use std::path::{Path, PathBuf};

    pub fn write(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(dir).unwrap();
        let d = dir.display();
        let script = format!(
            r#"#!/bin/sh
D="{d}"
echo "$@" > "$D/last-args"
ja=false; [ -f "$D/ja-installed" ] && ja=true
case "$1" in
  status) echo "{{\"locales\":[{{\"asked\":\"en-US\",\"locale\":\"en-US\",\"supported\":true,\"installed\":true}},{{\"asked\":\"ja-JP\",\"locale\":\"ja-JP\",\"supported\":true,\"installed\":$ja}}]}}" ;;
  install) [ "$2" = "ja-JP" ] && touch "$D/ja-installed"; echo "{{\"locale\":\"$2\",\"installed\":true}}" ;;
  transcribe)
    if [ "$2" = "ja-JP" ] && [ "$ja" = false ]; then echo '{{"error":"not_installed: ja-JP"}}'; exit 1; fi
    if [ ! -f "$3" ]; then echo '{{"error":"讀不了錄音檔"}}'; exit 1; fi
    if head -c 4 "$3" | grep -q RIFF; then t=$(tail -c +45 "$3"); else t=$(cat "$3"); fi
    printf '{{"locale":"%s","text":"%s","confidence":0.9}}\n' "$2" "$t" ;;
  *) echo '{{"error":"usage"}}'; exit 1 ;;
esac
"#
        );
        let bin = dir.join("jl-speech");
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real hardware: Apple's recogniser on speech made by `say`, converted
    /// to the 16 kHz mono WAV the page sends. Needs a Mac, the helper built
    /// (`swift build -c release` in tools/macos-speech) and both models
    /// installed. Run: `cargo test -p jl-core real_mac -- --ignored`.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    #[ignore = "real Apple engine: run by hand on a Mac"]
    async fn real_mac_round_trip() {
        let mac = MacSpeech::new(MacSpeech::find(None).expect("build tools/macos-speech first"));
        let dir = tempfile::tempdir().unwrap();
        for (lang, voice, text, want) in [
            // Apple writes "three" as "3" (plan §15: the shadowing compare treats them as equal).
            (Lang::En, "Samantha", "I'm planning to buy three new books this weekend.", "new books this weekend"),
            (Lang::Ja, "Kyoko", "駅の近くのカフェでコーヒーを飲みました。", "コーヒーを飲みました"),
        ] {
            let aiff = dir.path().join("s.aiff");
            let wav = dir.path().join("s.wav");
            assert!(std::process::Command::new("say").args(["-v", voice, "-o"]).arg(&aiff).arg(text).status().unwrap().success());
            assert!(std::process::Command::new("afconvert").args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"]).arg(&aiff).arg(&wav).status().unwrap().success());
            let audio = Audio { mime: "audio/wav".into(), bytes: std::fs::read(&wav).unwrap() };
            let t = mac.transcribe(&audio, lang).await.unwrap();
            assert!(t.text.contains(want), "{lang:?}: {}", t.text);
            assert!(t.confidence.unwrap_or(0.0) > 0.5, "{t:?}");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mac_speech_through_the_helper() {
        let dir = tempfile::tempdir().unwrap();
        let mac = MacSpeech::new(fake::write(dir.path()));
        let engine: &dyn SpeechProvider = &mac;
        assert_eq!(engine.id(), "macos");

        let st = mac.status().await.unwrap();
        assert_eq!(st.len(), 2);
        assert!(st[0].installed && st[0].locale == "en-US");
        assert!(st[1].supported && !st[1].installed);

        let said = Audio { mime: "audio/wav".into(), bytes: b"I am planning to buy three books".to_vec() };
        let t = engine.transcribe(&said, Lang::En).await.unwrap();
        assert_eq!(t.text, "I am planning to buy three books");
        assert_eq!(t.confidence, Some(0.9));
        let args = std::fs::read_to_string(dir.path().join("last-args")).unwrap();
        assert!(args.starts_with("transcribe en-US ") && args.trim_end().ends_with(".wav"), "{args}");
        let tmp = args.split_whitespace().last().unwrap();
        assert!(!std::path::Path::new(tmp).exists(), "the temporary recording is removed");

        // Japanese is not downloaded yet: a distinct error the page can act on.
        assert!(matches!(engine.transcribe(&said, Lang::Ja).await, Err(SpeechError::NotInstalled(l)) if l == "ja-JP"));
        mac.install(Lang::Ja).await.unwrap();
        assert!(mac.status().await.unwrap()[1].installed);
        assert!(engine.transcribe(&said, Lang::Ja).await.is_ok());

        assert!(engine.transcribe(&Audio { mime: "audio/wav".into(), bytes: vec![] }, Lang::En).await.is_err());
        assert!(engine.synthesize("Hi", Lang::En, None, 1.0).await.is_err(), "speaking is the page's job");
        let missing = MacSpeech::new(dir.path().join("nope"));
        assert!(matches!(missing.status().await, Err(SpeechError::Unavailable(_))));
    }

    #[tokio::test]
    async fn mock_round_trip_through_the_trait() {
        let engine: Box<dyn SpeechProvider> = Box::new(MockSpeech);
        assert_eq!(engine.id(), "mock");
        let audio = engine.synthesize("締め切りは金曜日です。", Lang::Ja, None, 1.0).await.unwrap();
        assert_eq!(audio.mime, "text/plain; lang=ja");
        let t = engine.transcribe(&audio, Lang::Ja).await.unwrap();
        assert_eq!(t.text, "締め切りは金曜日です。");
        assert!(engine.synthesize("  ", Lang::En, None, 1.0).await.is_err());
        assert!(engine.synthesize("Hi", Lang::En, None, 9.0).await.is_err());
    }
}
