//! The view of a file a viewer plugin opened, in the terminal (design
//! §11.13, principle 7): the unit as an image through kitty, iTerm2 or
//! sixel where the terminal draws them, else in half-block cells; with
//! colors off, its text. The part of the bitmap in view is cut and scaled
//! here to the cells it covers, so zoom and pan work as in the graphical
//! editor.

use kalem_core::DocumentState;
use kalem_core::viewer::ViewerState;
use ratatui::buffer::Buffer;
use ratatui::layout::{Rect, Size};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Widget;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;

use crate::caps::Caps;

/// What the image drawn was made from: the bitmap, the cells it covers,
/// the part of it in view and the protocol.
type Key = (
    u64,
    usize,
    u8,
    Rect,
    [i32; 4],
    ratatui_image::picker::ProtocolType,
);

/// The image shown, kept between frames.
#[derive(Default)]
pub struct ViewerImage {
    shown: Option<(Key, Protocol)>,
}

impl std::fmt::Debug for ViewerImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewerImage")
            .field("shown", &self.shown.as_ref().map(|(k, _)| k.3))
            .finish()
    }
}

/// The picker for a viewer: the terminal's protocol, else half-blocks
/// when colors are on.
pub fn picker(images: Option<&Picker>, caps: &Caps) -> Option<Picker> {
    if let Some(p) = images {
        return Some(p.clone());
    }
    if caps.ascii || caps.no_color {
        return None;
    }
    Some(Picker::halfblocks())
}

/// Draws the viewer's document `doc` into `area`.
pub fn draw(
    doc: &mut DocumentState,
    image: &mut ViewerImage,
    images: Option<&Picker>,
    caps: &Caps,
    buf: &mut Buffer,
    area: Rect,
) {
    let Some(v) = doc.viewer.as_deref_mut() else {
        return;
    };
    // The bottom row says what is shown.
    let (pic, status) = if area.height > 2 {
        (
            Rect::new(area.x, area.y, area.width, area.height - 1),
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        )
    } else {
        (area, Rect::default())
    };
    let (pic, info) = if v.info && pic.width > 40 {
        let w = 32.min(pic.width / 2);
        (
            Rect::new(pic.x, pic.y, pic.width - w - 1, pic.height),
            Some(Rect::new(pic.right() - w, pic.y, w, pic.height)),
        )
    } else {
        (pic, None)
    };
    if v.is_grid() && pic.height > 3 {
        // The sheets' tabs under the grid, when there are several.
        let tabs = v.sheet_tabs();
        let tab_row = (tabs.len() > 1 && pic.height > 14)
            .then(|| Rect::new(pic.x, pic.bottom() - 1, pic.width, 1));
        let grid_h = pic.height - 1 - u16::from(tab_row.is_some());
        // The formula bar above the grid, drawn after it: the cursor may
        // move while the grid is laid out.
        let bar = Rect::new(pic.x, pic.y, pic.width, 1);
        draw_grid(v, caps, buf, Rect::new(pic.x, pic.y + 1, pic.width, grid_h));
        draw_formula_bar(v, caps, buf, bar);
        if let Some(row) = tab_row {
            draw_tabs(&tabs, v.unit, caps, buf, row);
        }
    } else {
        match picker(images, caps) {
            Some(p) => draw_image(v, image, &p, buf, pic),
            None => text(&v.text(), buf, pic),
        }
    }
    if let Some(r) = info {
        draw_info(v, caps, buf, r);
    }
    if status.height > 0 {
        let line = v.status();
        buf.set_stringn(
            status.x,
            status.y,
            line,
            status.width as usize,
            Style::default().add_modifier(Modifier::DIM),
        );
    }
}

