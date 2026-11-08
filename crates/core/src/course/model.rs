//! The course data model: Course → Lesson (one sitting) → Unit (one
//! exercise that is completed and passed = 1 point). See docs/courses.md.
//!
//! Stored as TOML (see [`super::store`]): the outline in
//! `profiles/<id>/recipes/<course-id>.toml` (written once, then the user's to
//! edit) and the progress in `<course-id>.progress.toml` (written by the
//! app). Every field has a serde default so that hand-edited or older files
//! keep loading. Progress fields are left out of the outline file because
//! they are skipped when at their default (planned / active / empty), and
//! [`Course::outline_only`] resets them before the outline is written.

use crate::lang::Lang;
use serde::{Deserialize, Serialize};

/// What a course is for and how its lessons are made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CourseKind {
    /// Assembled each day from due reviews, interests and general topics.
    Daily,
    /// A series around keywords / a URL / photo text (default 5 lessons).
    Topic,
    /// From an uploaded word list and/or grammar list.
    Fixed,
    /// One lesson from one article (the first version's lessons).
    Single,
    /// Conversation practice only.
    Conversation,
    /// Weekly, from mistakes and lapsed cards.
    Weakness,
    /// Level or skip assessment.
    Check,
}

impl CourseKind {
    pub fn label(self) -> &'static str {
        match self {
            CourseKind::Daily => "每日課程",
            CourseKind::Topic => "主題課程",
            CourseKind::Fixed => "固定課程",
            CourseKind::Single => "單課",
            CourseKind::Conversation => "對話課程",
            CourseKind::Weakness => "弱項複習",
            CourseKind::Check => "程度檢核",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CourseStatus {
    #[default]
    Active,
    /// Every lesson done; a topic course may be extended or a new topic chosen.
    Finished,
    Archived,
}

/// Where a lesson stands. Content is generated only when the learner opens
/// the lesson.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LessonStatus {
    #[default]
    Planned,
    Generated,
    Done,
}

/// The lesson's place in its course; decides its unit plan and prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LessonRole {
    /// A normal lesson with a text (fixed courses, daily lessons).
    #[default]
    Regular,
    /// Topic lesson 1: background and core vocabulary.
    Background,
    /// Topic lessons 2–4: one text from one angle.
    Article,
    /// Review of the words so far (fixed: every 5th lesson; topic: the last
    /// lesson, with a topic conversation).
    Review,
    /// Conversation only.
    Conversation,
    /// Assessment.
    Check,
}

/// The five skills points are split into (docs/plan.md §8.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Skill {
    Listening,
    Reading,
    Speaking,
    Writing,
    VocabGrammar,
}

impl Skill {
    /// Name used in `records/points/*.csv`.
    pub fn code(self) -> &'static str {
        match self {
            Skill::Listening => "listening",
            Skill::Reading => "reading",
            Skill::Speaking => "speaking",
            Skill::Writing => "writing",
            Skill::VocabGrammar => "vocab-grammar",
        }
    }
}

/// Exercise types (docs/plan.md §12). Each passed unit = 1 point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnitKind {
    /// A round of due review cards.
    Review,
    /// Today's text with comprehension questions.
    Reading,
    /// Native language → target language.
    Translation,
    /// Make sentences with given words / grammar.
    SentenceMaking,
    /// Fill in the blanks (target words taken out of the texts).
    Cloze,
    /// Say it more simply / more formally.
    Paraphrase,
    Listening,
    Dictation,
    Shadowing,
    Conversation,
    /// Mixed assessment questions.
    Quiz,
}

impl UnitKind {
    pub fn code(self) -> &'static str {
        match self {
            UnitKind::Review => "review",
            UnitKind::Reading => "reading",
            UnitKind::Translation => "translation",
            UnitKind::SentenceMaking => "sentence-making",
            UnitKind::Cloze => "cloze",
            UnitKind::Paraphrase => "paraphrase",
            UnitKind::Listening => "listening",
            UnitKind::Dictation => "dictation",
            UnitKind::Shadowing => "shadowing",
            UnitKind::Conversation => "conversation",
            UnitKind::Quiz => "quiz",
        }
    }

    pub fn skill(self) -> Skill {
        match self {
            UnitKind::Review | UnitKind::Cloze => Skill::VocabGrammar,
            UnitKind::Reading | UnitKind::Quiz => Skill::Reading,
            UnitKind::Translation | UnitKind::SentenceMaking | UnitKind::Paraphrase => Skill::Writing,
            UnitKind::Listening | UnitKind::Dictation => Skill::Listening,
            UnitKind::Shadowing | UnitKind::Conversation => Skill::Speaking,
        }
    }

    /// Output exercises the model writes items for.
    pub fn is_output(self) -> bool {
        matches!(self, UnitKind::Translation | UnitKind::SentenceMaking | UnitKind::Cloze | UnitKind::Paraphrase)
    }
}

