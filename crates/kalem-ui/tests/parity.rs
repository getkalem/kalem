//! The two editors side by side (design §4.1, principle 7; D26): the same
//! files opened in the graphical and the terminal editor, the cursor in
//! the same place, must show the same lines, and on each line the same
//! words in the same order. What only one editor can draw (a formula or a
//! picture as an image; the terminal's line numbers and bars) is left out
//! of the comparison: widgets on both sides, and anything not a word.

#![allow(clippy::print_stderr)]

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use gpui::{Entity, TestAppContext, VisualTestContext};
use kalem_core::settings::Config;
use kalem_ui::editor::Editor;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::Workspace;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// The samples: one for each mode and the constructs each renders.
const SAMPLES: &[(&str, &str)] = &[
    (
        "notes.org",
        "#+TITLE: Notes\n\n* Heading one :tag:\nSome *bold*, /italic/, =code= and a [[https://example.com][link]].\n- item one\n- [ ] a task item\n  1. nested\n** TODO Second level\nA footnote[fn:1] and \\alpha.\n\n| Name | Qty |\n|------+-----|\n| a    |   1 |\n| bb   |  10 |\n\n#+begin_src rust\nfn main() {}\n#+end_src\n\n#+begin_quote\nQuoted words.\n#+end_quote\n\n[fn:1] The note.\n",
    ),
    (
        "paper.tex",
        "\\documentclass{article}\n\\usepackage{amsmath}\n\\title{A Paper}\n\\author{Ada}\n\\begin{document}\n\\maketitle\n\\section{Intro}\\label{s:i}\nText with \\emph{emphasis}, \\textbf{bold} and ``quotes'' -- see Section~\\ref{s:i}.\n\\begin{itemize}\n\\item First\n\\item Second\n\\end{itemize}\n\\begin{tabular}{lr}\nName & Qty \\\\\n\\hline\na & 1 \\\\\n\\end{tabular}\n\nA footnote\\footnote{Its text.} here.\n% a comment\n\\subsection{More}\nThe end.\n\\end{document}\n",
    ),
    (
        "data.csv",
        "name,qty,price\napple,3,1.50\n\"banana, ripe\",12,0.25\ncherry,100,9.00\n",
    ),
    (
        "refs.bib",
        "% refs\n@book{knuth84,\n  author = {Donald E. Knuth},\n  title = {The {\\TeX}book},\n  year = 1984,\n}\n\n@article{lamport,\n  author = {Lamport, Leslie},\n  title = {Paxos Made Simple},\n  year = {2001}\n}\n",
    ),
    (
        "main.rs",
        "// A comment\nfn main() {\n    let x = 1;\n    println!(\"{x}\");\n}\n",
    ),
    (
        "readme.md",
        "---\ntitle: Notes\ntags: [a, b]\n---\n# Title\n\nSome *text*, **bold**, ~~gone~~ and `code`, a [link](https://example.com) and [[Another Note]].\n\n## Lists\n\n- a list\n- [ ] a task\n- [x] done\n  1. nested\n\n> A quote\n> on two lines.\n\n| Name | Qty |\n|:-----|----:|\n| a    |   1 |\n\n```rust\nfn main() {}\n```\n\nA footnote[^1] and $x^2$.\n\n---\n\n![logo](logo.png)\n\n[^1]: The note.\n",
    ),
];

thread_local! {
    /// The lines the terminal cut at the screen's edge.
    static TRUNCATED: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The words of `s`, in order.
fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut w = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            w.push(c);
        } else if !w.is_empty() {
            out.push(std::mem::take(&mut w));
        }
    }
    if !w.is_empty() {
        out.push(w);
    }
    out
}

