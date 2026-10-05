//! The released interfaces of the plugin API do not change (the Book,
//! Part III, "Versions of the plugin API"): every file frozen at a
//! release (`wit-frozen/VERSION/`) is the same in `wit/`, its `package`
//! line aside. A new function goes into a new interface (`grid-2`) in a
//! file of its own, and `worlds.wit` exports it.

use std::path::Path;

/// The text without its `package …;` line, which a later version changes.
fn body(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim_start().starts_with("package "))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn released_interfaces_are_unchanged() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0;
    for release in std::fs::read_dir(root.join("wit-frozen"))
        .unwrap()
        .flatten()
    {
        if !release.path().is_dir() {
            continue;
        }
        for file in std::fs::read_dir(release.path()).unwrap().flatten() {
            let name = file.file_name();
            let frozen = std::fs::read_to_string(file.path()).unwrap();
            let now = std::fs::read_to_string(root.join("wit").join(&name)).unwrap_or_else(|_| {
                panic!(
                    "{} of release {} is gone from wit/: a released interface stays",
                    name.to_string_lossy(),
                    release.file_name().to_string_lossy()
                )
            });
            assert!(
                body(&now) == body(&frozen),
                "wit/{} changed since release {}: a released interface never changes; put new \
                 functions in a new interface (a file of its own) and export it in worlds.wit",
                name.to_string_lossy(),
                release.file_name().to_string_lossy()
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "no released interface found");
}
