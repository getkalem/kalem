//! Queries over the model: allowed property values and lookups by tag,
//! ID and date.

use jiff::civil::Date;

use crate::properties::SPECIAL_PROPERTIES;
use crate::{Document, EntryId, Inherit};

/// Allowed values of a property (`org-property-get-allowed-values`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AllowedValues {
    /// The values, in order.
    pub values: Vec<String>,
    /// Whether `:ETC` allows other values too.
    pub unrestricted: bool,
}

/// Emacs's `number-to-string` for the numbers the reader produces.
fn number_to_string(tok: &str) -> Option<String> {
    let t = tok.strip_prefix('+').unwrap_or(tok);
    let body = t.strip_prefix('-').unwrap_or(t);
    let neg = t.starts_with('-');
    // Integer: `[0-9]+` with an optional trailing dot.
    let int = body.strip_suffix('.').unwrap_or(body);
    if !int.is_empty() && int.bytes().all(|b| b.is_ascii_digit()) {
        let trimmed = int.trim_start_matches('0');
        let digits = if trimmed.is_empty() { "0" } else { trimmed };
        return Some(if neg && digits != "0" {
            format!("-{digits}")
        } else {
            digits.to_string()
        });
    }
    // Float: digits with a fraction or an exponent.
    let has_digit = body.bytes().any(|b| b.is_ascii_digit());
    let valid = has_digit
        && body
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
        && (body.contains('.') || body.contains(['e', 'E']));
    if valid && let Ok(f) = t.parse::<f64>() {
        let s = format!("{f:?}");
        return Some(if let Some((m, e)) = s.split_once('e') {
            if e.starts_with('-') {
                format!("{m}e{e}")
            } else {
                format!("{m}e+{e}")
            }
        } else {
            s
        });
    }
    None
}

/// Reads `( VALUES )` as `read-from-string` does, for the atoms used in
/// `_ALL` properties: strings, numbers and symbols (anything else is
/// `???`).
pub(crate) fn read_values(s: &str) -> Vec<String> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch == ';' {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '"' {
            let mut v = String::new();
            i += 1;
            while i < c.len() && c[i] != '"' {
                if c[i] == '\\' && i + 1 < c.len() {
                    i += 1;
                    match c[i] {
                        'n' => v.push('\n'),
                        't' => v.push('\t'),
                        '\n' | ' ' => {}
                        x => v.push(x),
                    }
                } else {
                    v.push(c[i]);
                }
                i += 1;
            }
            i += 1;
            out.push(v);
        } else if ch == '(' || ch == '[' {
            // A list or vector: skip to its end.
            let (open, close) = if ch == '(' { ('(', ')') } else { ('[', ']') };
            let mut depth = 0;
            while i < c.len() {
                if c[i] == open {
                    depth += 1;
                } else if c[i] == close {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                i += 1;
            }
            out.push("???".into());
        } else if ch == '\'' || ch == '`' {
            i += 1;
            let rest: String = c[i..].iter().collect();
            let mut inner = read_values(&rest);
            if !inner.is_empty() {
                inner.truncate(1);
                // `'x` reads as `(quote x)`, a list.
                out.push("???".into());
            }
            break;
        } else {
            let mut tok = String::new();
            while i < c.len()
                && !c[i].is_whitespace()
                && !matches!(c[i], '(' | ')' | '"' | ';' | '[' | ']')
            {
                if c[i] == '\\' && i + 1 < c.len() {
                    i += 1;
                }
                tok.push(c[i]);
                i += 1;
            }
            if tok.is_empty() {
                i += 1;
                continue;
            }
            out.push(number_to_string(&tok).unwrap_or(tok));
        }
    }
    out
}

