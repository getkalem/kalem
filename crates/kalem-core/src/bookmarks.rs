//! Bookmarks (Doom's `SPC RET`, `SPC b m`, `SPC b M`; T2.7i.18): named
//! places in files, kept in `bookmarks.json` in the state directory, each
//! a file, a line and a column, and the text of its line to find it again
//! when lines were added above it.

use std::path::{Path, PathBuf};

/// A bookmark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    /// Its name.
    pub name: String,
    /// The file.
    pub path: PathBuf,
    /// The line, from 1.
    pub line: u64,
    /// The byte column.
    pub column: usize,
    /// The text of the line when it was set.
    pub context: String,
}

/// Another file for the bookmarks than the state directory's (tests).
static FILE: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

/// Keeps the bookmarks in `path` rather than the state directory.
pub fn use_file(path: Option<PathBuf>) {
    if let Ok(mut f) = FILE.write() {
        *f = path;
    }
}

/// Where the bookmarks are kept.
fn file() -> Option<PathBuf> {
    if let Some(f) = FILE.read().ok().and_then(|f| f.clone()) {
        return Some(f);
    }
    crate::logging::state_dir().map(|d| d.join("bookmarks.json"))
}

/// The bookmarks in `path` (none when it cannot be read).
pub fn load_from(path: &Path) -> Vec<Bookmark> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str(&text) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|v| {
            Some(Bookmark {
                name: v.get("name")?.as_str()?.to_string(),
                path: PathBuf::from(v.get("path")?.as_str()?),
                line: v.get("line")?.as_u64()?,
                column: v.get("column").and_then(|c| c.as_u64()).unwrap_or(0) as usize,
                context: v
                    .get("context")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

/// Writes `marks` to `path`.
pub fn save_to(path: &Path, marks: &[Bookmark]) -> Result<(), String> {
    let items: Vec<serde_json::Value> = marks
        .iter()
        .map(|b| {
            serde_json::json!({
                "name": b.name,
                "path": b.path.display().to_string(),
                "line": b.line,
                "column": b.column,
                "context": b.context,
            })
        })
        .collect();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&items).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// The bookmarks.
pub fn load() -> Vec<Bookmark> {
    file().map(|f| load_from(&f)).unwrap_or_default()
}

/// Sets bookmark `mark`, replacing one of its name.
pub fn set(mark: Bookmark) -> Result<(), String> {
    let f = file().ok_or("no state directory")?;
    let mut marks = load_from(&f);
    marks.retain(|b| b.name != mark.name);
    marks.push(mark);
    marks.sort_by_key(|b| b.name.to_lowercase());
    save_to(&f, &marks)
}

/// Deletes bookmark `name`; whether there was one.
pub fn delete(name: &str) -> Result<bool, String> {
    let f = file().ok_or("no state directory")?;
    let mut marks = load_from(&f);
    let n = marks.len();
    marks.retain(|b| b.name != name);
    save_to(&f, &marks)?;
    Ok(marks.len() < n)
}

/// Where bookmark `b` is now in `text` (its file's): the line whose text
/// is the bookmark's nearest to the line it was set on, else that line.
pub fn line_now(b: &Bookmark, text: &str) -> u64 {
    if b.context.trim().is_empty() {
        return b.line;
    }
    let lines: Vec<&str> = text.lines().collect();
    let want = b.line.saturating_sub(1) as usize;
    (0..lines.len())
        .filter(|&i| lines[i] == b.context)
        .min_by_key(|&i| i.abs_diff(want))
        .map_or(b.line, |i| i as u64 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_round_trip_and_follow_their_line() {
        let dir = std::env::temp_dir().join(format!("kalem-bookmarks-{}", std::process::id()));
        let f = dir.join("bookmarks.json");
        let b = Bookmark {
            name: "intro".into(),
            path: "/x/a.org".into(),
            line: 3,
            column: 2,
            context: "* Intro".into(),
        };
        save_to(&f, std::slice::from_ref(&b)).unwrap();
        assert_eq!(load_from(&f), std::slice::from_ref(&b));
        // Two lines added above: the bookmark follows its line.
        assert_eq!(line_now(&b, "new\nnew\nx\ny\n* Intro\n"), 5);
        assert_eq!(line_now(&b, "gone\n"), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
