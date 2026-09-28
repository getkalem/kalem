//! Dates and times as Org computes them, on top of `jiff` (D13).
//!
//! Org does date arithmetic the way Emacs's `encode-time` does: fields are
//! added to and then normalized, so that January 31 plus one month is
//! March 3 (or 2), not February 28. The functions here follow that.

use jiff::civil::{Date, DateTime, Time};
use org_syntax::ast::{self, AstNode, Repeater, TimeUnit, TimestampType, Warning};
use org_syntax::{SyntaxNode, TextRange};

/// `encode-time` normalization: fields out of range carry into the next
/// larger field (month 13 is January of the next year, day 0 is the last
/// day of the previous month, minute 60 is the next hour).
pub fn normalize(year: i64, month: i64, day: i64, hour: i64, minute: i64) -> Option<DateTime> {
    let y = year + (month - 1).div_euclid(12);
    let m = (month - 1).rem_euclid(12) + 1;
    let total = hour * 60 + minute;
    let carry = total.div_euclid(1440);
    let rem = total.rem_euclid(1440);
    let first = Date::new(i16::try_from(y).ok()?, m as i8, 1).ok()?;
    let date = first
        .checked_add(jiff::Span::new().try_days(day - 1 + carry).ok()?)
        .ok()?;
    Some(date.to_datetime(Time::new((rem / 60) as i8, (rem % 60) as i8, 0, 0).ok()?))
}

/// The civil date and time of an Org date (midnight when it has no time).
pub fn datetime(d: &ast::DateTime) -> Option<DateTime> {
    let (h, m) = d.time.unwrap_or((0, 0));
    normalize(
        d.year as i64,
        d.month as i64,
        d.day as i64,
        h as i64,
        m as i64,
    )
}

/// `org-parse-time-string`: the first `YYYY-MM-DD[ DAY][ H:MM]` in `s`,
/// with a missing time as 0:00.
pub fn parse_time_string(s: &str) -> Option<DateTime> {
    let b = s.as_bytes();
    let digits = |i: usize, n: usize| {
        b.get(i..i + n)
            .is_some_and(|x| x.iter().all(u8::is_ascii_digit))
    };
    let num = |i: usize, n: usize| s[i..i + n].parse::<i64>().ok();
    let start = (0..b.len()).find(|&i| {
        digits(i, 4)
            && b.get(i + 4) == Some(&b'-')
            && digits(i + 5, 2)
            && b.get(i + 7) == Some(&b'-')
            && digits(i + 8, 2)
    })?;
    let (y, mo, d) = (num(start, 4)?, num(start + 5, 2)?, num(start + 8, 2)?);
    // `\( +[^]+0-9>\r\n -]+\)?\( +\([0-9]\{1,2\}\):\([0-9]\{2\}\)\)?`
    let mut i = start + 10;
    let skip_spaces = |mut j: usize| {
        while b.get(j) == Some(&b' ') {
            j += 1;
        }
        j
    };
    let j = skip_spaces(i);
    if j > i {
        let mut k = j;
        while let Some(c) = s[k..].chars().next() {
            if matches!(c, ']' | '+' | '>' | '\r' | '\n' | ' ' | '-') || c.is_ascii_digit() {
                break;
            }
            k += c.len_utf8();
        }
        if k > j {
            i = k;
        }
    }
    let (mut h, mut m) = (0, 0);
    let j = skip_spaces(i);
    if j > i {
        let hd = if digits(j, 2) && b.get(j + 2) == Some(&b':') {
            2
        } else if digits(j, 1) && b.get(j + 1) == Some(&b':') {
            1
        } else {
            0
        };
        if hd > 0 && digits(j + hd + 1, 2) {
            h = num(j, hd)?;
            m = num(j + hd + 1, 2)?;
        }
    }
    normalize(y, mo, d, h, m)
}

/// `org-timestamp-change` arithmetic: add `n` units to one field and
/// normalize.
pub fn add(dt: DateTime, n: i64, unit: TimeUnit) -> Option<DateTime> {
    let (y, mo, d) = (dt.year() as i64, dt.month() as i64, dt.day() as i64);
    let (h, mi) = (dt.hour() as i64, dt.minute() as i64);
    match unit {
        TimeUnit::Hour => normalize(y, mo, d, h + n, mi),
        TimeUnit::Day => normalize(y, mo, d + n, h, mi),
        TimeUnit::Week => normalize(y, mo, d + 7 * n, h, mi),
        TimeUnit::Month => normalize(y, mo + n, d, h, mi),
        TimeUnit::Year => normalize(y + n, mo, d, h, mi),
    }
}

