//! A spreadsheet's charts drawn over the grid: bars, lines, areas, points
//! and slices painted on a canvas, the title, category labels and legend
//! as text around it.

use gpui::{
    Bounds, Hsla, InteractiveElement, ParentElement, PathBuilder, Pixels, SharedString, Styled,
    div, point, px, size,
};
use kalem_viewer::{Chart, ChartKind, LegendPosition, Paint};

/// Excel's default series colors.
const PALETTE: [u32; 6] = [0x4472C4, 0xED7D31, 0xA5A5A5, 0xFFC000, 0x5B9BD5, 0x70AD47];

/// A point's own color, else its series'.
fn point_fill(s: &kalem_viewer::ChartSeries, i: usize, j: usize) -> Hsla {
    color(
        s.point_colors
            .iter()
            .find(|p| p.0 == i)
            .map(|p| p.1)
            .or(s.color),
        j,
    )
}

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
    font: SharedString,
) -> gpui::Div {
    let axes = !matches!(
        chart.kind,
        ChartKind::Pie | ChartKind::Doughnut | ChartKind::Other
    );
    let rgb = |[r, g, b]: [u8; 3]| -> Hsla {
        gpui::rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)).into()
    };
    let mut d = div()
        .debug_selector(move || format!("viewer-grid-chart-{index}"))
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(w))
        .h(px(h));
    // The chart area painted as the file says.
    d = match chart.background {
        Paint::Automatic => d.bg(background),
        Paint::None => d,
        Paint::Color(c) => d.bg(rgb(c)),
    };
    d = match chart.border {
        Paint::Automatic => d.border_1().border_color(border),
        Paint::None => d,
        Paint::Color(c) => d.border_1().border_color(rgb(c)),
    };
    let mut d = d
        .text_color(text)
        .p(px(6.))
        .flex()
        .flex_col()
        .overflow_hidden();
    if let Some(t) = &chart.title {
        let tf = &chart.title_font;
        let mut title = div()
            .debug_selector(move || format!("viewer-grid-chart-title-{index}"))
            .flex()
            .justify_center()
            .whitespace_nowrap()
            .overflow_hidden();
        // Bold as a chart's title is, unless its own font says otherwise.
        if tf.is_default() || tf.bold {
            title = title.font_weight(gpui::FontWeight::BOLD);
        }
        if tf.italic {
            title = title.italic();
        }
        if let Some(pt) = tf.size {
            title = title.text_size(px(pt * 4.0 / 3.0));
        }
        if let Some(c) = tf.color {
            title = title.text_color(rgb(c));
        }
        if let Some(face) = &tf.face {
            title = title.font_family(SharedString::from(face.clone()));
        }
        d = d.child(title.child(SharedString::from(t.clone())));
    }
    let plot = chart.clone();
    let mut canvas = div()
        .debug_selector(move || format!("viewer-grid-chart-plot-{index}"))
        .flex_1()
        .min_h(px(10.))
        .relative();
    // The plot area painted as the file says.
    if let Paint::Color(c) = chart.plot_background {
        canvas = canvas.bg(rgb(c));
    }
    if let Paint::Color(c) = chart.plot_border {
        canvas = canvas.border_1().border_color(rgb(c));
    }
    let canvas = canvas.child(
        gpui::canvas(
            |_, _, _| {},
            move |bounds, (), window, cx| paint(&plot, bounds, border, (&font, text), window, cx),
        )
        .absolute()
        .size_full(),
    );
    // The plot, its labels and its axes' titles, in a column.
    let mut middle = div()
        .flex_1()
        .min_w(px(10.))
        .min_h(px(10.))
        .flex()
        .flex_col();
    // The vertical axis's title beside the plot, a letter a line, as a
    // turned title reads.
    middle = match chart.vertical_title.as_ref().filter(|_| axes) {
        Some(t) => middle.child(
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
        None => middle.child(canvas),
    };
    if axes && chart.kind != ChartKind::Bar && chart.kind != ChartKind::Scatter {
        let n = chart
            .series
            .iter()
            .map(|s| s.values.len())
            .max()
            .unwrap_or(0);
        let hf = chart.horizontal_font.clone();
        let labels = (0..n).map(|i| {
            let mut l = div()
                .flex_1()
                .flex()
                .justify_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_xs();
            // The horizontal axis's font, as the file gives it.
            if let Some(pt) = hf.size {
                l = l.text_size(px(pt * 4.0 / 3.0));
            }
            if hf.bold {
                l = l.font_weight(gpui::FontWeight::BOLD);
            }
            if hf.italic {
                l = l.italic();
            }
            if let Some([r, g, b]) = hf.color {
                l = l.text_color(gpui::rgb(
                    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
                ));
            }
            if let Some(face) = &hf.face {
                l = l.font_family(SharedString::from(face.clone()));
            }
            l.child(SharedString::from(
                chart
                    .categories
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| (i + 1).to_string()),
            ))
        });
        middle = middle.child(div().flex().children(labels));
    }
    if let Some(t) = chart.horizontal_title.as_ref().filter(|_| axes) {
        middle = middle.child(
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
    // The legend: the slices of a pie, else the series.
    let entries: Vec<(String, Hsla)> = match chart.kind {
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
            .map(|(i, s)| (s.name.clone(), color(s.color, i)))
            .collect(),
    };
    let entry = |(name, c): (String, Hsla)| {
        div()
            .flex()
            .items_center()
            .gap(px(3.))
            .whitespace_nowrap()
            .child(div().size(px(8.)).flex_none().bg(c))
            .child(SharedString::from(name))
    };
    let lf = chart.legend_font.clone();
    let legend = |row: bool| {
        let mut l = div()
            .debug_selector(move || format!("viewer-grid-chart-legend-{index}"))
            .flex()
            .text_xs()
            .overflow_hidden()
            .children(entries.clone().into_iter().map(entry));
        // The legend's own font.
        if let Some(pt) = lf.size {
            l = l.text_size(px(pt * 4.0 / 3.0));
        }
        if lf.bold {
            l = l.font_weight(gpui::FontWeight::BOLD);
        }
        if lf.italic {
            l = l.italic();
        }
        if let Some(c) = lf.color {
            l = l.text_color(rgb(c));
        }
        if let Some(face) = &lf.face {
            l = l.font_family(SharedString::from(face.clone()));
        }
        if row {
            l.flex_wrap().justify_center().gap(px(8.))
        } else {
            l.flex_col().gap(px(2.)).max_w(px(w / 3.0))
        }
    };
    d = match chart.legend.filter(|_| !entries.is_empty()) {
        None => d.child(middle),
        Some(LegendPosition::Top) => d.child(legend(true)).child(middle),
        Some(LegendPosition::Bottom) => d.child(middle).child(legend(true)),
        Some(LegendPosition::Left) => d.child(
            div()
                .flex_1()
                .min_h(px(10.))
                .flex()
                .gap(px(6.))
                .child(legend(false).justify_center())
                .child(middle),
        ),
        Some(LegendPosition::Right) => d.child(
            div()
                .flex_1()
                .min_h(px(10.))
                .flex()
                .gap(px(6.))
                .child(middle)
                .child(legend(false).justify_center()),
        ),
        Some(LegendPosition::TopRight) => d.child(
            div()
                .flex_1()
                .min_h(px(10.))
                .flex()
                .gap(px(6.))
                .child(middle)
                .child(legend(false)),
        ),
    };
    d
}

/// Where a data label goes against its point.
#[derive(Clone, Copy)]
enum Place {
    Above,
    Right,
    Center,
}

/// A data label's text: the parts the chart asks for, as Excel joins them.
fn label_text(chart: &Chart, series: usize, i: usize, v: f64, total: f64) -> Option<String> {
    let l = chart.labels;
    let mut parts = Vec::new();
    if l.series {
        parts.push(chart.series.get(series)?.name.clone());
    }
    if l.category {
        parts.push(
            chart
                .categories
                .get(i)
                .cloned()
                .unwrap_or_else(|| (i + 1).to_string()),
        );
    }
    if l.value {
        parts.push(if v.fract() == 0.0 {
            format!("{v}")
        } else {
            format!("{v:.2}").trim_end_matches('0').to_owned()
        });
    }
    if l.percent && total > 0.0 {
        parts.push(format!("{:.0}%", v / total * 100.0));
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

fn paint(
    chart: &Chart,
    b: Bounds<Pixels>,
    grid: Hsla,
    (font, ink): (&SharedString, Hsla),
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) {
    // Data labels, and the value axis's labels (`true`).
    let mut marks: Vec<(f32, f32, String, Place, bool)> = Vec::new();
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
    // The value axis's scale: its own bounds, or a logarithmic one between
    // powers of ten.
    let sc = chart.scale;
    let log = sc.log;
    if log {
        let positive = chart
            .series
            .iter()
            .flat_map(|s| s.values.iter().flatten())
            .copied()
            .filter(|v| *v > 0.0);
        let (mut a, mut b) = (f64::MAX, f64::MIN);
        for v in positive {
            a = a.min(v);
            b = b.max(v);
        }
        if a > b {
            (a, b) = (1.0, 10.0);
        }
        lo = 10f64.powf(a.log10().floor());
        hi = 10f64.powf(b.log10().ceil()).max(lo * 10.0);
    }
    if let Some(m) = sc.min {
        lo = m;
    }
    if let Some(m) = sc.max {
        hi = m;
    }
    if hi <= lo {
        hi = lo + 1.0;
    }
    let t = |v: f64| {
        if log {
            v.max(f64::MIN_POSITIVE).log10()
        } else {
            v
        }
    };
    let frac = |v: f64| ((t(v) - t(lo)) / (t(hi) - t(lo))) as f32;
    // Where the gridlines go: every major unit, each power of ten, or
    // quarters.
    let ticks: Vec<f64> = if log {
        let mut v = 10f64.powf(lo.log10().ceil());
        let mut out = Vec::new();
        while v <= hi * 1.0001 && out.len() < 30 {
            out.push(v);
            v *= 10.0;
        }
        out
    } else if let Some(m) = sc.major.filter(|m| *m > 0.0 && (hi - lo) / m <= 50.0) {
        let mut v = (lo / m).ceil() * m;
        let mut out = Vec::new();
        while v <= hi + m * 1e-9 {
            out.push(v);
            v += m;
        }
        out
    } else {
        (0..=4)
            .map(|g| lo + (hi - lo) * f64::from(g) / 4.0)
            .collect()
    };
    // The axis's own number format, else a plain number.
    let tick_text = |v: f64| match &chart.axis_format {
        Some(code) => kalem_core::viewer::format_axis_number(v, code),
        None if v.abs() >= 1000.0 || v.fract() == 0.0 => format!("{v:.0}"),
        None => format!("{v:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned(),
    };
    match chart.kind {
        ChartKind::Column | ChartKind::Bar => {
            let horizontal = chart.kind == ChartKind::Bar;
            let (along, across) = if horizontal { (h, w) } else { (w, h) };
            let k = chart.series.len().max(1) as f32;
            let group = along / n.max(1) as f32;
            let bar = group * 0.7 / k;
            let scale = |v: f64| frac(v).clamp(0.0, 1.0) * across;
            let zero = scale(if log { lo } else { 0.0 });
            // The value axis's gridlines (across it) and the categories'
            // (between them), each as the chart asks.
            let gl = chart.gridlines;
            let (value_major, value_minor, cat_major, cat_minor) = if horizontal {
                (
                    gl.vertical_major,
                    gl.vertical_minor,
                    gl.horizontal_major,
                    gl.horizontal_minor,
                )
            } else {
                (
                    gl.horizontal_major,
                    gl.horizontal_minor,
                    gl.vertical_major,
                    gl.vertical_minor,
                )
            };
            let faint = grid.opacity(0.45);
            for (k, &v) in ticks.iter().enumerate() {
                let at = scale(v);
                if value_minor && let Some(&next) = ticks.get(k + 1) {
                    let mid = scale((v + next) / 2.0);
                    if horizontal {
                        rect(x0 + mid, y0, 0.5, h, faint, window);
                    } else {
                        rect(x0, y0 + h - mid, w, 0.5, faint, window);
                    }
                }
                if horizontal {
                    if value_major {
                        rect(x0 + at, y0, 0.5, h, grid, window);
                    }
                    marks.push((x0 + at, y0 + h, tick_text(v), Place::Above, true));
                } else {
                    if value_major {
                        rect(x0, y0 + h - at, w, 0.5, grid, window);
                    }
                    marks.push((x0 + 2.0, y0 + h - at, tick_text(v), Place::Right, true));
                }
            }
            for i in 1..n {
                let at = i as f32 * group;
                if cat_major {
                    if horizontal {
                        rect(x0, y0 + at, w, 0.5, grid, window);
                    } else {
                        rect(x0 + at, y0, 0.5, h, grid, window);
                    }
                }
                if cat_minor {
                    let mid = at - group / 2.0;
                    if horizontal {
                        rect(x0, y0 + mid, w, 0.5, faint, window);
                    } else {
                        rect(x0 + mid, y0, 0.5, h, faint, window);
                    }
                }
            }
            for i in 0..n {
                for (j, s) in chart.series.iter().enumerate() {
                    let Some(v) = s.values.get(i).copied().flatten() else {
                        continue;
                    };
                    let start = i as f32 * group + group * 0.15 + j as f32 * bar;
                    let (a, z) = (scale(v).min(zero), scale(v).max(zero));
                    let c = point_fill(s, i, j);
                    if horizontal {
                        rect(x0 + a, y0 + start, z - a, bar, c, window);
                    } else {
                        rect(x0 + start, y0 + h - z, bar, z - a, c, window);
                    }
                    if let Some(t) = label_text(chart, j, i, v, 0.0) {
                        if horizontal {
                            marks.push((
                                x0 + z + 3.0,
                                y0 + start + bar / 2.0,
                                t,
                                Place::Right,
                                false,
                            ));
                        } else {
                            marks.push((
                                x0 + start + bar / 2.0,
                                y0 + h - z - 2.0,
                                t,
                                Place::Above,
                                false,
                            ));
                        }
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
            let py = |v: f64| y0 + h - frac(v).clamp(-0.05, 1.05) * h;
            let gl = chart.gridlines;
            let faint = grid.opacity(0.45);
            for (k, &v) in ticks.iter().enumerate() {
                let y = y0 + h - frac(v) * h;
                if gl.horizontal_minor
                    && let Some(&next) = ticks.get(k + 1)
                {
                    rect(
                        x0,
                        y0 + h - frac((v + next) / 2.0) * h,
                        w,
                        0.5,
                        faint,
                        window,
                    );
                }
                if gl.horizontal_major {
                    rect(x0, y, w, 0.5, grid, window);
                }
                marks.push((x0 + 2.0, y, tick_text(v), Place::Right, true));
            }
            // Vertical lines: at each category, or quarters of the x range.
            let steps = if chart.kind == ChartKind::Scatter {
                4
            } else {
                n.max(2) - 1
            };
            for k in 0..=steps {
                let x = x0 + w * k as f32 / steps as f32;
                if gl.vertical_major {
                    rect(x, y0, 0.5, h, grid, window);
                }
                if gl.vertical_minor && k < steps {
                    rect(x + w / steps as f32 / 2.0, y0, 0.5, h, faint, window);
                }
            }
            for (j, s) in chart.series.iter().enumerate() {
                let c = color(s.color, j);
                let pts: Vec<(f32, f32)> = s
                    .values
                    .iter()
                    .enumerate()
                    .filter_map(|(i, v)| Some((px_of(i, s)?, py((*v)?))))
                    .collect();
                for (i, v) in s.values.iter().enumerate() {
                    if let (Some(v), Some(x)) = (v, px_of(i, s))
                        && let Some(t) = label_text(chart, j, i, *v, 0.0)
                    {
                        marks.push((x, py(*v) - 4.0, t, Place::Above, false));
                    }
                }
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
            // Slices pulled out push the pie in, so the farthest one fits.
            let pulled = |i: usize| -> f32 {
                let e = s
                    .point_explosions
                    .iter()
                    .find(|p| p.0 == i)
                    .map_or(s.explosion, |p| p.1);
                e as f32 / 100.0
            };
            let most = (0..s.values.len()).map(pulled).fold(0f32, f32::max);
            let r = (w.min(h) / 2.0 - 2.0) / (1.0 + most);
            let (cx, cy) = (x0 + w / 2.0, y0 + h / 2.0);
            let hole = if chart.kind == ChartKind::Doughnut {
                0.5
            } else {
                0.0
            };
            let mut angle = -std::f32::consts::FRAC_PI_2;
            for (i, v) in s.values.iter().enumerate() {
                let v = v.unwrap_or(0.0).max(0.0);
                if v == 0.0 {
                    continue;
                }
                let sweep = (v / total) as f32 * std::f32::consts::TAU;
                let mid = angle + sweep / 2.0;
                let off = r * pulled(i);
                let (sx, sy) = (cx + off * mid.cos(), cy + off * mid.sin());
                let steps = ((sweep / 0.05).ceil() as usize).max(2);
                let arc = |radius: f32| -> Vec<gpui::Point<Pixels>> {
                    (0..=steps)
                        .map(|k| {
                            let a = angle + sweep * k as f32 / steps as f32;
                            point(px(sx + radius * a.cos()), px(sy + radius * a.sin()))
                        })
                        .collect()
                };
                // A sector, or for a doughnut the ring's part.
                let mut poly = arc(r);
                if hole > 0.0 {
                    poly.extend(arc(r * hole).into_iter().rev());
                } else {
                    poly.insert(0, point(px(sx), px(sy)));
                }
                let mut p = PathBuilder::fill();
                p.add_polygon(&poly, true);
                if let Ok(path) = p.build() {
                    let own = s.point_colors.iter().find(|p| p.0 == i).map(|p| p.1);
                    window.paint_path(path, color(own, i));
                }
                if let Some(t) = label_text(chart, 0, i, v, total) {
                    let at = if hole > 0.0 { 0.75 } else { 0.62 };
                    marks.push((
                        sx + r * at * mid.cos(),
                        sy + r * at * mid.sin(),
                        t,
                        Place::Center,
                        false,
                    ));
                }
                angle += sweep;
            }
        }
        ChartKind::Other => {}
    }
    // The data labels over what was drawn.
    let fs = px(10.);
    // The value axis's font: the vertical axis's, a bar chart's horizontal.
    let axis = if chart.kind == ChartKind::Bar {
        &chart.horizontal_font
    } else {
        &chart.vertical_font
    };
    for (x, y, t, place, on_axis) in marks {
        let mut f = gpui::font(font.clone());
        let (mut size, mut color) = (fs, ink);
        if on_axis {
            if let Some(face) = &axis.face {
                f = gpui::font(SharedString::from(face.clone()));
            }
            if axis.bold {
                f.weight = gpui::FontWeight::BOLD;
            }
            if axis.italic {
                f.style = gpui::FontStyle::Italic;
            }
            if let Some(pt) = axis.size {
                size = px(pt * 4.0 / 3.0);
            }
            if let Some([r, g, b]) = axis.color {
                color = gpui::rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)).into();
            }
        }
        let lh = size * 1.2;
        let run = gpui::TextRun {
            len: t.len(),
            font: f,
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window
            .text_system()
            .shape_line(crate::one_line(&t), size, &[run], None);
        let tw = f32::from(shaped.width);
        let (ox, oy) = match place {
            Place::Above => (x - tw / 2.0, y - f32::from(lh)),
            Place::Right => (x, y - f32::from(lh) / 2.0),
            Place::Center => (x - tw / 2.0, y - f32::from(lh) / 2.0),
        };
        let _ = shaped.paint(
            point(px(ox), px(oy)),
            lh,
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        );
    }
}