/// The sheets' tabs: each name, the shown one reversed, a colored tab
/// with a bar of its color before the name.
fn draw_tabs(
    tabs: &[kalem_core::viewer::SheetTab],
    shown: usize,
    caps: &Caps,
    buf: &mut Buffer,
    row: Rect,
) {
    let mut x = row.x;
    for (u, name, color) in tabs {
        if x >= row.right() {
            break;
        }
        let mut st = Style::default();
        if *u == shown {
            st = st.add_modifier(Modifier::REVERSED | Modifier::BOLD);
        } else {
            st = st.add_modifier(Modifier::DIM);
        }
        if let Some([r, g, b]) = color {
            let mark = if caps.ascii { "|" } else { "▌" };
            let mut cs = Style::default();
            if !caps.no_color {
                cs = cs.fg(ratatui::style::Color::Rgb(*r, *g, *b));
            }
            buf.set_stringn(x, row.y, mark, 1, cs);
            x += 1;
        }
        let label = format!(" {name} ");
        let room = (row.right() - x) as usize;
        let (nx, _) = buf.set_stringn(x, row.y, &label, room, st);
        x = nx + 1;
    }
}

/// A column's width in terminal cells: Excel's width in characters.
fn col_cells(layout: &kalem_viewer::GridLayout, col: u32) -> u16 {
    let w = layout
        .widths
        .get(col as usize)
        .copied()
        .unwrap_or(layout.default_width);
    (w.round() as u16).clamp(3, 40)
}

/// The rows or columns in view: the frozen ones, then from `first` on,
/// hidden ones skipped, until `room` runs out.
fn in_view(
    frozen: u32,
    first: u32,
    max: u32,
    hidden: &[u32],
    room: u16,
    size: impl Fn(u32) -> u16,
) -> Vec<(u32, u16)> {
    let mut out = Vec::new();
    let mut used = 0u16;
    let mut push = |i: u32, out: &mut Vec<(u32, u16)>| -> bool {
        if hidden.contains(&i) {
            return true;
        }
        let w = size(i);
        if used >= room {
            return false;
        }
        out.push((i, w.min(room - used)));
        used = used.saturating_add(w);
        true
    };
    for i in 0..frozen.min(max) {
        if !push(i, &mut out) {
            return out;
        }
    }
    let mut i = first.max(frozen);
    while i < max && push(i, &mut out) {
        i += 1;
    }
    out
}

/// The cursor's cell and what it holds, in full: a spreadsheet's formula bar.
fn draw_formula_bar(v: &mut ViewerState, caps: &Caps, buf: &mut Buffer, bar: Rect) {
    let name = v.selection_name();
    let input: String = v
        .cell_input()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let name_w = (name.len() + 1).max(6);
    let head = Style::default().add_modifier(Modifier::BOLD);
    buf.set_stringn(bar.x, bar.y, format!("{name:<name_w$}"), name_w, head);
    let sep = if caps.ascii { "|" } else { "│" };
    buf.set_stringn(
        bar.x + name_w as u16,
        bar.y,
        format!("{sep} {input}"),
        (bar.width as usize).saturating_sub(name_w),
        Style::default(),
    );
}

