//! A plugin's document drawn as a graph (`kalem_core::graph_view`, plugin
//! API 0.2.12): the nodes as dots colored by their group, the edges as
//! lines, the labels of the nodes large enough to read or near the mouse.
//! The layout moves a few steps each frame until it settles. Dragging the
//! background pans, the wheel zooms about the mouse, dragging a node holds
//! it; a click chooses a node and a double click (or Enter) opens it, which
//! runs the plugin's command. Show Source shows the plugin's text.

use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Bounds, Context, Div, InteractiveElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, PathBuilder, Pixels, Point, ScrollWheelEvent,
    SharedString, Styled, Window, div, point, px,
};

use crate::editor::Editor;

/// The groups' colors but the most common group's (the theme's muted
/// grey): soft, apart from one another, readable on light and dark
/// backgrounds.
const PALETTE: [u32; 8] = [
    0x4E79A7, 0xF28E2B, 0x59A14F, 0xE15759, 0x76B7B2, 0xEDC948, 0xB07AA1, 0x9C755F,
];

/// What a press began.
#[derive(Debug, Clone, Copy)]
enum Drag {
    /// Panning, the mouse last here.
    Pan(Point<Pixels>),
    /// Holding a node.
    Node(usize),
}

/// The window's side of a graph: where it is drawn and what the mouse
/// does.
#[derive(Debug, Default)]
pub struct GraphUi {
    bounds: Option<Bounds<Pixels>>,
    drag: Option<Drag>,
    press: Option<Point<Pixels>>,
}

/// What a frame draws, taken from the view before painting.
struct Frame {
    /// Each node's place in the window, radius, color and faintness.
    nodes: Vec<([f32; 2], f32, gpui::Hsla, bool)>,
    /// The labels written: where, what, whether faint, and whether in
    /// the text's own color (the node chosen or under the mouse).
    labels: Vec<([f32; 2], SharedString, bool, bool)>,
    /// The edges: ends, and whether they touch the node under the mouse.
    edges: Vec<([f32; 2], [f32; 2], bool, bool)>,
    /// The node chosen and the focus: where, and their radius.
    rings: Vec<([f32; 2], f32, bool)>,
}

