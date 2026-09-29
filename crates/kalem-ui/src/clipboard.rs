//! What gpui's clipboard does not give: the HTML flavor of the system
//! clipboard, for pasting rich text as Org and copying it.
//!
//! macOS reads and writes the general pasteboard's `public.html`. Linux
//! and Windows have no reader or writer yet, so their pastes and copies
//! use the plain text.

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

/// The HTML on the system clipboard, if any.
#[cfg(not(target_os = "macos"))]
pub fn html() -> Option<String> {
    None
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
