//! How much goes into one lesson: new words, grammar points and text length
//! by language and level ([`content_size`]), and the unit plan by the
//! learner's daily goal ([`unit_plan`]). One table and one function each, so
//! the numbers are not scattered around the code.

use super::model::{LessonRole, UnitKind, UnitPlan};
use crate::lang::Lang;
use crate::level;
use serde::Serialize;

/// A closed range `min..=max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Span {
    pub min: u32,
    pub max: u32,
}

const fn span(min: u32, max: u32) -> Span {
    Span { min, max }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TextUnit {
    Words,
    Characters,
}

/// What one lesson holds at a given language and level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ContentSize {
    /// New words per lesson in a fixed course (the splitter's chunk size).
    pub fixed_new_words: u32,
    /// New words per lesson in a topic course.
    pub topic_new_words: Span,
    pub grammar: Span,
    /// Length of the lesson's text.
    pub text: Span,
    pub text_unit: TextUnit,
    /// Readings above kanji (Japanese N5–N4).
    pub furigana: bool,
}

/// The table (docs/courses.md). English B1–B2: ~10 new words in fixed
/// courses, 6–10 in topic lessons, 1–2 grammar points, 200–300 words.
/// Japanese N5: 6 (5–8) / 4–6 new words, 1 grammar point, 100–200
/// characters with furigana.
pub fn content_size(lang: Lang, difficulty: u32) -> ContentSize {
    let major = level::from_score(lang, difficulty).major;
    use TextUnit::*;
    let (fixed, topic, grammar, text, unit, furigana) = match (lang, major) {
        (Lang::En, "A1") => (6, span(4, 6), span(1, 1), span(80, 150), Words, false),
        (Lang::En, "A2") => (8, span(5, 8), span(1, 1), span(150, 200), Words, false),
        (Lang::En, "B1" | "B2") => (10, span(6, 10), span(1, 2), span(200, 300), Words, false),
        (Lang::En, _) => (12, span(8, 12), span(1, 2), span(300, 450), Words, false),
        (Lang::Ja, "N5") => (6, span(4, 6), span(1, 1), span(100, 200), Characters, true),
        (Lang::Ja, "N4") => (8, span(5, 7), span(1, 1), span(200, 300), Characters, true),
        (Lang::Ja, "N3") => (10, span(6, 8), span(1, 2), span(300, 500), Characters, false),
        (Lang::Ja, _) => (12, span(8, 10), span(1, 2), span(500, 800), Characters, false),
    };
    ContentSize { fixed_new_words: fixed, topic_new_words: topic, grammar, text, text_unit: unit, furigana }
}

impl ContentSize {
    /// "200–300 words" / "100–200 characters" for prompts.
    pub fn text_description(&self) -> String {
        let unit = match self.text_unit {
            TextUnit::Words => "words",
            TextUnit::Characters => "characters",
        };
        format!("{}–{} {unit}", self.text.min, self.text.max)
    }
}

/// Shortest and longest daily goal the plan is sized for.
pub const MIN_MINUTES: u32 = 10;
pub const MAX_MINUTES: u32 = 120;

/// Longest single lesson. A daily goal above it becomes two lessons a day.
pub const MAX_LESSON_MINUTES: u32 = 60;

/// Lessons per day and the minutes of each, for a daily goal.
///
/// Up to 60 minutes: one lesson. Above: two lessons of (nearly) equal
/// length, the first one a minute longer when the goal is odd, e.g. 90 →
/// 45 + 45, 61 → 31 + 30. An even split keeps both lessons at 30 minutes or
/// more, so both get the full unit plan (review, reading, output,
/// conversation) instead of a 60-minute lesson plus a stub. The goal is
/// capped at 120 minutes (two 60-minute lessons).
pub fn daily_sessions(daily_minutes: u32) -> Vec<u32> {
    let goal = daily_minutes.clamp(MIN_MINUTES, MAX_MINUTES);
    if goal <= MAX_LESSON_MINUTES {
        vec![goal]
    } else {
        vec![goal.div_ceil(2), goal / 2]
    }
}

