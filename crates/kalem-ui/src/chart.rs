//! A spreadsheet's charts drawn over the grid: bars, lines, areas, points
//! and slices painted on a canvas, the title, category labels and legend
//! as text around it.

use gpui::{
    Bounds, Hsla, InteractiveElement, ParentElement, PathBuilder, Pixels, SharedString, Styled,
    div, point, px, size,
};
use kalem_viewer::{Chart, ChartKind, LegendPosition};

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
    font: SharedString,
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
                        color(None, i),
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
    let legend = |row: bool| {
        let l = div()
            .debug_selector(move || format!("viewer-grid-chart-legend-{index}"))
            .flex()
            .text_xs()
            .overflow_hidden()
            .children(entries.clone().into_iter().map(entry));
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
    let mut marks: Vec<(f32, f32, String, Place)> = Vec::new();
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
    let tick_text = |v: f64| {
        if v.abs() >= 1000.0 || v.fract() == 0.0 {
            format!("{v:.0}")
        } else {
            format!("{v:.2}")
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_owned()
        }
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
            for &v in &ticks {
                let at = scale(v);
                if horizontal {
                    rect(x0 + at, y0, 0.5, h, grid, window);
                    marks.push((x0 + at, y0 + h, tick_text(v), Place::Above));
                } else {
                    rect(x0, y0 + h - at, w, 0.5, grid, window);
                    marks.push((x0 + 2.0, y0 + h - at, tick_text(v), Place::Right));
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
                    if let Some(t) = label_text(chart, j, i, v, 0.0) {
                        if horizontal {
                            marks.push((x0 + z + 3.0, y0 + start + bar / 2.0, t, Place::Right));
                        } else {
                            marks.push((x0 + start + bar / 2.0, y0 + h - z - 2.0, t, Place::Above));
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
            for &v in &ticks {
                let y = y0 + h - frac(v) * h;
                rect(x0, y, w, 0.5, grid, window);
                marks.push((x0 + 2.0, y, tick_text(v), Place::Right));
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
                        marks.push((x, py(*v) - 4.0, t, Place::Above));
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
                if let Some(t) = label_text(chart, 0, i, v, total) {
                    let mid = angle + sweep / 2.0;
                    let at = if chart.kind == ChartKind::Doughnut {
                        0.75
                    } else {
                        0.62
                    };
                    marks.push((
                        cx + r * at * mid.cos(),
                        cy + r * at * mid.sin(),
                        t,
                        Place::Center,
                    ));
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
    // The data labels over what was drawn.
    let fs = px(10.);
    let lh = fs * 1.2;
    for (x, y, t, place) in marks {
        let run = gpui::TextRun {
            len: t.len(),
            font: gpui::font(font.clone()),
            color: ink,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window.text_system().shape_line(t.into(), fs, &[run], None);
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
