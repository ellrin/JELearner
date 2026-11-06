//! Speech: text to speech and speech to text behind one trait
//! (docs/plan.md §15), like [`crate::provider::LlmProvider`] for text.
//!
//! Stage D step 1 runs entirely in the browser (`web/speech.js`: the Web
//! Speech API, which uses the operating system's voices; recognition only
//! when the user turns it on). This trait is the place for the engines that
//! come later, so upper layers never name one:
//!
//! - macOS built-in speech (`AVSpeechSynthesizer`, `SFSpeechRecognizer`) on
//!   the author's Mac, then Whisper large-v3-turbo (recognition) and
//!   Kokoro-82M (synthesis) once the Mac arrives;
//! - Windows built-in speech (`Windows.Media.SpeechSynthesis`,
//!   `Windows.Media.SpeechRecognition`; Japanese needs the language pack) on
//!   daidai's computer, which has little memory for local models;
//! - optionally a cloud pronunciation score, only if it becomes cheap and
//!   convenient (not planned now).
//!
//! None of these exists yet: they are **not tested on real hardware**. Only
//! [`MockSpeech`] is implemented, for tests.

use crate::lang::Lang;
use async_trait::async_trait;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum SpeechError {
    #[error("語音引擎無法使用：{0}")]
    Unavailable(String),
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

#[cfg(test)]
mod tests {
    use super::*;

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
