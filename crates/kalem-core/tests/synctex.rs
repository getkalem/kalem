//! SyncTeX against the `synctex` program of TeX Live (T2.7h.24): a
//! document compiled with `pdflatex -synctex=1`, then each source line
//! looked up both ways by Kalem and by `synctex view` and `synctex edit`.
//! Skipped when pdflatex or synctex is not installed.

#![allow(clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::process::Command;

use kalem_core::synctex::Synctex;

fn have(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success() || !o.stdout.is_empty())
}

/// The fields `synctex` prints, `Name:value`, of its first answer.
fn fields(out: &str) -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    for l in out.lines() {
        if let Some((k, v)) = l.split_once(':')
            && !m.contains_key(k)
        {
            m.insert(k.to_string(), v.to_string());
        }
    }
    m
}

fn compiled() -> Option<(PathBuf, PathBuf)> {
    if !have("pdflatex") || !have("synctex") {
        return None;
    }
    let dir = std::env::temp_dir().join(format!("kalem-synctex-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut main = String::from("\\documentclass{article}\n\\begin{document}\n");
    for s in 0..6 {
        main.push_str(&format!("\\section{{Section {s}}}\n"));
        for p in 0..4 {
            for l in 0..3 {
                main.push_str(&format!(
                    "Words of paragraph {p} in section {s}, line {l}, long enough to fill some of the measure.\n"
                ));
            }
            main.push('\n');
        }
        if s == 2 {
            main.push_str("\\input{part}\n\n");
        }
    }
    main.push_str("\\end{document}\n");
    std::fs::write(dir.join("main.tex"), main).unwrap();
    std::fs::write(
        dir.join("part.tex"),
        "Text of the included file,\non two lines of source.\n",
    )
    .unwrap();
    let ok = Command::new("pdflatex")
        .args(["-synctex=1", "-interaction=batchmode", "main.tex"])
        .current_dir(&dir)
        .output()
        .is_ok_and(|o| o.status.success());
    ok.then(|| (dir.join("main.tex"), dir.join("main.pdf")))
}

#[test]
fn synctex_agrees_with_texs_program() {
    let Some((main, pdf)) = compiled() else {
        eprintln!("pdflatex or synctex not installed: skipped");
        return;
    };
    let st = Synctex::load(&Synctex::for_pdf(&pdf).expect("written")).unwrap();
    let text = std::fs::read_to_string(&main).unwrap();
    let dir = main.parent().unwrap();
    let (mut forward, mut forward_ok) = (0, 0);
    let mut points: Vec<(usize, f64, f64)> = Vec::new();
    let mut lines: Vec<(&Path, usize)> = Vec::new();
    for (i, l) in text.lines().enumerate() {
        if l.starts_with("Words") || l.starts_with("\\section") {
            lines.push((main.as_path(), i + 1));
        }
    }
    let part = dir.join("part.tex");
    lines.push((part.as_path(), 1));
    for (file, line) in lines {
        let out = Command::new("synctex")
            .args([
                "view",
                "-i",
                &format!("{line}:0:{}", file.display()),
                "-o",
                &pdf.display().to_string(),
            ])
            .output()
            .unwrap();
        let theirs = fields(&String::from_utf8_lossy(&out.stdout));
        let Some(page) = theirs.get("Page").and_then(|p| p.parse::<usize>().ok()) else {
            continue;
        };
        forward += 1;
        let ours = st.forward(file, line);
        let v: f64 = theirs["v"].parse().unwrap();
        let h: f64 = theirs["H"].parse().unwrap();
        // The same page, and their box inside ours.
        if let Some(p) = &ours
            && p.page == page
            && p.y <= v - h + 0.01
            && v <= p.y + p.height + 0.01
        {
            forward_ok += 1;
        } else {
            eprintln!(
                "forward {}:{line}: synctex page {page} v {v}, Kalem {ours:?}",
                file.display()
            );
        }
        // A point in the middle of their box, for the way back.
        let x: f64 = theirs["h"].parse::<f64>().unwrap() + 20.0;
        points.push((page, x, v - h / 2.0));
    }
    let (mut inverse, mut inverse_ok) = (0, 0);
    for (page, x, y) in points {
        let out = Command::new("synctex")
            .args(["edit", "-o", &format!("{page}:{x}:{y}:{}", pdf.display())])
            .output()
            .unwrap();
        let theirs = fields(&String::from_utf8_lossy(&out.stdout));
        let (Some(input), Some(line)) = (theirs.get("Input"), theirs.get("Line")) else {
            continue;
        };
        inverse += 1;
        let line: usize = line.parse().unwrap();
        let ours = st.inverse(page, x, y);
        if ours
            .as_ref()
            .is_some_and(|(f, l)| f == Path::new(input) && *l == line)
        {
            inverse_ok += 1;
        } else {
            eprintln!("inverse {page}:{x}:{y}: synctex {input}:{line}, Kalem {ours:?}");
        }
    }
    eprintln!("forward {forward_ok}/{forward}, inverse {inverse_ok}/{inverse}");
    assert!(forward > 20 && inverse > 20);
    assert_eq!(forward_ok, forward);
    assert_eq!(inverse_ok, inverse);
}
