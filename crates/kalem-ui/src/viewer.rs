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

/// The textures shown, by generation, unit and turn.
type Key = (u64, usize, u8);

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
        let key = (v.generation(), v.unit, v.rotation);
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
}
