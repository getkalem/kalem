//! The view of a file a viewer plugin opened (design §11.13): the unit's
//! bitmap where `kalem_core::viewer` places it, panned by dragging and the
//! wheel, zoomed with the wheel and Command or Control at the mouse, the
//! information panel at the right, frames played on a timer.

use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Bounds, Context, Corners, Div, InteractiveElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, RenderImage, ScrollWheelEvent,
    SharedString, Styled, Task, Window, div, point, px, size,
};

use crate::editor::Editor;

/// The textures shown, by generation, unit, turn and the scale they were
/// rendered at.
type Key = (u64, usize, u8, u32);

/// The view's own state, kept by the editor.
#[derive(Default)]
pub struct ViewerView {
    /// The bitmaps as gpui images: the unit shown, or every frame of an
    /// animation.
    images: Vec<(Key, Arc<RenderImage>)>,
    /// Where a drag started, or last moved to.
    drag: Option<Point<Pixels>>,
    /// Where the button went down: released near it, it is a click.
    press: Option<Point<Pixels>>,
    /// The pointer is over a link.
    over_link: bool,
    /// The area as last laid out.
    pub bounds: Option<Bounds<Pixels>>,
    /// The next frame's timer.
    timer: Option<Task<()>>,
    /// The timer that looks again while a page renders.
    render_poll: Option<Task<()>>,
    /// A column's edge being dragged in a grid.
    col_drag: Option<ColDrag>,
    /// A row's edge being dragged in a grid.
    row_drag: Option<RowDrag>,
    /// Cells are being selected by dragging.
    selecting: bool,
}

/// A grid row resized by dragging its number's bottom edge.
#[derive(Debug, Clone, Copy)]
struct RowDrag {
    /// The row.
    row: u32,
    /// The pointer's y where the drag began.
    start_y: f32,
    /// The row's height then, in pixels.
    start_px: f32,
    /// Its height now, as drawn.
    px: f32,
}

/// A grid column resized by dragging its letter's right edge.
#[derive(Debug, Clone, Copy)]
struct ColDrag {
    /// The column.
    col: u32,
    /// The pointer's x where the drag began.
    start_x: f32,
    /// The column's width then, in pixels.
    start_px: f32,
    /// Its width now, as drawn.
    px: f32,
}

impl std::fmt::Debug for ViewerView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewerView")
            .field("images", &self.images.len())
            .field("bounds", &self.bounds)
            .finish_non_exhaustive()
    }
}

/// A bitmap as a gpui image (which wants BGRA).
fn render_image(b: &kalem_viewer::Bitmap) -> Option<Arc<RenderImage>> {
    let mut bgra = b.rgba.to_vec();
    for p in bgra.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    let buf = image::RgbaImage::from_raw(b.width, b.height, bgra)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buf)])))
}

