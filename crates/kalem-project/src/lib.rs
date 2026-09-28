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
    let t: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let mut score = 0i64;
    let mut at = 0usize;
    let mut last: Option<usize> = None;
    for q in query.chars().flat_map(char::to_lowercase) {
        if q == ' ' {
            continue;
        }
        let i = at + t.get(at..)?.iter().position(|c| *c == q)?;
        let word_start = i == 0 || !t[i - 1].is_alphanumeric();
        score += match last {
            Some(l) if l + 1 == i => 0,
            _ if word_start => 1,
            _ => 3 + (i - at) as i64,
        };
        last = Some(i);
        at = i + 1;
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
}
