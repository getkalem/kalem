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
    // `/C:/x` on Windows.
    let s = match s.as_bytes() {
        [b'/', d, b':', ..] if d.is_ascii_alphabetic() && cfg!(windows) => s[1..].to_string(),
        _ => s,
    };
    Some(PathBuf::from(s))
}

#[cfg(test)]
mod tests {
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