/// A grid unit (a sheet): letters above, row numbers at the left, the
/// cursor's cell reversed.
fn draw_grid(v: &mut ViewerState, caps: &Caps, buf: &mut Buffer, area: Rect) {
    let Some(layout) = v.grid_layout() else {
        return;
    };
    if area.width < 8 || area.height < 3 {
        return;
    }
    let pos = v.grid_pos();
    let gutter = ((pos.top + u32::from(area.height))
        .max(layout.rows)
        .to_string()
        .len() as u16
        + 1)
    .max(4);
    let room_w = area.width.saturating_sub(gutter);
    let room_h = area.height.saturating_sub(1);
    let cols = in_view(
        layout.frozen.1,
        pos.left,
        layout.max_cols,
        &layout.hidden_cols,
        room_w,
        |c| col_cells(&layout, c) + 1,
    );
    let rows = in_view(
        layout.frozen.0,
        pos.top,
        layout.max_rows,
        &layout.hidden_rows,
        room_h,
        |_| 1,
    );
    // Fully shown ones count for paging and keeping the cursor in view.
    let full_cols = cols
        .iter()
        .filter(|(c, w)| *w > col_cells(&layout, *c))
        .count() as u32;
    v.set_grid_visible(rows.len() as u32, full_cols.max(1));
    // The cursor may have scrolled the view: lay out again if so.
    if v.grid_pos() != pos {
        return draw_grid(v, caps, buf, area);
    }
    let dim = Style::default().add_modifier(Modifier::DIM);
    let head = Style::default().add_modifier(Modifier::BOLD);
    // The cells in view.
    let mut cells = std::collections::HashMap::new();
    let mut invalid = std::collections::HashSet::new();
    let has_list = v.cursor_has_list();
    let ranges = |list: &[(u32, u16)], frozen: u32| -> Vec<std::ops::Range<u32>> {
        let mut r = Vec::new();
        let f: Vec<u32> = list.iter().map(|x| x.0).filter(|&i| i < frozen).collect();
        let s: Vec<u32> = list.iter().map(|x| x.0).filter(|&i| i >= frozen).collect();
        for part in [f, s] {
            if let (Some(a), Some(b)) = (part.first(), part.last()) {
                r.push(*a..*b + 1);
            }
        }
        r
    };
    for rr in ranges(&rows, layout.frozen.0) {
        for cr in ranges(&cols, layout.frozen.1) {
            for (r, c, cell) in v.grid_cells(rr.clone(), cr.clone()) {
                cells.insert((r, c), cell);
            }
            invalid.extend(v.invalid_cells(rr.clone(), cr));
        }
    }
    // Letters.
    buf.set_stringn(
        area.x,
        area.y,
        " ".repeat(gutter as usize),
        gutter as usize,
        dim,
    );
    let mut x = area.x + gutter;
    for &(c, w) in &cols {
        let name = kalem_core::csv_tools::column_letters(c as usize);
        let style = if c == pos.col {
            head.add_modifier(Modifier::REVERSED)
        } else {
            head
        };
        let label = format!("{name:^width$}", width = w.saturating_sub(1) as usize);
        buf.set_stringn(x, area.y, &label, w.saturating_sub(1) as usize, style);
        x += w;
    }
    let sep = if caps.ascii { "|" } else { "│" };
    let sel = v.selection();
    let selecting = v.grid_pos().sel.is_some();
    let cut = v.cut_range();
    // Go To Special's ranges, selected together.
    let areas = v.areas.clone();
    // The cells a formula being typed points at.
    let pointer = v.pointer;
    // Trace Precedents' and Dependents' ends: the ranges read, the cells
    // reading them.
    let arrows = v.arrows.clone();
    // The outline's summary rows: − to collapse, + to expand.
    let marks = v.outline_marks();
    let merged = layout.merged.clone();
    let merge_of = |r: u32, c: u32| {
        merged
            .iter()
            .find(|m| (m[0]..=m[2]).contains(&r) && (m[1]..=m[3]).contains(&c))
            .copied()
    };
    for (i, &(r, _)) in rows.iter().enumerate() {
        let y = area.y + 1 + i as u16;
        let style = if r == pos.row {
            head.add_modifier(Modifier::REVERSED)
        } else {
            dim
        };
        buf.set_stringn(
            area.x,
            y,
            format!("{:>w$} ", r + 1, w = gutter as usize - 1),
            gutter as usize,
            style,
        );
        if let Some((_, collapsed)) = marks.iter().find(|m| m.0 == r) {
            let mark = match (caps.ascii, collapsed) {
                (_, true) => "+",
                (true, false) => "-",
                (false, false) => "−",
            };
            buf.set_stringn(area.x, y, mark, 1, head);
        }
        let mut x = area.x + gutter;
        let mut overflow: Option<(String, Style)> = None;
        // Center Across Selection: a cell's text over the empty cells at
        // its right that have it too, their column lines left out.
        let mut across: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        let mut no_line: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let mut under: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for (k, &(c, w)) in cols.iter().enumerate() {
            let Some(cell) = cells.get(&(r, c)) else {
                continue;
            };
            if !cell.center_across || cell.text.is_empty() {
                continue;
            }
            let mut total = w as usize;
            let mut last = c;
            for &(c2, w2) in &cols[k + 1..] {
                match cells.get(&(r, c2)) {
                    Some(x) if x.center_across && x.text.is_empty() => {
                        no_line.insert(last);
                        under.insert(c2);
                        total += w2 as usize;
                        last = c2;
                    }
                    _ => break,
                }
            }
            if last != c {
                across.insert(c, total - 1);
            }
        }
        for &(c, w) in &cols {
            let merge = merge_of(r, c);
            // A merged cell is drawn by its first cell over the whole width
            // shown; the others hold nothing of their own.
            let first = merge.is_some_and(|m| (m[0], m[1]) == (r, c));
            let covered = merge.is_some() && !first;
            let next_in_merge =
                merge.is_some_and(|m| c < m[3] && cols.iter().any(|(cc, _)| *cc == c + 1));
            let in_sel =
                (selecting && (sel[0]..=sel[2]).contains(&r) && (sel[1]..=sel[3]).contains(&c))
                    || areas
                        .iter()
                        .any(|m| (m[0]..=m[2]).contains(&r) && (m[1]..=m[3]).contains(&c));
            let in_cut =
                cut.is_some_and(|m| (m[0]..=m[2]).contains(&r) && (m[1]..=m[3]).contains(&c));
            let sel_style = |st: Style| {
                // Cut cells are underlined until they are pasted.
                let st = if in_cut {
                    st.add_modifier(Modifier::UNDERLINED)
                } else {
                    st
                };
                if !in_sel {
                    st
                } else if caps.no_color {
                    st.add_modifier(Modifier::UNDERLINED)
                } else {
                    st.bg(ratatui::style::Color::DarkGray)
                }
            };
            if covered {
                if in_sel
                    && merge.is_some_and(|m| m[0] != r || !cols.iter().any(|(cc, _)| *cc == m[1]))
                {
                    buf.set_stringn(
                        x,
                        y,
                        " ".repeat(w as usize),
                        w as usize,
                        sel_style(Style::default()),
                    );
                }
                if !next_in_merge {
                    buf.set_stringn(x + w.saturating_sub(1), y, sep, 1, dim);
                }
                x += w;
                continue;
            }
            let inner = if first {
                let m = merge.unwrap_or_default();
                let span: u16 = cols
                    .iter()
                    .filter(|(cc, _)| (m[1]..=m[3]).contains(cc) && *cc >= c)
                    .map(|(_, ww)| *ww)
                    .sum();
                span.saturating_sub(1) as usize
            } else {
                w.saturating_sub(1) as usize
            };
            let cell = cells.get(&(r, c));
            // An icon set's icon takes the cell's first two columns.
            let icon_w = if cell.is_some_and(|c| c.icon.is_some()) && inner > 2 {
                2
            } else {
                0
            };
            let mut style = Style::default();
            let text = match cell {
                Some(cell) => {
                    overflow = None;
                    let inner = inner - icon_w;
                    if cell.bold {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    if cell.italic {
                        style = style.add_modifier(Modifier::ITALIC);
                    }
                    if cell.underline {
                        style = style.add_modifier(Modifier::UNDERLINED);
                    }
                    if cell.strike {
                        style = style.add_modifier(Modifier::CROSSED_OUT);
                    }
                    if !caps.no_color {
                        if let Some([r, g, b]) = cell.color {
                            style = style.fg(ratatui::style::Color::Rgb(r, g, b));
                        }
                        if let Some([r, g, b]) = cell.fill {
                            style = style.bg(ratatui::style::Color::Rgb(r, g, b));
                        }
                    }
                    let t: String = cell.text.chars().filter(|ch| !ch.is_control()).collect();
                    let len = t.chars().count();
                    let right = matches!(cell.align, kalem_viewer::Align::Right)
                        || (cell.numeric && matches!(cell.align, kalem_viewer::Align::General));
                    let center = matches!(cell.align, kalem_viewer::Align::Center);
                    if len > inner {
                        if cell.numeric {
                            "#".repeat(inner)
                        } else {
                            let shown: String = t.chars().take(inner).collect();
                            overflow = Some((t.chars().skip(inner + 1).collect(), style));
                            shown
                        }
                    } else if right {
                        format!("{t:>inner$}")
                    } else if center {
                        format!("{t:^inner$}")
                    } else if cell.indent > 0 {
                        // Indented: two columns a level.
                        let pad = " ".repeat(usize::from(cell.indent) * 2);
                        format!("{pad}{t}").chars().take(inner).collect()
                    } else {
                        t
                    }
                }
                // Text runs on into empty cells, as in a spreadsheet.
                None => match overflow.take() {
                    Some((rest, st)) if !rest.is_empty() => {
                        style = st;
                        let shown: String = rest.chars().take(inner).collect();
                        if rest.chars().count() > inner + 1 {
                            overflow = Some((rest.chars().skip(inner + 1).collect(), st));
                        }
                        shown
                    }
                    _ => String::new(),
                },
            };
            style = sel_style(style);
            if pointer.is_some_and(|m| (m[0]..=m[2]).contains(&r) && (m[1]..=m[3]).contains(&c)) {
                style = style.add_modifier(Modifier::UNDERLINED | Modifier::BOLD);
                if !caps.no_color {
                    style = style.fg(ratatui::style::Color::Cyan);
                }
            }
            let read = arrows
                .iter()
                .any(|(m, _)| (m[0]..=m[2]).contains(&r) && (m[1]..=m[3]).contains(&c));
            let reading = arrows.iter().any(|(_, d)| *d == (r, c));
            if read || reading {
                style = style.add_modifier(if reading {
                    Modifier::BOLD
                } else {
                    Modifier::UNDERLINED
                });
                if !caps.no_color {
                    style = style.fg(ratatui::style::Color::Blue);
                }
            }
            if (r, c) == (pos.row, pos.col) {
                style = style.add_modifier(Modifier::REVERSED);
            }
            if in_sel || (r, c) == (pos.row, pos.col) || first {
                buf.set_stringn(x, y, " ".repeat(inner), inner, style);
            }
            match (across.get(&c), cell) {
                (Some(&total), Some(cell)) => {
                    let t: String = cell.text.chars().filter(|ch| !ch.is_control()).collect();
                    let shown = format!("{t:^total$}");
                    buf.set_stringn(x, y, &shown, total, style);
                }
                // The cells it runs over draw nothing of their own.
                _ if under.contains(&c) => {}
                _ => {
                    buf.set_stringn(x + icon_w as u16, y, &text, inner - icon_w, style);
                }
            }
            if let Some(cell) = cell
                && (r, c) != (pos.row, pos.col)
                && !in_sel
            {
                if let Some((glyph, [cr, cg, cb])) = &cell.icon
                    && icon_w > 0
                {
                    let g = if caps.ascii {
                        ascii_icon(glyph)
                    } else {
                        glyph.as_str()
                    };
                    let mut st = Style::default();
                    if !caps.no_color {
                        st = st.fg(ratatui::style::Color::Rgb(*cr, *cg, *cb));
                    }
                    buf.set_stringn(x, y, g, 1, st);
                }
                // A data bar: the cell's background over its share of the width.
                if let Some((len, [br, bg, bb])) = cell.bar
                    && !caps.no_color
                {
                    let n = (inner * usize::from(len)).div_ceil(1000).min(inner);
                    for i in 0..n {
                        buf[(x + i as u16, y)].set_bg(ratatui::style::Color::Rgb(br, bg, bb));
                    }
                }
            }
            // A sparkline: a block a point (as many as fit), its height the
            // point's; win/loss as upper and lower halves.
            if let Some(line) = cell.and_then(|c| c.sparkline.as_ref())
                && inner > 0
                && !line.points.is_empty()
            {
                let n = line.points.len().min(inner);
                for i in 0..n {
                    let k = i * line.points.len() / n;
                    let Some(v) = line.points[k] else { continue };
                    let below = line.zero.is_some_and(|z| v < z);
                    let sym = match (line.kind, caps.ascii) {
                        (kalem_viewer::SparklineKind::WinLoss, _) if v == 500 => continue,
                        (kalem_viewer::SparklineKind::WinLoss, true) => {
                            if v > 500 {
                                "+"
                            } else {
                                "-"
                            }
                        }
                        (kalem_viewer::SparklineKind::WinLoss, false) => {
                            if v > 500 {
                                "▀"
                            } else {
                                "▄"
                            }
                        }
                        (_, true) => ["_", ".", "-", "=", "^"][usize::from(v.min(999)) * 5 / 1000],
                        (_, false) => ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"]
                            [usize::from(v.min(999)) * 8 / 1000],
                    };
                    let marked = Some(k) == line.high || Some(k) == line.low || below;
                    let mut st = style;
                    if !caps.no_color {
                        let [cr, cg, cb] = if marked { line.marker } else { line.color };
                        st = st.fg(ratatui::style::Color::Rgb(cr, cg, cb));
                    } else if marked {
                        st = st.add_modifier(Modifier::BOLD);
                    }
                    buf.set_stringn(x + i as u16, y, sym, 1, st);
                }
            }
            // A filter's header: its button, filled when the column filters.
            if let Some(f) = layout.filter
                && r == f[0]
                && (f[1]..=f[3]).contains(&c)
                && inner > 0
            {
                let on = layout.filtered.contains(&c);
                let mark = match (caps.ascii, on) {
                    (true, true) => "V",
                    (true, false) => "v",
                    (false, true) => "▼",
                    (false, false) => "▾",
                };
                buf[(x + inner as u16 - 1, y)].set_symbol(mark);
            }
            if invalid.contains(&(r, c)) && inner > 0 {
                // Circle Invalid Data: the cell between red brackets.
                let red = if caps.no_color {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(ratatui::style::Color::Rgb(0xE0, 0x3C, 0x31))
                };
                let (open, close) = if caps.ascii {
                    ("(", ")")
                } else {
                    ("⦅", "⦆")
                };
                buf.set_stringn(x, y, open, 1, red);
                if inner > 1 {
                    buf.set_stringn(x + inner as u16 - 1, y, close, 1, red);
                }
            }
            if cell.is_some_and(|c| c.note) && inner > 0 {
                buf[(x + inner as u16 - 1, y)].set_symbol(if caps.ascii { "*" } else { "◥" });
            }
            // A threaded comment: a purple diamond in the corner.
            if cell.is_some_and(|c| c.thread) && inner > 0 {
                let at = &mut buf[(x + inner as u16 - 1, y)];
                at.set_symbol(if caps.ascii { "#" } else { "◆" });
                if !caps.no_color {
                    at.set_fg(ratatui::style::Color::Rgb(0x70, 0x30, 0xA0));
                }
            }
            // A list validation's drop-down button, over a note's mark.
            if (r, c) == (pos.row, pos.col) && has_list && inner > 0 {
                buf[(x + inner as u16 - 1, y)].set_symbol(if caps.ascii { "v" } else { "▾" });
            }
            if (!first || !next_in_merge) && !no_line.contains(&c) {
                buf.set_stringn(x + inner as u16, y, sep, 1, dim);
            }
            // Borders: a side as the line beside the cell in its color, the
            // bottom as the cell underlined.
            let side = |k: Option<&kalem_viewer::GridCell>, i: usize| {
                k.and_then(|k| k.borders[i].map(|col| (col, k.border_thick[i])))
            };
            let line = |(col, thick): ([u8; 3], bool)| {
                let sym = match (caps.ascii, thick) {
                    (true, _) => "|",
                    (false, true) => "┃",
                    (false, false) => "│",
                };
                // Automatic (black) in the text's color, seen on any theme.
                let st = match col {
                    _ if caps.no_color => Style::default().add_modifier(Modifier::BOLD),
                    [0, 0, 0] => Style::default(),
                    [r, g, b] => Style::default().fg(ratatui::style::Color::Rgb(r, g, b)),
                };
                (sym, st)
            };
            if !first || !next_in_merge {
                let right = side(cell, 1).or_else(|| side(cells.get(&(r, c + 1)), 3));
                if let Some(b) = right {
                    let (sym, st) = line(b);
                    buf.set_stringn(x + inner as u16, y, sym, 1, st);
                }
            }
            if cols.first().is_some_and(|f| f.0 == c)
                && x > area.x
                && let Some(b) = side(cell, 3)
            {
                let (sym, st) = line(b);
                buf.set_stringn(x - 1, y, sym, 1, st);
            }
            if side(cell, 2).is_some() || side(cells.get(&(r + 1, c)), 0).is_some() {
                for i in 0..inner as u16 {
                    buf[(x + i, y)].modifier.insert(Modifier::UNDERLINED);
                }
            }
            x += w;
        }
    }
    // Charts, pictures and shapes over the cells they cover.
    let charts = v.charts();
    let drawings = v.drawings();
    if charts.is_empty() && drawings.is_empty() {
        return;
    }
    let x0 = area.x + gutter;
    let mut col_x = std::collections::HashMap::new();
    let mut x = x0;
    for &(c, w) in &cols {
        col_x.insert(c, (x, w));
        x += w;
    }
    let row_y: std::collections::HashMap<u32, u16> = rows
        .iter()
        .enumerate()
        .map(|(i, (r, _))| (*r, area.y + 1 + i as u16))
        .collect();
    let rect_of = |a: [u32; 4]| -> Option<Rect> {
        let xs: Vec<(u16, u16)> = (a[1]..=a[3])
            .filter_map(|c| col_x.get(&c).copied())
            .collect();
        let ys: Vec<u16> = (a[0]..=a[2])
            .filter_map(|r| row_y.get(&r).copied())
            .collect();
        let (first_x, last_x, first_y, last_y) = (xs.first()?, xs.last()?, ys.first()?, ys.last()?);
        Some(
            Rect::new(
                first_x.0,
                *first_y,
                (last_x.0 + last_x.1).saturating_sub(first_x.0),
                (last_y + 1).saturating_sub(*first_y),
            )
            .intersection(area),
        )
    };
    for chart in &charts {
        if let Some(rect) = rect_of(chart.anchor)
            && rect.width >= 6
            && rect.height >= 3
        {
            crate::chart::draw(chart, caps, rect, buf);
        }
    }
    for d in &drawings {
        if let Some(rect) = rect_of(d.anchor)
            && rect.width >= 3
            && rect.height >= 2
        {
            draw_drawing(d, caps, rect, buf);
        }
    }
}

/// A picture or shape in a terminal: a box (in the shape's colors), its
/// text inside, a picture's name.
fn draw_drawing(d: &kalem_viewer::Drawing, caps: &Caps, rect: Rect, buf: &mut Buffer) {
    use kalem_viewer::DrawingKind;
    let rgb = |c: [u8; 3]| ratatui::style::Color::Rgb(c[0], c[1], c[2]);
    let (fill, line, text) = match &d.kind {
        DrawingKind::Picture => (
            None,
            None,
            if caps.ascii {
                format!("[{}]", d.name)
            } else {
                format!("▣ {}", d.name)
            },
        ),
        DrawingKind::Shape {
            fill, line, text, ..
        } => (*fill, *line, text.clone()),
    };
    let mut style = Style::default();
    if !caps.no_color {
        if let Some(f) = fill {
            style = style.bg(rgb(f));
        }
        if let Some(l) = line {
            style = style.fg(rgb(l));
        }
    }
    let (h, v, corners) = if caps.ascii {
        ("-", "|", ["+", "+", "+", "+"])
    } else {
        ("─", "│", ["┌", "┐", "└", "┘"])
    };
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            let top = y == rect.y;
            let bottom = y + 1 == rect.y + rect.height;
            let left = x == rect.x;
            let right = x + 1 == rect.x + rect.width;
            let sym = match (top, bottom, left, right) {
                (true, _, true, _) => corners[0],
                (true, _, _, true) => corners[1],
                (_, true, true, _) => corners[2],
                (_, true, _, true) => corners[3],
                (true, _, _, _) | (_, true, _, _) => h,
                (_, _, true, _) | (_, _, _, true) => v,
                _ => " ",
            };
            buf[(x, y)].set_symbol(sym).set_style(style);
        }
    }
    let inner = rect.width.saturating_sub(2) as usize;
    let mut text_style = Style::default();
    if !caps.no_color
        && let Some(f) = fill
    {
        text_style = text_style.bg(rgb(f));
    }
    for (i, line) in text
        .lines()
        .take(rect.height.saturating_sub(2) as usize)
        .enumerate()
    {
        let shown = format!("{line:^inner$}");
        buf.set_stringn(rect.x + 1, rect.y + 1 + i as u16, shown, inner, text_style);
    }
}

