//! Calc's date forms: a day number, 1 for January 1 of year 1 in the
//! proleptic Gregorian calendar, with the time as a fraction of the day;
//! read from and written as Org timestamps (`<2026-01-01 Thu 09:00>`,
//! Org's `calc-date-format`).

use num_traits::ToPrimitive;

use super::num::{self, Num, Prec};

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a number of days since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// The day number of 1970-01-01.
const EPOCH: i64 = 719_163;

/// Calc's day number of a date.
pub fn day_number(y: i64, m: i64, d: i64) -> i64 {
    days_from_civil(y, m, d) + EPOCH
}

/// Reads `<YYYY-MM-DD Www>` or `<YYYY-MM-DD Www HH:MM>` at the start of
/// `s` (after the `<`): the date and the length read, `>` included.
pub fn read(s: &str, prec: &Prec) -> Option<(Num, usize)> {
    let close = s.find('>')?;
    let inner = &s[..close];
    let b = inner.as_bytes();
    if b.len() < 10
        || !b[..10].iter().enumerate().all(|(k, c)| {
            if k == 4 || k == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
    {
        return None;
    }
    let y: i64 = inner[..4].parse().ok()?;
    let m: i64 = inner[5..7].parse().ok()?;
    let d: i64 = inner[8..10].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let mut rest = inner[10..].trim_start();
    // The weekday name, if any.
    if rest.chars().next().is_some_and(char::is_alphabetic) {
        rest = rest.trim_start_matches(char::is_alphabetic).trim_start();
    }
    let day = day_number(y, m, d);
    let value = if rest.is_empty() {
        Num::int(day)
    } else {
        let (h, mi) = rest.split_once(':')?;
        let h: i64 = h.parse().ok()?;
        let mi: i64 = mi.get(..2).unwrap_or(mi).parse().ok()?;
        // day + (h × 60 + m) / 1440 at the working precision.
        let frac = num::div(&Num::int(h * 60 + mi), &Num::int(1440), prec).ok()?;
        num::add(&Num::int(day), &frac, prec)
    };
    Some((value, close + 1))
}

/// The day and the seconds into it of a date value.
fn split(v: &Num, prec: &Prec) -> Option<(i64, i64)> {
    let day = num::floor(v);
    let Num::Int(di) = &day else { return None };
    let day_i = di.to_i64()?;
    let frac = num::sub(v, &day, prec);
    let secs = num::round(&num::mul(&frac, &Num::int(86_400), prec), prec);
    let secs = match secs {
        Num::Int(s) => s.to_i64()?,
        _ => 0,
    };
    Some((day_i, secs))
}

/// Writes a date as Org's `calc-date-format` does:
/// `<2026-01-31 Sat>`, with ` 09:00` when there is a time.
pub fn format(v: &Num, prec: &Prec) -> String {
    let Some((mut day, mut secs)) = split(v, prec) else {
        return format!("<{}>", num::format(v, num::Display::Float(0), prec));
    };
    if secs >= 86_400 {
        day += 1;
        secs -= 86_400;
    }
    let (y, m, d) = civil_from_days(day - EPOCH);
    let wd = WEEKDAYS[day.rem_euclid(7) as usize];
    if secs == 0 {
        format!("<{y:04}-{m:02}-{d:02} {wd}>")
    } else {
        format!(
            "<{y:04}-{m:02}-{d:02} {wd} {:02}:{:02}>",
            secs / 3600,
            secs % 3600 / 60
        )
    }
}

/// The parts of a date: year, month, day and weekday (0 for Sunday).
pub fn parts(v: &Num, prec: &Prec) -> Option<(i64, i64, i64, i64)> {
    let (day, _) = split(v, prec)?;
    let (y, m, d) = civil_from_days(day - EPOCH);
    Some((y, m, d, day.rem_euclid(7)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        let p = Prec::default();
        assert_eq!(day_number(1, 1, 1), 1);
        assert_eq!(day_number(1970, 1, 1), EPOCH);
        let (v, n) = read("2026-01-01 Thu>", &p).unwrap();
        assert_eq!(n, 15);
        assert_eq!(format(&v, &p), "<2026-01-01 Thu>");
        let later = num::add(&v, &Num::int(30), &p);
        assert_eq!(format(&later, &p), "<2026-01-31 Sat>");
        let (t, _) = read("2026-01-01 Thu 09:00>", &p).unwrap();
        assert_eq!(format(&t, &p), "<2026-01-01 Thu 09:00>");
        let (u, _) = read("2026-01-02 Fri 12:30>", &p).unwrap();
        assert_eq!(
            num::format(&num::sub(&u, &t, &p), num::Display::default(), &p),
            "1.145833"
        );
        assert_eq!(parts(&v, &p), Some((2026, 1, 1, 4)));
    }
}
