//! One source line on screen: its view model shaped into an inline layout,
//! with its block's decorations, the selection, widgets and the caret.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    App, AvailableSpace, Bounds, ElementId, ElementInputHandler, Entity, FontStyle, FontWeight,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, PathBuilder, Pixels,
    SharedString, Size, StrikethroughStyle, Style, TextRun, UnderlineStyle, Window, fill, point,
    px, quad, relative, size,
};
use kalem_core::view::{BlockKind, CheckState, LineRole, LineView, Widget};

use crate::editor::{Editor, Painted};
use crate::theme::Theme;
use gpui_rich_text::{InlineLayout, Piece};

/// How a widget is painted.
enum Paint {
    /// A checkbox.
    Checkbox(CheckState),
    /// A rendered formula.
    Image(std::sync::Arc<gpui::RenderImage>),
    /// A formula that cannot be rendered: its source in a red frame.
    Error(Rc<gpui::ShapedLine>),
    /// Text in a box (a formula as Unicode, an image's name).
    Text(Rc<gpui::ShapedLine>),
    /// A button with a label; clicking copies the block starting there.
    Copy(Rc<gpui::ShapedLine>, usize),
    /// A table of contents: its title and its rows, each leading to the
    /// heading starting there, one `row` high.
    Toc(
        Rc<gpui::ShapedLine>,
        Vec<(Rc<gpui::ShapedLine>, usize)>,
        Pixels,
    ),
}

/// A line, prepared for layout.
struct Prepared {
    view: Rc<LineView>,
    pieces: Vec<Piece>,
    /// Widgets by display offset, with their source ranges.
    widgets: Vec<(usize, Range<usize>, Widget, Paint)>,
    font_size: Pixels,
    /// Where wrapped rows start: after a list bullet and box, or after the
    /// indentation.
    hang_at: Option<usize>,
    /// For a heading with content: whether it is folded, and its start.
    fold: Option<(bool, usize)>,
    background: Option<Hsla>,
    /// Grid lines of a table row: column edges, and whether it is a rule.
    grid: Option<(Vec<Pixels>, bool)>,
    /// A bar in the margin (quotes).
    bar: bool,
    /// A horizontal rule across the line.
    rule: bool,
    /// Laid out without wrapping, scrolled sideways.
    nowrap: bool,
    /// The line spacing (the document's `#+KALEM: spacing=`).
    spacing: f32,
    /// Pictures fitted to the text width when laid out: their pieces, and
    /// the share of the width asked for (`#+ATTR_ORG: :width 50%`).
    fit: Vec<(usize, Option<u32>)>,
}

/// A table drawn as a grid in the proportional font.
#[derive(Debug)]
pub struct Grid {
    view: kalem_core::view::TableView,
    /// Content widths of the columns.
    widths: Vec<Pixels>,
    /// Rows above the first rule: the header, if there is a rule.
    header: usize,
    pad: Pixels,
}

/// The runs of a cell as text runs, bold in the header.
fn cell_runs(
    runs: &[kalem_core::view::Run],
    header: bool,
    theme: &Theme,
) -> (String, Vec<TextRun>) {
    let mut text = String::new();
    let mut out = Vec::new();
    for r in runs {
        let mut st = r.style;
        st.bold |= header;
        out.push(text_run(&st, r.text.len(), 0, false, theme));
        text.push_str(&r.text);
    }
    (text, out)
}

/// The grid of the table starting at `start`, measured.
fn grid(editor: &Editor, start: usize, fs: Pixels, window: &mut Window) -> Option<Rc<Grid>> {
    let key = (start, f32::from(fs) as u32);
    {
        let mut g = editor.grids.borrow_mut();
        if g.0 != editor.doc.version() {
            *g = (editor.doc.version(), std::collections::HashMap::new());
        }
        if let Some(grid) = g.1.get(&key) {
            return Some(grid.clone());
        }
    }
    let (p, current) = editor.doc.parse()?;
    if !current {
        return None;
    }
    let view = kalem_core::view::table_view(&p.syntax(), p.context(), start, None)?;
    let rule = view
        .rows
        .iter()
        .position(|r| matches!(r, kalem_core::view::TableRow::Rule { .. }));
    let header = match rule {
        Some(i) if i > 0 && i + 1 < view.rows.len() => i,
        _ => 0,
    };
    let mut widths = vec![fs; view.align.len()];
    for (ri, row) in view.rows.iter().enumerate() {
        if let kalem_core::view::TableRow::Data { cells, .. } = row {
            for (i, c) in cells.iter().enumerate() {
                let (text, runs) = cell_runs(&c.runs, ri < header, &editor.theme);
                if text.is_empty() {
                    continue;
                }
                let w = window
                    .text_system()
                    .shape_line(text.into(), fs, &runs, None)
                    .width;
                widths[i] = widths[i].max(w);
            }
        }
    }
    let g = Rc::new(Grid {
        view,
        widths,
        header,
        pad: fs * 0.5,
    });
    editor.grids.borrow_mut().1.insert(key, g.clone());
    Some(g)
}

