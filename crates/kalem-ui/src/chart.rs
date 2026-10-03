//! A spreadsheet's charts drawn over the grid: bars, lines, areas, points
//! and slices painted on a canvas, the title, category labels and legend
//! as text around it.

use gpui::{
    Bounds, Hsla, InteractiveElement, ParentElement, PathBuilder, Pixels, SharedString, Styled,
    div, point, px, size,
};
use kalem_viewer::{Chart, ChartKind};

/// Excel's default series colors.
const PALETTE: [u32; 6] = [0x4472C4, 0xED7D31, 0xA5A5A5, 0xFFC000, 0x5B9BD5, 0x70AD47];

fn color(c: Option<[u8; 3]>, i: usize) -> Hsla {
    let v = c.map_or(PALETTE[i % PALETTE.len()], |[r, g, b]| {
        (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
    });
    gpui::rgb(v).into()
}

/// The chart's box, `w` × `h` pixels at `(x, y)` of the grid.
pub fn chart_view(
    chart: &Chart,
    index: usize,
    (x, y, w, h): (f32, f32, f32, f32),
    background: Hsla,
    border: Hsla,
    text: Hsla,
) -> gpui::Div {
    let axes = !matches!(
        chart.kind,
        ChartKind::Pie | ChartKind::Doughnut | ChartKind::Other
    );
    let mut d = div()
        .debug_selector(move || format!("viewer-grid-chart-{index}"))
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(w))
        .h(px(h))
        .bg(background)
        .border_1()
        .border_color(border)
        .text_color(text)
        .p(px(6.))
        .flex()
        .flex_col()
        .overflow_hidden();
    if let Some(t) = &chart.title {
        d = d.child(
            div()
                .flex()
                .justify_center()
                .font_weight(gpui::FontWeight::BOLD)
                .whitespace_nowrap()
                .overflow_hidden()
                .child(SharedString::from(t.clone())),
        );
    }
    let plot = chart.clone();
    let canvas = div().flex_1().min_h(px(10.)).relative().child(
        gpui::canvas(
            |_, _, _| {},
            move |bounds, (), window, _| paint(&plot, bounds, border, window),
        )
        .absolute()
        .size_full(),
    );
    // The vertical axis's title beside the plot, a letter a line, as a
    // turned title reads.
    d = match chart.vertical_title.as_ref().filter(|_| axes) {
        Some(t) => d.child(
            div()
                .flex_1()
                .min_h(px(10.))
                .flex()
                .child(
                    div()
                        .debug_selector(move || format!("viewer-grid-chart-vtitle-{index}"))
                        .flex()
                        .flex_col()
                        .justify_center()
                        .items_center()
                        .pr(px(4.))
                        .text_xs()
                        .overflow_hidden()
                        .children(
                            t.chars()
                                .map(|c| div().child(SharedString::from(c.to_string()))),
                        ),
                )
                .child(canvas),
        ),
        None => d.child(canvas),
    };
    if axes && chart.kind != ChartKind::Bar && chart.kind != ChartKind::Scatter {
        let n = chart
            .series
            .iter()
            .map(|s| s.values.len())
            .max()
            .unwrap_or(0);
        let labels = (0..n).map(|i| {
            div()
                .flex_1()
                .flex()
                .justify_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_xs()
                .child(SharedString::from(
                    chart
                        .categories
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| (i + 1).to_string()),
                ))
        });
        d = d.child(div().flex().children(labels));
    }
    if let Some(t) = chart.horizontal_title.as_ref().filter(|_| axes) {
        d = d.child(
            div()
                .debug_selector(move || format!("viewer-grid-chart-htitle-{index}"))
                .flex()
                .justify_center()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .whitespace_nowrap()
                .overflow_hidden()
                .child(SharedString::from(t.clone())),
        );
    }
    let legend: Vec<(String, Hsla)> = match chart.kind {
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
                        color(None, i),
                    )
                })
                .collect()
        }
        _ if chart.series.len() > 1 => chart
            .series
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name.clone(), color(s.color, i)))
            .collect(),
        _ => Vec::new(),
    };
    if !legend.is_empty() {
        d = d.child(
            div()
                .flex()
                .flex_wrap()
                .justify_center()
                .gap(px(8.))
                .text_xs()
                .children(legend.into_iter().map(|(name, c)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(3.))
                        .child(div().size(px(8.)).bg(c))
                        .child(SharedString::from(name))
                })),
        );
    }
    d
}

