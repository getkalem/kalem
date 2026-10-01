//! The application's icon: Kalem's logo (`assets/kalem.svg`) in the Dock
//! when Kalem runs as a bare binary (`cargo run`), which macOS would
//! show as "exec". An app bundle has its own icon; setting it again is
//! harmless.

/// Shows the logo as the application's icon, on the main thread after the
/// application started.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub fn set_app_icon() {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Ok(png) = kalem_core::images::svg_png(kalem_core::images::LOGO_SVG, 512) else {
        return;
    };
    let data = NSData::with_bytes(&png);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: on the main thread (`mtm`), with a valid image the
    // application retains; AppKit marks the setter unsafe only because
    // any `NSImage` subclass could be passed.
    unsafe { app.setApplicationIconImage(Some(&image)) };
}

/// Elsewhere the window manager takes the icon from the desktop entry.
#[cfg(not(target_os = "macos"))]
pub fn set_app_icon() {}
