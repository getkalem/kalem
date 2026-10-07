//! Timestamps as the editing commands see them: `org-ts-regexp3` matches
//! on a line (`org-at-timestamp-p` with `lax`), and `org-timestamp-change`,
//! which moves one field and writes the timestamp again.

use jiff::civil::DateTime;
use org_model::time;

use crate::buffer::{Buf, EditError};
use crate::transaction::Transaction;

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
    /// Group 5, the day name.
    pub name: Option<(usize, usize)>,
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
    let name = (name_end > j).then_some((j, name_end));
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
        name,
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

/// Whether `pos` is in (or right after) a timestamp-like text of its line,
/// as `(org-at-timestamp-p 'lax)`: where S-up and the other timestamp keys
/// apply. Only the 128 bytes before `pos` are looked at, which hold any
/// timestamp around it.
pub fn at_timestamp(text: &str, pos: usize) -> bool {
    let pos = pos.min(text.len());
    let bol = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let mut from = bol.max(pos.saturating_sub(128));
    while !text.is_char_boundary(from) {
        from += 1;
    }
    if from == bol {
        return timestamp_at(text, pos).is_some();
    }
    // A line of its own from `from`, for the left-to-right matching.
    let eol = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
    let mut line = String::from("\n");
    line.push_str(&text[from..eol]);
    timestamp_at(&line, pos - from + 1).is_some()
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

/// Where a position is in a timestamp, as `org-at-timestamp-p` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TsPlace {
    Bracket,
    After,
    Year,
    Month,
    Hour,
    Minute,
    Day,
    /// In the end time, repeater or delay: the offset from the end of
    /// the time (or of the day name).
    Extra(usize),
}

impl Ts {
    /// The end of the hours (group 7) and of the minutes (group 8).
    fn hour_end(&self) -> Option<usize> {
        self.time.map(|(_, m)| m - 1)
    }

    fn minute_end(&self) -> Option<usize> {
        self.time.map(|(_, m)| m + 2)
    }

    /// `org-at-timestamp-p`'s answer for `pos`.
    fn place(&self, pos: usize) -> TsPlace {
        let s = self.start;
        let within = |r: Option<(usize, usize)>| r.is_some_and(|(a, b)| a <= pos && pos <= b);
        if pos == s || pos + 1 == self.end {
            return TsPlace::Bracket;
        }
        if pos == self.end {
            return TsPlace::After;
        }
        if within(Some((s + 1, s + 5))) {
            return TsPlace::Year;
        }
        if within(Some((s + 6, s + 8))) {
            return TsPlace::Month;
        }
        if within(self.time.map(|(h, _)| (h, self.hour_end().unwrap_or(h)))) {
            return TsPlace::Hour;
        }
        if within(self.time.map(|(_, m)| (m, m + 2))) {
            return TsPlace::Minute;
        }
        if within(Some((s + 9, s + 11))) || within(self.name) {
            return TsPlace::Day;
        }
        match self.minute_end().or(self.name.map(|n| n.1)) {
            Some(l) if pos > l && pos < self.end => TsPlace::Extra(pos - l),
            _ => TsPlace::Day,
        }
    }
}

