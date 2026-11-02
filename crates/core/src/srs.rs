//! Spaced repetition: a small SM-2 variant.
//!
//! Grades: 1 = 忘了, 3 = 模糊, 4 = 記得, 5 = 很熟. Below 3 counts as a lapse
//! and the card comes back tomorrow.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Card {
    pub ease: f64,
    pub interval_days: i64,
    pub reps: i64,
    pub lapses: i64,
}

impl Default for Card {
    fn default() -> Self {
        Card { ease: 2.5, interval_days: 0, reps: 0, lapses: 0 }
    }
}

pub fn review(card: Card, grade: u8) -> Card {
    let g = grade.min(5) as f64;
    let ease = (card.ease + 0.1 - (5.0 - g) * (0.08 + (5.0 - g) * 0.02)).max(1.3);
    if grade < 3 {
        return Card { ease, interval_days: 1, reps: 0, lapses: card.lapses + 1 };
    }
    let reps = card.reps + 1;
    let interval_days = match reps {
        1 => 1,
        2 => 3,
        _ => ((card.interval_days.max(1) as f64) * ease).round() as i64,
    };
    Card { ease, interval_days, reps, lapses: card.lapses }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals_grow_and_lapses_reset() {
        let mut c = Card::default();
        let mut seen = vec![];
        for _ in 0..4 {
            c = review(c, 4);
            seen.push(c.interval_days);
        }
        assert_eq!(&seen[..2], &[1, 3]);
        assert!(seen[2] > 3 && seen[3] > seen[2]);
        let lapsed = review(c, 1);
        assert_eq!((lapsed.interval_days, lapsed.reps, lapsed.lapses), (1, 0, 1));
        assert!(lapsed.ease >= 1.3 && lapsed.ease < c.ease);
    }
}