/// The parts of a lesson in the order they are done. `Output` and `Output2`
/// become concrete exercise types by lesson number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Review,
    Reading,
    Output,
    Conversation,
    Output2,
}

/// Most minutes one part may grow to when time is left over.
fn cap(p: Part) -> u32 {
    match p {
        Part::Review => 10,
        Part::Reading => 15,
        Part::Output => 10,
        Part::Conversation => 15,
        Part::Output2 => 10,
    }
}

/// Base minutes of each part by daily goal:
///
/// | goal | reading | review (words and grammar) | output | conversation | output 2 |
/// | --- | --- | --- | --- | --- | --- |
/// | < 15 | 5 | 3 | 2 | – | – |
/// | 15–24 | 7 | 3 | – | 5 | – |
/// | 25–29 | 7 | 3 | 5 | 10 | – |
/// | 30–44 | 10 | 5 | 5 | 10 | – |
/// | ≥ 45 | 10 | 5 | 5 | 10 | 5 |
///
/// Minutes left over are handed out one at a time, in the order
/// conversation, reading, output, review, output 2, each up to its cap.
fn base_parts(minutes: u32) -> Vec<(Part, u32)> {
    use Part::*;
    match minutes {
        // The order is the lesson's order: the text first, then its words
        // and grammar (with the review cards), then practice, then talking.
        0..15 => vec![(Reading, 5), (Review, 3), (Output, 2)],
        15..25 => vec![(Reading, 7), (Review, 3), (Conversation, 5)],
        25..30 => vec![(Reading, 7), (Review, 3), (Output, 5), (Conversation, 10)],
        30..45 => vec![(Reading, 10), (Review, 5), (Output, 5), (Conversation, 10)],
        _ => vec![(Reading, 10), (Review, 5), (Output, 5), (Conversation, 10), (Output2, 5)],
    }
}

/// Minutes per part for a daily goal (clamped to 10–120). Deterministic.
fn part_minutes(daily_minutes: u32) -> Vec<(Part, u32)> {
    let goal = daily_minutes.clamp(MIN_MINUTES, MAX_MINUTES);
    let mut parts = base_parts(goal);
    let mut left = goal.saturating_sub(parts.iter().map(|p| p.1).sum());
    let order = [Part::Conversation, Part::Reading, Part::Output, Part::Review, Part::Output2];
    while left > 0 {
        let mut gave = false;
        for want in order {
            if left == 0 {
                break;
            }
            if let Some(p) = parts.iter_mut().find(|p| p.0 == want)
                && p.1 < cap(want)
            {
                p.1 += 1;
                left -= 1;
                gave = true;
            }
        }
        if !gave {
            break; // every part at its cap: the lesson is shorter than the goal
        }
    }
    parts
}

/// Cards, questions, items or turns for a unit of `minutes`.
fn items_for(kind: UnitKind, minutes: u32) -> u32 {
    match kind {
        UnitKind::Review => minutes * 4,                          // 5 min → 20 cards
        // Gradable units have at least 4 items, so one miss is not a fail
        // at the 70% pass line (3/4 = 75%; docs/plan.md §20).
        UnitKind::Reading | UnitKind::Quiz => (minutes / 2).clamp(4, 8), // 10 min → 5 questions
        UnitKind::Conversation => (minutes * 2).clamp(6, 30),      // 10 min → 20 turns
        UnitKind::Cloze => minutes.clamp(4, 12),
        _ => (minutes * 3 / 5).clamp(4, 6),                         // 5 min → 4 items
    }
}

fn unit(kind: UnitKind, minutes: u32) -> UnitPlan {
    UnitPlan { kind, skill: kind.skill(), minutes, items: items_for(kind, minutes) }
}