/// `org-modify-ts-extra`: the end time, repeater or warning of `extra`
/// at `pos` changed by `n` steps (minutes by `step`).
fn modify_extra(extra: &str, pos: usize, n: i64, step: i64) -> String {
    let b = extra.as_bytes();
    let digits = |j: usize| b[j.min(b.len())..].iter().take_while(|c| c.is_ascii_digit()).count();
    let unit = |j: usize| matches!(b.get(j), Some(b'd' | b'w' | b'm' | b'y'));
    // `\(-\([012][0-9]\):\([0-5][0-9]\)\)?`
    let mut k = 0;
    let time = (b.first() == Some(&b'-')
        && matches!(b.get(1), Some(b'0'..=b'2'))
        && b.get(2).is_some_and(u8::is_ascii_digit)
        && b.get(3) == Some(&b':')
        && matches!(b.get(4), Some(b'0'..=b'5'))
        && b.get(5).is_some_and(u8::is_ascii_digit))
    .then_some(0);
    if time.is_some() {
        k = 6;
    }
    // ` +SIGN\([0-9]+\)\([dmwy]\)`: the number's and the unit's places.
    let cookie = |k: usize, sign: u8| -> Option<(usize, usize, usize)> {
        let sp = b[k..].iter().take_while(|c| **c == b' ').count();
        if sp == 0 || b.get(k + sp) != Some(&sign) {
            return None;
        }
        let d = k + sp + 1;
        let n = digits(d);
        (n > 0 && unit(d + n)).then_some((d, d + n, d + n + 1))
    };
    let repeat = cookie(k, b'+');
    if let Some((_, _, e)) = repeat {
        k = e;
    }
    let warning = cookie(k, b'-');
    let within = |a: usize, e: usize| a <= pos && pos <= e;
    const UNITS: [(&str, i64); 6] = [("d", 0), ("w", 1), ("m", 2), ("y", 3), ("d", -1), ("y", 4)];
    let next_unit = |u: &str| {
        let i = UNITS.iter().find(|(x, _)| *x == u).map_or(0, |(_, i)| *i);
        UNITS
            .iter()
            .find(|(_, j)| *j == i + n)
            .map_or("", |(x, _)| *x)
            .to_string()
    };
    let (range, new) = if time.is_some() && within(1, 6) {
        let (mut hour, mut minute): (i64, i64) =
            (extra[1..3].parse().unwrap_or(0), extra[4..6].parse().unwrap_or(0));
        if within(1, 3) {
            hour += n;
        } else {
            let rem = minute % step;
            if rem != 0 {
                minute += if n * step > 0 { -rem } else { step - rem };
            }
            minute += n * step;
        }
        if minute < 0 {
            minute += 60;
            hour -= 1;
        }
        if minute > 59 {
            minute -= 60;
            hour += 1;
        }
        ((0, 6), format!("-{:02}:{:02}", hour.rem_euclid(24), minute))
    } else if let Some((_, ne, ue)) = repeat.filter(|(_, ne, ue)| within(*ne, *ue)) {
        ((ne, ue), next_unit(&extra[ne..ue]))
    } else if let Some((ns, ne, _)) = repeat.filter(|(ns, ne, _)| within(*ns, *ne)) {
        let v: i64 = extra[ns..ne].parse().unwrap_or(0);
        ((ns, ne), (v + n).max(1).to_string())
    } else if let Some((_, ne, ue)) = warning.filter(|(_, ne, ue)| within(*ne, *ue)) {
        ((ne, ue), next_unit(&extra[ne..ue]))
    } else if let Some((ns, ne, _)) = warning.filter(|(ns, ne, _)| within(*ns, *ne)) {
        let v: i64 = extra[ns..ne].parse().unwrap_or(0);
        ((ns, ne), (v + n).max(0).to_string())
    } else {
        return extra.to_string();
    };
    format!("{}{new}{}", &extra[..range.0], &extra[range.1..])
}

