//! Typing a spreadsheet formula as Excel helps with it: F4 cycling a
//! reference through `$A$1`, `A$1`, `$A1`, `A1`; the arrow keys pointing at
//! cells to put their reference in; function and defined names completed,
//! and the arguments of the function the cursor is in shown.
//!
//! Positions are in characters, as [`crate::line_edit`] keeps its cursor.

/// Characters after which a reference can be pointed at.
const BEFORE_REFERENCE: &[char] = &[
    '=', '(', ',', '+', '-', '*', '/', '^', '&', '<', '>', ':', ';', ' ',
];

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

/// A column's letters (0 is `A`).
fn column(c: u32) -> String {
    crate::csv_tools::column_letters(c as usize)
}

/// Where the reference at or just before character `at` lies: its first
/// character and its end.
fn reference_at(text: &[char], at: usize) -> Option<(usize, usize)> {
    let part = |c: char| c.is_ascii_alphanumeric() || c == '$';
    let mut start = at.min(text.len());
    while start > 0 && part(text[start - 1]) {
        start -= 1;
    }
    let mut end = at.min(text.len());
    while end < text.len() && part(text[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    // Not part of a name or a function (`SUM`, `Sheet2`), nor in quotes.
    if start > 0 && (text[start - 1].is_alphabetic() || text[start - 1] == '_') {
        return None;
    }
    if end < text.len() && text[end] == '(' {
        return None;
    }
    let s: String = text[start..end].iter().filter(|c| **c != '$').collect();
    crate::csv_tools::parse_cell(&s)?;
    let digits_last = s.chars().last().is_some_and(|c| c.is_ascii_digit());
    let letters_first = s.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
    (digits_last && letters_first).then_some((start, end))
}

/// F4: the reference at the cursor (`at` characters in) made absolute,
/// then row-absolute, column-absolute and relative in turn; the text and
/// the cursor after it. `None` when the cursor is at no reference.
pub fn toggle_absolute(input: &str, at: usize) -> Option<(String, usize)> {
    let text = chars(input);
    let (start, end) = reference_at(&text, at)?;
    let r: String = text[start..end].iter().collect();
    let col_abs = r.starts_with('$');
    let bare: String = r.chars().filter(|c| *c != '$').collect();
    let split = bare.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = bare.split_at(split);
    let row_abs = r[1..].contains('$');
    let next = match (col_abs, row_abs) {
        (false, false) => format!("${letters}${digits}"),
        (true, true) => format!("{letters}${digits}"),
        (false, true) => format!("${letters}{digits}"),
        (true, false) => format!("{letters}{digits}"),
    };
    let mut out: String = text[..start].iter().collect();
    out.push_str(&next);
    let cursor = out.chars().count();
    out.extend(&text[end..]);
    Some((out, cursor))
}

/// Whether the arrow keys point at cells: a formula whose cursor follows
/// an operator, a `(`, a `,` or the `=`.
pub fn can_point(input: &str, at: usize) -> bool {
    let text = chars(input);
    text.first() == Some(&'=')
        && at > 0
        && text
            .get(..at)
            .and_then(|t| t.last())
            .is_some_and(|c| BEFORE_REFERENCE.contains(c))
}

/// Cells being pointed at while a formula is typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pointing {
    /// Where the reference begins in the formula, in characters.
    pub at: usize,
    /// How many characters it has.
    pub len: usize,
    /// The range's fixed corner (row, column).
    pub anchor: (u32, u32),
    /// The corner the arrows move.
    pub cursor: (u32, u32),
}

impl Pointing {
    /// The range pointed at: first row, first column, last row, last
    /// column.
    pub fn range(&self) -> [u32; 4] {
        let (a, c) = (self.anchor, self.cursor);
        [a.0.min(c.0), a.1.min(c.1), a.0.max(c.0), a.1.max(c.1)]
    }

    /// The reference the formula gets (`B3`, `B3:C5`).
    pub fn reference(&self) -> String {
        let r = self.range();
        let one = |row: u32, col: u32| format!("{}{}", column(col), row + 1);
        if (r[0], r[1]) == (r[2], r[3]) {
            one(r[0], r[1])
        } else {
            format!("{}:{}", one(r[0], r[1]), one(r[2], r[3]))
        }
    }
}

/// An arrow key while a formula is typed: the pointed cell moved (the
/// range grown with `extend`) and its reference put at the cursor, or
/// where the last one was. `from` is the cell being edited, `max` the
/// sheet's rows and columns. The formula and the cursor (characters from
/// the start) after it; `None` when the arrow does not point.
pub fn point(
    state: &mut Option<Pointing>,
    input: &str,
    at: usize,
    from: (u32, u32),
    (rows, cols): (i64, i64),
    extend: bool,
    max: (u32, u32),
) -> Option<(String, usize)> {
    let text = chars(input);
    let mut p = match *state {
        Some(p) if p.at + p.len == at && p.at + p.len <= text.len() => p,
        _ if can_point(input, at) => Pointing {
            at,
            len: 0,
            anchor: from,
            cursor: from,
        },
        _ => {
            *state = None;
            return None;
        }
    };
    let step = |v: u32, d: i64, max: u32| (i64::from(v) + d).clamp(0, i64::from(max) - 1) as u32;
    p.cursor = (step(p.cursor.0, rows, max.0), step(p.cursor.1, cols, max.1));
    if !extend {
        p.anchor = p.cursor;
    }
    let reference = p.reference();
    let mut out: String = text[..p.at].iter().collect();
    out.push_str(&reference);
    let cursor = out.chars().count();
    out.extend(&text[p.at + p.len..]);
    p.len = reference.chars().count();
    *state = Some(p);
    Some((out, cursor))
}

/// What to show while a formula is typed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hint {
    /// The names the word before the cursor begins, best first: functions
    /// (with `(`) and defined names.
    pub completions: Vec<String>,
    /// How many characters of the word are typed.
    pub typed: usize,
    /// The function the cursor is in, with its arguments, the one being
    /// typed between `⟨` and `⟩`.
    pub tip: Option<String>,
}

