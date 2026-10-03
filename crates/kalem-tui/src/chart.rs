//! A spreadsheet's charts drawn in the terminal: bars as ratatui's bar
//! charts, lines and points in braille, slices as bars of their shares.

use kalem_viewer::{Chart, ChartKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::Line;
use ratatui::widgets::{
    Axis, Bar, BarChart, BarGroup, Block, Borders, Clear, Dataset, GraphType, Paragraph, Widget,
};

use crate::caps::Caps;

/// Excel's default series colors.
const PALETTE: [[u8; 3]; 6] = [
    [0x44, 0x72, 0xC4],
    [0xED, 0x7D, 0x31],
    [0xA5, 0xA5, 0xA5],
    [0xFF, 0xC0, 0x00],
    [0x5B, 0x9B, 0xD5],
    [0x70, 0xAD, 0x47],
];

fn color(caps: &Caps, c: Option<[u8; 3]>, i: usize) -> Style {
    if caps.no_color {
        return Style::default();
    }
    let [r, g, b] = c.unwrap_or(PALETTE[i % PALETTE.len()]);
    Style::default().fg(Color::Rgb(r, g, b))
}

fn short(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Draws a chart in `area`, over what is there.
pub fn draw(chart: &Chart, caps: &Caps, area: Rect, buf: &mut Buffer) {
    Clear.render(area, buf);
    let title = chart.title.clone().unwrap_or_default();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Line::from(short(
            &title,
            area.width.saturating_sub(4) as usize,
        )));
    let inner = block.inner(area);
    block.render(area, buf);
    if inner.width < 2 || inner.height < 1 {
        return;
    }
    // A legend line at the bottom when there are several series.
    let (plot, legend) = if chart.series.len() > 1 && inner.height > 3 {
        (
            Rect::new(inner.x, inner.y, inner.width, inner.height - 1),
            Some(Rect::new(
                inner.x,
                inner.y + inner.height - 1,
                inner.width,
                1,
            )),
        )
    } else {
        (inner, None)
    };
    if let Some(l) = legend {
        let mut x = l.x;
        for (i, s) in chart.series.iter().enumerate() {
            let mark = if caps.ascii { "#" } else { "■" };
            let text = format!("{mark} {} ", s.name);
            let w = text.chars().count() as u16;
            if x + w > l.x + l.width {
                break;
            }
            buf.set_string(x, l.y, mark, color(caps, s.color, i));
            buf.set_string(x + 2, l.y, &text[mark.len() + 1..], Style::default());
            x += w;
        }
    }
    let label = |i: usize| {
        chart
            .categories
            .get(i)
            .cloned()
            .unwrap_or_else(|| (i + 1).to_string())
    };
    match chart.kind {
        ChartKind::Column | ChartKind::Bar => {
            let n = chart
                .series
                .iter()
                .map(|s| s.values.len())
                .max()
                .unwrap_or(0);
            let horizontal = chart.kind == ChartKind::Bar;
            let k = chart.series.len().max(1) as u16;
            let slots = if horizontal { plot.height } else { plot.width };
            let bar_width = (slots / (n as u16 * (k + 1)).max(1)).max(1);
            let groups: Vec<BarGroup<'_>> = (0..n)
                .map(|i| {
                    let bars: Vec<Bar<'_>> = chart
                        .series
                        .iter()
                        .enumerate()
                        .map(|(j, s)| {
                            let v = s.values.get(i).copied().flatten().unwrap_or(0.0);
                            Bar::default()
                                .value(v.max(0.0).round() as u64)
                                .text_value(String::new())
                                .style(color(caps, s.color, j))
                        })
                        .collect();
                    BarGroup::default()
                        .label(Line::from(short(&label(i), (bar_width * k) as usize)))
                        .bars(&bars)
                })
                .collect();
            let mut bc = BarChart::default()
                .bar_width(bar_width)
                .group_gap(1)
                .bar_gap(0);
            if horizontal {
                bc = bc.direction(ratatui::layout::Direction::Horizontal);
            }
            for g in groups {
                bc = bc.data(g);
            }
            bc.render(plot, buf);
        }
        ChartKind::Line | ChartKind::Area | ChartKind::Scatter => {
            if caps.ascii {
                return summary(chart, plot, buf);
            }
            let points: Vec<Vec<(f64, f64)>> = chart
                .series
                .iter()
                .map(|s| {
                    s.values
                        .iter()
                        .enumerate()
                        .filter_map(|(i, v)| {
                            let x = if chart.kind == ChartKind::Scatter {
                                s.x.get(i).copied().flatten()?
                            } else {
                                i as f64
                            };
                            Some((x, (*v)?))
                        })
                        .collect()
                })
                .collect();
            let all = points.iter().flatten();
            let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, 0f64, f64::MIN);
            for (x, y) in all {
                x0 = x0.min(*x);
                x1 = x1.max(*x);
                y0 = y0.min(*y);
                y1 = y1.max(*y);
            }
            if x0 > x1 {
                return;
            }
            if y1 <= y0 {
                y1 = y0 + 1.0;
            }
            let datasets: Vec<Dataset<'_>> = points
                .iter()
                .zip(&chart.series)
                .enumerate()
                .map(|(i, (p, s))| {
                    Dataset::default()
                        .marker(Marker::Braille)
                        .graph_type(if chart.kind == ChartKind::Scatter {
                            GraphType::Scatter
                        } else {
                            GraphType::Line
                        })
                        .style(color(caps, s.color, i))
                        .data(p)
                })
                .collect();
            let fmt = |v: f64| {
                if v.abs() >= 1000.0 || v.fract() == 0.0 {
                    format!("{v:.0}")
                } else {
                    format!("{v:.1}")
                }
            };
            let x_labels: Vec<Line<'_>> = if chart.kind == ChartKind::Scatter {
                vec![Line::from(fmt(x0)), Line::from(fmt(x1))]
            } else {
                vec![
                    Line::from(short(&label(0), 8)),
                    Line::from(short(&label(x1 as usize), 8)),
                ]
            };
            ratatui::widgets::Chart::new(datasets)
                .x_axis(
                    Axis::default()
                        .bounds([x0, x1.max(x0 + 1.0)])
                        .labels(x_labels),
                )
                .y_axis(
                    Axis::default()
                        .bounds([y0, y1])
                        .labels(vec![Line::from(fmt(y0)), Line::from(fmt(y1))]),
                )
                .render(plot, buf);
        }
        ChartKind::Pie | ChartKind::Doughnut => {
            // Each slice as a bar of its share.
            let Some(s) = chart.series.first() else {
                return;
            };
            let total: f64 = s.values.iter().flatten().filter(|v| **v > 0.0).sum();
            if total <= 0.0 {
                return;
            }
            let width = plot.width as usize;
            for (i, v) in s.values.iter().enumerate().take(plot.height as usize) {
                let v = v.unwrap_or(0.0).max(0.0);
                let share = v / total;
                let name = short(&label(i), 10);
                let text = format!("{name:<10} {:>3.0}% ", share * 100.0);
                let room = width.saturating_sub(text.chars().count());
                let n = (share * room as f64).round() as usize;
                let y = plot.y + i as u16;
                buf.set_stringn(plot.x, y, &text, width, Style::default());
                let mark = if caps.ascii { "#" } else { "█" };
                buf.set_stringn(
                    plot.x + text.chars().count() as u16,
                    y,
                    mark.repeat(n),
                    room,
                    color(caps, None, i),
                );
            }
        }
        ChartKind::Other => summary(chart, plot, buf),
    }
}

/// A chart the terminal does not draw: what it holds, in words.
fn summary(chart: &Chart, area: Rect, buf: &mut Buffer) {
    let lines: Vec<Line<'_>> = chart
        .series
        .iter()
        .map(|s| {
            let vals: Vec<String> = s
                .values
                .iter()
                .map(|v| v.map_or("-".into(), |v| format!("{v}")))
                .collect();
            Line::from(format!("{}: {}", s.name, vals.join(" ")))
        })
        .collect();
    Paragraph::new(lines).render(area, buf);
}
