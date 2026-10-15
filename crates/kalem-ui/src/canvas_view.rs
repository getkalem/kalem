//! A plugin's document drawn as a canvas (`kalem_core::canvas_view`,
//! plugin API 0.2.16): its shapes where the plugin placed them (boxes,
//! ellipses, frames, text, pictures and pen strokes, with their text
//! wrapped in them) and the connectors over them, with their arrows and
//! labels. Dragging pans, the wheel zooms about the mouse (with Shift it
//! pans), `0` fits; a click chooses a shape and a double click (or Enter)
//! opens one that leads somewhere, which runs the plugin's command. Show
//! Source shows the plugin's text.

use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Bounds, Context, Div, Hsla, InteractiveElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, PathBuilder, Pixels, Point, RenderImage,
    ScrollWheelEvent, SharedString, Styled, Window, div, point, px,
};
use kalem_core::canvas_view::{CanvasView, Color, ShapeKind, arrow_head, wrap};

use crate::editor::Editor;

/// The window's side of a canvas: where it is drawn and what the mouse
/// does.
#[derive(Debug, Default)]
pub struct CanvasUi {
    bounds: Option<Bounds<Pixels>>,
    /// Panning, the mouse last here.
    pan: Option<Point<Pixels>>,
    /// Where the press began.
    press: Option<Point<Pixels>>,
}

/// How a shape is painted, in the canvas element's pixels.
enum Look {
    /// A rectangle: its bounds, corner radius, fill and line.
    Rect([f32; 4], f32, Option<Hsla>, Option<Hsla>, bool),
    /// An ellipse in its bounds.
    Ellipse([f32; 4], Option<Hsla>, Option<Hsla>, bool),
    /// A picture in its bounds, or its name where it cannot be read.
    Picture([f32; 4], Option<Arc<RenderImage>>),
    /// A pen's line.
    Stroke(Vec<[f32; 2]>, Hsla),
    /// Text alone.
    Bare,
}

/// A shape painted: how, its text's lines and their places, and whether
/// it is the one chosen.
struct Painted {
    look: Look,
    text: Vec<(SharedString, [f32; 2])>,
    text_color: Hsla,
    /// The size its text is painted at.
    font: f32,
    rect: [f32; 4],
    chosen: bool,
}

/// A connector painted: its ends, color, dash, arrows' heads (the tip and
/// its two wings), and its label, where it starts and how wide it is.
struct Wire {
    a: [f32; 2],
    b: [f32; 2],
    color: Hsla,
    dashed: bool,
    heads: Vec<[[f32; 2]; 3]>,
    label: Option<(SharedString, [f32; 2], f32)>,
}

/// What a frame paints, taken from the view before painting.
struct Frame {
    shapes: Vec<Painted>,
    /// The connectors.
    lines: Vec<Wire>,
    font: Pixels,
}

/// The colors of the theme a canvas is painted in.
#[derive(Clone, Copy)]
struct Palette {
    text: Hsla,
    line: Hsla,
    muted: Hsla,
    background: Hsla,
}

