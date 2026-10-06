//! Paths typed into a prompt (Add Project's folder, Open File's path):
//! the entries of the folder typed so far whose names begin with what
//! follows its last separator, and Tab completing them, as Emacs's
//! minibuffer completes a file name.

use std::path::{Path, PathBuf};

/// An entry of the folder being typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its name.
    pub name: String,
    /// Whether it is a folder.
    pub dir: bool,
}

/// Whether argument `name` of `command` is a path the prompt completes:
/// `Some(true)` when only folders are wanted.
pub fn path_argument(command: &str, name: &str) -> Option<bool> {
    match (command, name) {
        ("project.add", "path") => Some(true),
        ("file.open", "path") => Some(false),
        _ => None,
    }
}

fn is_separator(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

/// `input` split after its last separator: the folder typed and the
/// beginning of a name.
fn split(input: &str) -> (&str, &str) {
    match input.rfind(is_separator) {
        Some(i) => (&input[..=i], &input[i + 1..]),
        None => ("", input),
    }
}

/// The folder `typed` names: from the home folder for `~`, from `base`
/// when relative.
fn folder(typed: &str, base: Option<&Path>) -> PathBuf {
    let p = PathBuf::from(crate::settings::expand_home(typed));
    if p.is_absolute() {
        return p;
    }
    match base {
        Some(b) => b.join(p),
        None => std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p),
    }
}

/// The most entries listed.
pub const LIMIT: usize = 200;

/// The entries of the folder `input` names up to its last separator
/// whose names begin with the rest of it, ignoring case: folders first,
/// each sorted by name; hidden ones only when the rest begins with a dot;
/// only folders when `folders_only`.
pub fn entries(input: &str, base: Option<&Path>, folders_only: bool) -> Vec<Entry> {
    let (typed, rest) = split(input);
    let Ok(read) = std::fs::read_dir(folder(typed, base)) else {
        return Vec::new();
    };
    let rest = rest.to_lowercase();
    let mut out: Vec<Entry> = read
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && !rest.starts_with('.') {
                return None;
            }
            if !name.to_lowercase().starts_with(&rest) {
                return None;
            }
            // A link to a folder counts as one.
            let dir = std::fs::metadata(e.path()).is_ok_and(|m| m.is_dir());
            (dir || !folders_only).then_some(Entry { name, dir })
        })
        .collect();
    out.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    out.truncate(LIMIT);
    out
}

/// `input` with its last part made `entry`'s name, a folder's followed by
/// a separator so that its own entries come next.
pub fn with_entry(input: &str, entry: &Entry) -> String {
    let (typed, _) = split(input);
    let sep = if entry.dir { "/" } else { "" };
    format!("{typed}{}{sep}", entry.name)
}

/// `input` completed as far as `entries` agree: the one entry, else the
/// beginning their names share (when longer than what is typed).
pub fn complete(input: &str, entries: &[Entry]) -> String {
    match entries {
        [] => input.to_string(),
        [one] => with_entry(input, one),
        [first, rest @ ..] => {
            let (typed, part) = split(input);
            let mut common: &str = &first.name;
            for e in rest {
                let n = common
                    .char_indices()
                    .zip(e.name.chars())
                    .take_while(|((_, a), b)| a == b)
                    .last()
                    .map_or(0, |((i, a), _)| i + a.len_utf8());
                common = &common[..n];
            }
            if common.chars().count() > part.chars().count() {
                format!("{typed}{common}")
            } else {
                input.to_string()
            }
        }
    }
}

/// The entries as a prompt's hint shows them: names, folders with a
/// separator.
pub fn hint(entries: &[Entry]) -> String {
    entries
        .iter()
        .take(40)
        .map(|e| {
            if e.dir {
                format!("{}/", e.name)
            } else {
                e.name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("  ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kalem-path-prompt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["notes", "Novels", "code", ".hidden"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        std::fs::write(dir.join("notes.org"), "").unwrap();
        dir
    }

    #[test]
    fn folders_then_files_beginning_with_the_rest() {
        let dir = tree("list");
        let input = format!("{}/no", dir.display());
        let names: Vec<_> = entries(&input, None, false)
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, ["notes", "Novels", "notes.org"]);
        let folders: Vec<_> = entries(&input, None, true)
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(folders, ["notes", "Novels"]);
        // Hidden ones when a dot is typed.
        let all = entries(&format!("{}/", dir.display()), None, true);
        assert!(!all.iter().any(|e| e.name == ".hidden"));
        let dot = entries(&format!("{}/.", dir.display()), None, true);
        assert_eq!(dot.len(), 1);
        // Relative to the base.
        let rel = entries("co", Some(&dir), true);
        assert_eq!(rel[0].name, "code");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tab_completes_as_far_as_the_entries_agree() {
        let dir = tree("tab");
        let d = dir.display().to_string();
        let one = format!("{d}/co");
        assert_eq!(
            complete(&one, &entries(&one, None, true)),
            format!("{d}/code/")
        );
        let two = format!("{d}/not");
        assert_eq!(
            complete(&two, &entries(&two, None, false)),
            format!("{d}/notes")
        );
        let none = format!("{d}/N");
        let found = entries(&none, None, true);
        assert_eq!(
            complete(&none, &found),
            none,
            "notes and Novels share no more"
        );
        assert_eq!(
            with_entry(&none, &found[1]),
            format!("{d}/Novels/"),
            "a chosen one"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
