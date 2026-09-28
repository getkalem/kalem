//! Directory listings: entries with type, permissions, size and time,
//! sorted like `ls`.

use std::cmp::Ordering;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What an entry is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A regular file.
    File,
    /// A directory.
    Dir,
    /// A symbolic link.
    Symlink {
        /// Where it points, as written.
        target: PathBuf,
        /// It points to a directory.
        to_dir: bool,
        /// What it points to does not exist.
        broken: bool,
    },
    /// A device, socket or pipe.
    Other,
}

/// An entry of a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The file name (lossy for names that are not UTF-8).
    pub name: String,
    /// The full path.
    pub path: PathBuf,
    /// What it is.
    pub kind: Kind,
    /// The size in bytes (of the link itself for symbolic links).
    pub size: u64,
    /// The modification time.
    pub modified: Option<SystemTime>,
    /// The Unix permission bits (0 elsewhere).
    pub mode: u32,
    /// Whether it can be run (Unix).
    pub executable: bool,
}

impl Entry {
    /// Reads the entry at `path` without following a symbolic link.
    pub fn read(path: &Path) -> io::Result<Entry> {
        let meta = std::fs::symlink_metadata(path)?;
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let ft = meta.file_type();
        let kind = if ft.is_symlink() {
            let target = std::fs::read_link(path).unwrap_or_default();
            let followed = std::fs::metadata(path);
            Kind::Symlink {
                target,
                to_dir: followed.as_ref().is_ok_and(std::fs::Metadata::is_dir),
                broken: followed.is_err(),
            }
        } else if ft.is_dir() {
            Kind::Dir
        } else if ft.is_file() {
            Kind::File
        } else {
            Kind::Other
        };
        let mode = mode_of(&meta);
        Ok(Entry {
            name,
            path: path.to_path_buf(),
            kind,
            size: meta.len(),
            modified: meta.modified().ok(),
            mode,
            executable: matches!(meta.file_type(), t if t.is_file()) && mode & 0o111 != 0,
        })
    }

    /// A directory, or a link to one: opening it lists it.
    pub fn is_dir(&self) -> bool {
        matches!(self.kind, Kind::Dir | Kind::Symlink { to_dir: true, .. })
    }

    /// A dot file.
    pub fn is_hidden(&self) -> bool {
        self.name.starts_with('.') && self.name != ".."
    }

    /// The extension, without the dot, lower case (none for dot files
    /// without another dot).
    pub fn extension(&self) -> String {
        let n = self.name.trim_start_matches('.');
        n.rsplit_once('.')
            .map(|(_, e)| e.to_lowercase())
            .unwrap_or_default()
    }
}

#[cfg(unix)]
fn mode_of(meta: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode()
}

#[cfg(not(unix))]
fn mode_of(meta: &std::fs::Metadata) -> u32 {
    if meta.permissions().readonly() {
        0o444
    } else {
        0o644
    }
}

/// What entries are sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    /// The name, ignoring case, numbers by value.
    #[default]
    Name,
    /// The modification time, newest first.
    Time,
    /// The size, largest first.
    Size,
    /// The extension, then the name.
    Extension,
}

impl SortKey {
    /// The next key when cycling: name, time, size, extension.
    pub fn next(self) -> SortKey {
        match self {
            SortKey::Name => SortKey::Time,
            SortKey::Time => SortKey::Size,
            SortKey::Size => SortKey::Extension,
            SortKey::Extension => SortKey::Name,
        }
    }

    /// The key's name in settings: `name`, `time`, `size`, `extension`.
    pub fn name(self) -> &'static str {
        match self {
            SortKey::Name => "name",
            SortKey::Time => "time",
            SortKey::Size => "size",
            SortKey::Extension => "extension",
        }
    }

    /// The key named `name`.
    pub fn from_name(name: &str) -> Option<SortKey> {
        Some(match name {
            "name" => SortKey::Name,
            "time" => SortKey::Time,
            "size" => SortKey::Size,
            "extension" => SortKey::Extension,
            _ => return None,
        })
    }
}

/// How a directory is listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListOptions {
    /// The sort key.
    pub sort: SortKey,
    /// The order reversed.
    pub reverse: bool,
    /// Directories before files.
    pub dirs_first: bool,
    /// Dot files shown.
    pub hidden: bool,
}

impl Default for ListOptions {
    fn default() -> Self {
        ListOptions {
            sort: SortKey::Name,
            reverse: false,
            dirs_first: true,
            hidden: false,
        }
    }
}

/// The entries of `dir` (without `.` and `..`), sorted.
pub fn read_dir(dir: &Path, opts: &ListOptions) -> io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir)? {
        let Ok(e) = e else { continue };
        match Entry::read(&e.path()) {
            Ok(entry) if opts.hidden || !entry.is_hidden() => out.push(entry),
            Ok(_) => {}
            Err(err) => tracing::debug!(path = %e.path().display(), %err, "unreadable entry"),
        }
    }
    sort(&mut out, opts);
    Ok(out)
}

