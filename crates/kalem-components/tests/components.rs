//! The bundled plugins built as components (wasm_todo W4) bind to this
//! Kalem's plugin host: each instantiates as a viewer, the workbook one
//! with its grid. Without the feature `build` there are none to check.

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
            .load(c.bytes)
            .unwrap_or_else(|e| panic!("{}: {e}", c.id));
        let v = kalem_script::viewer::Viewer::new(&host, &plugin, kalem_script::Limits::default())
            .unwrap_or_else(|e| panic!("{}: {e}", c.id));
        assert_eq!(v.is_grid(), c.id == "org.kalem.xlsx", "{}", c.id);
        // The manifest is the plugin's own; its `api` may lag, the
        // component being built against this checkout's API.
        let manifest: serde_json::Value = serde_json::from_str(c.manifest).unwrap();
        assert_eq!(manifest["id"], c.id);
    }
}
