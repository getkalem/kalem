//! What the editors draw of a chart beyond its plain series, worked out
//! once for the graphical editor, the terminal and printing: trendlines
//! fitted to a series (with their equation and R²), error bars, the
//! series of a combo chart drawn as another kind or against the secondary
//! axis, and the kinds drawn from shapes of their own (histograms,
//! waterfalls, stock, bubble and radar charts). Shapes are in data
//! coordinates: x a category's index (or a scatter's x), y a value; a
//! radar's in a unit square.

use kalem_viewer::{Chart, ChartKind, ChartSeries, ErrorBars, ErrorKind, TrendKind, Trendline};

/// Office's colors for series without their own.
pub const PALETTE: [[u8; 3]; 6] = [
    [0x44, 0x72, 0xC4],
    [0xED, 0x7D, 0x31],
    [0xA5, 0xA5, 0xA5],
    [0xFF, 0xC0, 0x00],
    [0x5B, 0x9B, 0xD5],
    [0x70, 0xAD, 0x47],
];

/// A series' color: its own, or the palette's by its place.
pub fn series_color(s: &ChartSeries, i: usize) -> [u8; 3] {
    s.color.unwrap_or(PALETTE[i % PALETTE.len()])
}

/// A fitted trendline: points along it, its equation and R².
#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    /// Points along it, x and y.
    pub points: Vec<(f64, f64)>,
    /// Its equation (`y = 2.5x + 1`); empty for a moving average.
    pub equation: String,
    /// Its R²; `None` for a moving average.
    pub r_squared: Option<f64>,
}

/// A number as a trendline's equation writes it: four significant digits.
fn short(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return "0".into();
    }
    let digits = (3 - v.abs().log10().floor() as i32).clamp(0, 8) as usize;
    let s = format!("{v:.digits$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    };
    if s == "-0" { "0".into() } else { s }
}

/// `+ 3` or `- 3`, a term after the first.
fn signed(v: f64, term: &str) -> String {
    if v < 0.0 {
        format!(" - {}{term}", short(-v))
    } else {
        format!(" + {}{term}", short(v))
    }
}

/// The least-squares line through points: slope and intercept.
fn line_fit(xs: &[f64], ys: &[f64]) -> Option<(f64, f64)> {
    let n = xs.len() as f64;
    if xs.len() < 2 {
        return None;
    }
    let (sx, sy) = (xs.iter().sum::<f64>(), ys.iter().sum::<f64>());
    let sxx: f64 = xs.iter().map(|x| x * x).sum();
    let sxy: f64 = xs.iter().zip(ys).map(|(x, y)| x * y).sum();
    let d = n * sxx - sx * sx;
    if d.abs() < f64::EPSILON {
        return None;
    }
    let m = (n * sxy - sx * sy) / d;
    Some((m, (sy - m * sx) / n))
}

/// R² of predictions `f` for points.
fn r_squared(xs: &[f64], ys: &[f64], f: impl Fn(f64) -> f64) -> f64 {
    let mean = ys.iter().sum::<f64>() / ys.len().max(1) as f64;
    let tot: f64 = ys.iter().map(|y| (y - mean).powi(2)).sum();
    let res: f64 = xs.iter().zip(ys).map(|(x, y)| (y - f(*x)).powi(2)).sum();
    if tot <= 0.0 { 1.0 } else { 1.0 - res / tot }
}

/// A polynomial's coefficients (lowest first) of `order` fitted to points.
fn poly_fit(xs: &[f64], ys: &[f64], order: usize) -> Option<Vec<f64>> {
    let k = order + 1;
    if xs.len() < k {
        return None;
    }
    // The normal equations, solved by elimination.
    let mut a = vec![vec![0.0; k + 1]; k];
    for (x, y) in xs.iter().zip(ys) {
        for (r, row) in a.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate().take(k) {
                *cell += x.powi((r + c) as i32);
            }
            row[k] += y * x.powi(r as i32);
        }
    }
    for c in 0..k {
        let p = (c..k).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        a.swap(c, p);
        if a[c][c].abs() < 1e-12 {
            return None;
        }
        let pivot = a[c].clone();
        for (r, row) in a.iter_mut().enumerate() {
            if r != c {
                let f = row[c] / pivot[c];
                for (cell, p) in row.iter_mut().zip(&pivot).skip(c) {
                    *cell -= f * p;
                }
            }
        }
    }
    Some((0..k).map(|i| a[i][k] / a[i][i]).collect())
}

