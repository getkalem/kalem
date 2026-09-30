//! The graphical editor in gpui's headless test platform: keys, typing
//! through the input handler, commands, folding.

use std::rc::Rc;

use std::sync::atomic::{AtomicUsize, Ordering};

use gpui::{Entity, TestAppContext, VisualTestContext};
use kalem_core::settings::Config;
use kalem_ui::editor::Editor;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::Workspace;

/// The primary modifier of the Word-like profile on this platform.
fn primary() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    }
}

fn open<'a>(text: &str, cx: &'a mut TestAppContext) -> (Entity<Editor>, &'a mut VisualTestContext) {
    open_with(text, || None, cx)
}

/// Opens `text` with `html` as the system clipboard's HTML.
fn open_with<'a>(
    text: &str,
    html: fn() -> Option<String>,
    cx: &'a mut TestAppContext,
) -> (Entity<Editor>, &'a mut VisualTestContext) {
    open_named(text, "t.org", html, cx)
}

/// Opens `text` saved as `name`.
fn open_named<'a>(
    text: &str,
    name: &str,
    html: fn() -> Option<String>,
    cx: &'a mut TestAppContext,
) -> (Entity<Editor>, &'a mut VisualTestContext) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("kalem-ui-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    let mut shared = kalem_ui::shared(Config::default());
    shared.html_clipboard = html;
    shared.settings_path = Some(dir.join("settings.toml"));
    shared.projects = std::cell::RefCell::new(kalem_core::projects::ProjectState::load(Some(
        dir.join("projects.toml"),
    )));
    let shared = Rc::new(shared);
    let mut editor = None;
    let (_ws, vcx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
        let focus = gpui::Focusable::focus_handle(e.read(cx), cx);
        window.focus(&focus, cx);
        editor = Some(e.clone());
        Workspace::new(e, window, cx)
    });
    vcx.run_until_parked();
    (editor.unwrap(), vcx)
}

fn text(e: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    e.read_with(cx, |e, _| e.doc.text().as_str().to_string())
}

fn at(e: &Entity<Editor>, pos: usize, cx: &mut VisualTestContext) {
    e.update(cx, |e, cx| {
        e.doc.move_cursor(pos, false);
        e.after_change(cx);
    });
    cx.run_until_parked();
}

#[gpui::test]
fn typing_and_commands(cx: &mut TestAppContext) {
    let (e, cx) = open("* A\nword\n", cx);
    at(&e, 8, cx);
    cx.simulate_input("s!");
    assert_eq!(text(&e, cx), "* A\nwords!\n");
    // Undo, then bold a selection.
    cx.simulate_keystrokes(&format!("{}-z", primary()));
    assert_eq!(text(&e, cx), "* A\nword\n");
    cx.simulate_keystrokes("shift-left shift-left shift-left shift-left");
    cx.simulate_keystrokes(&format!("{}-b", primary()));
    assert_eq!(text(&e, cx), "* A\n*word*\n");
}

#[gpui::test]
fn enter_in_lists(cx: &mut TestAppContext) {
    let (e, cx) = open("- one\n", cx);
    at(&e, 5, cx);
    cx.simulate_keystrokes("enter");
    cx.simulate_input("two");
    assert_eq!(text(&e, cx), "- one\n- two\n");
}

#[gpui::test]
fn folding_with_tab(cx: &mut TestAppContext) {
    let (e, cx) = open("* A\nbody\n* B\n", cx);
    at(&e, 1, cx);
    assert_eq!(e.read_with(cx, |e, _| e.visible.clone()), vec![0, 1, 2, 3]);
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(e.read_with(cx, |e, _| e.visible.clone()), vec![0, 2, 3]);
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(e.read_with(cx, |e, _| e.visible.clone()), vec![0, 1, 2, 3]);
}

#[gpui::test]
fn tables_keep_columns(cx: &mut TestAppContext) {
    let (e, cx) = open("| ab   | c |\n", cx);
    at(&e, 4, cx);
    cx.simulate_input("x");
    assert_eq!(text(&e, cx), "| abx  | c |\n");
    cx.simulate_keystrokes("tab");
    cx.simulate_input("Z");
    assert_eq!(text(&e, cx), "| abx | Z |\n");
}

