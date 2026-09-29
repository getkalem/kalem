//! What gpui's clipboard does not give: the HTML flavor of the system
//! clipboard, for pasting rich text as Org and copying it.
//!
//! macOS reads and writes the general pasteboard's `public.html`. Linux
//! reads `text/html` through `wl-paste` (Wayland) or `xclip` (X11) when
//! they are installed. Windows has no reader yet, and only macOS writes
//! HTML, so the other copies and pastes use the plain text.

/// The HTML on the system clipboard, if any.
#[cfg(target_os = "macos")]
pub fn html() -> Option<String> {
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::NSString;
    // `NSPasteboardTypeHTML`, without reading AppKit's constant (unsafe).
    let kind = NSString::from_str("public.html");
    NSPasteboard::generalPasteboard()
        .stringForType(&kind)
        .map(|s| s.to_string())
}

/// The HTML on the system clipboard, if any: asked of `wl-paste` or
/// `xclip`, which must answer within half a second.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn html() -> Option<String> {
    let wanted = |types: Vec<u8>| {
        String::from_utf8_lossy(&types)
            .lines()
            .any(|t| t.trim() == "text/html")
    };
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let types = run("wl-paste", &["--list-types"])?;
        return wanted(types)
            .then(|| run("wl-paste", &["--no-newline", "--type", "text/html"]))
            .flatten()
            .map(|b| decode(&b));
    }
    std::env::var_os("DISPLAY")?;
    let targets = run("xclip", &["-selection", "clipboard", "-t", "TARGETS", "-o"])?;
    wanted(targets)
        .then(|| {
            run(
                "xclip",
                &["-selection", "clipboard", "-t", "text/html", "-o"],
            )
        })
        .flatten()
        .map(|b| decode(&b))
}

/// The HTML on the system clipboard, if any.
#[cfg(not(unix))]
pub fn html() -> Option<String> {
    None
}

/// The output of `cmd` with `args`, if it runs and succeeds within half a
/// second.
#[cfg(all(unix, not(target_os = "macos")))]
fn run(cmd: &str, args: &[&str]) -> Option<Vec<u8>> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    // Read while it runs, so that a large clipboard cannot fill the pipe.
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = reader.join().ok()?;
                return status.success().then_some(out);
            }
            Ok(None) if start.elapsed() < std::time::Duration::from_millis(500) => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Clipboard HTML as text: UTF-8, or UTF-16 with a byte order mark (as
/// Firefox gives it on X11).
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn decode(bytes: &[u8]) -> String {
    let utf16 = |b: &[u8], le: bool| {
        let units: Vec<u16> = b
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                if le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Puts `html` and its plain `text` on the system clipboard; `false` when
/// the platform has no HTML flavor here (the caller copies the text).
#[cfg(target_os = "macos")]
pub fn write_rich(html: &str, text: &str) -> bool {
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::NSString;
    let board = NSPasteboard::generalPasteboard();
    board.clearContents();
    let ok = board.setString_forType(
        &NSString::from_str(html),
        &NSString::from_str("public.html"),
    );
    // `NSPasteboardTypeString`.
    let plain = board.setString_forType(
        &NSString::from_str(text),
        &NSString::from_str("public.utf8-plain-text"),
    );
    ok && plain
}

/// Puts `html` and its plain `text` on the system clipboard; `false` when
/// the platform has no HTML flavor here (the caller copies the text).
#[cfg(not(target_os = "macos"))]
pub fn write_rich(_html: &str, _text: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn decoding() {
        assert_eq!(super::decode(b"<b>x</b>"), "<b>x</b>");
        let mut le = vec![0xFF, 0xFE];
        for u in "<i>é</i>".encode_utf16() {
            le.extend(u.to_le_bytes());
        }
        assert_eq!(super::decode(&le), "<i>é</i>");
    }
}
