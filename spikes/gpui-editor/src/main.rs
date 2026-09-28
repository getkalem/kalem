//! Kalem gpui spike (tasks T0.6, decision D3).
//!
//! Usage:
//!   gpui-editor-spike FILE                    interactive editor
//!   gpui-editor-spike FILE --bench-scroll N   scroll N frames, print frame times
//!   gpui-editor-spike FILE --bench-jump N     jump a page per frame for N frames
//!   gpui-editor-spike FILE --bench-type N     type N characters, print frame times
//!   gpui-editor-spike FILE --script           fold, click widgets, print layouts

mod editor;
mod inline;
mod math;
mod view;

use gpui::{App, Application, Bounds, Focusable, KeyBinding, WindowBounds, WindowOptions, prelude::*, px, size};

use editor::{Backspace, Bench, Copy, Delete, Down, Editor, End, Enter, Home, Left, Open, Paste, Right, SaveAs, Up};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).cloned().unwrap_or_else(|| "../../tests/corpus/org-mode/org-manual.org".into());
    let text = std::fs::read_to_string(&path).expect("read file");
    let bench_arg = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<usize>().ok());
    let bench = if args.iter().any(|a| a == "--script") {
        Some(Bench::Script { step: 0 })
    } else if let Some(n) = bench_arg("--bench-scroll") {
        Some(Bench::Scroll { frames: n, done: 0, times: Vec::new() })
    } else if let Some(n) = bench_arg("--bench-jump") {
        Some(Bench::Jump { frames: n, done: 0, times: Vec::new() })
    } else {
        bench_arg("--bench-type").map(|n| Bench::Type { chars: n, done: 0, times: Vec::new() })
    };
    Application::new().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("left", Left, None),
            KeyBinding::new("right", Right, None),
            KeyBinding::new("up", Up, None),
            KeyBinding::new("down", Down, None),
            KeyBinding::new("backspace", Backspace, None),
            KeyBinding::new("delete", Delete, None),
            KeyBinding::new("enter", Enter, None),
            KeyBinding::new("cmd-v", Paste, None),
            KeyBinding::new("cmd-c", Copy, None),
            KeyBinding::new("home", Home, None),
            KeyBinding::new("end", End, None),
            KeyBinding::new("cmd-o", Open, None),
            KeyBinding::new("cmd-shift-s", SaveAs, None),
        ]);
        let bounds = Bounds::centered(None, size(px(900.), px(800.)), cx);
        let window = cx
            .open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() }, |window, cx| {
                cx.new(|cx| {
                    let mut e = Editor::new(text, cx);
                    e.path = Some(std::path::PathBuf::from(&path));
                    if let Some(b) = bench {
                        if matches!(b, Bench::Type { .. }) {
                            // Type in the middle of the document.
                            let mid = e.line_starts[e.line_starts.len() / 2];
                            e.cursor = mid;
                            e.list.scroll_to_reveal_item(e.line_starts.len() / 2);
                        }
                        e.bench = Some(b);
                    }
                    window.focus(&cx.focus_handle());
                    e
                })
            })
            .unwrap();
        window
            .update(cx, |editor, window, cx| {
                window.focus(&editor.focus_handle(cx));
            })
            .ok();
        cx.activate(true);
        // Watchdog: benchmarks end even if frames stop (for example when the
        // screen is locked).
        cx.spawn(async move |cx| {
            cx.background_executor().timer(std::time::Duration::from_secs(120)).await;
            let _ = cx.update(|cx| {
                println!("watchdog: quitting after 120 s");
                cx.quit()
            });
        })
        .detach();
    });
}
