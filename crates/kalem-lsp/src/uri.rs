//! `file:` URIs for paths and back.

use std::path::{Path, PathBuf};

/// Bytes kept as they are in a URI path (RFC 3986 unreserved and `/`,
/// with `:` for Windows drive letters).
fn keep(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/' | b':')
}

/// The `file:` URI of a path, a relative one taken from the working
/// directory. On Unix the name's bytes as they are: `\\` is a character
/// of a name there, and a name need not be UTF-8; on Windows `\\` is the
/// separator.
pub fn from_path(path: &Path) -> String {
    // A drive's path (`C:/x`, as a server on Windows may write it) is
    // not taken for a relative one elsewhere.
    let drive =
        matches!(path.as_os_str().as_encoded_bytes(), [d, b':', ..] if d.is_ascii_alphabetic());
    let absolute;
    let path = if path.has_root() || drive {
        path
    } else {
        absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        &absolute
    };
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    };
    #[cfg(not(unix))]
    let bytes = path.to_string_lossy().replace('\\', "/").into_bytes();
    let mut out = String::from("file://");
    if bytes.first() != Some(&b'/') {
        // `C:/x` is `file:///C:/x`.
        out.push('/');
    }
    for b in bytes {
        if keep(b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The path of a `file:` URI; `None` for other schemes.
pub fn to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // Skip an authority (`localhost`) up to the path.
    let rest = &rest[rest.find('/')?..];
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let s = match String::from_utf8(out) {
        Ok(s) => s,
        // A Unix name that is not UTF-8, as `from_path` writes it.
        #[cfg(unix)]
        Err(e) => {
            use std::os::unix::ffi::OsStrExt;
            return Some(PathBuf::from(std::ffi::OsStr::from_bytes(e.as_bytes())));
        }
        #[cfg(not(unix))]
        Err(_) => return None,
    };
    // A Windows verbatim path written as a URI (`file:////?/C:/x`, from a
    // server that canonicalized without simplifying): its plain form.
    let s = s.strip_prefix("//?/").map_or(s.clone(), String::from);
    // `/C:/x` on Windows.
    let mut s = match s.as_bytes() {
        [b'/', d, b':', ..] if d.is_ascii_alphabetic() && cfg!(windows) => s[1..].to_string(),
        _ => s,
    };
    // `c:` and `C:` are the same drive: one spelling.
    if let [d, b':', ..] = s.as_bytes()
        && d.is_ascii_lowercase()
    {
        let upper = char::from(d.to_ascii_uppercase());
        s.replace_range(0..1, upper.encode_utf8(&mut [0; 4]));
    }
    Some(PathBuf::from(s))
}

/// One spelling of a `file:` URI, whatever the server wrote: the drive
/// letter's case, `%3A` for `:`, a verbatim `//?/` prefix. Documents and
/// the diagnostics servers publish for them are matched by it. Other
/// schemes are left as they are.
pub fn normalize(uri: &str) -> String {
    to_path(uri).map_or_else(|| uri.to_string(), |p| from_path(&p))
}

#[cfg(test)]
mod tests {
    #[test]
    fn one_spelling() {
        // Verbatim, as `std::fs::canonicalize` gives on Windows.
        assert_eq!(normalize("file:////?/C:/a/b.ex"), "file:///C:/a/b.ex");
        assert_eq!(normalize("file:///tmp/a%20b.ex"), "file:///tmp/a%20b.ex");
        assert_eq!(normalize("untitled:x"), "untitled:x");
        if cfg!(windows) {
            // VS Code's spelling: lower-case drive, `:` encoded.
            assert_eq!(normalize("file:///c%3A/a/b.ex"), "file:///C:/a/b.ex");
        }
    }

    use super::*;

    #[test]
    fn round_trip() {
        let p = Path::new("/tmp/a b/ç#1.ex");
        let u = from_path(p);
        assert_eq!(u, "file:///tmp/a%20b/%C3%A7%231.ex");
        assert_eq!(to_path(&u).unwrap(), p);
        assert_eq!(to_path("file://localhost/x/y").unwrap(), Path::new("/x/y"));
        assert!(to_path("untitled:1").is_none());
    }

    #[test]
    fn relative_paths_from_the_working_directory() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(from_path(Path::new("a.ex")), from_path(&cwd.join("a.ex")));
    }

    #[cfg(unix)]
    #[test]
    fn unix_names_as_they_are() {
        use std::os::unix::ffi::OsStrExt;
        // A backslash is a character of the name, not a separator.
        let p = Path::new("/tmp/a\\b.ex");
        assert_eq!(from_path(p), "file:///tmp/a%5Cb.ex");
        assert_eq!(to_path(&from_path(p)).unwrap(), p);
        // A name that is not UTF-8 keeps its bytes.
        let p = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/caf\xe9.ex"));
        assert_eq!(from_path(p), "file:///tmp/caf%E9.ex");
        assert_eq!(to_path(&from_path(p)).unwrap(), p);
    }
}
