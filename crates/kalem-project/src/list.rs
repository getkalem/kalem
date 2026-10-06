//! The project list: the folders the user added, most recently used first,
//! with each project's recent files, kept in a TOML file.
//!
//! ```toml
//! # Recently opened files, in any project or none.
//! recent_files = ["/home/me/notes/todo.org"]
//!
//! [[project]]
//! name = "notes"
//! path = "/home/me/notes"
//! used = 1759000000
//! last_file = "/home/me/notes/todo.org"
//! recent_files = ["/home/me/notes/todo.org"]
//! ignore = ["*.log", "build/"]
//! ```

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

/// How many recent files are kept, in all and per project.
const RECENT: usize = 50;

/// A project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Its name: the folder's name unless renamed.
    pub name: String,
    /// Its folder.
    pub root: PathBuf,
    /// When it was last used, in seconds since 1970.
    pub used: u64,
    /// The file last opened in it.
    pub last_file: Option<PathBuf>,
    /// Files recently opened in it, the latest first.
    pub recent: Vec<PathBuf>,
    /// Extra ignore patterns (gitignore syntax) for its files.
    pub ignore: Vec<String>,
}

impl Project {
    /// A project for `root`, named after the folder.
    pub fn new(root: PathBuf) -> Project {
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        Project {
            name,
            root,
            used: 0,
            last_file: None,
            recent: Vec::new(),
            ignore: Vec::new(),
        }
    }

    /// Whether its folder is still there.
    pub fn exists(&self) -> bool {
        self.root.is_dir()
    }

    /// Whether `path` (normalized, see [`normal`]) is inside it.
    pub fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
    }

    /// `path` relative to the project's folder, with `/` separators.
    pub fn relative(&self, path: &Path) -> String {
        let rel = path.strip_prefix(&self.root).unwrap_or(path);
        let parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        parts.join("/")
    }
}

/// `path` as the list stores it: absolute, with links resolved when it
/// exists.
pub fn normal(path: &Path) -> PathBuf {
    dunce::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// What marks a folder as a project root for [`detect_root`]: version
/// control, Projectile's marker, Kalem's workspace settings.
pub const ROOT_MARKERS: &[&str] = &[".git", ".hg", ".svn", ".projectile", ".kalem"];

/// The innermost folder holding `path` (or `path` itself, if a folder)
/// that has one of [`ROOT_MARKERS`], as Projectile finds a project; never
/// the home folder or the file system's root.
pub fn detect_root(path: &Path) -> Option<PathBuf> {
    let path = normal(path);
    let home = std::env::var_os("HOME").map(|h| normal(Path::new(&h)));
    let start = if path.is_dir() {
        path.as_path()
    } else {
        path.parent()?
    };
    for dir in start.ancestors() {
        if dir.parent().is_none() || home.as_deref() == Some(dir) {
            return None;
        }
        if ROOT_MARKERS.iter().any(|m| dir.join(m).exists()) {
            return Some(dir.to_path_buf());
        }
    }
    None
}

/// Now, in seconds since 1970.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The project list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Projects {
    /// The projects, in the order they were added.
    pub list: Vec<Project>,
    /// Files recently opened anywhere, the latest first.
    pub recent: Vec<PathBuf>,
    /// The file the list is kept in.
    pub file: Option<PathBuf>,
    /// The folders removed from the list since it was loaded: a save
    /// leaves them out of what another Kalem wrote meanwhile.
    pub removed: Vec<PathBuf>,
}

