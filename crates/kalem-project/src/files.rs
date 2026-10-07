//! A project's files: walked in the background with ripgrep's ignore
//! rules, and walked again when the file watcher sees files come or go.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Folders of version control systems, never walked.
const VCS: &[&str] = &[
    ".git", ".hg", ".svn", ".bzr", "_darcs", ".jj", ".pijul", "CVS",
];

/// Extensions of files that are binary for sure, skipped without reading.
const BINARY: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "icns", "tif", "tiff", "psd", "pdf", "zip",
    "gz", "tgz", "bz2", "xz", "zst", "7z", "rar", "jar", "class", "o", "a", "so", "dylib", "dll",
    "exe", "bin", "wasm", "mp3", "mp4", "m4a", "mov", "avi", "mkv", "wav", "flac", "ogg", "ttf",
    "otf", "woff", "woff2", "sqlite", "db", "pyc", "DS_Store",
];

/// Whether the file at `path` looks binary: a known extension, or a NUL
/// byte in its first 8 KB.
fn binary(path: &Path) -> bool {
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| BINARY.iter().any(|b| b.eq_ignore_ascii_case(e)))
    {
        return true;
    }
    let mut buf = [0u8; 8192];
    match std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)) {
        Ok(n) => buf[..n].contains(&0),
        Err(_) => true,
    }
}

/// A walker over `root` with ripgrep's rules: `.gitignore` (with or
/// without a repository), `.ignore`, git's global excludes, and `ignore`
/// (gitignore patterns) on top; hidden files are included, version
/// control folders are not.
pub(crate) fn walker(root: &Path, ignore: &[String]) -> ignore::WalkBuilder {
    let mut b = ignore::WalkBuilder::new(root);
    b.hidden(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|e| {
            !(e.file_type().is_some_and(|t| t.is_dir()) && VCS.iter().any(|v| e.file_name() == *v))
        });
    if !ignore.is_empty() {
        let mut o = ignore::overrides::OverrideBuilder::new(root);
        for pat in ignore {
            if let Err(e) = o.add(&format!("!{pat}")) {
                tracing::warn!("ignore pattern {pat}: {e}");
            }
        }
        match o.build() {
            Ok(o) => {
                b.overrides(o);
            }
            Err(e) => tracing::warn!("ignore patterns: {e}"),
        }
    }
    b
}

/// Walks the text files of `root`, calling `sink` with each one's path
/// relative to `root`, until `cancel` is set.
pub fn walk(root: &Path, ignore: &[String], cancel: &AtomicBool, mut sink: impl FnMut(PathBuf)) {
    for entry in walker(root, ignore).build() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let Ok(e) = entry else { continue };
        if !e.file_type().is_some_and(|t| t.is_file()) || binary(e.path()) {
            continue;
        }
        if let Ok(rel) = e.path().strip_prefix(root) {
            sink(rel.to_path_buf());
        }
    }
}

/// The ignore files at the top of `root` and the project's own patterns,
/// for telling events in ignored folders apart (`target/` while building);
/// those deeper are not read, their events walking again as before.
fn top_ignores(root: &Path, ignore: &[String]) -> Option<ignore::gitignore::Gitignore> {
    let mut b = ignore::gitignore::GitignoreBuilder::new(root);
    for name in [".gitignore", ".ignore"] {
        let path = root.join(name);
        if path.is_file() {
            let _ = b.add(path);
        }
    }
    for pat in ignore {
        let _ = b.add_line(None, pat);
    }
    b.build().ok()
}

/// Whether an event at `p` may change the files found (`files`, sorted):
/// a file the walk would list that is not among them, one of them gone, a
/// folder that came or went. A save (a copy written beside the file and
/// renamed over it) and the changes in a version control folder or an
/// ignored one are none: each walked the whole project again.
fn comes_or_goes(
    root: &Path,
    ignored: Option<&ignore::gitignore::Gitignore>,
    files: &[PathBuf],
    p: &Path,
) -> bool {
    let Ok(rel) = p.strip_prefix(root) else {
        return true;
    };
    if rel
        .components()
        .any(|c| VCS.iter().any(|v| c.as_os_str() == *v))
    {
        return false;
    }
    let meta = std::fs::symlink_metadata(p).ok();
    let dir = meta.as_ref().is_some_and(std::fs::Metadata::is_dir);
    if ignored.is_some_and(|g| g.matched_path_or_any_parents(rel, dir).is_ignore()) {
        return false;
    }
    if dir {
        return true;
    }
    let at = files.partition_point(|f| f.as_path() < rel);
    let known = files.get(at).is_some_and(|f| f == rel);
    // A folder gone with files found in it.
    let held = files
        .get(at)
        .is_some_and(|f| f.starts_with(rel) && f != rel);
    match (known, meta.is_some_and(|m| m.is_file())) {
        (true, true) => false,
        (false, true) => !binary(p),
        (true, false) => true,
        (false, false) => held,
    }
}

#[derive(Debug, Default)]
struct State {
    /// Relative paths, sorted once the walk is done.
    files: Arc<Vec<PathBuf>>,
    /// The walk is done.
    done: bool,
    /// Files came or went since the walk started.
    stale: bool,
    /// Bumped by every walk, so an old walk's results are dropped.
    generation: u64,
}

/// The files of a project, walked in the background.
pub struct FileIndex {
    root: PathBuf,
    ignore: Vec<String>,
    state: Arc<Mutex<State>>,
    cancel: Arc<AtomicBool>,
    _watcher: Option<notify::RecommendedWatcher>,
}

