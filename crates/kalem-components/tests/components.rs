//! The bundled plugins, components released by getkalem/plugins (wasm_todo
//! W9), bind to this Kalem's plugin host: each instantiates as a viewer,
//! the workbook one with its grid, and asks for an API this host
//! implements. Without the feature `embed` there are none to check.

#[test]
fn the_bundled_components_bind() {
    let all = kalem_components::components();
    if all.is_empty() {
        return;
    }
    assert_eq!(all.len(), 3, "the three bundled plugins");
    let host = kalem_script::Host::new(None).unwrap();
    for c in all {
        let plugin = host
            .load(c.wasm())
            .unwrap_or_else(|e| panic!("{}: {e}", c.id));
        let v = kalem_script::viewer::Viewer::new(&host, &plugin, kalem_script::Limits::default())
            .unwrap_or_else(|e| panic!("{}: {e}", c.id));
        assert_eq!(v.is_grid(), c.id == "org.kalem.xlsx", "{}", c.id);
        let manifest: serde_json::Value = serde_json::from_str(c.manifest).unwrap();
        assert_eq!(manifest["id"], c.id);
        assert!(
            kalem_script::api_compatible(manifest["api"].as_str()),
            "{} asks for API {}",
            c.id,
            manifest["api"]
        );
    }
}

#[cfg(feature = "viewers")]
#[test]
fn the_built_in_workbook_viewer_opens_one_with_a_password() {
    // `data/locked.xlsx`: a workbook encrypted as Excel encrypts one with
    // a password to open (by xlsx 0.0.8, the password `kalem`). The
    // built-in viewer asks for it, asks again for a wrong one, opens it
    // with the right one, and saves it encrypted again. The viewer is
    // Kalem's own, with its manifest's limits: the password's key takes
    // longer than a plain call's 100 ms on CI.
    use kalem_viewer::{FileHandle, Viewer as _};
    let Some(c) = kalem_components::components()
        .iter()
        .find(|c| c.id == "org.kalem.xlsx")
    else {
        return;
    };
    let host = std::sync::Arc::new(kalem_script::Host::new(None).unwrap());
    let v = c.viewer(host);
    let dir = std::env::temp_dir().join(format!("kalem-locked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("locked.xlsx");
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/locked.xlsx"),
        &path,
    )
    .unwrap();
    let asked = v
        .open(FileHandle::new(&path))
        .err()
        .expect("a password asked");
    assert!(asked.is_needs_password(), "{asked}");
    let again = v
        .open_with_password(FileHandle::new(&path), "Kalem")
        .err()
        .expect("asked again");
    assert!(again.is_needs_password(), "{again}");
    let mut d = v
        .open_with_password(FileHandle::new(&path), "kalem")
        .unwrap();
    assert_eq!(d.cell_input(0, 1, 3), "=B2+C2");
    d.set_cell(0, 1, 1, "1300").unwrap();
    let saved = d.save().unwrap().bytes;
    assert!(saved.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]), "encrypted");
    std::fs::write(&path, saved).unwrap();
    assert!(v.open(FileHandle::new(&path)).is_err());
    let mut d = v
        .open_with_password(FileHandle::new(&path), "kalem")
        .unwrap();
    assert_eq!(d.cell_input(0, 1, 1), "1300");
    let _ = std::fs::remove_dir_all(&dir);
}
