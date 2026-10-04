//! A file that is not text in the terminal editor, through the image
//! viewer plugin (design §11.13, T3.7.2): half-block cells without a
//! graphics protocol, kitty's with one, the viewer's keys, the next file
//! of the folder, the information panel, a link inserted into a document
//! and a lossless turn saved.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kalem_core::DocumentMode;
use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

struct T {
    app: App,
    term: Terminal<TestBackend>,
    dir: PathBuf,
}

impl Drop for T {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl T {
    fn draw(&mut self) -> Buffer {
        let app = &mut self.app;
        self.term.draw(|f| app.draw(f)).unwrap();
        self.term.backend().buffer().clone()
    }

    fn screen(&mut self) -> String {
        let buf = self.draw();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn key(&mut self, code: KeyCode, m: KeyModifiers) {
        self.app.event(Event::Key(KeyEvent::new(code, m)));
        self.draw();
    }

    fn typ(&mut self, s: &str) {
        for c in s.chars() {
            let m = if c.is_ascii_uppercase() {
                KeyModifiers::SHIFT
            } else {
                KeyModifiers::NONE
            };
            self.key(KeyCode::Char(c), m);
        }
    }

    fn status(&mut self) -> String {
        self.app
            .doc
            .viewer
            .as_deref_mut()
            .map(|v| v.status())
            .unwrap_or_default()
    }
}

fn folder(name: &str) -> PathBuf {
    kalem_core::viewer::register(Arc::new(kalem_plugin_image_viewer::ImageViewer));
    let dir = std::env::temp_dir().join(format!("kalem-tui-viewer-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    image::RgbaImage::from_pixel(40, 20, image::Rgba([255, 0, 0, 255]))
        .save(dir.join("a.png"))
        .unwrap();
    image::RgbaImage::from_pixel(20, 40, image::Rgba([0, 0, 255, 255]))
        .save(dir.join("b.png"))
        .unwrap();
    std::fs::write(dir.join("notes.org"), "* Notes\n").unwrap();
    dir
}

fn open(dir: &Path, file: &str) -> T {
    let app = App::with_keymap(
        Some(&dir.join(file)),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let mut t = T {
        app,
        term: Terminal::new(TestBackend::new(60, 14)).unwrap(),
        dir: dir.to_path_buf(),
    };
    t.draw();
    t
}

#[test]
fn a_picture_opens_in_the_viewer() {
    let dir = folder("open");
    let mut t = open(&dir, "a.png");
    assert_eq!(t.app.doc.meta.mode, DocumentMode::Viewer);
    // No graphics protocol: half-block cells, in the picture's red (40 ×
    // 20 pixels are 4 cells of 10 × 20).
    let buf = t.draw();
    let red = ratatui::style::Color::Rgb(255, 0, 0);
    let cells = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
        .filter(|&(x, y)| buf[(x, y)].fg == red || buf[(x, y)].bg == red)
        .count();
    assert_eq!(cells, 4, "{}", t.screen());
    assert!(t.screen().contains("40 × 20"), "{}", t.screen());

    // With kitty's protocol: an image.
    let mut picker = ratatui_image::picker::Picker::halfblocks();
    picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
    t.app.editor.images.borrow_mut().picker = Some(picker);
    assert!(t.screen().contains("\x1b_G"));
    t.app.editor.images.borrow_mut().picker = None;

    // Zoom, fit, turn.
    t.typ("1");
    assert!(t.status().contains("100%"), "{}", t.status());
    t.typ("=");
    assert!(t.status().contains("125%"), "{}", t.status());
    t.typ("0");
    t.typ("r");
    assert!(t.status().starts_with("20 × 40"), "{}", t.status());

    // The information panel.
    t.typ("i");
    let s = t.screen();
    assert!(s.contains("Format") && s.contains("PNG"), "{s}");

    // The next picture of the folder, in place.
    t.typ("n");
    assert_eq!(
        t.app.doc.meta.path.as_deref().and_then(|p| p.file_name()),
        Some(std::ffi::OsStr::new("b.png"))
    );
    assert!(t.status().starts_with("20 × 40"), "{}", t.status());
    // And around again.
    t.typ("n");
    assert!(t.app.doc.meta.path.as_ref().unwrap().ends_with("a.png"));
}

#[test]
fn insert_link_at_point() {
    let dir = folder("link");
    let mut t = open(&dir, "notes.org");
    t.app.open_path(&dir.join("a.png"), None);
    t.draw();
    assert_eq!(t.app.doc.meta.mode, DocumentMode::Viewer);
    t.typ("L");
    assert_eq!(t.app.doc.meta.mode, DocumentMode::Org);
    assert!(
        t.app.doc.text().as_str().contains("[[file:a.png]]"),
        "{}",
        t.app.doc.text().as_str()
    );
}

#[test]
fn a_jpeg_turned_and_saved_changes_its_tag_only() {
    let dir = folder("jpeg");
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&image::RgbImage::from_pixel(8, 4, image::Rgb([0, 128, 0])))
        .unwrap();
    let jpeg = kalem_plugin_image_viewer::jpeg::set_orientation(&jpeg, 1).unwrap();
    let path = dir.join("photo.jpg");
    std::fs::write(&path, &jpeg).unwrap();
    let mut t = open(&dir, "photo.jpg");
    t.app
        .run_command("viewer.edit", serde_json::json!({ "edit": "rotate-right" }));
    assert!(t.app.doc.is_modified());
    assert!(t.status().starts_with("4 × 8"), "{}", t.status());
    // Undone and done again.
    t.key(KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert!(!t.app.doc.is_modified());
    t.app
        .run_command("viewer.edit", serde_json::json!({ "edit": "rotate-right" }));
    t.key(KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert!(!t.app.doc.is_modified());
    let saved = std::fs::read(&path).unwrap();
    assert_eq!(saved.len(), jpeg.len());
    assert_eq!((0..jpeg.len()).filter(|&i| jpeg[i] != saved[i]).count(), 1);
    assert_eq!(
        kalem_plugin_image_viewer::jpeg::orientation(&saved),
        Some(6)
    );
}

#[test]
fn vim_quit_closes_a_viewer_as_any_file() {
    // A picture open with Vim's keys: `:` opens the command line, and `:q`
    // closes the pane, then quits from the last one, as with text.
    let dir = folder("vimquit");
    let config = kalem_core::settings::Config::from_layers(&[(
        kalem_core::settings::Layer::User,
        None,
        "editor.keymap_profile = \"vim\"\n",
    )]);
    let app = App::with_keymap(Some(&dir.join("a.png")), config, Caps::full(), &[], Vec::new())
        .unwrap();
    let mut t = T {
        app,
        term: Terminal::new(TestBackend::new(60, 14)).unwrap(),
        dir: dir.clone(),
    };
    t.draw();
    assert_eq!(t.app.doc.meta.mode, DocumentMode::Viewer);
    t.typ(" wn");
    t.typ(":q");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(!t.app.quit, "the second pane closed");
    assert_eq!(t.app.doc.meta.mode, DocumentMode::Viewer);
    t.typ(":q");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    assert!(t.app.quit, "quit from the last pane");
}