/// One planned unit of a lesson.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitPlan {
    pub kind: UnitKind,
    pub skill: Skill,
    /// Expected minutes.
    pub minutes: u32,
    /// Cards, questions, items or conversation turns.
    pub items: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TargetWord {
    pub term: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reading: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub meaning: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TargetGrammar {
    pub pattern: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub meaning: String,
}

/// A real text a lesson is based on: a news article found when the lesson is
/// opened, or a page / photo text the learner gave for a topic course.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SourceText {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub text: String,
}

fn is_planned(s: &LessonStatus) -> bool {
    *s == LessonStatus::Planned
}
fn is_active(s: &CourseStatus) -> bool {
    *s == CourseStatus::Active
}
/// 0 (a `Default` lesson) counts as the first series too.
fn is_one(n: &u32) -> bool {
    *n <= 1
}

/// One lesson of the outline. Made when the course is created; the content
/// is generated when the learner opens it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LessonPlan {
    /// 1-based, continuous across extensions of a topic course.
    pub number: u32,
    #[serde(default)]
    pub role: LessonRole,
    #[serde(default)]
    pub title: String,
    /// The angle of the text (topic courses), e.g. technical / social / personal.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub angle: String,
    /// What the learner gets out of this lesson (one sentence, learner's language).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub purpose: String,
    /// Keywords to look for a news article with when the lesson is opened.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub search: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub conversation_topic: String,
    /// Which series of a topic course this lesson belongs to (1 = the first).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub series: u32,
    /// The text this lesson is based on, when the learner gave one (topic
    /// courses: the URL article or photo text becomes lesson 2's source).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub article: Option<SourceText>,
    /// Expected minutes (sum of the units).
    #[serde(default)]
    pub minutes: u32,
    /// New words introduced in this lesson.
    #[serde(default, rename = "word", skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<TargetWord>,
    /// Words from earlier lessons that reappear here (spaced).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review: Vec<String>,
    #[serde(default, rename = "grammar", skip_serializing_if = "Vec::is_empty")]
    pub grammar: Vec<TargetGrammar>,
    #[serde(default, rename = "unit", skip_serializing_if = "Vec::is_empty")]
    pub units: Vec<UnitPlan>,
    // ── Progress (stored in the progress file) ──
    #[serde(default, skip_serializing_if = "is_planned")]
    pub status: LessonStatus,
    /// Workspace-relative path of the generated lesson file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done: Option<String>,
    /// Indexes (0-based) of units completed and passed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub units_done: Vec<u32>,
}

impl LessonPlan {
    /// Does this lesson use the word (new or review)? Case-insensitive.
    pub fn uses_word(&self, term: &str) -> bool {
        let t = term.to_lowercase();
        self.words.iter().any(|w| w.term.to_lowercase() == t) || self.review.iter().any(|r| r.to_lowercase() == t)
    }

    /// `review:5,reading:10,…` for the lesson file's front matter.
    pub fn units_summary(&self) -> String {
        self.units.iter().map(|u| format!("{}:{}", u.kind.code(), u.minutes)).collect::<Vec<_>>().join(",")
    }
}

