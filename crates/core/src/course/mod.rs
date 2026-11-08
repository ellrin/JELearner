//! Course engine: Course → Lesson (one sitting) → Unit (one passed exercise
//! = 1 point). Design and file formats: docs/courses.md.
//!
//! - [`model`] — Course, CourseKind, LessonPlan, UnitPlan, statuses
//! - [`sizing`] — new words / grammar / text length by language and level;
//!   unit plan by daily goal
//! - [`topics`] — interests.csv, topics.toml, interest vs general per day
//! - [`fixed`] — word/grammar list → lessons (no model needed)
//! - [`outline`] — topic course outline (model), validation, ≥3 recurrence
//! - [`plan`] — lesson plan → lesson content (model) → Markdown
//! - [`store`] — outline files in `profiles/<id>/recipes/`
//!
//! Only the outline is generated when a course is created; a lesson's
//! content is generated when the learner opens it.

pub mod fixed;
pub mod model;
pub mod outline;
pub mod picker;
pub mod plan;
pub mod prompts;
pub mod sizing;
pub mod store;
pub mod topics;

pub use model::*;
pub use prompts::CourseTemplates;

use crate::lang::Lang;
use crate::lesson::LessonError;
use crate::provider::ProviderError;
use crate::workspace::Profile;

#[derive(Debug, thiserror::Error)]
pub enum CourseError {
    #[error("{0}")]
    Invalid(String),
    #[error("找不到課程：{0}")]
    NotFound(String),
    #[error("課程檔 {0} 格式錯誤：{1}")]
    Format(String, String),
    #[error(transparent)]
    Lesson(#[from] LessonError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// What the course engine needs to know about the learner for one language.
#[derive(Debug, Clone)]
pub struct Learner {
    pub lang: Lang,
    pub native_language: String,
    /// Content difficulty score (see [`crate::level`]).
    pub difficulty: u32,
    pub daily_minutes: u32,
    /// Words the learner already has (left out of new words).
    pub known_words: Vec<String>,
}

impl Learner {
    pub fn from_profile(p: &Profile, lang: Lang, known_words: Vec<String>) -> Learner {
        Learner {
            lang,
            native_language: p.native_language.clone(),
            difficulty: p.lang(lang).difficulty,
            daily_minutes: p.daily_minutes,
            known_words,
        }
    }

    pub fn level(&self) -> crate::level::Level {
        crate::level::from_score(self.lang, self.difficulty)
    }

    /// Minutes of one lesson (a daily goal above 60 minutes is two lessons).
    pub fn lesson_minutes(&self) -> u32 {
        sizing::daily_sessions(self.daily_minutes)[0]
    }

    pub fn lessons_per_day(&self) -> usize {
        sizing::daily_sessions(self.daily_minutes).len()
    }

    pub fn size(&self) -> sizing::ContentSize {
        sizing::content_size(self.lang, self.difficulty)
    }
}

/// Parse a model reply that should be one JSON object; tolerates a ```json
/// fence (local models).
pub(crate) fn parse_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, CourseError> {
    let t = text.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    serde_json::from_str(t).map_err(|e| CourseError::Invalid(format!("課程格式不符：{e}")))
}

/// A strict JSON Schema object: every property required, nothing extra.
pub(crate) fn strict_object(props: serde_json::Value) -> serde_json::Value {
    let required: Vec<String> = props.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    serde_json::json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}