/// A table row away from the cursor: cells at their column's width, the
/// gaps as boxes standing for the bars and padding.
fn prepare_grid(
    editor: &mut Editor,
    line: usize,
    base: Pixels,
    window: &mut Window,
) -> Option<Prepared> {
    use kalem_core::view::{PLACEHOLDER, Run, TableRow};
    let text = editor.doc.text();
    let range = text.line_range(line);
    let ls = range.start;
    let block = editor.block_at(ls)?;
    if block.kind != BlockKind::Table || editor.source {
        return None;
    }
    let c = editor.doc.selection.head;
    if block.range.start <= c && c <= block.content_end {
        return None;
    }
    let fs = base;
    let g = grid(editor, block.range.start, fs, window)?;
    let ri = g.view.rows.iter().position(|r| r.line().start == ls)?;
    let theme = editor.theme.clone();
    let mut runs: Vec<Run> = Vec::new();
    let mut pieces = Vec::new();
    let mut edges = vec![px(0.)];
    let gap_run = |src: Range<usize>| Run {
        src,
        text: PLACEHOLDER.into(),
        verbatim: false,
        style: kalem_core::view::Style::default(),
        widget: None,
    };
    let gap = |w: Pixels| Piece::Widget {
        len: PLACEHOLDER.len(),
        size: size(w, px(0.)),
        ascent: px(0.),
    };
    let total: Pixels = g.widths.iter().fold(px(0.), |a, w| a + *w + g.pad * 2.);
    match &g.view.rows[ri] {
        TableRow::Rule { line } => {
            runs.push(gap_run(line.clone()));
            pieces.push(gap(total));
            edges.push(total);
        }
        TableRow::Data { line, cells } => {
            let mut at = line.start;
            let mut x = px(0.);
            for (i, w) in g.widths.iter().enumerate() {
                let (text_, truns, cell_src) = match cells.get(i) {
                    Some(cell) => {
                        let (t, r) = cell_runs(&cell.runs, ri < g.header, &theme);
                        (t, r, cell.range.clone())
                    }
                    None => (String::new(), Vec::new(), line.end..line.end),
                };
                let cw = if text_.is_empty() {
                    px(0.)
                } else {
                    window
                        .text_system()
                        .shape_line(text_.clone().into(), fs, &truns, None)
                        .width
                };
                let free = *w - cw;
                let (left, right) = match g.view.align.get(i) {
                    Some('r') => (free, px(0.)),
                    Some('c') => (free / 2., free / 2.),
                    _ => (px(0.), free),
                };
                // The bar and padding before the cell.
                runs.push(gap_run(at..cell_src.start.max(at)));
                pieces.push(gap(g.pad + left));
                if let Some(cell) = cells.get(i) {
                    runs.extend(cell.runs.iter().cloned());
                    pieces.push(Piece::Text {
                        text: text_,
                        runs: truns,
                    });
                    at = cell.range.end;
                }
                // The padding after it.
                runs.push(gap_run(at..at));
                pieces.push(gap(right + g.pad));
                x += *w + g.pad * 2.;
                edges.push(x);
            }
            runs.push(gap_run(at..line.end.max(at)));
            pieces.push(gap(px(1.)));
        }
    }
    let view = LineView {
        range: range.clone(),
        runs,
        ..LineView::default()
    };
    let rule = matches!(g.view.rows[ri], TableRow::Rule { .. });
    Some(Prepared {
        view: Rc::new(view),
        pieces,
        widgets: Vec::new(),
        font_size: fs,
        hang_at: None,
        fold: None,
        background: None,
        grid: Some((edges, rule)),
        bar: false,
        rule: false,
        nowrap: false,
        spacing: 1.,
        fit: Vec::new(),
    })
}

/// A run standing for `src` that shows as `text` (not the source).
fn stand_in(src: Range<usize>, text: &str) -> kalem_core::view::Run {
    kalem_core::view::Run {
        src,
        text: text.into(),
        verbatim: false,
        style: kalem_core::view::Style::default(),
        widget: None,
    }
}

/// Block decorations away from the cursor: the first line of a block as
/// its label (and a copy button for code), its last line as a thin gap,
/// a horizontal rule as a line.
fn prepare_decoration(
    editor: &mut Editor,
    line: usize,
    base: Pixels,
    window: &mut Window,
) -> Option<Prepared> {
    use kalem_core::view::PLACEHOLDER;
    if editor.source {
        return None;
    }
    let range = editor.doc.text().line_range(line);
    let ls = range.start;
    let block = editor.block_at(ls)?;
    let text = editor.doc.text();
    let c = editor.doc.selection.head;
    let inside = block.range.start <= c && c <= block.content_end;
    let theme = editor.theme.clone();
    let empty = |range: Range<usize>, rule: bool, fs: Pixels| Prepared {
        view: Rc::new(LineView {
            range: range.clone(),
            runs: vec![stand_in(range, PLACEHOLDER)],
            ..LineView::default()
        }),
        pieces: vec![
            Piece::Widget {
                len: PLACEHOLDER.len(),
                size: size(px(1.), px(0.)),
                ascent: px(0.),
            },
            Piece::Spacer {
                len: 0,
                min: px(0.),
            },
        ],
        widgets: Vec::new(),
        font_size: fs,
        hang_at: None,
        fold: None,
        background: None,
        grid: None,
        bar: false,
        rule,
        nowrap: false,
        spacing: 1.,
        fit: Vec::new(),
    };
    if block.kind == BlockKind::Rule && !(ls <= c && c <= range.end) {
        return Some(empty(range, true, base));
    }
    let framed = matches!(
        block.kind,
        BlockKind::Code { .. }
            | BlockKind::Export { .. }
            | BlockKind::CommentBlock
            | BlockKind::Verbatim
            | BlockKind::Quote
            | BlockKind::Center
            | BlockKind::Verse
            | BlockKind::Special
            | BlockKind::Dynamic
    );
    let src = &text.as_str()[range.clone()];
    let lower = src.trim_start().to_ascii_lowercase();
    let begin = lower.starts_with("#+begin");
    let end = lower.starts_with("#+end");
    if !framed || inside || !(begin || end) || ls >= block.content_end {
        return None;
    }
    let code = block.kind.is_code();
    let background = code.then_some(theme.code_bg);
    if end {
        let mut p = empty(range, false, base * 0.45);
        p.background = background;
        return Some(p);
    }
    let label = block.kind.label(src);
    let fs = base * 0.8;
    let mut run = text_run(
        &kalem_core::view::Style::default(),
        label.len(),
        0,
        false,
        &theme,
    );
    run.color = theme.muted;
    let mut runs = vec![stand_in(ls..ls, &label), stand_in(ls..ls, PLACEHOLDER)];
    let mut pieces = vec![
        Piece::Text {
            text: label.clone(),
            runs: vec![run.clone()],
        },
        Piece::Spacer {
            len: PLACEHOLDER.len(),
            min: fs,
        },
    ];
    let mut widgets = Vec::new();
    if code {
        let copy = kalem_core::l10n::tr("copy-button");
        let mut brun = run;
        brun.len = copy.len();
        brun.color = theme.link;
        let shaped = window
            .text_system()
            .shape_line(copy.into(), fs, &[brun], None);
        let sz = size(shaped.width + px(12.), fs * 1.3);
        runs.push(stand_in(ls..range.end, PLACEHOLDER));
        pieces.push(Piece::Widget {
            len: PLACEHOLDER.len(),
            size: sz,
            ascent: fs * 1.05,
        });
        widgets.push((
            label.len() + PLACEHOLDER.len(),
            ls..range.end,
            Widget::Checkbox(CheckState::Unchecked),
            Paint::Copy(Rc::new(shaped), block.range.start),
        ));
    } else {
        runs.push(stand_in(ls..range.end, ""));
    }
    Some(Prepared {
        view: Rc::new(LineView {
            range,
            runs,
            ..LineView::default()
        }),
        pieces,
        widgets,
        font_size: fs,
        hang_at: None,
        fold: None,
        background,
        grid: None,
        bar: false,
        rule: false,
        nowrap: false,
        spacing: 1.,
        fit: Vec::new(),
    })
}

