//! Compile and compare (T2.7h.33): Kalem's build of the editing template
//! with each engine installed, twice with `SOURCE_DATE_EPOCH`, gives the
//! same PDF byte for byte, and a SyncTeX file beside it. pdfLaTeX and
//! Tectonic; an engine that is not installed is skipped (Tectonic in CI's
//! pdflatex job, where it can fetch its bundle), and so is pdfLaTeX
//! without the mwe package the template's picture comes from.

#![allow(clippy::print_stderr)]

use std::path::{Path, PathBuf};

use kalem_core::pdf::Engine;

fn on_path(exe: &str) -> bool {
    let search = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&search).any(|d| d.join(exe).is_file())
}

/// Whether the TeX installation has `file`: the template draws
/// `example-image` from the mwe package, which small installations
/// (BasicTeX, a minimal MiKTeX) leave out.
fn tex_has(file: &str) -> bool {
    std::process::Command::new("kpsewhich")
        .arg(file)
        .output()
        .is_ok_and(|o| o.status.success() && !o.stdout.is_empty())
}

fn template(dir: &Path) -> PathBuf {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/latex/edits/template.tex");
    let tex = dir.join("template.tex");
    std::fs::copy(src, &tex).unwrap();
    tex
}

/// Builds `tex` with `engine`: the PDF's bytes, and whether a SyncTeX
/// file came with it.
fn build(tex: &Path, engine: Engine) -> (Vec<u8>, bool) {
    let built = kalem_core::latex_build::build(tex, engine, None).expect("built");
    let errors: Vec<_> = built
        .problems
        .iter()
        .filter(|p| p.severity == kalem_core::latex_build::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let pdf = built.pdf.expect("a PDF");
    let synctex = kalem_core::synctex::Synctex::for_pdf(&pdf).is_some();
    (std::fs::read(&pdf).unwrap(), synctex)
}

#[test]
fn builds_are_reproducible() {
    // The builds inherit the environment: the test runs again in a
    // process of its own with the fixed date.
    if std::env::var_os("SOURCE_DATE_EPOCH").is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "builds_are_reproducible", "--nocapture"])
            .env("SOURCE_DATE_EPOCH", "1700000000")
            .env("FORCE_SOURCE_DATE", "1")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let mut ran = 0;
    for (engine, exe) in [
        (Engine::PdfLatex, "pdflatex"),
        (Engine::Tectonic, "tectonic"),
    ] {
        if !on_path(exe) {
            eprintln!("{exe} not installed: skipped");
            continue;
        }
        if exe == "pdflatex" && !tex_has("example-image.pdf") {
            eprintln!("{exe}: the mwe package (example-image) is not installed: skipped");
            continue;
        }
        let dir = std::env::temp_dir().join(format!("kalem-compile-{exe}-{}", std::process::id()));
        let tex = template(&dir);
        let (first, synctex) = build(&tex, engine);
        assert!(synctex, "{exe} wrote no SyncTeX file");
        let (second, _) = build(&tex, engine);
        assert!(first.starts_with(b"%PDF-"));
        assert!(first == second, "{exe}: two builds differ");
        let _ = std::fs::remove_dir_all(&dir);
        ran += 1;
    }
    eprintln!("{ran} engines compared");
}
