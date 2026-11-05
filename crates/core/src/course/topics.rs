//! Interest keywords (`profiles/<id>/interests.csv`) and general topics
//! (`topics.toml`), and the deterministic choice between them for a day
//! (docs/plan.md §7, §14).

use crate::lang::Lang;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneralTopic {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub group: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TopicsFile {
    #[serde(default, rename = "topic")]
    pub topics: Vec<GeneralTopic>,
}

/// General topics from `topics.toml` text; empty when missing or broken.
pub fn parse_topics(text: &str) -> Vec<GeneralTopic> {
    toml::from_str::<TopicsFile>(text).map(|f| f.topics).unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Interest {
    pub keyword: String,
    /// `None` = both languages.
    pub lang: Option<Lang>,
    pub weight: f32,
}

/// `keyword,lang,weight,updated_at`; rows without a keyword or with a
/// weight ≤ 0 are left out.
pub fn parse_interests(text: &str) -> Vec<Interest> {
    let mut rdr = crate::import::reader(text);
    let Ok(headers) = rdr.headers().cloned() else { return vec![] };
    let col = |name: &str| headers.iter().position(|h| h.trim().trim_start_matches('\u{feff}').eq_ignore_ascii_case(name));
    let (Some(k), l, w) = (col("keyword"), col("lang"), col("weight")) else { return vec![] };
    rdr.records()
        .flatten()
        .filter_map(|r| {
            let keyword = r.get(k)?.trim().to_string();
            if keyword.is_empty() || keyword.starts_with('#') {
                return None;
            }
            let lang = l.and_then(|i| r.get(i)).and_then(|s| s.parse().ok());
            let weight = w.and_then(|i| r.get(i)).and_then(|s| s.trim().parse::<f32>().ok()).unwrap_or(1.0);
            (weight > 0.0).then_some(Interest { keyword, lang, weight })
        })
        .collect()
}

/// Interests that apply to a language, heaviest first (ties by keyword).
pub fn interests_for(all: &[Interest], lang: Lang) -> Vec<Interest> {
    let mut v: Vec<Interest> = all.iter().filter(|i| i.lang.is_none_or(|l| l == lang)).cloned().collect();
    v.sort_by(|a, b| b.weight.total_cmp(&a.weight).then_with(|| a.keyword.cmp(&b.keyword)));
    v
}

/// Keywords for a topic course: the request's own first, then the heaviest
/// interests, without duplicates, at most `max`.
pub fn topic_keywords(requested: &[String], interests: &[Interest], lang: Lang, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let candidates = requested.iter().cloned().chain(interests_for(interests, lang).into_iter().map(|i| i.keyword));
    for k in candidates {
        let k = k.trim().to_string();
        if !k.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(&k)) {
            out.push(k);
        }
        if out.len() >= max {
            break;
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TopicPick {
    Interest { keyword: String },
    General { id: String, name: String },
}

/// Is day `n` an interest day? Spreads interest days evenly: over any run
/// of days the share is `percent`% (rounded down at the start).
pub fn is_interest_day(n: u64, percent: u32) -> bool {
    let p = percent.min(100) as u64;
    (n + 1) * p / 100 - n * p / 100 == 1
}

/// Smooth weighted round robin: the `k`-th interest pick (0-based). Heavier
/// keywords come up proportionally more often, evenly spread.
fn weighted_pick(interests: &[Interest], k: u64) -> usize {
    let weights: Vec<i64> = interests.iter().map(|i| ((i.weight * 100.0).round() as i64).max(1)).collect();
    let total: i64 = weights.iter().sum();
    // The sequence repeats every `total` picks.
    let steps = k % total as u64;
    let mut current = vec![0i64; weights.len()];
    let mut chosen = 0;
    for _ in 0..=steps {
        for (c, w) in current.iter_mut().zip(&weights) {
            *c += w;
        }
        chosen = (0..current.len()).max_by(|&a, &b| current[a].cmp(&current[b]).then(b.cmp(&a))).unwrap_or(0);
        current[chosen] -= total;
    }
    chosen
}

/// The topic for day `day` (e.g. days since the epoch, plus a per-learner
/// seed). Interest days follow `interest_ratio`; general topics rotate in
/// file order so every topic comes up regularly. Deterministic: the same
/// inputs give the same topic. `None` when there are no topics at all.
pub fn pick_topic(day: u64, interest_ratio: f32, interests: &[Interest], general: &[GeneralTopic], lang: Lang) -> Option<TopicPick> {
    let mine = interests_for(interests, lang);
    let percent = (interest_ratio.clamp(0.0, 1.0) * 100.0).round() as u32;
    let interest_days_before = day * percent as u64 / 100;
    let want_interest = is_interest_day(day, percent);
    let interest = |k: u64| {
        let i = weighted_pick(&mine, k);
        TopicPick::Interest { keyword: mine[i].keyword.clone() }
    };
    let general_pick = |k: u64| {
        let g = &general[(k % general.len() as u64) as usize];
        TopicPick::General { id: g.id.clone(), name: g.name.clone() }
    };
    match (want_interest, mine.is_empty(), general.is_empty()) {
        (_, true, true) => None,
        // Only one kind exists: rotate through it every day.
        (_, false, true) => Some(interest(day)),
        (_, true, false) => Some(general_pick(day)),
        (true, false, false) => Some(interest(interest_days_before)),
        (false, false, false) => Some(general_pick(day - interest_days_before)),
    }
}

/// Day number for [`pick_topic`] from a date (days since 1970-01-01).
pub fn day_number(date: chrono::NaiveDate) -> u64 {
    let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch");
    (date - epoch).num_days().max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn general() -> Vec<GeneralTopic> {
        parse_topics(include_str!("../../../../workspace-template/topics.toml"))
    }

    fn interests() -> Vec<Interest> {
        parse_interests("keyword,lang,weight,updated_at\nAI chips,en,2,\nsemiconductor,,1,\n料理,ja,1,\nold,,0,\n")
    }

    #[test]
    fn reads_files() {
        let g = general();
        assert!(g.len() >= 10, "the template's general topics");
        assert_eq!(g[0].id, "daily-life");
        let i = interests();
        assert_eq!(i.len(), 3, "weight 0 is left out");
        assert_eq!(i[1].lang, None);
        assert_eq!(interests_for(&i, Lang::En).iter().map(|i| i.keyword.as_str()).collect::<Vec<_>>(), vec!["AI chips", "semiconductor"]);
        assert_eq!(topic_keywords(&["Robots".into(), "ai chips".into()], &i, Lang::En, 3), vec!["Robots", "ai chips", "semiconductor"]);
    }

    #[test]
    fn interest_share_follows_ratio() {
        let (i, g) = (interests(), general());
        let picks: Vec<TopicPick> = (0..100).map(|d| pick_topic(d, 0.6, &i, &g, Lang::En).unwrap()).collect();
        let n = picks.iter().filter(|p| matches!(p, TopicPick::Interest { .. })).count();
        assert_eq!(n, 60);
        // No long runs of one kind at 60 %.
        let longest_general = picks.split(|p| matches!(p, TopicPick::Interest { .. })).map(|r| r.len()).max().unwrap();
        assert!(longest_general <= 1);
        // Weight 2 comes up twice as often as weight 1.
        let chips = picks.iter().filter(|p| matches!(p, TopicPick::Interest { keyword } if keyword == "AI chips")).count();
        assert_eq!(chips, 40);
        // Every general topic comes up in turn.
        let general_ids: Vec<&str> = picks.iter().filter_map(|p| if let TopicPick::General { id, .. } = p { Some(id.as_str()) } else { None }).collect();
        assert_eq!(general_ids[0], g[0].id);
        assert_eq!(general_ids[1], g[1].id);
        assert!(g.iter().all(|t| general_ids.contains(&t.id.as_str())));
    }

    #[test]
    fn deterministic_and_edge_cases() {
        let (i, g) = (interests(), general());
        let day = day_number(chrono::NaiveDate::from_ymd_opt(2026, 11, 5).unwrap());
        assert_eq!(pick_topic(day, 0.6, &i, &g, Lang::En), pick_topic(day, 0.6, &i, &g, Lang::En));
        assert!((0..20).all(|d| matches!(pick_topic(d, 0.0, &i, &g, Lang::En), Some(TopicPick::General { .. }))));
        assert!((0..20).all(|d| matches!(pick_topic(d, 1.0, &i, &g, Lang::En), Some(TopicPick::Interest { .. }))));
        assert!((0..20).all(|d| matches!(pick_topic(d, 0.6, &[], &g, Lang::En), Some(TopicPick::General { .. }))));
        assert!((0..20).all(|d| matches!(pick_topic(d, 0.0, &i, &[], Lang::Ja), Some(TopicPick::Interest { .. }))));
        assert_eq!(pick_topic(3, 0.6, &[], &[], Lang::En), None);
    }
}