const SUPERSCRIPTS: [&str; 7] = ["", "", "²", "³", "⁴", "⁵", "⁶"];

/// A trendline of points `(x, y)` from `lo` to `hi` along x.
pub fn trend(t: &Trendline, points: &[(f64, f64)], lo: f64, hi: f64) -> Option<Fit> {
    let along = |f: &dyn Fn(f64) -> f64, from: f64| -> Vec<(f64, f64)> {
        (0..=40)
            .map(|k| from + (hi - from) * f64::from(k) / 40.0)
            .map(|x| (x, f(x)))
            .filter(|p| p.1.is_finite())
            .collect()
    };
    let xs: Vec<f64> = points.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = points.iter().map(|p| p.1).collect();
    match t.kind {
        TrendKind::Linear => {
            let (m, b) = line_fit(&xs, &ys)?;
            let f = move |x: f64| m * x + b;
            Some(Fit {
                points: along(&f, lo),
                equation: format!("y = {}x{}", short(m), signed(b, "")),
                r_squared: Some(r_squared(&xs, &ys, f)),
            })
        }
        TrendKind::Exponential => {
            let keep: Vec<(f64, f64)> = points.iter().copied().filter(|p| p.1 > 0.0).collect();
            let lx: Vec<f64> = keep.iter().map(|p| p.0).collect();
            let ly: Vec<f64> = keep.iter().map(|p| p.1.ln()).collect();
            let (b, a) = line_fit(&lx, &ly)?;
            let c = a.exp();
            Some(Fit {
                points: along(&move |x| c * (b * x).exp(), lo),
                equation: format!("y = {}e^{}x", short(c), short(b)),
                r_squared: Some(r_squared(&lx, &ly, move |x| a + b * x)),
            })
        }
        TrendKind::Logarithmic => {
            let keep: Vec<(f64, f64)> = points.iter().copied().filter(|p| p.0 > 0.0).collect();
            let lx: Vec<f64> = keep.iter().map(|p| p.0.ln()).collect();
            let ly: Vec<f64> = keep.iter().map(|p| p.1).collect();
            let (m, b) = line_fit(&lx, &ly)?;
            Some(Fit {
                points: along(&move |x| m * x.ln() + b, lo.max(1e-9)),
                equation: format!("y = {}ln(x){}", short(m), signed(b, "")),
                r_squared: Some(r_squared(&lx, &ly, move |x| m * x + b)),
            })
        }
        TrendKind::Power => {
            let keep: Vec<(f64, f64)> = points
                .iter()
                .copied()
                .filter(|p| p.0 > 0.0 && p.1 > 0.0)
                .collect();
            let lx: Vec<f64> = keep.iter().map(|p| p.0.ln()).collect();
            let ly: Vec<f64> = keep.iter().map(|p| p.1.ln()).collect();
            let (b, a) = line_fit(&lx, &ly)?;
            let c = a.exp();
            Some(Fit {
                points: along(&move |x| c * x.powf(b), lo.max(1e-9)),
                equation: format!("y = {}x^{}", short(c), short(b)),
                r_squared: Some(r_squared(&lx, &ly, move |x| a + b * x)),
            })
        }
        TrendKind::Polynomial => {
            let order = (t.order as usize).clamp(2, 6);
            let k = poly_fit(&xs, &ys, order)?;
            let f = {
                let k = k.clone();
                move |x: f64| {
                    k.iter()
                        .enumerate()
                        .map(|(i, c)| c * x.powi(i as i32))
                        .sum()
                }
            };
            let mut eq = String::from("y = ");
            for (n, i) in (0..=order).rev().enumerate() {
                let term = match i {
                    0 => String::new(),
                    1 => "x".into(),
                    _ => format!("x{}", SUPERSCRIPTS[i]),
                };
                if n == 0 {
                    eq.push_str(&format!("{}{term}", short(k[i])));
                } else {
                    eq.push_str(&signed(k[i], &term));
                }
            }
            Some(Fit {
                points: along(&f, lo),
                equation: eq,
                r_squared: Some(r_squared(&xs, &ys, f)),
            })
        }
        TrendKind::MovingAverage => {
            let p = (t.period as usize).max(2);
            if points.len() < p {
                return None;
            }
            let pts = (p - 1..points.len())
                .map(|i| {
                    let w = &points[i + 1 - p..=i];
                    (points[i].0, w.iter().map(|q| q.1).sum::<f64>() / p as f64)
                })
                .collect();
            Some(Fit {
                points: pts,
                equation: String::new(),
                r_squared: None,
            })
        }
    }
}