/// An icon set's icon in a terminal without Unicode symbols.
fn ascii_icon(glyph: &str) -> &'static str {
    match glyph {
        "▲" | "↗" | "★" | "✔" => "^",
        "▼" | "↘" | "✖" => "v",
        "►" | "▬" | "⯪" | "◑" => "-",
        "!" => "!",
        _ => "o",
    }
}

fn text(s: &str, buf: &mut Buffer, area: Rect) {
    for (i, line) in s.lines().take(area.height as usize).enumerate() {
        buf.set_stringn(
            area.x,
            area.y + i as u16,
            line,
            area.width as usize,
            Style::default(),
        );
    }
}

fn draw_info(v: &ViewerState, caps: &Caps, buf: &mut Buffer, r: Rect) {
    let rule = crate::panels::panel_style(caps);
    for y in r.top()..r.bottom() {
        buf[(r.x - 1, y)]
            .set_symbol(if caps.ascii { "|" } else { "│" })
            .set_style(rule);
    }
    let mut y = r.y;
    for f in v.info_fields() {
        if y + 1 >= r.bottom() {
            break;
        }
        buf.set_stringn(
            r.x + 1,
            y,
            &f.label,
            r.width as usize - 1,
            Style::default().add_modifier(Modifier::DIM),
        );
        buf.set_stringn(
            r.x + 1,
            y + 1,
            &f.value,
            r.width as usize - 1,
            Style::default(),
        );
        y += 2;
    }
}

