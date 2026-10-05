//! Find and replace in a document's text, shared by the frontends.
//!
//! Searches are literal, or regular expressions with the `regex` option
//! (Rust's `regex` syntax, `^` and `$` at line ends; `$1` and `${name}` in
//! replacements). They ignore case unless the query has an upper case
//! letter (as Emacs's `case-fold-search` with `search-upper-case`).

use std::ops::Range;

use org_edit::{Selection, Transaction};

/// How to search.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FindOptions {
    /// The query is a regular expression.
    pub regex: bool,
}

/// Whether a search for `query` distinguishes case.
pub fn case_sensitive(query: &str) -> bool {
    query.chars().any(char::is_uppercase)
}

/// [`case_sensitive`] for a regular expression: letters after a
/// backslash (`\S`, `\W`) are escapes, not upper case.
fn regex_case_sensitive(query: &str) -> bool {
    let mut escaped = false;
    for c in query.chars() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c.is_uppercase() {
            return true;
        }
    }
    false
}

fn compile(query: &str) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(query)
        .multi_line(true)
        .case_insensitive(!regex_case_sensitive(query))
        .build()
        .map_err(|e| {
            // The last line of a syntax error says what is wrong.
            let s = e.to_string();
            let last = s.lines().last().unwrap_or("").trim();
            let msg = last.strip_prefix("error:").unwrap_or(last).trim();
            let mut c = msg.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_else(|| "Invalid regular expression".into())
        })
}

/// The byte ranges of every match of `query` in `text`, in order and
/// without overlaps; an error for a regular expression that does not
/// compile. Empty matches are left out.
pub fn find_with(text: &str, query: &str, opts: FindOptions) -> Result<Vec<Range<usize>>, String> {
    if !opts.regex {
        return Ok(find_all(text, query));
    }
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let re = compile(query)?;
    Ok(re
        .find_iter(text)
        .filter(|m| !m.is_empty())
        .map(|m| m.range())
        .collect())
}

/// The byte ranges of every literal match of `query` in `text`, in order
/// and without overlaps.
pub fn find_all(text: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    if case_sensitive(query) {
        return text
            .match_indices(query)
            .map(|(i, m)| i..i + m.len())
            .collect();
    }
    // Fold each character to the first character of its lower case, as
    // Emacs's case table does (`İ` and `I` fold to `i`), so byte offsets
    // stay those of `text`. Character by character, with nothing the size
    // of the text allocated: Find runs at every keystroke of the query
    // (publish_todo 3.6).
    let fold = |c: char| {
        if c.is_ascii() {
            c.to_ascii_lowercase()
        } else {
            c.to_lowercase().next().unwrap_or(c)
        }
    };
    let q: Vec<char> = query.chars().map(fold).collect();
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(c) = text[pos..].chars().next() {
        if fold(c) == q[0] {
            // The query from here, or not.
            let mut end = pos;
            let mut rest = text[pos..].chars();
            let whole = q.iter().all(|want| match rest.next() {
                Some(c) if fold(c) == *want => {
                    end += c.len_utf8();
                    true
                }
                _ => false,
            });
            if whole {
                out.push(pos..end);
                pos = end;
                continue;
            }
        }
        pos += c.len_utf8();
    }
    out
}

/// The text replacing the match `m` of `query`: `with` itself, or for a
/// regular expression `with` with `$1` and `${name}` expanded.
pub fn replacement(
    text: &str,
    m: Range<usize>,
    query: &str,
    with: &str,
    opts: FindOptions,
) -> Result<String, String> {
    if !opts.regex {
        return Ok(with.to_string());
    }
    let re = compile(query)?;
    let caps = re
        .captures_at(text, m.start)
        .filter(|c| c.get(0).is_some_and(|g| g.range() == m))
        .ok_or_else(|| "The match changed".to_string())?;
    let mut out = String::new();
    caps.expand(with, &mut out);
    Ok(out)
}

/// The first match at or after `from`, wrapping around to the start; or
/// before `from` (wrapping to the end) when `backward`.
pub fn next(matches: &[Range<usize>], from: usize, backward: bool) -> Option<Range<usize>> {
    if backward {
        matches
            .iter()
            .rev()
            .find(|m| m.start < from)
            .or(matches.last())
            .cloned()
    } else {
        matches
            .iter()
            .find(|m| m.start >= from)
            .or(matches.first())
            .cloned()
    }
}

