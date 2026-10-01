//! Sessions (Doom's `SPC q s`, `SPC q l`, `SPC q S`, `SPC q L`; T2.7i.14):
//! the open documents, where the cursor was in each, which one was shown
//! and the project, kept in `sessions/NAME.json` in the state directory.
//! The session called [`LAST`] is saved when Kalem quits and restored by
//! `SPC q l`, and on start when `editor.restore_session` says so.

use std::path::PathBuf;

/// The session saved on quitting.
pub const LAST: &str = "last";

/// An open document in a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDoc {
    /// The file.
    pub path: PathBuf,
    /// The cursor's line, from 1.
    pub line: u64,
    /// The cursor's byte column.
    pub column: usize,
}

/// A session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Session {
    /// The open documents with a file, in order.
    pub documents: Vec<SessionDoc>,
    /// The one shown, by its place in `documents`.
    pub active: usize,
    /// The project.
    pub project: Option<PathBuf>,
}

impl Session {
    /// The session as JSON.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "documents": self.documents.iter().map(|d| serde_json::json!({
                "path": d.path.display().to_string(),
                "line": d.line,
                "column": d.column,
            })).collect::<Vec<_>>(),
            "active": self.active,
            "project": self.project.as_ref().map(|p| p.display().to_string()),
        })
    }

    /// A session read from JSON; the documents it cannot read are left
    /// out.
    pub fn from_json(v: &serde_json::Value) -> Session {
        let documents = v
            .get("documents")
            .and_then(|d| d.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|d| {
                        Some(SessionDoc {
                            path: PathBuf::from(d.get("path")?.as_str()?),
                            line: d.get("line").and_then(|l| l.as_u64()).unwrap_or(1).max(1),
                            column: d.get("column").and_then(|c| c.as_u64()).unwrap_or(0) as usize,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Session {
            documents,
            active: v.get("active").and_then(|a| a.as_u64()).unwrap_or(0) as usize,
            project: v.get("project").and_then(|p| p.as_str()).map(PathBuf::from),
        }
    }

    /// The documents whose files are still there, the active one kept.
    pub fn existing(mut self) -> Session {
        let active = self.documents.get(self.active).map(|d| d.path.clone());
        self.documents.retain(|d| d.path.is_file());
        self.active = active
            .and_then(|a| self.documents.iter().position(|d| d.path == a))
            .unwrap_or(0);
        self
    }
}

/// Another folder for the sessions than the state directory's (tests).
static DIR: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

/// Keeps the sessions in `dir` rather than the state directory.
pub fn use_dir(dir: Option<PathBuf>) {
    if let Ok(mut d) = DIR.write() {
        *d = dir;
    }
}

/// Where the sessions are kept.
pub fn dir() -> Option<PathBuf> {
    if let Some(d) = DIR.read().ok().and_then(|d| d.clone()) {
        return Some(d);
    }
    crate::logging::state_dir().map(|d| d.join("sessions"))
}

/// The file of session `name`: a name in the sessions folder, or a path
/// (with a `/` or ending in `.json`) as it is.
pub fn file(name: &str) -> Option<PathBuf> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    if name.contains(std::path::MAIN_SEPARATOR) || name.contains('/') || name.ends_with(".json") {
        return Some(PathBuf::from(crate::settings::expand_home(name)));
    }
    dir().map(|d| d.join(format!("{name}.json")))
}

/// The marker that the next start restores the last session (`SPC q r`).
fn restore_marker() -> Option<PathBuf> {
    dir().map(|d| d.join(".restore"))
}

/// The next start restores the last session, whatever the settings.
pub fn restore_on_next_start() -> Result<(), String> {
    let f = restore_marker().ok_or_else(|| crate::tr!("msg-no-state-dir"))?;
    if let Some(d) = f.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&f, "").map_err(|e| e.to_string())
}

/// Whether this start restores the last session because of
/// [`restore_on_next_start`] (asked once).
pub fn take_restore_request() -> bool {
    restore_marker().is_some_and(|f| std::fs::remove_file(f).is_ok())
}

/// Saves `session` as `name`.
pub fn save(name: &str, session: &Session) -> Result<PathBuf, String> {
    let f = file(name).ok_or_else(|| crate::tr!("msg-no-state-dir"))?;
    if let Some(d) = f.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&session.to_json()).map_err(|e| e.to_string())?;
    std::fs::write(&f, text).map_err(|e| e.to_string())?;
    Ok(f)
}

/// Session `name`.
pub fn load(name: &str) -> Result<Session, String> {
    let f = file(name).ok_or_else(|| crate::tr!("msg-no-state-dir"))?;
    let text = std::fs::read_to_string(&f)
        .map_err(|_| crate::tr!("msg-no-session", name = name.to_string()))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(Session::from_json(&v))
}

/// The saved sessions' names, the last one first.
pub fn names() -> Vec<String> {
    let Some(d) = dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(d) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| {
            let p = e.ok()?.path();
            if p.extension()? != "json" {
                return None;
            }
            Some(p.file_stem()?.to_string_lossy().into_owned())
        })
        .collect();
    names.sort_by_key(|n| (n != LAST, n.to_lowercase()));
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_round_trip() {
        let dir = std::env::temp_dir().join(format!("kalem-sessions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.org");
        std::fs::write(&a, "* A\n").unwrap();
        let s = Session {
            documents: vec![
                SessionDoc {
                    path: dir.join("gone.org"),
                    line: 3,
                    column: 1,
                },
                SessionDoc {
                    path: a.clone(),
                    line: 1,
                    column: 2,
                },
            ],
            active: 1,
            project: Some(dir.clone()),
        };
        assert_eq!(Session::from_json(&s.to_json()), s);
        // Saved to a file named by a path, and read back.
        let f = dir.join("work.json");
        let name = f.display().to_string();
        save(&name, &s).unwrap();
        assert_eq!(load(&name).unwrap(), s);
        // A file that is gone is left out; the active one stays active.
        let e = s.existing();
        assert_eq!(e.documents.len(), 1);
        assert_eq!(e.active, 0);
        assert!(load(&dir.join("none.json").display().to_string()).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
