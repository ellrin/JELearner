//! Course prompt templates (MiniJinja files in `prompts/`), each
//! overridable by a file of the same name in the workspace's `prompts/`.

use crate::lesson::LessonError;
use serde_json::Value;
use std::path::Path;

pub const OUTLINE_SYSTEM: &str = "course_outline_system.md";
pub const OUTLINE_USER: &str = "course_outline_user.md";
pub const LESSON_SYSTEM: &str = "course_lesson_system.md";
pub const LESSON_USER: &str = "course_lesson_user.md";
pub const LESSON_MARKDOWN: &str = "course_lesson.md.j2";
pub const TOPICS_SYSTEM: &str = "today_topics_system.md";
pub const TOPICS_USER: &str = "today_topics_user.md";
pub const FOCUS_SYSTEM: &str = "focus_sheet_system.md";
pub const FOCUS_USER: &str = "focus_sheet_user.md";
pub const INTERESTS_SYSTEM: &str = "interests_suggest_system.md";
pub const INTERESTS_USER: &str = "interests_suggest_user.md";
pub const LIBRARY_SYSTEM: &str = "library_article_system.md";
pub const LIBRARY_USER: &str = "library_article_user.md";

const BUNDLED: [(&str, &str); 13] = [
    (LIBRARY_SYSTEM, include_str!("../../../../prompts/library_article_system.md")),
    (LIBRARY_USER, include_str!("../../../../prompts/library_article_user.md")),
    (INTERESTS_SYSTEM, include_str!("../../../../prompts/interests_suggest_system.md")),
    (INTERESTS_USER, include_str!("../../../../prompts/interests_suggest_user.md")),
    (TOPICS_SYSTEM, include_str!("../../../../prompts/today_topics_system.md")),
    (TOPICS_USER, include_str!("../../../../prompts/today_topics_user.md")),
    (FOCUS_SYSTEM, include_str!("../../../../prompts/focus_sheet_system.md")),
    (FOCUS_USER, include_str!("../../../../prompts/focus_sheet_user.md")),
    (OUTLINE_SYSTEM, include_str!("../../../../prompts/course_outline_system.md")),
    (OUTLINE_USER, include_str!("../../../../prompts/course_outline_user.md")),
    (LESSON_SYSTEM, include_str!("../../../../prompts/course_lesson_system.md")),
    (LESSON_USER, include_str!("../../../../prompts/course_lesson_user.md")),
    (LESSON_MARKDOWN, include_str!("../../../../prompts/course_lesson.md.j2")),
];

#[derive(Debug, Clone)]
pub struct CourseTemplates {
    files: Vec<(&'static str, String)>,
    /// Which files came from the workspace (for the UI).
    pub overridden: Vec<String>,
}

impl CourseTemplates {
    pub fn bundled() -> CourseTemplates {
        CourseTemplates { files: BUNDLED.iter().map(|(n, s)| (*n, s.to_string())).collect(), overridden: vec![] }
    }

    /// Bundled templates, each replaced by the workspace's file when present
    /// and not empty.
    pub fn load(workspace_prompts: Option<&Path>) -> CourseTemplates {
        let mut t = CourseTemplates::bundled();
        if let Some(dir) = workspace_prompts {
            for (name, slot) in t.files.iter_mut() {
                if let Ok(s) = std::fs::read_to_string(dir.join(*name))
                    && !s.trim().is_empty()
                {
                    *slot = s;
                    t.overridden.push(name.to_string());
                }
            }
        }
        t
    }

    pub fn get(&self, name: &str) -> &str {
        self.files.iter().find(|(n, _)| *n == name).map(|(_, s)| s.as_str()).unwrap_or("")
    }

    pub fn render(&self, name: &str, ctx: Value) -> Result<String, LessonError> {
        crate::lesson::render(name, self.get(name), ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_overrides() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(OUTLINE_USER), "topic: {{ keywords | join(', ') }}").unwrap();
        std::fs::write(dir.path().join(LESSON_USER), "  \n").unwrap();
        let t = CourseTemplates::load(Some(dir.path()));
        assert_eq!(t.overridden, vec![OUTLINE_USER.to_string()]);
        assert_eq!(t.render(OUTLINE_USER, serde_json::json!({"keywords": ["a", "b"]})).unwrap(), "topic: a, b");
        assert!(!t.get(LESSON_USER).trim().is_empty(), "empty file keeps the bundled one");
    }
}
