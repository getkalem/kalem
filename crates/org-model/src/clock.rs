//! Clocked time, as `org-clock-sum` totals it: minutes per heading,
//! including the heading's subtree.

use std::collections::HashMap;

use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use org_syntax::SyntaxKind;
use org_syntax::ast::{AstNode, Clock};

use crate::{Document, EntryId};

/// Seconds between two local times, in the system time zone (as Emacs's
/// `float-time` of `encode-time` sees them, daylight saving included).
fn local_seconds(a: DateTime, b: DateTime) -> Option<i64> {
    let tz = TimeZone::system();
    let za = a.to_zoned(tz.clone()).ok()?;
    let zb = b.to_zoned(tz).ok()?;
    Some(zb.timestamp().as_second() - za.timestamp().as_second())
}

impl Document {
    /// `org-clock-sum`: clocked minutes of each heading line with clocked
    /// time in its subtree, by the line's start. Inlinetask lines, their
    /// `END` lines included, take part as Emacs has them.
    pub fn clock_sums(&self) -> HashMap<usize, i64> {
        let root = self.parse.syntax();
        let text = root.to_string();
        // Closed clocks by line start.
        let mut clocks: HashMap<usize, i64> = HashMap::new();
        for c in root.descendants().filter_map(Clock::cast) {
            let Some(ts) = c.timestamp() else { continue };
            let (Some(s), Some(e)) = (ts.start(), ts.end()) else {
                continue;
            };
            let (Some(_), Some(_)) = (s.time, e.time) else {
                continue;
            };
            let (Some(a), Some(b)) = (crate::time::datetime(&s), crate::time::datetime(&e)) else {
                continue;
            };
            if let Some(dt) = local_seconds(a, b)
                && dt > 0
            {
                clocks.insert(
                    usize::from(c.syntax().text_range().start()),
                    dt.div_euclid(60),
                );
            }
        }
        let headings: HashMap<usize, usize> = self.heading_lines_pub().into_iter().collect();
        let mut lines: Vec<(usize, &str)> = Vec::new();
        let mut pos = 0;
        for raw in text.split_inclusive('\n') {
            lines.push((pos, raw.trim_end_matches(['\n', '\r'])));
            pos += raw.len();
        }
        let mut ltimes: Vec<i64> = vec![0; 30];
        let mut t1: i64 = 0;
        let mut out = HashMap::new();
        for &(start, line) in lines.iter().rev() {
            if let Some(&level) = headings.get(&start) {
                if level >= ltimes.len() {
                    ltimes.resize(level * 2, 0);
                }
                if t1 > 0 || ltimes[level] > 0 {
                    for l in ltimes.iter_mut().take(level + 1) {
                        *l += t1;
                    }
                    out.insert(start, ltimes[level]);
                    t1 = 0;
                    for l in ltimes.iter_mut().skip(level) {
                        *l = 0;
                    }
                }
                continue;
            }
            // `^[ \t]*CLOCK:[ \t]*` (case-folded), then a range or `=> H:MM`.
            let t = line.trim_start_matches([' ', '\t']);
            if !t
                .as_bytes()
                .get(..6)
                .is_some_and(|b| b.eq_ignore_ascii_case(b"CLOCK:"))
            {
                continue;
            }
            let rest = t[6..].trim_start_matches([' ', '\t']);
            if rest.starts_with('[') && rest[1..].contains(']') && {
                let close = rest.find(']').unwrap_or(0);
                let after = &rest[close + 1..];
                let dashes = after.bytes().take_while(|b| *b == b'-').count();
                dashes > 0 && after[dashes..].starts_with('[') && after[dashes..].contains(']')
            } {
                // Line start of the clock element: its node starts at the
                // line's first non-blank character or at the line start.
                let indent = line.len() - t.len();
                if let Some(m) = clocks.get(&start).or_else(|| clocks.get(&(start + indent))) {
                    t1 += m;
                }
            } else if let Some(r) = rest.strip_prefix("=>") {
                let r2 = r.trim_start_matches([' ', '\t']);
                if r2.len() < r.len() {
                    let h: String = r2.chars().take_while(char::is_ascii_digit).collect();
                    let after = &r2[h.len()..];
                    if !h.is_empty() && after.starts_with(':') {
                        let m: String = after[1..]
                            .chars()
                            .take_while(char::is_ascii_digit)
                            .collect();
                        if !m.is_empty() {
                            t1 +=
                                m.parse::<i64>().unwrap_or(0) + 60 * h.parse::<i64>().unwrap_or(0);
                        }
                    }
                }
            }
        }
        out
    }

    /// The clocked minutes of an entry and its subtree (`org-clock-sum`),
    /// or `None` when nothing was clocked.
    pub fn clock_minutes(&self, id: EntryId) -> Option<i64> {
        self.clock_sums()
            .get(&usize::from(self.entry(id).range.start()))
            .copied()
    }

    /// Every CLOCK line of the document.
    pub fn clock_lines(&self) -> Vec<Clock> {
        self.parse
            .syntax()
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::CLOCK)
            .filter_map(Clock::cast)
            .collect()
    }
}