impl Document {
    /// `org-property-get-allowed-values`.
    pub fn allowed_values(&self, id: Option<EntryId>, property: &str) -> AllowedValues {
        let ctx = self.parse.context();
        let mut values: Vec<String> = match property {
            "TODO" => {
                let mut v: Vec<String> = ctx
                    .todo_sequences
                    .iter()
                    .flat_map(|s| s.keywords.iter().map(|k| k.name.clone()))
                    .collect();
                v.push(String::new());
                v
            }
            "PRIORITY" => {
                let p = self.info().priorities;
                (p.highest..=p.lowest)
                    .filter_map(char::from_u32)
                    .map(|c| c.to_string())
                    .collect()
            }
            "CATEGORY" => Vec::new(),
            p if SPECIAL_PROPERTIES.contains(&p) => Vec::new(),
            p => match self.entry_get(id, &format!("{p}_ALL"), Inherit::Yes, false) {
                Some(v) if v.chars().any(|c| !c.is_whitespace()) => read_values(&v),
                _ => Vec::new(),
            },
        };
        let unrestricted = values.iter().any(|v| v == ":ETC");
        values.retain(|v| v != ":ETC");
        AllowedValues {
            values,
            unrestricted,
        }
    }

    /// Entries whose tags, inherited ones included, contain `tag`.
    pub fn headlines_with_tag(&self, tag: &str) -> Vec<EntryId> {
        (0..self.outline().entries.len())
            .map(EntryId)
            .filter(|id| self.tags(*id).iter().any(|t| t == tag))
            .collect()
    }

    /// The first entry whose `ID` property is `id`.
    pub fn find_by_id(&self, id: &str) -> Option<EntryId> {
        self.find_by_property("ID", id)
    }

    /// The first entry whose `CUSTOM_ID` property is `id`.
    pub fn find_by_custom_id(&self, id: &str) -> Option<EntryId> {
        self.find_by_property("CUSTOM_ID", id)
    }

    fn find_by_property(&self, prop: &str, value: &str) -> Option<EntryId> {
        (0..self.outline().entries.len()).map(EntryId).find(|e| {
            self.entry_get(Some(*e), prop, Inherit::No, false)
                .as_deref()
                == Some(value)
        })
    }

    /// Entries scheduled on a date from `from` to `to`, both included
    /// (repeaters are not expanded).
    pub fn scheduled_between(&self, from: Date, to: Date) -> Vec<EntryId> {
        (0..self.outline().entries.len())
            .map(EntryId)
            .filter(|id| {
                self.planning(*id)
                    .scheduled
                    .and_then(|t| t.start)
                    .is_some_and(|s| s.date() >= from && s.date() <= to)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lisp_reader_subset() {
        assert_eq!(
            read_values("0 0:10 0:30 1:00"),
            vec!["0", "0:10", "0:30", "1:00"]
        );
        assert_eq!(
            read_values("\"a b\" c 01 1.50 :ETC"),
            vec!["a b", "c", "1", "1.5", ":ETC"]
        );
        assert_eq!(read_values("(a b) x"), vec!["???", "x"]);
        assert_eq!(read_values("2. -0 3e2"), vec!["2", "0", "300.0"]);
    }

    #[test]
    fn allowed_values_and_queries() {
        let text = "#+PROPERTY: Effort_ALL 0 0:10 1:00\n* A :x:\n:PROPERTIES:\n:ID: abc\n:Size_ALL: s m :ETC\n:END:\n** B\nSCHEDULED: <2026-10-01 Thu>\n";
        let doc = Document::new(org_syntax::parse(text));
        let b = EntryId(1);
        assert_eq!(
            doc.allowed_values(Some(b), "Effort").values,
            vec!["0", "0:10", "1:00"]
        );
        let size = doc.allowed_values(Some(b), "Size");
        assert_eq!(
            (size.values, size.unrestricted),
            (vec!["s".to_string(), "m".to_string()], true)
        );
        assert_eq!(
            doc.allowed_values(Some(b), "PRIORITY").values,
            vec!["A", "B", "C"]
        );
        assert_eq!(doc.find_by_id("abc"), Some(EntryId(0)));
        assert_eq!(doc.headlines_with_tag("x"), vec![EntryId(0), EntryId(1)]);
        let d = jiff::civil::date(2026, 10, 1);
        assert_eq!(doc.scheduled_between(d, d), vec![b]);
    }
}