/// Compares names like people read them: case ignored, runs of digits by
/// value (`file2` before `file10`), then as written.
pub fn natural(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let mut n = String::new();
                while let Some(c) = x.peek().copied().filter(char::is_ascii_digit) {
                    n.push(c);
                    x.next();
                }
                let mut m = String::new();
                while let Some(d) = y.peek().copied().filter(char::is_ascii_digit) {
                    m.push(d);
                    y.next();
                }
                let (n1, m1) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                let o = n1.len().cmp(&m1.len()).then_with(|| n1.cmp(m1));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let o = c.to_lowercase().cmp(d.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

/// Sorts `entries` by `opts`.
pub fn sort(entries: &mut [Entry], opts: &ListOptions) {
    entries.sort_by(|a, b| {
        let dirs = if opts.dirs_first {
            b.is_dir().cmp(&a.is_dir())
        } else {
            Ordering::Equal
        };
        let key = match opts.sort {
            SortKey::Name => natural(&a.name, &b.name),
            SortKey::Time => b
                .modified
                .cmp(&a.modified)
                .then_with(|| natural(&a.name, &b.name)),
            SortKey::Size => b.size.cmp(&a.size).then_with(|| natural(&a.name, &b.name)),
            SortKey::Extension => a
                .extension()
                .cmp(&b.extension())
                .then_with(|| natural(&a.name, &b.name)),
        };
        dirs.then(if opts.reverse { key.reverse() } else { key })
    });
}

/// The permissions as `ls -l` shows them: `drwxr-xr-x`.
pub fn permissions(e: &Entry) -> String {
    let t = match e.kind {
        Kind::Dir => 'd',
        Kind::Symlink { .. } => 'l',
        Kind::File => '-',
        Kind::Other => '?',
    };
    let m = e.mode;
    let bit = |mask: u32, c: char| if m & mask != 0 { c } else { '-' };
    let mut s = String::with_capacity(10);
    s.push(t);
    s.push(bit(0o400, 'r'));
    s.push(bit(0o200, 'w'));
    s.push(match (m & 0o100 != 0, m & 0o4000 != 0) {
        (true, true) => 's',
        (false, true) => 'S',
        (true, false) => 'x',
        _ => '-',
    });
    s.push(bit(0o040, 'r'));
    s.push(bit(0o020, 'w'));
    s.push(match (m & 0o010 != 0, m & 0o2000 != 0) {
        (true, true) => 's',
        (false, true) => 'S',
        (true, false) => 'x',
        _ => '-',
    });
    s.push(bit(0o004, 'r'));
    s.push(bit(0o002, 'w'));
    s.push(match (m & 0o001 != 0, m & 0o1000 != 0) {
        (true, true) => 't',
        (false, true) => 'T',
        (true, false) => 'x',
        _ => '-',
    });
    s
}

/// A size as `ls -h` shows it: `512`, `4.0K`, `12K`, `1.5M`.
pub fn format_size(n: u64) -> String {
    const UNITS: [&str; 6] = ["K", "M", "G", "T", "P", "E"];
    if n < 1024 {
        return n.to_string();
    }
    let mut v = n as f64 / 1024.0;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    if v < 10.0 {
        format!("{:.1}{}", (v * 10.0).ceil() / 10.0, UNITS[u])
    } else {
        format!("{}{}", v.ceil() as u64, UNITS[u])
    }
}

/// A time in the local time zone: `2026-09-28 10:00`.
pub fn format_time(t: SystemTime) -> String {
    let Ok(ts) = jiff::Timestamp::try_from(t) else {
        return "????-??-?? ??:??".into();
    };
    let z = ts.to_zoned(jiff::tz::TimeZone::system());
    z.strftime("%Y-%m-%d %H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["file10", "File2", "file1", "a", "B"];
        v.sort_by(|a, b| natural(a, b));
        assert_eq!(v, vec!["a", "B", "file1", "File2", "file10"]);
    }

    #[test]
    fn sizes() {
        assert_eq!(format_size(512), "512");
        assert_eq!(format_size(4096), "4.0K");
        assert_eq!(format_size(12 * 1024 + 1), "13K");
        assert_eq!(format_size(3 * 1024 * 1024 / 2), "1.5M");
    }

    #[test]
    fn permission_string() {
        let e = Entry {
            name: "x".into(),
            path: "x".into(),
            kind: Kind::Dir,
            size: 0,
            modified: None,
            mode: 0o755,
            executable: false,
        };
        assert_eq!(permissions(&e), "drwxr-xr-x");
        let e = Entry {
            kind: Kind::File,
            mode: 0o4644,
            ..e
        };
        assert_eq!(permissions(&e), "-rwSr--r--");
    }
}
