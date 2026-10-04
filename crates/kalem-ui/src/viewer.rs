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
    /// A sheet's pictures as gpui images, by unit, place and generation.
    pictures: std::collections::HashMap<(usize, usize, u64), Arc<RenderImage>>,
    /// Where a drag started, or last moved to.
    drag: Option<Point<Pixels>>,
    /// Where the button went down: released near it, it is a click.
    press: Option<Point<Pixels>>,
    /// The pointer is over a link.
    over_link: bool,
    /// The pointer is over the page's text.
    over_text: bool,
    /// A drag that began on the page's text selects it.
    selecting_text: bool,
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
    /// A chart being moved or resized by dragging.
    chart_drag: Option<ChartDrag>,
    /// Cells being filled by dragging the fill handle: the source and the
    /// range the pointer has reached.
    fill_drag: Option<([u32; 4], [u32; 4])>,
    /// The grid's columns and rows as last drawn: index, start, size, in
    /// the grid's own pixels.
    grid_lines: (Vec<GridLine>, Vec<GridLine>),
}

/// A column or row as drawn: index, start, size.
type GridLine = (u32, f32, f32);

/// A chart dragged by its body (moved) or its corner (resized).
#[derive(Debug, Clone, Copy)]
struct ChartDrag {
    /// Its place among the sheet's charts.
    index: usize,
    /// The corner: resized, not moved.
    resize: bool,
    /// The pointer where the drag began.
    start: Point<Pixels>,
    /// Its box then, in the grid's pixels.
    rect: (f32, f32, f32, f32),
    /// How far the pointer has gone.
    delta: (f32, f32),
}

