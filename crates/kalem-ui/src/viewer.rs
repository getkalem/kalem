//! The view of a file a viewer plugin opened (design §11.13): the unit's
//! bitmap where `kalem_core::viewer` places it, panned by dragging and the
//! wheel, zoomed with the wheel and Command or Control at the mouse, the
//! information panel at the right, frames played on a timer.

use std::sync::Arc;
use std::time::Duration;

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
    /// The area as last laid out.
    pub bounds: Option<Bounds<Pixels>>,
    /// The next frame's timer.
    timer: Option<Task<()>>,
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
        let key = (
            v.generation(),
            v.unit,
            v.rotation,
            v.render_scale().to_bits(),
        );
        if let Some((_, img)) = self.viewer_view.images.iter().find(|(k, _)| *k == key) {
            return Ok(img.clone());
        }
        let bitmap = v.bitmap()?;
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
        let area = match self.viewer_image(window, cx) {
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    this.viewer_view.drag = Some(ev.position);
                    let handle = gpui::Focusable::focus_handle(this, cx);
                    window.focus(&handle, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| {
                    this.viewer_view.drag = None;
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                let Some(from) = this.viewer_view.drag else {
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
                    v.pan(-f32::from(d.x), -f32::from(d.y));
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
        // A digit's width: Excel's column widths count them.
        let digit = f32::from(
            window
                .text_system()
                .shape_line("0".into(), px(theme.size), &[run("0")], None)
                .width,
        );
        let row_h = (theme.size * 1.6).round();
        let bounds = self.viewer_view.bounds.map_or((900.0, 600.0), |b| {
            (f32::from(b.size.width), f32::from(b.size.height))
        });
        let Some(v) = self.doc.viewer.as_deref_mut() else {
            return div();
        };
        let Some(layout) = v.grid_layout() else {
            return div();
        };
        let col_px = |c: u32| -> f32 {
            let w = layout
                .widths
                .get(c as usize)
                .copied()
                .unwrap_or(layout.default_width);
            (w * digit + 10.0).clamp(24.0, 800.0)
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
            bounds.1 - row_h,
            &|_| row_h,
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
        let rgb = |c: [u8; 3]| -> gpui::Hsla {
            gpui::rgb(u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2])).into()
        };
        let header_bg = theme.bar;
        let cursor = theme.caret;
        let letters = div()
            .flex()
            .flex_row()
            .flex_none()
            .h(px(row_h))
            .bg(header_bg)
            .border_b_1()
            .border_color(theme.border)
            .child(div().w(px(gutter)).flex_none())
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
                    .child(SharedString::from(name))
            }));
        let body = rows.iter().map(|&(r, _)| {
            let number = div()
                .w(px(gutter))
                .flex_none()
                .h_full()
                .flex()
                .items_center()
                .justify_end()
                .pr(px(6.))
                .bg(header_bg)
                .border_r_1()
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
                .child(SharedString::from((r + 1).to_string()));
            let row_cells = cols.iter().map(|&(c, w)| {
                let cell = cells.get(&(r, c));
                let here = (r, c) == (pos.row, pos.col);
                let mut d = div()
                    .id(SharedString::from(format!("cell-{r}-{c}")))
                    .w(px(w))
                    .flex_none()
                    .h_full()
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .whitespace_nowrap()
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
                    if cell.note {
                        d = d.border_t_2().border_color(theme.todo);
                    }
                    d = d.child(SharedString::from(cell.text.clone()));
                }
                if here {
                    d = d.border_2().border_color(cursor);
                }
                d.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                        if let Some(v) = this.doc.viewer.as_deref_mut() {
                            v.grid_move_to(r, c);
                        }
                        let handle = gpui::Focusable::focus_handle(this, cx);
                        window.focus(&handle, cx);
                        if ev.click_count >= 2 {
                            this.run_command("viewer.grid.edit", serde_json::json!({}), window, cx);
                        }
                        cx.notify();
                    }),
                )
            });
            div()
                .flex()
                .flex_row()
                .flex_none()
                .h(px(row_h))
                .child(number)
                .children(row_cells)
        });
        let prepaint = entity.clone();
        div()
            .debug_selector(|| "viewer-grid".into())
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(theme.background)
            .text_size(px(theme.size))
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
            .child(div().flex().flex_col().child(letters).children(body))
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
}
