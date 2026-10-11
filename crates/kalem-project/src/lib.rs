//! Projects for text editors, a simple Projectile (Kalem's design, §2.8).
//!
//! A project is a folder the user added to the project list; nothing in
//! the folder marks it. This crate keeps the list ([`Projects`], a TOML
//! file in the user's configuration), walks a project's files with
//! ripgrep's ignore rules ([`FileIndex`], kept current by a file watcher)
//! and searches their text ([`Search`]). It knows nothing of the editor.

pub mod files;
pub mod list;
pub mod search;

pub use files::{FileIndex, walk};
pub use list::{Project, Projects};
pub use search::{Hit, Query, Search};

/// Fuzzy matching of `query` in `text`: every query character in order,
/// ignoring case. Lower scores are better: early, contiguous matches and
/// matches at word starts. Spaces in the query are ignored.
pub fn fuzzy(query: &str, text: &str) -> Option<i64> {
    // A path or a title is mostly ASCII: its bytes lowered as they come,
    // fourteen times as fast as `char::to_lowercase` (a project's file
    // list is matched whole at each keystroke).
    if text.is_ascii() {
        return fuzzy_in(
            query,
            text.bytes().map(|b| char::from(b.to_ascii_lowercase())),
        );
    }
    fuzzy_in(query, text.chars().flat_map(char::to_lowercase))
}

/// [`fuzzy`] over the text's characters in lower case, `t`: one pass, as
/// the matches only move forward, with no copy of the text.
fn fuzzy_in(query: &str, mut t: impl Iterator<Item = char>) -> Option<i64> {
    let mut score = 0i64;
    // The index of `t`'s next character, and the character before it.
    let mut at = 0usize;
    let mut before: Option<char> = None;
    let mut last: Option<usize> = None;
    for q in query.chars().flat_map(char::to_lowercase) {
        if q == ' ' {
            continue;
        }
        let from = at;
        let (i, prev) = loop {
            let c = t.next()?;
            let (i, prev) = (at, before);
            at += 1;
            before = Some(c);
            if c == q {
                break (i, prev);
            }
        };
        let word_start = prev.is_none_or(|p| !p.is_alphanumeric());
        score += match last {
            Some(l) if l + 1 == i => 0,
            _ if word_start => 1,
            _ => 3 + (i - from) as i64,
        };
        last = Some(i);
    }
    Some(score)
}

/// Fuzzy matching of `query` in a relative file path: matches in the file
/// name count most, then matches across the whole path.
pub fn fuzzy_path(query: &str, path: &str) -> Option<i64> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match fuzzy(query, name) {
        Some(s) => Some(s + path.len() as i64 / 16),
        None => fuzzy(query, path).map(|s| s + 20 + path.len() as i64 / 8),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_paths() {
        assert!(
            fuzzy_path("todo", "notes/todo.org").unwrap()
                < fuzzy_path("todo", "t/o/d/o.org").unwrap()
        );
        assert!(fuzzy_path("nto", "notes/todo.org").is_some());
        assert!(fuzzy_path("xyz", "notes/todo.org").is_none());
        assert_eq!(fuzzy("", "a"), Some(0));
    }

    #[test]
    fn fuzzy_scores_ascii_and_other_text_alike() {
        // The scores of the matching that copied the text into characters.
        for (q, t, s) in [
            ("kp", "crates/kalem-project/src/lib.rs", Some(2)),
            ("lib", "crates/kalem-project/src/lib.rs", Some(31)),
            ("çay", "Bir Çay Tabağı", Some(1)),
            ("tb", "Bir Çay Tabağı", Some(5)),
            ("ıi", "Iİıi", Some(1)),
            ("é", "CAFÉ", Some(6)),
            ("ae", "ÆAEae", Some(4)),
            ("x", "Bir Çay", None),
        ] {
            assert_eq!(fuzzy(q, t), s, "{q} in {t}");
        }
    }
}
