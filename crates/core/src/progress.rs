//! Level assessment (docs/plan.md §9) and content difficulty (§5.4).
//!
//! The level only goes up. A step within a major level is reached when
//! conditions 1–3 hold: (1) points since the current step reach the step's
//! share of the major level's points, (2) speaking + writing make up enough
//! of those points, (3) the recent units scored well. Crossing into the next
//! major level also needs the major-level check (stage B), so it is never
//! automatic here.
//!
//! The content difficulty moves both ways: ±15 from the "too easy / just
//! right / too hard" answers after the first lessons, then ±5 from recent
//! quiz scores.

use crate::lang::Lang;
use crate::level;
use crate::points::SkillPoints;
use crate::rules::{DifficultyRules, Rules};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Condition {
    /// `points` | `balance` | `recent` | `major_check`
    pub id: &'static str,
    pub ok: bool,
    pub value: f64,
    pub need: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Assessment {
    pub level: String,
    /// The next step, if any.
    pub next: Option<String>,
    /// The next step is in the next major level (needs the major check).
    pub major_boundary: bool,
    pub points_since: f64,
    pub step_points: f64,
    pub conditions: Vec<Condition>,
    /// Conditions 1–3 hold and the next step is within the major level.
    pub eligible: bool,
}

/// The step after `label`, and whether it starts a new major level.
pub fn next_step(lang: Lang, label: &str) -> Option<(String, bool)> {
    let cur = level::parse(lang, label)?;
    if cur.step < cur.steps {
        return Some((format!("{}-{}", cur.major, cur.step + 1), false));
    }
    let majors = level::majors(lang);
    let i = majors.iter().position(|m| *m == cur.major)?;
    majors.get(i + 1).map(|m| (format!("{m}-1"), true))
}

/// Assess the level `label` from the points earned since it was reached and
/// the most recent unit scores (newest first).
pub fn assess(lang: Lang, label: &str, since: &SkillPoints, recent: &[f64], rules: &Rules) -> Assessment {
    let cur = level::parse(lang, label).unwrap_or_else(|| level::from_score(lang, 0));
    let step_points = rules.step_points(lang, cur.major);
    let total = since.total();
    let a = &rules.assessment;
    let window: Vec<f64> = recent.iter().take(a.recent_units).copied().collect();
    let avg = if window.is_empty() { 0.0 } else { window.iter().sum::<f64>() / window.len() as f64 };
    let next = next_step(lang, &cur.label);
    let major_boundary = next.as_ref().is_some_and(|(_, m)| *m);
    let mut conditions = vec![
        Condition { id: "points", ok: total + 1e-9 >= step_points, value: total, need: step_points },
        Condition { id: "balance", ok: total > 0.0 && since.output_share() + 1e-9 >= a.output_share, value: since.output_share(), need: a.output_share },
        Condition { id: "recent", ok: window.len() >= a.recent_units && avg + 1e-9 >= a.recent_score, value: avg, need: a.recent_score },
    ];
    if major_boundary {
        conditions.push(Condition { id: "major_check", ok: false, value: 0.0, need: 1.0 });
    }
    let eligible = next.is_some() && !major_boundary && conditions.iter().all(|c| c.ok);
    Assessment { level: cur.label, next: next.map(|(n, _)| n), major_boundary, points_since: total, step_points, conditions, eligible }
}

/// The learner's answer to "how was this lesson?".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feedback {
    TooEasy,
    Right,
    TooHard,
}

pub fn early_delta(fb: Feedback, r: &DifficultyRules) -> i32 {
    match fb {
        Feedback::TooEasy => r.early_easy,
        Feedback::Right => 0,
        Feedback::TooHard => -r.early_hard,
    }
}