/// A course: its recipe (what it is made from) and its outline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Course {
    /// File stem of the outline file; safe characters only.
    pub id: String,
    pub kind: CourseKind,
    pub name: String,
    pub lang: Lang,
    /// Content difficulty when the course was made, e.g. `B1-3`.
    #[serde(default)]
    pub level: String,
    /// `YYYY-MM-DD`.
    #[serde(default)]
    pub created: String,
    /// Progress (stored in the progress file).
    #[serde(default, skip_serializing_if = "is_active")]
    pub status: CourseStatus,
    /// The learner's daily goal when the course was made; each lesson is
    /// sized for one sitting of it (see [`super::sizing::daily_sessions`]).
    #[serde(default = "thirty")]
    pub daily_minutes: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// Topic courses: keywords (from the request and interests.csv).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// Topic courses: the URL the topic came from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_url: String,
    /// Fixed courses: workspace-relative word / grammar lists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vocab: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grammar: Vec<String>,
    /// Fixed courses: new words per lesson; 0 = from the level table.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub new_per_lesson: u32,
    /// Fixed courses: a review lesson at every n-th position (0 = never).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub review_every: u32,
    /// Topic courses: how many series of lessons (1 + extensions); also
    /// derived from the lessons' `series` when loading.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub series: u32,
    /// Engine that wrote the outline.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub engine: String,
    /// The focus theme this course was made under (plan §19.2.6): its
    /// lessons take real names from `profiles/<id>/focus/<lang>.toml`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub focus: String,
    #[serde(default, rename = "lesson")]
    pub lessons: Vec<LessonPlan>,
}

fn thirty() -> u32 {
    30
}
fn one() -> u32 {
    1
}
fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Progress of one lesson (`<course-id>.progress.toml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LessonProgress {
    pub number: u32,
    #[serde(default)]
    pub status: LessonStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub units_done: Vec<u32>,
}

/// The progress file: everything the app changes while the course is used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Progress {
    #[serde(default)]
    pub course: String,
    #[serde(default)]
    pub status: CourseStatus,
    #[serde(default, rename = "lesson")]
    pub lessons: Vec<LessonProgress>,
}

impl Course {
    /// The progress part, for the progress file. Lessons still planned and
    /// untouched are left out.
    pub fn progress(&self) -> Progress {
        Progress {
            course: self.id.clone(),
            status: self.status,
            lessons: self
                .lessons
                .iter()
                .filter(|l| l.status != LessonStatus::Planned || l.file.is_some() || !l.units_done.is_empty())
                .map(|l| LessonProgress {
                    number: l.number,
                    status: l.status,
                    file: l.file.clone(),
                    generated: l.generated.clone(),
                    done: l.done.clone(),
                    units_done: l.units_done.clone(),
                })
                .collect(),
        }
    }

    /// Put a progress file's values over the outline. Entries for lessons
    /// that are no longer in the outline are ignored.
    pub fn apply_progress(&mut self, p: &Progress) {
        self.status = p.status;
        for lp in &p.lessons {
            if let Some(l) = self.lesson_mut(lp.number) {
                l.status = lp.status;
                l.file = lp.file.clone();
                l.generated = lp.generated.clone();
                l.done = lp.done.clone();
                l.units_done = lp.units_done.clone();
            }
        }
    }

    /// A copy without progress, for writing the outline file.
    pub fn outline_only(&self) -> Course {
        let mut c = self.clone();
        c.status = CourseStatus::Active;
        for l in c.lessons.iter_mut() {
            l.status = LessonStatus::Planned;
            l.file = None;
            l.generated = None;
            l.done = None;
            l.units_done.clear();
        }
        c
    }

    pub fn lesson(&self, number: u32) -> Option<&LessonPlan> {
        self.lessons.iter().find(|l| l.number == number)
    }

    pub fn lesson_mut(&mut self, number: u32) -> Option<&mut LessonPlan> {
        self.lessons.iter_mut().find(|l| l.number == number)
    }

    /// The first lesson not done yet.
    pub fn next_lesson(&self) -> Option<&LessonPlan> {
        self.lessons.iter().find(|l| l.status != LessonStatus::Done)
    }

    pub fn done_count(&self) -> usize {
        self.lessons.iter().filter(|l| l.status == LessonStatus::Done).count()
    }

    /// Every new word of the course, in order of introduction.
    pub fn all_words(&self) -> Vec<&TargetWord> {
        self.lessons.iter().flat_map(|l| l.words.iter()).collect()
    }

    /// A finished topic course may be extended by another series or replaced
    /// by a new topic.
    pub fn can_extend(&self) -> bool {
        self.kind == CourseKind::Topic && !self.lessons.is_empty() && self.done_count() == self.lessons.len()
    }
}
