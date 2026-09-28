//! What gpui's clipboard does not give: the HTML flavor of the system
//! clipboard, for pasting rich text as Org.
//!
//! macOS reads the general pasteboard's `public.html`. Linux and Windows
//! have no reader yet, so their pastes use the plain text.

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
