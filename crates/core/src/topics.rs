//! Topics: the learner's interests and the workspace's general topics
//! (docs/plan.md §7). Conversations mix them by `interest_ratio`; general
//! topics take turns so each comes up regularly.

use crate::lang::Lang;
use crate::workspace::slug;
use serde::{Deserialize, Serialize};
use std::path::Path;

const BUNDLED: &str = include_str!("../../../workspace-template/topics.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TopicKind {
    Interest,
    General,
}

impl TopicKind {
    pub fn code(self) -> &'static str {
        match self {
            TopicKind::Interest => "interest",
            TopicKind::General => "general",
        }
    }
    pub fn parse(s: &str) -> TopicKind {
        if s == "interest" { TopicKind::Interest } else { TopicKind::General }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Topic {
    pub id: String,
    pub name: String,
    pub kind: TopicKind,
    #[serde(default)]
    pub group: String,
}

#[derive(Deserialize)]
struct TopicsFile {
    #[serde(default, rename = "topic")]
    topics: Vec<GeneralTopic>,
}

#[derive(Deserialize)]
struct GeneralTopic {
    id: String,
    name: String,
    #[serde(default)]
    group: String,
}

fn parse_general(raw: &str) -> Result<Vec<Topic>, String> {
    let f: TopicsFile = toml::from_str(raw).map_err(|e| e.to_string())?;
    Ok(f.topics
        .into_iter()
        .filter(|t| !t.id.trim().is_empty() && !t.name.trim().is_empty())
        .map(|t| Topic { id: t.id, name: t.name, kind: TopicKind::General, group: t.group })
        .collect())
}

/// General topics from the workspace's `topics.toml`; the bundled list when
/// the file is missing or broken (with the reason).
pub fn load_general(file: &Path) -> (Vec<Topic>, Option<String>) {
    match std::fs::read_to_string(file) {
        Ok(raw) => match parse_general(&raw) {
            Ok(t) if !t.is_empty() => (t, None),
            Ok(_) => (parse_general(BUNDLED).unwrap_or_default(), Some("topics.toml 沒有主題，改用內建清單".into())),
            Err(e) => (parse_general(BUNDLED).unwrap_or_default(), Some(format!("topics.toml：{e}"))),
        },
        Err(_) => (parse_general(BUNDLED).unwrap_or_default(), None),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Interest {
    pub keyword: String,
    /// `None` = both languages.
    pub lang: Option<Lang>,
    pub weight: f64,
}

/// `interests.csv`: keyword,lang,weight,updated_at.
pub fn load_interests(file: &Path) -> Vec<Interest> {
    let Ok(text) = std::fs::read_to_string(file) else { return vec![] };
    let mut rdr = csv::ReaderBuilder::new().flexible(true).trim(csv::Trim::All).from_reader(text.as_bytes());
    rdr.records()
        .flatten()
        .filter_map(|r| {
            let k = r.get(0)?.trim();
            if k.is_empty() || k.starts_with('#') {
                return None;
            }
            Some(Interest {
                keyword: k.to_string(),
                lang: r.get(1).and_then(|l| l.parse().ok()),
                weight: r.get(2).and_then(|w| w.parse().ok()).unwrap_or(1.0),
            })
        })
        .collect()
}

pub fn interest_topic(i: &Interest) -> Topic {
    Topic { id: format!("interest:{}", slug(&i.keyword, 40)), name: i.keyword.clone(), kind: TopicKind::Interest, group: String::new() }
}

/// Choose the next topic.
///
/// `history`: topic ids and kinds of recent conversations, newest first.
/// The kind is chosen so the share of interest topics in the last ten
/// (including this one) stays as close as possible to `ratio`; within the
/// kind, the topic used longest ago (never used first), then the heavier
/// interest, then file order.
pub fn pick(general: &[Topic], interests: &[Interest], lang: Lang, ratio: f32, history: &[(String, TopicKind)]) -> Option<Topic> {
    let mine: Vec<&Interest> = interests.iter().filter(|i| i.lang.is_none_or(|l| l == lang) && i.weight > 0.0).collect();
    let recent = &history[..history.len().min(9)];
    let n = recent.len() as f64 + 1.0;
    let i = recent.iter().filter(|(_, k)| *k == TopicKind::Interest).count() as f64;
    let r = ratio.clamp(0.0, 1.0) as f64;
    let want_interest = ((i + 1.0) / n - r).abs() <= (i / n - r).abs();
    let kind = match (mine.is_empty(), general.is_empty()) {
        (true, true) => return None,
        (true, false) => TopicKind::General,
        (false, true) => TopicKind::Interest,
        (false, false) if want_interest && r > 0.0 => TopicKind::Interest,
        _ => TopicKind::General,
    };
    // How long ago a topic was used: its position in the history (never = ∞).
    let age = |id: &str| history.iter().position(|(h, _)| h == id).unwrap_or(usize::MAX);
    match kind {
        TopicKind::General => general.iter().enumerate().max_by_key(|(idx, t)| (age(&t.id), std::cmp::Reverse(*idx))).map(|(_, t)| t.clone()),
        TopicKind::Interest => {
            let cands: Vec<Topic> = mine.iter().map(|i| interest_topic(i)).collect();
            cands
                .iter()
                .enumerate()
                .max_by(|(ia, a), (ib, b)| {
                    age(&a.id)
                        .cmp(&age(&b.id))
                        .then(mine[*ia].weight.partial_cmp(&mine[*ib].weight).unwrap_or(std::cmp::Ordering::Equal))
                        .then(ib.cmp(ia))
                })
                .map(|(_, t)| t.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn general() -> Vec<Topic> {
        parse_general(BUNDLED).unwrap()
    }

    #[test]
    fn bundled_general_topics_match_the_plan() {
        let g = general();
        assert_eq!(g.len(), 16);
        assert!(g.iter().any(|t| t.name == "校園與社團"));
        assert!(g.iter().all(|t| t.kind == TopicKind::General));
    }

    #[test]
    fn ratio_and_rotation() {
        let g = general();
        let interests = vec![
            Interest { keyword: "AI chips".into(), lang: None, weight: 2.0 },
            Interest { keyword: "咖啡".into(), lang: None, weight: 1.0 },
            Interest { keyword: "アニメ".into(), lang: Some(Lang::Ja), weight: 1.0 },
        ];
        let mut history: Vec<(String, TopicKind)> = vec![];
        let mut picked = vec![];
        for _ in 0..10 {
            let t = pick(&g, &interests, Lang::En, 0.6, &history).unwrap();
            history.insert(0, (t.id.clone(), t.kind));
            picked.push(t);
        }
        let interest = picked.iter().filter(|t| t.kind == TopicKind::Interest).count();
        assert_eq!(interest, 6, "60% interest over ten conversations");
        assert_eq!(picked[0].name, "AI chips", "heavier interest first");
        assert!(!picked.iter().any(|t| t.name == "アニメ"), "a Japanese-only interest is not used for English");
        let generals: Vec<&str> = picked.iter().filter(|t| t.kind == TopicKind::General).map(|t| t.id.as_str()).collect();
        assert_eq!(generals, vec!["daily-life", "school", "food", "travel"], "general topics take turns in file order");
        let only_general = pick(&g, &[], Lang::En, 0.6, &[]).unwrap();
        assert_eq!(only_general.kind, TopicKind::General);
        let all_general = pick(&g, &interests, Lang::En, 0.0, &[]).unwrap();
        assert_eq!(all_general.kind, TopicKind::General, "ratio 0 = general only");
    }

    #[test]
    fn interests_csv() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("interests.csv");
        std::fs::write(&f, "keyword,lang,weight,updated_at\n半導體,,1,2026-11-05T12:00:00+08:00\nアニメ,ja,2,\n").unwrap();
        let i = load_interests(&f);
        assert_eq!(i.len(), 2);
        assert_eq!(i[1].lang, Some(Lang::Ja));
        assert_eq!(i[1].weight, 2.0);
        assert_eq!(interest_topic(&i[0]).id, "interest:半導體");
    }
}
