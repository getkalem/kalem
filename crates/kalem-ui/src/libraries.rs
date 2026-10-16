//! The window system's libraries the graphical editor needs on Linux:
//! libxcb and libxkbcommon-x11 under X11, libwayland-client under Wayland,
//! libxkbcommon under both. None of them is linked: gpui's Wayland layer
//! and x11rb (`dl-libxcb`) open theirs at run time, and so does Kalem's
//! copy of xkbcommon (vendor/xkbcommon/README.md), so that the binary
//! starts on a system without them and the terminal editor and the
//! command-line tools run there. The graphical editor checks for them
//! before it opens a window, where a missing one would end it with a
//! panic.

/// What the graphical editor lacks on this system, as a sentence naming
/// the libraries and the packages that have them; `None` when nothing is
/// missing, and always on macOS and Windows, where the window system is
/// part of the system.
#[cfg(target_os = "linux")]
pub fn missing_libraries() -> Option<String> {
    let needed: &[Library] = match gpui::guess_compositor() {
        "Wayland" => &[WAYLAND, XKBCOMMON],
        "X11" => &[XCB, XKBCOMMON, XKBCOMMON_X11],
        _ => &[],
    };
    let missing: Vec<&Library> = needed.iter().filter(|l| !opens(l.soname)).collect();
    if missing.is_empty() {
        return None;
    }
    let sonames: Vec<_> = missing.iter().map(|l| l.soname.to_string_lossy()).collect();
    let debian: Vec<_> = missing.iter().map(|l| l.debian).collect();
    let fedora: Vec<_> = missing.iter().map(|l| l.fedora).collect();
    Some(format!(
        "the graphical editor needs {}, which this system lacks (Debian, Ubuntu: sudo apt install {}; Fedora: sudo dnf install {})",
        sonames.join(", "),
        debian.join(" "),
        fedora.join(" "),
    ))
}

#[cfg(not(target_os = "linux"))]
pub fn missing_libraries() -> Option<String> {
    None
}

/// A library by the name the loader looks for, with the packages of
/// Debian and Ubuntu and of Fedora that install it.
#[cfg(target_os = "linux")]
struct Library {
    soname: &'static std::ffi::CStr,
    debian: &'static str,
    fedora: &'static str,
}

#[cfg(target_os = "linux")]
const XCB: Library = Library {
    soname: c"libxcb.so.1",
    debian: "libxcb1",
    fedora: "libxcb",
};

#[cfg(target_os = "linux")]
const XKBCOMMON: Library = Library {
    soname: c"libxkbcommon.so.0",
    debian: "libxkbcommon0",
    fedora: "libxkbcommon",
};

#[cfg(target_os = "linux")]
const XKBCOMMON_X11: Library = Library {
    soname: c"libxkbcommon-x11.so.0",
    debian: "libxkbcommon-x11-0",
    fedora: "libxkbcommon-x11",
};

#[cfg(target_os = "linux")]
const WAYLAND: Library = Library {
    soname: c"libwayland-client.so.0",
    debian: "libwayland-client0",
    fedora: "libwayland-client",
};

/// Whether the loader finds and opens `soname`. The handle is kept: the
/// editor opens the same library when its window opens.
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn opens(soname: &std::ffi::CStr) -> bool {
    // SAFETY: `soname` is a C string naming one of the window system's
    // libraries, whose initializers the editor would run anyway; the
    // handle is never closed.
    let handle = unsafe { libc::dlopen(soname.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
    !handle.is_null()
}