/// The font size of a line.
fn font_size(view: &LineView, base: Pixels) -> Pixels {
    if view.runs.iter().any(|r| r.style.title) {
        return base * 1.8;
    }
    match view.heading {
        1 => base * 1.55,
        2 => base * 1.3,
        3 => base * 1.15,
        _ if view.role == LineRole::Delimiter => base * 0.85,
        _ => base,
    }
}

/// The text style of a view run.
fn text_run(
    s: &kalem_core::view::Style,
    len: usize,
    heading: u8,
    mono: bool,
    theme: &Theme,
) -> TextRun {
    let family: SharedString = if mono || s.code {
        theme.mono.clone()
    } else if let Some(f) = s.rich.font {
        f.family().to_string()
    } else {
        theme.font.clone()
    }
    .into();
    let mut font = gpui::font(family);
    if s.bold || s.title || heading > 0 || s.todo.is_some() {
        font.weight = FontWeight::BOLD;
    }
    if s.italic || s.byline || s.expansion {
        font.style = FontStyle::Italic;
    }
    let color = if s.link || s.expansion {
        theme.link
    } else if let Some(done) = s.todo {
        if done { theme.done } else { theme.todo }
    } else if s.tag || s.dim || s.cookie || s.byline || s.target {
        theme.muted
    } else if s.timestamp {
        theme.timestamp
    } else if s.priority {
        theme.priority
    } else if let Some(c) = s.rich.color {
        crate::theme::color(c)
    } else if heading > 0 {
        theme.level(heading)
    } else {
        theme.foreground
    };
    // TODO keywords, priorities and timestamps as badges.
    let badge =
        (s.todo.is_some() || s.priority || s.timestamp).then_some(Hsla { a: 0.14, ..color });
    // A highlight, a little lighter on dark themes.
    let highlight = s.rich.highlight.map(|c| {
        let h = crate::theme::color(c);
        if theme.dark { Hsla { a: 0.45, ..h } } else { h }
    });
    TextRun {
        len,
        font,
        color,
        background_color: s.code.then_some(theme.code_bg).or(badge).or(highlight),
        underline: (s.underline || s.link).then_some(UnderlineStyle {
            color: Some(color),
            thickness: px(1.),
            wavy: false,
        }),
        strikethrough: s.strike.then_some(StrikethroughStyle {
            color: Some(color),
            thickness: px(1.),
        }),
    }
}

/// A file manager line's colors and weights (`kalem_core::dired`):
/// folders bold in the function color, details muted, marks and flags in
/// their colors.
fn color_listing(
    runs: Vec<TextRun>,
    display: &str,
    styles: &[(std::ops::Range<usize>, kalem_core::dired::DirStyle)],
    theme: &Theme,
) -> Vec<TextRun> {
    use kalem_core::dired::DirStyle as D;
    use kalem_highlight::Kind as K;
    let Some(base) = runs.first().cloned() else {
        return runs;
    };
    let mut out: Vec<TextRun> = Vec::new();
    let mut at = 0;
    let mut sorted: Vec<_> = styles.to_vec();
    sorted.sort_by_key(|(r, _)| r.start);
    for (r, st) in sorted {
        let (a, b) = (r.start.min(display.len()), r.end.min(display.len()));
        if a < at || b <= a {
            continue;
        }
        if a > at {
            out.push(TextRun {
                len: a - at,
                ..base.clone()
            });
        }
        let mut run = TextRun {
            len: b - a,
            ..base.clone()
        };
        let code = |k: K| theme.code(k).unwrap_or(base.color);
        match st {
            D::Header => {
                run.color = code(K::Keyword);
                run.font.weight = FontWeight::BOLD;
            }
            D::Detail | D::Hidden | D::Note => run.color = theme.muted,
            D::Dir => {
                run.color = code(K::Function);
                run.font.weight = FontWeight::BOLD;
            }
            D::Link => run.color = code(K::Macro),
            D::Broken => run.color = theme.todo,
            D::Executable => run.color = code(K::String),
            D::Marked => {
                run.color = code(K::Constant);
                run.font.weight = FontWeight::BOLD;
            }
            D::Flagged => {
                run.color = theme.todo;
                run.font.weight = FontWeight::BOLD;
            }
        }
        out.push(run);
        at = b;
    }
    if at < display.len() {
        out.push(TextRun {
            len: display.len() - at,
            ..base
        });
    }
    out
}

/// Splits `runs` so that source code ranges get their syntax colors.
fn color_code(
    runs: Vec<TextRun>,
    display: &str,
    view: &LineView,
    spans: &[kalem_highlight::Span],
    theme: &Theme,
) -> Vec<TextRun> {
    // Code lines are shown as their source, byte for byte.
    let base = runs.first().cloned();
    let Some(base) = base else { return runs };
    let line_start = view.range.start;
    let offset = view.runs.first().map_or(line_start, |r| r.src.start) - line_start;
    let mut out = Vec::new();
    let mut at = 0;
    for sp in spans {
        let (a, b) = (
            sp.range.start.saturating_sub(offset),
            sp.range.end.saturating_sub(offset),
        );
        let (a, b) = (a.min(display.len()), b.min(display.len()));
        if a > at {
            out.push(TextRun {
                len: a - at,
                ..base.clone()
            });
        }
        if b > a {
            let color = theme.code(sp.kind).unwrap_or(base.color);
            out.push(TextRun {
                len: b - a,
                color,
                ..base.clone()
            });
        }
        at = at.max(b);
    }
    if at < display.len() {
        out.push(TextRun {
            len: display.len() - at,
            ..base
        });
    }
    out
}

