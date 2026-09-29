//! LaTeX through pandoc (T2.7h.26): HTML, Markdown and Word for
//! co-authors, and a one-way conversion to Org. Skipped without pandoc.

#![allow(clippy::print_stderr)]

use std::sync::Arc;

#[test]
fn exports_and_converts() {
    let search = std::env::var_os("PATH").unwrap_or_default();
    let Some(pandoc) = kalem_core::pandoc::find(&search) else {
        eprintln!("no pandoc: LaTeX through pandoc is not checked");
        return;
    };
    let dir = std::env::temp_dir().join(format!("kalem-latex-pandoc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let tex = dir.join("paper.tex");
    std::fs::write(&tex, "\\documentclass{article}\\begin{document}\\section{Intro}Hello $x^2$ \\emph{world}.\\end{document}\n").unwrap();
    for (to, ext, want) in [("html5", "html", "<math"), ("markdown", "md", "# Intro"), ("docx", "docx", "")] {
        let out = dir.join(format!("paper.{ext}"));
        kalem_core::pandoc::export_latex(&pandoc, &tex, to, &out).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        assert!(!bytes.is_empty());
        if !want.is_empty() {
            assert!(String::from_utf8_lossy(&bytes).contains(want), "{to}");
        }
    }
    // Convert to Org, from the LaTeX mode.
    let base = kalem_core::Config::default().parse_base();
    let mut doc = kalem_core::DocumentState::open(&tex, Arc::new(org_model::Settings::default()), &base).unwrap();
    assert_eq!(doc.meta.mode, kalem_core::DocumentMode::Latex);
    let reg = kalem_core::CommandRegistry::with_builtins();
    let mut clip = kalem_core::command::Clipboard::default();
    let config = kalem_core::Config::default();
    let mut ctx = kalem_core::command::EditorContext {
        document: Some(&mut doc),
        clipboard: &mut clip,
        config: &config,
        now: std::time::Instant::now(),
        clock: jiff::civil::date(2026, 9, 29).at(10, 0, 0, 0),
        messages: Vec::new(),
        requests: Vec::new(),
    };
    reg.execute("latex.convertToOrg", &mut ctx, &serde_json::Value::Null).unwrap();
    let org = std::fs::read_to_string(dir.join("paper.org")).unwrap();
    assert!(org.contains("* Intro") && org.contains("/world/"), "{org}");
    let _ = std::fs::remove_dir_all(&dir);
}
