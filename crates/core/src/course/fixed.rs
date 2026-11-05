//! Fixed courses: an uploaded word list and/or grammar list split into
//! lessons. The split needs no model; titles and angles may be written by
//! the model afterwards ([`super::outline::describe_fixed`]) and texts when
//! each lesson is opened.

use super::model::{LessonPlan, LessonRole, TargetGrammar, TargetWord};
use super::sizing::{unit_plan, total_minutes, ContentSize};
use crate::import::{field, header_map, reader, GRAMMAR_ALIASES, WORD_ALIASES};

/// Default position of review lessons: every 5th lesson.
pub const REVIEW_EVERY: u32 = 5;

/// Words of a vocab CSV (same columns as `vocab/*.csv`), in file order,
/// without duplicates. Unknown or missing columns are tolerated; a file
/// without a term column gives nothing.
pub fn read_word_list(text: &str) -> Vec<TargetWord> {
    let mut rdr = reader(text);
    let Ok(h) = rdr.headers().cloned() else { return vec![] };
    let map = header_map(&h, WORD_ALIASES);
    if !map.contains_key("term") {
        // A bare list: one word per line, no header (and no columns).
        if text.lines().any(|l| l.contains(',') || l.contains('\t')) {
            return vec![];
        }
        return dedup_words(
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|l| TargetWord { term: l.to_string(), ..Default::default() })
                .collect(),
        );
    }
    let words = rdr
        .records()
        .flatten()
        .map(|r| TargetWord { term: field(&r, &map, "term"), reading: field(&r, &map, "reading"), meaning: field(&r, &map, "meaning") })
        .filter(|w| !w.term.is_empty() && !w.term.starts_with('#'))
        .collect();
    dedup_words(words)
}

/// Grammar points of a grammar CSV (same columns as `grammar/*.csv`).
pub fn read_grammar_list(text: &str) -> Vec<TargetGrammar> {
    let mut rdr = reader(text);
    let Ok(h) = rdr.headers().cloned() else { return vec![] };
    let map = header_map(&h, GRAMMAR_ALIASES);
    if !map.contains_key("pattern") {
        return vec![];
    }
    let mut out: Vec<TargetGrammar> = Vec::new();
    for r in rdr.records().flatten() {
        let g = TargetGrammar { pattern: field(&r, &map, "pattern"), meaning: field(&r, &map, "meaning") };
        if !g.pattern.is_empty() && !g.pattern.starts_with('#') && !out.iter().any(|o| o.pattern == g.pattern) {
            out.push(g);
        }
    }
    out
}