/// Change from recent quiz scores (newest first, only those since the last
/// change): needs a full window, then up above `up_above`, down below
/// `down_below`.
pub fn accuracy_delta(scores_since_change: &[f64], r: &DifficultyRules) -> i32 {
    if r.window == 0 || scores_since_change.len() < r.window {
        return 0;
    }
    let w = &scores_since_change[..r.window];
    let avg = w.iter().sum::<f64>() / w.len() as f64;
    if avg > r.up_above {
        r.step
    } else if avg < r.down_below {
        -r.step
    } else {
        0
    }
}

/// A difficulty score moved by `delta`, kept within the language's scale.
pub fn apply(lang: Lang, score: u32, delta: i32) -> u32 {
    (score as i64 + delta as i64).clamp(0, level::max_score(lang) as i64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sp(writing: f64, reading: f64) -> SkillPoints {
        SkillPoints { writing, reading, ..Default::default() }
    }

    #[test]
    fn steps_and_major_boundaries() {
        assert_eq!(next_step(Lang::En, "B1-1"), Some(("B1-2".into(), false)));
        assert_eq!(next_step(Lang::En, "B1-4"), Some(("B2-1".into(), true)));
        assert_eq!(next_step(Lang::Ja, "N5-5"), Some(("N5-6".into(), false)));
        assert_eq!(next_step(Lang::Ja, "N5-6"), Some(("N4-1".into(), true)));
        assert_eq!(next_step(Lang::En, "C2-4"), None);
    }

    #[test]
    fn step_needs_all_three_conditions() {
        let r = Rules::bundled();
        let good = [0.9, 0.8, 0.85, 0.9, 0.8];
        let a = assess(Lang::En, "B1-1", &sp(150.0, 250.0), &good, &r);
        assert_eq!(a.step_points, 375.0);
        assert!(a.eligible, "{a:?}");
        assert_eq!(a.next.as_deref(), Some("B1-2"));

        let few = assess(Lang::En, "B1-1", &sp(150.0, 200.0), &good, &r);
        assert!(!few.eligible && !few.conditions[0].ok, "350 < 375 points");
        let unbalanced = assess(Lang::En, "B1-1", &sp(50.0, 400.0), &good, &r);
        assert!(!unbalanced.eligible && !unbalanced.conditions[1].ok, "output 11% < 30%");
        let weak = assess(Lang::En, "B1-1", &sp(150.0, 250.0), &[0.5, 0.6, 0.7, 0.9, 0.9], &r);
        assert!(!weak.eligible && !weak.conditions[2].ok, "recent average 0.72 < 0.75");
        let short = assess(Lang::En, "B1-1", &sp(150.0, 250.0), &[1.0, 1.0], &r);
        assert!(!short.conditions[2].ok, "too few recent units");
    }

    #[test]
    fn last_step_of_a_major_waits_for_the_major_check() {
        let r = Rules::bundled();
        let a = assess(Lang::Ja, "N5-6", &sp(200.0, 300.0), &[1.0; 5], &r);
        assert!(a.major_boundary);
        assert!(!a.eligible);
        assert_eq!(a.conditions.last().unwrap().id, "major_check");
    }

    #[test]
    fn difficulty_moves_both_ways_within_scale() {
        let r = Rules::bundled().difficulty;
        assert_eq!(early_delta(Feedback::TooEasy, &r), 15);
        assert_eq!(early_delta(Feedback::Right, &r), 0);
        assert_eq!(early_delta(Feedback::TooHard, &r), -15);
        assert_eq!(accuracy_delta(&[0.95; 4], &r), 0, "needs a full window");
        assert_eq!(accuracy_delta(&[0.95, 0.92, 1.0, 0.9, 0.96], &r), 5);
        assert_eq!(accuracy_delta(&[0.5, 0.6, 0.7, 0.6, 0.6, 1.0], &r), -5, "only the newest window counts");
        assert_eq!(accuracy_delta(&[0.8; 5], &r), 0);
        assert_eq!(apply(Lang::Ja, 5, -15), 0);
        assert_eq!(apply(Lang::Ja, 495, 15), 499);
        assert_eq!(apply(Lang::En, 255, 15), 270);
    }
}
