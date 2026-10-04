//! A spreadsheet's chart as a TikZ picture, to print it with its sheet:
//! columns, bars, lines, areas (clustered or stacked), pies, doughnuts
//! and scatter charts with their title, axes, gridlines, axis titles,
//! legend and value labels, in the colors the file gives or Office's.

use kalem_viewer::{Chart, ChartKind, LegendPosition, Paint};

const PALETTE: [[u8; 3]; 6] = [
    [0x44, 0x72, 0xC4],
    [0xED, 0x7D, 0x31],
    [0xA5, 0xA5, 0xA5],
    [0xFF, 0xC0, 0x00],
    [0x5B, 0x9B, 0xD5],
    [0x70, 0xAD, 0x47],
];

/// A TikZ color option of `c` (`fill={rgb,255:red,…}`).
fn rgb(c: [u8; 3]) -> String {
    format!("{{rgb,255:red,{};green,{};blue,{}}}", c[0], c[1], c[2])
}

/// A TikZ option drawing or filling in `c`.
fn color_of(c: [u8; 3]) -> String {
    format!("color={}", rgb(c))
}

fn series_color(chart: &Chart, i: usize) -> [u8; 3] {
    chart.series[i].color.unwrap_or(PALETTE[i % PALETTE.len()])
}

fn point_color(chart: &Chart, s: usize, j: usize) -> [u8; 3] {
    let series = &chart.series[s];
    series
        .point_colors
        .iter()
        .find(|(k, _)| *k == j)
        .map_or(PALETTE[j % PALETTE.len()], |(_, c)| *c)
}

/// A step between gridlines for values from `lo` to `hi`: 1, 2 or 5 times
/// a power of ten, about five of them.
pub fn nice_step(lo: f64, hi: f64) -> f64 {
    let span = (hi - lo).abs().max(f64::EPSILON);
    let raw = span / 5.0;
    let mag = 10f64.powf(raw.log10().floor());
    let n = raw / mag;
    let k = if n <= 1.0 {
        1.0
    } else if n <= 2.0 {
        2.0
    } else if n <= 5.0 {
        5.0
    } else {
        10.0
    };
    k * mag
}

/// A tick's number as the axis shows it.
fn tick(v: f64, step: f64, format: Option<&str>) -> String {
    let percent = format.is_some_and(|f| f.contains('%'));
    let (v, step) = if percent {
        (v * 100.0, step * 100.0)
    } else {
        (v, step)
    };
    let decimals = if step >= 1.0 {
        0
    } else {
        (-step.log10().floor()) as usize
    };
    let mut s = format!("{v:.decimals$}");
    if s == "-0" {
        s = "0".into();
    }
    if format.is_some_and(|f| f.contains(',')) && decimals == 0 {
        let neg = s.starts_with('-');
        let digits: Vec<char> = s.trim_start_matches('-').chars().collect();
        let mut out = String::new();
        for (i, c) in digits.iter().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(*c);
        }
        s = if neg { format!("-{out}") } else { out };
    }
    if percent {
        s.push('%');
    }
    crate::sheet_print::escape(&s)
}

