//! The released interfaces of the plugin API do not change (the Book,
//! Part III, "Versions of the plugin API"): every file frozen at a
//! release (`wit-frozen/VERSION/`) is the same in `wit/`, its `package`
//! line aside. A new function goes into a new interface (`grid-2`) in a
//! file of its own, and `worlds.wit` exports it.

use std::path::Path;

/// The files of the API's core, which every extension plugin binds: the
/// editor's commands and the plugin's own export (`kalem.wit`), the
/// document (`editor.wit`), what a plugin shows (`ui.wit`), settings,
/// files and the network (`access.wit`), and Babel's executors
/// (`executor.wit`). They were left out of `wit-frozen/` until 0.2.21
/// (see its README): a copy of each must stay there.
const CORE: [&str; 5] = [
    "kalem.wit",
    "editor.wit",
    "ui.wit",
    "access.wit",
    "executor.wit",
];

/// The text without its `package …;` line, which a later version changes.
fn body(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim_start().starts_with("package "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The releases' folders of `wit-frozen/`.
fn releases(root: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(root.join("wit-frozen"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect()
}

#[test]
fn released_interfaces_are_unchanged() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0;
    for release in releases(root) {
        for file in std::fs::read_dir(&release).unwrap().flatten() {
            let name = file.file_name();
            let frozen = std::fs::read_to_string(file.path()).unwrap();
            let now = std::fs::read_to_string(root.join("wit").join(&name)).unwrap_or_else(|_| {
                panic!(
                    "{} of release {} is gone from wit/: a released interface stays",
                    name.to_string_lossy(),
                    release.file_name().unwrap().to_string_lossy()
                )
            });
            assert!(
                body(&now) == body(&frozen),
                "wit/{} changed since release {}: a released interface never changes; put new \
                 functions in a new interface (a file of its own) and export it in worlds.wit",
                name.to_string_lossy(),
                release.file_name().unwrap().to_string_lossy()
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "no released interface found");
}

#[test]
fn the_core_is_frozen() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let releases = releases(root);
    for name in CORE {
        assert!(
            releases.iter().any(|r| r.join(name).is_file()),
            "{name} has no copy in wit-frozen/: the core of the plugin API is released and \
             frozen like every other interface"
        );
    }
}

#[test]
fn only_worlds_wit_declares_worlds() {
    // A world gains the interfaces of each later version, so a file that
    // holds one cannot be frozen; kalem.wit held the extension worlds and
    // so went unfrozen until 0.2.21.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for file in std::fs::read_dir(root.join("wit")).unwrap().flatten() {
        let name = file.file_name();
        if name == "worlds.wit" {
            continue;
        }
        let text = std::fs::read_to_string(file.path()).unwrap();
        assert!(
            !text.lines().any(|l| l.trim_start().starts_with("world ")),
            "wit/{} declares a world: worlds live in worlds.wit, so that every interface file \
             can be frozen whole",
            name.to_string_lossy()
        );
    }
}
