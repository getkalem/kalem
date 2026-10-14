//! A plugin's document drawn as a canvas (`kalem_core::canvas_view`,
//! plugin API 0.2.16) in the terminal: Braille dots, two across and four
//! down a cell, for the shapes' outlines, ellipses, pen strokes and the
//! connectors with their arrows; the shapes' text in the cells inside
//! them, a frame's above it, a picture's name in place of the picture.
//! The window's pixels are the dots.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::widgets::Widget;
use ratatui::widgets::canvas::{Canvas, Line, Points};

use kalem_core::canvas_view::{self, CanvasView, ShapeKind, arrow_head, wrap};

pub use crate::graph_view::{DOTS, size};

fn color(c: Option<canvas_view::Color>, default: Color) -> Color {
    c.map_or(default, |c| {
        let (r, g, b) = c.rgb();
        Color::Rgb(r, g, b)
    })
}

/// Draws `v` into `area` of `buf`: lines in `line`, text in `text`, frames
/// in `muted`.
pub fn draw(v: &CanvasView, buf: &mut Buffer, area: Rect, line: Color, text: Color, muted: Color) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let size = size(area);
    let c = v.canvas();
    // The canvas counts up from the bottom; the view down from the top.
    let at = |p: [f32; 2]| {
        let s = v.to_screen(p, size);
        (f64::from(s[0]), f64::from(size[1] - s[1]))
    };
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, f64::from(size[0])])
        .y_bounds([0.0, f64::from(size[1])])
        .paint(|ctx| {
            let segment = |ctx: &mut ratatui::widgets::canvas::Context<'_>,
                           a: (f64, f64),
                           b: (f64, f64),
                           color: Color| {
                ctx.draw(&Line {
                    x1: a.0,
                    y1: a.1,
                    x2: b.0,
                    y2: b.1,
                    color,
                });
            };
            for (i, s) in c.shapes.iter().enumerate() {
                let [x, y, w, h] = s.rect;
                let chosen = v.selected == Some(i);
                let stroke = if chosen { text } else { color(s.stroke, line) };
                let corners = [
                    at([x, y]),
                    at([x + w, y]),
                    at([x + w, y + h]),
                    at([x, y + h]),
                ];
                let outline = |ctx: &mut ratatui::widgets::canvas::Context<'_>, col: Color| {
                    for k in 0..4 {
                        segment(ctx, corners[k], corners[(k + 1) % 4], col);
                    }
                };
                match &s.kind {
                    ShapeKind::Box(_) | ShapeKind::Image(_) => outline(ctx, stroke),
                    ShapeKind::Group => {
                        outline(ctx, if chosen { text } else { color(s.stroke, muted) })
                    }
                    ShapeKind::Text if chosen => outline(ctx, stroke),
                    ShapeKind::Text => {}
                    ShapeKind::Ellipse => {
                        let (cx, cy) = at([x + w / 2.0, y + h / 2.0]);
                        let (rx, ry) = (f64::from(w * v.zoom / 2.0), f64::from(h * v.zoom / 2.0));
                        let steps = ((rx + ry) * 3.0).clamp(12.0, 2000.0) as usize;
                        let dots: Vec<(f64, f64)> = (0..steps)
                            .map(|k| {
                                let a = k as f64 / steps as f64 * std::f64::consts::TAU;
                                (cx + rx * a.cos(), cy + ry * a.sin())
                            })
                            .collect();
                        ctx.draw(&Points {
                            coords: &dots,
                            color: stroke,
                        });
                    }
                    ShapeKind::Stroke(points) => {
                        let stroke = if chosen { text } else { color(s.stroke, text) };
                        for pair in points.windows(2) {
                            segment(
                                ctx,
                                at([x + pair[0][0], y + pair[0][1]]),
                                at([x + pair[1][0], y + pair[1][1]]),
                                stroke,
                            );
                        }
                    }
                }
            }
            for k in &c.connectors {
                let (a, b) = c.ends(k);
                let col = color(k.stroke, line);
                segment(ctx, at(a), at(b), col);
                let len = 3.0 / v.zoom.max(0.01);
                for (tip, from, on) in [(b, a, k.arrow_end), (a, b, k.arrow_start)] {
                    if on {
                        for wing in arrow_head(tip, from, len.max(1.0)) {
                            segment(ctx, at(wing), at(tip), col);
                        }
                    }
                }
            }
            ctx.layer();
            // The text, in the cells inside each shape: a line of cells is
            // DOTS.1 dots high, a cell DOTS.0 wide.
            let (cw, ch) = (f64::from(DOTS.0), f64::from(DOTS.1));
            for (i, s) in c.shapes.iter().enumerate() {
                let [x, y, w, h] = s.rect;
                let words = match &s.kind {
                    ShapeKind::Image(p) => format!(
                        "▣ {}",
                        p.file_name()
                            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                    ),
                    ShapeKind::Stroke(_) => continue,
                    _ => s.text.clone(),
                };
                if words.trim().is_empty() {
                    continue;
                }
                let (left, top) = at([x, y]);
                let (wd, hd) = (f64::from(w * v.zoom), f64::from(h * v.zoom));
                let cols = ((wd / cw).floor() as usize).saturating_sub(1).max(1);
                let rows = (hd / ch).floor() as usize;
                let lines = wrap(&words, cols);
                let chosen = v.selected == Some(i);
                let style = Style::default()
                    .fg(if s.kind == ShapeKind::Group {
                        muted
                    } else {
                        text
                    })
                    .add_modifier(if chosen {
                        Modifier::REVERSED
                    } else {
                        Modifier::empty()
                    });
                let (first, room) = match s.kind {
                    // A frame's name on the line above it.
                    ShapeKind::Group => (top + ch, 1),
                    ShapeKind::Text => (top, rows.max(1)),
                    _ => {
                        let n = lines.len().min(rows.max(1));
                        (
                            top - ((rows.saturating_sub(n)) / 2) as f64 * ch,
                            rows.max(1),
                        )
                    }
                };
                for (k, l) in lines.into_iter().take(room).enumerate() {
                    let row_y = first - k as f64 * ch - ch / 2.0;
                    let w = l.chars().count() as f64 * cw;
                    let col_x = match s.kind {
                        ShapeKind::Group | ShapeKind::Text => left + cw / 2.0,
                        _ => left + ((wd - w) / 2.0).max(cw / 2.0),
                    };
                    ctx.print(col_x, row_y, ratatui::text::Line::styled(l, style));
                }
            }
            for k in &c.connectors {
                if k.label.trim().is_empty() {
                    continue;
                }
                let (a, b) = c.ends(k);
                let (pa, pb) = (at(a), at(b));
                let w = k.label.chars().count() as f64 * cw;
                ctx.print(
                    (pa.0 + pb.0) / 2.0 - w / 2.0,
                    (pa.1 + pb.1) / 2.0,
                    ratatui::text::Line::styled(k.label.clone(), Style::default().fg(text)),
                );
            }
        });
    canvas.render(area, buf);
}
