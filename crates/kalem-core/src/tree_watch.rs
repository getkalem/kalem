//! The folders a plugin's layer serves (a Logseq graph, an Obsidian
//! vault), watched as a whole: another program writing their files
//! (Logseq, Obsidian, a sync) is heard, each file changed handed to the
//! plugins as `workspace:file-changed` by the editors' loop, so that a
//! plugin's index of the folder follows. The editors' own watching covers
//! the documents they hold; this covers the files they do not.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

struct State {
    /// Each folder watched: its real path, and the path it was named by.
    roots: BTreeMap<PathBuf, PathBuf>,
    /// The files changed since the editors last asked, as named.
    changed: BTreeSet<PathBuf>,
}

/// What the watcher's thread reads and writes.
static STATE: Mutex<State> = Mutex::new(State {
    roots: BTreeMap::new(),
    changed: BTreeSet::new(),
});

/// The watcher, under a lock of its own: adding a folder may wait for its
/// thread (FSEvents restarts), which may be waiting for [`STATE`].
static WATCHER: Mutex<Option<notify::RecommendedWatcher>> = Mutex::new(None);

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A changed file `p` (a real path) under a folder watched, by the path
/// the folder was named by; not in a `.git` folder.
fn named(roots: &BTreeMap<PathBuf, PathBuf>, p: &Path) -> Option<PathBuf> {
    roots.iter().find_map(|(real, named)| {
        let rest = p.strip_prefix(real).ok()?;
        (!rest.components().any(|c| c.as_os_str() == ".git")).then(|| named.join(rest))
    })
}

/// Hears the watcher: the files changed noted, by the names given.
fn heard(res: notify::Result<notify::Event>) {
    let Ok(event) = res else { return };
    if matches!(event.kind, notify::EventKind::Access(_)) {
        return;
    }
    for p in &event.paths {
        // The folder's real path (a file removed has none of its own).
        let real = p
            .parent()
            .and_then(|d| dunce::canonicalize(d).ok())
            .zip(p.file_name())
            .map_or_else(|| p.clone(), |(d, n)| d.join(n));
        let mut s = state();
        if let Some(n) = named(&s.roots, &real) {
            s.changed.insert(n);
        }
    }
}

/// Watches folder `root` and everything under it, once.
pub fn watch(root: &Path) {
    use notify::Watcher;
    let Ok(real) = dunce::canonicalize(root) else {
        return;
    };
    {
        let mut s = state();
        if s.roots.keys().any(|r| real.starts_with(r)) {
            return;
        }
        s.roots.insert(real.clone(), root.to_path_buf());
    }
    let mut w = WATCHER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if w.is_none() {
        match notify::recommended_watcher(heard) {
            Ok(n) => *w = Some(n),
            Err(e) => tracing::warn!("watching {}: {e}", root.display()),
        }
    }
    let watched = w
        .as_mut()
        .map(|w| w.watch(&real, notify::RecursiveMode::Recursive));
    if !matches!(watched, Some(Ok(()))) {
        if let Some(Err(e)) = watched {
            tracing::warn!("watching {}: {e}", root.display());
        }
        state().roots.remove(&real);
    }
}

/// Whether folder `root` is watched.
pub fn watched(root: &Path) -> bool {
    dunce::canonicalize(root).is_ok_and(|real| state().roots.keys().any(|r| real.starts_with(r)))
}

/// The files changed under the folders watched since the last call.
pub fn take_changed() -> Vec<PathBuf> {
    std::mem::take(&mut state().changed).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn a_file_another_program_writes_is_heard() {
        let base = std::env::temp_dir().join(format!("kalem-tree-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("pages")).unwrap();
        std::fs::create_dir_all(base.join(".git")).unwrap();
        watch(&base);
        assert!(watched(&base.join("pages")));
        // Let the watcher start (FSEvents gives what happened before).
        std::thread::sleep(Duration::from_millis(300));
        let _ = take_changed();
        let new = base.join("pages/New.md");
        std::fs::write(&new, "- written elsewhere\n").unwrap();
        std::fs::write(base.join(".git/index"), "x").unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        let mut got = Vec::new();
        while Instant::now() < until {
            got.extend(take_changed());
            if got.iter().any(|p| p.ends_with("pages/New.md")) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(got.iter().any(|p| p.ends_with("pages/New.md")), "{got:?}");
        assert!(
            !got.iter().any(|p| p.to_string_lossy().contains(".git")),
            "{got:?}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