/// The cell under a point of the grid, from the lines last drawn; past
/// them, as many more of the last one's size.
fn line_at(lines: &[GridLine], at: f32) -> Option<u32> {
    let first = lines.first()?;
    let last = lines.last()?;
    if at < first.1 {
        let back = ((first.1 - at) / first.2.max(1.0)).ceil() as u32;
        return Some(first.0.saturating_sub(back));
    }
    if let Some(l) = lines.iter().find(|l| at >= l.1 && at < l.1 + l.2) {
        return Some(l.0);
    }
    let more = ((at - (last.1 + last.2)) / last.2.max(1.0)).floor() as u32 + 1;
    Some(last.0 + more)
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
    let buf = image::RgbaImage::from_raw(b.width, b.height, b.bgra())?;
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

    /// Draws again soon while a page renders or a search runs on a thread.
    fn viewer_poll(&mut self, cx: &mut Context<'_, Editor>) {
        // The find bar's search: matches taken, the first one shown.
        if let Some(v) = self.doc.viewer.as_deref_mut() {
            v.search_poll();
        }
        let busy = self
            .doc
            .viewer
            .as_deref()
            .is_some_and(|v| v.rendering() || v.searching());
        if !busy || self.viewer_view.render_poll.is_some() {
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
                let mark = theme.mark.opacity(0.45);
                let mark_shown = theme.selection.opacity(0.6);
                let selection = theme.selection.opacity(0.45);
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
                                    Some((v.placement(), v.search_marks(), v.selection_marks()))
                                })
                            },
                            move |bounds, placed, window, _cx| {
                                let Some((p, marks, selected)) = placed else {
                                    return;
                                };
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
                                // The find bar's matches over the page, the
                                // one shown in the selection's color; seen
                                // through, so the text stays readable.
                                // The text selected, as text is selected.
                                for [x, y, w, h] in selected {
                                    window.paint_quad(gpui::fill(
                                        Bounds::new(
                                            bounds.origin + point(px(x), px(y)),
                                            size(px(w), px(h)),
                                        ),
                                        selection,
                                    ));
                                }
                                for ([x, y, w, h], shown) in marks {
                                    let color = if shown { mark_shown } else { mark };
                                    window.paint_quad(gpui::fill(
                                        Bounds::new(
                                            bounds.origin + point(px(x), px(y)),
                                            size(px(w), px(h)),
                                        ),
                                        color,
                                    ));
                                }
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
            .when(
                !self.viewer_view.over_link && self.viewer_view.over_text,
                |d| d.cursor_text(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    // On the page's text a drag selects it; elsewhere it
                    // pans.
                    let at = ev.position
                        - this
                            .viewer_view
                            .bounds
                            .map(|b| b.origin)
                            .unwrap_or_default();
                    // A double click selects the word, a triple one the
                    // line; the release is no click then.
                    if ev.click_count >= 2
                        && let Some(v) = this.doc.viewer.as_deref_mut()
                        && v.select_word(f32::from(at.x), f32::from(at.y), ev.click_count >= 3)
                    {
                        this.viewer_view.selecting_text = false;
                        this.viewer_view.drag = None;
                        this.viewer_view.press = None;
                        let handle = gpui::Focusable::focus_handle(this, cx);
                        window.focus(&handle, cx);
                        cx.notify();
                        return;
                    }
                    let selecting = this
                        .doc
                        .viewer
                        .as_deref_mut()
                        .is_some_and(|v| v.select_from(f32::from(at.x), f32::from(at.y)));
                    this.viewer_view.selecting_text = selecting;
                    this.viewer_view.drag = (!selecting).then_some(ev.position);
                    this.viewer_view.press = Some(ev.position);
                    cx.notify();
                    let handle = gpui::Focusable::focus_handle(this, cx);
                    window.focus(&handle, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseUpEvent, _, cx| {
                    this.viewer_view.drag = None;
                    this.viewer_view.selecting_text = false;
                    let Some(down) = this.viewer_view.press.take() else {
                        return;
                    };
                    let moved = ev.position - down;
                    if f32::from(moved.x).hypot(f32::from(moved.y)) > 4.0 {
                        return;
                    }
                    // A click selects nothing.
                    if let Some(v) = this.doc.viewer.as_deref_mut() {
                        v.clear_text_selection();
                    }
                    // A click: a link goes to its page, or opens outside.
                    let at = ev.position
                        - this
                            .viewer_view
                            .bounds
                            .map(|b| b.origin)
                            .unwrap_or_default();
                    let pdf = this.doc.meta.path.clone();
                    let Some(v) = this.doc.viewer.as_deref_mut() else {
                        return;
                    };
                    // Ctrl-click (Cmd on macOS) on a PDF LaTeX built: the
                    // source line typeset there, by its SyncTeX file.
                    if ev.modifiers.secondary()
                        && let Some(pdf) = pdf
                    {
                        let (x, y) = v.unit_point(f32::from(at.x), f32::from(at.y));
                        let found = kalem_core::synctex::Synctex::cached(&pdf)
                            .and_then(|st| st.inverse(v.unit + 1, f64::from(x), f64::from(y)));
                        if let Some((file, line)) = found {
                            let file = pdf.parent().map_or(file.clone(), |d| d.join(&file));
                            cx.emit(crate::editor::DocEvent::Open {
                                path: file,
                                at: Some((line as u64, 0)),
                            });
                        }
                        return;
                    }
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
                let at = ev.position
                    - this
                        .viewer_view
                        .bounds
                        .map(|b| b.origin)
                        .unwrap_or_default();
                let (x, y) = (f32::from(at.x), f32::from(at.y));
                if this.viewer_view.selecting_text {
                    if ev.pressed_button != Some(MouseButton::Left) {
                        this.viewer_view.selecting_text = false;
                    } else if let Some(v) = this.doc.viewer.as_deref_mut() {
                        v.select_to(x, y);
                        cx.notify();
                    }
                    return;
                }
                let Some(from) = this.viewer_view.drag else {
                    // Over a link, the pointer is a hand; over text, a
                    // text cursor.
                    let Some(v) = this.doc.viewer.as_deref_mut() else {
                        return;
                    };
                    let over_link = v.link_at(x, y).is_some();
                    let over_text = !over_link && v.text_hit(x, y);
                    if (over_link, over_text)
                        != (this.viewer_view.over_link, this.viewer_view.over_text)
                    {
                        this.viewer_view.over_link = over_link;
                        this.viewer_view.over_text = over_text;
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
                    .shape_line(crate::one_line(t), px(text_size), &[run(t)], None)
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
        let mut invalid = std::collections::HashSet::new();
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
                for (r, c, cell) in v.grid_cells(rr.clone(), cr.clone()) {
                    cells.insert((r, c), cell);
                }
                invalid.extend(v.invalid_cells(rr.clone(), cr));
            }
        }
        // The cursor's cell in full, as entered: the formula bar.
        let input = v.cell_input();
        let has_list = v.cursor_has_list();
        let editable = layout.editable;
        let sel_name = v.selection_name();
        let sel = v.selection();
        let selecting = v.grid_pos().sel.is_some();
        let cut = v.cut_range();
        let pointer = v.pointer;
        let marks = v.outline_marks();
        let arrows = v.arrows.clone();
        // Go To Special's ranges, selected together.
        let areas = v.areas.clone();
        let in_sel = move |r: u32, c: u32| {
            (selecting && (sel[0]..=sel[2]).contains(&r) && (sel[1]..=sel[3]).contains(&c))
                || areas
                    .iter()
                    .any(|m| (m[0]..=m[2]).contains(&r) && (m[1]..=m[3]).contains(&c))
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
        // Charts over the cells they cover, the part in view.
        let drag = self.viewer_view.chart_drag;
        self.viewer_view.grid_lines = (
            col_x.iter().map(|(c, (x, w))| (*c, *x, *w)).collect(),
            row_y.iter().map(|(r, (y, h))| (*r, *y, *h)).collect(),
        );
        self.viewer_view.grid_lines.0.sort_by_key(|l| l.0);
        self.viewer_view.grid_lines.1.sort_by_key(|l| l.0);
        // Pictures and shapes, over the cells they cover.
        let drawings = v.drawings();
        let generation = v.generation();
        let unit = v.unit;
        for (i, d) in drawings.iter().enumerate() {
            if matches!(d.kind, kalem_viewer::DrawingKind::Picture)
                && !self
                    .viewer_view
                    .pictures
                    .contains_key(&(unit, i, generation))
                && let Some(img) = v.drawing_bitmap(i).as_ref().and_then(render_image)
            {
                self.viewer_view.pictures.insert((unit, i, generation), img);
            }
        }
        self.viewer_view
            .pictures
            .retain(|k, _| k.0 == unit && k.2 == generation);
        let pictures = self.viewer_view.pictures.clone();
        let drawing_views: Vec<_> = drawings
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                let a = d.anchor;
                let xs: Vec<(f32, f32)> = (a[1]..=a[3])
                    .filter_map(|c| col_x.get(&c).copied())
                    .collect();
                let ys: Vec<(f32, f32)> = (a[0]..=a[2])
                    .filter_map(|r| row_y.get(&r).copied())
                    .collect();
                let (x0, y0) = (xs.first()?.0, ys.first()?.0);
                let w: f32 = xs.iter().map(|v| v.1).sum();
                let h: f32 = ys.iter().map(|v| v.1).sum();
                let base = div()
                    .debug_selector(move || format!("viewer-grid-drawing-{i}"))
                    .absolute()
                    .left(px(x0))
                    .top(px(y0))
                    .w(px(w))
                    .h(px(h))
                    .overflow_hidden();
                Some(match &d.kind {
                    kalem_viewer::DrawingKind::Picture => base.children(
                        pictures
                            .get(&(unit, i, generation))
                            .cloned()
                            .map(|img| gpui::img(img).size_full()),
                    ),
                    kalem_viewer::DrawingKind::Shape {
                        preset,
                        fill,
                        line,
                        text,
                        ..
                    } => {
                        let color = |c: [u8; 3]| -> gpui::Hsla {
                            gpui::rgb(
                                u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2]),
                            )
                            .into()
                        };
                        let mut s = base.flex().items_center().justify_center().p(px(4.));
                        if let Some(f) = fill {
                            s = s.bg(color(*f));
                        }
                        if let Some(l) = line {
                            s = s.border_1().border_color(color(*l));
                        }
                        s = match preset.as_str() {
                            "ellipse" => s.rounded_full(),
                            "roundRect" => s.rounded(px(10.)),
                            _ => s,
                        };
                        // Light text on a dark fill, as Excel's shape style.
                        let dark = fill.is_some_and(|f| {
                            u32::from(f[0]) * 299 + u32::from(f[1]) * 587 + u32::from(f[2]) * 114
                                < 128_000
                        });
                        s.text_color(if dark { gpui::white() } else { gpui::black() })
                            .child(SharedString::from(text.clone()))
                    }
                })
            })
            .collect();
        let charts: Vec<_> = v
            .charts()
            .iter()
            .enumerate()
            .filter_map(|(i, chart)| {
                let a = chart.anchor;
                let xs: Vec<(f32, f32)> = (a[1]..=a[3])
                    .filter_map(|c| col_x.get(&c).copied())
                    .collect();
                let ys: Vec<(f32, f32)> = (a[0]..=a[2])
                    .filter_map(|r| row_y.get(&r).copied())
                    .collect();
                let (x0, y0) = (xs.first()?.0, ys.first()?.0);
                let (w, h) = (
                    xs.iter().map(|v| v.1).sum::<f32>(),
                    ys.iter().map(|v| v.1).sum::<f32>(),
                );
                (w > 20.0 && h > 20.0).then(|| {
                    // Drawn where the drag has taken it.
                    let (mut x, mut y, mut cw, mut ch) = (x0, y0, w, h);
                    if let Some(d) = drag.filter(|d| d.index == i) {
                        if d.resize {
                            cw = (w + d.delta.0).max(30.0);
                            ch = (h + d.delta.1).max(30.0);
                        } else {
                            x += d.delta.0;
                            y += d.delta.1;
                        }
                    }
                    let rect = (x0, y0, w, h);
                    crate::chart::chart_view(
                        chart,
                        i,
                        (x, y, cw, ch),
                        theme.background,
                        theme.border,
                        theme.foreground,
                        SharedString::from(theme.font.clone()),
                    )
                    .cursor_move()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.viewer_view.chart_drag = Some(ChartDrag {
                                index: i,
                                resize: false,
                                start: ev.position,
                                rect,
                                delta: (0.0, 0.0),
                            });
                        }),
                    )
                    .child(
                        // The corner that resizes it.
                        div()
                            .debug_selector(move || format!("viewer-grid-chart-corner-{i}"))
                            .absolute()
                            .right_0()
                            .bottom_0()
                            .size(px(10.))
                            .bg(theme.border)
                            .cursor_nwse_resize()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.viewer_view.chart_drag = Some(ChartDrag {
                                        index: i,
                                        resize: true,
                                        start: ev.position,
                                        rect,
                                        delta: (0.0, 0.0),
                                    });
                                }),
                            ),
                    )
                })
            })
            .collect();
        // Cells cut: a dashed frame around the part in view, as Excel's.
        // The fill handle: over everything, at the selection's bottom right
        // corner (a merged cell's too).
        let fill_handle = (editable && self.viewer_view.fill_drag.is_none())
            .then(|| {
                let (x, w) = col_x.get(&sel[3]).copied()?;
                let (y, h) = row_y.get(&sel[2]).copied()?;
                let src = sel;
                Some(
                    div()
                        .debug_selector(|| "viewer-grid-fill-handle".into())
                        .id("fill-handle")
                        .absolute()
                        .left(px(x + w - 5.))
                        .top(px(y + h - 5.))
                        .size(px(8.))
                        .bg(cursor)
                        .border_1()
                        .border_color(theme.background)
                        .cursor_crosshair()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                // A double click fills down along the data
                                // beside, as Excel's.
                                if ev.click_count >= 2 {
                                    this.viewer_view.fill_drag = None;
                                    let lists = kalem_core::viewer::fill_lists(&this.shared.config);
                                    if let Some(v) = this.doc.viewer.as_deref_mut()
                                        && let Err(e) = {
                                            v.set_fill_lists(lists);
                                            v.fill_to_end()
                                        }
                                    {
                                        this.message(e, true);
                                    }
                                } else {
                                    this.viewer_view.fill_drag = Some((src, src));
                                }
                                cx.notify();
                            }),
                        ),
                )
            })
            .flatten();
        // The range a fill handle's drag has reached.
        let fill_frame = self.viewer_view.fill_drag.and_then(|(_, m)| {
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
            Some(
                div()
                    .debug_selector(|| "viewer-grid-fill-frame".into())
                    .absolute()
                    .left(px(x0))
                    .top(px(y0))
                    .w(px(w))
                    .h(px(h))
                    .border_2()
                    .border_dashed()
                    .border_color(cursor),
            )
        });
        // The cells a formula being typed points at.
        let pointer_frame = pointer.and_then(|m| {
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
            Some(
                div()
                    .debug_selector(|| "viewer-grid-pointer".into())
                    .absolute()
                    .left(px(x0))
                    .top(px(y0))
                    .w(px(w))
                    .h(px(h))
                    .border_2()
                    .border_dashed()
                    .border_color(theme.link),
            )
        });
        // Trace Precedents' and Dependents' arrows: from the middle of the
        // range read to the middle of the cell reading it.
        let middle = |m: [u32; 4]| -> Option<(f32, f32)> {
            let xs: Vec<(f32, f32)> = (m[1]..=m[3])
                .filter_map(|c| col_x.get(&c).copied())
                .collect();
            let ys: Vec<(f32, f32)> = (m[0]..=m[2])
                .filter_map(|r| row_y.get(&r).copied())
                .collect();
            let (x0, y0) = (xs.first()?.0, ys.first()?.0);
            let w: f32 = xs.iter().map(|v| v.1).sum();
            let h: f32 = ys.iter().map(|v| v.1).sum();
            Some((x0 + w / 2.0, y0 + h / 2.0))
        };
        let lines: Vec<((f32, f32), (f32, f32))> = arrows
            .iter()
            .filter_map(|(m, d)| Some((middle(*m)?, middle([d.0, d.1, d.0, d.1])?)))
            .collect();
        let link = theme.link;
        let arrow_layer = (!lines.is_empty()).then(|| {
            div()
                .debug_selector(|| "viewer-grid-arrows".into())
                .absolute()
                .inset_0()
                .child(
                    gpui::canvas(
                        |_, _, _| {},
                        move |bounds, (), window, _| {
                            let at = |(x, y): (f32, f32)| {
                                gpui::point(bounds.origin.x + px(x), bounds.origin.y + px(y))
                            };
                            for &(a, b) in &lines {
                                let mut p = gpui::PathBuilder::stroke(px(1.5));
                                p.move_to(at(a));
                                p.line_to(at(b));
                                // The head: two strokes back from the end.
                                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                                let len = (dx * dx + dy * dy).sqrt().max(1.0);
                                let (ux, uy) = (dx / len, dy / len);
                                for side in [-1.0f32, 1.0] {
                                    let hx = b.0 - 8.0 * ux + side * 4.0 * uy;
                                    let hy = b.1 - 8.0 * uy - side * 4.0 * ux;
                                    p.move_to(at(b));
                                    p.line_to(at((hx, hy)));
                                }
                                if let Ok(path) = p.build() {
                                    window.paint_path(path, link);
                                }
                                // A dot where it starts.
                                window.paint_quad(gpui::fill(
                                    gpui::Bounds::new(
                                        at((a.0 - 2.5, a.1 - 2.5)),
                                        gpui::size(px(5.), px(5.)),
                                    ),
                                    link,
                                ));
                            }
                        },
                    )
                    .size_full(),
                )
        });
        let cut_mark = cut.and_then(|m| {
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
            Some(
                div()
                    .debug_selector(|| "viewer-grid-cut".into())
                    .absolute()
                    .left(px(x0))
                    .top(px(y0))
                    .w(px(w))
                    .h(px(h))
                    .border_2()
                    .border_dashed()
                    .border_color(theme.link),
            )
        });
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
                    d = valign(d, cell.valign);
                    d = d.children(border_lines(cell, m[0], m[1], theme.foreground));
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
                    // The cell's own size and typeface.
                    if let Some(tenths) = cell.font_size {
                        d = d.text_size(px(f32::from(tenths) / 10.0 * 4.0 / 3.0));
                    }
                    if let Some(face) = &cell.face {
                        d = d.font_family(SharedString::from(face.clone()));
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
                // The outline's − or + of a summary row: pressed, its group
                // collapses or expands.
                .children(marks.iter().find(|m| m.0 == r).map(|(_, collapsed)| {
                    div()
                        .debug_selector(move || format!("viewer-grid-outline-{r}"))
                        .id(SharedString::from(format!("outline-{r}")))
                        .absolute()
                        .left(px(2.))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .text_color(theme.foreground)
                        .cursor_pointer()
                        .child(if *collapsed { "+" } else { "−" })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                if let Some(v) = this.doc.viewer.as_deref_mut()
                                    && let Err(e) = v.toggle_detail_at(r)
                                {
                                    this.message(e, true);
                                }
                                cx.notify();
                            }),
                        )
                }))
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
            let mut overflow: Vec<(f32, f32, u32, bool)> = Vec::new();
            let mut x = gutter;
            for (k, &(c, w)) in cols.iter().enumerate() {
                // Center Across Selection: over the empty cells at its right
                // that have it too.
                if let Some(cell) = cells.get(&(r, c))
                    && cell.center_across
                    && !cell.text.is_empty()
                {
                    let mut span = w;
                    for &(c2, w2) in &cols[k + 1..] {
                        match cells.get(&(r, c2)) {
                            Some(x2) if x2.center_across && x2.text.is_empty() => span += w2,
                            _ => break,
                        }
                    }
                    if span > w {
                        overflow.push((x, span, c, true));
                        x += w;
                        continue;
                    }
                }
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
                            overflow.push((x, span, c, false));
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
                    d = valign(d, cell.valign);
                    d = d.children(border_lines(cell, r, c, theme.foreground));
                    let right = matches!(cell.align, kalem_viewer::Align::Right)
                        || (cell.numeric && matches!(cell.align, kalem_viewer::Align::General));
                    if right {
                        d = d.justify_end();
                    } else if matches!(cell.align, kalem_viewer::Align::Center) {
                        d = d.justify_center();
                    } else if cell.indent > 0 {
                        // Indented: about three characters a level.
                        d = d.pl(px(PAD + f32::from(cell.indent) * 9.0));
                    }
                    if let Some(f) = cell.fill {
                        d = d.bg(rgb(f));
                    }
                    if let Some((len, color)) = cell.bar {
                        // A data bar: behind the text, its share of the width.
                        d = d.child(
                            div()
                                .debug_selector(move || format!("viewer-grid-bar-{r}-{c}"))
                                .absolute()
                                .left(px(1.))
                                .top(px(2.))
                                .bottom(px(2.))
                                .w(px((w - 2.) * f32::from(len) / 1000.))
                                .bg(rgb(color))
                                .opacity(0.6),
                        );
                    }
                    if let Some(line) = cell.sparkline.clone() {
                        d = d.child(sparkline(line, r, c));
                    }
                    if let Some((glyph, color)) = &cell.icon {
                        // An icon set's icon: at the cell's left, as Excel's.
                        d = d.pl(px(PAD + 14.)).child(
                            div()
                                .debug_selector(move || format!("viewer-grid-icon-{r}-{c}"))
                                .absolute()
                                .left(px(PAD))
                                .top_0()
                                .bottom_0()
                                .flex()
                                .items_center()
                                .text_color(rgb(*color))
                                .child(SharedString::from(glyph.clone())),
                        );
                    }
                    if let Some(c) = cell.color {
                        d = d.text_color(rgb(c));
                    }
                    if cell.bold {
                        d = d.font_weight(gpui::FontWeight::BOLD);
                    }
                    // The cell's own size and typeface.
                    if let Some(tenths) = cell.font_size {
                        d = d.text_size(px(f32::from(tenths) / 10.0 * 4.0 / 3.0));
                    }
                    if let Some(face) = &cell.face {
                        d = d.font_family(SharedString::from(face.clone()));
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
                    // Shrink to Fit: text wider than the cell made smaller.
                    let wide = measure(&cell.text) + 2.0 * PAD;
                    if cell.shrink && wide > w && !cell.wrap {
                        let base = cell
                            .font_size
                            .map_or(theme.size, |t| f32::from(t) / 10.0 * 4.0 / 3.0);
                        d = d.text_size(px((base * (w - 2.0 * PAD) / (wide - 2.0 * PAD)).max(4.0)));
                    }
                    if !spilled.contains(&c) {
                        // A number too wide shows as #, as Excel shows it;
                        // text ends in an ellipsis.
                        let text = if cell.numeric && !cell.shrink && wide > w {
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
                // A filter's header: its button, filled when the column filters.
                if let Some(f) = layout.filter
                    && r == f[0]
                    && (f[1]..=f[3]).contains(&c)
                {
                    let on = layout.filtered.contains(&c);
                    d = d.child(
                        div()
                            .debug_selector(move || format!("viewer-grid-filter-{c}"))
                            .id(SharedString::from(format!("filter-{c}")))
                            .absolute()
                            .right(px(2.))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .text_color(if on { theme.link } else { theme.muted })
                            .child(SharedString::from(if on { "▼" } else { "▾" }))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    this.run_command(
                                        "viewer.grid.filterColumn",
                                        serde_json::json!({ "col": c }),
                                        window,
                                        cx,
                                    );
                                }),
                            ),
                    );
                }
                if invalid.contains(&(r, c)) {
                    // Circle Invalid Data: a red ring around the cell.
                    d = d.child(
                        div()
                            .debug_selector(move || format!("viewer-grid-invalid-{r}-{c}"))
                            .absolute()
                            .inset(px(1.))
                            .rounded_full()
                            .border_2()
                            .border_color(gpui::rgb(0xE0_3C_31)),
                    );
                }
                if here {
                    d = d.child(div().absolute().inset_0().border_2().border_color(cursor));
                }
                if here && has_list {
                    // A list validation's drop-down button, as Excel's.
                    d = d.child(
                        div()
                            .debug_selector(|| "viewer-grid-list".to_string())
                            .id("validation-list")
                            .absolute()
                            .right(px(2.))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .text_color(theme.muted)
                            .child("▾")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    this.run_command(
                                        "viewer.grid.pickFromList",
                                        serde_json::json!({}),
                                        window,
                                        cx,
                                    );
                                }),
                            ),
                    );
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
            let spills = overflow.into_iter().filter_map(|(x, span, c, centered)| {
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
                d = valign(d, cell.valign);
                if centered {
                    d = d.justify_center();
                }
                if let Some(c) = cell.color {
                    d = d.text_color(rgb(c));
                }
                if cell.bold {
                    d = d.font_weight(gpui::FontWeight::BOLD);
                }
                // The cell's own size and typeface.
                if let Some(tenths) = cell.font_size {
                    d = d.text_size(px(f32::from(tenths) / 10.0 * 4.0 / 3.0));
                }
                if let Some(face) = &cell.face {
                    d = d.font_family(SharedString::from(face.clone()));
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
                    .children(merges)
                    .children(charts)
                    .children(drawing_views)
                    .children(cut_mark)
                    .children(fill_frame)
                    .children(pointer_frame)
                    .children(arrow_layer)
                    .children(fill_handle),
            )
            // A fill handle dropped past the grid still fills.
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    if let Some((src, target)) = this.viewer_view.fill_drag.take() {
                        if target != src
                            && let Some(v) = this.doc.viewer.as_deref_mut()
                            && let Err(e) = {
                                v.set_fill_lists(kalem_core::viewer::fill_lists(
                                    &this.shared.config,
                                ));
                                v.fill_to(src, target, true)
                            }
                        {
                            this.message(e, true);
                        }
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if let Some((src, target)) = this.viewer_view.fill_drag.as_mut() {
                    if ev.pressed_button != Some(MouseButton::Left) {
                        this.viewer_view.fill_drag = None;
                        cx.notify();
                        return;
                    }
                    let origin = this
                        .viewer_view
                        .bounds
                        .map_or(point(px(0.), px(0.)), |b| b.origin);
                    let at = ev.position - origin;
                    let (cols, rows) = &this.viewer_view.grid_lines;
                    if let (Some(row), Some(col)) = (
                        line_at(rows, f32::from(at.y)),
                        line_at(cols, f32::from(at.x)),
                    ) {
                        // The way the pointer has gone furthest past the cells.
                        let s = *src;
                        let down = row.saturating_sub(s[2]);
                        let up = s[0].saturating_sub(row);
                        let right = col.saturating_sub(s[3]);
                        let left = s[1].saturating_sub(col);
                        *target = if down.max(up) == 0 && right.max(left) == 0 {
                            s
                        } else if down.max(up) >= right.max(left) {
                            if down > 0 {
                                [s[0], s[1], row, s[3]]
                            } else {
                                [row, s[1], s[2], s[3]]
                            }
                        } else if right > 0 {
                            [s[0], s[1], s[2], col]
                        } else {
                            [s[0], col, s[2], s[3]]
                        };
                    }
                    cx.notify();
                    return;
                }
                if let Some(d) = this.viewer_view.chart_drag.as_mut() {
                    if ev.pressed_button != Some(MouseButton::Left) {
                        this.viewer_view.chart_drag = None;
                    } else {
                        let m = ev.position - d.start;
                        d.delta = (f32::from(m.x), f32::from(m.y));
                    }
                    cx.notify();
                    return;
                }
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
                    if let Some((src, target)) = this.viewer_view.fill_drag.take() {
                        if target != src
                            && let Some(v) = this.doc.viewer.as_deref_mut()
                            && let Err(e) = {
                                v.set_fill_lists(kalem_core::viewer::fill_lists(
                                    &this.shared.config,
                                ));
                                v.fill_to(src, target, true)
                            }
                        {
                            this.message(e, true);
                        }
                        cx.notify();
                        return;
                    }
                    if let Some(d) = this.viewer_view.chart_drag.take() {
                        // Dropped: the cells under its new box.
                        let (cols, rows) = &this.viewer_view.grid_lines;
                        let (x, y, w, h) = d.rect;
                        let moved = d.delta.0.abs() >= 3.0 || d.delta.1.abs() >= 3.0;
                        let target = if d.resize {
                            (
                                line_at(rows, y + 2.0),
                                line_at(cols, x + 2.0),
                                line_at(rows, y + h + d.delta.1 - 2.0),
                                line_at(cols, x + w + d.delta.0 - 2.0),
                            )
                        } else {
                            let top = line_at(rows, y + d.delta.1 + 2.0);
                            let left = line_at(cols, x + d.delta.0 + 2.0);
                            let (r0, c0) = (line_at(rows, y + 2.0), line_at(cols, x + 2.0));
                            let (r1, c1) = (line_at(rows, y + h - 2.0), line_at(cols, x + w - 2.0));
                            let span = r0
                                .zip(r1)
                                .map(|(a, b)| b - a)
                                .zip(c0.zip(c1).map(|(a, b)| b - a));
                            match (top, left, span) {
                                (Some(t), Some(l), Some((dr, dc))) => {
                                    (Some(t), Some(l), Some(t + dr), Some(l + dc))
                                }
                                _ => (None, None, None, None),
                            }
                        };
                        if moved
                            && let (Some(r0), Some(c0), Some(r1), Some(c1)) = target
                            && let Some(v) = this.doc.viewer.as_deref_mut()
                        {
                            let anchor = [r0, c0, r1.max(r0), c1.max(c0)];
                            if let Some(old) = v.charts().get(d.index).map(|c| c.anchor)
                                && old != anchor
                                && let Err(e) = v.move_chart(d.index, anchor)
                            {
                                this.message(e, true);
                            }
                        }
                        cx.notify();
                        return;
                    }
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
                    .shape_line(crate::one_line(t), size, &[run], None)
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
                    .shape_line(crate::one_line(t), size, &[run], None)
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

/// A cell's borders: a line along each side drawn, two pixels when
/// thick; automatic (black) in the text's color, seen on any theme.
fn border_lines(cell: &kalem_viewer::GridCell, r: u32, c: u32, text: gpui::Hsla) -> Vec<gpui::Div> {
    (0..4)
        .filter_map(|i| {
            let col = cell.borders[i]?;
            let t = px(if cell.border_thick[i] { 2. } else { 1. });
            let color = if col == [0, 0, 0] {
                text
            } else {
                gpui::rgb(u32::from(col[0]) << 16 | u32::from(col[1]) << 8 | u32::from(col[2]))
                    .into()
            };
            let d = div()
                .debug_selector(move || format!("viewer-grid-border-{r}-{c}-{i}"))
                .absolute()
                .bg(color);
            Some(match i {
                // The right and bottom ones over the cell's gridline.
                0 => d.top_0().left_0().right(px(-1.)).h(t),
                1 => d.top_0().bottom(px(-1.)).right(px(-1.)).w(t),
                2 => d.bottom(px(-1.)).left_0().right(px(-1.)).h(t),
                _ => d.top_0().bottom(px(-1.)).left_0().w(t),
            })
        })
        .collect()
}

/// A cell's text placed up and down as its vertical alignment says
/// (Excel's default is the bottom).
fn valign<E: gpui::Styled>(d: E, v: kalem_viewer::VAlign) -> E {
    match v {
        kalem_viewer::VAlign::Top => d.items_start(),
        kalem_viewer::VAlign::Middle => d.items_center(),
        _ => d.items_end(),
    }
}

/// A sparkline drawn over its cell: a line through its points (the
/// highest and lowest marked), columns from zero, or win/loss halves.
fn sparkline(line: kalem_viewer::Sparkline, r: u32, c: u32) -> gpui::Div {
    use kalem_viewer::SparklineKind;
    let rgb = |c: [u8; 3]| -> gpui::Hsla {
        gpui::rgb(u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2])).into()
    };
    let (color, marker) = (rgb(line.color), rgb(line.marker));
    div()
        .debug_selector(move || format!("viewer-grid-sparkline-{r}-{c}"))
        .absolute()
        .left(px(3.))
        .right(px(3.))
        .top(px(3.))
        .bottom(px(3.))
        .child(
            gpui::canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    let n = line.points.len();
                    if n == 0 {
                        return;
                    }
                    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                    let o = bounds.origin;
                    let y = |v: u16| h - h * f32::from(v) / 1000.0;
                    let at = |x: f32, yy: f32| gpui::point(o.x + px(x), o.y + px(yy));
                    let slot = w / n as f32;
                    match line.kind {
                        SparklineKind::Line => {
                            let x = |i: usize| {
                                if n == 1 {
                                    w / 2.0
                                } else {
                                    w * i as f32 / (n - 1) as f32
                                }
                            };
                            let mut p = gpui::PathBuilder::stroke(px(1.25));
                            let mut down = false;
                            for (i, v) in line.points.iter().enumerate() {
                                match v {
                                    // A gap where a value is missing.
                                    None => down = false,
                                    Some(v) if down => p.line_to(at(x(i), y(*v))),
                                    Some(v) => {
                                        p.move_to(at(x(i), y(*v)));
                                        down = true;
                                    }
                                }
                            }
                            if let Ok(path) = p.build() {
                                window.paint_path(path, color);
                            }
                            for k in [line.high, line.low].into_iter().flatten() {
                                if let Some(Some(v)) = line.points.get(k) {
                                    window.paint_quad(gpui::fill(
                                        gpui::Bounds::new(
                                            at(x(k) - 2.0, y(*v) - 2.0),
                                            gpui::size(px(4.), px(4.)),
                                        ),
                                        marker,
                                    ));
                                }
                            }
                        }
                        SparklineKind::Column | SparklineKind::WinLoss => {
                            let zero = y(line.zero.unwrap_or(0));
                            for (i, v) in line.points.iter().enumerate() {
                                let Some(v) = *v else { continue };
                                let (top, bottom) = match line.kind {
                                    SparklineKind::WinLoss if v == 500 => continue,
                                    SparklineKind::WinLoss if v > 500 => (0.0, h / 2.0),
                                    SparklineKind::WinLoss => (h / 2.0, h),
                                    // At least a pixel high.
                                    _ => (y(v).min(zero), y(v).max(zero).max(y(v).min(zero) + 1.0)),
                                };
                                let below = line.zero.is_some_and(|z| v < z);
                                let marked = below || Some(i) == line.high || Some(i) == line.low;
                                window.paint_quad(gpui::fill(
                                    gpui::Bounds::new(
                                        at(slot * i as f32 + slot * 0.15, top),
                                        gpui::size(px(slot * 0.7), px(bottom - top)),
                                    ),
                                    if marked { marker } else { color },
                                ));
                            }
                        }
                    }
                },
            )
            .size_full(),
        )
}
