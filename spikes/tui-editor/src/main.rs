//! Kalem terminal spike (tasks T0.8, decision D14).
//!
//! Usage:
//!   tui-editor-spike FILE                      interactive editor
//!   tui-editor-spike --detect                  print terminal capabilities
//!   tui-editor-spike FILE --snapshot 80x24 [--cursor N]   render once, print the screen
//!   tui-editor-spike FILE --bench-scroll N     frame cost of scrolling (headless)
//!   tui-editor-spike FILE --bench-type N       frame cost of typing (headless)

mod app;
mod caps;
mod math;
mod render;
#[allow(dead_code)]
#[path = "../../gpui-editor/src/view.rs"]
mod view;

use std::io::Write;
use std::time::{Duration, Instant};

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use app::App;

fn flag(args: &[String], f: &str) -> Option<String> {
    args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned()
}

fn strip_escapes(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            // OSC ... ESC \
            if it.peek() == Some(&']') {
                while let Some(d) = it.next() {
                    if d == '\x1b' && it.peek() == Some(&'\\') {
                        it.next();
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn headless_caps() -> caps::Caps {
    caps::Caps {
        terminal: Some("test backend".into()),
        true_color: true,
        italic: true,
        strikethrough: true,
        hyperlinks: true,
        graphics: "none (headless)".into(),
        ..Default::default()
    }
}

fn report(name: &str, d: &[Duration]) {
    let mut v: Vec<f64> = d.iter().map(|x| x.as_secs_f64() * 1000.).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |q: f64| v[((v.len() as f64 - 1.) * q) as usize];
    println!("{name}: {} frames, p50 {:.3} ms, p99 {:.3} ms, max {:.3} ms", v.len(), q(0.5), q(0.99), q(1.0));
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--detect") {
        crossterm::terminal::enable_raw_mode()?;
        let mut c = caps::query();
        crossterm::terminal::disable_raw_mode()?;
        let mut report = match ratatui_image::picker::Picker::from_query_stdio() {
            Ok(p) => {
                c.graphics = format!("{:?}", p.protocol_type());
                let f = p.font_size();
                c.cell_size = Some((f.width, f.height));
                format!("{}\ncapabilities        {:?}", c.report(), p.capabilities())
            }
            Err(e) => {
                c.graphics = format!("query failed: {e}");
                c.report()
            }
        };
        report.push_str(&format!(
            "\nenv                 TERM={:?} TERM_PROGRAM={:?} COLORTERM={:?}",
            std::env::var("TERM").ok(),
            std::env::var("TERM_PROGRAM").ok(),
            std::env::var("COLORTERM").ok()
        ));
        match flag(&args, "--out") {
            Some(out) => std::fs::write(out, report + "\n")?,
            None => println!("{report}"),
        }
        return Ok(());
    }
    let path = std::path::PathBuf::from(args.get(1).cloned().expect("FILE"));
    let text = std::fs::read_to_string(&path)?;

    if let Some(size) = flag(&args, "--snapshot") {
        let (w, h) = size.split_once('x').map(|(w, h)| (w.parse().unwrap(), h.parse().unwrap())).unwrap_or((80, 24));
        let mut app = App::new(path, text, headless_caps(), None);
        if let Some(c) = flag(&args, "--cursor") {
            app.cursor = c.parse().unwrap();
        } else {
            app.cursor = app.text.len();
            app.top = 0;
        }
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer().clone();
        for y in 0..h {
            let mut line = String::new();
            let mut x = 0;
            while x < w {
                let cell = &buf[(x, y)];
                line.push_str(&strip_escapes(cell.symbol()));
                x += 1;
            }
            println!("{}", line.trim_end());
        }
        return Ok(());
    }

    if let Some(n) = flag(&args, "--bench-scroll").and_then(|n| n.parse::<usize>().ok()) {
        let mut app = App::new(path, text, headless_caps(), None);
        app.cursor = app.text.len();
        let mut term = Terminal::new(TestBackend::new(120, 50)).unwrap();
        for i in 0..n {
            app.top = i % app.line_count().max(1);
            term.draw(|f| app.draw(f)).unwrap();
        }
        report("scroll (headless, 120x50)", &app.stats.frame[3..]);
        return Ok(());
    }
    if let Some(n) = flag(&args, "--bench-type").and_then(|n| n.parse::<usize>().ok()) {
        let mut app = App::new(path, text, headless_caps(), None);
        let mid = app.text.len() / 2;
        app.cursor = app.text[..mid].rfind('\n').map_or(0, |i| i + 1);
        let mut term = Terminal::new(TestBackend::new(120, 50)).unwrap();
        for i in 0..n {
            let c = if i % 7 == 0 { ' ' } else { 'x' };
            app.key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char(c)));
            term.draw(|f| app.draw(f)).unwrap();
        }
        report("typing (headless, 120x50)", &app.stats.frame[3..]);
        let mut r: Vec<f64> = app.stats.reparse.iter().map(|d| d.as_secs_f64() * 1e6).collect();
        r.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("  reparse per edit: p50 {:.0} µs, max {:.0} µs", r[r.len() / 2], r[r.len() - 1]);
        return Ok(());
    }

    // Interactive.
    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture)?;
    let mut c = caps::query();
    let picker = ratatui_image::picker::Picker::from_query_stdio().ok();
    if let Some(p) = &picker {
        c.graphics = format!("{:?}", p.protocol_type());
    }
    let mut app = App::new(path, text, c, picker);
    let result = (|| -> std::io::Result<()> {
        while !app.quit {
            let sync = app.caps.synchronized_output;
            if sync {
                execute!(std::io::stdout(), BeginSynchronizedUpdate)?;
            }
            let t = Instant::now();
            terminal.draw(|f| app.draw(f))?;
            if sync {
                execute!(std::io::stdout(), EndSynchronizedUpdate)?;
            }
            let _ = t;
            if event::poll(Duration::from_millis(500))? {
                app.event(event::read()?);
                while event::poll(Duration::ZERO)? {
                    app.event(event::read()?);
                }
            }
        }
        Ok(())
    })();
    execute!(std::io::stdout(), DisableMouseCapture)?;
    ratatui::restore();
    std::io::stdout().flush()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier};

    fn render(text: &str, cursor: usize) -> ratatui::buffer::Buffer {
        let mut app = App::new("t.org".into(), text.into(), headless_caps(), None);
        app.cursor = cursor;
        let mut term = Terminal::new(TestBackend::new(60, 10)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        term.backend().buffer().clone()
    }

    fn row(buf: &ratatui::buffer::Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| strip_escapes(buf[(x, y)].symbol())).collect::<String>().trim_end().to_string()
    }

    #[test]
    fn styles_links_and_reveal() {
        let text = "Some *bold* and /it/ and [[https://orgmode.org][Org]] here\n* TODO Head\nend\n";
        let end = text.len();
        let buf = render(text, end);
        assert_eq!(row(&buf, 0), " Some bold and it and Org here");
        // "bold" is bold, "it" italic.
        assert!(buf[(6, 0)].modifier.contains(Modifier::BOLD));
        assert!(buf[(15, 0)].modifier.contains(Modifier::ITALIC));
        // The link opens with OSC 8 on its first cell and closes on its last.
        let first = buf[(22, 0)].symbol();
        assert!(first.starts_with("\x1b]8;;https://orgmode.org\x1b\\"), "{first:?}");
        assert!(buf[(24, 0)].symbol().ends_with("\x1b]8;;\x1b\\"));
        assert!(buf[(23, 0)].modifier.contains(Modifier::UNDERLINED));
        // Headline: level glyph, level color, TODO in red.
        assert_eq!(row(&buf, 1), " ◉ TODO Head");
        assert_eq!(buf[(3, 1)].fg, Color::Red);
        // Cursor inside the bold text reveals its markers.
        let buf = render(text, 7);
        assert_eq!(row(&buf, 0), " Some *bold* and it and Org here");
    }

    #[test]
    fn clicking_a_checkbox_toggles_it() {
        let text = "- [ ] task\nend\n";
        let mut app = App::new("t.org".into(), text.into(), headless_caps(), None);
        app.cursor = text.len();
        let mut term = Terminal::new(TestBackend::new(40, 5)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        app.click(3, 0);
        assert_eq!(app.text, "- [X] task\nend\n");
    }
}
