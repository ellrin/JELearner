//! Points, level and difficulty rules (docs/plan.md §5.4, §8, §9.1).
//!
//! The numbers live in `config/progress.toml` (bundled); a workspace may
//! override any key with a `progress.toml` at its top level. Nothing here is
//! hard-coded in the logic that uses it.

use crate::lang::Lang;
use crate::level;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const FILE: &str = "progress.toml";
const BUNDLED: &str = include_str!("../../../config/progress.toml");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointRules {
    pub pass_score: f64,
    pub bonus_score: f64,
    pub bonus: f64,
    pub passive_minutes: f64,
    pub review_round: i64,
    pub conversation_pass: f64,
    pub minutes_per_point: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssessmentRules {
    pub output_share: f64,
    pub recent_units: usize,
    pub recent_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DifficultyRules {
    pub early_easy: i32,
    pub early_hard: i32,
    pub window: usize,
    pub up_above: f64,
    pub down_below: f64,
    pub step: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rules {
    pub points: PointRules,
    pub assessment: AssessmentRules,
    pub difficulty: DifficultyRules,
    /// Points per major level, per language.
    pub levels: BTreeMap<Lang, BTreeMap<String, f64>>,
}

fn merge(base: &mut toml::Table, over: toml::Table) {
    for (k, v) in over {
        match (base.get_mut(&k), v) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => merge(b, o),
            (_, v) => {
                base.insert(k, v);
            }
        }
    }
}

impl Rules {
    pub fn bundled() -> Rules {
        toml::from_str(BUNDLED).expect("bundled config/progress.toml is valid")
    }

    /// Bundled rules with the workspace's `progress.toml` laid over them.
    /// A broken override is ignored and reported.
    pub fn load(workspace_root: Option<&Path>) -> (Rules, Option<String>) {
        let Some(raw) = workspace_root.and_then(|r| std::fs::read_to_string(r.join(FILE)).ok()) else {
            return (Rules::bundled(), None);
        };
        let mut base: toml::Table = BUNDLED.parse().expect("bundled rules parse");
        let res = raw
            .parse::<toml::Table>()
            .map_err(|e| e.to_string())
            .and_then(|over| {
                merge(&mut base, over);
                toml::Value::Table(base).try_into::<Rules>().map_err(|e| e.to_string())
            });
        match res {
            Ok(r) => (r, None),
            Err(e) => (Rules::bundled(), Some(format!("{FILE}：{e}（改用內建數值）"))),
        }
    }

    /// Points needed for one step of `major` (the major level's points
    /// divided evenly over its steps).
    pub fn step_points(&self, lang: Lang, major: &str) -> f64 {
        let total = self.levels.get(&lang).and_then(|m| m.get(major)).copied().unwrap_or(1500.0);
        total / level::steps_in(lang, major) as f64
    }

    /// Daily goal in points for a daily goal in minutes.
    pub fn daily_goal(&self, minutes: u32) -> f64 {
        (minutes as f64 / self.points.minutes_per_point.max(1.0)).round().max(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_has_every_major_and_plan_numbers() {
        let r = Rules::bundled();
        for lang in Lang::ALL {
            for m in level::majors(lang) {
                assert!(r.levels[&lang].contains_key(*m), "{lang} {m}");
            }
        }
        assert_eq!(r.step_points(Lang::En, "B1"), 375.0);
        assert_eq!(r.step_points(Lang::Ja, "N5"), 350.0, "N5 has six steps");
        assert_eq!(r.daily_goal(30), 3.0);
    }

    #[test]
    fn workspace_overrides_single_keys() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "[points]\nbonus = 0\n[levels.en]\nB1 = 400\n").unwrap();
        let (r, err) = Rules::load(Some(dir.path()));
        assert!(err.is_none());
        assert_eq!(r.points.bonus, 0.0);
        assert_eq!(r.points.pass_score, 0.7, "untouched keys keep the bundled value");
        assert_eq!(r.step_points(Lang::En, "B1"), 100.0);
        assert_eq!(r.step_points(Lang::En, "B2"), 375.0);
        std::fs::write(dir.path().join(FILE), "[points]\nbonus = \"x\"\n").unwrap();
        let (r, err) = Rules::load(Some(dir.path()));
        assert!(err.is_some());
        assert_eq!(r.points.bonus, 0.5);
    }
}
