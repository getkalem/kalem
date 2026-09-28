//! Dates typed by the user, for the date picker and date prompts
//! (T1.5.18): a basic version of `org-read-date`'s input.
//!
//! Accepted: `today`, `.` or nothing; `tomorrow`, `yesterday`; `+3`,
//! `+3d`, `-2w`, `+1m`, `+1y` from today; weekday names (`fri`, `friday`:
//! the next one, today included); `2026-10-01`; `10-01` (this year, or the
//! next when the day has passed, as `org-read-date-prefer-future`); and a
//! time `14:30` alone or after any of these.

use jiff::civil::{Date, DateTime, Time, Weekday};
use org_syntax::{SyntaxKind, SyntaxNode, TextSize};

/// A date and time the user typed, relative to `now`; whether it has a
/// time.
pub fn parse(input: &str, now: DateTime) -> Option<(DateTime, bool)> {
    let today = now.date();
    let mut time: Option<Time> = None;
    let mut date_part = Vec::new();
    for w in input.split_whitespace() {
        if let Some(t) = parse_time(w) {
            time = Some(t);
        } else {
            date_part.push(w.to_ascii_lowercase());
        }
    }
    let date = match date_part.as_slice() {
        [] => today,
        [w] => parse_date(w, today)?,
        _ => return None,
    };
    let with_time = time.is_some();
    Some((
        date.to_datetime(time.unwrap_or(Time::midnight())),
        with_time,
    ))
}

fn parse_time(w: &str) -> Option<Time> {
    let (h, m) = w.split_once(':')?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 {
        return None;
    }
    Time::new(h.parse().ok()?, m.parse().ok()?, 0, 0).ok()
}

fn parse_date(w: &str, today: Date) -> Option<Date> {
    match w {
        "." | "today" => return Some(today),
        "tomorrow" => return today.tomorrow().ok(),
        "yesterday" => return today.yesterday().ok(),
        _ => {}
    }
    if let Some(sign) = w.chars().next().filter(|c| matches!(c, '+' | '-')) {
        let rest = &w[1..];
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let n: i64 = rest[..digits].parse().ok()?;
        let n = if sign == '-' { -n } else { n };
        let span = match &rest[digits..] {
            "" | "d" => jiff::Span::new().days(n),
            "w" => jiff::Span::new().weeks(n),
            "m" => jiff::Span::new().months(n),
            "y" => jiff::Span::new().years(n),
            _ => return None,
        };
        return today.checked_add(span).ok();
    }
    if let Some(day) = weekday(w) {
        let ahead = (day.to_monday_zero_offset() - today.weekday().to_monday_zero_offset() + 7) % 7;
        return today
            .checked_add(jiff::Span::new().days(i64::from(ahead)))
            .ok();
    }
    let parts: Vec<&str> = w.split('-').collect();
    match parts.as_slice() {
        [y, m, d] if y.len() == 4 => {
            Date::new(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?).ok()
        }
        [m, d] => {
            let (m, d): (i8, i8) = (m.parse().ok()?, d.parse().ok()?);
            let this = Date::new(today.year(), m, d).ok()?;
            if this < today {
                Date::new(today.year() + 1, m, d).ok()
            } else {
                Some(this)
            }
        }
        _ => None,
    }
}

fn weekday(w: &str) -> Option<Weekday> {
    let names = [
        ("mon", Weekday::Monday),
        ("tue", Weekday::Tuesday),
        ("wed", Weekday::Wednesday),
        ("thu", Weekday::Thursday),
        ("fri", Weekday::Friday),
        ("sat", Weekday::Saturday),
        ("sun", Weekday::Sunday),
    ];
    let full = |short: &str| match short {
        "mon" => "monday",
        "tue" => "tuesday",
        "wed" => "wednesday",
        "thu" => "thursday",
        "fri" => "friday",
        "sat" => "saturday",
        _ => "sunday",
    };
    names
        .iter()
        .find(|(s, _)| w.len() >= 3 && full(s).starts_with(w))
        .map(|(_, d)| *d)
}

/// The start of the timestamp at `point`, and whether it has a time: what
/// a date picker starts from when changing it.
pub fn timestamp_at(root: &SyntaxNode, point: usize) -> Option<(DateTime, bool)> {
    let len = usize::from(root.text_range().end());
    if len == 0 {
        return None;
    }
    let tok = root
        .token_at_offset(TextSize::from(point.min(len) as u32))
        .find(|t| {
            t.parent_ancestors()
                .any(|a| a.kind() == SyntaxKind::TIMESTAMP)
        })?;
    let node = tok
        .parent_ancestors()
        .find(|a| a.kind() == SyntaxKind::TIMESTAMP)?;
    let ts = org_model::time::Timestamp::from_node(&node)?;
    Some((ts.start?, ts.has_time))
}

/// A date as the `date` argument of `org.insert.date` takes it.
pub fn argument(date: DateTime, with_time: bool) -> String {
    if with_time {
        date.strftime("%Y-%m-%d %H:%M").to_string()
    } else {
        date.strftime("%Y-%m-%d").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn inputs() {
        // A Monday.
        let now = date(2026, 9, 28).at(15, 0, 0, 0);
        let d = |s: &str| parse(s, now).map(|(d, t)| argument(d, t));
        assert_eq!(d("").as_deref(), Some("2026-09-28"));
        assert_eq!(d("tomorrow").as_deref(), Some("2026-09-29"));
        assert_eq!(d("+3").as_deref(), Some("2026-10-01"));
        assert_eq!(d("-1w").as_deref(), Some("2026-09-21"));
        assert_eq!(d("+1m 9:30").as_deref(), Some("2026-10-28 09:30"));
        assert_eq!(d("fri").as_deref(), Some("2026-10-02"));
        assert_eq!(d("Monday").as_deref(), Some("2026-09-28"));
        assert_eq!(d("2027-01-05 14:00").as_deref(), Some("2027-01-05 14:00"));
        assert_eq!(d("10-01").as_deref(), Some("2026-10-01"));
        assert_eq!(d("09-01").as_deref(), Some("2027-09-01"));
        assert_eq!(d("14:30").as_deref(), Some("2026-09-28 14:30"));
        assert_eq!(d("soon"), None);
        assert_eq!(d("2026-02-30"), None);
        let t = "x <2026-10-02 Fri 10:00> y";
        let p = org_syntax::parse(t);
        let (dt, time) = timestamp_at(&p.syntax(), 5).unwrap();
        assert_eq!(argument(dt, time), "2026-10-02 10:00");
        assert!(timestamp_at(&p.syntax(), 0).is_none());
    }
}
