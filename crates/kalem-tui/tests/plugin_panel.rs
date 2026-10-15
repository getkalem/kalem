//! A plugin's panel in the terminal (`kalem_core::extensions`, D11): a
//! long text wrapped under its widget, a click on any of its lines
//! reaching the plugin as that widget's, the panel scrolled by the wheel
//! and by keys once it has them. In a test binary of its own, as the
//! plugins' panels and events are the process's.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use kalem_core::extensions::{self as x, Panel, PanelEvent, Widget, WidgetKind};
use kalem_core::settings::{Config, Layer};
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::sync::Mutex;

/// What the plugin heard: each widget's key and event.
static HEARD: Mutex<Vec<(String, PanelEvent)>> = Mutex::new(Vec::new());

/// The plugins' panels are the process's: one test at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// A plugin that only listens to its panel.
struct Listener;

impl x::Extensions for Listener {
    fn run(&mut self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn event(&mut self, _: &kalem_core::events::Event) -> Option<String> {
        None
    }
    fn answer(&mut self, _: u64, _: x::Answer) {}
    fn panel_event(&mut self, _: &str, key: &str, event: &PanelEvent) {
        HEARD
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((key.to_string(), event.clone()));
    }
    fn settings_changed(&mut self, _: &[String]) {}
    fn respond(&mut self, _: u64, _: Result<x::HttpResponse, String>) {}
    fn process_done(&mut self, _: u64, _: Result<x::ProcessExit, String>) {}
}

fn heard() -> Vec<(String, PanelEvent)> {
    std::mem::take(&mut *HEARD.lock().unwrap_or_else(|e| e.into_inner()))
}

const PANEL: &str = "paneltest.refs";

const LONG: &str = "First try of Editor on the Book's plugins chapter, read whole at last";

/// The backlinks of a page as the graph plugin gives them: a section, a
/// page under it, its references under that; `more` references after.
fn widgets(more: usize) -> Vec<Widget> {
    let w = |key: &str, kind: WidgetKind, children: Vec<u32>| Widget {
        key: key.into(),
        kind,
        children,
    };
    let item = |label: &str, detail: Option<&str>, expanded: Option<bool>| WidgetKind::Item {
        label: label.into(),
        detail: detail.map(str::to_string),
        expanded,
        selected: false,
    };
    let mut refs: Vec<u32> = vec![3];
    let mut out = vec![
        w("", WidgetKind::Column, vec![1]),
        w("1", item("Linked references", None, Some(true)), vec![2]),
        w("1.0", item("Oct 13th, 2026", None, Some(true)), Vec::new()),
        w("1.0.0", item(LONG, None, None), Vec::new()),
    ];
    for n in 0..more {
        refs.push(out.len() as u32);
        out.push(w(
            &format!("1.0.{}", n + 1),
            item(&format!("Reference {}", n + 1), None, None),
            Vec::new(),
        ));
    }
    out[2].children = refs;
    out
}

struct T {
    app: App,
    term: Terminal<TestBackend>,
    _serial: std::sync::MutexGuard<'static, ()>,
}

impl Drop for T {
    fn drop(&mut self) {
        x::remove_panel(PANEL);
    }
}

/// A terminal editor `width` by `height` on a note, the panel shown at
/// its side with `more` references after the long one.
fn shown(width: u16, height: u16, more: usize) -> T {
    shown_with(Config::default(), width, height, more)
}

fn shown_with(config: Config, width: u16, height: u16, more: usize) -> T {
    let serial = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| x::install(Box::new(Listener)));
    let dir = std::env::temp_dir().join(format!("kalem-tui-panel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("note.md");
    std::fs::write(&path, "body\n").unwrap();
    let app = App::with_keymap(Some(&path), config, Caps::full(), &[], Vec::new()).unwrap();
    x::add_panel(Panel {
        plugin: "paneltest".into(),
        id: PANEL.into(),
        title: "Backlinks".into(),
        bottom: false,
        widgets: Vec::new(),
    });
    x::set_panel(PANEL, widgets(more));
    let mut t = T {
        app,
        term: Terminal::new(TestBackend::new(width, height)).unwrap(),
        _serial: serial,
    };
    t.app
        .run_command("view.pluginPanel", serde_json::json!({ "id": PANEL }));
    heard();
    t
}

impl T {
    fn screen(&mut self) -> Vec<String> {
        let app = &mut self.app;
        self.term.draw(|f| app.draw(f)).unwrap();
        let buf = self.term.backend().buffer();
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    /// Where `text` is on the screen: its column (in characters) and row.
    fn find(&mut self, text: &str) -> Option<(u16, u16)> {
        self.screen().iter().enumerate().find_map(|(y, row)| {
            row.find(text)
                .map(|b| (row[..b].chars().count() as u16, y as u16))
        })
    }

    fn mouse(&mut self, kind: MouseEventKind, at: (u16, u16)) {
        self.app.event(Event::Mouse(MouseEvent {
            kind,
            column: at.0,
            row: at.1,
            modifiers: KeyModifiers::NONE,
        }));
        self.screen();
    }

    fn key(&mut self, code: KeyCode) {
        self.chord(code, KeyModifiers::NONE);
    }

    fn chord(&mut self, code: KeyCode, m: KeyModifiers) {
        self.app.event(Event::Key(KeyEvent::new(code, m)));
        self.screen();
    }

    fn typ(&mut self, keys: &str) {
        for c in keys.chars() {
            self.key(KeyCode::Char(c));
        }
    }
}

#[test]
fn a_long_reference_reads_whole_and_a_click_on_any_of_its_lines_is_its() {
    let mut t = shown(80, 20, 0);
    let screen = t.screen();
    let (x0, y0) = t.find("First try of").expect("the reference shown");
    // Its text over the lines under it, at its indent, every word shown.
    let lines: Vec<String> = screen[y0 as usize..]
        .iter()
        .map(|r| r.chars().skip(x0 as usize).collect::<String>())
        .take_while(|r| !r.starts_with(' '))
        .map(|r| r.split("  ").next().unwrap_or("").trim_end().to_string())
        .collect();
    assert!(lines.len() > 1, "{lines:?}");
    assert_eq!(lines.join(" "), LONG);
    // A click on its last line, past its text, reaches the plugin as its.
    let last = y0 + lines.len() as u16 - 1;
    t.mouse(MouseEventKind::Down(MouseButton::Left), (x0 + 2, last));
    assert_eq!(heard(), [("1.0.0".to_string(), PanelEvent::Clicked)]);
    // A click on the page above it folds the page.
    let (px, py) = t.find("Oct 13th").unwrap();
    t.mouse(MouseEventKind::Down(MouseButton::Left), (px, py));
    assert_eq!(heard(), [("1.0".to_string(), PanelEvent::Expanded(false))]);
}

#[test]
fn the_wheel_and_the_keys_scroll_a_panel_longer_than_its_height() {
    let mut t = shown(80, 12, 20);
    let marked = |t: &mut T, mark: char| t.screen().iter().any(|r| r.contains(mark));
    // Nine lines show under the title: more below, marked.
    assert!(t.find("Reference 3").is_some() && t.find("Reference 4").is_none());
    assert!(marked(&mut t, '▼') && !marked(&mut t, '▲'));
    // The wheel over the panel scrolls it, never past its last line.
    let (x0, y0) = t.find("Backlinks").unwrap();
    for _ in 0..10 {
        t.mouse(MouseEventKind::ScrollDown, (x0, y0 + 4));
    }
    assert!(t.find("Reference 20").is_some() && t.find("Linked references").is_none());
    assert!(marked(&mut t, '▲') && !marked(&mut t, '▼'));
    t.mouse(MouseEventKind::ScrollUp, (x0, y0 + 4));
    assert!(t.find("Reference 20").is_none());

    // Moving the focus left from the only pane gives the panel the keys:
    // typing does not reach the note while it has them.
    t.app
        .run_command("pane.focus", serde_json::json!({ "dir": "left" }));
    t.key(KeyCode::Char('x'));
    assert_eq!(t.app.doc.text().as_str(), "body\n");
    t.key(KeyCode::Home);
    assert!(t.find("Linked references").is_some());
    // Down to the long reference, Enter acts on it.
    t.key(KeyCode::Down);
    t.key(KeyCode::Down);
    t.key(KeyCode::Enter);
    assert_eq!(heard(), [("1.0.0".to_string(), PanelEvent::Clicked)]);
    // The last entry, brought into view.
    t.key(KeyCode::End);
    assert!(t.find("Reference 20").is_some());
    t.key(KeyCode::Enter);
    assert_eq!(heard(), [("1.0.20".to_string(), PanelEvent::Clicked)]);
    // A page up: the entry chosen scrolled away, the first shown is.
    t.key(KeyCode::PageUp);
    assert!(t.find("Reference 20").is_none());
    t.key(KeyCode::Enter);
    let first = heard();
    assert!(
        first.len() == 1 && first[0].0 != "1.0.20" && first[0].1 == PanelEvent::Clicked,
        "{first:?}"
    );
    // Escape gives the keys back to the note.
    t.key(KeyCode::Esc);
    t.key(KeyCode::Char('x'));
    assert_eq!(t.app.doc.text().as_str(), "xbody\n");

    // A click on the panel away from its entries gives it the keys; one
    // in the note takes them back.
    t.mouse(MouseEventKind::Down(MouseButton::Left), (x0, y0));
    t.key(KeyCode::Char('y'));
    assert_eq!(t.app.doc.text().as_str(), "xbody\n");
    let (bx, by) = t.find("xbody").unwrap();
    t.mouse(MouseEventKind::Down(MouseButton::Left), (bx + 5, by));
    t.key(KeyCode::Char('y'));
    assert_eq!(t.app.doc.text().as_str(), "xbodyy\n");
    // Moving the focus left again and then right does the same.
    t.app
        .run_command("pane.focus", serde_json::json!({ "dir": "left" }));
    t.key(KeyCode::Char('z'));
    t.app
        .run_command("pane.focus", serde_json::json!({ "dir": "right" }));
    t.key(KeyCode::Char('z'));
    assert_eq!(t.app.doc.text().as_str(), "xbodyyz\n");
}

#[test]
fn vims_window_keys_move_to_the_panel_and_back() {
    let vim = Config::from_layers(&[(Layer::User, None, "editor.keymap_profile = \"vim\"\n")]);
    let mut t = shown_with(vim, 80, 20, 2);
    // Ctrl-W h from the only pane: the panel has the keys, its first
    // entry chosen; j j l acts on the long reference.
    t.chord(KeyCode::Char('w'), KeyModifiers::CONTROL);
    t.key(KeyCode::Char('h'));
    t.typ("jjl");
    assert_eq!(heard(), [("1.0.0".to_string(), PanelEvent::Clicked)]);
    // Ctrl-W l: back in the text, where Vim's keys work again.
    t.chord(KeyCode::Char('w'), KeyModifiers::CONTROL);
    t.key(KeyCode::Char('l'));
    t.typ("ix");
    t.key(KeyCode::Esc);
    assert_eq!(t.app.doc.text().as_str(), "xbody\n");
    // Doom's SPC w h and SPC w l do the same.
    t.typ("\u{20}wh");
    t.typ("k");
    t.key(KeyCode::Enter);
    assert_eq!(heard(), [("1.0".to_string(), PanelEvent::Expanded(false))]);
    t.typ("\u{20}wl");
    t.typ("x");
    assert_eq!(t.app.doc.text().as_str(), "body\n");
}