/// The units of one lesson.
///
/// - A text lesson (regular, background, article): the text with questions,
///   its words and grammar with a review round, output practice, a
///   conversation continuing the text. At 30 minutes: reading 10 (5
///   questions), words and grammar 5 (20 cards),
///   output 5 (3 items), conversation 10 (20 turns).
/// - Output alternates by lesson number: odd lessons translate from the
///   learner's language, even ones make sentences; the second output task
///   (≥ 45 minutes) alternates cloze / paraphrase.
/// - A review lesson replaces the reading by a cloze over the texts so far
///   and uses sentence making as its output.
/// - A conversation lesson is one conversation of the whole goal (up to 30
///   minutes); a check is quiz + translation + conversation.
pub fn unit_plan(role: LessonRole, daily_minutes: u32, number: u32) -> Vec<UnitPlan> {
    match role {
        LessonRole::Conversation => {
            return vec![unit(UnitKind::Conversation, daily_minutes.clamp(5, 30))];
        }
        LessonRole::Check => {
            let m = daily_minutes.clamp(MIN_MINUTES, 60);
            let quiz = m * 2 / 5;
            let writing = m / 5;
            return vec![unit(UnitKind::Quiz, quiz), unit(UnitKind::Translation, writing), unit(UnitKind::Conversation, m - quiz - writing)];
        }
        _ => {}
    }
    let odd = number % 2 == 1;
    part_minutes(daily_minutes)
        .into_iter()
        .map(|(part, m)| {
            let kind = match (part, role) {
                (Part::Review, _) => UnitKind::Review,
                (Part::Reading, LessonRole::Review) => UnitKind::Cloze,
                (Part::Reading, _) => UnitKind::Reading,
                (Part::Output, LessonRole::Review) => UnitKind::SentenceMaking,
                (Part::Output, _) if odd => UnitKind::Translation,
                (Part::Output, _) => UnitKind::SentenceMaking,
                (Part::Conversation, _) => UnitKind::Conversation,
                (Part::Output2, LessonRole::Review) => UnitKind::Translation,
                (Part::Output2, _) if odd => UnitKind::Cloze,
                (Part::Output2, _) => UnitKind::Paraphrase,
            };
            unit(kind, m)
        })
        .collect()
}