/// `org-timestamp-change`, as S-up and S-down (`org-timestamp-up`,
/// `updown`) and S-left and S-right (`org-timestamp-up-day`, with
/// `what` the day) call it: the timestamp at `point` changes by `n` of
/// `what`, or of the part of it at `point` (minutes in steps of five);
/// on a bracket the timestamp turns active or inactive; in its end time,
/// repeater or warning that part changes. A clock line's duration
/// follows.
pub fn timestamp_change(
    text: &str,
    point: usize,
    n: i64,
    what: Option<TsField>,
    updown: bool,
) -> Result<Transaction, EditError> {
    let Some(ts) = timestamp_at(text, point) else {
        return crate::buffer::user_error("Not at a timestamp");
    };
    let mut buf = Buf::new(text, point);
    let place = ts.place(point);
    if what.is_none() && place == TsPlace::Bracket {
        // `org-toggle-timestamp-type`.
        let new: String = text[ts.start..ts.end]
            .chars()
            .map(|c| match c {
                '[' => '<',
                ']' => '>',
                '<' => '[',
                '>' => ']',
                c => c,
            })
            .collect();
        buf.replace(ts.start, ts.end, &new);
        buf.point = point;
        return Ok(buf.transaction("Toggle timestamp type"));
    }
    let field = what.or(match place {
        TsPlace::Year => Some(TsField::Year),
        TsPlace::Month => Some(TsField::Month),
        TsPlace::Hour => Some(TsField::Hour),
        TsPlace::Minute => Some(TsField::Minute),
        TsPlace::Day => Some(TsField::Day),
        _ => None,
    });
    let s = &text[ts.start..ts.end];
    let inactive = s.starts_with('[');
    let mut ex = extra(s);
    let with_hm = s.char_indices().nth(10).is_some_and(|(i, _)| {
        let r = &s.as_bytes()[i..];
        (1..r.len().saturating_sub(2)).any(|k| {
            r[k] == b':' && r[k - 1].is_ascii_digit() && r[k + 1].is_ascii_digit() && r[k + 2].is_ascii_digit()
        })
    });
    let Some(mut t0) = time::parse_time_string(s) else {
        return Err(EditError::new(&format!("Not an Org time string: {s}")));
    };
    // `org-timestamp-rounding-minutes`: S-up and S-down on the minutes
    // move by five, from a multiple of five.
    let mut dm = 5;
    let mut increment = n;
    if updown && field == Some(TsField::Minute) {
        increment = dm * n.signum();
        let rem = i64::from(t0.minute()) % dm;
        if rem != 0 {
            let by = if n > 0 { -rem } else { dm - rem };
            t0 = shift(t0, by, TsField::Minute).unwrap_or(t0);
        }
    } else {
        dm = 1;
    }
    let t = match field {
        Some(f) => shift(t0, increment, f).ok_or_else(|| EditError::new("Date out of range"))?,
        None => t0,
    };
    let ts_field_is_time = matches!(field, Some(TsField::Hour | TsField::Minute));
    if ts_field_is_time && has_end_time(&ex) {
        let at = if field == Some(TsField::Hour) { 2 } else { 5 };
        ex = modify_extra(&ex, at, n, dm);
    }
    if what.is_none()
        && let TsPlace::Extra(k) = place
    {
        ex = modify_extra(&ex, k, n, dm);
    }
    let open = if inactive { '[' } else { '<' };
    let close = if inactive { ']' } else { '>' };
    let new = format!("{open}{}{ex}{close}", time::format(t, with_hm));
    buf.replace(ts.start, ts.end, &new);
    // Point stays in the part it was in.
    let nt = ts3_at(&buf.text, ts.start).unwrap_or(Ts {
        start: ts.start,
        end: ts.start + new.len(),
        inner_end: ts.start + new.len(),
        time: None,
        name: None,
    });
    buf.point = match place {
        TsPlace::Day => nt
            .time
            .map(|(h, _)| h)
            .or(nt.name.map(|(_, e)| e - 1))
            .map_or(point, |p| p.min(point)),
        TsPlace::Hour => nt.hour_end().map_or(point, |p| p.min(point)),
        TsPlace::Minute => nt.minute_end().map_or(point, |p| (p - 1).min(point)),
        TsPlace::Extra(_) => (nt.end - 1).min(point),
        TsPlace::After => nt.end,
        _ => point,
    };
    update_clock_line(&mut buf);
    Ok(buf.transaction("Change timestamp"))
}

/// Whether `extra` has an end time, `-HH:MM`, somewhere.
fn has_end_time(extra: &str) -> bool {
    let b = extra.as_bytes();
    (0..b.len()).any(|i| {
        b[i] == b'-'
            && matches!(b.get(i + 1), Some(b'0'..=b'2'))
            && b.get(i + 2).is_some_and(u8::is_ascii_digit)
            && b.get(i + 3) == Some(&b':')
            && matches!(b.get(i + 4), Some(b'0'..=b'5'))
            && b.get(i + 5).is_some_and(u8::is_ascii_digit)
    })
}

