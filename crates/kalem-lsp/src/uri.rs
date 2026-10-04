//! `file:` URIs for paths and back.

use std::path::{Path, PathBuf};

/// Bytes kept as they are in a URI path (RFC 3986 unreserved and `/`,
/// with `:` for Windows drive letters).
fn keep(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/' | b':')
}

/// The `file:` URI of an absolute path.
pub fn from_path(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        // `C:/x` is `file:///C:/x`.
        out.push('/');
    }
    for b in s.bytes() {
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
    let s = String::from_utf8(out).ok()?;
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
        s.replace_range(0..1, &d.to_ascii_uppercase().to_string());
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
}