fn dedup_words(words: Vec<TargetWord>) -> Vec<TargetWord> {
    let mut out: Vec<TargetWord> = Vec::new();
    for w in words {
        if !out.iter().any(|o| o.term.eq_ignore_ascii_case(&w.term) && o.reading == w.reading) {
            out.push(w);
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct SplitOptions {
    /// New words per lesson (0 = the level table's `fixed_new_words`).
    pub new_per_lesson: u32,
    /// A review lesson at every n-th position (0 = none).
    pub review_every: u32,
    pub daily_minutes: u32,
    /// Words the learner already knows: left out of the course.
    pub known: Vec<String>,
}

/// Split a word list and a grammar list into lessons.
///
/// - Word lessons: `ceil(words / per_lesson)`, or more when the grammar
///   needs them (at most `size.grammar.max` points per lesson). 300 English
///   B1 words → 30 word lessons.
/// - Grammar is spread evenly over the word lessons (earlier lessons get
///   the extra point when it does not divide).
/// - Every `review_every`-th position is a review lesson for the words and
///   grammar since the previous review (positions 5, 10, …); a last review
///   follows when the course does not end on one.
/// - Each word lesson also reviews the words of the word lessons one and
///   three before it (spaced), up to `per_lesson` words.
pub fn split(words: &[TargetWord], grammar: &[TargetGrammar], size: &ContentSize, opt: &SplitOptions) -> Vec<LessonPlan> {
    let known: Vec<String> = opt.known.iter().map(|k| k.to_lowercase()).collect();
    let words: Vec<TargetWord> = words.iter().filter(|w| !known.contains(&w.term.to_lowercase())).cloned().collect();
    let per = if opt.new_per_lesson == 0 { size.fixed_new_words } else { opt.new_per_lesson }.max(1) as usize;
    let gmax = size.grammar.max.max(1) as usize;
    let content = words.len().div_ceil(per).max(grammar.len().div_ceil(gmax));
    if content == 0 {
        return vec![];
    }
    let word_chunks: Vec<Vec<TargetWord>> = (0..content).map(|i| words.iter().skip(i * per).take(per).cloned().collect()).collect();
    // Even spread: lesson i gets grammar[g*i/c .. g*(i+1)/c], extra points first.
    let g = grammar.len();
    let grammar_chunks: Vec<Vec<TargetGrammar>> = (0..content)
        .map(|i| {
            let start = (g * i).div_ceil(content);
            let end = (g * (i + 1)).div_ceil(content);
            grammar[start..end].to_vec()
        })
        .collect();

    let mut out: Vec<LessonPlan> = Vec::new();
    let mut since_review: (Vec<String>, Vec<TargetGrammar>) = (vec![], vec![]);
    let push_review = |out: &mut Vec<LessonPlan>, since: &mut (Vec<String>, Vec<TargetGrammar>)| {
        let number = out.len() as u32 + 1;
        let units = unit_plan(LessonRole::Review, opt.daily_minutes, number);
        out.push(LessonPlan {
            number,
            role: LessonRole::Review,
            title: format!("第 {number} 課：複習"),
            purpose: "複習前幾課的單字與文法".into(),
            minutes: total_minutes(&units),
            review: std::mem::take(&mut since.0),
            grammar: std::mem::take(&mut since.1),
            units,
            ..Default::default()
        });
    };
    for (i, (ws, gs)) in word_chunks.into_iter().zip(grammar_chunks).enumerate() {
        if opt.review_every > 1 && (out.len() as u32 + 1).is_multiple_of(opt.review_every) {
            push_review(&mut out, &mut since_review);
        }
        let number = out.len() as u32 + 1;
        let mut review: Vec<String> = Vec::new();
        for back in [1, 3] {
            if i >= back {
                let earlier = &words[((i - back) * per).min(words.len())..((i - back + 1) * per).min(words.len())];
                review.extend(earlier.iter().map(|w| w.term.clone()));
            }
        }
        review.truncate(per);
        let units = unit_plan(LessonRole::Regular, opt.daily_minutes, number);
        let title = {
            let first: Vec<&str> = ws.iter().take(3).map(|w| w.term.as_str()).collect();
            let gp: Vec<&str> = gs.iter().map(|g| g.pattern.as_str()).collect();
            let what = if first.is_empty() { gp.join("、") } else { first.join("、") };
            format!("第 {number} 課：{what}")
        };
        since_review.0.extend(ws.iter().map(|w| w.term.clone()));
        since_review.1.extend(gs.iter().cloned());
        out.push(LessonPlan {
            number,
            role: LessonRole::Regular,
            title,
            minutes: total_minutes(&units),
            words: ws,
            review,
            grammar: gs,
            units,
            ..Default::default()
        });
    }
    if opt.review_every > 1 && (!since_review.0.is_empty() || !since_review.1.is_empty()) {
        push_review(&mut out, &mut since_review);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::sizing::content_size;
    use crate::lang::Lang;

    fn opt() -> SplitOptions {
        SplitOptions { new_per_lesson: 0, review_every: REVIEW_EVERY, daily_minutes: 30, known: vec![] }
    }

    fn words(n: usize) -> Vec<TargetWord> {
        (0..n).map(|i| TargetWord { term: format!("word{i:03}"), ..Default::default() }).collect()
    }

    #[test]
    fn three_hundred_english_words() {
        let size = content_size(Lang::En, 200); // B1
        let grammar: Vec<TargetGrammar> = (0..45).map(|i| TargetGrammar { pattern: format!("g{i}"), ..Default::default() }).collect();
        let plan = split(&words(300), &grammar, &size, &opt());
        let word_lessons: Vec<&LessonPlan> = plan.iter().filter(|l| l.role == LessonRole::Regular).collect();
        assert_eq!(word_lessons.len(), 30, "300 words ≈ 30 lessons");
        assert!(word_lessons.iter().all(|l| l.words.len() == 10));
        // Review every 5th position, and one at the end.
        let reviews: Vec<u32> = plan.iter().filter(|l| l.role == LessonRole::Review).map(|l| l.number).collect();
        assert_eq!(reviews, vec![5, 10, 15, 20, 25, 30, 35, 38]);
        assert_eq!(plan.len(), 38);
        assert!(plan.iter().enumerate().all(|(i, l)| l.number == i as u32 + 1));
        // The review covers the four word lessons before it.
        assert_eq!(plan[4].review.len(), 40);
        assert_eq!(plan[4].review[0], "word000");
        // Grammar: 1–2 per lesson, all of it, in order.
        assert!(word_lessons.iter().all(|l| (1..=2).contains(&l.grammar.len())));
        let all: Vec<&str> = word_lessons.iter().flat_map(|l| l.grammar.iter().map(|g| g.pattern.as_str())).collect();
        assert_eq!(all.len(), 45);
        assert_eq!(all[0], "g0");
        // Spaced review of earlier words; every word appears at least twice.
        assert_eq!(plan[1].review.len(), 10);
        assert!(plan[1].review.contains(&"word000".to_string()));
        assert!(plan.iter().all(|l| !l.units.is_empty() && l.minutes == 30));
    }

    #[test]
    fn japanese_n5_and_options() {
        let size = content_size(Lang::Ja, 0);
        let plan = split(&words(30), &[], &size, &SplitOptions { review_every: 0, ..opt() });
        assert_eq!(plan.len(), 5, "N5: 6 new words per lesson");
        assert!(plan.iter().all(|l| l.words.len() == 6 && l.role == LessonRole::Regular));
        let plan = split(&words(30), &[], &size, &SplitOptions { new_per_lesson: 10, known: vec!["WORD000".into()], review_every: 0, ..opt() });
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].words[0].term, "word001", "known words are left out");
        // Grammar only.
        let g: Vec<TargetGrammar> = (0..3).map(|i| TargetGrammar { pattern: format!("〜{i}"), ..Default::default() }).collect();
        let plan = split(&[], &g, &size, &SplitOptions { review_every: 0, ..opt() });
        assert_eq!(plan.len(), 3, "N5: one grammar point per lesson");
        assert!(split(&[], &[], &size, &opt()).is_empty());
    }

    #[test]
    fn reads_lists() {
        let w = read_word_list("單字,讀音,意思\nresilient,/rɪˈzɪliənt/,有韌性的\nresilient,/rɪˈzɪliənt/,dup\n# skip\nleverage,,善用\n");
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].meaning, "有韌性的");
        let bare = read_word_list("apple\nbanana\n\n");
        assert_eq!(bare.iter().map(|w| w.term.as_str()).collect::<Vec<_>>(), vec!["apple", "banana"]);
        let g = read_grammar_list("pattern,meaning\n〜に伴って,隨著\n");
        assert_eq!(g[0].pattern, "〜に伴って");
    }
}