/// A LaTeX environment away from the cursor: the whole environment on its
/// first line, rendered as a displayed formula (its other lines are
/// hidden, see `Editor::compute_lines`).
fn prepare_math_block(
    editor: &mut Editor,
    line: usize,
    base: Pixels,
    window: &mut Window,
) -> Option<Prepared> {
    use kalem_core::view::PLACEHOLDER;
    if editor.source || !editor.math {
        return None;
    }
    let range = editor.doc.text().line_range(line);
    let block = editor.block_at(range.start)?;
    let c = editor.doc.selection.head;
    if block.kind != BlockKind::Math
        || range.start != block.range.start
        || (block.range.start <= c && c <= block.content_end)
    {
        return None;
    }
    let src = editor.doc.text().as_str()[block.range.start..block.content_end]
        .trim_end()
        .to_string();
    let theme = editor.theme.clone();
    let macros = editor.math_macros();
    let (paint, sz, ascent) =
        match editor
            .shared
            .math
            .get(&src, &macros, base, window.scale_factor(), theme.foreground)
        {
            crate::math::Formula::Image {
                image,
                size,
                ascent,
            } => (Paint::Image(image), size, ascent),
            crate::math::Formula::Error(_) => {
                // The first line of the source, in a red frame.
                let first = src.lines().next().unwrap_or("").to_string();
                let mut run = text_run(
                    &kalem_core::view::Style::default(),
                    first.len(),
                    0,
                    true,
                    &theme,
                );
                run.color = theme.todo;
                let shaped =
                    window
                        .text_system()
                        .shape_line(first.into(), base * 0.9, &[run], None);
                let sz = size(shaped.width + px(8.), base * 1.3);
                (Paint::Error(Rc::new(shaped)), sz, base)
            }
        };
    let src_range = block.range.start..block.content_end;
    let widget = Widget::Math {
        source: src,
        display: true,
    };
    let mut run = stand_in(range.clone(), PLACEHOLDER);
    run.src = src_range.clone();
    run.widget = Some(widget.clone());
    Some(Prepared {
        view: Rc::new(LineView {
            range,
            runs: vec![run],
            ..LineView::default()
        }),
        pieces: vec![Piece::Widget {
            len: PLACEHOLDER.len(),
            size: sz,
            ascent,
        }],
        widgets: vec![(0, src_range, widget, paint)],
        font_size: base,
        hang_at: None,
        fold: None,
        background: None,
        grid: None,
        bar: false,
        rule: false,
        nowrap: false,
        spacing: 1.,
        fit: Vec::new(),
    })
}

/// A `#+TOC: headlines` line away from the cursor: the table of contents
/// the export puts there, each row leading to its heading.
fn prepare_toc(
    editor: &mut Editor,
    line: usize,
    base: Pixels,
    window: &mut Window,
) -> Option<Prepared> {
    use kalem_core::view::PLACEHOLDER;
    if editor.source {
        return None;
    }
    let range = editor.doc.text().line_range(line);
    let rows = kalem_core::toc::shown(&mut editor.doc, range.clone())?;
    let theme = editor.theme.clone();
    let shape = |t: String, color: Hsla, window: &mut Window| {
        let mut run = text_run(
            &kalem_core::view::Style::default(),
            t.len(),
            0,
            false,
            &theme,
        );
        run.color = color;
        Rc::new(
            window
                .text_system()
                .shape_line(t.into(), base, &[run], None),
        )
    };
    let title = if rows.is_empty() {
        "Contents: no headings"
    } else {
        "Contents"
    };
    let title = shape(title.to_string(), theme.muted, window);
    let rows: Vec<_> = rows
        .into_iter()
        .map(|(t, start)| (shape(t, theme.link, window), start))
        .collect();
    let row = base * 1.5;
    let width = rows
        .iter()
        .map(|(s, _)| s.width)
        .fold(title.width, |a, b| a.max(b))
        + px(16.);
    let sz = size(width, row * (rows.len() + 1) as f32 + px(8.));
    let mut run = stand_in(range.clone(), PLACEHOLDER);
    run.src = range.clone();
    Some(Prepared {
        view: Rc::new(LineView {
            range: range.clone(),
            runs: vec![run],
            ..LineView::default()
        }),
        pieces: vec![Piece::Widget {
            len: PLACEHOLDER.len(),
            size: sz,
            ascent: base,
        }],
        // Painted as rows, not hit as a widget: a click elsewhere in it goes
        // to the keyword line.
        widgets: vec![(
            0,
            range,
            Widget::TocRow { start: 0 },
            Paint::Toc(title, rows, row),
        )],
        font_size: base,
        hang_at: None,
        fold: None,
        background: None,
        grid: None,
        bar: false,
        rule: false,
        nowrap: false,
        spacing: 1.,
        fit: Vec::new(),
    })
}