/// The chart as a TikZ picture `w` by `h` inches.
pub fn tikz(chart: &Chart, w: f32, h: f32) -> String {
    let (w, h) = (f64::from(w.max(0.5)), f64::from(h.max(0.4)));
    let mut out = String::from("\\begin{tikzpicture}[x=1in,y=1in,font=\\tiny]\n");
    out.push_str(&format!(
        "\\path[use as bounding box] (0,0) rectangle ({w:.3},{h:.3});\n"
    ));
    let area = |p: Paint, default: Option<[u8; 3]>| match p {
        Paint::Color(c) => Some(c),
        Paint::None => None,
        Paint::Automatic => default,
    };
    let bg = area(chart.background, Some([0xFF, 0xFF, 0xFF]));
    let border = area(chart.border, Some([0xD9, 0xD9, 0xD9]));
    let mut opts = Vec::new();
    if let Some(c) = bg {
        opts.push(format!("fill={}", rgb(c)));
    }
    if let Some(c) = border {
        opts.push(format!("draw={}", rgb(c)));
    }
    if !opts.is_empty() {
        out.push_str(&format!(
            "\\path[{}] (0,0) rectangle ({w:.3},{h:.3});\n",
            opts.join(",")
        ));
    }
    // The room the parts take: title on top, legend at its side.
    let (mut left, mut right, mut bottom, mut top) = (0.08, w - 0.08, 0.08, h - 0.08);
    if let Some(t) = chart.title.as_deref().filter(|t| !t.is_empty()) {
        out.push_str(&format!(
            "\\node[anchor=north,font=\\footnotesize\\bfseries] at ({:.3},{top:.3}) {{{}}};\n",
            w / 2.0,
            crate::sheet_print::escape(t)
        ));
        top -= 0.22;
    }
    let pie = matches!(chart.kind, ChartKind::Pie | ChartKind::Doughnut);
    let names: Vec<(String, [u8; 3])> = if pie {
        chart
            .categories
            .iter()
            .enumerate()
            .map(|(j, c)| (c.clone(), point_color(chart, 0, j)))
            .collect()
    } else {
        (0..chart.series.len())
            .map(|i| (chart.series[i].name.clone(), series_color(chart, i)))
            .collect()
    };
    if let Some(pos) = chart.legend.filter(|_| !names.is_empty()) {
        let key = |out: &mut String, x: f64, y: f64, name: &str, c: [u8; 3]| {
            out.push_str(&format!(
                "\\fill[{}] ({x:.3},{:.3}) rectangle ({:.3},{:.3});\\node[anchor=west] at ({:.3},{y:.3}) {{{}}};\n",
                color_of(c),
                y - 0.03,
                x + 0.06,
                y + 0.03,
                x + 0.07,
                crate::sheet_print::escape(name)
            ));
        };
        match pos {
            LegendPosition::Bottom | LegendPosition::Top => {
                let each = (right - left) / names.len() as f64;
                let y = if pos == LegendPosition::Bottom {
                    bottom + 0.06
                } else {
                    top - 0.06
                };
                for (i, (n, c)) in names.iter().enumerate() {
                    key(&mut out, left + each * i as f64, y, n, *c);
                }
                if pos == LegendPosition::Bottom {
                    bottom += 0.18;
                } else {
                    top -= 0.18;
                }
            }
            LegendPosition::Left => {
                for (i, (n, c)) in names.iter().enumerate() {
                    key(
                        &mut out,
                        left,
                        (top + bottom) / 2.0 + 0.12 * (names.len() as f64 / 2.0 - i as f64),
                        n,
                        *c,
                    );
                }
                left += (w * 0.22).min(1.2);
            }
            LegendPosition::Right | LegendPosition::TopRight => {
                let x = right - (w * 0.22).min(1.2);
                for (i, (n, c)) in names.iter().enumerate() {
                    let y = if pos == LegendPosition::TopRight {
                        top - 0.06 - 0.12 * i as f64
                    } else {
                        (top + bottom) / 2.0 + 0.12 * (names.len() as f64 / 2.0 - i as f64)
                    };
                    key(&mut out, x, y, n, *c);
                }
                right = x - 0.05;
            }
        }
    }
    if pie {
        pie_chart(&mut out, chart, left, right, bottom, top);
    } else if chart.kind == ChartKind::Other {
        out.push_str(&format!(
            "\\node at ({:.3},{:.3}) {{Chart}};\n",
            (left + right) / 2.0,
            (top + bottom) / 2.0
        ));
    } else {
        axes_chart(&mut out, chart, left, right, bottom, top);
    }
    out.push_str("\\end{tikzpicture}");
    out
}

fn pie_chart(out: &mut String, chart: &Chart, left: f64, right: f64, bottom: f64, top: f64) {
    let Some(series) = chart.series.first() else {
        return;
    };
    let values: Vec<f64> = series
        .values
        .iter()
        .map(|v| v.unwrap_or(0.0).max(0.0))
        .collect();
    let total: f64 = values.iter().sum();
    if total <= 0.0 {
        return;
    }
    let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
    let r = ((right - left).min(top - bottom) / 2.0 - 0.05).max(0.1);
    let mut angle = 90.0;
    for (j, v) in values.iter().enumerate() {
        if *v <= 0.0 {
            continue;
        }
        let sweep = v / total * 360.0;
        let end = angle - sweep;
        let c = point_color(chart, 0, j);
        out.push_str(&format!(
            "\\filldraw[fill={},draw=white] ({cx:.3},{cy:.3}) -- ++({angle:.2}:{r:.3}) arc[start angle={angle:.2},end angle={end:.2},radius={r:.3}] -- cycle;\n",
            rgb(c)
        ));
        if chart.labels.any() {
            let mid = (angle + end) / 2.0;
            let text = if chart.labels.percent {
                format!("{:.0}\\%", v / total * 100.0)
            } else {
                tick(*v, 1.0, chart.axis_format.as_deref())
            };
            out.push_str(&format!(
                "\\node at ([shift={{({mid:.2}:{:.3})}}]{cx:.3},{cy:.3}) {{{text}}};\n",
                r * 0.7
            ));
        }
        angle = end;
    }
    if chart.kind == ChartKind::Doughnut {
        out.push_str(&format!(
            "\\fill[white] ({cx:.3},{cy:.3}) circle[radius={:.3}];\n",
            r * 0.5
        ));
    }
}