/// Each point's error amount, above and below it.
pub fn error_amounts(bars: &ErrorBars, values: &[Option<f64>]) -> Vec<Option<f64>> {
    let present: Vec<f64> = values.iter().flatten().copied().collect();
    let n = present.len() as f64;
    let mean = present.iter().sum::<f64>() / n.max(1.0);
    let sd = (present.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0)).sqrt();
    values
        .iter()
        .map(|v| {
            let v = (*v)?;
            Some(match bars.kind {
                ErrorKind::Fixed => bars.value,
                ErrorKind::Percent => v.abs() * bars.value / 100.0,
                ErrorKind::StdDev => sd * if bars.value > 0.0 { bars.value } else { 1.0 },
                ErrorKind::StdErr => sd / n.max(1.0).sqrt(),
            })
        })
        .collect()
}

/// A waterfall's step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    /// Up from the total before.
    Up,
    /// Down from it.
    Down,
    /// A total standing on the axis.
    Total,
}

/// A waterfall's bars: each one's bottom, top and step.
pub fn waterfall(s: &ChartSeries) -> Vec<(f64, f64, Step)> {
    let mut run = 0.0;
    s.values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let v = v.unwrap_or(0.0);
            if s.subtotals.contains(&i) {
                run = v;
                (0.0, v, Step::Total)
            } else {
                let from = run;
                run += v;
                (from, run, if v >= 0.0 { Step::Up } else { Step::Down })
            }
        })
        .collect()
}

/// Whether a series is drawn as the chart's own kind on the primary axis.
pub fn is_main(chart: &Chart, s: &ChartSeries) -> bool {
    !s.secondary && s.kind.is_none_or(|k| k == chart.kind)
}

/// The values of a chart's primary (or secondary) axis span: the lowest
/// and highest, a waterfall's running totals and error bars counted.
pub fn value_bounds(chart: &Chart, secondary: bool) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut any = false;
    let mut see = |v: f64| {
        lo = lo.min(v);
        hi = hi.max(v);
        any = true;
    };
    for s in chart.series.iter().filter(|s| s.secondary == secondary) {
        if chart.kind == ChartKind::Waterfall {
            for (a, b, _) in waterfall(s) {
                see(a);
                see(b);
            }
            continue;
        }
        let errs = s
            .error_bars
            .map(|b| error_amounts(&b, &s.values))
            .unwrap_or_default();
        for (i, v) in s.values.iter().enumerate() {
            if let Some(v) = v {
                let e = errs.get(i).copied().flatten().unwrap_or(0.0);
                see(v - e);
                see(v + e);
            }
        }
    }
    any.then_some((lo, hi))
}