/// The day name Org writes in timestamps (English abbreviations).
pub fn day_name(d: Date) -> &'static str {
    use jiff::civil::Weekday::*;
    match d.weekday() {
        Monday => "Mon",
        Tuesday => "Tue",
        Wednesday => "Wed",
        Thursday => "Thu",
        Friday => "Fri",
        Saturday => "Sat",
        Sunday => "Sun",
    }
}

/// The body of a timestamp for `dt`: `2026-10-01 Thu` or
/// `2026-10-01 Thu 10:00`.
pub fn format(dt: DateTime, with_time: bool) -> String {
    let d = dt.date();
    let mut s = format!(
        "{:04}-{:02}-{:02} {}",
        d.year(),
        d.month(),
        d.day(),
        day_name(d)
    );
    if with_time {
        s.push_str(&format!(" {:02}:{:02}", dt.hour(), dt.minute()));
    }
    s
}

/// A timestamp with its dates resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timestamp {
    /// Where it is.
    pub range: TextRange,
    /// Active, inactive, range or diary.
    pub kind: TimestampType,
    /// `:raw-value`.
    pub raw: String,
    /// The start (midnight when there is no time); `None` for diary sexps.
    pub start: Option<DateTime>,
    /// Whether the start has a time.
    pub has_time: bool,
    /// The end of a range.
    pub end: Option<DateTime>,
    /// The repeater, such as `+1w`.
    pub repeater: Option<Repeater>,
    /// The warning delay, such as `-2d`.
    pub warning: Option<Warning>,
}

impl Timestamp {
    /// Reads a timestamp node.
    pub fn from_node(node: &SyntaxNode) -> Option<Timestamp> {
        let t = ast::Timestamp::cast(node.clone())?;
        let start = t.start();
        Some(Timestamp {
            range: node.text_range(),
            kind: t.timestamp_type(),
            raw: t.raw_value(),
            has_time: start.as_ref().is_some_and(|s| s.time.is_some()),
            start: start.as_ref().and_then(datetime),
            end: t.end().as_ref().and_then(datetime),
            repeater: t.repeater(),
            warning: t.warning(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn emacs_normalization() {
        let jan31 = date(2026, 1, 31).at(0, 0, 0, 0);
        assert_eq!(
            add(jan31, 1, TimeUnit::Month),
            Some(date(2026, 3, 3).at(0, 0, 0, 0))
        );
        assert_eq!(
            normalize(2026, 13, 1, 0, 0),
            Some(date(2027, 1, 1).at(0, 0, 0, 0))
        );
        assert_eq!(
            normalize(2026, 3, 0, 0, 0),
            Some(date(2026, 2, 28).at(0, 0, 0, 0))
        );
        assert_eq!(
            normalize(2026, 1, 1, 23, 90),
            Some(date(2026, 1, 2).at(0, 30, 0, 0))
        );
        assert_eq!(
            add(date(2024, 2, 29).at(9, 0, 0, 0), 1, TimeUnit::Year),
            Some(date(2025, 3, 1).at(9, 0, 0, 0))
        );
    }

    #[test]
    fn time_strings() {
        assert_eq!(
            parse_time_string("<2026-10-01 Thu 10:05>"),
            Some(date(2026, 10, 1).at(10, 5, 0, 0))
        );
        assert_eq!(
            parse_time_string("[2026-10-01]"),
            Some(date(2026, 10, 1).at(0, 0, 0, 0))
        );
        assert_eq!(
            parse_time_string("x 2026-10-01 9:30"),
            Some(date(2026, 10, 1).at(9, 30, 0, 0))
        );
        assert_eq!(
            parse_time_string("2026-02-30"),
            Some(date(2026, 3, 2).at(0, 0, 0, 0))
        );
        assert_eq!(parse_time_string("nothing"), None);
        assert_eq!(
            format(date(2026, 10, 1).at(10, 0, 0, 0), true),
            "2026-10-01 Thu 10:00"
        );
    }
}
