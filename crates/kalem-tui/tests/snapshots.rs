//! Screens of the terminal editor, as text and as a map of styles, for
//! catching rendering changes (`cargo insta review` to accept them).

use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};

const DOC: &str = "#+TITLE: Snapshot
#+AUTHOR: Ada
#+OPTIONS: toc:nil
#+STARTUP: showall
* TODO Heading one :tag:
:PROPERTIES:
:ID: 1
:END:
Some *bold*, /italic/, =code= and [[https://orgmode.org][a link]], \\alpha and x^{2}.
- [ ] a task
- [X] done
  1. nested
| Name  | Qty |
|-------+-----|
| apple |   3 |
** Second level
#+begin_src rust
fn main() { println!(\"hi\"); }
#+end_src
A formula $E=mc^2$ and a long line that wraps around the edge of the narrow screen.
";

fn strip(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            while let Some(d) = it.next() {
                if d == '\x1b' && it.peek() == Some(&'\\') {
                    it.next();
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// One letter per cell: the most telling style (B bold, I italic, U
/// underline, D dim, R reversed, c code background, k colored code, r and
/// g red and green bold, H colored bold, * another color).
fn style_of(cell: &ratatui::buffer::Cell) -> char {
    let m = cell.modifier;
    if m.contains(Modifier::REVERSED) {
        'R'
    } else if cell.bg != Color::Reset && cell.bg != Color::Yellow {
        // Code: `k` where highlighting colors it.
        if cell.fg == Color::Reset { 'c' } else { 'k' }
    } else if m.contains(Modifier::BOLD) {
        match cell.fg {
            Color::Red => 'r',
            Color::Green => 'g',
            Color::Reset => 'B',
            _ => 'H',
        }
    } else if m.contains(Modifier::ITALIC) {
        'I'
    } else if m.contains(Modifier::UNDERLINED) {
        'U'
    } else if m.contains(Modifier::DIM) || cell.fg == Color::DarkGray {
        'D'
    } else if cell.fg != Color::Reset {
        '*'
    } else {
        '.'
    }
}

fn screen(caps: Caps, width: u16, height: u16, cursor: usize) -> String {
    screen_of("snap.org", DOC, caps, width, height, cursor)
}

/// The screen of file `name` holding `text`.
fn screen_of(name: &str, text: &str, caps: Caps, width: u16, height: u16, cursor: usize) -> String {
    let dir = std::env::temp_dir().join(format!(
        "kalem-snap-{}-{name}-{width}-{cursor}-{}",
        std::process::id(),
        caps.ascii
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    let mut app = App::with_keymap(Some(&path), Config::default(), caps, &[], Vec::new()).unwrap();
    app.doc.move_cursor(cursor, false);
    app.editor.follow = true;
    let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    // Twice: the first frame scrolls to the cursor.
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..height.saturating_sub(1) {
        let text: String = (0..width).map(|x| strip(buf[(x, y)].symbol())).collect();
        let styles: String = (0..width).map(|x| style_of(&buf[(x, y)])).collect();
        out.push_str(&format!(
            "{:<w$}|{}\n",
            text.trim_end(),
            styles.trim_end_matches('.'),
            w = width as usize
        ));
    }
    let _ = std::fs::remove_dir_all(&dir);
    out
}

#[test]
fn whole_document() {
    insta::assert_snapshot!(screen(Caps::full(), 60, 26, DOC.len()));
}

#[test]
fn cursor_in_markup_table_and_code() {
    let bold = DOC.find("*bold*").unwrap() + 2;
    insta::assert_snapshot!("cursor_in_bold", screen(Caps::full(), 60, 26, bold));
    let table = DOC.find("| apple").unwrap() + 3;
    insta::assert_snapshot!("cursor_in_table", screen(Caps::full(), 60, 26, table));
    let code = DOC.find("fn main").unwrap();
    insta::assert_snapshot!("cursor_in_code", screen(Caps::full(), 60, 26, code));
}

#[test]
fn ascii_without_color() {
    let caps = Caps {
        ascii: true,
        no_color: true,
        italic: false,
        hyperlinks: false,
        ..Caps::full()
    };
    insta::assert_snapshot!(screen(caps, 44, 30, DOC.len()));
}

/// A Markdown document (T2.7c.8): front matter, headings, emphasis,
/// links and wiki links, lists and tasks, a quote, a table, code, a
/// footnote and a formula.
const MD: &str = "---
title: Snapshot
tags: [a, b]
---
# Heading one

Some *emphasis*, **strong**, ~~struck~~, `code`, a [link](https://example.com) and [[Another Note]].

## Lists

- a list item
- [ ] a task
- [x] done
  1. nested

> A quoted line
> and another.

| Name  | Qty |
|:------|----:|
| apple |   3 |

```rust
fn main() { println!(\"hi\"); }
```

A footnote[^1], a formula $E=mc^2$ and a long line that wraps around the edge of the narrow screen.

---

[^1]: The note.
";

#[test]
fn markdown_document() {
    insta::assert_snapshot!(screen_of("snap.md", MD, Caps::full(), 60, 34, MD.len()));
}

#[test]
fn markdown_cursor_in_markup_table_and_code() {
    let at = |s: &str| MD.find(s).unwrap() + 2;
    insta::assert_snapshot!(
        "markdown_cursor_in_strong",
        screen_of("snap.md", MD, Caps::full(), 60, 34, at("**strong"))
    );
    insta::assert_snapshot!(
        "markdown_cursor_in_table",
        screen_of("snap.md", MD, Caps::full(), 60, 34, at("| apple"))
    );
    insta::assert_snapshot!(
        "markdown_cursor_in_code",
        screen_of("snap.md", MD, Caps::full(), 60, 34, at("fn main"))
    );
}