/// A shape to draw.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// A line through points.
    Line {
        /// Its points.
        points: Vec<(f64, f64)>,
        /// Its color.
        color: [u8; 3],
        /// Its width in points.
        width: f32,
        /// Dashed.
        dashed: bool,
    },
    /// A filled rectangle between two x and two y.
    Rect {
        /// Left and right.
        x: (f64, f64),
        /// Bottom and top.
        y: (f64, f64),
        /// Its color.
        color: [u8; 3],
    },
    /// A filled polygon.
    Polygon {
        /// Its corners.
        points: Vec<(f64, f64)>,
        /// Its color.
        color: [u8; 3],
        /// How opaque, 0 to 1.
        alpha: f32,
    },
    /// A filled circle; its radius a share of the plot's smaller side.
    Dot {
        /// Its center.
        at: (f64, f64),
        /// Its radius.
        radius: f32,
        /// Its color.
        color: [u8; 3],
        /// How opaque.
        alpha: f32,
    },
    /// Text centered at a point (above it when `above`).
    Text {
        /// Where.
        at: (f64, f64),
        /// What.
        text: String,
        /// Above the point rather than on it.
        above: bool,
    },
}

/// A shape on the primary or secondary axis.
pub type Placed = (Shape, bool);

/// A series' x for point `i`: a scatter's own, else the index.
fn x_of(chart: &Chart, s: &ChartSeries, i: usize) -> Option<f64> {
    if matches!(chart.kind, ChartKind::Scatter | ChartKind::Bubble) {
        s.x.get(i).copied().flatten()
    } else {
        Some(i as f64)
    }
}

/// What is drawn over a chart of the plain kinds: the series of other
/// kinds (a combo chart's), trendlines with their equations, error bars.
pub fn overlays(chart: &Chart) -> Vec<Placed> {
    let mut out = Vec::new();
    let n = chart
        .series
        .iter()
        .map(|s| s.values.len())
        .max()
        .unwrap_or(0);
    let overlaid: Vec<usize> = (0..chart.series.len())
        .filter(|&i| !is_main(chart, &chart.series[i]))
        .collect();
    let columns: Vec<usize> = overlaid
        .iter()
        .copied()
        .filter(|&i| chart.series[i].kind.unwrap_or(chart.kind) == ChartKind::Column)
        .collect();
    for &i in &overlaid {
        let s = &chart.series[i];
        let color = series_color(s, i);
        let kind = s.kind.unwrap_or(chart.kind);
        let pts: Vec<(f64, f64)> = (0..n)
            .filter_map(|j| Some((x_of(chart, s, j)?, s.values.get(j).copied().flatten()?)))
            .collect();
        match kind {
            ChartKind::Column | ChartKind::Bar => {
                let k = columns.iter().position(|&c| c == i).unwrap_or(0) as f64;
                let w = 0.7 / columns.len().max(1) as f64;
                for (x, y) in &pts {
                    let left = x - 0.35 + w * k;
                    out.push((
                        Shape::Rect {
                            x: (left, left + w),
                            y: (0.0, *y),
                            color,
                        },
                        s.secondary,
                    ));
                }
            }
            ChartKind::Area => {
                let mut poly = pts.clone();
                if let (Some(first), Some(last)) = (pts.first(), pts.last()) {
                    poly.push((last.0, 0.0));
                    poly.push((first.0, 0.0));
                }
                out.push((
                    Shape::Polygon {
                        points: poly,
                        color,
                        alpha: 0.6,
                    },
                    s.secondary,
                ));
            }
            _ => {
                out.push((
                    Shape::Line {
                        points: pts.clone(),
                        color,
                        width: 2.0,
                        dashed: false,
                    },
                    s.secondary,
                ));
            }
        }
    }
    for (i, s) in chart.series.iter().enumerate() {
        let color = series_color(s, i);
        let pts: Vec<(f64, f64)> = (0..s.values.len())
            .filter_map(|j| Some((x_of(chart, s, j)?, s.values.get(j).copied().flatten()?)))
            .collect();
        if let Some(t) = &s.trendline {
            // A category's x counts from 1, as Excel's equations do.
            let one = if matches!(chart.kind, ChartKind::Scatter | ChartKind::Bubble) {
                0.0
            } else {
                1.0
            };
            let shifted: Vec<(f64, f64)> = pts.iter().map(|p| (p.0 + one, p.1)).collect();
            let lo = shifted.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
            let hi = shifted
                .iter()
                .map(|p| p.0)
                .fold(f64::NEG_INFINITY, f64::max);
            if let Some(fit) = trend(t, &shifted, lo, hi) {
                let line: Vec<(f64, f64)> = fit.points.iter().map(|p| (p.0 - one, p.1)).collect();
                if let Some(&end) = line.last() {
                    let mut text = Vec::new();
                    if t.equation && !fit.equation.is_empty() {
                        text.push(fit.equation.clone());
                    }
                    if t.r_squared
                        && let Some(r) = fit.r_squared
                    {
                        text.push(format!("R² = {r:.4}"));
                    }
                    if !text.is_empty() {
                        out.push((
                            Shape::Text {
                                at: end,
                                text: text.join("  "),
                                above: true,
                            },
                            s.secondary,
                        ));
                    }
                }
                out.push((
                    Shape::Line {
                        points: line,
                        color,
                        width: 1.25,
                        dashed: true,
                    },
                    s.secondary,
                ));
            }
        }
        if let Some(b) = &s.error_bars {
            let amounts = error_amounts(b, &s.values);
            for (j, v) in s.values.iter().enumerate() {
                let (Some(v), Some(e), Some(x)) =
                    (v, amounts.get(j).copied().flatten(), x_of(chart, s, j))
                else {
                    continue;
                };
                let ink = [0x40, 0x40, 0x40];
                out.push((
                    Shape::Line {
                        points: vec![(x, v - e), (x, v + e)],
                        color: ink,
                        width: 1.0,
                        dashed: false,
                    },
                    s.secondary,
                ));
                for y in [v - e, v + e] {
                    out.push((
                        Shape::Line {
                            points: vec![(x - 0.08, y), (x + 0.08, y)],
                            color: ink,
                            width: 1.0,
                            dashed: false,
                        },
                        s.secondary,
                    ));
                }
            }
        }
    }
    out
}

