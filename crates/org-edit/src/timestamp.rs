//! Timestamps as the editing commands see them: `org-ts-regexp3` matches
//! on a line (`org-at-timestamp-p` with `lax`), and `org-timestamp-change`,
//! which moves one field and writes the timestamp again.

use jiff::civil::DateTime;
use org_model::time;

use crate::buffer::{Buf, EditError};

/// A match of `org-ts-regexp3`: `[[<]` date, optional day name and time,
/// up to 16 more characters, `[]>]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ts {
    /// The whole timestamp.
    pub start: usize,
    pub end: usize,
    /// Group 1: date, day name and time (`YYYY-MM-DD Day HH:MM`).
    pub inner_end: usize,
    /// Groups 7 and 8, hours and minutes.
    pub time: Option<(usize, usize)>,
}

/// `org-ts-regexp3` at `i`.
fn ts3_at(text: &str, i: usize) -> Option<Ts> {
    let b = text.as_bytes();
    if !matches!(b.get(i), Some(b'[' | b'<')) {
        return None;
    }
    let d = i + 1;
    let digits = |j: usize, n: usize| {
        b.get(j..j + n)
            .is_some_and(|x| x.iter().all(u8::is_ascii_digit))
    };
    if !(digits(d, 4)
        && b.get(d + 4) == Some(&b'-')
        && digits(d + 5, 2)
        && b.get(d + 7) == Some(&b'-')
        && digits(d + 8, 2))
    {
        return None;
    }
    let mut k = d + 10;
    // `\(?: *\([^]+0-9>\r\n -]+\)\)?`
    let mut j = k;
    while b.get(j) == Some(&b' ') {
        j += 1;
    }
    let name_end = text[j..]
        .char_indices()
        .find(|(_, c)| matches!(c, ']' | '+' | '>' | '\r' | '\n' | ' ' | '-') || c.is_ascii_digit())
        .map_or(text.len(), |(x, _)| j + x);
    if name_end > j {
        k = name_end;
    }
    // `\( \([0-9]\{1,2\}\):\([0-9]\{2\}\)\)?`
    let mut time = None;
    if b.get(k) == Some(&b' ') {
        let n = b[k + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
        if matches!(n, 1 | 2) && b.get(k + 1 + n) == Some(&b':') && digits(k + 2 + n, 2) {
            time = Some((k + 1, k + 2 + n));
            k += 4 + n;
        }
    }
    let inner_end = k;
    // `[^]>\n]\{0,16\}[]>]`
    let rest = &text[k..];
    let stop = rest.find([']', '>', '\n'])?;
    if rest[..stop].chars().count() > 16 || rest.as_bytes()[stop] == b'\n' {
        return None;
    }
    Some(Ts {
        start: i,
        end: k + stop + 1,
        inner_end,
        time,
    })
}

/// `(org-in-regexp org-ts-regexp3)`: the timestamp of the line at `pos`
/// that contains `pos` (ends included), from left-to-right matches.
pub(crate) fn timestamp_at(text: &str, pos: usize) -> Option<Ts> {
    let bol = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let eol = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
    let mut i = bol;
    while i < eol {
        match ts3_at(text, i).filter(|t| t.end <= eol) {
            Some(t) => {
                if t.start > pos {
                    return None;
                }
                if t.end >= pos {
                    return Some(t);
                }
                i = t.end;
            }
            None => i += text[i..].chars().next().map_or(1, char::len_utf8),
        }
    }
    None
}

/// The fields `org-timestamp-change` can move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsField {
    /// Minutes.
    Minute,
    /// Hours.
    Hour,
    /// Days.
    Day,
    /// Months.
    Month,
    /// Years.
    Year,
}

