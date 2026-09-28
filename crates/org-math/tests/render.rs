//! Rendering: the engine's behavior, and image snapshots of a formula
//! corpus (`corpus.txt`, from the D4 evaluation) compared with a pixel
//! threshold. `KALEM_UPDATE_SNAPSHOTS=1` writes new snapshots.

#![allow(clippy::print_stderr)]

use org_math::{Cache, MathEngine, Ratex, Request, source};

fn request(latex: &str, display: bool) -> Request {
    Request {
        latex: latex.to_string(),
        display,
        size: 20.,
        scale: 2.,
        color: [0, 0, 0, 255],
    }
}

#[test]
fn renders_and_fails() {
    let img = Ratex.render(&request("x^2 + y^2 = z^2", false)).unwrap();
    assert!(img.width > 100 && img.height > 20, "{img:?}");
    assert!(img.baseline > 0. && img.baseline < img.height as f32);
    assert!(img.rgba.as_chunks::<4>().0.iter().any(|p| p[3] == 255));
    // The color is the one asked for.
    let red = Ratex
        .render(&Request {
            color: [200, 0, 0, 255],
            ..request("x", false)
        })
        .unwrap();
    assert!(
        red.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] == 255 && p[0] == 200 && p[1] == 0)
    );
    // Display style makes big operators bigger.
    let inline = Ratex.render(&request("\\sum_{i=1}^n i", false)).unwrap();
    let display = Ratex.render(&request("\\sum_{i=1}^n i", true)).unwrap();
    assert!(display.height > inline.height);
    // Errors are reported, never an empty image.
    assert!(Ratex.render(&request("\\frac{1}{", false)).is_err());
    assert!(Ratex.render(&request("\\nosuchcommand", false)).is_err());
    // Definitions from #+LATEX_HEADER apply.
    let m = source::macros(&["\\newcommand{\\R}{\\mathbb{R}}".into()]);
    assert!(
        Ratex
            .render(&request(&source::prepare("x \\in \\R", &m), false))
            .is_ok()
    );
    // What RaTeX lacks is mapped (decision D4).
    let multline = source::prepare("\\begin{multline}a+b\\\\+c\\end{multline}", "");
    assert!(Ratex.render(&request(&multline, true)).is_ok());
    assert!(
        Ratex
            .render(&request(&source::prepare("\\mbox{if } x", ""), false))
            .is_ok()
    );
}

#[test]
fn caches() {
    let cache = Cache::new(Box::new(Ratex), 4);
    let a = cache.get(&request("a", false));
    let again = cache.get(&request("a", false));
    assert!(std::sync::Arc::ptr_eq(&a, &again));
    for f in ["b", "c", "d", "e", "f"] {
        cache.get(&request(f, false));
    }
    assert!(cache.len() <= 4);
    assert!(cache.peek(&request("f", false)).is_some());
}

/// The share of pixels whose alpha differs by more than a quarter.
fn difference(a: &image::RgbaImage, b: &org_math::Image) -> f64 {
    if a.width() != b.width || a.height() != b.height {
        return 1.;
    }
    let differing = a
        .pixels()
        .zip(b.rgba.as_chunks::<4>().0.iter())
        .filter(|(p, q)| (i16::from(p.0[3]) - i16::from(q[3])).abs() > 64)
        .count();
    differing as f64 / f64::from(b.width * b.height)
}

#[test]
fn snapshots() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/snapshots");
    let update = std::env::var_os("KALEM_UPDATE_SNAPSHOTS").is_some();
    let corpus = include_str!("corpus.txt");
    let mut failed = Vec::new();
    for (i, line) in corpus.lines().enumerate() {
        let mut it = line.splitn(3, '\t');
        let (Some(_), Some(mode), Some(latex)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let latex = source::prepare(latex, "");
        let path = format!("{dir}/{i:03}.png");
        let Ok(img) = Ratex.render(&request(&latex, mode == "d")) else {
            failed.push(format!("{i}: {latex}: does not render"));
            continue;
        };
        if update || !std::path::Path::new(&path).exists() {
            image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone())
                .unwrap()
                .save(&path)
                .unwrap();
            continue;
        }
        let expected = image::open(&path).unwrap().to_rgba8();
        let d = difference(&expected, &img);
        if d > 0.005 {
            failed.push(format!("{i}: {latex}: {:.1}% of pixels differ", d * 100.));
        }
    }
    for f in &failed {
        eprintln!("{f}");
    }
    assert!(
        failed.is_empty(),
        "{} formulas differ from their snapshots",
        failed.len()
    );
}