fn draw_image(
    v: &mut ViewerState,
    image: &mut ViewerImage,
    picker: &Picker,
    buf: &mut Buffer,
    area: Rect,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let fs = picker.font_size();
    let (cw, ch) = (f32::from(fs.width.max(1)), f32::from(fs.height.max(1)));
    let (aw, ah) = (f32::from(area.width) * cw, f32::from(area.height) * ch);
    v.set_area(aw, ah);
    let p = v.placement();
    let (sx, sy, sw, sh) = p.visible(aw, ah);
    if sw <= 0.0 || sh <= 0.0 {
        return;
    }
    // The cells the visible part covers.
    let (x, y) = (p.x.max(0.0), p.y.max(0.0));
    let (w, h) = (sw * p.scale, sh * p.scale);
    let col = (x / cw).floor() as u16;
    let row = (y / ch).floor() as u16;
    let cols = ((w / cw).round() as u16).clamp(1, area.width - col.min(area.width - 1));
    let rows = ((h / ch).round() as u16).clamp(1, area.height - row.min(area.height - 1));
    let cells = Rect::new(area.x + col, area.y + row, cols, rows);
    let key = (
        v.generation(),
        v.unit,
        v.rotation,
        cells,
        [sx, sy, sw, sh].map(|n| n.round() as i32),
        picker.protocol_type(),
    );
    if image.shown.as_ref().is_none_or(|(k, _)| *k != key) {
        image.shown = make(v, picker, cells, (sx, sy, sw, sh), (cw, ch)).map(|p| (key, p));
    }
    if let Some((_, proto)) = &image.shown {
        ratatui_image::Image::new(proto).render(cells, buf);
    }
}