impl Editor {
    /// The graph the document is drawn as, when it is one and its source
    /// is not shown.
    pub(crate) fn graph_element(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Editor>,
    ) -> Option<Div> {
        if self.source {
            return None;
        }
        let view = self.doc.graph_view.as_deref_mut()?;
        // A frame's share of the layout; the next frame while it moves.
        if view.moving() {
            view.step(Duration::from_millis(8));
            window.request_animation_frame();
        }
        let theme = self.theme.clone();
        let entity = cx.entity();
        let palette = PALETTE.map(|c| gpui::Hsla::from(gpui::rgb(c)));
        let neutral = theme.muted;
        let (edge, near, faint_edge) = (
            theme.muted.opacity(0.35),
            theme.link.opacity(0.9),
            theme.muted.opacity(0.08),
        );
        // The names soft, as Logseq's; the one chosen or under the mouse
        // in the text's color.
        let (text, soft_text, faint_text, ring) = (
            theme.foreground,
            theme.foreground.opacity(0.7),
            theme.muted.opacity(0.4),
            theme.caret,
        );
        let font = theme.font.clone();
        let canvas = gpui::canvas(
            move |bounds, _window, cx| {
                entity.update(cx, |e, _| {
                    e.graph_ui.bounds = Some(bounds);
                    let v = e.doc.graph_view.as_deref_mut()?;
                    let size = [f32::from(bounds.size.width), f32::from(bounds.size.height)];
                    v.follow(size);
                    Some(frame(v, size, &palette, neutral))
                })
            },
            move |bounds, frame, window, cx| {
                let Some(f) = frame else {
                    return;
                };
                let at = |p: [f32; 2]| bounds.origin + point(px(p[0]), px(p[1]));
                // The edges, in three strokes: faint, plain, near the mouse.
                for (want_near, want_faint, color, width) in [
                    (false, true, faint_edge, 1.0),
                    (false, false, edge, 1.0),
                    (true, false, near, 1.6),
                ] {
                    let mut p = PathBuilder::stroke(px(width));
                    let mut any = false;
                    for (a, b, n, fa) in &f.edges {
                        if *n == want_near && *fa == want_faint {
                            p.move_to(at(*a));
                            p.line_to(at(*b));
                            any = true;
                        }
                    }
                    if any && let Ok(path) = p.build() {
                        window.paint_path(path, color);
                    }
                }
                for (c, r, color, faint) in &f.nodes {
                    let color = if *faint { color.opacity(0.2) } else { *color };
                    let b = Bounds::new(
                        at([c[0] - r, c[1] - r]),
                        gpui::size(px(2.0 * r), px(2.0 * r)),
                    );
                    window.paint_quad(gpui::fill(b, color).corner_radii(px(*r)));
                }
                for (c, r, focus) in &f.rings {
                    let r = r + 3.0;
                    let b = Bounds::new(
                        at([c[0] - r, c[1] - r]),
                        gpui::size(px(2.0 * r), px(2.0 * r)),
                    );
                    window.paint_quad(
                        gpui::outline(
                            b,
                            if *focus { text } else { ring },
                            gpui::BorderStyle::Solid,
                        )
                        .corner_radii(px(r))
                        .border_widths(px(2.0)),
                    );
                }
                let size = px(11.0);
                for (p, label, faint, strong) in &f.labels {
                    let run = gpui::TextRun {
                        len: label.len(),
                        font: gpui::font(SharedString::from(font.clone())),
                        color: if *faint {
                            faint_text
                        } else if *strong {
                            text
                        } else {
                            soft_text
                        },
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let shaped = window
                        .text_system()
                        .shape_line(label.clone(), size, &[run], None);
                    let w = f32::from(shaped.width);
                    let _ = shaped.paint(
                        at([p[0] - w / 2.0, p[1]]),
                        size * 1.2,
                        gpui::TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }
            },
        )
        .size_full();
        let hovering = self
            .doc
            .graph_view
            .as_deref()
            .is_some_and(|v| v.hovered.is_some());
        let area = div()
            .id("graph-view")
            .debug_selector(|| "graph-view".into())
            .size_full()
            .overflow_hidden()
            .bg(theme.background)
            .when(hovering, |d| d.cursor_pointer())
            .child(canvas)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    let handle = gpui::Focusable::focus_handle(this, cx);
                    window.focus(&handle, cx);
                    let (at, size) = this.graph_point(ev.position);
                    let Some(v) = this.doc.graph_view.as_deref_mut() else {
                        return;
                    };
                    let hit = v.node_at(at, size);
                    if ev.click_count >= 2 && hit.is_some() {
                        v.selected = hit;
                        this.graph_ui.drag = None;
                        this.graph_ui.press = None;
                        this.open_graph_node(window, cx);
                        return;
                    }
                    this.graph_ui.drag = Some(match hit {
                        Some(i) => Drag::Node(i),
                        None => Drag::Pan(ev.position),
                    });
                    this.graph_ui.press = Some(ev.position);
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseUpEvent, _, cx| {
                    let drag = this.graph_ui.drag.take();
                    let press = this.graph_ui.press.take();
                    let (at, size) = this.graph_point(ev.position);
                    let Some(v) = this.doc.graph_view.as_deref_mut() else {
                        return;
                    };
                    v.release();
                    // A click, not a drag: chooses the node under it,
                    // or none.
                    let moved = press.map_or(f32::MAX, |p| {
                        let d = ev.position - p;
                        f32::from(d.x).hypot(f32::from(d.y))
                    });
                    if moved <= 4.0 {
                        v.selected = match drag {
                            Some(Drag::Node(i)) => Some(i),
                            _ => v.node_at(at, size),
                        };
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                let (at, size) = this.graph_point(ev.position);
                let drag = this.graph_ui.drag;
                let Some(v) = this.doc.graph_view.as_deref_mut() else {
                    return;
                };
                match drag {
                    Some(_) if ev.pressed_button != Some(MouseButton::Left) => {
                        v.release();
                        this.graph_ui.drag = None;
                    }
                    Some(Drag::Pan(from)) => {
                        let d = ev.position - from;
                        v.pan_by([f32::from(d.x), f32::from(d.y)]);
                        this.graph_ui.drag = Some(Drag::Pan(ev.position));
                        cx.notify();
                    }
                    // Drawn again, the layout going on from there: a frame
                    // is asked for while drawing, not here, outside a
                    // view's drawing (gpui's `current_view` panics).
                    Some(Drag::Node(i)) => {
                        v.drag(i, at, size);
                        cx.notify();
                    }
                    None => {
                        let over = v.node_at(at, size);
                        if over != v.hovered {
                            v.hovered = over;
                            cx.notify();
                        }
                    }
                }
            }))
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                let d = ev.delta.pixel_delta(px(20.));
                let (at, size) = this.graph_point(ev.position);
                let Some(v) = this.doc.graph_view.as_deref_mut() else {
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

    /// A point of the window as one of the graph's drawing, and the
    /// drawing's size.
    fn graph_point(&self, p: Point<Pixels>) -> ([f32; 2], [f32; 2]) {
        let b = self.graph_ui.bounds.unwrap_or_default();
        let at = p - b.origin;
        (
            [f32::from(at.x), f32::from(at.y)],
            [f32::from(b.size.width), f32::from(b.size.height)],
        )
    }

    /// Opens the node chosen: runs the graph's command with its id.
    fn open_graph_node(&mut self, window: &mut Window, cx: &mut Context<'_, Editor>) {
        let Some(v) = self.doc.graph_view.as_deref() else {
            return;
        };
        let Some(i) = v.selected else {
            return;
        };
        let command = v.graph().command.clone();
        let id = v.graph().nodes[i].id.clone();
        self.run_command(&command, serde_json::json!({ "node": id }), window, cx);
    }

    /// The graph's keys: Enter opens the node chosen, Tab and Shift+Tab
    /// choose another (the heaviest first), the arrows pan, `+` and `-`
    /// zoom, `0` fits the graph. Whether the key was the graph's.
    pub(crate) fn graph_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<'_, Editor>,
    ) -> bool {
        if self.source {
            return false;
        }
        let size = self.graph_ui.bounds.map_or([800.0, 600.0], |b| {
            [f32::from(b.size.width), f32::from(b.size.height)]
        });
        let Some(v) = self.doc.graph_view.as_deref_mut() else {
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
                self.open_graph_node(window, cx);
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

/// What view `v` draws in a window of `size`: the nodes and edges on it,
/// and the labels to write.
fn frame(
    v: &kalem_core::graph_view::GraphView,
    size: [f32; 2],
    palette: &[gpui::Hsla; 8],
    neutral: gpui::Hsla,
) -> Frame {
    let g = v.graph();
    let n = g.nodes.len();
    let screen: Vec<[f32; 2]> = (0..n).map(|i| v.to_screen(v.place(i), size)).collect();
    let radius: Vec<f32> = (0..n)
        .map(|i| (v.radius(i) * v.zoom).clamp(2.0, 24.0))
        .collect();
    let on = |p: [f32; 2], r: f32| {
        p[0] + r >= 0.0 && p[1] + r >= 0.0 && p[0] - r <= size[0] && p[1] - r <= size[1]
    };
    let mut f = Frame {
        nodes: Vec::with_capacity(n),
        labels: Vec::new(),
        edges: Vec::with_capacity(g.edges.len()),
        rings: Vec::new(),
    };
    for &(a, b) in &g.edges {
        let (pa, pb) = (screen[a], screen[b]);
        // Left out when both ends are off on one side.
        let off = (pa[0] < 0.0 && pb[0] < 0.0)
            || (pa[1] < 0.0 && pb[1] < 0.0)
            || (pa[0] > size[0] && pb[0] > size[0])
            || (pa[1] > size[1] && pb[1] > size[1]);
        if off {
            continue;
        }
        let near = v.hovered.is_some_and(|h| h == a || h == b);
        let faint = v.hovered.is_some() && !near;
        f.edges.push((pa, pb, near, faint));
    }
    // The names, none over another: an 11-pixel font's letters about six
    // pixels wide.
    let named = v.labels(
        &screen,
        |i| on(screen[i], radius[i]),
        |i| g.nodes[i].label.chars().count() as f32 * 6.2,
        14.0,
    );
    for i in named {
        let (p, r) = (screen[i], radius[i]);
        let strong = Some(i) == v.hovered || Some(i) == v.selected;
        f.labels.push((
            [p[0], p[1] + r + 2.0],
            crate::one_line(&g.nodes[i].label),
            v.faint(i),
            strong,
        ));
    }
    for i in 0..n {
        let (p, r) = (screen[i], radius[i]);
        if !on(p, r) {
            continue;
        }
        let color = v.color(i, palette.len()).map_or(neutral, |c| palette[c]);
        f.nodes.push((p, r, color, v.faint(i)));
        if Some(i) == v.selected || Some(i) == g.focus {
            f.rings
                .push((p, r, Some(i) == g.focus && Some(i) != v.selected));
        }
    }
    f
}