/// `org-clock-update-time-maybe`: on a `CLOCK:` line with two
/// timestamps, both are written again and the duration after `=>`
/// computed again; point does not move past the line's new end.
fn update_clock_line(buf: &mut Buf) {
    let origin = buf.point;
    let bol = buf.bol(origin);
    let eol = buf.eol(origin);
    let line = &buf.text[bol..eol];
    let ws = line.len() - line.trim_start_matches([' ', '\t']).len();
    let Some(rest) = line[ws..].strip_prefix("CLOCK:") else {
        return;
    };
    let after = rest.trim_start_matches(' ');
    let t1 = bol + ws + 6 + (rest.len() - after.len());
    let Some(a) = ts3_at(&buf.text, t1).filter(|t| t.end <= eol) else {
        return;
    };
    let dashes = buf.text[a.end..eol].bytes().take_while(|c| *c == b'-').count();
    if dashes == 0 {
        return;
    }
    let Some(b) = ts3_at(&buf.text, a.end + dashes).filter(|t| t.end <= eol) else {
        return;
    };
    // Both timestamps as `org-timestamp-change 0 'day` writes them.
    let rewrite = |buf: &mut Buf, at: usize| -> Option<()> {
        let ts = ts3_at(&buf.text, at)?;
        let s = buf.text[ts.start..ts.end].to_string();
        let t = time::parse_time_string(&s)?;
        let with_hm = ts.time.is_some();
        let (open, close) = if s.starts_with('[') { ('[', ']') } else { ('<', '>') };
        let new = format!("{open}{}{}{close}", time::format(t, with_hm), extra(&s));
        buf.replace(ts.start, ts.end, &new);
        Some(())
    };
    let b_start = b.start;
    let old_b_end = b.end;
    let a_len = a.end - a.start;
    rewrite(buf, a.start);
    let shift_by = ts3_at(&buf.text, a.start).map_or(0, |t| t.end - t.start) as isize - a_len as isize;
    rewrite(buf, (b_start as isize + shift_by) as usize);
    let eol = buf.eol(bol);
    let Some(b) = ts3_at(&buf.text, (b_start as isize + shift_by) as usize) else {
        return;
    };
    let (Some(ta), Some(tb)) = (
        time::parse_time_string(&buf.text[a.start..a.start + (a_len as isize + shift_by) as usize]),
        time::parse_time_string(&buf.text[b.start..b.end]),
    ) else {
        return;
    };
    let secs = ta.duration_until(tb).as_secs();
    let (neg, s) = (secs < 0, secs.abs());
    let (h, m) = (s / 3600, (s % 3600) / 60);
    let duration = if neg {
        format!(" => -{h}:{m:02}")
    } else {
        format!(" => {h:2}:{m:02}")
    };
    // The old `=> …` part goes; the new one ends the line.
    let tail = &buf.text[b.end..eol];
    let ws_end = b.end + (tail.len() - tail.trim_start_matches([' ', '\t']).len());
    if buf.text[ws_end..eol].starts_with("=>") {
        buf.delete(b.end, eol);
    }
    let eol = buf.eol(bol);
    buf.insert_before_point(eol, &duration);
    // Point stays, unless it was in the old duration, which is gone.
    buf.point = if origin <= old_b_end {
        origin
    } else {
        b.end.min(origin)
    };
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
    fn at_a_timestamp() {
        let t = "x <2026-10-05 Mon> y";
        assert!(!at_timestamp(t, 1));
        assert!(at_timestamp(t, 2) && at_timestamp(t, 10) && at_timestamp(t, 18));
        assert!(!at_timestamp(t, 19));
        // Far into a long line, the same answers.
        let long = format!("{} <2026-10-05 Mon> y", "w".repeat(500));
        assert!(at_timestamp(&long, 510));
        assert!(!at_timestamp(&long, 498));
        assert!(!at_timestamp(&long, long.len()));
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
