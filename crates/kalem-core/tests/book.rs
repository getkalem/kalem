//! The sample book of `examples/book` (§9.4, T2.7): exported to Word
//! through pandoc and to PDF through LaTeX when those tools are installed.
//! Its HTML, LaTeX, Markdown and text exports are compared with Emacs's in
//! `org-export`'s tests.

#![allow(clippy::print_stderr)]

use std::path::{Path, PathBuf};

fn book() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/book"))
}

/// A copy of the book in a folder of its own.
fn copy(to: &Path) {
    fn walk(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let p = e.unwrap().path();
            let t = to.join(p.file_name().unwrap());
            if p.is_dir() {
                walk(&p, &t);
            } else {
                std::fs::copy(&p, &t).unwrap();
            }
        }
    }
    let _ = std::fs::remove_dir_all(to);
    walk(&book(), to);
}

#[test]
fn book_to_word() {
    let search = std::env::var_os("PATH").unwrap_or_default();
    let Some(pandoc) = kalem_core::pandoc::find(&search) else {
        eprintln!("no pandoc: the Word export of the sample book is not checked");
        return;
    };
    let dir = std::env::temp_dir().join(format!("kalem-book-word-{}", std::process::id()));
    copy(&dir);
    let org = dir.join("book.org");
    let text = std::fs::read_to_string(&org).unwrap();
    let out = dir.join("book.docx");
    kalem_core::pandoc::export(&pandoc, &text, &org, kalem_core::pandoc::Format::Docx, &out)
        .unwrap();
    assert!(std::fs::read(&out).unwrap().starts_with(b"PK"));
    // Read back: citations rendered, references labelled, the equation
    // and the picture kept.
    let back = std::process::Command::new(&pandoc)
        .args(["--to", "plain"])
        .arg(&out)
        .output()
        .unwrap();
    let back = String::from_utf8_lossy(&back.stdout);
    for want in [
        "(Knuth 1984)",
        "as Figure 1 shows",
        "Table 1 lists",
        "equation\n(1)",
        "mc²",
        "Addison-Wesley",
        "Plain text also diffs well",
    ] {
        assert!(back.contains(want), "{want:?} not in:\n{back}");
    }
    let media = std::process::Command::new("unzip")
        .arg("-l")
        .arg(&out)
        .output();
    if let Ok(m) = media {
        assert!(String::from_utf8_lossy(&m.stdout).contains("media/"));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn book_to_pdf() {
    let search = std::env::var_os("PATH").unwrap_or_default();
    let engine = kalem_core::pdf::Engine::PdfLatex;
    let Some(tool) = kalem_core::pdf::detect(engine, &search) else {
        eprintln!("no TeX: the PDF export of the sample book is not checked");
        return;
    };
    let dir = std::env::temp_dir().join(format!("kalem-book-pdf-{}", std::process::id()));
    copy(&dir);
    let org = dir.join("book.org");
    let text = std::fs::read_to_string(&org).unwrap();
    let tex = org_export::export(
        &text,
        &org_export::Latex::default(),
        &org_export::Settings {
            input_file: Some(org.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let tex_path = dir.join("book.tex");
    std::fs::write(&tex_path, tex).unwrap();
    let compiled = kalem_core::pdf::compile(&tool, engine, &tex_path).unwrap();
    let errors: Vec<_> = compiled.problems.iter().filter(|p| p.error).collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(compiled.pdf.is_some_and(|p| p.is_file()));
    let _ = std::fs::remove_dir_all(&dir);
}
