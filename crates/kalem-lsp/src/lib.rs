//! The language server client of Kalem (design §11.12, D57; T3.8.1).
//!
//! One client, in the core, knowing no language: a language plugin says
//! which program to start for which files and with what settings, and
//! this crate runs it. The protocol is spoken with `serde_json` values
//! rather than `lsp-types`: the client reads a handful of fields from
//! each answer, and servers in the wild bend the types (ElixirLS sends
//! numbers where strings are declared), so a typed parse would reject
//! answers the editor can use.
//!
//! - [`position`]: byte offsets against lines and UTF-16, UTF-8 or UTF-32
//!   characters;
//! - [`rpc`]: the framing;
//! - [`client`]: one server process, its requests, notifications and
//!   documents;
//! - [`features`]: answers read into values with byte ranges;
//! - [`find_program`]: where a server's program is.

pub mod client;
#[doc(hidden)]
pub mod fake;
pub mod features;
pub mod position;
pub mod rpc;
pub mod uri;

pub use client::{
    CONTENT_MODIFIED, Client, Edit, Event, Health, Pending, RpcError, SERVER_CANCELLED,
    ServerConfig, ServerStatus, StatusSpec, Wake,
};
pub use position::{Encoding, Position};

use std::path::{Path, PathBuf};

/// `~` at the start of a path is the home directory.
pub fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

fn executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// Finds a program: a name with a slash is a path (relative ones to the
/// project root, `~` expanded); a bare name is looked for in `local_dirs`
/// under the root (`node_modules/.bin`, `.venv/bin`), then on the `PATH`.
pub fn find_program(name: &str, root: Option<&Path>, local_dirs: &[String]) -> Option<PathBuf> {
    if name.contains('/') || name.contains('\\') {
        let p = expand_home(name);
        let p = match root {
            Some(r) if p.is_relative() => r.join(p),
            _ => p,
        };
        return executable(&p).then_some(p);
    }
    let exts: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(r) = root {
        dirs.extend(local_dirs.iter().map(|d| r.join(expand_home(d))));
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs.iter().find_map(|d| {
        exts.iter()
            .map(|e| d.join(format!("{name}{e}")))
            .find(|p| executable(p))
    })
}

/// The root of `file` by markers: the outermost directory up from it
/// holding one of `markers` when `outermost`, else the nearest; `None`
/// when no directory does.
pub fn find_root(file: &Path, markers: &[String], outermost: bool) -> Option<PathBuf> {
    let mut found = None;
    for dir in file.ancestors().skip(1) {
        if markers.iter().any(|m| dir.join(m).exists()) {
            found = Some(dir.to_path_buf());
            if !outermost {
                break;
            }
        }
    }
    found
}