fn hsla(c: Color) -> Hsla {
    let (r, g, b) = c.rgb();
    Hsla::from(gpui::rgb(
        (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
    ))
}

impl Editor {
    /// The canvas the document is drawn as, when it is one and its source
    /// is not shown.
    pub(crate) fn canvas_element(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<'_, Editor>,
    ) -> Option<Div> {
        if self.source {
            return None;
        }
        self.doc.canvas_view.as_deref()?;
        let theme = self.theme.clone();
        let entity = cx.entity();
        let palette = Palette {
            text: theme.foreground,
            line: theme.foreground.opacity(0.55),
            muted: theme.muted,
            background: theme.background,
        };
        let ring = theme.caret;
        let font = theme.font.clone();
        let canvas = gpui::canvas(
            move |bounds, _window, cx| {
                entity.update(cx, |e, _| {
                    e.canvas_ui.bounds = Some(bounds);
                    let size = [f32::from(bounds.size.width), f32::from(bounds.size.height)];
                    let pictures = &e.shared.pictures;
                    let v = e.doc.canvas_view.as_deref_mut()?;
                    v.follow(size);
                    Some(frame(v, size, palette, |p| {
                        pictures.get(p).map(|(image, _, _)| image)
                    }))
                })
            },
            move |bounds, frame, window, cx| {
                let Some(f) = frame else {
                    return;
                };
                let at = |p: [f32; 2]| bounds.origin + point(px(p[0]), px(p[1]));
                let rect =
                    |r: [f32; 4]| Bounds::new(at([r[0], r[1]]), gpui::size(px(r[2]), px(r[3])));
                let style = |dashed: bool| {
                    if dashed {
                        gpui::BorderStyle::Dashed
                    } else {
                        gpui::BorderStyle::Solid
                    }
                };
                window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                    for s in &f.shapes {
                        match &s.look {
                            Look::Rect(r, radius, fill, line, dashed) => {
                                if let Some(c) = fill {
                                    window.paint_quad(
                                        gpui::fill(rect(*r), *c).corner_radii(px(*radius)),
                                    );
                                }
                                if let Some(c) = line {
                                    window.paint_quad(
                                        gpui::outline(rect(*r), *c, style(*dashed))
                                            .corner_radii(px(*radius))
                                            .border_widths(px(1.5)),
                                    );
                                }
                            }
                            Look::Ellipse(r, fill, line, dashed) => {
                                let points: Vec<Point<Pixels>> = (0..64)
                                    .map(|k| {
                                        let a = k as f32 / 64.0 * std::f32::consts::TAU;
                                        at([
                                            r[0] + r[2] / 2.0 * (1.0 + a.cos()),
                                            r[1] + r[3] / 2.0 * (1.0 + a.sin()),
                                        ])
                                    })
                                    .collect();
                                if let Some(c) = fill {
                                    let mut p = PathBuilder::fill();
                                    p.add_polygon(&points, true);
                                    if let Ok(path) = p.build() {
                                        window.paint_path(path, *c);
                                    }
                                }
                                if let Some(c) = line {
                                    let mut p = PathBuilder::stroke(px(1.5));
                                    if *dashed {
                                        p = p.dash_array(&[px(6.), px(4.)]);
                                    }
                                    p.add_polygon(&points, true);
                                    if let Ok(path) = p.build() {
                                        window.paint_path(path, *c);
                                    }
                                }
                            }
                            Look::Picture(r, image) => match image {
                                Some(image) => {
                                    let _ = window.paint_image(
                                        rect(*r),
                                        rect(*r),
                                        Default::default(),
                                        image.clone(),
                                        0,
                                        false,
                                    );
                                }
                                None => window.paint_quad(
                                    gpui::outline(
                                        rect(*r),
                                        palette.muted,
                                        gpui::BorderStyle::Dashed,
                                    )
                                    .border_widths(px(1.)),
                                ),
                            },
                            Look::Stroke(points, color) => {
                                let mut p = PathBuilder::stroke(px((2.0 * f32::from(f.font)
                                    / 16.0)
                                    .clamp(1.0, 6.0)));
                                for (k, q) in points.iter().enumerate() {
                                    if k == 0 {
                                        p.move_to(at(*q));
                                    } else {
                                        p.line_to(at(*q));
                                    }
                                }
                                if points.len() > 1
                                    && let Ok(path) = p.build()
                                {
                                    window.paint_path(path, *color);
                                }
                            }
                            Look::Bare => {}
                        }
                        if s.chosen {
                            let r = s.rect;
                            let b = rect([r[0] - 3.0, r[1] - 3.0, r[2] + 6.0, r[3] + 6.0]);
                            window.paint_quad(
                                gpui::outline(b, ring, gpui::BorderStyle::Solid)
                                    .corner_radii(px(4.0))
                                    .border_widths(px(2.0)),
                            );
                        }
                        for (line, p) in &s.text {
                            paint_text(line, at(*p), px(s.font), s.text_color, &font, window, cx);
                        }
                    }
                    for Wire {
                        a,
                        b,
                        color,
                        dashed,
                        heads,
                        label,
                    } in &f.lines
                    {
                        let mut p = PathBuilder::stroke(px(1.5));
                        if *dashed {
                            p = p.dash_array(&[px(6.), px(4.)]);
                        }
                        p.move_to(at(*a));
                        p.line_to(at(*b));
                        for [tip, l, r] in heads {
                            p.move_to(at(*l));
                            p.line_to(at(*tip));
                            p.line_to(at(*r));
                        }
                        if let Ok(path) = p.build() {
                            window.paint_path(path, *color);
                        }
                        if let Some((text, at_, w)) = label {
                            // On the line, over a patch of the background.
                            let h = f32::from(f.font) * 1.25;
                            window.paint_quad(gpui::fill(
                                rect([at_[0] - 3.0, at_[1], w + 6.0, h]),
                                palette.background,
                            ));
                            paint_text(text, at(*at_), f.font, palette.text, &font, window, cx);
                        }
                    }
                });
            },
        )
        .size_full();
        let pointing = self.doc.canvas_view.as_deref().is_some_and(|v| {
            v.hovered
                .is_some_and(|i| v.canvas().shapes[i].target.is_some())
        });
        let area = div()
            .id("canvas-view")
            .debug_selector(|| "canvas-view".into())
            .size_full()
            .overflow_hidden()
            .bg(theme.background)
            .when(pointing, |d| d.cursor_pointer())
            .child(canvas)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    let handle = gpui::Focusable::focus_handle(this, cx);
                    window.focus(&handle, cx);
                    let (at, size) = this.canvas_point(ev.position);
                    let Some(v) = this.doc.canvas_view.as_deref_mut() else {
                        return;
                    };
                    let hit = v.shape_at(at, size);
                    if ev.click_count >= 2 && hit.is_some() {
                        v.selected = hit;
                        this.canvas_ui.pan = None;
                        this.canvas_ui.press = None;
                        this.open_canvas_shape(window, cx);
                        return;
                    }
                    this.canvas_ui.pan = Some(ev.position);
                    this.canvas_ui.press = Some(ev.position);
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseUpEvent, _, cx| {
                    this.canvas_ui.pan = None;
                    let press = this.canvas_ui.press.take();
                    let (at, size) = this.canvas_point(ev.position);
                    let Some(v) = this.doc.canvas_view.as_deref_mut() else {
                        return;
                    };
                    // A click, not a drag: chooses the shape under it, or
                    // none.
                    let moved = press.map_or(f32::MAX, |p| {
                        let d = ev.position - p;
                        f32::from(d.x).hypot(f32::from(d.y))
                    });
                    if moved <= 4.0 {
                        v.selected = v.shape_at(at, size);
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                let (at, size) = this.canvas_point(ev.position);
                let pan = this.canvas_ui.pan;
                let Some(v) = this.doc.canvas_view.as_deref_mut() else {
                    return;
                };
                match pan {
                    Some(_) if ev.pressed_button != Some(MouseButton::Left) => {
                        this.canvas_ui.pan = None;
                    }
                    Some(from) => {
                        let d = ev.position - from;
                        v.pan_by([f32::from(d.x), f32::from(d.y)]);
                        this.canvas_ui.pan = Some(ev.position);
                        cx.notify();
                    }
                    None => {
                        let over = v.shape_at(at, size);
                        if over != v.hovered {
                            v.hovered = over;
                            cx.notify();
                        }
                    }
                }
            }))
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                let d = ev.delta.pixel_delta(px(20.));
                let (at, size) = this.canvas_point(ev.position);
                let Some(v) = this.doc.canvas_view.as_deref_mut() else {
                    return;
                };
                // The wheel zooms about the mouse; with Shift it pans.
                if ev.modifiers.shift {
                    v.pan_by([f32::from(d.x), f32::from(d.y)]);
                } else {
                    v.zoom_at(1.003_f32.powf(f32::from(d.y)), at, size);
                }
                cx.notify();
            }));
        Some(div().size_full().child(area))
    }

    /// A point of the window as one of the canvas's element, and the
    /// element's size.
    fn canvas_point(&self, p: Point<Pixels>) -> ([f32; 2], [f32; 2]) {
        let b = self.canvas_ui.bounds.unwrap_or_default();
        let at = p - b.origin;
        (
            [f32::from(at.x), f32::from(at.y)],
            [f32::from(b.size.width), f32::from(b.size.height)],
        )
    }

    /// Opens the shape chosen, when it leads somewhere: runs the canvas's
    /// command with its target.
    fn open_canvas_shape(&mut self, window: &mut Window, cx: &mut Context<'_, Editor>) {
        let Some(v) = self.doc.canvas_view.as_deref() else {
            return;
        };
        let Some(target) = v.target().map(str::to_string) else {
            return;
        };
        let command = v.canvas().command.clone();
        self.run_command(&command, serde_json::json!({ "shape": target }), window, cx);
    }

    /// The canvas's keys: Enter opens the shape chosen, Tab and Shift+Tab
    /// choose another that leads somewhere (in reading order), the arrows
    /// pan, `+` and `-` zoom, `0` fits the canvas. Whether the key was the
    /// canvas's.
    pub(crate) fn canvas_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<'_, Editor>,
    ) -> bool {
        if self.source {
            return false;
        }
        // Vim's keys while its command line is open or a command is half
        // typed.
        if self
            .vim
            .as_ref()
            .is_some_and(|v| v.command_line.is_some() || !(v.takes_text() || v.idle_command()))
        {
            return false;
        }
        let size = self.canvas_ui.bounds.map_or([800.0, 600.0], |b| {
            [f32::from(b.size.width), f32::from(b.size.height)]
        });
        let Some(v) = self.doc.canvas_view.as_deref_mut() else {
            return false;
        };
        let k = &ev.keystroke;
        let m = k.modifiers;
        if m.control || m.platform || m.alt {
            return false;
        }
        let middle = [size[0] / 2.0, size[1] / 2.0];
        match k.key.as_str() {
            "enter" => {
                self.open_canvas_shape(window, cx);
                return true;
            }
            "tab" => {
                v.select_next(m.shift);
                v.center_on_selected();
            }
            "left" => v.pan_by([60.0, 0.0]),
            "right" => v.pan_by([-60.0, 0.0]),
            "up" => v.pan_by([0.0, 60.0]),
            "down" => v.pan_by([0.0, -60.0]),
            "+" | "=" => v.zoom_at(1.25, middle, size),
            "-" => v.zoom_at(0.8, middle, size),
            "0" => v.fit(size),
            _ => return false,
        }
        cx.notify();
        true
    }
}