impl std::fmt::Debug for FileIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.state.lock().expect("the index");
        f.debug_struct("FileIndex")
            .field("root", &self.root)
            .field("files", &s.files.len())
            .field("done", &s.done)
            .finish()
    }
}

impl FileIndex {
    /// Starts walking `root`, and watches it for files that come or go.
    pub fn new(root: &Path, ignore: &[String]) -> FileIndex {
        let state = Arc::new(Mutex::new(State::default()));
        let watcher = {
            use notify::Watcher;
            let st = state.clone();
            let base = root.to_path_buf();
            let ignored = top_ignores(root, ignore);
            let w = notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
                let Ok(ev) = ev else { return };
                use notify::EventKind as K;
                let structural = matches!(
                    ev.kind,
                    K::Create(_) | K::Remove(_) | K::Modify(notify::event::ModifyKind::Name(_))
                ) || matches!(ev.kind, K::Any | K::Other);
                if !structural {
                    return;
                }
                // Looked at without the lock, which the editor waits for.
                let (done, files) = match st.lock() {
                    Ok(s) if !s.stale => (s.done, s.files.clone()),
                    _ => return,
                };
                let changed = !done
                    || ev.paths.is_empty()
                    || ev
                        .paths
                        .iter()
                        .any(|p| comes_or_goes(&base, ignored.as_ref(), &files, p));
                if changed && let Ok(mut s) = st.lock() {
                    s.stale = true;
                }
            });
            w.and_then(|mut w| {
                w.watch(root, notify::RecursiveMode::Recursive)?;
                Ok(w)
            })
            .map_err(|e| tracing::info!("not watching {}: {e}", root.display()))
            .ok()
        };
        let idx = FileIndex {
            root: root.to_path_buf(),
            ignore: ignore.to_vec(),
            state,
            cancel: Arc::new(AtomicBool::new(false)),
            _watcher: watcher,
        };
        idx.start();
        idx
    }

    fn start(&self) {
        let generation = {
            let mut s = self.state.lock().expect("the index");
            s.generation += 1;
            s.done = false;
            s.stale = false;
            s.generation
        };
        let (root, ignore, state, cancel) = (
            self.root.clone(),
            self.ignore.clone(),
            self.state.clone(),
            self.cancel.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("kalem-project-walk".into())
            .spawn(move || {
                let mut batch = Vec::new();
                let mut found: Vec<PathBuf> = Vec::new();
                let publish = |found: &[PathBuf], done: bool| {
                    let mut s = state.lock().expect("the index");
                    if s.generation == generation {
                        s.files = Arc::new(found.to_vec());
                        s.done = done;
                    }
                };
                walk(&root, &ignore, &cancel, |p| {
                    batch.push(p);
                    // Publish now and then, so a picker fills while the
                    // walk goes on.
                    if batch.len() >= 2000 {
                        found.append(&mut batch);
                        publish(&found, false);
                    }
                });
                found.append(&mut batch);
                found.sort();
                publish(&found, true);
            });
        if let Err(e) = spawned {
            tracing::warn!("cannot walk {}: {e}", self.root.display());
        }
    }

    /// The project's folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The files found so far (relative paths), and whether the walk is
    /// done. A finished walk starts again when files came or went.
    pub fn files(&self) -> (Arc<Vec<PathBuf>>, bool) {
        let (files, done, stale) = {
            let s = self.state.lock().expect("the index");
            (s.files.clone(), s.done, s.stale)
        };
        if done && stale {
            self.start();
        }
        (files, done && !stale)
    }

    /// Walks again.
    pub fn refresh(&self) {
        self.start();
    }

    /// Waits until the walk is done (for tests and the command line).
    pub fn wait(&self) -> Arc<Vec<PathBuf>> {
        loop {
            let (files, done) = self.files();
            if done {
                return files;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

impl Drop for FileIndex {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_that_change_no_file_found() {
        let root =
            std::env::temp_dir().join(format!("kalem-project-events-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (f, text) in [
            ("a.org", "x"),
            ("sub/b.org", "y"),
            (".gitignore", "target/\n"),
            ("target/out.o", "z"),
            (".git/index", "i"),
            ("new.org", "n"),
            ("pic.png", "p"),
        ] {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        let files: Vec<PathBuf> = ["a.org", "sub/b.org"].iter().map(PathBuf::from).collect();
        let ignored = top_ignores(&root, &["*.log".into()]);
        let changes = |rel: &str| comes_or_goes(&root, ignored.as_ref(), &files, &root.join(rel));
        // A file saved again, the version control folder, ignored ones, a
        // picture, a save's copy already gone: nothing came or went.
        assert!(!changes("a.org"));
        assert!(!changes(".git/index"));
        assert!(!changes("target/out.o"));
        assert!(!changes("x.log"));
        assert!(!changes("pic.png"));
        assert!(!changes(".a.org.kalem-save"));
        // A new file, a file gone, a folder gone with files in it.
        assert!(changes("new.org"));
        std::fs::remove_file(root.join("sub/b.org")).unwrap();
        assert!(changes("sub/b.org"));
        std::fs::remove_dir_all(root.join("sub")).unwrap();
        assert!(changes("sub"));
        // Outside the project: walked again, to be sure.
        assert!(comes_or_goes(
            &root,
            None,
            &files,
            Path::new("/elsewhere/a.org")
        ));
        let _ = std::fs::remove_dir_all(&root);
    }
}