impl Editor {
    /// The image for the unit shown, made once.
    fn viewer_image(
        &mut self,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<Arc<RenderImage>, String> {
        let Some(v) = self.doc.viewer.as_deref_mut() else {
            return Err(String::new());
        };
        v.set_pixel_ratio(window.scale_factor());
        // A thread's render that ended is taken, and the next neighbor
        // started, even when the page shown is already drawn.
        if v.rendering() {
            v.bitmap_now()?;
        }
        let want = (
            v.generation(),
            v.unit,
            v.rotation,
            v.render_scale().to_bits(),
        );
        if let Some((_, img)) = self.viewer_view.images.iter().find(|(k, _)| *k == want) {
            return Ok(img.clone());
        }
        // A page renders on a thread: until it is done, the same page at
        // another scale (stretched), or nothing for a page not seen yet.
        let Some((bitmap, scale)) = v.bitmap_now()? else {
            return Err(String::new());
        };
        let key = (want.0, want.1, want.2, scale.to_bits());
        if let Some((_, img)) = self.viewer_view.images.iter().find(|(k, _)| *k == key) {
            return Ok(img.clone());
        }
        let img = render_image(&bitmap).ok_or("a broken bitmap")?;
        // An animation keeps its frames; anything else, the one shown.
        let animated = v.structure().animated();
        let (old, keep): (Vec<_>, Vec<_>) = self
            .viewer_view
            .images
            .drain(..)
            .partition(|(k, _)| !(animated && k.0 == key.0 && k.2 == key.2));
        self.viewer_view.images = keep;
        for (_, img) in old {
            cx.drop_image(img, Some(window));
        }
        self.viewer_view.images.push((key, img.clone()));
        Ok(img)
    }

    /// Draws again soon while a page renders on a thread.
    fn viewer_poll(&mut self, cx: &mut Context<'_, Editor>) {
        let rendering = self.doc.viewer.as_deref().is_some_and(|v| v.rendering());
        if !rendering || self.viewer_view.render_poll.is_some() {
            return;
        }
        self.viewer_view.render_poll = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            let _ = this.update(cx, |e, cx| {
                e.viewer_view.render_poll = None;
                cx.notify();
            });
        }));
    }

    /// Plays the next frame after the frame shown has had its time.
    fn viewer_schedule(&mut self, cx: &mut Context<'_, Editor>) {
        let Some(ms) = self.doc.viewer.as_deref().and_then(|v| v.frame_delay()) else {
            self.viewer_view.timer = None;
            return;
        };
        if self.viewer_view.timer.is_some() {
            return;
        }
        self.viewer_view.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(u64::from(ms)))
                .await;
            let _ = this.update(cx, |e, cx| {
                e.viewer_view.timer = None;
                if let Some(v) = e.doc.viewer.as_deref_mut()
                    && v.frame_delay().is_some()
                {
                    v.advance_frame();
                }
                cx.notify();
            });
        }));
    }

    /// The viewer's area and its information panel, for a document a
    /// viewer opened.
    pub(crate) fn viewer_element(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Editor>,
    ) -> Option<Div> {
        self.doc.viewer.as_ref()?;
        self.viewer_schedule(cx);
        let theme = self.theme.clone();
        let entity = cx.entity();
        if self.doc.viewer.as_deref().is_some_and(|v| v.is_grid()) {
            return Some(self.grid_element(window, cx));
        }
        let image = self.viewer_image(window, cx);
        self.viewer_poll(cx);
        let area = match image {
            Ok(image) => {
                let prepaint = entity.clone();
                div()
                    .debug_selector(|| "viewer".into())
                    .size_full()
                    .overflow_hidden()
                    .child(
                        gpui::canvas(
                            move |bounds, _window, cx| {
                                prepaint.update(cx, |e, _| {
                                    e.viewer_view.bounds = Some(bounds);
                                    let v = e.doc.viewer.as_deref_mut()?;
                                    v.set_area(
                                        f32::from(bounds.size.width),
                                        f32::from(bounds.size.height),
                                    );
                                    Some(v.placement())
                                })
                            },
                            move |bounds, placement, window, _cx| {
                                let Some(p) = placement else { return };
                                let at = Bounds {
                                    origin: bounds.origin + point(px(p.x), px(p.y)),
                                    size: size(px(p.width), px(p.height)),
                                };
                                let _ = window.paint_image(
                                    bounds,
                                    at,
                                    Corners::default(),
                                    image,
                                    0,
                                    false,
                                );
                            },
                        )
                        .size_full(),
                    )
            }
            Err(e) => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.muted)
                .child(SharedString::from(e)),
        };
        let area = area
            .id("viewer-area")
            .when(self.viewer_view.over_link, |d| d.cursor_pointer())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    this.viewer_view.drag = Some(ev.position);
                    this.viewer_view.press = Some(ev.position);
                    let handle = gpui::Focusable::focus_handle(this, cx);
                    window.focus(&handle, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseUpEvent, _, cx| {
                    this.viewer_view.drag = None;
                    let Some(down) = this.viewer_view.press.take() else {
                        return;
                    };
                    let moved = ev.position - down;
                    if f32::from(moved.x).hypot(f32::from(moved.y)) > 4.0 {
                        return;
                    }
                    // A click: a link goes to its page, or opens outside.
                    let at = ev.position
                        - this
                            .viewer_view
                            .bounds
                            .map(|b| b.origin)
                            .unwrap_or_default();
                    let Some(v) = this.doc.viewer.as_deref_mut() else {
                        return;
                    };
                    let Some(target) = v.link_at(f32::from(at.x), f32::from(at.y)) else {
                        return;
                    };
                    if let Some(url) = v.follow(&target)
                        && (url.contains("://") || url.starts_with("mailto:"))
                    {
                        cx.open_url(&url);
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                let Some(from) = this.viewer_view.drag else {
                    // Over a link, the pointer is a hand.
                    let at = ev.position
                        - this
                            .viewer_view
                            .bounds
                            .map(|b| b.origin)
                            .unwrap_or_default();
                    let over = this
                        .doc
                        .viewer
                        .as_deref_mut()
                        .and_then(|v| v.link_at(f32::from(at.x), f32::from(at.y)))
                        .is_some();
                    if over != this.viewer_view.over_link {
                        this.viewer_view.over_link = over;
                        cx.notify();
                    }
                    return;
                };
                if ev.pressed_button != Some(MouseButton::Left) {
                    this.viewer_view.drag = None;
                    return;
                }
                let d = ev.position - from;
                this.viewer_view.drag = Some(ev.position);
                if let Some(v) = this.doc.viewer.as_deref_mut() {
                    // The picture follows the mouse.
                    v.pan(-f32::from(d.x), -f32::from(d.y));
                    cx.notify();
                }
            }))
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                let d = ev.delta.pixel_delta(px(20.));
                let origin = this
                    .viewer_view
                    .bounds
                    .map(|b| b.origin)
                    .unwrap_or_default();
                let Some(v) = this.doc.viewer.as_deref_mut() else {
                    return;
                };
                if ev.modifiers.platform || ev.modifiers.control {
                    let at = ev.position - origin;
                    let factor = 1.005_f32.powf(f32::from(d.y));
                    v.zoom_at(factor, f32::from(at.x), f32::from(at.y));
                } else {
                    v.scroll(-f32::from(d.x), -f32::from(d.y));
                }
                cx.stop_propagation();
                cx.notify();
            }));
        let info = self.doc.viewer.as_deref().filter(|v| v.info).map(|v| {
            div()
                .debug_selector(|| "viewer-info".into())
                .w(px(300.))
                .flex_none()
                .h_full()
                .overflow_hidden()
                .border_l_1()
                .border_color(theme.border)
                .bg(theme.bar)
                .px(px(12.))
                .py(px(10.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .text_size(px(theme.size * 0.85))
                .children(v.info_fields().into_iter().map(|f| {
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_color(theme.muted)
                                .child(SharedString::from(f.label)),
                        )
                        .child(SharedString::from(f.value))
                }))
        });
        Some(
            div()
                .size_full()
                .flex()
                .flex_row()
                .child(area)
                .children(info),
        )
    }

    /// A grid unit (a sheet): letters above, row numbers at the left, the
    /// cells in view with their styles, the cursor's cell outlined; a click
    /// moves the cursor, a double click edits, the wheel scrolls.
    fn grid_element(&mut self, window: &mut Window, cx: &mut Context<'_, Editor>) -> Div {
        let theme = self.theme.clone();
        let entity = cx.entity();
        let run = |text: &str| gpui::TextRun {
            len: text.len(),
            font: gpui::font(SharedString::from(theme.font.clone())),
            color: theme.foreground,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        // The grid's text is a little smaller than the body's, its rows
        // roomier, so that a sheet reads as a sheet.
        let text_size = (theme.size * 0.93).round();
        let measure = |t: &str| -> f32 {
            f32::from(
                window
                    .text_system()
                    .shape_line(
                        SharedString::from(t.to_string()),
                        px(text_size),
                        &[run(t)],
                        None,
                    )
                    .width,
            )
        };
        // A digit's width: Excel's column widths count them.
        let digit = measure("0");
        let row_h = (text_size * 1.9).round();
        const PAD: f32 = 6.0;
        let bounds = self.viewer_view.bounds.map_or((900.0, 600.0), |b| {
            (f32::from(b.size.width), f32::from(b.size.height))
        });
        let Some(v) = self.doc.viewer.as_deref_mut() else {
            return div();
        };
        let Some(layout) = v.grid_layout() else {
            return div();
        };
        let drag = self.viewer_view.col_drag;
        let row_drag = self.viewer_view.row_drag;
        // Rows in points, drawn so that a default row is `row_h` high.
        let default_pt = if layout.default_height > 0.0 {
            layout.default_height
        } else {
            15.0
        };
        let heights: std::collections::HashMap<u32, f32> = layout.heights.iter().copied().collect();
        let row_px = |r: u32| -> f32 {
            if let Some(d) = row_drag
                && d.row == r
            {
                return d.px;
            }
            heights
                .get(&r)
                .map_or(row_h, |h| (h / default_pt * row_h).clamp(4.0, 600.0))
        };
        let col_px = |c: u32| -> f32 {
            let w = layout
                .widths
                .get(c as usize)
                .copied()
                .unwrap_or(layout.default_width);
            if let Some(d) = drag
                && d.col == c
            {
                return d.px;
            }
            (w * digit + 2.0 * PAD).clamp(24.0, 800.0)
        };
        let pos = v.grid_pos();
        let gutter =
            ((pos.top + 200).max(layout.rows).to_string().len() as f32 * digit + 16.0).max(40.0);
        // Rows and columns in view: the frozen ones, then from the scroll on.
        let pick = |frozen: u32,
                    first: u32,
                    max: u32,
                    hidden: &[u32],
                    room: f32,
                    size: &dyn Fn(u32) -> f32|
         -> (Vec<(u32, f32)>, u32) {
            let mut out = Vec::new();
            let mut used = 0.0;
            let mut full = 0;
            let mut i = 0;
            while i < max && used < room {
                if i == frozen.min(max) || (i >= frozen && i < first.max(frozen)) {
                    i = i.max(first.max(frozen));
                    if i >= max {
                        break;
                    }
                }
                if !hidden.contains(&i) {
                    let w = size(i);
                    out.push((i, w));
                    used += w;
                    if used <= room {
                        full += 1;
                    }
                }
                i += 1;
            }
            (out, full)
        };
        let (cols, full_cols) = pick(
            layout.frozen.1,
            pos.left,
            layout.max_cols,
            &layout.hidden_cols,
            bounds.0 - gutter,
            &col_px,
        );
        let (rows, full_rows) = pick(
            layout.frozen.0,
            pos.top,
            layout.max_rows,
            &layout.hidden_rows,
            // The formula bar and the letters.
            bounds.1 - 2.0 * row_h,
            &row_px,
        );
        v.set_grid_visible(full_rows.max(1), full_cols.max(1));
        if v.grid_pos() != pos {
            // The cursor scrolled the view: lay out once more.
            return self.grid_element(window, cx);
        }
        let Some(v) = self.doc.viewer.as_deref_mut() else {
            return div();
        };
        let mut cells = std::collections::HashMap::new();
        let split = |list: &[(u32, f32)], frozen: u32| -> Vec<std::ops::Range<u32>> {
            let mut out = Vec::new();
            for part in [
                list.iter()
                    .map(|x| x.0)
                    .filter(|&i| i < frozen)
                    .collect::<Vec<_>>(),
                list.iter()
                    .map(|x| x.0)
                    .filter(|&i| i >= frozen)
                    .collect::<Vec<_>>(),
            ] {
                if let (Some(a), Some(b)) = (part.first(), part.last()) {
                    out.push(*a..*b + 1);
                }
            }
            out
        };
        for rr in split(&rows, layout.frozen.0) {
            for cr in split(&cols, layout.frozen.1) {
                for (r, c, cell) in v.grid_cells(rr.clone(), cr) {
                    cells.insert((r, c), cell);
                }
            }
        }
        // The cursor's cell in full, as entered: the formula bar.
        let input = v.cell_input();
        let sel_name = v.selection_name();
        let sel = v.selection();
        let selecting = v.grid_pos().sel.is_some();
        let in_sel = move |r: u32, c: u32| {
            selecting && (sel[0]..=sel[2]).contains(&r) && (sel[1]..=sel[3]).contains(&c)
        };
        let rgb = |c: [u8; 3]| -> gpui::Hsla {
            gpui::rgb(u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2])).into()
        };
        let header_bg = theme.bar;
        let cursor = theme.caret;
        let name = sel_name;
        let formula_bar = div()
            .debug_selector(|| "viewer-grid-formula".into())
            .flex()
            .flex_row()
            .flex_none()
            .h(px(row_h))
            .items_center()
            .bg(header_bg)
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .w(px(gutter.max(80.0)))
                    .flex_none()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_r_1()
                    .border_color(theme.border)
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(SharedString::from(name)),
            )
            .child(
                div()
                    .flex_1()
                    .px(px(PAD + 2.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(SharedString::from(input)),
            );
        let letters = div()
            .flex()
            .flex_row()
            .flex_none()
            .h(px(row_h))
            .bg(header_bg)
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .w(px(gutter))
                    .flex_none()
                    .border_r_1()
                    .border_color(theme.border),
            )
            .children(cols.iter().map(|&(c, w)| {
                let name = kalem_core::csv_tools::column_letters(c as usize);
                div()
                    .w(px(w))
                    .flex_none()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_r_1()
                    .border_color(theme.border)
                    .text_color(if c == pos.col {
                        theme.foreground
                    } else {
                        theme.muted
                    })
                    .font_weight(if c == pos.col {
                        gpui::FontWeight::BOLD
                    } else {
                        gpui::FontWeight::NORMAL
                    })
                    .relative()
                    .child(SharedString::from(name))
                    // The right edge: dragged to resize, double-clicked to fit.
                    .child(
                        div()
                            .debug_selector(move || format!("viewer-grid-edge-{c}"))
                            .id(SharedString::from(format!("column-edge-{c}")))
                            .absolute()
                            .top_0()
                            .right(px(-3.))
                            .w(px(7.))
                            .h_full()
                            .cursor_col_resize()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    if ev.click_count >= 2 {
                                        this.viewer_view.col_drag = None;
                                        this.grid_autofit(c, window, cx);
                                        return;
                                    }
                                    let x = f32::from(ev.position.x);
                                    this.viewer_view.col_drag = Some(ColDrag {
                                        col: c,
                                        start_x: x,
                                        start_px: w,
                                        px: w,
                                    });
                                    cx.notify();
                                }),
                            ),
                    )
                    .id(SharedString::from(format!("column-{c}")))
                    // A click goes to the column; a double click fits its width.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                            if ev.click_count >= 2 {
                                this.grid_autofit(c, window, cx);
                            } else if let Some(v) = this.doc.viewer.as_deref_mut() {
                                let row = v.grid_pos().row;
                                v.grid_move_to(row, c);
                            }
                            cx.notify();
                        }),
                    )
            }));
        let empty = |r: u32, c: u32| cells.get(&(r, c)).is_none_or(|x| x.text.is_empty());
        // Merged cells: one cell over the rows and columns in view, drawn
        // above the cells it covers, from its first cell.
        let mut col_x = std::collections::HashMap::new();
        let mut x = gutter;
        for &(c, w) in &cols {
            col_x.insert(c, (x, w));
            x += w;
        }
        let mut row_y = std::collections::HashMap::new();
        let mut y = 2.0 * row_h;
        for &(r, h) in &rows {
            row_y.insert(r, (y, h));
            y += h;
        }
        let merges: Vec<_> = layout
            .merged
            .iter()
            .filter_map(|m| {
                let xs: Vec<(f32, f32)> = (m[1]..=m[3])
                    .filter_map(|c| col_x.get(&c).copied())
                    .collect();
                let ys: Vec<(f32, f32)> = (m[0]..=m[2])
                    .filter_map(|r| row_y.get(&r).copied())
                    .collect();
                let (x0, y0) = (xs.first()?.0, ys.first()?.0);
                let (w, h) = (
                    xs.iter().map(|v| v.1).sum::<f32>(),
                    ys.iter().map(|v| v.1).sum::<f32>(),
                );
                let cell = cells.get(&(m[0], m[1]));
                let mut d = div()
                    .absolute()
                    .left(px(x0))
                    .top(px(y0))
                    .w(px(w))
                    .h(px(h))
                    .px(px(PAD))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .bg(cell.and_then(|c| c.fill).map_or(theme.background, rgb))
                    .border_r_1()
                    .border_b_1()
                    .border_color(theme.border);
                if let Some(cell) = cell {
                    let right = matches!(cell.align, kalem_viewer::Align::Right)
                        || (cell.numeric && matches!(cell.align, kalem_viewer::Align::General));
                    if right {
                        d = d.justify_end();
                    } else if matches!(cell.align, kalem_viewer::Align::Center) {
                        d = d.justify_center();
                    }
                    if let Some(c) = cell.color {
                        d = d.text_color(rgb(c));
                    }
                    if cell.bold {
                        d = d.font_weight(gpui::FontWeight::BOLD);
                    }
                    if cell.italic {
                        d = d.italic();
                    }
                    if !cell.wrap {
                        d = d.whitespace_nowrap();
                    }
                    d = d.child(
                        div()
                            .overflow_hidden()
                            .child(SharedString::from(cell.text.clone())),
                    );
                }
                if in_sel(m[0], m[1]) {
                    d = d.child(div().absolute().inset_0().bg(theme.selection).opacity(0.45));
                }
                if (pos.row, pos.col) == (m[0], m[1]) {
                    d = d.child(div().absolute().inset_0().border_2().border_color(cursor));
                }
                Some(d)
            })
            .collect();
        let body = rows.iter().map(|&(r, rh)| {
            let number = div()
                .w(px(gutter))
                .flex_none()
                .h_full()
                .flex()
                .items_center()
                .justify_end()
                .pr(px(PAD))
                .bg(header_bg)
                .border_r_1()
                .border_b_1()
                .border_color(theme.border)
                .text_color(if r == pos.row {
                    theme.foreground
                } else {
                    theme.muted
                })
                .font_weight(if r == pos.row {
                    gpui::FontWeight::BOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                .relative()
                .child(SharedString::from((r + 1).to_string()))
                // The bottom edge: dragged to resize, double-clicked to reset.
                .child(
                    div()
                        .debug_selector(move || format!("viewer-grid-row-edge-{r}"))
                        .id(SharedString::from(format!("row-edge-{r}")))
                        .absolute()
                        .left_0()
                        .bottom(px(-3.))
                        .w_full()
                        .h(px(7.))
                        .cursor_row_resize()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                if ev.click_count >= 2 {
                                    // Fitted to its wrapped text, as Excel does.
                                    this.viewer_view.row_drag = None;
                                    this.grid_fit_row(r, window, cx);
                                    return;
                                }
                                this.viewer_view.row_drag = Some(RowDrag {
                                    row: r,
                                    start_y: f32::from(ev.position.y),
                                    start_px: rh,
                                    px: rh,
                                });
                                cx.notify();
                            }),
                        ),
                );
            // Text wider than its cell runs on over the empty cells at its
            // right, as in a spreadsheet; drawn above them.
            let mut overflow: Vec<(f32, f32, u32)> = Vec::new();
            let mut x = gutter;
            for (k, &(c, w)) in cols.iter().enumerate() {
                if let Some(cell) = cells.get(&(r, c))
                    && !cell.numeric
                    && !cell.wrap
                    && !cell.text.is_empty()
                    && matches!(
                        cell.align,
                        kalem_viewer::Align::General | kalem_viewer::Align::Left
                    )
                {
                    let need = measure(&cell.text) + 2.0 * PAD;
                    if need > w {
                        let mut span = w;
                        for &(c2, w2) in &cols[k + 1..] {
                            if span >= need
                                || !empty(r, c2)
                                || c2 < layout.frozen.1 && c >= layout.frozen.1
                            {
                                break;
                            }
                            span += w2;
                        }
                        if span > w {
                            overflow.push((x, span, c));
                        }
                    }
                }
                x += w;
            }
            let spilled: Vec<u32> = overflow.iter().map(|o| o.2).collect();
            let row_cells = cols.iter().map(|&(c, w)| {
                let cell = cells.get(&(r, c));
                let here = (r, c) == (pos.row, pos.col);
                let mut d = div()
                    .debug_selector(move || format!("viewer-grid-cell-{r}-{c}"))
                    .id(SharedString::from(format!("cell-{r}-{c}")))
                    .relative()
                    .w(px(w))
                    .flex_none()
                    .h_full()
                    .px(px(PAD))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .border_r_1()
                    .border_b_1()
                    .border_color(theme.border);
                if cell.is_none_or(|c| !c.wrap) {
                    d = d.whitespace_nowrap();
                }
                if let Some(cell) = cell {
                    let right = matches!(cell.align, kalem_viewer::Align::Right)
                        || (cell.numeric && matches!(cell.align, kalem_viewer::Align::General));
                    if right {
                        d = d.justify_end();
                    } else if matches!(cell.align, kalem_viewer::Align::Center) {
                        d = d.justify_center();
                    }
                    if let Some(f) = cell.fill {
                        d = d.bg(rgb(f));
                    }
                    if let Some(c) = cell.color {
                        d = d.text_color(rgb(c));
                    }
                    if cell.bold {
                        d = d.font_weight(gpui::FontWeight::BOLD);
                    }
                    if cell.italic {
                        d = d.italic();
                    }
                    if cell.underline {
                        d = d.underline();
                    }
                    if cell.strike {
                        d = d.line_through();
                    }
                    if !spilled.contains(&c) {
                        // A number too wide shows as #, as Excel shows it;
                        // text ends in an ellipsis.
                        let text = if cell.numeric && measure(&cell.text) + 2.0 * PAD > w {
                            "#".repeat(((w - 2.0 * PAD) / measure("#")).max(1.0) as usize)
                        } else {
                            cell.text.clone()
                        };
                        d = if cell.wrap {
                            // Wrapped: lines within the cell's width.
                            d.child(
                                div()
                                    .w_full()
                                    .overflow_hidden()
                                    .child(SharedString::from(text)),
                            )
                        } else {
                            d.child(
                                div()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(SharedString::from(text)),
                            )
                        };
                    }
                    if cell.note {
                        // A note: a mark in the corner, as Excel's red triangle.
                        d = d.child(
                            div()
                                .absolute()
                                .top_0()
                                .right_0()
                                .size(px(6.))
                                .bg(theme.todo),
                        );
                    }
                }
                if in_sel(r, c) {
                    d = d.child(div().absolute().inset_0().bg(theme.selection).opacity(0.45));
                }
                if here {
                    d = d.child(div().absolute().inset_0().border_2().border_color(cursor));
                }
                d.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                        if let Some(v) = this.doc.viewer.as_deref_mut() {
                            // Shift and a click select to here; a drag selects as it goes.
                            if ev.modifiers.shift {
                                v.grid_extend_to(r, c);
                            } else {
                                v.grid_move_to(r, c);
                            }
                        }
                        this.viewer_view.selecting = true;
                        let handle = gpui::Focusable::focus_handle(this, cx);
                        window.focus(&handle, cx);
                        if ev.click_count >= 2 {
                            this.viewer_view.selecting = false;
                            this.run_command("viewer.grid.edit", serde_json::json!({}), window, cx);
                        }
                        cx.notify();
                    }),
                )
                .on_mouse_move(cx.listener(
                    move |this, ev: &MouseMoveEvent, _, cx| {
                        if !this.viewer_view.selecting
                            || ev.pressed_button != Some(MouseButton::Left)
                        {
                            return;
                        }
                        if let Some(v) = this.doc.viewer.as_deref_mut() {
                            let p = v.grid_pos();
                            if (p.row, p.col) != (r, c) {
                                v.grid_extend_to(r, c);
                                cx.notify();
                            }
                        }
                    },
                ))
            });
            let spills = overflow.into_iter().filter_map(|(x, span, c)| {
                let cell = cells.get(&(r, c))?;
                let mut d = div()
                    .absolute()
                    .top_0()
                    .left(px(x))
                    .w(px(span))
                    .h(px(rh))
                    .px(px(PAD))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .whitespace_nowrap();
                if let Some(c) = cell.color {
                    d = d.text_color(rgb(c));
                }
                if cell.bold {
                    d = d.font_weight(gpui::FontWeight::BOLD);
                }
                if cell.italic {
                    d = d.italic();
                }
                Some(
                    d.child(
                        div()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(SharedString::from(cell.text.clone())),
                    ),
                )
            });
            div()
                .relative()
                .flex()
                .flex_row()
                .flex_none()
                .h(px(rh))
                .child(number)
                .children(row_cells)
                .children(spills)
        });
        let prepaint = entity.clone();
        div()
            .debug_selector(|| "viewer-grid".into())
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(theme.background)
            .text_size(px(text_size))
            .font_family(SharedString::from(theme.font.clone()))
            .child(
                gpui::canvas(
                    move |bounds, _window, cx| {
                        prepaint.update(cx, |e, _| e.viewer_view.bounds = Some(bounds));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .child(formula_bar)
                    .child(letters)
                    .children(body)
                    .children(merges),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if let Some(d) = this.viewer_view.row_drag.as_mut() {
                    if ev.pressed_button != Some(MouseButton::Left) {
                        this.viewer_view.row_drag = None;
                    } else {
                        d.px = (d.start_px + f32::from(ev.position.y) - d.start_y).max(4.0);
                    }
                    cx.notify();
                    return;
                }
                let Some(d) = this.viewer_view.col_drag.as_mut() else {
                    return;
                };
                if ev.pressed_button != Some(MouseButton::Left) {
                    this.viewer_view.col_drag = None;
                } else {
                    d.px = (d.start_px + f32::from(ev.position.x) - d.start_x).max(2.0 * PAD + 4.0);
                }
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                    this.viewer_view.selecting = false;
                    if let Some(d) = this.viewer_view.row_drag.take() {
                        if (d.px - d.start_px).abs() >= 1.0
                            && let Some(v) = this.doc.viewer.as_deref_mut()
                        {
                            // Back to points, as the file counts them.
                            let pt = (d.px / row_h * default_pt * 4.0).round() / 4.0;
                            if let Err(e) = v.set_row_height(d.row, pt) {
                                this.message(e, true);
                            }
                        }
                        cx.notify();
                        return;
                    }
                    let Some(d) = this.viewer_view.col_drag.take() else {
                        return;
                    };
                    if (d.px - d.start_px).abs() >= 1.0
                        && let Some(v) = this.doc.viewer.as_deref_mut()
                    {
                        // Back to the spreadsheet's unit: digits of the grid's font.
                        let width = ((d.px - 2.0 * PAD) / digit * 100.0).round() / 100.0;
                        if let Err(e) = v.set_col_width(d.col, width) {
                            this.message(e, true);
                        }
                    }
                    cx.notify();
                }),
            )
            .on_scroll_wheel(cx.listener(move |this, ev: &ScrollWheelEvent, _, cx| {
                let d = ev.delta.pixel_delta(px(row_h));
                if let Some(v) = this.doc.viewer.as_deref_mut() {
                    let rows = (-f32::from(d.y) / row_h).round() as i64;
                    let cols = (-f32::from(d.x) / (digit * 9.0)).round() as i64;
                    if rows != 0 || cols != 0 {
                        v.grid_scroll(rows, cols);
                        cx.notify();
                    }
                }
                cx.stop_propagation();
            }))
    }

    /// Fits column `col` to its widest cell, measured in the grid's font.
    pub(crate) fn grid_autofit(
        &mut self,
        col: u32,
        window: &mut Window,
        cx: &mut Context<'_, Editor>,
    ) {
        let theme = self.theme.clone();
        let size = px((theme.size * 0.93).round());
        let font = gpui::font(SharedString::from(theme.font.clone()));
        let width = |t: &str| -> f32 {
            let run = gpui::TextRun {
                len: t.len(),
                font: font.clone(),
                color: theme.foreground,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            f32::from(
                window
                    .text_system()
                    .shape_line(SharedString::from(t.to_string()), size, &[run], None)
                    .width,
            )
        };
        let digit = width("0").max(1.0);
        let Some(v) = self.doc.viewer.as_deref_mut() else {
            return;
        };
        if let Err(e) = v.autofit_col(col, &|t| width(t) / digit) {
            self.message(e, true);
        }
        cx.notify();
    }

    /// Fits row `row` to its wrapped cells' lines, measured in the grid's font.
    pub(crate) fn grid_fit_row(
        &mut self,
        row: u32,
        window: &mut Window,
        cx: &mut Context<'_, Editor>,
    ) {
        let theme = self.theme.clone();
        let size = px((theme.size * 0.93).round());
        let font = gpui::font(SharedString::from(theme.font.clone()));
        let width = |t: &str| -> f32 {
            let run = gpui::TextRun {
                len: t.len(),
                font: font.clone(),
                color: theme.foreground,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            f32::from(
                window
                    .text_system()
                    .shape_line(SharedString::from(t.to_string()), size, &[run], None)
                    .width,
            )
        };
        let digit = width("0").max(1.0);
        let Some(v) = self.doc.viewer.as_deref_mut() else {
            return;
        };
        if let Err(e) = v.fit_row_height(row, &|t| width(t) / digit) {
            self.message(e, true);
        }
        cx.notify();
    }
}