fn paint(chart: &Chart, b: Bounds<Pixels>, grid: Hsla, window: &mut gpui::Window) {
    let (x0, y0) = (f32::from(b.origin.x), f32::from(b.origin.y));
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    if w < 4.0 || h < 4.0 {
        return;
    }
    let rect = |x: f32, y: f32, rw: f32, rh: f32, c: Hsla, window: &mut gpui::Window| {
        window.paint_quad(gpui::fill(
            Bounds::new(point(px(x), px(y)), size(px(rw.max(0.5)), px(rh.max(0.5)))),
            c,
        ));
    };
    let n = chart
        .series
        .iter()
        .map(|s| s.values.len())
        .max()
        .unwrap_or(0);
    let values = chart.series.iter().flat_map(|s| s.values.iter().flatten());
    let (mut lo, mut hi) = (0f64, f64::MIN);
    for v in values {
        lo = lo.min(*v);
        hi = hi.max(*v);
    }
    if hi <= lo {
        hi = lo + 1.0;
    }
    match chart.kind {
        ChartKind::Column | ChartKind::Bar => {
            let horizontal = chart.kind == ChartKind::Bar;
            let (along, across) = if horizontal { (h, w) } else { (w, h) };
            let k = chart.series.len().max(1) as f32;
            let group = along / n.max(1) as f32;
            let bar = group * 0.7 / k;
            let scale = |v: f64| ((v - lo) / (hi - lo)) as f32 * across;
            let zero = scale(0.0);
            for g in 1..4 {
                let t = across * g as f32 / 4.0;
                if horizontal {
                    rect(x0 + t, y0, 0.5, h, grid, window);
                } else {
                    rect(x0, y0 + h - t, w, 0.5, grid, window);
                }
            }
            for i in 0..n {
                for (j, s) in chart.series.iter().enumerate() {
                    let Some(v) = s.values.get(i).copied().flatten() else {
                        continue;
                    };
                    let start = i as f32 * group + group * 0.15 + j as f32 * bar;
                    let (a, z) = (scale(v).min(zero), scale(v).max(zero));
                    let c = color(s.color, j);
                    if horizontal {
                        rect(x0 + a, y0 + start, z - a, bar, c, window);
                    } else {
                        rect(x0 + start, y0 + h - z, bar, z - a, c, window);
                    }
                }
            }
        }
        ChartKind::Line | ChartKind::Area | ChartKind::Scatter => {
            let xs: Vec<f64> = chart
                .series
                .iter()
                .flat_map(|s| s.x.iter().flatten().copied())
                .collect();
            let (xl, xh) = if chart.kind == ChartKind::Scatter && !xs.is_empty() {
                let l = xs.iter().copied().fold(f64::MAX, f64::min);
                let hgh = xs.iter().copied().fold(f64::MIN, f64::max);
                (l, if hgh > l { hgh } else { l + 1.0 })
            } else {
                (0.0, (n.max(2) - 1) as f64)
            };
            let px_of = |i: usize, s: &kalem_viewer::ChartSeries| -> Option<f32> {
                let xv = if chart.kind == ChartKind::Scatter {
                    s.x.get(i).copied().flatten()?
                } else {
                    i as f64
                };
                Some(x0 + ((xv - xl) / (xh - xl)) as f32 * w)
            };
            let py = |v: f64| y0 + h - ((v - lo) / (hi - lo)) as f32 * h;
            for g in 1..4 {
                rect(x0, y0 + h * g as f32 / 4.0, w, 0.5, grid, window);
            }
            for (j, s) in chart.series.iter().enumerate() {
                let c = color(s.color, j);
                let pts: Vec<(f32, f32)> = s
                    .values
                    .iter()
                    .enumerate()
                    .filter_map(|(i, v)| Some((px_of(i, s)?, py((*v)?))))
                    .collect();
                match chart.kind {
                    ChartKind::Scatter => {
                        for (x, y) in &pts {
                            rect(x - 2.5, y - 2.5, 5.0, 5.0, c, window);
                        }
                    }
                    _ if pts.len() >= 2 => {
                        if chart.kind == ChartKind::Area {
                            let mut p = PathBuilder::fill();
                            let base = py(lo.max(0.0).min(hi));
                            let mut poly = vec![point(px(pts[0].0), px(base))];
                            poly.extend(pts.iter().map(|(x, y)| point(px(*x), px(*y))));
                            poly.push(point(px(pts[pts.len() - 1].0), px(base)));
                            p.add_polygon(&poly, true);
                            if let Ok(path) = p.build() {
                                window.paint_path(path, c.opacity(0.6));
                            }
                        }
                        let mut p = PathBuilder::stroke(px(2.));
                        p.move_to(point(px(pts[0].0), px(pts[0].1)));
                        for (x, y) in &pts[1..] {
                            p.line_to(point(px(*x), px(*y)));
                        }
                        if let Ok(path) = p.build() {
                            window.paint_path(path, c);
                        }
                    }
                    _ => {}
                }
            }
        }
        ChartKind::Pie | ChartKind::Doughnut => {
            let Some(s) = chart.series.first() else {
                return;
            };
            let total: f64 = s.values.iter().flatten().filter(|v| **v > 0.0).sum();
            if total <= 0.0 {
                return;
            }
            let r = w.min(h) / 2.0 - 2.0;
            let (cx, cy) = (x0 + w / 2.0, y0 + h / 2.0);
            let mut angle = -std::f32::consts::FRAC_PI_2;
            for (i, v) in s.values.iter().enumerate() {
                let v = v.unwrap_or(0.0).max(0.0);
                if v == 0.0 {
                    continue;
                }
                let sweep = (v / total) as f32 * std::f32::consts::TAU;
                let steps = ((sweep / 0.05).ceil() as usize).max(2);
                let mut poly = vec![point(px(cx), px(cy))];
                for k in 0..=steps {
                    let a = angle + sweep * k as f32 / steps as f32;
                    poly.push(point(px(cx + r * a.cos()), px(cy + r * a.sin())));
                }
                let mut p = PathBuilder::fill();
                p.add_polygon(&poly, true);
                if let Ok(path) = p.build() {
                    window.paint_path(path, color(None, i));
                }
                angle += sweep;
            }
            if chart.kind == ChartKind::Doughnut {
                let mut poly = Vec::new();
                for k in 0..72 {
                    let a = k as f32 * std::f32::consts::TAU / 72.0;
                    poly.push(point(
                        px(cx + r * 0.5 * a.cos()),
                        px(cy + r * 0.5 * a.sin()),
                    ));
                }
                let mut p = PathBuilder::fill();
                p.add_polygon(&poly, true);
                if let Ok(path) = p.build() {
                    window.paint_path(path, gpui::white());
                }
            }
        }
        ChartKind::Other => {}
    }
}
