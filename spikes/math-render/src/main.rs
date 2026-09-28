//! Math rendering spike (tasks T0.7, decision D4): renders the formula
//! corpus with each engine, reports coverage and timing, and writes SVGs
//! for visual comparison.
//!
//! Usage: math-render-spike [corpus.txt] [--out DIR] [--engine typst|ratex]

use std::time::{Duration, Instant};

pub struct Formula {
    pub category: String,
    pub display: bool,
    pub latex: String,
}

/// One rendered formula.
pub struct Rendered {
    pub svg: String,
    /// Size in pixels at the requested font size.
    pub width: f64,
    pub height: f64,
    /// Distance from the top to the baseline, in pixels.
    pub baseline: f64,
}

pub trait Engine {
    fn name(&self) -> &'static str;
    fn render(&mut self, f: &Formula, font_size: f64) -> Result<Rendered, String>;
}

#[cfg(feature = "typst")]
mod typst_engine;

#[cfg(feature = "ratex")]
mod ratex_engine {
    use super::{Engine, Formula, Rendered};
    use ratex_layout::LayoutOptions;
    use ratex_types::math_style::MathStyle;

    pub struct Ratex;

    impl Engine for Ratex {
        fn name(&self) -> &'static str {
            "ratex"
        }

        fn render(&mut self, f: &Formula, font_size: f64) -> Result<Rendered, String> {
            let nodes = ratex_parser::parse(&f.latex).map_err(|e| format!("{e:?}"))?;
            let opts = LayoutOptions { style: if f.display { MathStyle::Display } else { MathStyle::Text }, ..Default::default() };
            let lbox = ratex_layout::layout(&nodes, &opts);
            let list = ratex_layout::to_display_list(&lbox);
            let svg_opts = ratex_svg::SvgOptions { font_size, padding: 0.0, embed_glyphs: true, ..Default::default() };
            let svg = ratex_svg::render_to_svg(&list, &svg_opts);
            Ok(Rendered {
                svg,
                width: list.width * font_size,
                height: (list.height + list.depth) * font_size,
                baseline: list.height * font_size,
            })
        }
    }
}

fn load_corpus(path: &str) -> Vec<Formula> {
    let text = std::fs::read_to_string(path).expect("read corpus");
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let mut it = l.splitn(3, '\t');
            let category = it.next().unwrap().to_string();
            let display = it.next().unwrap() == "d";
            let latex = it.next().unwrap().to_string();
            Formula { category, display, latex }
        })
        .collect()
}

fn engines(which: Option<&str>) -> Vec<Box<dyn Engine>> {
    let mut out: Vec<Box<dyn Engine>> = Vec::new();
    #[cfg(feature = "typst")]
    if which.is_none_or(|w| w == "typst") {
        out.push(Box::new(typst_engine::Typst::new()));
    }
    #[cfg(feature = "ratex")]
    if which.is_none_or(|w| w == "ratex") {
        out.push(Box::new(ratex_engine::Ratex));
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let corpus = args.get(1).filter(|a| !a.starts_with("--")).cloned().unwrap_or_else(|| "corpus.txt".into());
    let flag = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let out = flag("--out");
    let formulas = load_corpus(&corpus);
    let font_size = 20.0;
    for mut engine in engines(flag("--engine").as_deref()) {
        let name = engine.name();
        // Start-up: the first render pays for fonts and library set-up.
        let t = Instant::now();
        let _ = engine.render(&Formula { category: "warmup".into(), display: false, latex: "y = mx + b".into() }, font_size);
        let startup = t.elapsed();
        let mut times: Vec<Duration> = Vec::new();
        let mut raster: Vec<Duration> = Vec::new();
        let opts = resvg::usvg::Options::default();
        let mut failures: Vec<(usize, String)> = Vec::new();
        let mut report = String::new();
        for (i, f) in formulas.iter().enumerate() {
            let t = Instant::now();
            let r = engine.render(f, font_size);
            times.push(t.elapsed());
            match r {
                Ok(r) => {
                    // Rasterize at 2x, as for a Retina display.
                    let t = Instant::now();
                    if let Ok(tree) = resvg::usvg::Tree::from_str(&r.svg, &opts) {
                        let s = tree.size();
                        if let Some(mut pm) = resvg::tiny_skia::Pixmap::new((s.width() * 2.).ceil() as u32 + 1, (s.height() * 2.).ceil() as u32 + 1) {
                            resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(2., 2.), &mut pm.as_mut());
                        }
                    }
                    raster.push(t.elapsed());
                    report.push_str(&format!("{i}\tok\t{:.1}\t{:.1}\t{:.1}\n", r.width, r.height, r.baseline));
                    if let Some(dir) = &out {
                        let d = format!("{dir}/{name}");
                        std::fs::create_dir_all(&d).unwrap();
                        std::fs::write(format!("{d}/{i:03}.svg"), &r.svg).unwrap();
                    }
                }
                Err(e) => {
                    let e: String = e.chars().take(200).collect();
                    report.push_str(&format!("{i}\terror\t{e}\n"));
                    failures.push((i, e));
                }
            }
        }
        // Warm pass: the same formulas again (caches warm).
        let t = Instant::now();
        for f in &formulas {
            let _ = engine.render(f, font_size);
        }
        let warm = t.elapsed() / formulas.len() as u32;
        let mut sorted = times.clone();
        sorted.sort();
        let ms = |d: Duration| d.as_secs_f64() * 1000.;
        println!(
            "{name}: {}/{} rendered; start-up {:.1} ms; first render p50 {:.2} ms, p90 {:.2} ms, max {:.2} ms; repeat {:.3} ms",
            formulas.len() - failures.len(),
            formulas.len(),
            ms(startup),
            ms(sorted[sorted.len() / 2]),
            ms(sorted[sorted.len() * 9 / 10]),
            ms(*sorted.last().unwrap()),
            ms(warm),
        );
        raster.sort();
        if !raster.is_empty() {
            println!("  SVG parse + rasterize at 2x: p50 {:.2} ms, p90 {:.2} ms, max {:.2} ms", ms(raster[raster.len() / 2]), ms(raster[raster.len() * 9 / 10]), ms(*raster.last().unwrap()));
        }
        for (i, e) in &failures {
            println!("  #{i} [{}] {}: {e}", formulas[*i].category, formulas[*i].latex);
        }
        if let Some(dir) = &out {
            std::fs::write(format!("{dir}/{name}.tsv"), report).unwrap();
        }
    }
}