fn axes_chart(out: &mut String, chart: &Chart, left: f64, right: f64, bottom: f64, top: f64) {
    let horizontal = chart.kind == ChartKind::Bar;
    let scatter = chart.kind == ChartKind::Scatter;
    let stacked = chart.stacked
        && matches!(
            chart.kind,
            ChartKind::Column | ChartKind::Bar | ChartKind::Area
        );
    let n = chart
        .series
        .iter()
        .map(|s| s.values.len())
        .max()
        .unwrap_or(0)
        .max(chart.categories.len());
    if n == 0 {
        return;
    }
    // The values' range, stacked ones summed.
    let mut lo: f64 = 0.0;
    let mut hi: f64 = 0.0;
    for j in 0..n {
        let (mut pos, mut neg) = (0.0, 0.0);
        for s in &chart.series {
            let v = s.values.get(j).copied().flatten().unwrap_or(0.0);
            if stacked {
                if v >= 0.0 {
                    pos += v;
                } else {
                    neg += v;
                }
            } else {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        lo = lo.min(neg);
        hi = hi.max(pos);
    }
    let log = chart.scale.log;
    let tf = |v: f64| {
        if log {
            v.max(f64::MIN_POSITIVE).log10()
        } else {
            v
        }
    };
    let (mut lo, mut hi) = if log {
        let positive = chart
            .series
            .iter()
            .flat_map(|s| s.values.iter().flatten())
            .copied()
            .filter(|v| *v > 0.0)
            .fold(f64::INFINITY, f64::min);
        (
            positive.log10().floor().min(0.0),
            hi.max(1.0).log10().ceil(),
        )
    } else {
        (lo, hi)
    };
    if let Some(m) = chart.scale.min {
        lo = tf(m);
    }
    if let Some(m) = chart.scale.max {
        hi = tf(m);
    }
    if hi <= lo {
        hi = lo + 1.0;
    }
    let step = if log {
        1.0
    } else {
        chart.scale.major.unwrap_or_else(|| nice_step(lo, hi))
    };
    if !log && chart.scale.max.is_none() {
        hi = (hi / step).ceil() * step;
    }
    if !log && chart.scale.min.is_none() && lo < 0.0 {
        lo = (lo / step).floor() * step;
    }
    // The plot area inside the axes' labels and titles.
    let mut pl = left + 0.4;
    let mut pb = bottom + 0.18;
    if chart
        .vertical_title
        .as_deref()
        .is_some_and(|t| !t.is_empty())
    {
        pl += 0.14;
    }
    if chart
        .horizontal_title
        .as_deref()
        .is_some_and(|t| !t.is_empty())
    {
        pb += 0.14;
    }
    let (pr, pt) = (right - 0.05, top - 0.05);
    if pr - pl < 0.2 || pt - pb < 0.2 {
        return;
    }
    if let Paint::Color(c) = chart.plot_background {
        out.push_str(&format!(
            "\\fill[{}] ({pl:.3},{pb:.3}) rectangle ({pr:.3},{pt:.3});\n",
            color_of(c)
        ));
    }
    // Value v along the value axis, in inches.
    let along = |v: f64| -> f64 {
        let t = ((tf(v) - lo) / (hi - lo)).clamp(-0.05, 1.05);
        if horizontal {
            pl + t * (pr - pl)
        } else {
            pb + t * (pt - pb)
        }
    };
    let gray = color_of([0xD9, 0xD9, 0xD9]);
    let mut k = (lo / step).ceil();
    while k * step <= hi + step * 1e-9 {
        let v = k * step;
        let real = if log { 10f64.powf(v) } else { v };
        let label = tick(
            real,
            if log { 1.0 } else { step },
            chart.axis_format.as_deref(),
        );
        let x = along(real);
        if horizontal {
            if chart.gridlines.vertical_major || chart.gridlines.horizontal_major {
                out.push_str(&format!(
                    "\\draw[{gray}] ({x:.3},{pb:.3}) -- ({x:.3},{pt:.3});\n"
                ));
            }
            out.push_str(&format!(
                "\\node[anchor=north] at ({x:.3},{pb:.3}) {{{label}}};\n"
            ));
        } else {
            if chart.gridlines.horizontal_major {
                out.push_str(&format!(
                    "\\draw[{gray}] ({pl:.3},{x:.3}) -- ({pr:.3},{x:.3});\n"
                ));
            }
            out.push_str(&format!(
                "\\node[anchor=east] at ({pl:.3},{x:.3}) {{{label}}};\n"
            ));
        }
        k += 1.0;
    }
    // Categories along the other axis (scatter: its x values).
    let (xlo, xhi) = if scatter {
        let xs: Vec<f64> = chart
            .series
            .iter()
            .flat_map(|s| s.x.iter().flatten())
            .copied()
            .collect();
        if xs.is_empty() {
            (1.0, n as f64)
        } else {
            let a = xs.iter().copied().fold(f64::INFINITY, f64::min).min(0.0);
            let b = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let s = nice_step(a, b);
            (a, (b / s).ceil() * s)
        }
    } else {
        (0.0, 0.0)
    };
    let slot = if horizontal {
        (pt - pb) / n as f64
    } else {
        (pr - pl) / n as f64
    };
    let center = |j: usize| -> f64 {
        if horizontal {
            pt - slot * (j as f64 + 0.5)
        } else {
            pl + slot * (j as f64 + 0.5)
        }
    };
    if scatter {
        let s = nice_step(xlo, xhi);
        let mut v = xlo;
        while v <= xhi + s * 1e-9 {
            let x = pl + (v - xlo) / (xhi - xlo).max(f64::EPSILON) * (pr - pl);
            out.push_str(&format!(
                "\\node[anchor=north] at ({x:.3},{pb:.3}) {{{}}};\n",
                tick(v, s, None)
            ));
            v += s;
        }
    } else {
        for (j, c) in chart.categories.iter().enumerate().take(n) {
            let p = center(j);
            let text = crate::sheet_print::escape(c);
            if horizontal {
                out.push_str(&format!(
                    "\\node[anchor=east] at ({pl:.3},{p:.3}) {{{text}}};\n"
                ));
            } else {
                out.push_str(&format!(
                    "\\node[anchor=north,text width={:.3}in,align=center] at ({p:.3},{pb:.3}) {{{text}}};\n",
                    slot.max(0.2)
                ));
            }
        }
    }
    let zero = along(if log {
        10f64.powf(lo)
    } else {
        0.0f64.clamp(lo, hi)
    });
    let axis = color_of([0xBF, 0xBF, 0xBF]);
    if horizontal {
        out.push_str(&format!(
            "\\draw[{axis}] ({zero:.3},{pb:.3}) -- ({zero:.3},{pt:.3});\n"
        ));
    } else {
        out.push_str(&format!(
            "\\draw[{axis}] ({pl:.3},{zero:.3}) -- ({pr:.3},{zero:.3});\n"
        ));
    }
    if let Paint::Color(c) = chart.plot_border {
        out.push_str(&format!(
            "\\draw[{}] ({pl:.3},{pb:.3}) rectangle ({pr:.3},{pt:.3});\n",
            color_of(c)
        ));
    }
    if let Some(t) = chart.vertical_title.as_deref().filter(|t| !t.is_empty()) {
        out.push_str(&format!(
            "\\node[rotate=90,anchor=north] at ({:.3},{:.3}) {{{}}};\n",
            left,
            (pb + pt) / 2.0,
            crate::sheet_print::escape(t)
        ));
    }
    if let Some(t) = chart.horizontal_title.as_deref().filter(|t| !t.is_empty()) {
        out.push_str(&format!(
            "\\node[anchor=south] at ({:.3},{:.3}) {{{}}};\n",
            (pl + pr) / 2.0,
            bottom,
            crate::sheet_print::escape(t)
        ));
    }
    let label = |out: &mut String, x: f64, y: f64, v: f64, anchor: &str| {
        if chart.labels.value {
            out.push_str(&format!(
                "\\node[anchor={anchor},inner sep=1pt] at ({x:.3},{y:.3}) {{{}}};\n",
                tick(v, 1.0, chart.axis_format.as_deref())
            ));
        }
    };
    let count = chart.series.len().max(1);
    match chart.kind {
        ChartKind::Column | ChartKind::Bar => {
            let mut pos = vec![0.0; n];
            let mut neg = vec![0.0; n];
            let width = slot * 0.7 / if stacked { 1.0 } else { count as f64 };
            for (i, s) in chart.series.iter().enumerate() {
                let c = color_of(series_color(chart, i));
                for j in 0..n {
                    let Some(v) = s.values.get(j).copied().flatten() else {
                        continue;
                    };
                    let (from, to) = if stacked {
                        let base = if v >= 0.0 { &mut pos[j] } else { &mut neg[j] };
                        let from = *base;
                        *base += v;
                        (from, *base)
                    } else {
                        (if log { 10f64.powf(lo) } else { 0.0 }, v)
                    };
                    let mid = center(j);
                    let off = if stacked {
                        -width / 2.0
                    } else {
                        -slot * 0.35 + width * i as f64
                    };
                    let (a, b) = (along(from), along(to));
                    if horizontal {
                        let y0 = mid - off - width;
                        out.push_str(&format!(
                            "\\fill[{c}] ({a:.3},{y0:.3}) rectangle ({b:.3},{:.3});\n",
                            y0 + width
                        ));
                        label(out, b, y0 + width / 2.0, v, "west");
                    } else {
                        let x0 = mid + off;
                        out.push_str(&format!(
                            "\\fill[{c}] ({x0:.3},{a:.3}) rectangle ({:.3},{b:.3});\n",
                            x0 + width
                        ));
                        label(out, x0 + width / 2.0, b, v, "south");
                    }
                }
            }
        }
        ChartKind::Area => {
            let mut base = vec![0.0; n];
            for (i, s) in chart.series.iter().enumerate() {
                let c = color_of(series_color(chart, i));
                let mut upper = Vec::new();
                let mut lower = Vec::new();
                for (j, b) in base.iter_mut().enumerate() {
                    let v = s.values.get(j).copied().flatten().unwrap_or(0.0);
                    let from = if stacked { *b } else { 0.0 };
                    let to = from + v;
                    if stacked {
                        *b = to;
                    }
                    upper.push(format!("({:.3},{:.3})", center(j), along(to)));
                    lower.push(format!("({:.3},{:.3})", center(j), along(from)));
                }
                lower.reverse();
                out.push_str(&format!(
                    "\\fill[{c},fill opacity=0.85] {} -- {} -- cycle;\n",
                    upper.join(" -- "),
                    lower.join(" -- ")
                ));
            }
        }
        ChartKind::Line | ChartKind::Scatter => {
            for (i, s) in chart.series.iter().enumerate() {
                let c = color_of(series_color(chart, i));
                let mut path: Vec<String> = Vec::new();
                let mut points = Vec::new();
                for j in 0..n {
                    let Some(v) = s.values.get(j).copied().flatten() else {
                        continue;
                    };
                    let x = if scatter {
                        let xv = s.x.get(j).copied().flatten().unwrap_or(j as f64 + 1.0);
                        pl + (xv - xlo) / (xhi - xlo).max(f64::EPSILON) * (pr - pl)
                    } else {
                        center(j)
                    };
                    let y = along(v);
                    path.push(format!("({x:.3},{y:.3})"));
                    points.push((x, y, v));
                }
                if !scatter && path.len() > 1 {
                    out.push_str(&format!(
                        "\\draw[{c},line width=1.2pt] {};\n",
                        path.join(" -- ")
                    ));
                }
                for (x, y, v) in points {
                    out.push_str(&format!(
                        "\\fill[{c}] ({x:.3},{y:.3}) circle[radius=0.025];\n"
                    ));
                    label(out, x, y, v, "south");
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kalem_viewer::ChartSeries;

    #[test]
    fn steps_and_ticks() {
        assert_eq!(nice_step(0.0, 100.0), 20.0);
        assert_eq!(nice_step(0.0, 7.0), 2.0);
        assert_eq!(nice_step(0.0, 0.4), 0.1);
        assert_eq!(tick(1500.0, 500.0, Some("#,##0")), "1,500");
        assert_eq!(tick(0.25, 0.05, Some("0%")), "25\\%");
        assert_eq!(tick(2.5, 0.5, None), "2.5");
    }

    #[test]
    fn charts_as_tikz() {
        let mut chart = Chart {
            kind: ChartKind::Column,
            title: Some("Sales & Costs".into()),
            categories: vec!["Q1".into(), "Q2".into()],
            series: vec![ChartSeries {
                name: "2026".into(),
                values: vec![Some(10.0), Some(20.0)],
                ..ChartSeries::default()
            }],
            legend: Some(LegendPosition::Right),
            ..Chart::default()
        };
        let t = tikz(&chart, 4.0, 3.0);
        assert!(t.contains("Sales \\& Costs"), "{t}");
        assert!(t.contains("rectangle"), "{t}");
        assert!(t.contains("{Q2}"), "{t}");
        chart.kind = ChartKind::Pie;
        let t = tikz(&chart, 3.0, 3.0);
        assert!(t.contains("arc[start angle=90.00"), "{t}");
        chart.kind = ChartKind::Line;
        assert!(tikz(&chart, 3.0, 2.0).contains(" -- "));
    }
}