pub fn total_minutes(units: &[UnitPlan]) -> u32 {
    units.iter().map(|u| u.minutes).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(u: &[UnitPlan]) -> Vec<(UnitKind, u32, u32)> {
        u.iter().map(|u| (u.kind, u.minutes, u.items)).collect()
    }

    #[test]
    fn content_table_by_language_and_level() {
        let b1 = content_size(Lang::En, level::major_start(Lang::En, "B1").unwrap());
        assert_eq!(b1.fixed_new_words, 10);
        assert_eq!(b1.topic_new_words, Span { min: 6, max: 10 });
        assert_eq!(b1.grammar, Span { min: 1, max: 2 });
        assert_eq!((b1.text.min, b1.text.max, b1.text_unit), (200, 300, TextUnit::Words));
        assert!(!b1.furigana);
        assert_eq!(content_size(Lang::En, 399), b1, "B2-4 is the same row as B1");

        let n5 = content_size(Lang::Ja, 0);
        assert_eq!(n5.fixed_new_words, 6);
        assert!((5..=8).contains(&n5.fixed_new_words));
        assert_eq!(n5.topic_new_words, Span { min: 4, max: 6 });
        assert_eq!(n5.grammar, Span { min: 1, max: 1 });
        assert_eq!((n5.text.min, n5.text.max, n5.text_unit), (100, 200, TextUnit::Characters));
        assert!(n5.furigana);
        assert_eq!(n5.text_description(), "100–200 characters");
        assert!(!content_size(Lang::Ja, 250).furigana, "N3 has no furigana");
        // Grows with the level.
        for lang in Lang::ALL {
            let mut last = 0;
            for score in (0..=level::max_score(lang)).step_by(100) {
                let s = content_size(lang, score);
                assert!(s.fixed_new_words >= last);
                assert!(s.topic_new_words.min <= s.topic_new_words.max && s.text.min < s.text.max);
                last = s.fixed_new_words;
            }
        }
    }

    #[test]
    fn thirty_minute_lesson() {
        let u = unit_plan(LessonRole::Article, 30, 3);
        assert_eq!(
            kinds(&u),
            vec![
                (UnitKind::Reading, 10, 5),
                (UnitKind::Review, 5, 20),
                (UnitKind::Translation, 5, 4),
                (UnitKind::Conversation, 10, 20),
            ]
        );
        assert_eq!(unit_plan(LessonRole::Article, 30, 2)[2].kind, UnitKind::SentenceMaking);
        assert_eq!(u[3].skill, super::super::model::Skill::Speaking);
    }

    #[test]
    fn scales_with_daily_goal() {
        for goal in [10, 12, 15, 20, 25, 27, 30, 40, 45, 60] {
            let u = unit_plan(LessonRole::Regular, goal, 1);
            assert!((3..=5).contains(&u.len()), "{goal}: {} units", u.len());
            assert_eq!(total_minutes(&u), goal, "{goal} minutes: {:?}", kinds(&u));
            assert_eq!(unit_plan(LessonRole::Regular, goal, 1), u, "deterministic");
        }
        let short = unit_plan(LessonRole::Regular, 15, 1);
        assert_eq!(kinds(&short), vec![(UnitKind::Reading, 7, 4), (UnitKind::Review, 3, 12), (UnitKind::Conversation, 5, 10)]);
        for goal in [10, 15, 30, 45, 60] {
            for u in unit_plan(LessonRole::Regular, goal, 1).iter().chain(&unit_plan(LessonRole::Regular, goal, 2)) {
                if !matches!(u.kind, UnitKind::Review | UnitKind::Conversation) {
                    assert!(u.items >= 4, "{goal}: {:?} has {} items", u.kind, u.items);
                }
            }
        }
        let long = unit_plan(LessonRole::Regular, 45, 1);
        assert_eq!(long.len(), 5, "a second output task from 45 minutes");
        assert_eq!(long[4].kind, UnitKind::Cloze);
        assert_eq!(unit_plan(LessonRole::Regular, 60, 2)[4].kind, UnitKind::Paraphrase);
        // Beyond every cap the lesson stays at 60 minutes.
        assert_eq!(total_minutes(&unit_plan(LessonRole::Regular, 120, 1)), 60);
        assert_eq!(total_minutes(&unit_plan(LessonRole::Regular, 3, 1)), MIN_MINUTES);
    }

    #[test]
    fn two_lessons_a_day_above_sixty_minutes() {
        assert_eq!(daily_sessions(30), vec![30]);
        assert_eq!(daily_sessions(60), vec![60]);
        assert_eq!(daily_sessions(61), vec![31, 30]);
        assert_eq!(daily_sessions(90), vec![45, 45]);
        assert_eq!(daily_sessions(95), vec![48, 47]);
        assert_eq!(daily_sessions(120), vec![60, 60]);
        assert_eq!(daily_sessions(200), vec![60, 60]);
        assert_eq!(daily_sessions(5), vec![MIN_MINUTES]);
        for goal in 61..=120 {
            let s = daily_sessions(goal);
            assert_eq!(s.iter().sum::<u32>(), goal);
            for m in s {
                let units = unit_plan(LessonRole::Article, m, 1);
                assert!((30..=MAX_LESSON_MINUTES).contains(&m));
                assert_eq!(total_minutes(&units), m, "each lesson fills its sitting");
                assert!(units.iter().any(|u| u.kind == UnitKind::Conversation) && units.iter().any(|u| u.kind.is_output()));
            }
        }
    }

    #[test]
    fn review_and_other_roles() {
        let r = unit_plan(LessonRole::Review, 30, 5);
        assert_eq!(r.iter().map(|u| u.kind).collect::<Vec<_>>(), vec![UnitKind::Cloze, UnitKind::Review, UnitKind::SentenceMaking, UnitKind::Conversation]);
        assert_eq!(kinds(&unit_plan(LessonRole::Conversation, 30, 1)), vec![(UnitKind::Conversation, 30, 30)]);
        let c = unit_plan(LessonRole::Check, 30, 1);
        assert_eq!(total_minutes(&c), 30);
        assert_eq!(c[0].kind, UnitKind::Quiz);
    }
}