/// The shapes of the kinds drawn from shapes of their own: histogram,
/// waterfall, stock and bubble charts in data coordinates (x a
/// category's index or a bubble's x), a radar chart in a unit square with
/// its center at (0.5, 0.5). `None` for the other kinds.
pub fn special(chart: &Chart) -> Option<Vec<Shape>> {
    let mut out = Vec::new();
    match chart.kind {
        ChartKind::Histogram => {
            let s = chart.series.first()?;
            let color = series_color(s, 0);
            for (i, v) in s.values.iter().enumerate() {
                let v = v.unwrap_or(0.0);
                let x = i as f64;
                out.push(Shape::Rect {
                    x: (x - 0.5, x + 0.5),
                    y: (0.0, v),
                    color,
                });
                // Each bin's outline, as Excel draws it.
                out.push(Shape::Line {
                    points: vec![(x - 0.5, 0.0), (x - 0.5, v), (x + 0.5, v), (x + 0.5, 0.0)],
                    color: [0xFF, 0xFF, 0xFF],
                    width: 0.75,
                    dashed: false,
                });
            }
        }
        ChartKind::Waterfall => {
            let s = chart.series.first()?;
            let steps = waterfall(s);
            for (i, (a, b, step)) in steps.iter().enumerate() {
                let color = match step {
                    Step::Up => PALETTE[0],
                    Step::Down => PALETTE[1],
                    Step::Total => PALETTE[2],
                };
                let x = i as f64;
                out.push(Shape::Rect {
                    x: (x - 0.3, x + 0.3),
                    y: (a.min(*b), a.max(*b)),
                    color,
                });
                // A connector to the next step.
                if i + 1 < steps.len() {
                    out.push(Shape::Line {
                        points: vec![(x + 0.3, *b), (x + 0.7, *b)],
                        color: [0x80, 0x80, 0x80],
                        width: 0.75,
                        dashed: false,
                    });
                }
            }
        }
        ChartKind::Stock => {
            // High, low, close; or open, high, low, close.
            let k = chart.series.len();
            if k < 3 {
                return Some(out);
            }
            let (open, high, low, close) = if k >= 4 {
                (Some(0), 1, 2, 3)
            } else {
                (None, 0, 1, 2)
            };
            let n = chart.series[high].values.len();
            let at = |s: usize, i: usize| chart.series[s].values.get(i).copied().flatten();
            for i in 0..n {
                let x = i as f64;
                if let (Some(h), Some(l)) = (at(high, i), at(low, i)) {
                    out.push(Shape::Line {
                        points: vec![(x, l), (x, h)],
                        color: [0x40, 0x40, 0x40],
                        width: 1.0,
                        dashed: false,
                    });
                }
                if let (Some(o), Some(c)) = (open.and_then(|o| at(o, i)), at(close, i)) {
                    let color = if c >= o {
                        [0xFF, 0xFF, 0xFF]
                    } else {
                        [0x40, 0x40, 0x40]
                    };
                    out.push(Shape::Rect {
                        x: (x - 0.2, x + 0.2),
                        y: (o.min(c), o.max(c)),
                        color,
                    });
                    out.push(Shape::Line {
                        points: vec![
                            (x - 0.2, o.min(c)),
                            (x - 0.2, o.max(c)),
                            (x + 0.2, o.max(c)),
                            (x + 0.2, o.min(c)),
                            (x - 0.2, o.min(c)),
                        ],
                        color: [0x40, 0x40, 0x40],
                        width: 0.75,
                        dashed: false,
                    });
                } else if let Some(c) = at(close, i) {
                    out.push(Shape::Line {
                        points: vec![(x, c), (x + 0.2, c)],
                        color: [0x40, 0x40, 0x40],
                        width: 1.5,
                        dashed: false,
                    });
                }
            }
        }
        ChartKind::Bubble => {
            let largest = chart
                .series
                .iter()
                .flat_map(|s| s.sizes.iter().flatten())
                .copied()
                .fold(0.0f64, f64::max);
            for (i, s) in chart.series.iter().enumerate() {
                let color = series_color(s, i);
                for (j, v) in s.values.iter().enumerate() {
                    let (Some(y), Some(x)) =
                        (v, s.x.get(j).copied().flatten().or(Some(j as f64 + 1.0)))
                    else {
                        continue;
                    };
                    let size = s.sizes.get(j).copied().flatten().unwrap_or(1.0).max(0.0);
                    let r = if largest > 0.0 {
                        0.12 * (size / largest).sqrt()
                    } else {
                        0.05
                    };
                    out.push(Shape::Dot {
                        at: (x, *y),
                        radius: r as f32,
                        color,
                        alpha: 0.75,
                    });
                }
            }
        }
        ChartKind::Radar => {
            let n = chart
                .series
                .iter()
                .map(|s| s.values.len())
                .max()
                .unwrap_or(0);
            if n < 3 {
                return Some(out);
            }
            let hi = chart
                .series
                .iter()
                .flat_map(|s| s.values.iter().flatten())
                .copied()
                .fold(0.0f64, f64::max);
            let hi = if hi > 0.0 { hi } else { 1.0 };
            let corner = |i: usize, r: f64| {
                let a = std::f64::consts::FRAC_PI_2 - std::f64::consts::TAU * i as f64 / n as f64;
                (0.5 + 0.42 * r * a.cos(), 0.5 + 0.42 * r * a.sin())
            };
            let grid = [0xD9, 0xD9, 0xD9];
            for ring in 1..=4 {
                let r = f64::from(ring) / 4.0;
                let mut pts: Vec<(f64, f64)> = (0..n).map(|i| corner(i, r)).collect();
                pts.push(pts[0]);
                out.push(Shape::Line {
                    points: pts,
                    color: grid,
                    width: 0.5,
                    dashed: false,
                });
            }
            for i in 0..n {
                out.push(Shape::Line {
                    points: vec![(0.5, 0.5), corner(i, 1.0)],
                    color: grid,
                    width: 0.5,
                    dashed: false,
                });
                if let Some(c) = chart.categories.get(i) {
                    out.push(Shape::Text {
                        at: corner(i, 1.12),
                        text: c.clone(),
                        above: false,
                    });
                }
            }
            for (k, s) in chart.series.iter().enumerate() {
                let mut pts: Vec<(f64, f64)> = (0..n)
                    .map(|i| {
                        corner(
                            i,
                            s.values.get(i).copied().flatten().unwrap_or(0.0).max(0.0) / hi,
                        )
                    })
                    .collect();
                pts.push(pts[0]);
                out.push(Shape::Line {
                    points: pts,
                    color: series_color(s, k),
                    width: 2.0,
                    dashed: false,
                });
            }
        }
        _ => return None,
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trendlines_fitted() {
        let pts = [(1.0, 3.0), (2.0, 5.0), (3.0, 7.0)];
        let lin = Trendline {
            kind: TrendKind::Linear,
            equation: true,
            r_squared: true,
            ..Trendline::default()
        };
        let f = trend(&lin, &pts, 1.0, 3.0).unwrap();
        assert_eq!(f.equation, "y = 2x + 1");
        assert!((f.r_squared.unwrap() - 1.0).abs() < 1e-9);
        let exp = Trendline {
            kind: TrendKind::Exponential,
            ..lin
        };
        let e = trend(&exp, &[(0.0, 1.0), (1.0, 2.0), (2.0, 4.0)], 0.0, 2.0).unwrap();
        assert_eq!(e.equation, "y = 1e^0.6931x");
        let poly = Trendline {
            kind: TrendKind::Polynomial,
            order: 2,
            ..lin
        };
        let p = trend(
            &poly,
            &[(0.0, 1.0), (1.0, 2.0), (2.0, 5.0), (3.0, 10.0)],
            0.0,
            3.0,
        )
        .unwrap();
        assert_eq!(p.equation, "y = 1x² + 0x + 1");
        let ma = Trendline {
            kind: TrendKind::MovingAverage,
            period: 2,
            ..lin
        };
        let m = trend(&ma, &pts, 1.0, 3.0).unwrap();
        assert_eq!(m.points, vec![(2.0, 4.0), (3.0, 6.0)]);
        assert_eq!(m.r_squared, None);
    }

    #[test]
    fn errors_and_waterfalls() {
        let pct = ErrorBars {
            kind: ErrorKind::Percent,
            value: 10.0,
        };
        assert_eq!(
            error_amounts(&pct, &[Some(50.0), None]),
            vec![Some(5.0), None]
        );
        let s = ChartSeries {
            values: vec![Some(100.0), Some(-30.0), Some(20.0), Some(90.0)],
            subtotals: vec![3],
            ..ChartSeries::default()
        };
        assert_eq!(
            waterfall(&s),
            vec![
                (0.0, 100.0, Step::Up),
                (100.0, 70.0, Step::Down),
                (70.0, 90.0, Step::Up),
                (0.0, 90.0, Step::Total)
            ]
        );
        let chart = Chart {
            kind: ChartKind::Waterfall,
            series: vec![s],
            ..Chart::default()
        };
        assert_eq!(value_bounds(&chart, false), Some((0.0, 100.0)));
        assert_eq!(special(&chart).unwrap().len(), 7);
    }

    #[test]
    fn combo_overlays() {
        let chart = Chart {
            kind: ChartKind::Column,
            series: vec![
                ChartSeries {
                    values: vec![Some(1.0), Some(2.0)],
                    ..ChartSeries::default()
                },
                ChartSeries {
                    values: vec![Some(10.0), Some(20.0)],
                    kind: Some(ChartKind::Line),
                    secondary: true,
                    ..ChartSeries::default()
                },
            ],
            ..Chart::default()
        };
        assert!(is_main(&chart, &chart.series[0]) && !is_main(&chart, &chart.series[1]));
        let o = overlays(&chart);
        assert_eq!(o.len(), 1);
        assert!(matches!(&o[0], (Shape::Line { points, .. }, true) if points.len() == 2));
        assert_eq!(value_bounds(&chart, true), Some((10.0, 20.0)));
    }
}