/// The completions and the argument tip for a formula with the cursor
/// `at` characters in, from the `functions` (name and arguments) and the
/// defined `names`.
pub fn hint(input: &str, at: usize, functions: &[(String, String)], names: &[String]) -> Hint {
    let text = chars(input);
    if text.first() != Some(&'=') {
        return Hint::default();
    }
    let at = at.min(text.len());
    let mut out = Hint::default();
    // In quotes the cursor is in text.
    let quotes = text[..at].iter().filter(|c| **c == '"').count();
    if quotes % 2 == 1 {
        return out;
    }
    let word_char = |c: char| c.is_alphanumeric() || c == '_' || c == '.';
    let mut start = at;
    while start > 1 && word_char(text[start - 1]) {
        start -= 1;
    }
    let word: String = text[start..at].iter().collect();
    let after_word = text.get(at).copied();
    if !word.is_empty()
        && word.chars().next().is_some_and(char::is_alphabetic)
        && after_word.is_none_or(|c| !word_char(c) && c != '(')
    {
        let low = word.to_lowercase();
        let mut fs: Vec<String> = functions
            .iter()
            .filter(|(n, _)| n.to_lowercase().starts_with(&low))
            .map(|(n, _)| format!("{n}("))
            .collect();
        fs.sort_by_key(|n| n.len());
        let ns = names
            .iter()
            .filter(|n| n.to_lowercase().starts_with(&low))
            .cloned();
        out.completions = fs.into_iter().chain(ns).take(50).collect();
        out.typed = word.chars().count();
    }
    // The innermost open call before the cursor, and which argument.
    let mut depth = 0i32;
    let mut commas = 0usize;
    let mut i = at;
    let mut in_quotes = false;
    while i > 0 {
        i -= 1;
        match text[i] {
            '"' => in_quotes = !in_quotes,
            _ if in_quotes => {}
            ')' => depth += 1,
            ',' | ';' if depth == 0 => commas += 1,
            '(' if depth == 0 => {
                let mut s = i;
                while s > 0 && word_char(text[s - 1]) {
                    s -= 1;
                }
                let name: String = text[s..i].iter().collect();
                if let Some((n, args)) = functions
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(&name))
                {
                    let parts: Vec<&str> = args.split(", ").collect();
                    let k = commas.min(parts.len().saturating_sub(1));
                    let shown: Vec<String> = parts
                        .iter()
                        .enumerate()
                        .map(|(j, p)| {
                            // Past the last named one, "..." stands for it.
                            if j == k && !p.is_empty() {
                                format!("⟨{p}⟩")
                            } else {
                                (*p).to_owned()
                            }
                        })
                        .collect();
                    out.tip = Some(format!("{n}({})", shown.join(", ")));
                }
                break;
            }
            '(' => depth -= 1,
            _ => {}
        }
    }
    out
}

