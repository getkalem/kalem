//! File operations: copy, move (across devices too), trash, delete, new
//! directory, touch, symbolic link and permissions.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// What an operation does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    /// Copy to a destination.
    Copy,
    /// Move or rename to a destination.
    Move,
    /// Move to the system trash.
    Trash,
    /// Delete for good.
    Delete,
}

/// What to do when a destination exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    /// Replace a file; merge into a directory.
    Overwrite,
    /// Leave the source where it is.
    Skip,
    /// Give the new one another name: `name (2).ext`.
    KeepBoth,
}

/// A file operation on some paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    /// What it does.
    pub kind: OpKind,
    /// Sources and their destinations (none for trash and delete).
    pub items: Vec<(PathBuf, Option<PathBuf>)>,
    /// The choice for destinations that exist, by destination.
    pub choices: HashMap<PathBuf, Conflict>,
    /// The choice for other destinations that exist.
    pub default_choice: Conflict,
}

impl Operation {
    /// `sources` copied (`kind` [`OpKind::Copy`]) or moved to `target`, as
    /// Dired does: into `target` when it is a directory (it must be for
    /// several sources), else as `target`.
    pub fn transfer(kind: OpKind, sources: &[PathBuf], target: &Path) -> io::Result<Operation> {
        let into = target.is_dir() || sources.len() > 1;
        if into && !target.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} is not a directory", target.display()),
            ));
        }
        let items = sources
            .iter()
            .map(|s| {
                let dst = if into {
                    target.join(s.file_name().unwrap_or_default())
                } else {
                    target.to_path_buf()
                };
                (s.clone(), Some(dst))
            })
            .collect();
        Ok(Operation {
            kind,
            items,
            choices: HashMap::new(),
            default_choice: Conflict::Skip,
        })
    }

    /// `paths` moved to the trash (`OpKind::Trash`) or deleted.
    pub fn remove(kind: OpKind, paths: &[PathBuf]) -> Operation {
        Operation {
            kind,
            items: paths.iter().map(|p| (p.clone(), None)).collect(),
            choices: HashMap::new(),
            default_choice: Conflict::Skip,
        }
    }
}

/// What an operation did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Sources done, with where they went.
    pub done: Vec<(PathBuf, Option<PathBuf>)>,
    /// Sources left alone (their destination existed).
    pub skipped: Vec<PathBuf>,
    /// Sources that failed, with the error.
    pub errors: Vec<(PathBuf, String)>,
    /// Stopped before the end.
    pub cancelled: bool,
}

/// The destinations of `op` that exist already.
pub fn conflicts(op: &Operation) -> Vec<PathBuf> {
    op.items
        .iter()
        .filter_map(|(src, dst)| {
            let d = dst.as_ref()?;
            (std::fs::symlink_metadata(d).is_ok() && !same_file(src, d)).then(|| d.clone())
        })
        .collect()
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// `path` with ` (2)`, ` (3)`… before its extension: the first that does
/// not exist.
pub fn unique_name(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_string(), name[i..].to_string()),
        _ => (name.clone(), String::new()),
    };
    let parent = path.parent().unwrap_or(Path::new(""));
    (2..)
        .map(|n| parent.join(format!("{stem} ({n}){ext}")))
        .find(|p| std::fs::symlink_metadata(p).is_err())
        .expect("a free name")
}

/// Reports what a copy is doing: the file, and its size when done.
pub(crate) trait Report {
    fn file(&self, path: &Path);
    fn bytes(&self, n: u64);
    fn cancelled(&self) -> bool;
}

/// No reporting.
struct Quiet;

impl Report for Quiet {
    fn file(&self, _: &Path) {}
    fn bytes(&self, _: u64) {}
    fn cancelled(&self) -> bool {
        false
    }
}

fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "cancelled")
}

/// Copies `src` to `dst`: directories recursively (merging into an
/// existing one), links as links; permissions and modification times
/// kept.
pub fn copy_path(src: &Path, dst: &Path) -> io::Result<()> {
    copy_with(src, dst, &Quiet)
}

pub(crate) fn copy_with(src: &Path, dst: &Path, r: &dyn Report) -> io::Result<()> {
    if r.cancelled() {
        return Err(cancelled_error());
    }
    let meta = std::fs::symlink_metadata(src)?;
    let ft = meta.file_type();
    if ft.is_dir() {
        if let Ok(c) = src.canonicalize()
            && let Some(parent) = dst.parent().and_then(|p| p.canonicalize().ok())
            && parent.starts_with(&c)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("cannot copy {} into itself", src.display()),
            ));
        }
        match std::fs::symlink_metadata(dst) {
            Ok(m) if !m.is_dir() => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} is not a directory", dst.display()),
                ));
            }
            Ok(_) => {}
            Err(_) => std::fs::create_dir(dst)?,
        }
        let mut children: Vec<PathBuf> = std::fs::read_dir(src)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        children.sort();
        for c in children {
            copy_with(&c, &dst.join(c.file_name().unwrap_or_default()), r)?;
        }
        std::fs::set_permissions(dst, meta.permissions())?;
        let _ =
            filetime::set_file_mtime(dst, filetime::FileTime::from_last_modification_time(&meta));
        return Ok(());
    }
    r.file(src);
    if let Ok(m) = std::fs::symlink_metadata(dst) {
        if same_file(src, dst) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("cannot copy {} onto itself", src.display()),
            ));
        }
        if m.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} is a directory", dst.display()),
            ));
        }
        std::fs::remove_file(dst)?;
    }
    if ft.is_symlink() {
        let target = std::fs::read_link(src)?;
        make_symlink(&target, dst)?;
    } else {
        std::fs::copy(src, dst)?;
        let _ =
            filetime::set_file_mtime(dst, filetime::FileTime::from_last_modification_time(&meta));
    }
    r.bytes(meta.len());
    Ok(())
}

