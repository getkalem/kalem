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
        // The formula bar above the grid, drawn after it: the cursor may
        // move while the grid is laid out.
        let bar = Rect::new(pic.x, pic.y, pic.width, 1);
        draw_grid(
            v,
            caps,
            buf,
            Rect::new(pic.x, pic.y + 1, pic.width, pic.height - 1),
        );
        draw_formula_bar(v, caps, buf, bar);
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
            for (r, c, cell) in v.grid_cells(rr.clone(), cr) {
                cells.insert((r, c), cell);
            }
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
        let mut x = area.x + gutter;
        let mut overflow: Option<(String, Style)> = None;
        for &(c, w) in &cols {
            let merge = merge_of(r, c);
            // A merged cell is drawn by its first cell over the whole width
            // shown; the others hold nothing of their own.
            let first = merge.is_some_and(|m| (m[0], m[1]) == (r, c));
            let covered = merge.is_some() && !first;
            let next_in_merge =
                merge.is_some_and(|m| c < m[3] && cols.iter().any(|(cc, _)| *cc == c + 1));
            let in_sel =
                selecting && (sel[0]..=sel[2]).contains(&r) && (sel[1]..=sel[3]).contains(&c);
            let sel_style = |st: Style| {
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
            let mut style = Style::default();
            let text = match cell {
                Some(cell) => {
                    overflow = None;
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
            if (r, c) == (pos.row, pos.col) {
                style = style.add_modifier(Modifier::REVERSED);
            }
            if in_sel || (r, c) == (pos.row, pos.col) || first {
                buf.set_stringn(x, y, " ".repeat(inner), inner, style);
            }
            buf.set_stringn(x, y, &text, inner, style);
            if cell.is_some_and(|c| c.note) && inner > 0 {
                buf[(x + inner as u16 - 1, y)].set_symbol(if caps.ascii { "*" } else { "◥" });
            }
            if !first || !next_in_merge {
                buf.set_stringn(x + inner as u16, y, sep, 1, dim);
            }
            x += w;
        }
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