fn prepare(editor: &mut Editor, line: usize, base: Pixels, window: &mut Window) -> Prepared {
    if let Some(p) = prepare_grid(editor, line, base, window) {
        return p;
    }
    if let Some(p) = prepare_toc(editor, line, base, window) {
        return p;
    }
    if let Some(p) = prepare_math_block(editor, line, base, window) {
        return p;
    }
    if let Some(p) = prepare_decoration(editor, line, base, window) {
        return p;
    }
    let theme = editor.doc_theme();
    let view = editor.line_view(line);
    let ls = view.range.start;
    let block = editor.block_at(ls);
    // The source view is plain text: one size, blanks as they are.
    let source = editor.source;
    let fs = if source { base } else { font_size(&view, base) };
    let mono = view.mono;
    let mut pieces = Vec::new();
    let mut widgets = Vec::new();
    let mut fit = Vec::new();
    let mut fit_next: Option<Option<u32>> = None;
    let (mut text, mut runs) = (String::new(), Vec::new());
    let mut at = 0;
    for (i, r) in view.runs.iter().enumerate() {
        let paint = match &r.widget {
            Some(Widget::Checkbox(c)) => {
                Some((Paint::Checkbox(*c), size(fs * 0.95, fs * 0.95), fs * 0.8))
            }
            Some(Widget::Math { source, .. }) if editor.math => {
                let macros = editor.math_macros();
                match editor.shared.math.get(
                    source,
                    &macros,
                    fs,
                    window.scale_factor(),
                    theme.foreground,
                ) {
                    crate::math::Formula::Image {
                        image,
                        size: sz,
                        ascent,
                    } => Some((Paint::Image(image), sz, ascent)),
                    crate::math::Formula::Error(_) => {
                        let mut run = text_run(&r.style, source.len(), view.heading, true, &theme);
                        run.color = theme.todo;
                        let shaped = window.text_system().shape_line(
                            source.clone().into(),
                            fs * 0.9,
                            &[run],
                            None,
                        );
                        let sz = size(shaped.width + px(8.), fs * 1.3);
                        Some((Paint::Error(Rc::new(shaped)), sz, fs))
                    }
                }
            }
            Some(Widget::Image { path, width })
                if let Some((image, w, h)) = editor.picture(path) =>
            {
                // The picture at its size, or the width `#+ATTR_ORG` asks
                // for; fitted to the text width when laid out.
                let (w, h) = (w.max(1) as f32, h.max(1) as f32);
                let tw = match width {
                    Some(kalem_core::view::ImageWidth::Pixels(p)) => *p as f32,
                    _ => w,
                };
                let th = h * tw / w;
                fit_next = Some(match width {
                    Some(kalem_core::view::ImageWidth::Percent(p)) => Some(*p),
                    _ => None,
                });
                Some((Paint::Image(image), size(px(tw), px(th)), px(th)))
            }
            Some(w @ (Widget::Math { .. } | Widget::Image { .. })) => {
                // A formula as Unicode when previews are off; an image's
                // name until images (T1.5.7).
                let (shown, color) = match w {
                    Widget::Math { source, .. } => (kalem_core::math::unicode(source), theme.link),
                    Widget::Image { path, .. } => (format!("[image: {path}]"), theme.muted),
                    Widget::Checkbox(_) | Widget::TocRow { .. } => unreachable!("handled above"),
                };
                let mut run = text_run(&r.style, shown.len(), view.heading, mono, &theme);
                run.color = color;
                if matches!(w, Widget::Math { .. }) {
                    run.font.style = FontStyle::Italic;
                }
                let shaped = window
                    .text_system()
                    .shape_line(shown.into(), fs, &[run], None);
                let sz = size(shaped.width, fs * 1.2);
                Some((Paint::Text(Rc::new(shaped)), sz, fs * 0.9))
            }
            // Only in tables of contents, prepared apart.
            Some(Widget::TocRow { .. }) | None => None,
        };
        if let Some((paint, sz, ascent)) = paint {
            if !text.is_empty() {
                pieces.push(Piece::Text {
                    text: std::mem::take(&mut text),
                    runs: std::mem::take(&mut runs),
                });
            }
            if let Some(f) = fit_next.take() {
                fit.push((pieces.len(), f));
            }
            pieces.push(Piece::Widget {
                len: r.text.len(),
                size: sz,
                ascent,
            });
            widgets.push((
                at,
                r.src.clone(),
                r.widget.clone().expect("a widget"),
                paint,
            ));
            at += r.text.len();
            continue;
        }
        // The blanks before a heading's tags push them to the right edge.
        let before_tags = !source
            && view.heading > 0
            && !r.text.is_empty()
            && r.text.bytes().all(|b| b == b' ' || b == b'\t')
            && view.runs.get(i + 1).is_some_and(|n| n.style.tag);
        if before_tags {
            if !text.is_empty() {
                pieces.push(Piece::Text {
                    text: std::mem::take(&mut text),
                    runs: std::mem::take(&mut runs),
                });
            }
            pieces.push(Piece::Spacer {
                len: r.text.len(),
                min: fs * 0.8,
            });
            at += r.text.len();
            continue;
        }
        // Superscripts and subscripts: smaller, raised or lowered.
        if r.style.superscript || r.style.subscript {
            if !text.is_empty() {
                pieces.push(Piece::Text {
                    text: std::mem::take(&mut text),
                    runs: std::mem::take(&mut runs),
                });
            }
            pieces.push(Piece::Script {
                text: r.text.clone(),
                runs: vec![text_run(&r.style, r.text.len(), view.heading, mono, &theme)],
                sup: r.style.superscript,
            });
            at += r.text.len();
            continue;
        }
        // Kalem's font sizes: text in its own size.
        if let Some(sz) = r.style.rich.size.filter(|_| !source && !mono) {
            if !text.is_empty() {
                pieces.push(Piece::Text {
                    text: std::mem::take(&mut text),
                    runs: std::mem::take(&mut runs),
                });
            }
            pieces.push(Piece::Sized {
                text: r.text.clone(),
                runs: vec![text_run(&r.style, r.text.len(), view.heading, mono, &theme)],
                size: px(f32::from(sz) / 10.) * (base / px(theme.size)),
            });
            at += r.text.len();
            continue;
        }
        runs.push(text_run(&r.style, r.text.len(), view.heading, mono, &theme));
        text.push_str(&r.text);
        at += r.text.len();
    }
    // The file manager: its own colors.
    if let Some(d) = editor.doc.dired.as_deref() {
        runs = color_listing(runs, &text, d.styles(line), &theme);
    }
    // Plain text: syntax colors of its language.
    if editor.doc.meta.mode != kalem_core::DocumentMode::Org
        && let Some((_, Some(h), _)) = editor.plain.borrow().as_ref()
    {
        runs = color_code(runs, &text, &view, h.line(line), &theme);
    }
    // Source code in a block: syntax colors.
    let code_line = block
        .as_ref()
        .is_some_and(|b| b.kind.highlight_language().is_some())
        && view.role == LineRole::Content
        && mono;
    if code_line
        && let Some(b) = &block
        && let Some((start, spans)) = editor.code_spans(b)
        && ls >= start
    {
        let text_ = editor.doc.text();
        let i = text_.line_of(ls) - text_.line_of(start);
        if let Some(sp) = spans.get(i) {
            runs = color_code(runs, &text, &view, sp, &theme);
        }
    }
    let heading_fold = (view.heading > 0 && !source).then(|| {
        let hides = block
            .as_ref()
            .is_some_and(|b| matches!(b.kind, BlockKind::Heading { .. }));
        (editor.folds.get(ls).is_some(), ls, hides)
    });
    let folded_block = !source && editor.folded_blocks.contains(&ls);
    if heading_fold.is_some_and(|(f, _, _)| f) || folded_block {
        let mut run = text_run(
            &kalem_core::view::Style::default(),
            " …".len(),
            0,
            false,
            &theme,
        );
        run.color = theme.muted;
        runs.push(run);
        text.push_str(" …");
    }
    if !text.is_empty() {
        pieces.push(Piece::Text { text, runs });
    }
    let background = match block.as_ref().map(|b| &b.kind) {
        Some(k) if k.is_code() && !editor.source => Some(theme.code_bg),
        _ => None,
    };
    let hang_at = hang_at(&view);
    let kind = block.as_ref().map(|b| b.kind.clone());
    let quote = !source
        && matches!(kind, Some(BlockKind::Quote | BlockKind::Verse))
        && view.role == LineRole::Content;
    if quote {
        for piece in &mut pieces {
            if let Piece::Text { runs, .. } = piece {
                for r in runs {
                    r.color = theme.muted;
                }
            }
        }
    }
    // Kalem's alignment: right-aligned lines start with a spacer.
    if view.align == kalem_core::rich::Align::Right && !editor.source {
        pieces.insert(
            0,
            Piece::Spacer {
                len: 0,
                min: px(0.),
            },
        );
    }
    let centered =
        view.align == kalem_core::rich::Align::Center && !matches!(kind, Some(BlockKind::Center));
    if centered && !editor.source {
        pieces.insert(
            0,
            Piece::Spacer {
                len: 0,
                min: px(0.),
            },
        );
        pieces.push(Piece::Spacer {
            len: 0,
            min: px(0.),
        });
    }
    if matches!(kind, Some(BlockKind::Center)) && view.role == LineRole::Content && !editor.source {
        pieces.insert(
            0,
            Piece::Spacer {
                len: 0,
                min: px(0.),
            },
        );
        pieces.push(Piece::Spacer {
            len: 0,
            min: px(0.),
        });
    }
    Prepared {
        view: Rc::new(view),
        pieces,
        widgets,
        font_size: fs,
        hang_at,
        fold: heading_fold.map(|(f, s, _)| (f, s)),
        background,
        grid: None,
        bar: quote,
        rule: false,
        nowrap: !editor.wrap,
        spacing: if source {
            1.
        } else {
            editor
                .doc_defaults()
                .spacing
                .map_or(1., |s| f32::from(s) / 10.)
        },
        fit,
    }
}