fn strings(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn array(items: impl IntoIterator<Item = String>) -> Item {
    let mut a = Array::new();
    for s in items {
        a.push(s);
    }
    value(a)
}

fn push_recent(list: &mut Vec<PathBuf>, path: &Path) {
    list.retain(|p| p != path);
    list.insert(0, path.to_path_buf());
    list.truncate(RECENT);
}

impl Projects {
    /// The list kept in `file`; empty if the file does not exist or cannot
    /// be read (the problem is logged).
    pub fn load(file: Option<PathBuf>) -> Projects {
        let text = match file.as_deref().map(std::fs::read_to_string) {
            Some(Ok(t)) => t,
            Some(Err(e)) if e.kind() != io::ErrorKind::NotFound => {
                tracing::warn!("cannot read the project list: {e}");
                String::new()
            }
            _ => String::new(),
        };
        let mut p = Projects::parse(&text);
        p.file = file;
        p
    }

    /// The list in `text`; entries without a path are skipped.
    pub fn parse(text: &str) -> Projects {
        let doc: DocumentMut = match text.parse() {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("the project list is not valid TOML: {e}");
                return Projects::default();
            }
        };
        let mut out = Projects {
            recent: strings(doc.get("recent_files"))
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            ..Projects::default()
        };
        if let Some(tables) = doc.get("project").and_then(Item::as_array_of_tables) {
            for t in tables {
                let Some(path) = t.get("path").and_then(Item::as_str) else {
                    continue;
                };
                let mut p = Project::new(PathBuf::from(path));
                if let Some(n) = t.get("name").and_then(Item::as_str) {
                    p.name = n.to_string();
                }
                p.used = t
                    .get("used")
                    .and_then(Item::as_integer)
                    .map_or(0, |u| u.max(0) as u64);
                p.last_file = t.get("last_file").and_then(Item::as_str).map(PathBuf::from);
                p.recent = strings(t.get("recent_files"))
                    .into_iter()
                    .map(PathBuf::from)
                    .collect();
                p.ignore = strings(t.get("ignore"));
                if !out.list.iter().any(|q| q.root == p.root) {
                    out.list.push(p);
                }
            }
        }
        out
    }

    /// The list as TOML.
    pub fn to_toml(&self) -> String {
        let mut doc = DocumentMut::new();
        doc.insert(
            "recent_files",
            array(self.recent.iter().map(|p| p.display().to_string())),
        );
        let mut tables = ArrayOfTables::new();
        for p in &self.list {
            let mut t = Table::new();
            t.insert("name", value(p.name.clone()));
            t.insert("path", value(p.root.display().to_string()));
            t.insert("used", value(i64::try_from(p.used).unwrap_or(i64::MAX)));
            if let Some(f) = &p.last_file {
                t.insert("last_file", value(f.display().to_string()));
            }
            t.insert(
                "recent_files",
                array(p.recent.iter().map(|f| f.display().to_string())),
            );
            if !p.ignore.is_empty() {
                t.insert("ignore", array(p.ignore.iter().cloned()));
            }
            tables.push(t);
        }
        doc.insert("project", Item::ArrayOfTables(tables));
        format!("# Kalem's projects: folders added with Add Project, and recent files.\n{doc}")
    }

    /// Writes the list to its file (through a temporary file, so that it
    /// is never half written), with what another Kalem wrote there since
    /// this one read it: its projects stay unless this one removed them,
    /// and its recent files follow this one's. A file that cannot be read
    /// is left alone (the error); one that is not a project list is kept
    /// beside it as `projects.toml.broken` before it is written over.
    pub fn save(&self) -> io::Result<()> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut out = self.clone();
        match std::fs::read_to_string(file) {
            Ok(text) => match text.parse::<DocumentMut>() {
                Ok(_) => out.merge(&Projects::parse(&text)),
                Err(e) => {
                    tracing::warn!("the project list is not valid TOML, kept aside: {e}");
                    let aside = file.with_extension("toml.broken");
                    if !aside.exists() {
                        std::fs::write(&aside, &text)?;
                    }
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let tmp = file.with_extension("toml.tmp");
        std::fs::write(&tmp, out.to_toml())?;
        std::fs::rename(&tmp, file)
    }

    /// `disk`'s projects this list lacks and did not remove, and its recent
    /// files after this list's.
    fn merge(&mut self, disk: &Projects) {
        for p in &disk.list {
            if !self.list.iter().any(|q| q.root == p.root) && !self.removed.contains(&p.root) {
                self.list.push(p.clone());
            }
        }
        for f in &disk.recent {
            if !self.recent.contains(f) {
                self.recent.push(f.clone());
            }
        }
        self.recent.truncate(RECENT);
    }

    /// Adds the folder `root`; returns its index. Errors if it is not a
    /// folder or already listed (then the error names the project).
    pub fn add(&mut self, root: &Path) -> Result<usize, String> {
        let root = normal(root);
        if !root.is_dir() {
            return Err(format!("{} is not a folder", root.display()));
        }
        if let Some(p) = self.list.iter().find(|p| p.root == root) {
            return Err(format!(
                "{} is already the project {}",
                root.display(),
                p.name
            ));
        }
        self.removed.retain(|r| *r != root);
        let mut p = Project::new(root);
        p.used = now();
        self.list.push(p);
        Ok(self.list.len() - 1)
    }

    /// Removes the project of folder `root` from the list (its files are
    /// untouched); `false` if it was not listed.
    pub fn remove(&mut self, root: &Path) -> bool {
        let n = self.list.len();
        self.list.retain(|p| p.root != root);
        if self.list.len() == n {
            return false;
        }
        self.removed.push(root.to_path_buf());
        true
    }

    /// Renames the project of folder `root`.
    pub fn rename(&mut self, root: &Path, name: &str) -> bool {
        match self.list.iter_mut().find(|p| p.root == root) {
            Some(p) if !name.trim().is_empty() => {
                p.name = name.trim().to_string();
                true
            }
            _ => false,
        }
    }

    /// The project of folder `root`.
    pub fn get(&self, root: &Path) -> Option<&Project> {
        self.list.iter().find(|p| p.root == root)
    }

    /// The project holding `path`: the innermost one for nested projects.
    pub fn containing(&self, path: &Path) -> Option<&Project> {
        let path = normal(path);
        self.list
            .iter()
            .filter(|p| p.contains(&path))
            .max_by_key(|p| p.root.components().count())
    }

    /// The projects, most recently used first.
    pub fn by_use(&self) -> Vec<&Project> {
        let mut v: Vec<&Project> = self.list.iter().collect();
        v.sort_by(|a, b| b.used.cmp(&a.used).then_with(|| a.name.cmp(&b.name)));
        v
    }

    /// Notes that `path` was opened: the recent files, and the last file
    /// and use of its project.
    pub fn opened(&mut self, path: &Path) {
        let path = normal(path);
        push_recent(&mut self.recent, &path);
        let root = self.containing(&path).map(|p| p.root.clone());
        if let Some(p) = root.and_then(|r| self.list.iter_mut().find(|p| p.root == r)) {
            push_recent(&mut p.recent, &path);
            p.last_file = Some(path);
            p.used = now().max(p.used);
        }
    }

    /// Notes that the project of folder `root` was switched to.
    pub fn used(&mut self, root: &Path) {
        if let Some(p) = self.list.iter_mut().find(|p| p.root == root) {
            p.used = now().max(p.used + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kalem-project-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        normal(&d)
    }

    #[test]
    fn list_round_trip() {
        let dir = temp("list");
        let inner = dir.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        let mut p = Projects {
            file: Some(dir.join("projects.toml")),
            ..Projects::default()
        };
        assert_eq!(p.add(&dir), Ok(0));
        assert!(p.add(&dir).is_err());
        assert!(p.add(&dir.join("missing")).is_err());
        p.add(&inner).unwrap();
        // The innermost project holds a file.
        let file = inner.join("a.org");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(p.containing(&file).unwrap().root, inner);
        assert_eq!(p.containing(&dir.join("b.org")).unwrap().root, dir);
        assert!(p.containing(Path::new("/")).is_none());
        p.opened(&file);
        assert_eq!(
            p.get(&inner).unwrap().last_file.as_deref(),
            Some(file.as_path())
        );
        assert_eq!(p.recent, vec![file.clone()]);
        assert_eq!(p.by_use()[0].root, inner);
        assert!(p.rename(&dir, "Everything"));
        p.list[0].ignore = vec!["*.log".into()];
        p.save().unwrap();
        let q = Projects::load(Some(dir.join("projects.toml")));
        assert_eq!(p, q);
        assert!(p.remove(&inner));
        assert!(!p.remove(&inner));
        assert_eq!(p.list.len(), 1);
        assert_eq!(
            p.get(&dir).unwrap().relative(&dir.join("x/y.org")),
            "x/y.org"
        );
    }

    /// Two Kalems at once (the window and the terminal, or two of either):
    /// what each adds and removes stays, whichever saves last.
    #[test]
    fn two_lists_saved_in_turn_keep_both() {
        let dir = temp("two");
        let (a_dir, b_dir, c_dir) = (dir.join("a"), dir.join("b"), dir.join("c"));
        for d in [&a_dir, &b_dir, &c_dir] {
            std::fs::create_dir_all(d).unwrap();
        }
        let file = dir.join("projects.toml");
        let mut first = Projects::load(Some(file.clone()));
        first.add(&c_dir).unwrap();
        first.save().unwrap();
        // Both read the list with c in it.
        let mut one = Projects::load(Some(file.clone()));
        let mut two = Projects::load(Some(file.clone()));
        one.add(&a_dir).unwrap();
        one.save().unwrap();
        two.add(&b_dir).unwrap();
        assert!(two.remove(&c_dir));
        two.save().unwrap();
        let roots: Vec<PathBuf> = Projects::load(Some(file.clone()))
            .list
            .into_iter()
            .map(|p| p.root)
            .collect();
        assert!(
            roots.contains(&a_dir) && roots.contains(&b_dir),
            "{roots:?}"
        );
        assert!(!roots.contains(&c_dir), "{roots:?}");
    }

    /// A file that is not a project list is kept aside, not lost.
    #[test]
    fn a_broken_list_is_kept_aside() {
        let dir = temp("broken");
        let file = dir.join("projects.toml");
        std::fs::write(&file, "[[project]\npath = \"/x").unwrap();
        let mut p = Projects::load(Some(file.clone()));
        assert!(p.list.is_empty());
        p.add(&dir).unwrap();
        p.save().unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("projects.toml.broken")).unwrap(),
            "[[project]\npath = \"/x"
        );
        assert_eq!(Projects::load(Some(file)).list.len(), 1);
    }

    #[test]
    fn bad_files() {
        assert_eq!(Projects::parse("not toml ["), Projects::default());
        let p = Projects::parse("[[project]]\nname = \"x\"\n[[project]]\npath = \"/a\"\n");
        assert_eq!(p.list.len(), 1);
        assert_eq!(p.list[0].name, "a");
    }
}