/// Writes `text` with its top left at `at`.
fn paint_text(
    text: &SharedString,
    at: Point<Pixels>,
    size: Pixels,
    color: Hsla,
    font: &str,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let run = gpui::TextRun {
        len: text.len(),
        font: gpui::font(SharedString::from(font.to_string())),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped = window
        .text_system()
        .shape_line(text.clone(), size, &[run], None);
    let _ = shaped.paint(at, size * 1.25, gpui::TextAlign::Left, None, window, cx);
}

/// What view `v` paints in an element of `size`: the shapes on it with
/// their text in lines that fit them, the connectors, in a font that grows
/// with the zoom, a shape's own size when it has one (none below a readable
/// size); `picture` gives an image
/// file's picture.
fn frame(
    v: &CanvasView,
    size: [f32; 2],
    palette: Palette,
    picture: impl Fn(&std::path::Path) -> Option<Arc<RenderImage>>,
) -> Frame {
    let c = v.canvas();
    let font = (16.0 * v.zoom).min(48.0);
    let readable = font >= 6.0;
    // A letter about half the font's size wide; a line 1.25 of it high.
    let (letter, line) = (font * 0.55, font * 1.25);
    let screen = |p: [f32; 2]| v.to_screen(p, size);
    let mut f = Frame {
        shapes: Vec::with_capacity(c.shapes.len()),
        lines: Vec::with_capacity(c.connectors.len()),
        font: px(font),
    };
    for (i, s) in c.shapes.iter().enumerate() {
        let a = screen([s.rect[0], s.rect[1]]);
        let r = [a[0], a[1], s.rect[2] * v.zoom, s.rect[3] * v.zoom];
        // Off the window: left out.
        if r[0] > size[0] || r[1] > size[1] || r[0] + r[2] < 0.0 || r[1] + r[3] < 0.0 {
            continue;
        }
        let stroke = s.stroke.map_or(palette.line, hsla);
        let fill = s.fill.map(|c| hsla(c).opacity(0.25));
        let look = match &s.kind {
            ShapeKind::Box(radius) => Look::Rect(r, radius * v.zoom, fill, Some(stroke), s.dashed),
            ShapeKind::Ellipse => Look::Ellipse(r, fill, Some(stroke), s.dashed),
            ShapeKind::Group => Look::Rect(
                r,
                6.0,
                fill,
                Some(s.stroke.map_or(palette.muted, hsla)),
                true,
            ),
            ShapeKind::Image(path) => Look::Picture(r, picture(path)),
            ShapeKind::Stroke(points) => Look::Stroke(
                points
                    .iter()
                    .map(|p| screen([s.rect[0] + p[0], s.rect[1] + p[1]]))
                    .collect(),
                s.stroke.map_or(palette.text, hsla),
            ),
            ShapeKind::Text => match fill {
                Some(fill) => Look::Rect(r, 0.0, Some(fill), None, false),
                None => Look::Bare,
            },
        };
        // The text: in lines that fit its width, in the middle of a box or
        // an ellipse, at the top of text, a frame's above it, a picture's
        // name in it.
        let mut text = Vec::new();
        let words = match &s.kind {
            ShapeKind::Image(path) if matches!(look, Look::Picture(_, None)) => path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            ShapeKind::Image(_) | ShapeKind::Stroke(_) => String::new(),
            _ => s.text.clone(),
        };
        // In its own size (API 0.2.19), else the window's.
        let font_s = s.text_px(v.zoom);
        let (letter_s, line_s) = (font_s * 0.55, font_s * 1.25);
        if font_s >= 6.0 && !words.trim().is_empty() {
            let pad = 6.0 * v.zoom.min(1.0);
            let width = ((r[2] - 2.0 * pad) / letter_s).floor().max(1.0) as usize;
            let lines = wrap(&words, width);
            let tall = lines.len() as f32 * line_s;
            let top = match s.kind {
                ShapeKind::Group => r[1] - line_s - 2.0,
                ShapeKind::Text => r[1],
                _ => r[1] + ((r[3] - tall) / 2.0).max(pad),
            };
            for (k, l) in lines.into_iter().enumerate() {
                let y = top + k as f32 * line_s;
                if s.kind != ShapeKind::Group
                    && s.kind != ShapeKind::Text
                    && y + line_s > r[1] + r[3]
                {
                    break;
                }
                let w = l.chars().count() as f32 * letter_s;
                let x = match s.kind {
                    ShapeKind::Text | ShapeKind::Group => {
                        r[0] + if s.kind == ShapeKind::Group {
                            0.0
                        } else {
                            pad.min(2.0)
                        }
                    }
                    _ => r[0] + ((r[2] - w) / 2.0).max(pad),
                };
                text.push((SharedString::from(l), [x, y]));
            }
        }
        f.shapes.push(Painted {
            look,
            text,
            font: font_s,
            text_color: if s.kind == ShapeKind::Group {
                palette.muted
            } else {
                palette.text
            },
            rect: r,
            chosen: v.selected == Some(i),
        });
    }
    for k in &c.connectors {
        let (a, b) = c.ends(k);
        let (a, b) = (screen(a), screen(b));
        let head = (10.0 * v.zoom).clamp(4.0, 16.0);
        let mut heads = Vec::new();
        if k.arrow_end {
            let [l, r] = arrow_head(b, a, head);
            heads.push([b, l, r]);
        }
        if k.arrow_start {
            let [l, r] = arrow_head(a, b, head);
            heads.push([a, l, r]);
        }
        let label = (readable && !k.label.trim().is_empty()).then(|| {
            let w = k.label.chars().count() as f32 * letter;
            (
                SharedString::from(k.label.clone()),
                [
                    (a[0] + b[0]) / 2.0 - w / 2.0,
                    (a[1] + b[1]) / 2.0 - line / 2.0,
                ],
                w,
            )
        });
        f.lines.push(Wire {
            a,
            b,
            color: k.stroke.map_or(palette.line, hsla),
            dashed: k.dashed,
            heads,
            label,
        });
    }
    f
}