#[gpui::test]
fn lines_are_painted(cx: &mut TestAppContext) {
    let (e, cx) = open("* TODO Head\nSome *bold* text\n- [ ] task\n", cx);
    at(&e, 0, cx);
    let painted = e.read_with(cx, |e, _| e.painted.borrow().len());
    assert_eq!(painted, 4);
    // The checkbox is a widget that toggles on click.
    let b = e.read_with(cx, |e, _| {
        e.painted
            .borrow()
            .get(&2)
            .and_then(|p| p.widgets.first().map(|w| w.0))
            .expect("a checkbox")
    });
    cx.simulate_click(b.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(text(&e, cx), "* TODO Head\nSome *bold* text\n- [X] task\n");
}

#[gpui::test]
fn accessible_text(cx: &mut TestAppContext) {
    let (e, cx) = open("* Head\nSome *bold* text\n- [X] task\n", cx);
    at(&e, 12, cx);
    let t = e.read_with(cx, |e, _| e.a11y_text());
    let lines: Vec<&str> = t.lines.iter().map(|(_, s)| s.as_str()).collect();
    // Hidden markup is not read; the cursor's line shows its markup.
    assert_eq!(lines, ["Head", "Some *bold* text", "• ☑ task", ""]);
    assert_eq!(t.selection, Some(((1, 5), (1, 5))));
}

#[gpui::test]
fn tags_at_the_right_edge(cx: &mut TestAppContext) {
    let text = "* Head :work:\nbody\n";
    let (e, cx) = open(text, cx);
    at(&e, text.len(), cx);
    let (x, width) = e.read_with(cx, |e, _| {
        let p = e.painted.borrow().get(&0).cloned().expect("painted");
        let tag = text.find(":work:").unwrap();
        (
            p.layout.caret(p.view.display_offset(tag)).origin.x,
            p.bounds.size.width,
        )
    });
    assert!(x > width / 2., "tags at {x:?} of {width:?}");
}

#[gpui::test]
fn scripts_are_smaller(cx: &mut TestAppContext) {
    let text = "E = mc^{2} and H_{2}O\nend\n";
    let (e, cx) = open(text, cx);
    at(&e, text.len(), cx);
    // The superscript is narrower than the same text at full size.
    let (w2, wm) = e.read_with(cx, |e, _| {
        let p = e.painted.borrow().get(&0).cloned().expect("painted");
        let d = |s: usize| p.layout.caret(p.view.display_offset(s)).origin.x;
        let two = text.find('2').unwrap();
        let m = text.find('m').unwrap();
        (d(two + 1) - d(two), d(m + 1) - d(m))
    });
    assert!(w2 < wm, "{w2:?} {wm:?}");
}

#[gpui::test]
fn wrapped_items_hang(cx: &mut TestAppContext) {
    let item = format!("- [ ] {}\n", "word ".repeat(80));
    let (e, cx) = open(&format!("{item}end\n"), cx);
    at(&e, item.len() + 1, cx);
    // Wrapped rows start where the item's text does, not at the bullet.
    let (text_x, second_row_x) = e.read_with(cx, |e, _| {
        let p = e.painted.borrow().get(&0).cloned().expect("painted");
        assert!(p.layout.rows.len() > 1);
        let first = p.layout.caret(p.view.display_offset(6)).origin.x;
        let row2 = p.layout.rows[1].start;
        (first, p.layout.caret(row2).origin.x)
    });
    assert_eq!(text_x, second_row_x);
}

#[gpui::test]
fn tables_as_grids(cx: &mut TestAppContext) {
    let text = "| Name | Qty |\n|---+---|\n| *apple* | 3 |\n| b | 10 |\nafter\n";
    let (e, cx) = open(text, cx);
    at(&e, text.len(), cx);
    let x = |e: &Editor, line: usize, src: usize| {
        let p = e.painted.borrow().get(&line).cloned().expect("painted");
        p.layout.caret(p.view.display_offset(src)).origin.x
    };
    let (name, apple, b, three_end, ten_end) = e.read_with(cx, |e, _| {
        (
            x(e, 0, text.find("Name").unwrap()),
            x(e, 2, text.find("*apple*").unwrap() + 1),
            x(e, 3, text.find("| b").unwrap() + 2),
            x(e, 2, text.find("3 |").unwrap() + 1),
            x(e, 3, text.find("10").unwrap() + 2),
        )
    });
    // Text columns start together, number columns end together.
    let near = |a: gpui::Pixels, b: gpui::Pixels| (f32::from(a) - f32::from(b)).abs() < 0.01;
    assert!(near(name, b) && near(apple, b), "{name:?} {apple:?} {b:?}");
    assert!(near(three_end, ten_end), "{three_end:?} {ten_end:?}");
    // A click on a cell lands in its source.
    let (pos, bounds) = e.read_with(cx, |e, _| {
        let p = e.painted.borrow().get(&3).cloned().unwrap();
        let caret = p
            .layout
            .caret(p.view.display_offset(text.find("10").unwrap()));
        (
            text.find("10").unwrap(),
            gpui::Bounds::new(p.bounds.origin + caret.origin, caret.size),
        )
    });
    cx.simulate_click(bounds.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let head = e.read_with(cx, |e, _| e.doc.selection.head);
    assert!(head == pos || head == pos + 1, "{head} {pos}");
}

#[gpui::test]
fn code_blocks_copy(cx: &mut TestAppContext) {
    let text =
        "#+begin_src rust\nfn main() {}\n#+end_src\n-----\n#+begin_quote\nq\n#+end_quote\nend\n";
    let (e, cx) = open(text, cx);
    at(&e, text.len(), cx);
    let (shown, button) = e.read_with(cx, |e, _| {
        let p = e.painted.borrow().get(&0).cloned().expect("painted");
        (p.view.display(), p.buttons.first().map(|b| b.0))
    });
    assert!(
        shown.starts_with("rust") && !shown.contains("#+begin"),
        "{shown:?}"
    );
    cx.simulate_click(
        button.expect("a copy button").center(),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    let copied = cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(copied.as_deref(), Some("fn main() {}\n"));
    // Inside the block, its first line is the source again.
    at(&e, 20, cx);
    let shown = e.read_with(cx, |e, _| {
        e.painted.borrow().get(&0).map(|p| p.view.display())
    });
    assert_eq!(shown.as_deref(), Some("#+begin_src rust"));
}

#[gpui::test]
fn table_of_contents(cx: &mut TestAppContext) {
    let text = "#+TOC: headlines 2\n* One\n** One A\n* Two\n";
    let (e, cx) = open(text, cx);
    at(&e, text.len(), cx);
    let rows = e.read_with(cx, |e, _| {
        let p = e.painted.borrow().get(&0).cloned().expect("painted");
        p.jumps.clone()
    });
    assert_eq!(
        rows.iter().map(|r| r.1).collect::<Vec<_>>(),
        [19, 25, 34],
        "{rows:?}"
    );
    // A row leads to its heading.
    cx.simulate_click(rows[1].0.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(e.read_with(cx, |e, _| e.doc.selection.head), 25);
    // On the cursor's line, the keyword is itself.
    at(&e, 3, cx);
    let (shown, rows) = e.read_with(cx, |e, _| {
        let p = e.painted.borrow().get(&0).cloned().expect("painted");
        (p.view.display(), p.jumps.len())
    });
    assert_eq!((shown.as_str(), rows), ("#+TOC: headlines 2", 0));
}

#[gpui::test]
fn following_links(cx: &mut TestAppContext) {
    let text = "* Target\ntext [[*Target][go]] here\n";
    let (e, cx) = open(text, cx);
    at(&e, text.len(), cx);
    let b = e.read_with(cx, |e, _| {
        let p = e.painted.borrow().get(&1).cloned().expect("painted");
        let go = text.find("go]").unwrap();
        let caret = p.layout.caret(p.view.display_offset(go));
        gpui::Bounds::new(p.bounds.origin + caret.origin, caret.size)
    });
    let mut m = gpui::Modifiers::default();
    if cfg!(target_os = "macos") {
        m.platform = true;
    } else {
        m.control = true;
    }
    cx.simulate_click(b.center() + gpui::point(gpui::px(3.), gpui::px(0.)), m);
    cx.run_until_parked();
    assert_eq!(e.read_with(cx, |e, _| e.doc.selection.head), 0);
}

#[gpui::test]
fn ime_composition(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;
    let (e, cx) = open("日本\n", cx);
    at(&e, 6, cx);
    e.update_in(cx, |e, window, cx| {
        e.replace_and_mark_text_in_range(None, "k", None, window, cx);
        e.replace_and_mark_text_in_range(None, "か", None, window, cx);
    });
    assert_eq!(text(&e, cx), "日本か\n");
    assert_eq!(e.read_with(cx, |e, _| e.marked.clone()), Some(6..9));
    // UTF-16 ranges for the input method.
    let marked = e.update_in(cx, |e, window, cx| e.marked_text_range(window, cx));
    assert_eq!(marked, Some(2..3));
    e.update_in(cx, |e, window, cx| {
        e.replace_text_in_range(None, "語", window, cx)
    });
    assert_eq!(text(&e, cx), "日本語\n");
    assert_eq!(
        e.read_with(cx, |e, _| (e.marked.clone(), e.doc.selection.head)),
        (None, 9)
    );
}

#[gpui::test]
fn clicks_select(cx: &mut TestAppContext) {
    let text = "alpha beta gamma\nnext\n";
    let (e, cx) = open(text, cx);
    at(&e, text.len(), cx);
    let point_at = |e: &Entity<Editor>, cx: &mut VisualTestContext, pos: usize| {
        e.read_with(cx, |e, _| {
            let p = e.painted.borrow().get(&0).cloned().expect("painted");
            let c = p.layout.caret(p.view.display_offset(pos));
            p.bounds.origin + c.origin + gpui::point(gpui::px(2.), c.size.height / 2.)
        })
    };
    let beta = point_at(&e, cx, 7);
    cx.simulate_event(gpui::MouseDownEvent {
        button: gpui::MouseButton::Left,
        position: beta,
        modifiers: gpui::Modifiers::default(),
        click_count: 2,
        first_mouse: false,
    });
    cx.run_until_parked();
    assert_eq!(
        e.read_with(cx, |e, _| e.doc.selected_text().map(str::to_string)),
        Some("beta".into())
    );
    cx.simulate_event(gpui::MouseDownEvent {
        button: gpui::MouseButton::Left,
        position: beta,
        modifiers: gpui::Modifiers::default(),
        click_count: 3,
        first_mouse: false,
    });
    cx.run_until_parked();
    assert_eq!(
        e.read_with(cx, |e, _| e.doc.selected_text().map(str::to_string)),
        Some("alpha beta gamma\n".into())
    );
    // Shift with arrows extends from the cursor.
    at(&e, 0, cx);
    cx.simulate_keystrokes("shift-right shift-right");
    assert_eq!(
        e.read_with(cx, |e, _| e.doc.selected_text().map(str::to_string)),
        Some("al".into())
    );
}

#[gpui::test]
fn completion_menus(cx: &mut TestAppContext) {
    let (e, cx) = open("* Intro\n\n", cx);
    at(&e, 8, cx);
    cx.simulate_input("#+ti");
    assert!(e.read_with(cx, |e, _| e.completion.is_some()));
    cx.simulate_keystrokes("enter");
    cx.simulate_input("Doc");
    assert_eq!(text(&e, cx), "* Intro\n#+title: Doc\n");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("see [[In");
    cx.simulate_keystrokes("down up tab");
    assert_eq!(text(&e, cx), "* Intro\n#+title: Doc\nsee [[*Intro]]\n");
}

#[gpui::test]
fn tab_by_context(cx: &mut TestAppContext) {
    let (e, cx) = open("- a\n- b\n", cx);
    at(&e, 6, cx);
    // In a list, Tab indents the item; Shift+Tab outdents it.
    cx.simulate_keystrokes("tab");
    assert_eq!(text(&e, cx), "- a\n  - b\n");
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(text(&e, cx), "- a\n- b\n");
    // Enter on an empty item leaves the list.
    at(&e, 7, cx);
    cx.simulate_keystrokes("enter enter");
    cx.simulate_input("after");
    assert_eq!(text(&e, cx), "- a\n- b\nafter\n");
}

#[gpui::test]
fn pasting(cx: &mut TestAppContext) {
    let (e, cx) = open("text\n", cx);
    at(&e, 4, cx);
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("a\tb\n1\t2\n".into()));
    cx.simulate_keystrokes(&format!("{}-v", primary()));
    assert_eq!(text(&e, cx), "text\n| a | b |\n| 1 | 2 |\n");
    // Undone in one step, and pasted again as plain text.
    cx.simulate_keystrokes(&format!("{}-z", primary()));
    assert_eq!(text(&e, cx), "text\n");
    cx.simulate_keystrokes(&format!("{}-shift-v", primary()));
    assert_eq!(text(&e, cx), "texta\tb\n1\t2\n\n");
}

#[gpui::test]
fn pasting_html(cx: &mut TestAppContext) {
    let (e, cx) = open_with(
        "x\n",
        || Some("<p>Some <b>bold</b> <a href=\"https://kalem.dev\">text</a></p>".into()),
        cx,
    );
    at(&e, 1, cx);
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("Some bold text".into()));
    cx.simulate_keystrokes(&format!("{}-v", primary()));
    assert_eq!(text(&e, cx), "xSome *bold* [[https://kalem.dev][text]]\n");
}

#[gpui::test]
fn source_view(cx: &mut TestAppContext) {
    let (e, cx) = open("* Head\nSome *bold* \\alpha\n", cx);
    at(&e, 7, cx);
    let rich = e.read_with(cx, |e, _| e.line_view(1).display());
    assert_eq!(rich, "Some bold α");
    cx.simulate_keystrokes(&format!("{}-/", primary()));
    let (head, body, bold) = e.read_with(cx, |e, _| {
        let v = e.line_view(1);
        let bold = v
            .runs
            .iter()
            .any(|r| r.style.bold && r.text.contains("bold"));
        (e.line_view(0), v.display(), bold)
    });
    assert_eq!(
        (head.display().as_str(), body.as_str()),
        ("* Head", "Some *bold* \\alpha")
    );
    assert!(head.heading == 1 && head.mono && bold);
    // Painted at one size, and edits go to the same document and history.
    let sizes = e.read_with(cx, |e, _| {
        let p = e.painted.borrow();
        (
            p.get(&0).map(|p| p.bounds.size.height),
            p.get(&1).map(|p| p.bounds.size.height),
        )
    });
    assert!(sizes.0.is_some() && sizes.0 == sizes.1, "{sizes:?}");
    cx.simulate_input("x");
    cx.simulate_keystrokes(&format!("{}-/", primary()));
    cx.simulate_keystrokes(&format!("{}-z", primary()));
    assert_eq!(text(&e, cx), "* Head\nSome *bold* \\alpha\n");
}

#[gpui::test]
fn split_view(cx: &mut TestAppContext) {
    let (e, cx) = open("* Head\nSome *bold* text\n", cx);
    at(&e, 0, cx);
    cx.simulate_keystrokes(&format!("{}-\\", primary()));
    cx.run_until_parked();
    let shown = |e: &Entity<Editor>, cx: &mut VisualTestContext| {
        e.read_with(cx, |e, _| {
            let mine = e.painted.borrow().get(&1).map(|p| p.view.display());
            let other = e
                .other
                .as_ref()
                .and_then(|o| o.painted.borrow().get(&1).map(|p| p.view.display()));
            (e.source, mine, other)
        })
    };
    // The rich view stays active, the source shows beside it.
    assert_eq!(
        shown(&e, cx),
        (
            false,
            Some("Some bold text".into()),
            Some("Some *bold* text".into())
        )
    );
    // Both follow edits.
    at(&e, 7, cx);
    cx.simulate_input("New ");
    cx.run_until_parked();
    let (_, mine, other) = shown(&e, cx);
    assert_eq!(other.as_deref(), Some("New Some *bold* text"));
    assert!(mine.is_some_and(|m| m.starts_with("New Some")));
    // A click in the source pane makes it the active one.
    let target = e.read_with(cx, |e, _| {
        let o = e.other.as_ref().expect("a split");
        let p = o.painted.borrow().get(&0).cloned().expect("painted");
        p.bounds.origin + gpui::point(gpui::px(1.), p.bounds.size.height / 2.)
    });
    cx.simulate_click(target, gpui::Modifiers::default());
    cx.run_until_parked();
    let (source, head) = e.read_with(cx, |e, _| (e.source, e.doc.selection.head));
    assert!(source);
    assert_eq!(head, 0);
    // Closing keeps the active view.
    cx.simulate_keystrokes(&format!("{}-\\", primary()));
    let (source, split) = e.read_with(cx, |e, _| (e.source, e.other.is_some()));
    assert!(source && !split);
}

#[gpui::test]
fn outline_sidebar(cx: &mut TestAppContext) {
    let (e, cx) = open("* A\na\n** A1\n* B\n* C\n", cx);
    cx.simulate_keystrokes(&format!("{}-shift-o", primary()));
    cx.run_until_parked();
    let none = gpui::Modifiers::default();
    // A click jumps to the heading.
    let c = cx.debug_bounds("outline-3").expect("the row of C");
    cx.simulate_click(c.center(), none);
    cx.run_until_parked();
    assert_eq!(e.read_with(cx, |e, _| e.doc.selection.head), 16);
    // Folding A in the tree hides A1.
    let fold = cx.debug_bounds("outline-fold-0").expect("A's arrow");
    cx.simulate_click(fold.center(), none);
    cx.run_until_parked();
    assert!(cx.debug_bounds("outline-1").is_none());
    assert_eq!(e.read_with(cx, |e, _| e.doc.selection.head), 16);
    // Dragging C onto the upper half of A moves it first.
    let from = cx.debug_bounds("outline-3").expect("C").center();
    let a = cx.debug_bounds("outline-0").expect("A");
    let to = gpui::point(a.center().x, a.origin.y + gpui::px(2.));
    let left = gpui::MouseButton::Left;
    cx.simulate_mouse_down(from, left, none);
    cx.simulate_mouse_move(gpui::point(from.x, from.y - gpui::px(6.)), left, none);
    cx.simulate_mouse_move(to, left, none);
    cx.simulate_mouse_up(to, left, none);
    cx.run_until_parked();
    assert_eq!(text(&e, cx), "* C\n* A\na\n** A1\n* B\n");
    // On the lower half of a folded heading: after its subtree, as a
    // sibling.
    let from = cx.debug_bounds("outline-0").expect("C").center();
    let a = cx.debug_bounds("outline-1").expect("A");
    let to = gpui::point(a.center().x, a.origin.y + a.size.height - gpui::px(2.));
    cx.simulate_mouse_down(from, left, none);
    cx.simulate_mouse_move(gpui::point(from.x, from.y + gpui::px(6.)), left, none);
    cx.simulate_mouse_move(to, left, none);
    cx.simulate_mouse_up(to, left, none);
    cx.run_until_parked();
    assert_eq!(text(&e, cx), "* A\na\n** A1\n* C\n* B\n");
    assert!(cx.debug_bounds("outline-1").is_none());
    // A folded heading moves folded.
    let from = cx.debug_bounds("outline-0").expect("A").center();
    let b = cx.debug_bounds("outline-3").expect("B");
    let to = gpui::point(b.center().x, b.origin.y + b.size.height - gpui::px(2.));
    cx.simulate_mouse_down(from, left, none);
    cx.simulate_mouse_move(gpui::point(from.x, from.y + gpui::px(6.)), left, none);
    cx.simulate_mouse_move(to, left, none);
    cx.simulate_mouse_up(to, left, none);
    cx.run_until_parked();
    assert_eq!(text(&e, cx), "* C\n* B\n* A\na\n** A1\n");
    assert!(cx.debug_bounds("outline-2").is_some() && cx.debug_bounds("outline-3").is_none());
}

#[gpui::test]
fn command_palette(cx: &mut TestAppContext) {
    let (e, cx) = open("* A\n", cx);
    at(&e, 2, cx);
    cx.simulate_keystrokes(&format!("{}-shift-p", primary()));
    cx.simulate_input("heading level");
    let first = e.read_with(cx, |e, _| {
        e.palette
            .as_ref()
            .and_then(|p| p.matches().first().map(|i| i.id.clone()))
    });
    assert_eq!(first.as_deref(), Some("org.headline.setLevel"));
    assert!(cx.debug_bounds("palette-0").is_some());
    // The command needs a level: the palette asks for it.
    cx.simulate_keystrokes("enter");
    let label = e.read_with(cx, |e, _| {
        e.palette
            .as_ref()
            .and_then(|p| p.arg.as_ref().map(|a| a.label.clone()))
    });
    assert_eq!(label.as_deref(), Some("Heading Level: level"));
    cx.simulate_input("3");
    cx.simulate_keystrokes("enter");
    assert_eq!(text(&e, cx), "*** A\n");
    // Escape closes it; typing goes to the document again.
    cx.simulate_keystrokes(&format!("{}-shift-p", primary()));
    cx.simulate_keystrokes("escape");
    cx.simulate_input("x");
    assert_eq!(text(&e, cx), "*** xA\n");
}

#[gpui::test]
fn export_dialog(cx: &mut TestAppContext) {
    let (e, cx) = open("* A\n", cx);
    cx.simulate_keystrokes("ctrl-alt-e");
    cx.run_until_parked();
    let ids = e.read_with(cx, |e, _| {
        e.palette
            .as_ref()
            .map(|p| p.matches().iter().map(|i| i.id.clone()).collect::<Vec<_>>())
            .unwrap_or_default()
    });
    for id in [
        "export.html",
        "export.gfm",
        "export.toggleBodyOnly",
        "export.toggleMath",
    ] {
        assert!(ids.iter().any(|i| i == id), "{id} in {ids:?}");
    }
    assert!(cx.debug_bounds("palette-0").is_some());
    cx.simulate_keystrokes("escape");
}

#[gpui::test]
fn find_and_replace(cx: &mut TestAppContext) {
    let (e, cx) = open("one two one\nthree one\n", cx);
    at(&e, 0, cx);
    cx.simulate_keystrokes(&format!("{}-f", primary()));
    cx.simulate_input("one");
    let state = |e: &Entity<Editor>, cx: &mut VisualTestContext| {
        e.read_with(cx, |e, _| {
            (
                e.doc.selection.anchor,
                e.highlights.len(),
                e.find.as_ref().map(|f| f.focused),
            )
        })
    };
    assert_eq!(state(&e, cx), (0, 3, Some(true)));
    assert!(cx.debug_bounds("find").is_some());
    cx.simulate_keystrokes("enter");
    assert_eq!(state(&e, cx).0, 8);
    cx.simulate_keystrokes("shift-enter");
    assert_eq!(state(&e, cx).0, 0);
    cx.simulate_keystrokes("escape");
    assert_eq!(state(&e, cx), (0, 0, None));
    // Find and replace: one match, then all.
    cx.simulate_keystrokes(&format!("{}-h", primary()));
    cx.simulate_keystrokes("tab");
    cx.simulate_input("1");
    cx.simulate_keystrokes("enter");
    assert_eq!(text(&e, cx), "1 two one\nthree one\n");
    cx.simulate_keystrokes("alt-enter");
    assert_eq!(text(&e, cx), "1 two 1\nthree 1\n");
    // A regular expression with a group.
    cx.simulate_keystrokes("tab backspace backspace backspace alt-r");
    cx.simulate_input(r"(\w+) (\d)");
    assert_eq!(state(&e, cx).1, 2);
    cx.simulate_keystrokes("tab backspace");
    cx.simulate_input("$2-$1");
    cx.simulate_keystrokes("alt-enter");
    assert_eq!(text(&e, cx), "1 1-two\n1-three\n");
    // Undone in one step.
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes(&format!("{}-z", primary()));
    assert_eq!(text(&e, cx), "1 two 1\nthree 1\n");
}

#[gpui::test]
fn word_counts(cx: &mut TestAppContext) {
    let (e, cx) = open("* One *two*\nthree four\n** Five\nsix\n", cx);
    at(&e, 30, cx);
    let counts = e.read_with(cx, |e, _| e.words.borrow_mut().get(&e.doc));
    assert_eq!(counts, Some((6, Some(2))));
    cx.simulate_input(" seven");
    // While typing goes on, the counts wait; after a pause they catch up.
    std::thread::sleep(std::time::Duration::from_millis(350));
    assert!(e.read_with(cx, |e, _| e.words.borrow().due(&e.doc)));
    let counts = e.read_with(cx, |e, _| e.words.borrow_mut().get(&e.doc));
    assert_eq!(counts, Some((7, Some(3))));
}

#[gpui::test]
fn word_targets_and_chapters(cx: &mut TestAppContext) {
    let (e, cx) = open("* One\nthree four five\n* Two\nsix\n", cx);
    at(&e, 8, cx);
    for (cmd, words) in [
        ("stats.setDocumentTarget", "1k"),
        ("stats.setSectionTarget", "10"),
    ] {
        e.update_in(cx, |e, window, cx| {
            e.run_command(cmd, serde_json::json!({ "words": words }), window, cx)
        });
        cx.run_until_parked();
    }
    let text = text_of(&e, cx);
    assert!(text.starts_with("#+KALEM: word_target=1000\n"), "{text}");
    assert!(text.contains(":WORD_TARGET: 10\n"), "{text}");
    // The counts catch up after a pause in typing.
    std::thread::sleep(std::time::Duration::from_millis(350));
    let targets = e.read_with(cx, |e, _| {
        let mut w = e.words.borrow_mut();
        w.get(&e.doc);
        w.targets()
    });
    assert_eq!((targets.document, targets.section), (Some(1000), Some(10)));
    // The chapters, with their words; choosing one goes there.
    e.update_in(cx, |e, window, cx| {
        e.run_command("stats.chapters", serde_json::Value::Null, window, cx)
    });
    cx.run_until_parked();
    let items = e.read_with(cx, |e, _| {
        e.palette
            .as_ref()
            .map(|p| {
                p.matches()
                    .iter()
                    .map(|i| (i.title.clone(), i.category.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    assert_eq!(
        items,
        [
            ("One".to_string(), "4 of 10 (40%)".to_string()),
            ("Two".to_string(), "2".to_string())
        ]
    );
    cx.simulate_input("Two");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let (head, text) = e.read_with(cx, |e, _| {
        (e.doc.selection.head, e.doc.text().as_str().to_string())
    });
    assert_eq!(head, text.find("* Two").unwrap());
}

#[gpui::test]
fn date_picker(cx: &mut TestAppContext) {
    let (e, cx) = open("* A\nx <2026-10-02 Fri 10:00> y\n", cx);
    // On a timestamp: the picker starts at its date and time.
    at(&e, 8, cx);
    cx.simulate_keystrokes("alt-shift-d");
    let chosen = e.read_with(cx, |e, _| e.date_picker.as_ref().map(|p| p.argument()));
    assert_eq!(chosen.as_deref(), Some("2026-10-02 10:00"));
    assert!(cx.debug_bounds("date-picker").is_some());
    cx.simulate_keystrokes("right down enter");
    assert_eq!(text(&e, cx), "* A\nx <2026-10-10 Sat 10:00> y\n");
    // A typed date.
    at(&e, 3, cx);
    cx.simulate_keystrokes("alt-shift-d");
    cx.simulate_input("2026-12-24");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        text(&e, cx),
        "* A<2026-12-24 Thu>\nx <2026-10-10 Sat 10:00> y\n"
    );
    // A clicked day, in the month of the typed date.
    at(&e, 0, cx);
    cx.simulate_keystrokes("alt-shift-d");
    cx.simulate_input("2026-12-01");
    let day = cx.debug_bounds("date-2026-12-25").expect("the 25th");
    cx.simulate_click(day.center(), gpui::Modifiers::default());
    assert!(text(&e, cx).starts_with("<2026-12-25 Fri>* A"));
    assert!(e.read_with(cx, |e, _| e.date_picker.is_none()));
}

#[gpui::test]
fn word_completion(cx: &mut TestAppContext) {
    let (e, cx) = open_named("quartz quantum\n", "w.txt", || None, cx);
    at(&e, 15, cx);
    cx.simulate_input("qua");
    let labels = e.read_with(cx, |e, _| {
        e.completion.as_ref().map(|m| {
            m.items()
                .iter()
                .map(|i| i.label.clone())
                .collect::<Vec<_>>()
        })
    });
    assert_eq!(
        labels,
        Some(vec!["quantum".to_string(), "quartz".to_string()])
    );
    cx.simulate_keystrokes("tab");
    assert_eq!(text(&e, cx), "quartz quantum\nquantum");
    cx.simulate_input(" qua");
    cx.simulate_keystrokes("enter");
    assert_eq!(text(&e, cx), "quartz quantum\nquantum qua\n");
}

#[gpui::test]
fn tag_completion(cx: &mut TestAppContext) {
    let (e, cx) = open("#+TAGS: work home\n* A\n", cx);
    at(&e, 21, cx);
    cx.simulate_input(" :h");
    let labels = e.read_with(cx, |e, _| {
        e.completion.as_ref().map(|m| {
            m.items()
                .iter()
                .map(|i| i.label.clone())
                .collect::<Vec<_>>()
        })
    });
    assert_eq!(labels, Some(vec!["home".to_string()]));
    cx.simulate_keystrokes("enter");
    let t = text(&e, cx);
    let line = t.lines().nth(1).unwrap();
    assert!(
        line.starts_with("* A ") && line.ends_with(" :home:"),
        "{line:?}"
    );
}

#[gpui::test]
fn settings_panel(cx: &mut TestAppContext) {
    let (e, cx) = open("* A\n", cx);
    cx.simulate_keystrokes(&format!("{}-,", primary()));
    assert!(cx.debug_bounds("settings").is_some());
    let none = gpui::Modifiers::default();
    for id in [
        "settings-theme-dark",
        "settings-size-up",
        "settings-keys-vim",
    ] {
        let b = cx.debug_bounds(id).expect(id);
        cx.simulate_click(b.center(), none);
        cx.run_until_parked();
    }
    let (dark, size, profile, path) = e.read_with(cx, |e, _| {
        (
            e.theme.dark,
            e.theme.size,
            e.shared.config.str("editor.keymap_profile").to_string(),
            e.shared.settings_path.clone().unwrap(),
        )
    });
    assert!(dark);
    assert_eq!((size, profile.as_str()), (17., "vim"));
    let saved = std::fs::read_to_string(path).unwrap();
    assert!(
        saved.contains("theme = \"dark\"") && saved.contains("font_size = 17"),
        "{saved}"
    );
    // Vim keys work now.
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes("A");
    cx.simulate_input("!");
    cx.simulate_keystrokes("escape");
    assert_eq!(text(&e, cx), "* A!\n");
    assert!(e.read_with(cx, |e, _| e.settings.is_none()));
}

#[gpui::test]
fn saving_and_outside_changes(cx: &mut TestAppContext) {
    let (e, cx) = open("* A\n", cx);
    let path = e.read_with(cx, |e, _| e.doc.meta.path.clone().unwrap());
    at(&e, 3, cx);
    cx.simulate_input("B");
    cx.simulate_keystrokes(&format!("{}-s", primary()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "* AB\n");
    assert!(!e.read_with(cx, |e, _| e.doc.is_modified()));
    // Another program changes the file: it is reloaded.
    std::fs::write(&path, "* Changed elsewhere\n").unwrap();
    e.update(cx, |e, cx| e.check_disk(cx));
    assert_eq!(text(&e, cx), "* Changed elsewhere\n");
    // With unsaved changes: a warning; Revert to Saved takes the file.
    cx.simulate_input("x");
    std::fs::write(&path, "* A third, longer version\n").unwrap();
    e.update(cx, |e, cx| e.check_disk(cx));
    let status = e.read_with(cx, |e, _| e.status.clone());
    assert!(status.is_some_and(|(m, error)| error && m.contains("changed on disk")));
    assert!(text(&e, cx).contains('x'));
    cx.dispatch_action(kalem_ui::editor::RunCommand::new("app.revert"));
    assert_eq!(text(&e, cx), "* A third, longer version\n");
    assert!(!e.read_with(cx, |e, _| e.doc.is_modified()));
}

#[gpui::test]
fn toolbar_menus_and_history(cx: &mut TestAppContext) {
    let (e, cx) = open("* A\nword\n", cx);
    at(&e, 2, cx);
    // The toolbar's TODO button, and a menu's command.
    let todo = cx.debug_bounds("tool-7").expect("the TODO button");
    cx.simulate_click(todo.center(), gpui::Modifiers::default());
    assert_eq!(text(&e, cx), "* TODO A\nword\n");
    cx.dispatch_action(kalem_ui::editor::RunCommand::with(
        "org.headline.setLevel",
        serde_json::json!({ "level": 2 }),
    ));
    assert_eq!(text(&e, cx), "** TODO A\nword\n");
    // Select all, copy, cut; undo and redo.
    let p = primary();
    cx.simulate_keystrokes(&format!("{p}-a {p}-c"));
    let copied = cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(copied.as_deref(), Some("** TODO A\nword\n"));
    cx.simulate_keystrokes(&format!("{p}-x"));
    assert_eq!(text(&e, cx), "");
    cx.simulate_keystrokes(&format!("{p}-z"));
    assert_eq!(text(&e, cx), "** TODO A\nword\n");
    cx.simulate_keystrokes(&format!("{p}-shift-z"));
    assert_eq!(text(&e, cx), "");
    cx.simulate_keystrokes(&format!("{p}-z {p}-z"));
    assert_eq!(text(&e, cx), "* TODO A\nword\n");
}

#[gpui::test]
fn focus_narrowing_and_line_width(cx: &mut TestAppContext) {
    let (e, cx) = open("intro\n* A\na\n* B\nb\n", cx);
    at(&e, 10, cx);
    let visible =
        |e: &Entity<Editor>, cx: &mut VisualTestContext| e.read_with(cx, |e, _| e.visible.clone());
    let all = visible(&e, cx);
    // Focus mode: the section holding the cursor.
    cx.simulate_keystrokes("f8");
    assert_eq!(visible(&e, cx), [1, 2]);
    at(&e, 14, cx);
    assert_eq!(visible(&e, cx), [3, 4, 5]);
    cx.simulate_keystrokes("f8");
    assert_eq!(visible(&e, cx), all);
    // Narrowing shows the narrowed part only.
    at(&e, 10, cx);
    cx.dispatch_action(kalem_ui::editor::RunCommand::new("view.narrowToSubtree"));
    assert_eq!(visible(&e, cx), [1, 2]);
    cx.dispatch_action(kalem_ui::editor::RunCommand::new("view.widen"));
    assert_eq!(visible(&e, cx), all);
    // The text column is about 80 characters wide.
    cx.run_until_parked();
    let width = e.read_with(cx, |e, _| {
        e.painted.borrow().get(&0).map(|p| p.bounds.size.width)
    });
    assert!(width.is_some_and(|w| w <= gpui::px(641.)), "{width:?}");
}

/// An editor with the Vim profile.
fn open_vim<'a>(
    text: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<Editor>, &'a mut VisualTestContext) {
    let (e, cx) = open(text, cx);
    e.update(cx, |e, _| {
        let mut shared = kalem_ui::shared(Config::from_layers(&[(
            kalem_core::settings::Layer::User,
            None,
            "editor.keymap_profile = \"vim\"\n",
        )]));
        shared.html_clipboard = || None;
        shared.settings_path = e.shared.settings_path.clone();
        shared.projects = std::cell::RefCell::new(kalem_core::projects::ProjectState::load(
            e.shared.projects.borrow().list.file.clone(),
        ));
        e.shared = Rc::new(shared);
        e.refresh_vim();
    });
    (e, cx)
}

#[gpui::test]
fn vim_keys(cx: &mut TestAppContext) {
    let (e, cx) = open_vim("one two\nthree\n", cx);
    at(&e, 0, cx);
    // Normal mode: keys are commands, typed text goes nowhere.
    cx.simulate_keystrokes("w d w");
    cx.simulate_input("zz");
    assert_eq!(text(&e, cx), "one \nthree\n");
    cx.simulate_keystrokes("u j d d");
    assert_eq!(text(&e, cx), "one two\n");
    // Insert mode takes text and the Word-like keys; Escape leaves it.
    cx.simulate_keystrokes("k i");
    cx.simulate_input("new ");
    cx.simulate_keystrokes("escape");
    assert_eq!(text(&e, cx), "new one two\n");
    let mode = e.read_with(cx, |e, _| e.vim.as_ref().map(|v| v.mode));
    assert_eq!(mode, Some(kalem_core::vim::Mode::Normal));
    // Visual mode selects; `:w` saves through the command registry.
    cx.simulate_keystrokes("0 v e");
    let sel = e.read_with(cx, |e, _| e.doc.selected_text().map(str::to_string));
    assert_eq!(sel.as_deref(), Some("new"));
    cx.simulate_keystrokes("escape : w enter");
    let (path, modified) = e.read_with(cx, |e, _| {
        (e.doc.meta.path.clone().unwrap(), e.doc.is_modified())
    });
    assert!(!modified);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "new one two\n");
}

#[gpui::test]
fn vim_block_selection(cx: &mut TestAppContext) {
    let (e, cx) = open_vim("abcd\nefgh\n", cx);
    at(&e, 1, cx);
    // Ctrl+V reaches Vim (it is not Paste there).
    cx.simulate_keystrokes("ctrl-v j l");
    let (mode, block) = e.read_with(cx, |e, _| {
        let v = e.vim.as_ref().unwrap();
        (v.mode, v.block_ranges(&e.doc))
    });
    assert_eq!(mode, kalem_core::vim::Mode::VisualBlock);
    assert_eq!(block, Some(vec![1..3, 6..8]));
    cx.simulate_keystrokes("shift-i");
    cx.simulate_input("-");
    cx.simulate_keystrokes("escape");
    assert_eq!(text(&e, cx), "a-bcd\ne-fgh\n");
}

#[gpui::test]
fn line_commands(cx: &mut TestAppContext) {
    let (e, cx) = open_named("pear\napple\nfig\n", "l.txt", || None, cx);
    at(&e, 0, cx);
    cx.simulate_keystrokes("alt-down");
    assert_eq!(text_of(&e, cx), "apple\npear\nfig\n");
    cx.simulate_keystrokes("ctrl-shift-d");
    assert_eq!(text_of(&e, cx), "apple\npear\npear\nfig\n");
    let sel = |e: &Entity<Editor>, cx: &mut VisualTestContext| {
        e.read_with(cx, |e, _| e.doc.selected_text().map(str::to_string))
    };
    cx.simulate_keystrokes("ctrl-alt-right");
    assert_eq!(sel(&e, cx).as_deref(), Some("pear"));
    // The line is the word: the paragraph next.
    cx.simulate_keystrokes("ctrl-alt-right");
    assert_eq!(sel(&e, cx).as_deref(), Some("apple\npear\npear\nfig"));
    cx.simulate_keystrokes("ctrl-alt-left");
    assert_eq!(sel(&e, cx).as_deref(), Some("pear"));
}

#[gpui::test]
fn editing_code(cx: &mut TestAppContext) {
    let (e, cx) = open_named("fn a() {}\n", "a.rs", || None, cx);
    at(&e, 8, cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(text_of(&e, cx), "fn a() {\n    \n}\n");
    cx.simulate_input("x");
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.toggleComment", serde_json::Value::Null, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(text_of(&e, cx), "fn a() {\n    // x\n}\n");
    at(&e, 8, cx);
    let pair = e.read_with(cx, |e, _| kalem_core::code::pair_at_cursor(&e.doc));
    assert_eq!(pair, Some((7, 18)));
}

#[gpui::test]
fn long_lines(cx: &mut TestAppContext) {
    let long = format!("{}\n", "abc ".repeat(100_000));
    let (e, cx) = open_named(&long, "long.txt", || None, cx);
    at(&e, 200_000, cx);
    let shown = e.read_with(cx, |e, _| e.line_view(0).display());
    assert!(
        shown.starts_with('…') && shown.ends_with('…'),
        "{}",
        shown.len()
    );
    assert!(shown.len() < 20_000);
}

#[gpui::test]
fn long_org_lines(cx: &mut TestAppContext) {
    let long = format!("* A\n{}\n", "abc ".repeat(100_000));
    let start = std::time::Instant::now();
    let (e, cx) = open(&long, cx);
    at(&e, 200_000, cx);
    let shown = e.read_with(cx, |e, _| e.line_view(1).display());
    assert!(shown.starts_with('…') && shown.len() < 20_000);
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}

#[gpui::test]
fn multiple_cursors(cx: &mut TestAppContext) {
    let (e, cx) = open("one\ntwo\nthree\n", cx);
    at(&e, 0, cx);
    e.update_in(cx, |e, window, cx| {
        e.run_command("cursor.addBelow", serde_json::Value::Null, window, cx);
        e.run_command("cursor.addBelow", serde_json::Value::Null, window, cx);
    });
    cx.simulate_input("- ");
    assert_eq!(text_of(&e, cx), "- one\n- two\n- three\n");
    cx.simulate_keystrokes("end");
    cx.simulate_input(";");
    assert_eq!(text_of(&e, cx), "- one;\n- two;\n- three;\n");
    cx.simulate_keystrokes("backspace");
    assert_eq!(text_of(&e, cx), "- one\n- two\n- three\n");
    // Copy takes each selection, one a line; paste puts one at each.
    cx.simulate_keystrokes("shift-home");
    let copied = e.update(cx, |e, _| e.doc.copy_text());
    assert_eq!(copied.as_deref(), Some("- one\n- two\n- three"));
    cx.simulate_keystrokes("escape");
    assert!(e.read_with(cx, |e, _| e.doc.extra.is_empty()));
}

#[gpui::test]
fn next_occurrence(cx: &mut TestAppContext) {
    let (e, cx) = open("cat dog cat\n", cx);
    at(&e, 1, cx);
    cx.simulate_keystrokes("ctrl-d ctrl-d");
    cx.simulate_input("cow");
    assert_eq!(text_of(&e, cx), "cow dog cow\n");
}

#[gpui::test]
fn legacy_encodings(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("kalem-ui-enc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("notlar.org");
    let turkish =
        "* Ağaçların gölgesinde çalışan işçiler\ngüneşin doğuşunu şarkılarla karşıladı.\n";
    let (bytes, _, _) = kalem_core::encoding_rs::WINDOWS_1254.encode(turkish);
    std::fs::write(&path, &bytes).unwrap();
    let mut shared = kalem_ui::shared(Config::default());
    shared.html_clipboard = || None;
    shared.settings_path = Some(dir.join("settings.toml"));
    shared.projects = std::cell::RefCell::new(kalem_core::projects::ProjectState::load(Some(
        dir.join("projects.toml"),
    )));
    let shared = Rc::new(shared);
    let mut editor = None;
    let (_ws, cx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
        window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
        editor = Some(e.clone());
        Workspace::new(e, window, cx)
    });
    cx.run_until_parked();
    let e = editor.unwrap();
    let (text, encoding, status) = e.read_with(cx, |e, _| {
        (
            e.doc.text().as_str().to_string(),
            e.doc.meta.encoding,
            e.status.clone(),
        )
    });
    assert_eq!(text, turkish);
    assert_eq!(encoding, kalem_core::encoding_rs::WINDOWS_1254);
    assert!(status.is_some_and(|(s, _)| s.contains("windows-1254")));
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "file.saveWithEncoding",
            serde_json::json!({"encoding": "UTF-16LE"}),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let saved = std::fs::read(&path).unwrap();
    assert_eq!(&saved[..2], &[0xFF, 0xFE]);
    let back = kalem_core::files::decode(Some(&path), saved).unwrap();
    assert_eq!(back.0, turkish);
}

#[gpui::test]
fn plain_text_view(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("kalem-ui-plain-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.rs");
    let long = "x".repeat(400);
    let text = format!("fn main() {{\n    let a = 1;\n}}\n{long}\n");
    std::fs::write(&path, &text).unwrap();
    let mut shared = kalem_ui::shared(Config::default());
    shared.html_clipboard = || None;
    shared.settings_path = Some(dir.join("settings.toml"));
    shared.projects = std::cell::RefCell::new(kalem_core::projects::ProjectState::load(Some(
        dir.join("projects.toml"),
    )));
    let shared = Rc::new(shared);
    let mut editor = None;
    let (_ws, cx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
        window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
        editor = Some(e.clone());
        Workspace::new(e, window, cx)
    });
    cx.run_until_parked();
    let e = editor.unwrap();
    // Monospace lines with numbers, highlighting and a four-space step.
    let (numbers, mono, colored, step) = e.read_with(cx, |e, _| {
        let p = e.plain.borrow();
        let (_, h, step) = p.as_ref().expect("plain text state");
        (
            e.line_numbers(),
            e.line_view(0).mono,
            h.as_ref().is_some_and(|h| !h.line(0).is_empty()),
            *step,
        )
    });
    assert!(numbers && mono && colored);
    assert_eq!(step, 4);
    // The long line wraps; Alt+Z makes it one row that scrolls sideways.
    let height = |e: &Entity<Editor>, cx: &mut VisualTestContext| {
        e.read_with(cx, |e, _| {
            e.painted.borrow().get(&3).map(|p| p.bounds.size.height)
        })
    };
    let wrapped = height(&e, cx).expect("painted");
    cx.simulate_keystrokes("alt-z");
    cx.run_until_parked();
    let one = height(&e, cx).expect("painted");
    assert!(one < wrapped, "{one:?} {wrapped:?}");
    at(&e, text.len() - 2, cx);
    cx.run_until_parked();
    assert!(e.read_with(cx, |e, _| e.hscroll) > gpui::px(0.));
    let _ = std::fs::remove_dir_all(dir);
}

/// Phase 1 exit criterion: the Org manual opens, is edited and saves
/// without a diff.
#[gpui::test]
fn org_manual_round_trip(cx: &mut TestAppContext) {
    let src = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/corpus/org-mode/org-manual.org"
    );
    let text = std::fs::read_to_string(src).unwrap();
    let (e, cx) = open(&text, cx);
    let path = e.read_with(cx, |e, _| e.doc.meta.path.clone().unwrap());
    let middle = text[..text.len() / 2].rfind("\n\n").unwrap() + 1;
    at(&e, middle, cx);
    cx.simulate_input("Kalem");
    cx.run_until_parked();
    for _ in 0..5 {
        cx.simulate_keystrokes("backspace");
    }
    cx.simulate_keystrokes(&format!("{}-s", primary()));
    let saved = std::fs::read_to_string(path).unwrap();
    let first = saved
        .bytes()
        .zip(text.bytes())
        .position(|(a, b)| a != b)
        .unwrap_or(saved.len().min(text.len()));
    assert!(
        saved == text,
        "differs at {first}: {:?} / {:?}",
        &saved[first.saturating_sub(40)..(first + 40).min(saved.len())],
        &text[first.saturating_sub(40)..(first + 40).min(text.len())]
    );
}

#[gpui::test]
fn table_formulas(cx: &mut TestAppContext) {
    let text = "| a | b | c |\n|---+---+---|\n| 2 | 3 |   |\n| 4 | 5 |   |\n#+TBLFM: $3=$1*$2\n";
    let (e, cx) = open(text, cx);
    at(&e, text.find("| 2").unwrap() + 2, cx);
    cx.simulate_keystrokes("f9");
    assert_eq!(
        crate::text(&e, cx),
        "| a | b |  c |\n|---+---+----|\n| 2 | 3 |  6 |\n| 4 | 5 | 20 |\n#+TBLFM: $3=$1*$2\n"
    );
    let now = crate::text(&e, cx);
    at(&e, now.find(" 6 |").unwrap() + 1, cx);
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let (status, refs) = e.read_with(cx, |e, _| (e.formula_status.clone(), e.formula_refs.len()));
    assert_eq!(status.as_deref(), Some("$3 = $1*$2"));
    assert_eq!(refs, 2);
    cx.simulate_keystrokes("f2");
    for _ in 0.."*$2".len() {
        cx.simulate_keystrokes("backspace");
    }
    cx.simulate_input("+$2");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        crate::text(&e, cx),
        "| a | b | c |\n|---+---+---|\n| 2 | 3 | 5 |\n| 4 | 5 | 9 |\n#+TBLFM: $3=$1+$2\n"
    );
}

#[gpui::test]
fn math_environments(cx: &mut TestAppContext) {
    let text = "* A\n\\begin{align}\na &= b \\\\\nc &= d\n\\end{align}\nafter\n";
    let (e, cx) = open(text, cx);
    at(&e, 0, cx);
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    // Away from the cursor the environment is one formula on its first line.
    let visible = e.read_with(cx, |e, _| e.visible.clone());
    assert_eq!(visible, vec![0, 1, 5, 6]);
    at(&e, text.find("c &=").unwrap(), cx);
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let visible = e.read_with(cx, |e, _| e.visible.clone());
    assert_eq!(visible, vec![0, 1, 2, 3, 4, 5, 6]);
    // With the preview off every line shows.
    at(&e, 0, cx);
    cx.update(|window, cx| {
        e.update(cx, |e, cx| {
            e.run_command("view.toggleMath", serde_json::Value::Null, window, cx)
        })
    });
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let visible = e.read_with(cx, |e, _| e.visible.clone());
    assert_eq!(visible, vec![0, 1, 2, 3, 4, 5, 6]);
    // A formula RaTeX cannot lay out is an error, not an image.
    let bad = e.read_with(cx, |e, cx| {
        let _ = cx;
        matches!(
            e.shared
                .math
                .get("$\\frac{a$", "", gpui::px(16.), 1., gpui::black()),
            kalem_ui::math::Formula::Error(_)
        )
    });
    assert!(bad);
}

/// A window on `file` in a temporary folder holding a project `proj`
/// (with `a.org`, `sub/b.org`) and a file outside it, `loose.org`.
fn open_project(
    vim: bool,
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    std::path::PathBuf,
    &mut VisualTestContext,
) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("kalem-ui-proj-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("proj/sub")).unwrap();
    let dir = kalem_core::projects::normal(&dir);
    std::fs::write(dir.join("proj/a.org"), "* A\nalpha\n").unwrap();
    std::fs::write(dir.join("proj/sub/b.org"), "* B\nbeta\nthe needle here\n").unwrap();
    std::fs::write(dir.join("loose.org"), "* Loose\n").unwrap();
    let config = if vim {
        "editor.keymap_profile = \"vim\"\n"
    } else {
        ""
    };
    let mut shared = kalem_ui::shared(Config::from_layers(&[(
        kalem_core::settings::Layer::User,
        None,
        config,
    )]));
    shared.html_clipboard = || None;
    shared.settings_path = Some(dir.join("settings.toml"));
    shared.projects = std::cell::RefCell::new(kalem_core::projects::ProjectState::load(Some(
        dir.join("projects.toml"),
    )));
    shared.projects.borrow_mut().add(&dir.join("proj")).unwrap();
    let shared = Rc::new(shared);
    let path = dir.join("proj/a.org");
    let (ws, vcx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
        window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
        Workspace::new(e, window, cx)
    });
    vcx.run_until_parked();
    (ws, dir, vcx)
}

fn active_title(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> String {
    ws.read_with(cx, |ws, cx| ws.editor.read(cx).title())
}

/// Waits for the active editor's picker to have every file.
fn settle_picker(ws: &Entity<Workspace>, cx: &mut VisualTestContext) {
    for _ in 0..500 {
        let e = ws.read_with(cx, |ws, _| ws.editor.clone());
        let busy = e.update(cx, |e, cx| {
            e.tick_palette(cx);
            e.palette.as_ref().is_some_and(|p| {
                p.pick.as_ref().is_some_and(|k| k.partial)
                    || p.search.as_ref().is_some_and(|s| s.busy())
            })
        });
        if !busy {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cx.run_until_parked();
}

#[gpui::test]
fn folder_tree(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open_project(false, cx);
    // The project's files are found in the background.
    for _ in 0..300 {
        cx.run_until_parked();
        ws.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        if cx.debug_bounds("tree-1").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // `sub/` then `a.org`; a click opens `sub`, another its file.
    let tree = cx.debug_bounds("tree-0").expect("the folder tree");
    cx.simulate_click(tree.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let file = cx.debug_bounds("tree-1").expect("sub's file");
    cx.simulate_click(file.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(active_title(&ws, cx), "b.org");
    let path = ws.read_with(cx, |ws, cx| ws.editor.read(cx).doc.meta.path.clone());
    assert_eq!(path.as_deref(), Some(dir.join("proj/sub/b.org").as_path()));
}

#[gpui::test]
fn open_documents_and_projects(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open_project(false, cx);
    let p = primary();
    // Files open in the same window, listed by project.
    ws.update_in(cx, |ws, window, cx| {
        ws.open(&dir.join("proj/sub/b.org"), None, window, cx);
        ws.open(&dir.join("loose.org"), None, window, cx);
        ws.open(&dir.join("proj/a.org"), None, window, cx);
    });
    cx.run_until_parked();
    let (count, entries) = ws.read_with(cx, |ws, cx| {
        let files = ws.open_files(cx);
        (
            ws.editors.len(),
            kalem_core::projects::entries(&files, &ws.shared.projects.borrow().list),
        )
    });
    assert_eq!(count, 3);
    assert_eq!(active_title(&ws, cx), "a.org");
    assert!(
        matches!(&entries[0], kalem_core::projects::Entry::Project { name, .. } if name == "proj")
    );
    assert_eq!(entries.len(), 4);
    // The list shows on the left; a click shows a document.
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let b = cx
        .debug_bounds("open-file-2")
        .expect("the loose file in the list");
    cx.simulate_click(b.center(), gpui::Modifiers::none());
    assert_eq!(active_title(&ws, cx), "loose.org");
    // Next and previous follow the list: the project's files, then the rest.
    cx.simulate_keystrokes(&format!("{p}-pagedown"));
    assert_eq!(active_title(&ws, cx), "a.org");
    cx.simulate_keystrokes(&format!("{p}-pagedown"));
    assert_eq!(active_title(&ws, cx), "b.org");
    cx.simulate_keystrokes(&format!("{p}-pageup {p}-pageup"));
    assert_eq!(active_title(&ws, cx), "loose.org");
    // Find File in Project: in a project file, its files; typed text
    // narrows them.
    cx.simulate_keystrokes(&format!("{p}-pageup"));
    assert_eq!(active_title(&ws, cx), "b.org");
    cx.simulate_keystrokes(&format!("{p}-p"));
    settle_picker(&ws, cx);
    cx.simulate_input("a.o");
    cx.simulate_keystrokes("enter");
    assert_eq!(active_title(&ws, cx), "a.org");
    // Switch Document.
    cx.simulate_keystrokes(&format!("{p}-alt-o"));
    cx.simulate_input("loose");
    cx.simulate_keystrokes("enter");
    assert_eq!(active_title(&ws, cx), "loose.org");
    // Outside a project, Find File in Project offers the projects first.
    cx.simulate_keystrokes(&format!("{p}-p"));
    let kind = ws.read_with(cx, |ws, cx| {
        ws.editor
            .read(cx)
            .palette
            .as_ref()
            .and_then(|p| p.pick.as_ref())
            .map(|k| k.kind)
    });
    assert_eq!(kind, Some(kalem_core::command::PickKind::Projects));
    cx.simulate_keystrokes("enter");
    settle_picker(&ws, cx);
    let kind = ws.read_with(cx, |ws, cx| {
        ws.editor
            .read(cx)
            .palette
            .as_ref()
            .and_then(|p| p.pick.as_ref())
            .map(|k| k.kind)
    });
    assert_eq!(kind, Some(kalem_core::command::PickKind::ProjectFiles));
    cx.simulate_keystrokes("escape");
    // Search in Project: the match opens at its line.
    cx.simulate_keystrokes(&format!("{p}-pagedown"));
    assert_eq!(active_title(&ws, cx), "a.org");
    cx.simulate_keystrokes(&format!("{p}-shift-f"));
    cx.simulate_input("needle");
    settle_picker(&ws, cx);
    let hits = ws.read_with(cx, |ws, cx| {
        ws.editor
            .read(cx)
            .palette
            .as_ref()
            .and_then(|p| p.search.as_ref())
            .map_or(0, |s| s.hits.len())
    });
    assert_eq!(hits, 1);
    cx.simulate_keystrokes("enter");
    assert_eq!(active_title(&ws, cx), "b.org");
    let (line, _) = ws.read_with(cx, |ws, cx| {
        let e = ws.editor.read(cx);
        e.doc.text().line_col(e.doc.selection.head)
    });
    assert_eq!(line, 2);
    // Recent files remember what was opened.
    let recent = ws.read_with(cx, |ws, _| ws.shared.projects.borrow().list.recent.len());
    assert_eq!(recent, 3);
    // Closing shows a neighbor; closing the last closes the window.
    cx.simulate_keystrokes(&format!("{p}-w"));
    let count = ws.read_with(cx, |ws, _| ws.editors.len());
    assert_eq!(count, 2);
    // The status bar names the project.
    ws.update_in(cx, |ws, window, cx| {
        ws.open(&dir.join("proj/a.org"), None, window, cx)
    });
    let modified = ws.read_with(cx, |ws, cx| ws.editor.read(cx).doc.is_modified());
    assert!(!modified);
}

#[gpui::test]
fn doom_leader_keys(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open_project(true, cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open(&dir.join("loose.org"), None, window, cx);
    });
    cx.run_until_parked();
    assert_eq!(active_title(&ws, cx), "loose.org");
    // SPC b p: the previous buffer; the status shows what follows SPC.
    cx.simulate_keystrokes("space");
    let hint = ws.read_with(cx, |ws, cx| {
        let e = ws.editor.read(cx);
        let seq = kalem_core::keys::KeySequence(e.pending.clone());
        e.shared
            .keymap
            .which_key(&e.shared.registry, &seq, &e.context())
    });
    assert!(
        hint.contains(&("p".to_string(), "+Project".to_string()))
            && hint.contains(&("f".to_string(), "+File".to_string())),
        "{hint:?}"
    );
    cx.simulate_keystrokes("b p");
    assert_eq!(active_title(&ws, cx), "a.org");
    // `:bn` and `gt` go on; `:e` opens a file.
    cx.simulate_keystrokes(": b n enter");
    assert_eq!(active_title(&ws, cx), "loose.org");
    cx.simulate_keystrokes("g t");
    assert_eq!(active_title(&ws, cx), "a.org");
    // SPC p f: the project's files; SPC , the open documents.
    cx.simulate_keystrokes("space p f");
    settle_picker(&ws, cx);
    cx.simulate_input("b.org");
    cx.simulate_keystrokes("enter");
    assert_eq!(active_title(&ws, cx), "b.org");
    cx.simulate_keystrokes("space ,");
    cx.simulate_input("loose");
    cx.simulate_keystrokes("enter");
    assert_eq!(active_title(&ws, cx), "loose.org");
    // Space still moves nothing in the text: the document is unchanged.
    let text = ws.read_with(cx, |ws, cx| {
        ws.editor.read(cx).doc.text().as_str().to_string()
    });
    assert_eq!(text, "* Loose\n");
}

#[gpui::test]
fn font_search_and_recent_colors(cx: &mut TestAppContext) {
    let text = "one two three\n";
    let (ws, cx) = {
        let dir = std::env::temp_dir().join(format!("kalem-ui-fonts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f.klm");
        std::fs::write(&path, text).unwrap();
        let _ = std::fs::remove_file(dir.join("settings.toml"));
        let mut shared = kalem_ui::shared(Config::default());
        shared.html_clipboard = || None;
        shared.settings_path = Some(dir.join("settings.toml"));
        shared.projects = std::cell::RefCell::new(kalem_core::projects::ProjectState::load(Some(
            dir.join("projects.toml"),
        )));
        let shared = Rc::new(shared);
        cx.add_window_view(|window, cx| {
            let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
            window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
            Workspace::new(e, window, cx)
        })
    };
    cx.run_until_parked();
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    at(&e, 4, cx);
    cx.simulate_keystrokes("shift-right shift-right shift-right");
    // Typing in the font menu searches the fonts (the test platform has
    // none to list); Backspace takes a letter back, Escape closes it.
    let b = cx.debug_bounds("tool-font").expect("the font menu");
    cx.simulate_click(b.center(), gpui::Modifiers::none());
    cx.simulate_input("geo");
    cx.simulate_keystrokes("backspace");
    let filter = ws.read_with(cx, |ws, _| ws.font_filter.clone());
    assert_eq!(filter, "ge");
    assert_eq!(text_of(&e, cx), text, "typing went to the menu");
    assert!(cx.debug_bounds("font-search").is_some());
    cx.simulate_keystrokes("escape");
    let open = ws.read_with(cx, |ws, _| ws.menu.is_some());
    assert!(!open);
    // A color used shows among the recent ones next time.
    let b = cx.debug_bounds("tool-color").expect("the color button");
    cx.simulate_click(b.center(), gpui::Modifiers::none());
    let s = cx.debug_bounds("swatch-7").expect("blue");
    cx.simulate_click(s.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let b = cx.debug_bounds("tool-color").expect("the color button");
    cx.simulate_click(b.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(cx.debug_bounds("recent-swatch-0").is_some());
    assert!(cx.debug_bounds("recent-swatch-1").is_none());
}

#[gpui::test]
fn word_formatting(cx: &mut TestAppContext) {
    let text = "one two three\n";
    let (ws, cx) = {
        let dir = std::env::temp_dir().join(format!("kalem-ui-fmt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f.klm");
        std::fs::write(&path, text).unwrap();
        let mut shared = kalem_ui::shared(Config::default());
        shared.html_clipboard = || None;
        shared.settings_path = Some(dir.join("settings.toml"));
        shared.projects = std::cell::RefCell::new(kalem_core::projects::ProjectState::load(Some(
            dir.join("projects.toml"),
        )));
        let shared = Rc::new(shared);
        cx.add_window_view(|window, cx| {
            let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
            window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
            Workspace::new(e, window, cx)
        })
    };
    cx.run_until_parked();
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    let p = primary();
    // "two" selected, 14 points from the toolbar's menu.
    at(&e, 4, cx);
    cx.simulate_keystrokes("shift-right shift-right shift-right");
    let b = cx.debug_bounds("tool-size").expect("the size menu");
    cx.simulate_click(b.center(), gpui::Modifiers::none());
    let s = cx.debug_bounds("size-14").expect("the sizes");
    cx.simulate_click(s.center(), gpui::Modifiers::none());
    assert_eq!(
        text_of(&e, cx),
        "one @@kalem:size=14@@two@@kalem:end@@ three\n"
    );
    // The markers do not show; the selection is still the word.
    let shown = e.update(cx, |e, _| e.line_view(0).display());
    assert_eq!(shown, "one two three");
    let sel = e.read_with(cx, |e, _| e.doc.selected_text().map(str::to_string));
    assert_eq!(sel.as_deref(), Some("two"));
    // A color from the swatches, then a size step up with Ctrl+].
    let b = cx.debug_bounds("tool-color").expect("the color button");
    cx.simulate_click(b.center(), gpui::Modifiers::none());
    let s = cx.debug_bounds("swatch-2").expect("red");
    cx.simulate_click(s.center(), gpui::Modifiers::none());
    // 14 up is 16, the document's size, so the size goes.
    cx.simulate_keystrokes(&format!("{p}-]"));
    assert_eq!(
        text_of(&e, cx),
        "one @@kalem:color=#c00000@@two@@kalem:end@@ three\n"
    );
    cx.simulate_keystrokes(&format!("{p}-]"));
    assert_eq!(
        text_of(&e, cx),
        "one @@kalem:size=18 color=#c00000@@two@@kalem:end@@ three\n"
    );
    let f = e.read_with(cx, |e, _| e.format_at_cursor());
    assert_eq!(f.size, Some(180));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    // Deleting the last letter keeps the span's end; deleting all of it
    // takes the span.
    let end = text_of(&e, cx).find("@@kalem:end").unwrap();
    at(&e, end, cx);
    cx.simulate_keystrokes("backspace");
    assert_eq!(
        text_of(&e, cx),
        "one @@kalem:size=18 color=#c00000@@tw@@kalem:end@@ three\n"
    );
    cx.simulate_keystrokes("backspace backspace");
    assert_eq!(text_of(&e, cx), "one  three\n");
    cx.simulate_keystrokes(&format!("{p}-z {p}-z {p}-z"));
    // Right alignment: an attribute line, hidden in the rich view.
    at(&e, 0, cx);
    cx.simulate_keystrokes(&format!("{p}-r"));
    let t = text_of(&e, cx);
    assert!(t.starts_with("#+ATTR_KALEM: :align right\none "), "{t}");
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let visible = e.read_with(cx, |e, _| e.visible.clone());
    assert_eq!(visible[0], 1);
    cx.simulate_keystrokes(&format!("{p}-l"));
    assert!(text_of(&e, cx).starts_with("one "));
    // Clearing takes the formatting away.
    cx.simulate_keystrokes(&format!("{p}-a {p}-space"));
    assert_eq!(text_of(&e, cx), "one two three\n");
    // The document's line spacing and font: a `#+KALEM:` line.
    at(&e, 0, cx);
    let b = cx.debug_bounds("tool-spacing").expect("the spacing menu");
    cx.simulate_click(b.center(), gpui::Modifiers::none());
    let s = cx.debug_bounds("spacing-15").expect("1.5");
    cx.simulate_click(s.center(), gpui::Modifiers::none());
    cx.update(|window, cx| {
        e.update(cx, |e, cx| {
            e.run_command(
                "format.documentFont",
                serde_json::json!({ "family": "Georgia" }),
                window,
                cx,
            )
        })
    });
    assert_eq!(
        text_of(&e, cx),
        "#+KALEM: font=\"Georgia\" spacing=1.5\none two three\n"
    );
    let (spacing, font) = e.read_with(cx, |e, _| (e.doc_defaults().spacing, e.doc_theme().font));
    assert_eq!((spacing, font.as_str()), (Some(15), "Georgia"));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

fn text_of(e: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    e.read_with(cx, |e, _| e.doc.text().as_str().to_string())
}

/// The active document's line at the cursor.
fn cursor_line(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> String {
    ws.read_with(cx, |ws, cx| {
        let e = ws.editor.read(cx);
        let t = e.doc.text();
        t.as_str()[t.line_range(t.line_of(e.doc.selection.head))].to_string()
    })
}

/// Lets the file operations finish.
fn settle_jobs(ws: &Entity<Workspace>, cx: &mut VisualTestContext) {
    for _ in 0..500 {
        let e = ws.read_with(cx, |ws, _| ws.editor.clone());
        e.update(cx, |e, cx| e.tick(cx));
        cx.run_until_parked();
        if ws.read_with(cx, |ws, _| ws.shared.jobs.borrow().is_empty()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cx.run_until_parked();
}

#[gpui::test]
fn file_manager_and_projects_view(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open_project(false, cx);
    let p = primary();
    // Ctrl+Alt+D: the document's folder, the cursor on its file.
    cx.simulate_keystrokes(&format!("{p}-alt-d"));
    assert_eq!(active_title(&ws, cx), "proj/");
    assert!(
        cursor_line(&ws, cx).ends_with(" a.org"),
        "{}",
        cursor_line(&ws, cx)
    );
    // `p` goes up a line, Enter lists the folder, Backspace goes back.
    cx.simulate_keystrokes("p enter");
    assert_eq!(active_title(&ws, cx), "sub/");
    cx.simulate_keystrokes("backspace");
    assert_eq!(active_title(&ws, cx), "proj/");
    assert!(cursor_line(&ws, cx).ends_with(" sub/"));
    // Typing changes nothing.
    let before = ws.read_with(cx, |ws, cx| {
        ws.editor.read(cx).doc.text().as_str().to_string()
    });
    cx.simulate_input("zz");
    let after = ws.read_with(cx, |ws, cx| {
        ws.editor.read(cx).doc.text().as_str().to_string()
    });
    assert_eq!(before, after);
    // `P`: the projects, as if in one folder; a project opens as a folder,
    // and going up from it shows the projects again.
    cx.simulate_keystrokes("shift-p");
    assert!(
        cursor_line(&ws, cx).starts_with("  proj"),
        "{}",
        cursor_line(&ws, cx)
    );
    cx.simulate_keystrokes("enter");
    assert_eq!(active_title(&ws, cx), "proj/");
    cx.simulate_keystrokes("backspace");
    assert!(cursor_line(&ws, cx).starts_with("  proj"));
    cx.simulate_keystrokes("enter");
    // Only one file manager: the menu's File Manager comes back to it.
    let count = ws.read_with(cx, |ws, _| ws.editors.len());
    assert_eq!(count, 2);
    // Copy a.org into sub twice: the second time a dialog asks.
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    let at = e.read_with(cx, |e, _| e.doc.text().as_str().find(" a.org").unwrap() + 1);
    at_pos(&e, at, cx);
    let target = dir.join("proj/sub").display().to_string();
    for round in 0..2 {
        e.update_in(cx, |e, window, cx| {
            e.run_command(
                "dired.copy",
                serde_json::json!({ "target": target }),
                window,
                cx,
            )
        });
        cx.run_until_parked();
        if round == 1 {
            assert!(cx.has_pending_prompt());
            cx.simulate_prompt_answer("Keep Both");
            cx.run_until_parked();
        }
        settle_jobs(&ws, cx);
    }
    assert!(dir.join("proj/sub/a.org").is_file() && dir.join("proj/sub/a (2).org").is_file());
    // `c` makes a file, or a folder when the name ends with a slash; F2
    // renames; Delete asks, then moves to the trash (not run here).
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "dired.newFile",
            serde_json::json!({ "name": "made/" }),
            window,
            cx,
        );
        e.run_command(
            "dired.move",
            serde_json::json!({ "target": "renamed" }),
            window,
            cx,
        );
    });
    settle_jobs(&ws, cx);
    assert!(
        dir.join("proj/renamed").is_dir(),
        "{:?}",
        std::fs::read_dir(dir.join("proj"))
            .unwrap()
            .collect::<Vec<_>>()
    );
    assert!(
        cursor_line(&ws, cx).ends_with("renamed/")
            || ws.read_with(cx, |ws, cx| ws
                .editor
                .read(cx)
                .doc
                .text()
                .as_str()
                .contains("renamed/"))
    );
    // Ctrl+Z takes the rename back, and redoes nothing more.
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert!(dir.join("proj/made").is_dir() && !dir.join("proj/renamed").exists());
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "dired.move",
            serde_json::json!({ "target": "renamed" }),
            window,
            cx,
        );
    });
    settle_jobs(&ws, cx);
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "dired.deletePermanently",
            serde_json::Value::Null,
            window,
            cx,
        )
    });
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("No");
    cx.run_until_parked();
    assert!(dir.join("proj/renamed").is_dir());
}

fn at_pos(e: &Entity<Editor>, pos: usize, cx: &mut VisualTestContext) {
    e.update(cx, |e, cx| {
        e.doc.move_cursor(pos, false);
        e.after_change(cx);
    });
    cx.run_until_parked();
}

#[gpui::test]
fn enter_below_a_table(cx: &mut TestAppContext) {
    let (e, cx) = open("| a | b |\n", cx);
    at(&e, 10, cx);
    cx.simulate_keystrokes("enter");
    cx.simulate_input("x");
    assert_eq!(text(&e, cx), "| a | b |\n\nx");
}

#[gpui::test]
fn file_manager_from_the_toolbar_and_the_list(cx: &mut TestAppContext) {
    let (ws, _dir, cx) = open_project(false, cx);
    fn click(name: &'static str, cx: &mut VisualTestContext) {
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let b = cx.debug_bounds(name).unwrap_or_else(|| panic!("{name}"));
        cx.simulate_click(b.center(), gpui::Modifiers::none());
        cx.run_until_parked();
    }
    // The toolbar's File Manager, and again to come back.
    click("tool-files", cx);
    assert_eq!(active_title(&ws, cx), "proj/");
    click("tool-files", cx);
    assert_eq!(active_title(&ws, cx), "a.org");
    // The list of open files: the projects.
    click("files-projects", cx);
    let path = ws.read_with(cx, |ws, cx| ws.editor.read(cx).doc.meta.path.clone());
    assert_eq!(path, None);
    // The key, back to the document.
    cx.simulate_keystrokes(&format!("{}-alt-d", primary()));
    assert_eq!(active_title(&ws, cx), "a.org");
}

#[gpui::test]
fn citations(cx: &mut TestAppContext) {
    let text = "#+bibliography: refs.bib\n\nAs [cite:@knuth84] said.\n";
    let (e, cx) = open(text, cx);
    let dir = e.read_with(cx, |e, _| {
        e.doc
            .meta
            .path
            .clone()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    });
    std::fs::write(
        dir.join("refs.bib"),
        "@book{knuth84, author = {Donald E. Knuth}, title = {The {\\TeX}book}, year = 1984}\n\
         @article{doe20, author = {Jane Doe}, title = {A study}, journal = {Journal}, year = 2020}\n",
    )
    .unwrap();
    // The entry cited under the cursor, in the status bar.
    let knuth = text.find("knuth84").unwrap();
    at(&e, knuth, cx);
    cx.run_until_parked();
    let status = e.read_with(cx, |e, _| e.formula_status.clone());
    assert_eq!(
        status.as_deref(),
        Some("@knuth84: Donald E. Knuth (1984). The \\TeXbook.")
    );
    // The picker: typing finds an entry, Enter cites it.
    at(&e, text.find(" said").unwrap(), cx);
    e.update_in(cx, |e, window, cx| {
        e.run_command("org.cite.insert", serde_json::Value::Null, window, cx)
    });
    cx.run_until_parked();
    cx.simulate_input("doe");
    let first = e.read_with(cx, |e, _| {
        e.palette
            .as_ref()
            .and_then(|p| p.matches().first().map(|i| i.title.clone()))
    });
    assert_eq!(
        first.as_deref(),
        Some("@doe20  Jane Doe (2020). A study. Journal.")
    );
    cx.simulate_keystrokes("enter");
    assert_eq!(
        text_of(&e, cx),
        "#+bibliography: refs.bib\n\nAs [cite:@knuth84][cite:@doe20] said.\n"
    );
    // The mouse over a citation shows the entry it cites.
    cx.run_until_parked();
    let line = e.read_with(cx, |e, _| e.painted.borrow().get(&2).map(|p| p.bounds));
    let line = line.expect("line 2 painted");
    let mut shown = None;
    let mut x = line.origin.x + gpui::px(1.);
    while x < line.origin.x + line.size.width && shown.is_none() {
        let at = gpui::point(x, line.origin.y + line.size.height / 2.);
        cx.simulate_mouse_move(at, None, gpui::Modifiers::default());
        shown = e.read_with(cx, |e, _| e.cite_hover.as_ref().map(|h| h.1.clone()));
        x += gpui::px(3.);
    }
    assert_eq!(
        shown.as_deref(),
        Some("@knuth84: Donald E. Knuth (1984). The \\TeXbook.")
    );
}

#[gpui::test]
fn pictures(cx: &mut TestAppContext) {
    let text =
        "Before.\n[[file:pic.png]]\n#+ATTR_ORG: :width 50%\n[[file:pic.png]]\n[[file:none.png]]\n";
    let (e, cx) = open(text, cx);
    let dir = e.read_with(cx, |e, _| {
        e.doc
            .meta
            .path
            .clone()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    });
    image::RgbaImage::from_pixel(80, 40, image::Rgba([200, 0, 0, 255]))
        .save(dir.join("pic.png"))
        .unwrap();
    at(&e, 0, cx);
    e.update(cx, |e, cx| {
        e.painted.borrow_mut().clear();
        cx.notify();
    });
    cx.run_until_parked();
    let widget = |line: usize, e: &Entity<Editor>, cx: &mut VisualTestContext| {
        e.read_with(cx, |e, _| {
            let p = e.painted.borrow();
            let p = p.get(&line)?;
            p.widgets.first().map(|w| (w.0.size, p.bounds.size.width))
        })
    };
    // The picture at its size.
    let (sz, _) = widget(1, &e, cx).expect("a picture on line 2");
    assert_eq!((sz.width, sz.height), (gpui::px(80.), gpui::px(40.)));
    // Half the text width, its shape kept.
    let (sz, line) = widget(3, &e, cx).expect("a picture on line 4");
    assert!(
        (sz.width - line * 0.5).abs() < gpui::px(1.),
        "{sz:?} of {line:?}"
    );
    assert!((sz.height - sz.width * 0.5).abs() < gpui::px(1.));
    // A missing file stays a name.
    let (sz, _) = widget(4, &e, cx).expect("a name on line 5");
    assert!(sz.height < gpui::px(40.));
    // A dropped picture from elsewhere is copied beside the document and
    // linked where it falls.
    let other = std::env::temp_dir().join(format!("kalem-ui-drop-{}", std::process::id()));
    std::fs::create_dir_all(&other).unwrap();
    let dropped = other.join("dropped.png");
    std::fs::copy(dir.join("pic.png"), &dropped).unwrap();
    at(&e, 7, cx);
    e.update_in(cx, |e, window, cx| {
        e.drop_paths(std::slice::from_ref(&dropped), window, cx)
    });
    assert!(
        text_of(&e, cx).contains("[[file:t_assets/dropped.png]]"),
        "{}",
        text_of(&e, cx)
    );
    assert!(dir.join("t_assets/dropped.png").is_file());
}

#[gpui::test]
fn footnotes(cx: &mut TestAppContext) {
    let (e, cx) = open("Some text here.\n", cx);
    at(&e, 9, cx);
    cx.simulate_keystrokes("ctrl-alt-f");
    cx.run_until_parked();
    assert_eq!(
        text_of(&e, cx),
        "Some text[fn:1] here.\n\n* Footnotes\n\n[fn:1] \n"
    );
    cx.simulate_input("The note.");
    // The text of the footnote at the cursor, in the status bar.
    at(&e, 11, cx);
    let status = e.read_with(cx, |e, _| e.formula_status.clone());
    assert_eq!(status.as_deref(), Some("Footnote 1: The note."));
}

#[gpui::test]
fn scheduling(cx: &mut TestAppContext) {
    let (e, cx) = open("* TODO Task\nBody\n", cx);
    at(&e, 3, cx);
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "org.schedule",
            serde_json::json!({"date": "2026-10-05"}),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    assert_eq!(
        text_of(&e, cx),
        "* TODO Task\nSCHEDULED: <2026-10-05 Mon>\nBody\n"
    );
    e.update_in(cx, |e, window, cx| {
        e.run_command("org.schedule.remove", serde_json::Value::Null, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(text_of(&e, cx), "* TODO Task\nBody\n");
}

#[gpui::test]
fn editing_properties(cx: &mut TestAppContext) {
    let (e, cx) = open("* A\n:PROPERTIES:\n:ID: 42\n:END:\n", cx);
    at(&e, 2, cx);
    e.update_in(cx, |e, window, cx| {
        e.run_command("org.property.edit", serde_json::Value::Null, window, cx)
    });
    cx.run_until_parked();
    let titles = e.read_with(cx, |e, _| {
        e.palette
            .as_ref()
            .map(|p| {
                p.matches()
                    .iter()
                    .map(|i| i.title.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    assert!(titles.iter().any(|t| t == "ID: 42"), "{titles:?}");
    // Choosing it asks for the value, starting with the old one.
    cx.simulate_input("ID: 42");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let input = e.read_with(cx, |e, _| e.palette.as_ref().map(|p| p.input.clone()));
    assert_eq!(input.as_deref(), Some("42"));
    cx.simulate_keystrokes("backspace backspace");
    cx.simulate_input("7");
    cx.simulate_keystrokes("enter");
    assert_eq!(text_of(&e, cx), "* A\n:PROPERTIES:\n:ID:       7\n:END:\n");
}

#[gpui::test]
fn copying_as_html(cx: &mut TestAppContext) {
    let (e, cx) = open("Some *bold* text.\n", cx);
    e.update(cx, |e, cx| {
        e.doc.move_cursor(0, false);
        e.doc.move_cursor(11, true);
        e.after_change(cx);
    });
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.copyHtml", serde_json::Value::Null, window, cx)
    });
    cx.run_until_parked();
    let copied = cx
        .read_from_clipboard()
        .and_then(|c| c.text())
        .unwrap_or_default();
    assert!(copied.contains("<b>bold</b>"), "{copied}");
    // Without an HTML clipboard here, rich text is copied as plain text.
    if !cfg!(target_os = "macos") {
        e.update_in(cx, |e, window, cx| {
            e.run_command("edit.copyRichText", serde_json::Value::Null, window, cx)
        });
        cx.run_until_parked();
        let copied = cx.read_from_clipboard().and_then(|c| c.text());
        assert_eq!(copied.as_deref(), Some("Some *bold*"));
    }
}

#[gpui::test]
fn archiving_and_refiling(cx: &mut TestAppContext) {
    let (e, cx) = open("* A\n** a1\n* B\nb\n* C\n", cx);
    at(&e, 11, cx);
    e.update_in(cx, |e, window, cx| {
        e.run_command("org.refile", serde_json::Value::Null, window, cx)
    });
    cx.run_until_parked();
    cx.simulate_input("A/a1");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(text_of(&e, cx), "* A\n** a1\n*** B\nb\n* C\n");
    at(&e, 16, cx);
    e.update_in(cx, |e, window, cx| {
        e.run_command("org.archive.sibling", serde_json::Value::Null, window, cx)
    });
    cx.run_until_parked();
    let text = text_of(&e, cx);
    assert!(
        text.contains("*** Archive") && text.contains("**** B\n:PROPERTIES:\n:ARCHIVE_TIME:"),
        "{text}"
    );
}

#[gpui::test]
fn macros_and_snippets(cx: &mut TestAppContext) {
    let text = "#+MACRO: v version $1\nThis is {{{v(2)}}} @@html:<br>@@ ok.\nend\n";
    let (e, cx) = open(text, cx);
    at(&e, text.len() - 1, cx);
    let shown = e.read_with(cx, |e, _| {
        e.painted.borrow().get(&1).map(|p| p.view.display())
    });
    assert_eq!(shown.as_deref(), Some("This is version 2 html:<br> ok."));
}

#[gpui::test]
fn captions_names_and_references(cx: &mut TestAppContext) {
    let (e, cx) = open("#+CAPTION: Old\n| 1 |\n\nSee \n", cx);
    at(&e, 16, cx);
    e.update_in(cx, |e, window, cx| {
        e.run_command("org.caption.set", serde_json::Value::Null, window, cx)
    });
    cx.run_until_parked();
    let input = e.read_with(cx, |e, _| e.palette.as_ref().map(|p| p.input.clone()));
    assert_eq!(input.as_deref(), Some("Old"));
    cx.simulate_keystrokes("backspace backspace backspace");
    cx.simulate_input("Numbers");
    cx.simulate_keystrokes("enter");
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "org.name.set",
            serde_json::json!({"name": "tab:n"}),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let text = "#+NAME: tab:n\n#+CAPTION: Numbers\n| 1 |\n\nSee \n";
    assert_eq!(text_of(&e, cx), text);
    at(&e, text.len() - 1, cx);
    e.update_in(cx, |e, window, cx| {
        e.run_command("org.insert.reference", serde_json::Value::Null, window, cx)
    });
    cx.run_until_parked();
    cx.simulate_input("tab:n");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(
        text_of(&e, cx).ends_with("See [[tab:n]]\n"),
        "{}",
        text_of(&e, cx)
    );
}

#[gpui::test]
fn inserting_drawers(cx: &mut TestAppContext) {
    let (e, cx) = open("One.\nTwo.\n", cx);
    e.update(cx, |e, cx| {
        e.doc.move_cursor(0, false);
        e.doc.move_cursor(9, true);
        e.after_change(cx);
    });
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "org.insert.drawer",
            serde_json::json!({"name": "LOGBOOK"}),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    assert_eq!(text_of(&e, cx), ":LOGBOOK:\nOne.\nTwo.\n:END:\n");
}

#[gpui::test]
fn csv_grid(cx: &mut TestAppContext) {
    let (e, cx) = open_named("name,age\nAda,36\nBob,7\n", "p.csv", || None, cx);
    let shown = e.read_with(cx, |e, _| e.line_view(1).display());
    assert!(shown.contains("Ada  │ 36"), "{shown}");
    at(&e, 9, cx);
    cx.simulate_keystrokes("tab");
    assert_eq!(e.read_with(cx, |e, _| e.doc.selection.head), 13);
    e.update_in(cx, |e, window, cx| {
        e.run_command("csv.moveRowDown", serde_json::Value::Null, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(text_of(&e, cx), "name,age\nBob,7\nAda,36\n");
}

#[gpui::test]
fn print_compiles_first(cx: &mut TestAppContext) {
    // Without a file there is nothing to compile beside (a test must not
    // reach a real print dialog; `kalem-core/tests/book.rs` covers the rest).
    let (e, cx) = open_named("Hello.\n", "p.org", || None, cx);
    e.update_in(cx, |e, window, cx| {
        e.doc.meta.path = None;
        e.run_command("file.print", serde_json::Value::Null, window, cx)
    });
    let status = e.read_with(cx, |e, _| e.status.clone().map(|s| s.0).unwrap_or_default());
    assert!(status.contains("Save"), "{status}");
}

#[gpui::test]
fn latex_rendered(cx: &mut TestAppContext) {
    let text = "\\section{Intro}\nSome \\emph{very} ``good'' text---yes.\n\\begin{itemize}\n\\item First\n\\end{itemize}\n";
    let (e, cx) = open_named(text, "paper.tex", || None, cx);
    // Away from the heading's command.
    at(&e, text.len(), cx);
    let (heading, body) = e.read_with(cx, |e, _| {
        let h = e.line_view(0);
        ((h.display(), h.heading), e.line_view(1).display())
    });
    assert_eq!(heading, ("1\u{2003}Intro".to_string(), 1));
    assert_eq!(body, "Some very \u{201c}good\u{201d} text\u{2014}yes.");
    assert_eq!(
        e.read_with(cx, |e, _| e.line_view(3).display()),
        "\u{2022} First"
    );
    // At the command, its markers show for editing.
    at(&e, text.find("\\emph").unwrap(), cx);
    let body = e.read_with(cx, |e, _| e.line_view(1).display());
    assert!(body.contains("\\emph{very}"), "{body}");
}

#[gpui::test]
fn latex_math(cx: &mut TestAppContext) {
    let text = "Inline $a^2$.\n\\begin{equation}\\label{e}\n  E = mc^2\n\\end{equation}\nAfter.\n";
    let (e, cx) = open_named(text, "m.tex", || None, cx);
    at(&e, text.len(), cx);
    // Away from the cursor, the equation shows as one formula on its first
    // line; the others are hidden.
    assert_eq!(e.read_with(cx, |e, _| e.visible.clone()), vec![0, 1, 4, 5]);
    let inline = e.read_with(cx, |e, _| e.line_view(0));
    assert!(
        inline
            .runs
            .iter()
            .any(|r| matches!(r.widget, Some(kalem_core::view::Widget::Math { .. })))
    );
    // In it, the source.
    at(&e, text.find("mc^2").unwrap(), cx);
    assert_eq!(
        e.read_with(cx, |e, _| e.visible.clone()),
        vec![0, 1, 2, 3, 4, 5]
    );
}

#[gpui::test]
fn latex_floats(cx: &mut TestAppContext) {
    let text = "\\begin{figure}\n\\centering\n\\includegraphics[width=\\linewidth]{fig}\n\\caption{Cats.}\n\\end{figure}\n";
    let (e, cx) = open_named(text, "f.tex", || None, cx);
    let dir = e.read_with(cx, |e, _| {
        e.doc
            .meta
            .path
            .clone()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    });
    std::fs::write(dir.join("fig.png"), b"not really").unwrap();
    at(&e, text.len(), cx);
    let pic = e.read_with(cx, |e, _| e.line_view(2));
    assert!(matches!(
        &pic.runs[0].widget,
        Some(kalem_core::view::Widget::Image { path, .. }) if path == "fig.png"
    ));
    assert_eq!(
        e.read_with(cx, |e, _| e.line_view(3).display()),
        "Figure 1: Cats."
    );
}

#[gpui::test]
fn latex_references(cx: &mut TestAppContext) {
    let text = "\\section{One}\\label{s}\nSee \\ref{s} and \\cite{knuth}.\n\\bibliography{refs}\n";
    let (e, cx) = open_named(text, "r.tex", || None, cx);
    let dir = e.read_with(cx, |e, _| {
        e.doc
            .meta
            .path
            .clone()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    });
    std::fs::write(
        dir.join("refs.bib"),
        "@book{knuth, author = {Knuth, Donald}, title = {The TeXbook}, year = 1984}\n",
    )
    .unwrap();
    at(&e, text.len(), cx);
    assert_eq!(
        e.read_with(cx, |e, _| e.line_view(1).display()),
        "See 1 and [Knuth 1984]."
    );
    at(&e, text.find("\\cite").unwrap() + 3, cx);
    let status = e
        .read_with(cx, |e, _| e.formula_status.clone())
        .unwrap_or_default();
    assert!(status.contains("@knuth: Knuth, Donald. 1984."), "{status}");
}

#[gpui::test]
fn latex_theorems_and_code(cx: &mut TestAppContext) {
    let text = "\\newtheorem{thm}{Theorem}\n\\begin{thm}[Main]\nTrue.\n\\end{thm}\n\\begin{lstlisting}[language=Rust]\nfn main() {}\n\\end{lstlisting}\n";
    let (e, cx) = open_named(text, "t.tex", || None, cx);
    at(&e, text.len(), cx);
    assert_eq!(
        e.read_with(cx, |e, _| e.line_view(1).display()),
        // Without amsthm, LaTeX puts no period after the head.
        "Theorem 1 (Main) "
    );
    // The listing's lines are code, colored as Rust.
    let colored = e.update(cx, |e, _| {
        let b = e.block_at(text.find("fn main").unwrap())?;
        e.code_spans(&b).map(|(_, spans)| !spans.is_empty())
    });
    assert_eq!(colored, Some(true));
    assert!(e.read_with(cx, |e, _| e.line_view(5).mono));
}

#[gpui::test]
fn latex_inline_code_colored(cx: &mut TestAppContext) {
    let text = "See \\lstinline[language=Rust]{fn main} here.\n";
    let (e, cx) = open_named(text, "t.tex", || None, cx);
    at(&e, text.len(), cx);
    // The code shown as it is, its keyword in a syntax color.
    assert_eq!(
        e.read_with(cx, |e, _| e.line_view(0).display()),
        "See fn main here."
    );
    let colors = e.read_with(cx, |e, _| {
        let theme = e.doc_theme();
        kalem_ui::line::inline_code_colors(e, 0..text.len() - 1, &theme)
    });
    let fn_at = text.find("fn main").unwrap();
    assert!(colors.iter().any(|(r, _)| r.start == fn_at), "{colors:?}");
}

#[gpui::test]
fn latex_class_front_matter(cx: &mut TestAppContext) {
    let text = "\\documentclass{acmart}\n\\begin{document}\n\\title{Deep}\n\\keywords{a, b}\n\\end{document}\n";
    let (e, cx) = open_named(text, "a.tex", || None, cx);
    at(&e, 0, cx);
    assert_eq!(e.read_with(cx, |e, _| e.line_view(2).display()), "Deep");
    assert_eq!(
        e.read_with(cx, |e, _| e.line_view(3).display()),
        "Keywords: a, b"
    );
}

#[gpui::test]
fn latex_figure_dialog(cx: &mut TestAppContext) {
    let text = "\\begin{document}\n\n\\end{document}\n";
    let (e, cx) = open_named(text, "f.tex", || None, cx);
    at(&e, 17, cx);
    // After the picture (the system's dialog), its width and caption.
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "latex.insert.figure",
            serde_json::json!({ "path": "cat.png", "ask": true }),
            window,
            cx,
        )
    });
    let label = |e: &Entity<Editor>, cx: &mut gpui::VisualTestContext| {
        e.read_with(cx, |e, _| {
            e.palette
                .as_ref()
                .and_then(|p| p.arg.as_ref().map(|a| a.label.clone()))
        })
    };
    assert_eq!(label(&e, cx).as_deref(), Some("Insert Figure: width"));
    cx.simulate_keystrokes("enter");
    assert_eq!(label(&e, cx).as_deref(), Some("Insert Figure: caption"));
    cx.simulate_input("A cat");
    cx.simulate_keystrokes("enter");
    let t = text_of(&e, cx);
    assert!(
        t.contains("\\includegraphics[width=0.8\\linewidth]{cat.png}"),
        "{t}"
    );
    assert!(t.contains("\\caption{A cat}"), "{t}");
}

#[gpui::test]
fn latex_build_command(cx: &mut TestAppContext) {
    let (e, cx) = open_named("\\documentclass{article}\n", "b.tex", || None, cx);
    e.update(cx, |e, _| e.doc.meta.path = None);
    cx.simulate_keystrokes("f5");
    let status = e.read_with(cx, |e, _| e.status.clone().map(|s| s.0).unwrap_or_default());
    assert!(status.contains("Save"), "{status}");
}

#[gpui::test]
fn latex_structural_editing(cx: &mut TestAppContext) {
    let text = "\\begin{itemize}\n\\item One\n\\end{itemize}\nsome words\n";
    let (e, cx) = open_named(text, "e.tex", || None, cx);
    at(&e, text.find("One").unwrap() + 3, cx);
    cx.simulate_keystrokes("enter");
    cx.simulate_input("Two");
    assert!(text_of(&e, cx).contains("\\item One\n\\item Two\n"));
    let at_word = text_of(&e, cx).find("words").unwrap() + 1;
    at(&e, at_word, cx);
    cx.simulate_keystrokes("ctrl-b");
    assert!(text_of(&e, cx).contains("some \\textbf{words}"));
    let end = text_of(&e, cx).len();
    at(&e, end, cx);
    cx.simulate_input("\\begin{center}");
    assert!(text_of(&e, cx).ends_with("\\begin{center}\n  \n\\end{center}"));
    at(&e, text_of(&e, cx).find("some").unwrap(), cx);
    cx.simulate_keystrokes("ctrl-1");
    assert!(text_of(&e, cx).contains("\\section{some \\textbf{words}}"));
}

#[gpui::test]
fn latex_completion(cx: &mut TestAppContext) {
    let text = "\\section{A}\\label{sec:a}\n";
    let (e, cx) = open_named(text, "c.tex", || None, cx);
    at(&e, text.len(), cx);
    cx.simulate_input("\\ref{se");
    let labels = e.read_with(cx, |e, _| {
        e.completion.as_ref().map(|m| {
            m.items()
                .iter()
                .map(|i| i.label.clone())
                .collect::<Vec<_>>()
        })
    });
    assert_eq!(labels, Some(vec!["sec:a".to_string()]));
    cx.simulate_keystrokes("enter");
    assert!(
        text_of(&e, cx).ends_with("\\ref{sec:a"),
        "{}",
        text_of(&e, cx)
    );
}

#[gpui::test]
fn latex_math_and_inserts(cx: &mut TestAppContext) {
    let text = "Let \n";
    let (e, cx) = open_named(text, "m.tex", || None, cx);
    at(&e, 4, cx);
    cx.simulate_input("$\\frac");
    assert_eq!(text_of(&e, cx), "Let $\\frac$\n");
    e.update_in(cx, |e, window, cx| {
        e.run_command("latex.insert.equation", serde_json::Value::Null, window, cx)
    });
    cx.run_until_parked();
    assert!(text_of(&e, cx).contains("\\begin{equation}\n  \n  \\label{eq:}\n\\end{equation}\n"));
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "latex.math.toggleNumbering",
            serde_json::Value::Null,
            window,
            cx,
        )
    });
    cx.run_until_parked();
    assert!(text_of(&e, cx).contains("\\begin{equation*}"));
}

#[gpui::test]
fn latex_new_from_template(cx: &mut TestAppContext) {
    let (e, cx) = open_named("notes\n", "n.org", || None, cx);
    let dir = e.read_with(cx, |e, _| {
        e.doc
            .meta
            .path
            .clone()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    });
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "file.newFromTemplate",
            serde_json::json!({"template": "beamer"}),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let written = std::fs::read_to_string(dir.join("beamer.tex")).unwrap();
    assert!(written.starts_with("\\documentclass{beamer}"));
}

#[gpui::test]
fn latex_outline(cx: &mut TestAppContext) {
    let text = "\\section{Intro}\ntext\n\\subsection{Details}\n\\section{End}\n";
    let (e, cx) = open_named(text, "o.tex", || None, cx);
    cx.simulate_keystrokes(&format!("{}-shift-o", primary()));
    cx.run_until_parked();
    // A click on End's row jumps to it.
    let c = cx.debug_bounds("outline-2").expect("the row of End");
    cx.simulate_click(c.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        e.read_with(cx, |e, _| e.doc.selection.head),
        text.find("\\section{End}").unwrap()
    );
}

#[gpui::test]
fn latex_tables_as_grids(cx: &mut TestAppContext) {
    let text = "\\begin{tabular}{lr}\nName & Qty \\\\\n\\hline\n\\emph{apple} & 3 \\\\\nb & 10 \\\\\n\\end{tabular}\n\nafter\n";
    let (e, cx) = open_named(text, "t.tex", || None, cx);
    at(&e, text.len(), cx);
    let x = |e: &Editor, line: usize, src: usize| {
        let p = e.painted.borrow().get(&line).cloned().expect("painted");
        p.layout.caret(p.view.display_offset(src)).origin.x
    };
    let (name, apple, b, three_end, ten_end) = e.read_with(cx, |e, _| {
        (
            x(e, 1, text.find("Name").unwrap()),
            x(e, 3, text.find("apple").unwrap()),
            x(e, 4, text.find("b &").unwrap()),
            x(e, 3, text.find("3 \\").unwrap() + 1),
            x(e, 4, text.find("10").unwrap() + 2),
        )
    });
    // Text columns start together, right-aligned columns end together.
    let near = |a: gpui::Pixels, b: gpui::Pixels| (f32::from(a) - f32::from(b)).abs() < 0.01;
    assert!(near(name, b) && near(apple, b), "{name:?} {apple:?} {b:?}");
    assert!(near(three_end, ten_end), "{three_end:?} {ten_end:?}");
    // In the table, Tab goes to the next cell.
    at(&e, text.find("Name").unwrap(), cx);
    cx.simulate_keystrokes("tab");
    assert_eq!(
        e.read_with(cx, |e, _| e.doc.selection.head),
        text.find("Qty").unwrap()
    );
}

#[gpui::test]
fn csv_typing_quotes_the_field(cx: &mut TestAppContext) {
    let text = "name,note\napple,red\n";
    let (e, cx) = open_named(text, "d.csv", || None, cx);
    at(&e, text.find("red").unwrap() + 3, cx);
    cx.simulate_input(", ripe");
    assert_eq!(text_of(&e, cx), "name,note\napple,\"red, ripe\"\n");
}

#[gpui::test]
fn csv_malformed_field_in_the_status_bar(cx: &mut TestAppContext) {
    let text = "name,note\napple,6\" long\n";
    let (e, cx) = open_named(text, "d.csv", || None, cx);
    at(&e, text.find("long").unwrap(), cx);
    // What the workspace's status bar shows for the table at the cursor.
    let status = e.read_with(cx, |e, _| kalem_core::formulas::selection_stats(&e.doc));
    assert!(status.is_some_and(|s| s.contains("quote inside an unquoted value")));
}

#[gpui::test]
fn enter_in_csv_keeps_no_indentation(cx: &mut TestAppContext) {
    // Leading tabs are empty fields: Enter does not copy them.
    let text = "\ta\tb\n";
    let (e, cx) = open_named(text, "d.tsv", || None, cx);
    at(&e, 4, cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(text_of(&e, cx), "\ta\tb\n\n");
}

#[gpui::test]
fn bibtex_grid(cx: &mut TestAppContext) {
    let text = "% refs\n@book{knuth84,\n  author = {Donald E. Knuth},\n  title = {The {\\TeX}book},\n  year = 1984,\n}\n\n@article{lamport,\n  author = {Lamport, Leslie},\n  title = {Paxos},\n  year = {1998}\n}\n";
    let (e, cx) = open_named(text, "refs.bib", || None, cx);
    at(&e, 0, cx);
    let row = e.read_with(cx, |e, _| e.line_view(1).display());
    assert!(
        row.starts_with("knuth84 │ book    │ Knuth   │ The TeXbook │ 1984"),
        "{row}"
    );
    // Only the entries' first lines show away from them.
    let lines = e.read_with(cx, |e, _| e.visible.clone());
    assert_eq!(lines, vec![0, 1, 7]);
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "bib.sortView",
            serde_json::json!({ "column": "year", "reverse": true }),
            window,
            cx,
        )
    });
    let lines = e.read_with(cx, |e, _| e.visible.clone());
    assert_eq!(lines, vec![0, 7, 1]);
    assert_eq!(text_of(&e, cx), text);
    // In an entry: all its lines, as source.
    e.update_in(cx, |e, window, cx| {
        e.run_command("bib.unsortView", serde_json::json!({}), window, cx)
    });
    at(&e, text.find("Paxos").unwrap(), cx);
    let lines = e.read_with(cx, |e, _| e.visible.clone());
    assert_eq!(lines, vec![0, 1, 7, 8, 9, 10, 11]);
    assert_eq!(
        e.read_with(cx, |e, _| e.line_view(9).display()),
        "  title = {Paxos},"
    );
}

#[gpui::test]
fn latex_table_spans(cx: &mut TestAppContext) {
    let text = "\\begin{tabular}{lll}\n\\multicolumn{2}{c}{Head} & z \\\\ \\hline\nalpha & beta & gamma \\\\\n\\end{tabular}\n\nafter\n";
    let (e, cx) = open_named(text, "t.tex", || None, cx);
    at(&e, text.len(), cx);
    let x = |e: &Editor, line: usize, src: usize| {
        let p = e.painted.borrow().get(&line).cloned().expect("painted");
        p.layout.caret(p.view.display_offset(src)).origin.x
    };
    let (head, alpha, beta, z, gamma) = e.read_with(cx, |e, _| {
        (
            x(e, 1, text.find("Head").unwrap()),
            x(e, 2, text.find("alpha").unwrap()),
            x(e, 2, text.find("beta").unwrap()),
            x(e, 1, text.find("z \\").unwrap()),
            x(e, 2, text.find("gamma").unwrap()),
        )
    });
    // The span centered across the two columns it covers; the column
    // after it where the next row's third column is.
    assert!(head > alpha && head < beta, "{head:?} {alpha:?} {beta:?}");
    let near = |a: gpui::Pixels, b: gpui::Pixels| (f32::from(a) - f32::from(b)).abs() < 0.01;
    assert!(near(z, gamma), "{z:?} {gamma:?}");
}

#[gpui::test]
fn file_manager_editable_names(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open_project(false, cx);
    let p = primary();
    cx.simulate_keystrokes(&format!("{p}-alt-d"));
    assert!(cursor_line(&ws, cx).ends_with(" a.org"));
    // `e` makes the names text; typing edits them, Ctrl+S renames.
    cx.simulate_keystrokes("e");
    cx.simulate_input("new-");
    assert!(
        cursor_line(&ws, cx).ends_with(" new-a.org"),
        "{}",
        cursor_line(&ws, cx)
    );
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    assert!(dir.join("proj/new-a.org").is_file() && !dir.join("proj/a.org").exists());
    // Escape discards an edit.
    cx.simulate_keystrokes("e");
    cx.simulate_input("x");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(cursor_line(&ws, cx).ends_with(" new-a.org"));
    assert!(dir.join("proj/new-a.org").is_file());
}

#[gpui::test]
fn file_manager_find_and_search(cx: &mut TestAppContext) {
    let (ws, _dir, cx) = open_project(false, cx);
    let p = primary();
    cx.simulate_keystrokes(&format!("{p}-alt-d"));
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "dired.findName",
            serde_json::json!({ "pattern": "*.org" }),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let text = ws.read_with(cx, |ws, cx| {
        ws.editor.read(cx).doc.text().as_str().to_string()
    });
    let sep = std::path::MAIN_SEPARATOR;
    assert!(text.contains(&format!("sub{sep}b.org")), "{text}");
    // `A` searches the text of the folder's files.
    cx.simulate_keystrokes("^ shift-a");
    cx.simulate_input("needle");
    settle_picker(&ws, cx);
    let hits = ws.read_with(cx, |ws, cx| {
        ws.editor
            .read(cx)
            .palette
            .as_ref()
            .and_then(|p| p.search.as_ref())
            .map_or(0, |s| s.hits.len())
    });
    assert_eq!(hits, 1);
    cx.simulate_keystrokes("enter");
    assert_eq!(active_title(&ws, cx), "b.org");
}

#[gpui::test]
fn file_manager_stored_links(cx: &mut TestAppContext) {
    let (ws, _dir, cx) = open_project(false, cx);
    let p = primary();
    cx.simulate_keystrokes(&format!("{p}-alt-d"));
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    e.update_in(cx, |e, window, cx| {
        e.run_command("link.store", serde_json::Value::Null, window, cx)
    });
    // Back in a.org, the link goes in at the cursor.
    cx.simulate_keystrokes(&format!("{p}-alt-d"));
    cx.run_until_parked();
    assert_eq!(active_title(&ws, cx), "a.org");
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    let end = e.read_with(cx, |e, _| e.doc.text().len());
    at(&e, end, cx);
    e.update_in(cx, |e, window, cx| {
        e.run_command("org.link.insertStored", serde_json::Value::Null, window, cx)
    });
    assert!(
        crate::text(&e, cx).ends_with("[[file:a.org][a.org]]"),
        "{}",
        crate::text(&e, cx)
    );
}

#[cfg(unix)]
#[gpui::test]
fn file_manager_shell_command(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open_project(false, cx);
    let p = primary();
    cx.simulate_keystrokes(&format!("{p}-alt-d"));
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "dired.shellCommand",
            serde_json::json!({ "command": "cp ? copy.org" }),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Yes");
    cx.run_until_parked();
    settle_jobs(&ws, cx);
    assert!(dir.join("proj/copy.org").is_file());
}

#[gpui::test]
fn file_manager_preview(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open_project(false, cx);
    image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 0, 0, 255]))
        .save(dir.join("proj/pic.png"))
        .unwrap();
    let p = primary();
    cx.simulate_keystrokes(&format!("{p}-alt-d"));
    // `v`: the file at the cursor beside the listing.
    cx.simulate_keystrokes("v");
    cx.run_until_parked();
    assert!(cx.debug_bounds("preview").is_some());
    let shown = ws.read_with(cx, |ws, cx| {
        let e = ws.editor.read(cx);
        e.preview_cache
            .borrow()
            .as_ref()
            .map(|(p, _, v)| (p.clone(), (**v).clone()))
    });
    assert_eq!(
        shown,
        Some((
            dir.join("proj/a.org"),
            kalem_core::dired::Preview::Text("* A\nalpha".into())
        ))
    );
    // Ctrl+T: the pictures as thumbnails; a click goes to its line.
    cx.simulate_keystrokes("ctrl-t");
    cx.run_until_parked();
    let thumb = cx.debug_bounds("thumbnail-0").expect("a thumbnail");
    cx.simulate_click(thumb.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(
        cursor_line(&ws, cx).ends_with(" pic.png"),
        "{}",
        cursor_line(&ws, cx)
    );
    // Again: hidden.
    cx.simulate_keystrokes("ctrl-t");
    cx.run_until_parked();
    assert!(cx.debug_bounds("preview").is_none());
}

#[gpui::test]
fn latex_follow_reference(cx: &mut TestAppContext) {
    let text = "\\section{Intro}\\label{intro}\ntext\nSee \\ref{intro}.\n";
    let (e, cx) = open_named(text, "r.tex", || None, cx);
    at(&e, text.find("\\ref").unwrap() + 2, cx);
    e.update_in(cx, |e, window, cx| {
        e.run_command("latex.link.open", serde_json::Value::Null, window, cx)
    });
    assert_eq!(
        e.read_with(cx, |e, _| e.doc.selection.head),
        text.find("\\label").unwrap()
    );
}

#[gpui::test]
fn empty_latex_document(cx: &mut TestAppContext) {
    // An empty `.tex` file opens, draws and takes typing.
    let (e, cx) = open_named("", "empty.tex", || None, cx);
    cx.simulate_input("x");
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert_eq!(crate::text(&e, cx), "");
}

#[gpui::test]
fn menus_and_toolbar_follow_the_mode(cx: &mut TestAppContext) {
    // Commands that do not serve a document's type are not offered: no
    // Italic for LaTeX, in the menus or on the toolbar.
    let reg = kalem_core::CommandRegistry::with_builtins();
    let doc = |mode: &str, ty: &str| {
        let mut c = kalem_core::when::Context::default();
        c.set("editorMode", kalem_core::when::Value::Str(mode.into()));
        c.set("textType", kalem_core::when::Value::Str(ty.into()));
        c
    };
    let ids = |mode: &str, ty: &str| -> Vec<String> {
        kalem_ui::workspace::menus_for(&reg, &doc(mode, ty))
            .into_iter()
            .flat_map(|m| m.items)
            .filter_map(|it| match it {
                gpui::MenuItem::Action { action, .. } => action
                    .as_any()
                    .downcast_ref::<kalem_ui::editor::RunCommand>()
                    .map(|rc| rc.id.to_string()),
                _ => None,
            })
            .collect()
    };
    let klm = ids("org", "klm");
    let latex = ids("latex", "latex");
    assert!(klm.iter().any(|i| i == "org.emphasis.italic"));
    assert!(!latex.iter().any(|i| i == "org.emphasis.italic"));
    assert!(!latex.iter().any(|i| i == "org.headline.setLevel"));
    assert!(!latex.iter().any(|i| i == "export.html"));
    assert!(latex.iter().any(|i| i == "app.save"));
    // Commands that turn on the cursor stay: Fold is on a heading only.
    assert!(klm.iter().any(|i| i == "view.fold"));
    // No separator at either end of a menu, nor two in a row.
    for m in kalem_ui::workspace::menus_for(&reg, &doc("latex", "latex")) {
        let sep: Vec<bool> = m
            .items
            .iter()
            .map(|i| matches!(i, gpui::MenuItem::Separator))
            .collect();
        assert!(
            !sep.is_empty() && !sep[0] && !sep[sep.len() - 1],
            "{}",
            m.name
        );
        assert!(!sep.windows(2).any(|w| w[0] && w[1]), "{}", m.name);
    }
    // Each mode brings its own: LaTeX's formatting and sections, CSV's
    // rows and columns, code's comments and lines.
    assert!(latex.iter().any(|i| i == "latex.format.italic"));
    // BibTeX's grid commands in a `.bib` file only.
    let bib = ids("text", "bib");
    assert!(bib.iter().any(|i| i == "bib.sortView"));
    assert!(!latex.iter().any(|i| i == "bib.sortView"));
    assert!(latex.iter().any(|i| i == "latex.section.setLevel"));
    assert!(latex.iter().any(|i| i == "latex.insert.equation"));
    assert!(latex.iter().any(|i| i == "latex.build"));
    assert!(
        !klm.iter()
            .any(|i| i.starts_with("latex.") || i.starts_with("csv."))
    );
    let csv = ids("csv", "csv");
    assert!(csv.iter().any(|i| i == "csv.insertRow"));
    assert!(!csv.iter().any(|i| i.starts_with("latex.")));
    let python = ids("text", "python");
    assert!(python.iter().any(|i| i == "edit.toggleComment"));
    assert!(python.iter().any(|i| i == "lines.moveUp"));
    let (_e, cx) = open_named("\\section{A}\n", "a.tex", || None, cx);
    assert!(cx.debug_bounds("tool-1").is_none(), "no Org Italic button");
    assert!(
        cx.debug_bounds("tool-10").is_some(),
        "LaTeX's Emphasis button"
    );
    assert!(cx.debug_bounds("tool-files").is_some());
}

#[gpui::test]
fn latex_diagnostics_in_the_editor(cx: &mut TestAppContext) {
    // A deprecated command flagged with its message at the cursor, and
    // fixed by Quick Fix.
    let (e, cx) = open_named("Some {\\bf x} here.\n", "d.tex", || None, cx);
    e.update(cx, |e, _| {
        e.doc.update_latex_diagnostics();
        e.doc.move_cursor(7, false);
    });
    let flagged = e.read_with(cx, |e, _| {
        e.line_view(0)
            .runs
            .iter()
            .filter(|r| r.style.flagged == Some(false))
            .map(|r| r.text.clone())
            .collect::<String>()
    });
    assert_eq!(flagged, "\\bf");
    let note = e.update(cx, |e, _| e.cite_preview.get(&mut e.doc));
    assert!(note.is_some_and(|n| n.starts_with("ⓘ")));
    cx.dispatch_action(kalem_ui::editor::RunCommand::new("latex.fix"));
    assert_eq!(text(&e, cx), "Some {\\bfseries x} here.\n");
}

#[gpui::test]
fn csv_filter_and_header(cx: &mut TestAppContext) {
    let mut rows = String::from("name,city\n");
    for i in 0..60 {
        let city = if i % 10 == 0 { "Izmir" } else { "Ankara" };
        rows.push_str(&format!("p{i},{city}\n"));
    }
    let (e, cx) = open_named(&rows, "people.csv", || None, cx);
    // Scrolled down: the header row is pinned at the top.
    assert!(cx.debug_bounds("csv-header").is_none());
    e.update(cx, |e, _| {
        e.list.scroll_to(gpui::ListOffset {
            item_ix: 40,
            offset_in_item: gpui::px(0.),
        })
    });
    cx.run_until_parked();
    e.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert!(cx.debug_bounds("csv-header").is_some(), "the header pinned");
    // A filter keeps the header, the matching rows and the cursor's.
    cx.dispatch_action(kalem_ui::editor::RunCommand::with(
        "csv.filter",
        serde_json::json!({ "text": "izmir" }),
    ));
    let lines = e.read_with(cx, |e, _| e.visible.clone());
    assert_eq!(lines, vec![0, 1, 11, 21, 31, 41, 51]);
    let status = e.read_with(cx, |e, _| kalem_core::csv::status(&e.doc));
    assert!(status.is_some_and(|s| s.contains("6")));
    cx.dispatch_action(kalem_ui::editor::RunCommand::new("csv.clearFilter"));
    assert_eq!(e.read_with(cx, |e, _| e.visible.len()), 62);
}

#[gpui::test]
fn latex_view_with_replaced_text(cx: &mut TestAppContext) {
    // The view shows `↵` for `\\`: the syntax colors of the source must
    // not cut it (they did, and the frame callback aborted).
    for text in [
        "x\\\\y \\ref{a} \\\\*[2pt]\n",
        "Some {\\bf x}\\\\\nnext \\ref{nope}\\\\\n",
        "\\begin{tabular}{ll}\na & b \\\\\n\\hline\nc & d \\\\\n\\end{tabular}\n",
    ] {
        let (e, cx) = open_named(text, "t.tex", || None, cx);
        e.update(cx, |e, _| e.doc.update_latex_diagnostics());
        for pos in (0..=text.len()).filter(|p| text.is_char_boundary(*p)) {
            e.update(cx, |e, cx| {
                e.doc.move_cursor(pos, false);
                cx.notify();
            });
            cx.run_until_parked();
        }
    }
}

#[gpui::test]
fn latex_preamble_folds(cx: &mut TestAppContext) {
    let text = "\\documentclass{article}\n\\usepackage{amsmath}\n\\usepackage{graphicx}\n\\begin{document}\nHello.\n\\end{document}\n";
    let (e, cx) = open_named(text, "p.tex", || None, cx);
    let at = text.find("Hello").unwrap();
    e.update(cx, |e, cx| {
        e.doc.move_cursor(at, false);
        e.after_change(cx);
    });
    let lines = e.read_with(cx, |e, _| e.visible.clone());
    assert_eq!(&lines[..3], &[0, 3, 4], "{lines:?}");
}

#[gpui::test]
fn csv_view_sorted(cx: &mut TestAppContext) {
    let text = "name,age\nAda,36\nBob,7\nCem,20\n";
    let (e, cx) = open_named(text, "s.csv", || None, cx);
    let at = text.find("36").unwrap();
    e.update(cx, |e, cx| {
        e.doc.move_cursor(at, false);
        e.after_change(cx);
    });
    cx.dispatch_action(kalem_ui::editor::RunCommand::new("csv.sortView"));
    let lines = e.read_with(cx, |e, _| e.visible.clone());
    assert_eq!(lines, vec![0, 2, 3, 1, 4]);
    assert_eq!(
        text,
        e.read_with(cx, |e, _| e.doc.text().as_str().to_string())
    );
    cx.dispatch_action(kalem_ui::editor::RunCommand::new("csv.unsortView"));
    assert_eq!(
        e.read_with(cx, |e, _| e.visible.clone()),
        vec![0, 1, 2, 3, 4]
    );
}

#[gpui::test]
fn latex_sections_fold(cx: &mut TestAppContext) {
    let text = "\\section{One}\nFirst text.\n\\section{Two}\nSecond text.\n";
    let (e, cx) = open_named(text, "f.tex", || None, cx);
    e.update(cx, |e, cx| {
        e.doc.move_cursor(2, false);
        e.after_change(cx);
    });
    cx.simulate_keystrokes("tab");
    let second = text.find("Second").unwrap();
    e.update(cx, |e, cx| {
        e.doc.move_cursor(second, false);
        e.after_change(cx);
    });
    let lines = e.read_with(cx, |e, _| e.visible.clone());
    assert!(!lines.contains(&1), "{lines:?}");
    assert!(lines.contains(&2) && lines.contains(&3), "{lines:?}");
}