/// Where a line's wrapped rows start: after its indentation, and after a
/// list bullet and checkbox if it has them.
fn hang_at(view: &LineView) -> Option<usize> {
    let blank = |t: &str| !t.is_empty() && t.bytes().all(|b| b == b' ' || b == b'\t');
    let bullet = |t: &str| {
        let t = t.trim();
        matches!(t, "•" | "-" | "+" | "*")
            || (t.len() >= 2
                && (t.ends_with('.') || t.ends_with(')'))
                && t[..t.len() - 1].chars().all(|c| c.is_ascii_alphanumeric()))
    };
    let runs = &view.runs;
    let mut d = 0;
    let mut i = 0;
    let skip_blank = |i: &mut usize, d: &mut usize| {
        while *i < runs.len() && blank(&runs[*i].text) && runs[*i].widget.is_none() {
            *d += runs[*i].text.len();
            *i += 1;
        }
    };
    skip_blank(&mut i, &mut d);
    if i < runs.len() && runs[i].widget.is_none() && bullet(&runs[i].text) {
        d += runs[i].text.len();
        i += 1;
        skip_blank(&mut i, &mut d);
        let boxed = runs.get(i).is_some_and(|r| {
            matches!(r.widget, Some(Widget::Checkbox(_)))
                || matches!(r.text.as_str(), "[ ]" | "[X]" | "[x]" | "[-]")
        });
        if boxed {
            d += runs[i].text.len();
            i += 1;
            skip_blank(&mut i, &mut d);
        }
    }
    (d > 0 && i < runs.len()).then_some(d)
}

/// A source line on screen.
#[derive(Debug)]
pub struct LineElement {
    /// The editor.
    pub editor: Entity<Editor>,
    /// The source line.
    pub line: usize,
    /// In the other pane of a split view.
    pub other: bool,
}

type Shaped = (Rc<InlineLayout>, Rc<Prepared>, Pixels);

/// The layout of a line element.
pub struct LineState {
    shaped: Rc<RefCell<Option<Shaped>>>,
}

impl std::fmt::Debug for LineState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LineState").finish_non_exhaustive()
    }
}

