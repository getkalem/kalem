//! LaTeX export through pandoc, in a process of its own: the jobs it
//! waits for are global.

#![allow(clippy::print_stderr)]

#[test]
fn latex_export_opens_the_result() {
    let search = std::env::var_os("PATH").unwrap_or_default();
    if kalem_core::pandoc::find(&search).is_none() {
        eprintln!("no pandoc: the LaTeX export is not checked");
        return;
    }
    let dir = std::env::temp_dir().join(format!("kalem-latex-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("paper.tex");
    std::fs::write(
        &path,
        "\\documentclass{article}\n\\begin{document}\nHello $x^2$.\n\\end{document}\n",
    )
    .unwrap();
    let base = kalem_core::settings::Config::default().parse_base();
    let mut doc = kalem_core::DocumentState::open(
        &path,
        std::sync::Arc::new(org_model::Settings::default()),
        &base,
    )
    .unwrap();
    let reg = kalem_core::CommandRegistry::with_builtins();
    let mut clip = kalem_core::command::Clipboard::default();
    let config = kalem_core::Config::from_layers(&[(
        kalem_core::settings::Layer::User,
        None,
        "export.open_after = true\n",
    )]);
    let mut ctx = kalem_core::command::EditorContext {
        document: Some(&mut doc),
        clipboard: &mut clip,
        config: &config,
        now: std::time::Instant::now(),
        clock: jiff::civil::date(2026, 9, 30).at(10, 0, 0, 0),
        messages: Vec::new(),
        requests: Vec::new(),
    };
    reg.execute("latex.export.html", &mut ctx, &serde_json::Value::Null)
        .unwrap();
    let done = kalem_core::jobs::wait_all();
    let mine: Vec<_> = done
        .iter()
        .filter(|f| f.message.contains("paper.html"))
        .collect();
    assert_eq!(mine.len(), 1, "{done:?}");
    assert!(!mine[0].error, "{}", mine[0].message);
    assert!(matches!(
        &mine[0].open,
        Some(kalem_core::input::LinkAction::Url(u)) if u.ends_with("paper.html")
    ));
    assert!(dir.join("paper.html").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}
