//! Fine-grained levels (docs/plan.md §5).
//!
//! Internally a **difficulty score**, 100 points per major level:
//! English A1–C2 = 0–600, Japanese N5–N1 = 0–500. Shown as
//! "major-step", e.g. `B1-3` or `N5-2`. Each major level has 4 steps,
//! except N5 which has 6 (more frequent progress at the very start).
//!
//! The same scale serves two things that are kept apart: the *content
//! difficulty* (adjusts both ways) and the *level* (only goes up after an
//! assessment).

use crate::lang::Lang;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Level {
    /// Major level name, e.g. `B1`, `N5`.
    pub major: &'static str,
    /// 1-based step within the major level.
    pub step: u32,
    pub steps: u32,
    /// `B1-3`
    pub label: String,
    /// Score at the start of this step.
    pub score: u32,
}

/// Major levels from easiest to hardest.
pub fn majors(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::En => &["A1", "A2", "B1", "B2", "C1", "C2"],
        Lang::Ja => &["N5", "N4", "N3", "N2", "N1"],
    }
}

pub fn steps_in(lang: Lang, major: &str) -> u32 {
    if lang == Lang::Ja && major == "N5" { 6 } else { 4 }
}

pub fn max_score(lang: Lang) -> u32 {
    majors(lang).len() as u32 * 100 - 1
}

/// Score at the start of a major level, e.g. `B1` → 200.
pub fn major_start(lang: Lang, major: &str) -> Option<u32> {
    majors(lang).iter().position(|m| m.eq_ignore_ascii_case(major)).map(|i| i as u32 * 100)
}

pub fn from_score(lang: Lang, score: u32) -> Level {
    let score = score.min(max_score(lang));
    let i = (score / 100) as usize;
    let major = majors(lang)[i];
    let steps = steps_in(lang, major);
    let within = score % 100;
    let step = (within * steps / 100) + 1;
    let start = i as u32 * 100 + step_offset(steps, step);
    Level { major, step, steps, label: format!("{major}-{step}"), score: start }
}

/// Parse `B1-3`, `N5-2`, or a bare major level (`B1` = its first step).
pub fn parse(lang: Lang, label: &str) -> Option<Level> {
    let label = label.trim().to_ascii_uppercase();
    let (major, step) = match label.split_once('-') {
        Some((m, s)) => (m.to_string(), s.parse::<u32>().ok()?),
        None => (label.clone(), 1),
    };
    let start = major_start(lang, &major)?;
    let steps = steps_in(lang, &major);
    if step == 0 || step > steps {
        return None;
    }
    Some(from_score(lang, start + step_offset(steps, step)))
}

/// First score of a step within its major level (rounded up, so the score
/// maps back to the same step).
fn step_offset(steps: u32, step: u32) -> u32 {
    ((step - 1) * 100).div_ceil(steps)
}

/// How the level is described to the model in prompts.
pub fn describe(lang: Lang, l: &Level) -> String {
    let scale = match lang {
        Lang::En => "CEFR",
        Lang::Ja => "JLPT",
    };
    format!("{scale} {} (step {} of {} within {})", l.major, l.step, l.steps, l.major)
}

/// One-line self-assessment help for the start-up wizard.
pub fn self_assessment_hint(lang: Lang, major: &str) -> &'static str {
    match (lang, major) {
        (Lang::En, "A1") => "認得基本單字，能用很簡單的句子自我介紹",
        (Lang::En, "A2") => "能處理日常簡單對話，例如購物、問路",
        (Lang::En, "B1") => "能聊熟悉的話題、看懂簡單新聞，但常卡詞",
        (Lang::En, "B2") => "能和母語者流暢交談一般話題，看懂大部分新聞",
        (Lang::En, "C1") => "能自然表達複雜想法，工作與學術場合都能用",
        (Lang::En, "C2") => "接近母語者，細微語氣也能掌握",
        (Lang::Ja, "N5") => "會假名與基本單字，能讀很簡單的句子",
        (Lang::Ja, "N4") => "能理解日常基本對話與簡單文章",
        (Lang::Ja, "N3") => "能理解日常場合中較自然的日文",
        (Lang::Ja, "N2") => "能看懂新聞與一般文章，能應付大部分對話",
        (Lang::Ja, "N1") => "能理解各種場合的日文，包括抽象與專業內容",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_map_to_labels() {
        assert_eq!(from_score(Lang::En, 0).label, "A1-1");
        assert_eq!(from_score(Lang::En, 260).label, "B1-3");
        assert_eq!(from_score(Lang::En, 9999).label, "C2-4");
        assert_eq!(from_score(Lang::Ja, 0).label, "N5-1");
        assert_eq!(from_score(Lang::Ja, 99).label, "N5-6");
        assert_eq!(from_score(Lang::Ja, 100).label, "N4-1");
    }

    #[test]
    fn parse_round_trips() {
        for lang in Lang::ALL {
            for score in 0..=max_score(lang) {
                let l = from_score(lang, score);
                assert_eq!(parse(lang, &l.label).unwrap().label, l.label);
            }
        }
        assert_eq!(parse(Lang::En, "b2").unwrap().score, 300);
        assert!(parse(Lang::Ja, "N5-7").is_none());
        assert!(parse(Lang::Ja, "B1").is_none());
        assert!(describe(Lang::Ja, &parse(Lang::Ja, "N5-2").unwrap()).contains("step 2 of 6"));
    }
}
