//! A notes graph's folder offered as a project when its plugin's layer
//! arrives after the document was opened: the plugin installed while
//! Kalem runs, or started after the first document (compiling it the
//! first time). In a test binary of its own, as the layers are the
//! process's.

use std::time::Instant;

use kalem_core::layers::{self, LayerSpec};
use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn screen(term: &mut Terminal<TestBackend>, app: &mut App) -> String {
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_layer_added_after_the_document_offers_its_folder() {
    let dir = std::env::temp_dir().join(format!("kalem-tui-layer-offer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("notes/pages")).unwrap();
    std::fs::write(dir.join("notes/notes.marker"), "").unwrap();
    let path = dir.join("notes/pages/n.md");
    std::fs::write(&path, "- a note\n").unwrap();
    let mut app = App::with_keymap(
        Some(&path),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let mut term = Terminal::new(TestBackend::new(100, 12)).unwrap();
    app.tick(Instant::now());
    assert!(!screen(&mut term, &mut app).contains("A notes graph"));
    // The plugin started: its layer serves the folder.
    layers::register(LayerSpec {
        plugin: "test".into(),
        id: "notes".into(),
        markers: vec!["notes.marker".into()],
        modes: vec!["markdown".into()],
        open: None,
    });
    app.tick(Instant::now());
    let shown = screen(&mut term, &mut app);
    assert!(shown.contains("A notes graph"), "{shown}");
    let _ = std::fs::remove_dir_all(&dir);
}