/// The formula with the word before the cursor replaced by `completion`;
/// the text and the cursor after it.
pub fn complete(input: &str, at: usize, typed: usize, completion: &str) -> (String, usize) {
    let text = chars(input);
    let at = at.min(text.len());
    let start = at.saturating_sub(typed);
    let mut out: String = text[..start].iter().collect();
    out.push_str(completion);
    let cursor = out.chars().count();
    out.extend(&text[at..]);
    (out, cursor)
}

/// A reference in a formula: its sheet when it names one, the range, and
/// where it lies in the formula (characters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// The sheet it names (`Sheet2!A1`), unquoted.
    pub sheet: Option<String>,
    /// First row, first column, last row, last column.
    pub range: [u32; 4],
    /// Its first character, its sheet included.
    pub start: usize,
    /// The character after it.
    pub end: usize,
}

/// The cell references and ranges of a formula, in order (text in quotes,
/// names and functions left out).
pub fn references(formula: &str) -> Vec<Reference> {
    let text = chars(formula);
    let mut out = Vec::new();
    let mut i = 0;
    let cell = |from: usize| -> Option<(u32, u32, usize)> {
        let (start, end) = {
            let part = |c: char| c.is_ascii_alphanumeric() || c == '$';
            let mut end = from;
            while end < text.len() && part(text[end]) {
                end += 1;
            }
            (from, end)
        };
        if start == end
            || (start > 0 && (text[start - 1].is_alphanumeric() || text[start - 1] == '_'))
        {
            return None;
        }
        if text.get(end) == Some(&'(') {
            return None;
        }
        let s: String = text[start..end].iter().filter(|c| **c != '$').collect();
        let digits_last = s.chars().last().is_some_and(|c| c.is_ascii_digit());
        let letters_first = s.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
        if !(digits_last && letters_first) {
            return None;
        }
        let (r, c) = crate::csv_tools::parse_cell(&s)?;
        Some((r as u32, c as u32, end))
    };
    while i < text.len() {
        let c = text[i];
        if c == '"' {
            i += 1;
            while i < text.len() && text[i] != '"' {
                i += 1;
            }
            i += 1;
            continue;
        }
        // A sheet's name before `!`.
        let mut sheet = None;
        let mut at = i;
        if c == '\'' {
            let mut j = i + 1;
            let mut name = String::new();
            while j < text.len() {
                if text[j] == '\'' {
                    if text.get(j + 1) == Some(&'\'') {
                        name.push('\'');
                        j += 2;
                        continue;
                    }
                    break;
                }
                name.push(text[j]);
                j += 1;
            }
            if text.get(j + 1) == Some(&'!') {
                sheet = Some(name);
                at = j + 2;
            }
        } else if (c.is_alphabetic() || c == '_') && (i == 0 || !text[i - 1].is_alphanumeric()) {
            let mut j = i;
            while j < text.len() && (text[j].is_alphanumeric() || text[j] == '_' || text[j] == '.')
            {
                j += 1;
            }
            if text.get(j) == Some(&'!') {
                sheet = Some(text[i..j].iter().collect());
                at = j + 1;
            }
        }
        if let Some((r, col, end)) = cell(at) {
            let mut range = [r, col, r, col];
            let mut end = end;
            if text.get(end) == Some(&':')
                && let Some((r2, c2, e2)) = cell(end + 1)
            {
                range = [r.min(r2), col.min(c2), r.max(r2), col.max(c2)];
                end = e2;
            }
            out.push(Reference {
                sheet,
                range,
                start: i,
                end,
            });
            i = end;
            continue;
        }
        if sheet.is_some() {
            i = at;
            continue;
        }
        // Past a word (a function or a name).
        if c.is_alphanumeric() || c == '_' {
            while i < text.len() && (text[i].is_alphanumeric() || text[i] == '_' || text[i] == '.')
            {
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    out
}

/// A reference written back as a formula writes it.
fn reference_text(r: &Reference, row: u32, col: u32) -> String {
    let cell = format!("{}{}", column(col), row + 1);
    match &r.sheet {
        Some(s) if s.chars().all(|c| c.is_alphanumeric() || c == '_') => format!("{s}!{cell}"),
        Some(s) => format!("'{}'!{cell}", s.replace('\'', "''")),
        None => cell,
    }
}

/// Evaluate Formula's steps, as Excel's dialog shows them: the formula,
/// then each reference made its value (a range an array), then each
/// innermost function call made its result, to the value. `eval`
/// computes formulas (without `=`) as literals.
pub fn evaluation_steps(
    formula: &str,
    eval: &mut dyn FnMut(&[String]) -> Vec<Option<String>>,
) -> Vec<String> {
    let mut steps = vec![formula.to_owned()];
    let mut now: String = formula.trim_start_matches('=').to_owned();
    for _ in 0..60 {
        let text = chars(&now);
        // A reference: its value.
        if let Some(r) = references(&now).into_iter().next() {
            let [r0, c0, r1, c1] = r.range;
            if (r1 - r0 + 1) * (c1 - c0 + 1) > 200 {
                break;
            }
            let cells: Vec<String> = (r0..=r1)
                .flat_map(|row| (c0..=c1).map(move |col| (row, col)))
                .map(|(row, col)| reference_text(&r, row, col))
                .collect();
            let values = eval(&cells);
            let Some(values) = values.into_iter().collect::<Option<Vec<String>>>() else {
                break;
            };
            let literal = if values.len() == 1 {
                values[0].clone()
            } else {
                let width = (c1 - c0 + 1) as usize;
                let rows: Vec<String> = values.chunks(width).map(|row| row.join(",")).collect();
                format!("{{{}}}", rows.join(";"))
            };
            let mut next: String = text[..r.start].iter().collect();
            next.push_str(&literal);
            next.extend(&text[r.end..]);
            now = next;
            steps.push(format!("={now}"));
            continue;
        }
        // The innermost function call (its arguments all values).
        let mut call = None;
        let mut quoted = false;
        let mut open = None;
        for (i, c) in text.iter().enumerate() {
            match c {
                '"' => quoted = !quoted,
                _ if quoted => {}
                '(' => open = Some(i),
                ')' => {
                    if let Some(o) = open {
                        let mut s = o;
                        while s > 0
                            && (text[s - 1].is_alphanumeric()
                                || text[s - 1] == '.'
                                || text[s - 1] == '_')
                        {
                            s -= 1;
                        }
                        call = Some((s, i + 1));
                        break;
                    }
                }
                _ => {}
            }
        }
        let whole = call.is_none_or(|(s, e)| s == 0 && e == text.len());
        let (s, e) = call.unwrap_or((0, text.len()));
        let piece: String = text[s..e].iter().collect();
        let Some(Some(value)) = eval(std::slice::from_ref(&piece)).into_iter().next() else {
            break;
        };
        if whole && piece == value {
            break;
        }
        let mut next: String = text[..s].iter().collect();
        next.push_str(&value);
        next.extend(&text[e..]);
        now = next;
        steps.push(format!("={now}"));
        if whole {
            break;
        }
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f4_cycles_a_reference() {
        let t = |s: &str, at: usize| toggle_absolute(s, at).map(|x| x.0);
        assert_eq!(t("=A1+B2", 3).as_deref(), Some("=$A$1+B2"));
        assert_eq!(t("=$A$1+B2", 5).as_deref(), Some("=A$1+B2"));
        assert_eq!(t("=A$1+B2", 4).as_deref(), Some("=$A1+B2"));
        assert_eq!(t("=$A1+B2", 4).as_deref(), Some("=A1+B2"));
        // The cursor at the end of a reference, or in it.
        assert_eq!(t("=SUM(B2:C10)", 12).as_deref(), None);
        assert_eq!(t("=SUM(B2:C10)", 11).as_deref(), Some("=SUM(B2:$C$10)"));
        assert_eq!(t("=SUM(B2:C10)", 6).as_deref(), Some("=SUM($B$2:C10)"));
        // Not a function's name, nor a sheet's.
        assert_eq!(t("=SUM(1)", 3), None);
        assert_eq!(t("=LOG10(5)", 5), None);
    }

    #[test]
    fn arrows_point_at_cells() {
        let mut s = None;
        let max = (100, 30);
        // "=" then Down: the cell under the edited one (B3, from B2).
        let (f, at) = point(&mut s, "=", 1, (1, 1), (1, 0), false, max).unwrap();
        assert_eq!((f.as_str(), at), ("=B3", 3));
        // Right again: C3 in its place; Shift+Down: C3:C4.
        let (f, at) = point(&mut s, &f, at, (1, 1), (0, 1), false, max).unwrap();
        assert_eq!(f, "=C3");
        let (f, at) = point(&mut s, &f, at, (1, 1), (1, 0), true, max).unwrap();
        assert_eq!((f.as_str(), at), ("=C3:C4", 6));
        // After "+" a new reference is pointed at.
        let f = format!("{f}+");
        let (f, _) = point(&mut s, &f, 7, (1, 1), (-1, 0), false, max).unwrap();
        assert_eq!(f, "=C3:C4+B1");
        // Not after a reference or in text: the arrows move the cursor.
        let mut s = None;
        assert!(point(&mut s, "=A1", 3, (1, 1), (1, 0), false, max).is_none());
        assert!(point(&mut s, "abc", 3, (1, 1), (1, 0), false, max).is_none());
    }

    #[test]
    fn names_and_arguments_hinted() {
        let functions = vec![
            ("SUM".to_string(), "number1, [number2], ...".to_string()),
            (
                "SUMIF".to_string(),
                "range, criteria, [sum_range]".to_string(),
            ),
            (
                "IF".to_string(),
                "logical_test, value_if_true, [value_if_false]".to_string(),
            ),
        ];
        let names = vec!["Sales".to_string()];
        let h = hint("=su", 3, &functions, &names);
        assert_eq!(h.completions, ["SUM(", "SUMIF("]);
        assert_eq!(h.typed, 2);
        let h = hint("=1+sa", 5, &functions, &names);
        assert_eq!(h.completions, ["Sales"]);
        // In SUMIF's second argument, inside an IF.
        let h = hint("=IF(SUMIF(A1:A3,\"x,y\",", 22, &functions, &names);
        assert_eq!(
            h.tip.as_deref(),
            Some("SUMIF(range, criteria, ⟨[sum_range]⟩)")
        );
        let h = hint("=IF(SUM(1,2),", 13, &functions, &names);
        assert_eq!(
            h.tip.as_deref(),
            Some("IF(logical_test, ⟨value_if_true⟩, [value_if_false])")
        );
        assert_eq!(complete("=1+su", 5, 2, "SUM("), ("=1+SUM(".to_string(), 7));
        assert_eq!(hint("text", 4, &functions, &names), Hint::default());
    }

    #[test]
    fn references_found() {
        let r = references("=SUM(B2:C3)+'My Data'!$A$1*Sheet2!D4-\"A1\"&LOG10(5)");
        let ranges: Vec<_> = r.iter().map(|x| (x.sheet.clone(), x.range)).collect();
        assert_eq!(
            ranges,
            vec![
                (None, [1, 1, 2, 2]),
                (Some("My Data".into()), [0, 0, 0, 0]),
                (Some("Sheet2".into()), [3, 3, 3, 3]),
            ]
        );
    }

    #[test]
    fn formulas_evaluated_in_steps() {
        // An engine that knows a few cells and sums.
        let mut eval = |fs: &[String]| -> Vec<Option<String>> {
            fs.iter()
                .map(|f| {
                    Some(
                        match f.as_str() {
                            "B2" => "10",
                            "B3" => "5",
                            "C1" => "2",
                            "SUM({10;5})" => "15",
                            "15*2" => "30",
                            other => other,
                        }
                        .to_owned(),
                    )
                })
                .collect()
        };
        let steps = evaluation_steps("=SUM(B2:B3)*C1", &mut eval);
        assert_eq!(
            steps,
            [
                "=SUM(B2:B3)*C1",
                "=SUM({10;5})*C1",
                "=SUM({10;5})*2",
                "=15*2",
                "=30"
            ]
        );
    }
}