/// The part (`sx`, `sy`, `sw`, `sh`) of the unit, in its pixels at scale
/// 1, scaled to `cells`.
fn make(
    v: &mut ViewerState,
    picker: &Picker,
    cells: Rect,
    (sx, sy, sw, sh): (f32, f32, f32, f32),
    (cw, ch): (f32, f32),
) -> Option<Protocol> {
    let b = v.bitmap().ok()?;
    // A page is rendered at the scale shown: its bitmap has more pixels
    // than the unit.
    let k = b.width as f32 / v.unit_size().0.max(1.0);
    let (sx, sy, sw, sh) = (sx * k, sy * k, sw * k, sh * k);
    let img = image::RgbaImage::from_raw(b.width, b.height, b.rgba.to_vec())?;
    let (x, y) = (sx.floor() as u32, sy.floor() as u32);
    let w = (sw.ceil() as u32).clamp(1, b.width.saturating_sub(x).max(1));
    let h = (sh.ceil() as u32).clamp(1, b.height.saturating_sub(y).max(1));
    let part = image::imageops::crop_imm(&img, x, y, w, h).to_image();
    let (pw, ph) = (
        (f32::from(cells.width) * cw) as u32,
        (f32::from(cells.height) * ch) as u32,
    );
    let scaled = image::imageops::resize(
        &part,
        pw.max(1),
        ph.max(1),
        image::imageops::FilterType::Triangle,
    );
    picker
        .new_protocol(
            image::DynamicImage::ImageRgba8(scaled),
            Size::new(cells.width, cells.height),
            ratatui_image::Resize::Fit(None),
        )
        .ok()
}