impl IntoElement for LineElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for LineElement {
    type RequestLayoutState = LineState;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, LineState) {
        let base = px(self.editor.read(cx).doc_theme().size);
        let (line, other) = (self.line, self.other);
        let prepared = Rc::new(self.editor.update(cx, |e, _| {
            if other && e.other.is_some() {
                e.with_other(|e| prepare(e, line, base, window))
                    .expect("a split")
            } else {
                prepare(e, line, base, window)
            }
        }));
        let shaped: Rc<RefCell<Option<Shaped>>> = Rc::new(RefCell::new(None));
        let slot = shaped.clone();
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let layout_id =
            window.request_measured_layout(style, move |_known, available, window, _cx| {
                let available = match available.width {
                    AvailableSpace::Definite(w) => Some(w),
                    _ => None,
                };
                // Without wrapping, one row as long as the line, in the
                // width there is.
                let wrap = available.filter(|_| !prepared.nowrap);
                if let Some((layout, _, line_height)) = slot.borrow().as_ref()
                    && (prepared.nowrap || wrap.is_none_or(|w| layout.width == w))
                {
                    return Size {
                        width: available
                            .filter(|_| prepared.nowrap)
                            .unwrap_or(layout.width),
                        height: layout.height.max(*line_height),
                    };
                }
                let line_height = prepared.font_size * 1.45 * prepared.spacing;
                // Pictures no wider than the text, or the share of it
                // they ask for.
                let fitted = wrap.filter(|_| !prepared.fit.is_empty()).map(|w| {
                    let mut pieces = prepared.pieces.clone();
                    for &(i, share) in &prepared.fit {
                        if let Some(Piece::Widget { size, ascent, .. }) = pieces.get_mut(i) {
                            let want = share.map_or(size.width, |p| w * (p as f32 / 100.));
                            let width = want.min(w);
                            if width != size.width && size.width > px(0.) {
                                let k = width / size.width;
                                *size = gpui::size(width, size.height * k);
                                *ascent = size.height;
                            }
                        }
                    }
                    pieces
                });
                let mut layout = InlineLayout::new(
                    fitted.as_deref().unwrap_or(&prepared.pieces),
                    prepared.font_size,
                    line_height,
                    wrap,
                    prepared.hang_at,
                    window,
                );
                // Kalem's justified paragraphs (the source view has no
                // alignment).
                if prepared.view.align == kalem_core::rich::Align::Justify {
                    layout.justify();
                }
                let sz = Size {
                    width: available
                        .filter(|_| prepared.nowrap)
                        .unwrap_or(layout.width),
                    height: layout.height.max(line_height),
                };
                *slot.borrow_mut() = Some((Rc::new(layout), prepared.clone(), line_height));
                sz
            });
        (layout_id, LineState { shaped })
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut LineState,
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut LineState,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some((layout, p, line_height)) = state.shaped.borrow().clone() else {
            return;
        };
        let editor = self.editor.read(cx);
        let theme = editor.theme.clone();
        let focus = gpui::Focusable::focus_handle(editor, cx);
        let sel = editor.doc.selection;
        // Lines that do not wrap are painted scrolled sideways, and clipped.
        let shift = if p.nowrap { editor.hscroll } else { px(0.) };
        let origin = bounds.origin - point(shift, px(0.));
        let numbers = editor.line_numbers();
        let current = editor.doc.text().line_of(sel.head) == self.line;
        let plain = editor.doc.meta.mode != kalem_core::DocumentMode::Org;
        let step = editor.plain.borrow().as_ref().map_or(0, |p| p.2);
        // With Vim keys outside insert mode, a block on the character: where,
        // and where the next character starts.
        let block = editor
            .vim
            .as_ref()
            .filter(|v| v.block_cursor())
            .map(|v| v.caret(&editor.doc))
            .map(|c| (c, editor.doc.grapheme_after(c)));
        let block_sel = editor
            .vim
            .as_ref()
            .and_then(|v| v.block_ranges(&editor.doc))
            .unwrap_or_default();
        let marked = editor.marked.clone();
        let view = p.view.clone();
        let (ls, le) = (view.range.start, view.range.end);
        let marks: Vec<std::ops::Range<usize>> = {
            // Search matches, else the fields the formula at the cursor
            // refers to.
            let h = if editor.highlights.is_empty() {
                &editor.formula_refs
            } else {
                &editor.highlights
            };
            let i = h.partition_point(|r| r.end < ls);
            h[i..]
                .iter()
                .take_while(|r| r.start <= le)
                .cloned()
                .collect()
        };
        let painted = match (&editor.other, self.other) {
            (Some(o), true) => o.painted.clone(),
            _ => editor.painted.clone(),
        };
        if p.bar {
            window.paint_quad(fill(
                Bounds::new(
                    point(bounds.origin.x - px(14.), bounds.origin.y),
                    size(px(3.), bounds.size.height),
                ),
                theme.border,
            ));
        }
        if p.rule {
            window.paint_quad(fill(
                Bounds::new(
                    point(bounds.origin.x, bounds.origin.y + bounds.size.height / 2.),
                    size(bounds.size.width, px(1.)),
                ),
                theme.muted,
            ));
        }
        // Table grid lines.
        if let Some((edges, rule)) = &p.grid {
            let top = bounds.origin.y;
            let h = bounds.size.height;
            for x in edges {
                window.paint_quad(fill(
                    Bounds::new(point(bounds.origin.x + *x, top), size(px(1.), h)),
                    theme.border,
                ));
            }
            if *rule && let Some(w) = edges.last() {
                window.paint_quad(fill(
                    Bounds::new(point(bounds.origin.x, top + h / 2.), size(*w, px(1.))),
                    theme.border,
                ));
            }
        }
        if let Some(bg) = p.background {
            let wide = Bounds::new(
                point(bounds.origin.x - px(12.), bounds.origin.y),
                size(bounds.size.width + px(24.), bounds.size.height),
            );
            window.paint_quad(fill(wide, bg));
        }
        // The cursor's line in plain text and the source view.
        if current && numbers {
            let wide = Bounds::new(
                point(bounds.origin.x - px(12.), bounds.origin.y),
                size(bounds.size.width + px(24.), bounds.size.height),
            );
            window.paint_quad(fill(
                wide,
                gpui::Hsla {
                    a: 0.6,
                    ..theme.bar
                },
            ));
        }
        // The line number in the margin.
        if numbers {
            let n: SharedString = (self.line + 1).to_string().into();
            let fs = p.font_size * 0.8;
            let run = TextRun {
                len: n.len(),
                font: gpui::font(SharedString::from(theme.mono.clone())),
                color: if current {
                    theme.foreground
                } else {
                    theme.muted
                },
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window.text_system().shape_line(n, fs, &[run], None);
            let o = point(
                bounds.origin.x - px(10.) - shaped.width,
                bounds.origin.y + (line_height - fs * 1.2) / 2.,
            );
            let _ = shaped.paint(o, fs * 1.2, gpui::TextAlign::Left, None, window, cx);
        }
        let mask = gpui::ContentMask {
            bounds: Bounds::new(
                point(bounds.origin.x - px(2.), bounds.origin.y),
                size(bounds.size.width + px(10.), bounds.size.height),
            ),
        };
        window.with_content_mask(Some(mask), |window| {
            // Indentation guides in plain text.
            if plain && step > 1 {
                let text = view.display();
                let blanks = text.len() - text.trim_start_matches(' ').len();
                for col in (step..blanks).step_by(step) {
                    let x = layout.caret(col).origin.x;
                    window.paint_quad(fill(
                        Bounds::new(
                            point(origin.x + x, bounds.origin.y),
                            size(px(1.), bounds.size.height),
                        ),
                        theme.border,
                    ));
                }
            }
            // Search matches, under the selection.
            for m in &marks {
                let (a, b) = (
                    view.display_offset(m.start.max(ls)),
                    view.display_offset(m.end.min(le)),
                );
                for r in layout.range_rects(a, b) {
                    window.paint_quad(fill(Bounds::new(origin + r.origin, r.size), theme.mark));
                }
            }
            // Vim's block selection, a range per line.
            for m in block_sel.iter().filter(|m| m.end >= ls && m.start <= le) {
                let (a, b) = (
                    view.display_offset(m.start.max(ls)),
                    view.display_offset(m.end.min(le)),
                );
                for r in layout.range_rects(a, b) {
                    window.paint_quad(fill(
                        Bounds::new(origin + r.origin, r.size),
                        theme.selection,
                    ));
                }
            }
            // The selection, under the text.
            let (sa, sb) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
            if sa < sb && sa <= le && sb >= ls {
                let (a, b) = (
                    view.display_offset(sa.max(ls)),
                    view.display_offset(sb.min(le)),
                );
                let mut rects = layout.range_rects(a, b);
                if sb > le
                    && let Some(last) = rects.last_mut()
                {
                    last.size.width += px(6.);
                }
                for r in rects {
                    window.paint_quad(fill(
                        Bounds::new(origin + r.origin, r.size),
                        theme.selection,
                    ));
                }
            }
            layout.paint(origin, window, cx);
            // IME composition: underlined.
            if let Some(m) = &marked
                && m.start >= ls
                && m.end <= le
            {
                for r in
                    layout.range_rects(view.display_offset(m.start), view.display_offset(m.end))
                {
                    let y = origin.y + r.origin.y + r.size.height - px(3.);
                    window.paint_quad(fill(
                        Bounds::new(point(origin.x + r.origin.x, y), size(r.size.width, px(1.5))),
                        theme.caret,
                    ));
                }
            }
        });
        // Widgets.
        let boxes: std::collections::HashMap<usize, Bounds<Pixels>> = layout.widgets().collect();
        let mut hit = Vec::new();
        let mut buttons = Vec::new();
        let mut jumps = Vec::new();
        for (d, src, w, paint) in &p.widgets {
            let Some(b) = boxes.get(d) else { continue };
            let b = Bounds::new(origin + b.origin, b.size);
            match paint {
                Paint::Checkbox(s) => paint_checkbox(b, *s, &theme, window),
                Paint::Copy(shaped, start) => {
                    window.paint_quad(quad(
                        b,
                        px(4.),
                        theme.background,
                        px(1.),
                        theme.border,
                        Default::default(),
                    ));
                    let o = point(
                        b.origin.x + px(6.),
                        b.origin.y + (b.size.height - shaped.ascent - shaped.descent) / 2.,
                    );
                    let _ = shaped.paint(
                        o,
                        shaped.ascent + shaped.descent,
                        gpui::TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                    buttons.push((b, *start));
                    continue;
                }
                Paint::Toc(title, rows, row) => {
                    window.paint_quad(quad(
                        b,
                        px(4.),
                        gpui::transparent_black(),
                        px(1.),
                        theme.border,
                        Default::default(),
                    ));
                    let x = b.origin.x + px(8.);
                    let mut y = b.origin.y + px(4.);
                    let _ = title.paint(point(x, y), *row, gpui::TextAlign::Left, None, window, cx);
                    for (shaped, start) in rows {
                        y += *row;
                        let _ = shaped.paint(
                            point(x, y),
                            *row,
                            gpui::TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                        jumps.push((
                            Bounds::new(point(b.origin.x, y), size(b.size.width, *row)),
                            *start,
                        ));
                    }
                    continue;
                }
                Paint::Text(shaped) => {
                    let _ = shaped.paint(
                        b.origin,
                        b.size.height,
                        gpui::TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }
                Paint::Image(image) => {
                    let _ = window.paint_image(b, b, Default::default(), image.clone(), 0, false);
                }
                Paint::Error(shaped) => {
                    window.paint_quad(quad(
                        b,
                        px(3.),
                        gpui::transparent_black(),
                        px(1.),
                        theme.todo,
                        Default::default(),
                    ));
                    let o = point(b.origin.x + px(4.), b.origin.y);
                    let _ = shaped.paint(o, b.size.height, gpui::TextAlign::Left, None, window, cx);
                }
            }
            hit.push((b, src.clone(), w.clone()));
        }
        // The fold arrow of a heading, in the margin.
        let fold = p.fold.map(|(folded, start)| {
            let s = px(9.);
            let c = point(
                bounds.origin.x - px(20.),
                bounds.origin.y + line_height / 2.,
            );
            let mut path = PathBuilder::fill();
            if folded {
                path.add_polygon(
                    &[
                        point(c.x - s / 3., c.y - s / 2.),
                        point(c.x + s / 2., c.y),
                        point(c.x - s / 3., c.y + s / 2.),
                    ],
                    true,
                );
            } else {
                path.add_polygon(
                    &[
                        point(c.x - s / 2., c.y - s / 3.),
                        point(c.x + s / 2., c.y - s / 3.),
                        point(c.x, c.y + s / 2.),
                    ],
                    true,
                );
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, theme.muted);
            }
            (
                Bounds::new(point(c.x - px(10.), c.y - px(10.)), size(px(20.), px(20.))),
                start,
            )
        });
        let cursor = block.map_or(sel.head, |b| b.0);
        if ls <= cursor && cursor <= le {
            // The other pane of a split shows where the caret is, muted.
            if !self.other {
                window.handle_input(
                    &focus,
                    ElementInputHandler::new(bounds, self.editor.clone()),
                    cx,
                );
            }
            if focus.is_focused(window) {
                let mut caret = layout.caret(view.display_offset(cursor));
                let mut color = if self.other { theme.muted } else { theme.caret };
                if let Some((_, next)) = block {
                    let next = next.min(le);
                    let x = layout.caret(view.display_offset(next)).origin.x;
                    caret.size.width = if next > cursor && x > caret.origin.x {
                        x - caret.origin.x
                    } else {
                        p.font_size * 0.55
                    };
                    color.a *= 0.45;
                }
                window.paint_quad(fill(Bounds::new(origin + caret.origin, caret.size), color));
            }
        }
        painted.borrow_mut().insert(
            self.line,
            Painted {
                // Where the text is: scrolled sideways without wrapping.
                bounds: Bounds::new(origin, bounds.size),
                layout,
                view,
                widgets: hit,
                buttons,
                jumps,
                fold,
            },
        );
    }
}

fn paint_checkbox(b: Bounds<Pixels>, state: CheckState, theme: &Theme, window: &mut Window) {
    let inset = Bounds::new(
        b.origin + point(px(1.), px(1.)),
        size(b.size.width - px(2.), b.size.height - px(2.)),
    );
    let (bg, border) = match state {
        CheckState::Unchecked => (theme.background, theme.muted),
        _ => (theme.link, theme.link),
    };
    window.paint_quad(quad(inset, px(3.), bg, px(1.5), border, Default::default()));
    let w = inset.size.width;
    let o = inset.origin;
    match state {
        CheckState::Checked => {
            let mut p = PathBuilder::stroke(px(2.));
            p.move_to(o + point(w * 0.22, w * 0.52));
            p.line_to(o + point(w * 0.42, w * 0.72));
            p.line_to(o + point(w * 0.78, w * 0.3));
            if let Ok(path) = p.build() {
                window.paint_path(path, theme.background);
            }
        }
        CheckState::Partial => {
            let bar = Bounds::new(o + point(w * 0.22, w * 0.45), size(w * 0.56, w * 0.1));
            window.paint_quad(fill(bar, theme.background));
        }
        CheckState::Unchecked => {}
    }
}
