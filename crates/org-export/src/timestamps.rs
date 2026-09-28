//! Timestamps written again from their parts
//! (`org-element-timestamp-interpreter`), as `org-timestamp-translate`
//! does without custom formats: the day name in English, hours on two
//! digits, impossible dates carried over (`2014-02-30` is March 2).

use org_syntax::SyntaxNode;
use org_syntax::ast::{self, AstNode, RangeType, RepeaterType, TimeUnit, TimestampType};

fn unit(u: TimeUnit) -> &'static str {
    match u {
        TimeUnit::Hour => "h",
        TimeUnit::Day => "d",
        TimeUnit::Week => "w",
        TimeUnit::Month => "m",
        TimeUnit::Year => "y",
    }
}

/// `YYYY-MM-DD Day[ HH:MM]` for a date `encode-time` would normalize.
fn date(year: i32, month: u32, day: u32, time: Option<(u32, u32)>) -> Option<String> {
    let (hour, minute) = time.unwrap_or((0, 0));
    let months = i64::from(year) * 12 + i64::from(month) - 1;
    let (y, m) = (months.div_euclid(12), months.rem_euclid(12) + 1);
    let first = jiff::civil::Date::new(i16::try_from(y).ok()?, m as i8, 1).ok()?;
    let days = i64::from(day) - 1 + i64::from(hour / 24);
    let d = first.checked_add(jiff::Span::new().days(days)).ok()?;
    let name = match d.weekday() {
        jiff::civil::Weekday::Monday => "Mon",
        jiff::civil::Weekday::Tuesday => "Tue",
        jiff::civil::Weekday::Wednesday => "Wed",
        jiff::civil::Weekday::Thursday => "Thu",
        jiff::civil::Weekday::Friday => "Fri",
        jiff::civil::Weekday::Saturday => "Sat",
        jiff::civil::Weekday::Sunday => "Sun",
    };
    let mut s = format!("{:04}-{:02}-{:02} {name}", d.year(), d.month(), d.day());
    if time.is_some() {
        s.push_str(&format!(" {:02}:{:02}", hour % 24, minute));
    }
    Some(s)
}

/// The timestamp `node` as Org writes it again, with the blanks after it
/// as spaces (`org-element-interpret-data`).
pub fn interpret(node: &SyntaxNode) -> String {
    let raw = node.text().to_string();
    let Some(ts) = ast::Timestamp::cast(node.clone()) else {
        return raw;
    };
    let blanks = " ".repeat(ast::post_blank(node));
    let ty = ts.timestamp_type();
    let (Some(start), false) = (ts.start(), ty == TimestampType::Diary) else {
        return format!("{}{blanks}", ts.raw_value());
    };
    let (open, close) = match ty {
        TimestampType::Inactive | TimestampType::InactiveRange => ("[", "]"),
        _ => ("<", ">"),
    };
    let repeat = ts
        .repeater()
        .map(|r| {
            let kind = match r.kind {
                RepeaterType::Cumulate => "+",
                RepeaterType::CatchUp => "++",
                RepeaterType::Restart => ".+",
            };
            let deadline = r
                .deadline
                .map(|(v, u)| format!("/{v}{}", unit(u)))
                .unwrap_or_default();
            format!(" {kind}{}{}{deadline}", r.value, unit(r.unit))
        })
        .unwrap_or_default();
    let warning = ts
        .warning()
        .map(|w| {
            format!(
                " {}{}{}",
                if w.first_only { "--" } else { "-" },
                w.value,
                unit(w.unit)
            )
        })
        .unwrap_or_default();
    let end_part = format!("{repeat}{warning}{close}");
    let Some(first) = date(start.year, start.month, start.day, start.time) else {
        return raw;
    };
    let mut out = format!("{open}{first}");
    if matches!(
        ty,
        TimestampType::ActiveRange | TimestampType::InactiveRange
    ) {
        let end = ts.end().unwrap_or(start);
        match ts.range_type() {
            Some(RangeType::TimeRange) => {
                let (h, m) = end.time.or(start.time).unwrap_or((0, 0));
                out.push_str(&format!("-{h:02}:{m:02}"));
            }
            _ => {
                let Some(second) = date(end.year, end.month, end.day, end.time) else {
                    return raw;
                };
                out.push_str(&format!("{end_part}--{open}{second}"));
            }
        }
    }
    out.push_str(&end_part);
    out.push_str(&blanks);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use org_syntax::SyntaxKind;

    fn first(text: &str) -> String {
        let p = org_syntax::parse(text);
        let n = p
            .syntax()
            .descendants()
            .find(|n| n.kind() == SyntaxKind::TIMESTAMP)
            .expect("a timestamp");
        interpret(&n)
    }

    #[test]
    fn written_again() {
        assert_eq!(first("a <2014-04-09 Mi> b\n"), "<2014-04-09 Wed> ");
        assert_eq!(first("a [2015-01-08] b\n"), "[2015-01-08 Thu] ");
        assert_eq!(
            first("<2024-01-01 Mon 9:05-12:00 +1w -2d>\n"),
            "<2024-01-01 Mon 09:05-12:00 +1w -2d>"
        );
        assert_eq!(
            first("<2024-01-01 Mon>--<2024-01-03 Wed>\n"),
            "<2024-01-01 Mon>--<2024-01-03 Wed>"
        );
        assert_eq!(first("[2014-02-30 x 10:00]\n"), "[2014-03-02 Sun 10:00]");
        assert_eq!(first("<2024-01-01 .+1d/3d>\n"), "<2024-01-01 Mon .+1d/3d>");
        assert_eq!(
            first("<%%(diary-float t 4 2)>\n"),
            "<%%(diary-float t 4 2)>"
        );
    }
}
