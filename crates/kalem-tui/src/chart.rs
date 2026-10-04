//! A spreadsheet's charts drawn in the terminal: bars as ratatui's bar
//! charts, lines and points in braille, slices as bars of their shares.

use kalem_viewer::{Chart, ChartKind, LegendPosition, Paint};
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
    let tint = |c: [u8; 3]| Color::Rgb(c[0], c[1], c[2]);
    let mut block = Block::default()
        .borders(if chart.border == Paint::None {
            Borders::NONE
        } else {
            Borders::ALL
        })
        .title(Line::from(short(
            &title,
            area.width.saturating_sub(4) as usize,
        )));
    // The axes' titles on the bottom edge: the horizontal one centered,
    // the vertical one at the left, pointing up.
    let room = area.width.saturating_sub(4) as usize;
    if let Some(t) = &chart.vertical_title {
        let arrow = if caps.ascii { "^ " } else { "↑ " };
        block =
            block.title_bottom(Line::from(short(&format!("{arrow}{t}"), room / 2)).left_aligned());
    }
    if let Some(t) = &chart.horizontal_title {
        block = block.title_bottom(Line::from(short(t, room / 2)).centered());
    }
    if !caps.no_color {
        if let Paint::Color(c) = chart.border {
            block = block.border_style(Style::default().fg(tint(c)));
        }
        if let Paint::Color(c) = chart.background {
            block = block.style(Style::default().bg(tint(c)));
        }
    }
    let inner = block.inner(area);
    block.render(area, buf);
    if inner.width < 2 || inner.height < 1 {
        return;
    }
    // The legend where the chart asks: a line above or below the plot, or
    // a column beside it.
    let entries: Vec<(String, Style)> = match chart.kind {
        ChartKind::Pie | ChartKind::Doughnut => {
            let n = chart.series.first().map_or(0, |s| s.values.len());
            (0..n)
                .map(|i| {
                    (
                        chart
                            .categories
                            .get(i)
                            .cloned()
                            .unwrap_or_else(|| (i + 1).to_string()),
                        color(
                            caps,
                            chart
                                .series
                                .first()
                                .and_then(|s| s.point_colors.iter().find(|p| p.0 == i))
                                .map(|p| p.1),
                            i,
                        ),
                    )
                })
                .collect()
        }
        _ => chart
            .series
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name.clone(), color(caps, s.color, i)))
            .collect(),
    };
    let mark = if caps.ascii { "#" } else { "■" };
    let line_legend = |buf: &mut Buffer, l: Rect| {
        let mut x = l.x;
        for (name, style) in &entries {
            let w = name.chars().count() as u16 + 3;
            if x + w > l.x + l.width {
                break;
            }
            buf.set_string(x, l.y, mark, *style);
            buf.set_string(x + 2, l.y, name, Style::default());
            x += w;
        }
    };
    let column_legend = |buf: &mut Buffer, l: Rect, top: bool| {
        let n = (entries.len() as u16).min(l.height);
        let y0 = if top { l.y } else { l.y + (l.height - n) / 2 };
        for (k, (name, style)) in entries.iter().take(n as usize).enumerate() {
            let y = y0 + k as u16;
            buf.set_string(l.x, y, mark, *style);
            buf.set_stringn(
                l.x + 2,
                y,
                name,
                l.width.saturating_sub(2) as usize,
                Style::default(),
            );
        }
    };
    let side = (entries
        .iter()
        .map(|e| e.0.chars().count())
        .max()
        .unwrap_or(0) as u16
        + 3)
    .min(inner.width / 3);
    let pos = chart.legend.filter(|_| !entries.is_empty());
    let plot = match pos {
        Some(LegendPosition::Top) if inner.height > 3 => {
            line_legend(buf, Rect::new(inner.x, inner.y, inner.width, 1));
            Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1)
        }
        Some(LegendPosition::Bottom) if inner.height > 3 => {
            line_legend(
                buf,
                Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
            );
            Rect::new(inner.x, inner.y, inner.width, inner.height - 1)
        }
        Some(LegendPosition::Left) if side > 2 => {
            column_legend(buf, Rect::new(inner.x, inner.y, side, inner.height), false);
            Rect::new(
                inner.x + side + 1,
                inner.y,
                inner.width - side - 1,
                inner.height,
            )
        }
        Some(p @ (LegendPosition::Right | LegendPosition::TopRight)) if side > 2 => {
            let x = inner.x + inner.width - side;
            column_legend(
                buf,
                Rect::new(x, inner.y, side, inner.height),
                p == LegendPosition::TopRight,
            );
            Rect::new(inner.x, inner.y, inner.width - side - 1, inner.height)
        }
        _ => inner,
    };
    if !caps.no_color
        && let Paint::Color(c) = chart.plot_background
    {
        buf.set_style(plot, Style::default().bg(tint(c)));
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
                            let own = s.point_colors.iter().find(|p| p.0 == i).map(|p| p.1);
                            Bar::default()
                                .value(v.max(0.0).round() as u64)
                                .text_value(if chart.labels.value {
                                    if v.fract() == 0.0 {
                                        format!("{v}")
                                    } else {
                                        format!("{v:.1}")
                                    }
                                } else {
                                    String::new()
                                })
                                .style(color(caps, own.or(s.color), j))
                        })
                        .collect();
                    BarGroup::default()
                        .label(Line::from(short(&label(i), (bar_width * k) as usize)))
                        .bars(&bars)
                })
                .collect();
            let top = chart
                .series
                .iter()
                .flat_map(|s| s.values.iter().flatten())
                .fold(0f64, |m, v| m.max(v.round()))
                .max(1.0);
            // The axis's own maximum, when it has one.
            let top = chart.scale.max.unwrap_or(top).max(1.0).round() as u64;
            let mut bc = BarChart::default()
                .bar_width(bar_width)
                .group_gap(1)
                .bar_gap(0)
                .max(top);
            if horizontal {
                bc = bc.direction(ratatui::layout::Direction::Horizontal);
            }
            for g in groups {
                bc = bc.data(g);
            }
            bc.render(plot, buf);
            // Values over their columns, where the bars are too narrow to
            // hold them.
            if chart.labels.value && !horizontal && plot.height > 2 {
                let rows = plot.height - 1;
                let group_w = bar_width * k + 1;
                for i in 0..n {
                    for (j, s) in chart.series.iter().enumerate() {
                        let Some(v) = s.values.get(i).copied().flatten() else {
                            continue;
                        };
                        let text = if v.fract() == 0.0 {
                            format!("{v}")
                        } else {
                            format!("{v:.1}")
                        };
                        let x = plot.x + i as u16 * group_w + j as u16 * bar_width;
                        let cells = ((v.max(0.0) / top as f64) * f64::from(rows)).ceil() as u16;
                        let y = (plot.y + rows).saturating_sub(cells + 1).max(plot.y);
                        let w = text.chars().count() as u16;
                        let x = (x + bar_width / 2).saturating_sub(w / 2).max(plot.x);
                        if x + w <= plot.x + plot.width {
                            buf.set_string(x, y, &text, Style::default());
                        }
                    }
                }
            }
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
            if let Some(m) = chart.scale.min {
                y0 = m;
            }
            if let Some(m) = chart.scale.max {
                y1 = m;
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
            let fmt = |v: f64| match &chart.axis_format {
                Some(code) => kalem_core::viewer::format_axis_number(v, code),
                None if v.abs() >= 1000.0 || v.fract() == 0.0 => format!("{v:.0}"),
                None => format!("{v:.1}"),
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
                // A slice pulled out, marked before its name.
                let out = s.point_explosions.iter().any(|p| p.0 == i && p.1 > 0) || s.explosion > 0;
                let mark = match (out, caps.ascii) {
                    (false, _) => "",
                    (true, true) => "> ",
                    (true, false) => "» ",
                };
                let name = short(&format!("{mark}{}", label(i)), 10);
                // The value beside the share when the labels ask for it.
                let text = if chart.labels.value && !chart.labels.percent {
                    format!("{name:<10} {v:>6} ")
                } else if chart.labels.value {
                    format!("{name:<10} {v} {:>3.0}% ", share * 100.0)
                } else {
                    format!("{name:<10} {:>3.0}% ", share * 100.0)
                };
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
                    color(
                        caps,
                        s.point_colors.iter().find(|p| p.0 == i).map(|p| p.1),
                        i,
                    ),
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