/// The "extra" part of a timestamp: an end time and repeater or delay
/// cookies, `\(-[012][0-9]:[0-5][0-9]\)?\( +[.+]?-?[-+][0-9]+[hdwmy]\(/[0-9]+[hdwmy]\)?\)*`
/// before the closing bracket, at its first position.
fn extra(ts: &str) -> String {
    let b = ts.as_bytes();
    let digits = |j: usize| {
        b[j.min(b.len())..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count()
    };
    let unit = |j: usize| matches!(b.get(j), Some(b'h' | b'd' | b'w' | b'm' | b'y'));
    // One ` +[.+]?-?[-+][0-9]+[hdwmy]\(/[0-9]+[hdwmy]\)?` at `j`.
    let cookie = |j: usize| -> Option<usize> {
        let mut k = j;
        while b.get(k) == Some(&b' ') {
            k += 1;
        }
        if k == j {
            return None;
        }
        let tries = [(1usize, 1usize), (1, 0), (0, 1), (0, 0)];
        for (dot, dash) in tries {
            let mut m = k;
            if dot == 1 {
                if !matches!(b.get(m), Some(b'.' | b'+')) {
                    continue;
                }
                m += 1;
            }
            if dash == 1 {
                if b.get(m) != Some(&b'-') {
                    continue;
                }
                m += 1;
            }
            if !matches!(b.get(m), Some(b'-' | b'+')) {
                continue;
            }
            m += 1;
            let n = digits(m);
            if n == 0 || !unit(m + n) {
                continue;
            }
            m += n + 1;
            if b.get(m) == Some(&b'/') {
                let n2 = digits(m + 1);
                if n2 > 0 && unit(m + 1 + n2) {
                    m += 2 + n2;
                }
            }
            return Some(m);
        }
        None
    };
    for p in 0..b.len() {
        let mut k = p;
        let range = b.get(p) == Some(&b'-')
            && matches!(b.get(p + 1), Some(b'0'..=b'2'))
            && b.get(p + 2).is_some_and(u8::is_ascii_digit)
            && b.get(p + 3) == Some(&b':')
            && matches!(b.get(p + 4), Some(b'0'..=b'5'))
            && b.get(p + 5).is_some_and(u8::is_ascii_digit);
        if range {
            k += 6;
        }
        while let Some(m) = cookie(k) {
            k = m;
        }
        if matches!(b.get(k), Some(b']' | b'>')) {
            return ts[p..k].to_string();
        }
    }
    String::new()
}

/// Removes ` --N[hdwmy]` delays.
fn suppress_delays(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find(" --") {
        let after = &rest[i + 3..];
        let n = after.bytes().take_while(u8::is_ascii_digit).count();
        if n > 0
            && matches!(
                after.as_bytes().get(n),
                Some(b'h' | b'd' | b'w' | b'm' | b'y')
            )
        {
            out.push_str(&rest[..i]);
            rest = &after[n + 1..];
        } else {
            out.push_str(&rest[..i + 3]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// `org-timestamp-change` with an explicit field: moves the timestamp at
/// `pos` by `n` of `field` and writes it again (English day name, the
/// extra part kept). Returns the new timestamp.
pub(crate) fn change(
    buf: &mut Buf,
    pos: usize,
    n: i64,
    field: TsField,
    suppress_tmp_delay: bool,
) -> Result<String, EditError> {
    let Some(ts) = timestamp_at(&buf.text, pos) else {
        return Err(EditError::new("Not at a timestamp"));
    };
    let s = buf.text[ts.start..ts.end].to_string();
    let inactive = s.starts_with('[');
    let mut ex = extra(&s);
    if suppress_tmp_delay {
        ex = suppress_delays(&ex);
    }
    // `^.\{10\}.*?[0-9]+:[0-9][0-9]`
    let with_hm = s.char_indices().nth(10).is_some_and(|(i, _)| {
        let r = &s.as_bytes()[i..];
        (1..r.len().saturating_sub(2)).any(|k| {
            r[k] == b':'
                && r[k - 1].is_ascii_digit()
                && r[k + 1].is_ascii_digit()
                && r[k + 2].is_ascii_digit()
        })
    });
    let Some(t0) = time::parse_time_string(&s) else {
        return Err(EditError::new(&format!("Not an Org time string: {s}")));
    };
    let t = shift(t0, n, field).ok_or_else(|| EditError::new("Date out of range"))?;
    let mut new = format!(
        "{}{}",
        if inactive { '[' } else { '<' },
        time::format(t, with_hm)
    );
    new.push_str(&ex);
    new.push(if inactive { ']' } else { '>' });
    buf.replace_before_markers(ts.start, ts.end, &new);
    Ok(new)
}

/// `n` of `field` added to `t`, normalized like `encode-time`.
pub(crate) fn shift(t: DateTime, n: i64, field: TsField) -> Option<DateTime> {
    let (y, mo, d) = (t.year() as i64, t.month() as i64, t.day() as i64);
    let (h, mi) = (t.hour() as i64, t.minute() as i64);
    match field {
        TsField::Minute => time::normalize(y, mo, d, h, mi + n),
        TsField::Hour => time::normalize(y, mo, d, h + n, mi),
        TsField::Day => time::normalize(y, mo, d + n, h, mi),
        TsField::Month => time::normalize(y, mo + n, d, h, mi),
        TsField::Year => time::normalize(y + n, mo, d, h, mi),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_and_extra() {
        let t = "SCHEDULED: <2026-09-28 Mon 10:00-11:00 +1w --2d> x";
        let ts = timestamp_at(t, 20).unwrap();
        assert_eq!(
            &t[ts.start..ts.end],
            "<2026-09-28 Mon 10:00-11:00 +1w --2d>"
        );
        assert_eq!(extra(&t[ts.start..ts.end]), "-11:00 +1w --2d");
        assert_eq!(suppress_delays(" +1w --2d"), " +1w");
        assert_eq!(extra("<2026-09-28 Mon .+2d/3d>"), " .+2d/3d");
        assert_eq!(extra("<2026-09-28>"), "");
        assert!(timestamp_at("<2026-09-28 Mon this is a long note>", 3).is_none());
    }

    #[test]
    fn change_normalizes() {
        let mut b = Buf::new("<2026-01-31 Sat +1m>", 0);
        change(&mut b, 5, 1, TsField::Month, true).unwrap();
        assert_eq!(b.text, "<2026-03-03 Tue +1m>");
        let mut b = Buf::new("[2026-09-28 Mon 23:30]", 0);
        change(&mut b, 5, 1, TsField::Hour, false).unwrap();
        assert_eq!(b.text, "[2026-09-29 Tue 00:30]");
    }
}
