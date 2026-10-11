//! A plugin's document drawn as a graph (`kalem_core::graph_view`, plugin
//! API 0.2.12) in the terminal: Braille dots, two across and four down a
//! cell, the edges as lines and the nodes as small discs colored by their
//! group, the labels of the heavy nodes and of the one chosen written
//! beside them. The layout's pixels are the dots.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::widgets::Widget;
use ratatui::widgets::canvas::{Canvas, Line, Points};

use kalem_core::graph_view::GraphView;

/// The groups' colors in the 256 of terminals, the most common group's
/// first, as near as they come to the window's.
const PALETTE: [u8; 8] = [67, 208, 71, 167, 109, 221, 139, 137];

/// The dots a cell holds across and down.
pub const DOTS: (f32, f32) = (2.0, 4.0);

/// The drawing's size in dots for `area`.
pub fn size(area: Rect) -> [f32; 2] {
    [
        f32::from(area.width) * DOTS.0,
        f32::from(area.height) * DOTS.1,
    ]
}

/// Draws `v` into `area` of `buf`, its edges in `edge`, its labels in
/// `text`.
pub fn draw(v: &GraphView, buf: &mut Buffer, area: Rect, edge: Color, text: Color) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let size = size(area);
    let g = v.graph();
    let n = g.nodes.len();
    // The canvas counts up from the bottom; the layout down from the top.
    let at = |i: usize| {
        let s = v.to_screen(v.place(i), size);
        (f64::from(s[0]), f64::from(size[1] - s[1]))
    };
    let places: Vec<(f64, f64)> = (0..n).map(at).collect();
    let near = |a: usize, b: usize| v.hovered.or(v.selected).is_some_and(|h| h == a || h == b);
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, f64::from(size[0])])
        .y_bounds([0.0, f64::from(size[1])])
        .paint(|ctx| {
            for &(a, b) in &g.edges {
                let ((x1, y1), (x2, y2)) = (places[a], places[b]);
                ctx.draw(&Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    color: if near(a, b) { text } else { edge },
                });
            }
            ctx.layer();
            for (i, &(x, y)) in places.iter().enumerate() {
                let r = f64::from((v.radius(i) * v.zoom).clamp(1.0, 6.0));
                let color = Color::Indexed(PALETTE[v.color(i, PALETTE.len())]);
                // A disc of dots.
                let mut dots = Vec::new();
                let steps = r.ceil() as i32;
                for dy in -steps..=steps {
                    for dx in -steps..=steps {
                        let (fx, fy) = (f64::from(dx), f64::from(dy));
                        if fx * fx + fy * fy <= r * r + 0.5 {
                            dots.push((x + fx, y + fy));
                        }
                    }
                }
                ctx.draw(&Points {
                    coords: &dots,
                    color,
                });
            }
            ctx.layer();
            let heavy = heavy(v);
            for (i, &(x, y)) in places.iter().enumerate() {
                let chosen = Some(i) == v.selected || Some(i) == v.hovered;
                // Heavy nodes, and the one chosen, named under them.
                if !(chosen || g.nodes[i].weight >= heavy) {
                    continue;
                }
                let label = &g.nodes[i].label;
                let w = label.chars().count() as f64 * f64::from(DOTS.0);
                let style = if chosen {
                    Style::default().fg(text).add_modifier(Modifier::REVERSED)
                } else {
                    Style::default().fg(text)
                };
                ctx.print(
                    (x - w / 2.0).max(0.0),
                    y - f64::from(DOTS.1) * 1.2,
                    ratatui::text::Line::styled(label.clone(), style),
                );
            }
        });
    canvas.render(area, buf);
}

/// The weight above which a node's label is written in the terminal,
/// where labels take whole cells: the heaviest eight.
fn heavy(v: &GraphView) -> u32 {
    let mut w: Vec<u32> = v.graph().nodes.iter().map(|n| n.weight).collect();
    w.sort_unstable_by(|a, b| b.cmp(a));
    w.get(7).copied().unwrap_or(1).max(2)
}