fn dir_for(name: &str) -> std::path::PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("kalem-parity-{}-{n}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The graphical editor's lines: for each painted source line, its words
/// (widgets left out), and the list of visible lines.
fn gui_lines(
    path: &std::path::Path,
    cursor: usize,
    cx: &mut TestAppContext,
) -> (Vec<usize>, std::collections::BTreeMap<usize, Vec<String>>) {
    let shared = Rc::new(kalem_ui::shared(Config::default()));
    let mut editor = None;
    let (_ws, vcx): (Entity<Workspace>, &mut VisualTestContext) =
        cx.add_window_view(|window, cx| {
            let e = kalem_ui::editor::open(Some(path), shared, Theme::light(), cx).unwrap();
            let focus = gpui::Focusable::focus_handle(e.read(cx), cx);
            window.focus(&focus, cx);
            editor = Some(e.clone());
            Workspace::new(e, window, cx)
        });
    let e: Entity<Editor> = editor.unwrap();
    e.update(vcx, |e, cx| {
        e.doc.move_cursor(cursor, false);
        e.after_change(cx);
    });
    vcx.run_until_parked();
    // A frame after the cursor moved.
    e.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    e.update(vcx, |e, _| {
        let mut out = std::collections::BTreeMap::new();
        let painted: Vec<(usize, Rc<kalem_core::view::LineView>, bool)> = e
            .painted
            .borrow()
            .iter()
            .map(|(l, p)| {
                let toc = p
                    .widgets
                    .iter()
                    .any(|w| matches!(w.2, kalem_core::view::Widget::TocRow { .. }));
                (*l, p.view.clone(), toc)
            })
            .collect();
        for (line, view, toc) in painted {
            if std::env::var("PARITY_DEBUG").is_ok() {
                eprintln!("GUI {line} toc={toc} {:?}", view.display());
            }
            // A table of contents or a list of notes (painted as rows over
            // one stand-in run): its title and rows.
            let stand_in = view.runs.len() == 1
                && view.runs[0].widget.is_none()
                && view.runs[0].text == kalem_core::view::PLACEHOLDER;
            if toc || stand_in {
                let range = e.doc.text().line_range(line);
                if let Some(l) = kalem_core::toc::listing(&mut e.doc, range) {
                    let mut t = l.title.clone();
                    for (r, _) in &l.rows {
                        t.push(' ');
                        t.push_str(r);
                    }
                    out.insert(line, words(&t));
                    continue;
                }
            }
            // A formula or a picture drawn as an image: the terminal may
            // draw it as text; nothing to compare.
            if !view.runs.is_empty()
                && view
                    .runs
                    .iter()
                    .all(|r| r.widget.is_some() || r.text.trim().is_empty())
                && view.runs.iter().any(|r| r.widget.is_some())
            {
                continue;
            }
            let text: String = view
                .runs
                .iter()
                .filter(|r| r.widget.is_none())
                .map(|r| r.text.as_str())
                .collect::<Vec<_>>()
                .join("");
            // A CSV grid's row number in its gutter is not the line's text.
            let mut w = words(&text);
            if e.doc.meta.mode == kalem_core::DocumentMode::Csv
                && w.first().is_some_and(|f| *f == (line + 1).to_string())
            {
                w.remove(0);
            }
            out.insert(line, w);
        }
        (e.visible.clone(), out)
    })
}

/// The terminal editor's lines: each screen cell mapped back to its source
/// offset; the words of each source line's cells (widgets left out).
fn tui_lines(
    path: &std::path::Path,
    cursor: usize,
) -> (Vec<usize>, std::collections::BTreeMap<usize, Vec<String>>) {
    TRUNCATED.with(|c| c.borrow_mut().clear());
    let caps = kalem_tui::caps::Caps::full();
    let mut app = kalem_tui::app::App::with_keymap(
        Some(path),
        Config::default(),
        caps.clone(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let mut term = Terminal::new(TestBackend::new(220, 120)).unwrap();
    app.doc.move_cursor(cursor, false);
    app.editor.follow = true;
    term.draw(|f| app.draw(f)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer().clone();
    let mut cells: std::collections::BTreeMap<usize, String> = Default::default();
    // Where each line's last row ended in the text: a row going on right
    // after it continues a word the wrapping broke.
    let mut ended: std::collections::HashMap<usize, usize> = Default::default();
    for y in 0..buf.area.height.saturating_sub(1) {
        let mut row_line: Option<usize> = None;
        let mut first_off: Option<usize> = None;
        let mut last_off: Option<usize> = None;
        let mut row = String::new();
        let mut skip = false;
        for x in 0..buf.area.width {
            if skip {
                // The second cell of a wide character.
                skip = false;
                continue;
            }
            let sym = buf[(x, y)].symbol().to_string();
            // Links carry OSC 8 escapes around their text.
            let sym: String = if sym.contains('\x1b') {
                let mut s = String::new();
                let mut it = sym.chars().peekable();
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
                    s.push(c);
                }
                s
            } else {
                sym
            };
            skip = unicode_width::UnicodeWidthStr::width(sym.as_str()) > 1;
            match app.editor.hit(&app.doc, &caps, x, y) {
                Some((off, None)) => {
                    let l = app.doc.text().line_of(off.min(app.doc.text().len()));
                    row_line.get_or_insert(l);
                    if !sym.trim().is_empty() {
                        first_off.get_or_insert(off);
                        last_off = Some(off + sym.len());
                    }
                    row.push_str(&sym);
                }
                // A row of a table of contents: its text.
                Some((off, Some((kalem_core::view::Widget::TocRow { .. }, _, _)))) => {
                    let l = app.doc.text().line_of(off.min(app.doc.text().len()));
                    row_line.get_or_insert(l);
                    row.push_str(&sym);
                }
                // A widget, or no text (the margin, the line numbers).
                Some((_, Some(_))) => row.push(' '),
                None => row.push(' '),
            }
        }
        if std::env::var("PARITY_DEBUG").is_ok() {
            eprintln!("{y:3} {row_line:?} {row}");
        }
        if let Some(l) = row_line
            && row.trim_end().chars().count() + 8 >= buf.area.width as usize
        {
            TRUNCATED.with(|c| c.borrow_mut().push(l));
        }
        if let Some(l) = row_line {
            let e = cells.entry(l).or_default();
            let broken = first_off.is_some() && ended.get(&l).copied() == first_off;
            if broken {
                let trimmed = e.trim_end().len();
                e.truncate(trimmed);
                e.push_str(row.trim_start());
            } else {
                e.push(' ');
                e.push_str(&row);
            }
            if let Some(end) = last_off {
                ended.insert(l, end);
            }
        }
    }
    let visible: Vec<usize> = cells.keys().copied().collect();
    // The line numbers in the margin (plain text, LaTeX, CSV) are not the
    // line's words.
    let out = cells
        .into_iter()
        .map(|(l, s)| {
            let mut w = words(&s);
            if w.first().is_some_and(|f| *f == (l + 1).to_string()) {
                w.remove(0);
            }
            (l, w)
        })
        .collect();
    (visible, out)
}

/// Compares the two editors on `text` (named `name`) with the cursor at
/// `cursor`, adding what differs to `problems`.
fn compare(
    name: &str,
    text: &str,
    cursor: usize,
    cx: &mut TestAppContext,
    problems: &mut Vec<String>,
) {
    let dir = dir_for(name);
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    let (g_visible, g) = gui_lines(&path, cursor, cx);
    let (t_visible, t) = tui_lines(&path, cursor);
    let _ = std::fs::remove_dir_all(&dir);
    // Only as far as both drew (the window and the screen hold different
    // numbers of lines).
    let first = g
        .keys()
        .min()
        .copied()
        .unwrap_or(0)
        .max(t_visible.first().copied().unwrap_or(0));
    let last = g
        .keys()
        .max()
        .copied()
        .unwrap_or(0)
        .min(t_visible.last().copied().unwrap_or(0));
    let within = |l: &usize| first <= *l && *l <= last;
    let g_visible: Vec<usize> = g_visible.into_iter().filter(within).collect();
    let t_visible: Vec<usize> = t_visible.into_iter().filter(within).collect();
    let name = format!("{name} (cursor at {cursor})");
    let name = name.as_str();
    {
        let g_shown: Vec<usize> = g_visible
            .iter()
            .copied()
            .filter(|l| g.contains_key(l))
            .collect();
        // Lines the terminal shows that the graphical editor hides, or the
        // other way round.
        for l in &g_shown {
            if !t_visible.contains(l) && !g[l].is_empty() {
                problems.push(format!(
                    "{name}: line {} shown only in the graphical editor: {:?}",
                    l + 1,
                    g[l]
                ));
            }
        }
        for l in &t_visible {
            if !g_visible.contains(l) && !t[l].is_empty() {
                problems.push(format!(
                    "{name}: line {} shown only in the terminal: {:?}",
                    l + 1,
                    t[l]
                ));
            }
        }
        for l in g_shown.iter().filter(|l| t.contains_key(l)) {
            // The terminal's line numbers are text in the margin with no
            // source offset: left out by the hit test already.
            // A line wider than the screen that does not wrap (a table
            // row): the terminal shows its beginning.
            let cut = t.get(l).is_some_and(|tw| {
                !tw.is_empty()
                    && tw.len() <= g[l].len()
                    && tw[..tw.len() - 1] == g[l][..tw.len() - 1]
                    && g[l][tw.len() - 1].starts_with(tw[tw.len() - 1].as_str())
            }) && TRUNCATED.with(|c| c.borrow().contains(l));
            if g[l] != t[l] && !cut {
                problems.push(format!(
                    "{name}: line {}:\n    graphical: {:?}\n    terminal:  {:?}",
                    l + 1,
                    g[l],
                    t[l]
                ));
            }
        }
    }
}

#[gpui::test]
fn the_two_editors_show_the_same(cx: &mut TestAppContext) {
    let mut problems = Vec::new();
    for (name, text) in SAMPLES {
        // The cursor at the end: the lines shown rendered, away from it;
        // then at the start.
        compare(name, text, text.len(), cx, &mut problems);
        compare(name, text, 0, cx, &mut problems);
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Real files: the Book's chapters, Org's test files, the synthetic Org
/// and LaTeX corpora, the table corpus.
#[gpui::test]
fn the_two_editors_show_the_same_on_the_corpus(cx: &mut TestAppContext) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in [
        "book/part-1",
        "book/part-2",
        "tests/corpus/org-mode/examples",
        "tests/corpus/synthetic",
        "tests/corpus/tables",
        "tests/corpus/latex/synthetic",
        "tests/corpus/markdown/readmes",
        "tests/corpus/markdown/vault/foam-docs",
        "tests/corpus/markdown/vault/foam-docs/user",
        "tests/corpus/markdown/vault/foam-docs/user/features",
        "tests/corpus/markdown/vault/foam-docs/user/getting-started",
    ] {
        let Ok(rd) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        let mut v: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        v.sort();
        files.extend(v);
    }
    let mut problems = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        if text.len() > 200_000
            || f.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("huge"))
        {
            continue;
        }
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        compare(&name, &text, text.len(), cx, &mut problems);
    }
    assert!(
        problems.is_empty(),
        "{} differences:\n{}",
        problems.len(),
        problems.join("\n")
    );
}