/// Replaces every match of `query` with `with`, as one step; the cursor
/// goes after the last replacement.
pub fn replace_all(text: &str, query: &str, with: &str) -> Option<Transaction> {
    replace_all_with(text, query, with, FindOptions::default())
        .ok()
        .flatten()
}

/// [`replace_all`] with options: `None` when nothing matches.
pub fn replace_all_with(
    text: &str,
    query: &str,
    with: &str,
    opts: FindOptions,
) -> Result<Option<Transaction>, String> {
    let matches = find_with(text, query, opts)?;
    let Some(last) = matches.last().cloned() else {
        return Ok(None);
    };
    let re = if opts.regex {
        Some(compile(query)?)
    } else {
        None
    };
    let mut tx = Transaction::new("Replace all");
    let mut shift: isize = 0;
    for m in &matches {
        let new = match &re {
            Some(re) => {
                let mut out = String::new();
                if let Some(c) = re.captures_at(text, m.start) {
                    c.expand(with, &mut out);
                }
                out
            }
            None => with.to_string(),
        };
        shift += new.len() as isize - m.len() as isize;
        tx.edit(m.clone(), new);
    }
    let end = (last.end as isize + shift) as usize;
    Ok(Some(tx.select(Selection::caret(end))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // A single match is a one-range list.
    #[allow(clippy::single_range_in_vec_init)]
    fn finding() {
        let t = "Foo foo FOO fOo";
        assert_eq!(find_all(t, "foo"), [0..3, 4..7, 8..11, 12..15]);
        assert_eq!(find_all(t, "Foo"), [0..3]);
        assert_eq!(find_all("aaaa", "aa"), [0..2, 2..4]);
        // Turkish capitals fold to `i`; offsets are the text's.
        assert_eq!(find_all("İstanbul ı", "istanbul"), [0..9]);
        assert_eq!(find_all("Straße STRASSE", "straße"), [0..7]);
        // Byte offsets of the text, a match never overlapping another.
        assert_eq!(find_all("ŞİŞE şişe", "şişe"), [0..7, 8..14]);
        assert_eq!(find_all("aAaA", "aa"), [0..2, 2..4]);
        let m = find_all(t, "foo");
        assert_eq!(next(&m, 5, false), Some(8..11));
        assert_eq!(next(&m, 13, false), Some(0..3));
        assert_eq!(next(&m, 4, true), Some(0..3));
        assert_eq!(next(&m, 0, true), Some(12..15));
        let tx = replace_all(t, "foo", "x").unwrap();
        assert_eq!(tx.apply(t), "x x x x");
        assert_eq!(tx.selection_after.unwrap().head, 7);
    }

    #[test]
    // A single match is a one-range list.
    #[allow(clippy::single_range_in_vec_init)]
    fn regular_expressions() {
        let re = FindOptions { regex: true };
        let t = "* TODO one\n** DONE two\ntext\n";
        assert_eq!(find_with(t, r"^\*+ \w+", re).unwrap(), [0..6, 11..18]);
        // Smart case: `\S` is not upper case, `DONE` is.
        assert_eq!(find_with(t, r"todo\S*", re).unwrap(), [2..6]);
        assert!(find_with(t, r"Done", re).unwrap().is_empty());
        // Empty matches are left out.
        assert!(find_with(t, "x*", re).unwrap().len() == 1);
        assert!(find_with(t, "(", re).is_err());
        let tx = replace_all_with(t, r"(\w+) (\w+)$", "$2 ${1}!", re)
            .unwrap()
            .unwrap();
        assert_eq!(tx.apply(t), "* one TODO!\n** two DONE!\ntext\n");
        assert_eq!(
            replacement(t, 2..10, r"(\w+) (\w+)", "$2", re).unwrap(),
            "one"
        );
        assert_eq!(
            replacement(t, 2..10, "x", "$2", FindOptions::default()).unwrap(),
            "$2"
        );
    }
}
