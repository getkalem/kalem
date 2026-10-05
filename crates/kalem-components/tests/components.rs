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
