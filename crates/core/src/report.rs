//! The secretary's outgoing messages: weekly report, lesson confirmation,
//! and review sessions on the calendar. Pure functions over database numbers;
//! sending goes through the `Mailer` / `Calendar` traits.

use crate::db::Stats;
use crate::google::{CalendarEvent, Email};
use crate::lang::Lang;
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};

pub fn weekly_report(to: &str, owner: Option<&str>, per_lang: &[(Lang, Stats)], week_label: &str) -> Email {
    let mut body = String::new();
    let name = owner.filter(|s| !s.trim().is_empty()).unwrap_or("你");
    body.push_str(&format!("{name}好，這是 {week_label} 的學習週報。\n\n"));
    for (lang, s) in per_lang {
        let rate = if s.reviews > 0 { format!("{:.0}%", s.reviews_correct as f64 * 100.0 / s.reviews as f64) } else { "—".into() };
        body.push_str(&format!(
            "【{}】\n  新單字 {}　新文法 {}　新教材 {}\n  複習 {} 次（答對率 {}）\n  新錯題 {}，尚未訂正 {}\n  單字庫 {}　文法庫 {}　待複習 {}\n\n",
            lang.label(),
            s.new_words,
            s.new_grammar,
            s.new_lessons,
            s.reviews,
            rate,
            s.new_mistakes,
            s.open_mistakes,
            s.words,
            s.grammar,
            s.due
        ));
    }
    let due: i64 = per_lang.iter().map(|(_, s)| s.due).sum();
    let open: i64 = per_lang.iter().map(|(_, s)| s.open_mistakes).sum();
    body.push_str("下週建議：\n");
    if due > 0 {
        body.push_str(&format!("  - 先清掉 {due} 張待複習卡片\n"));
    }
    if open > 0 {
        body.push_str(&format!("  - 訂正 {open} 題錯題\n"));
    }
    body.push_str("  - 每種語言至少讀一篇新聞並產生教材\n\n— JELearner 學習秘書\n");
    Email { to: to.to_string(), subject: format!("JELearner 學習週報 {week_label}"), body }
}

pub fn lesson_confirmation(to: &str, ev: &CalendarEvent, time_zone: &str) -> Email {
    Email {
        to: to.to_string(),
        subject: format!("課程確認：{}", ev.summary),
        body: format!(
            "已幫你排好課程：\n\n  {}\n  時間：{} – {}（{}）\n\n{}\n\n— JELearner 學習秘書\n",
            ev.summary,
            ev.start.format("%Y-%m-%d %H:%M UTC"),
            ev.end.format("%H:%M UTC"),
            time_zone,
            ev.description
        ),
    }
}

/// One review event per day that has due cards, at `at` (UTC) on that day;
/// overdue cards are counted on the first day.
pub fn review_events(due_by_day: &[(String, i64)], today: NaiveDate, at: NaiveTime, minutes: i64) -> Vec<CalendarEvent> {
    let mut by_day: std::collections::BTreeMap<NaiveDate, i64> = Default::default();
    for (day, n) in due_by_day {
        let Ok(d) = NaiveDate::parse_from_str(day, "%Y-%m-%d") else { continue };
        *by_day.entry(d.max(today)).or_default() += n;
    }
    by_day
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(d, n)| {
            let start: DateTime<Utc> = Utc.from_utc_datetime(&d.and_time(at));
            CalendarEvent {
                summary: format!("JELearner 複習 {n} 張"),
                description: format!("{n} 張單字與文法卡片到期，開啟 JELearner 的「今日」頁複習。"),
                start,
                end: start + Duration::minutes(minutes.max(5)),
                reminder_minutes: Some(10),
            }
        })
        .collect()
}

/// ISO week label for a date, e.g. `2026-W45`.
pub fn iso_week(d: NaiveDate) -> String {
    use chrono::Datelike;
    let w = d.iso_week();
    format!("{}-W{:02}", w.year(), w.week())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_mentions_numbers_and_advice() {
        let s = Stats { new_words: 12, reviews: 10, reviews_correct: 8, due: 5, open_mistakes: 2, ..Default::default() };
        let e = weekly_report("me@example.com", Some("Ellrin"), &[(Lang::Ja, s)], "2026-W45");
        assert!(e.subject.contains("2026-W45"));
        assert!(e.body.contains("新單字 12"));
        assert!(e.body.contains("80%"));
        assert!(e.body.contains("5 張待複習"));
    }

    #[test]
    fn review_events_fold_overdue_into_today() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
        let ev = review_events(
            &[("2026-10-30".into(), 3), ("2026-11-02".into(), 2), ("2026-11-04".into(), 4)],
            today,
            NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
            20,
        );
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].summary, "JELearner 複習 5 張");
        assert_eq!(ev[1].start.format("%Y-%m-%d").to_string(), "2026-11-04");
        assert_eq!(iso_week(today), "2026-W45");
    }
}