/// Moves `src` to `dst`: a rename, or a copy and a delete across devices;
/// onto an existing directory, merges into it.
pub fn move_path(src: &Path, dst: &Path) -> io::Result<()> {
    move_with(src, dst, &Quiet)
}

pub(crate) fn move_with(src: &Path, dst: &Path, r: &dyn Report) -> io::Result<()> {
    if r.cancelled() {
        return Err(cancelled_error());
    }
    if same_file(src, dst) {
        // The same file: nothing to do, or a change of case on a file
        // system that ignores case.
        if src.file_name() == dst.file_name() {
            return Ok(());
        }
        return std::fs::rename(src, dst);
    }
    let meta = std::fs::symlink_metadata(src)?;
    match std::fs::symlink_metadata(dst) {
        Ok(d) if d.is_dir() && meta.is_dir() => {
            // Merge: move the children, then remove the empty source.
            let mut children: Vec<PathBuf> = std::fs::read_dir(src)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .collect();
            children.sort();
            for c in children {
                move_with(&c, &dst.join(c.file_name().unwrap_or_default()), r)?;
            }
            return std::fs::remove_dir(src);
        }
        Ok(d) if d.is_dir() != meta.is_dir() => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} exists and is of another kind", dst.display()),
            ));
        }
        Ok(_) => std::fs::remove_file(dst)?,
        Err(_) => {}
    }
    r.file(src);
    match std::fs::rename(src, dst) {
        Ok(()) => {
            r.bytes(meta.len());
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            copy_with(src, dst, r)?;
            delete_path(src)
        }
        Err(e) => Err(e),
    }
}

/// Deletes `path` for good: a directory with everything in it; a link,
/// not what it points to.
pub fn delete_path(path: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Moves `paths` to the system trash. On macOS this asks the file
/// manager service (`NSFileManager`), which needs no extra permission.
pub fn trash_paths(paths: &[PathBuf]) -> Result<(), String> {
    #[allow(unused_mut)]
    let mut ctx = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        ctx.set_delete_method(DeleteMethod::NsFileManager);
    }
    ctx.delete_all(paths).map_err(|e| e.to_string())
}

/// Makes directory `path` and its missing parents; fails if it exists.
pub fn mkdir(path: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(path).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists", path.display()),
        ));
    }
    std::fs::create_dir_all(path)
}

/// Makes an empty file at `path`, or sets its modification time to now.
pub fn touch(path: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(path).is_ok() {
        filetime::set_file_mtime(path, filetime::FileTime::now())
    } else {
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .map(|_| ())
    }
}

#[cfg(unix)]
fn make_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn make_symlink(target: &Path, link: &Path) -> io::Result<()> {
    let dir = link
        .parent()
        .map_or(target.to_path_buf(), |p| p.join(target))
        .is_dir();
    if dir {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

#[cfg(not(any(unix, windows)))]
fn make_symlink(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no symbolic links",
    ))
}

/// Makes `link` a symbolic link to `target`.
pub fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    make_symlink(target, link)
}

/// Sets the Unix permission bits of `path` (elsewhere, only whether it
/// can be written).
pub fn chmod(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let mut p = std::fs::metadata(path)?.permissions();
        p.set_readonly(mode & 0o200 == 0);
        std::fs::set_permissions(path, p)
    }
}

/// Runs `op`, reporting to `r`, stopping when `cancel` is set.
pub(crate) fn run(op: &Operation, r: &dyn Report, cancel: &AtomicBool) -> Outcome {
    let mut out = Outcome::default();
    for (src, dst) in &op.items {
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            break;
        }
        let mut dst = dst.clone();
        if let Some(d) = &dst
            && std::fs::symlink_metadata(d).is_ok()
            && !same_file(src, d)
        {
            match op.choices.get(d).copied().unwrap_or(op.default_choice) {
                Conflict::Skip => {
                    out.skipped.push(src.clone());
                    continue;
                }
                Conflict::KeepBoth => dst = Some(unique_name(d)),
                Conflict::Overwrite => {}
            }
        }
        let result = match (op.kind, &dst) {
            (OpKind::Copy, Some(d)) => copy_with(src, d, r).map_err(|e| e.to_string()),
            (OpKind::Move, Some(d)) => move_with(src, d, r).map_err(|e| e.to_string()),
            (OpKind::Trash, _) => {
                r.file(src);
                trash_paths(std::slice::from_ref(src))
            }
            (OpKind::Delete, _) => {
                r.file(src);
                delete_path(src).map_err(|e| e.to_string())
            }
            (_, None) => Err("no destination".into()),
        };
        match result {
            Ok(()) => out.done.push((src.clone(), dst)),
            Err(e) if cancel.load(Ordering::Relaxed) => {
                tracing::debug!(%e, "cancelled");
                out.cancelled = true;
                break;
            }
            Err(e) => out.errors.push((src.clone(), e)),
        }
    }
    out
}
