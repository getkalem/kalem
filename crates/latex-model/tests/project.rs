//! Projects of several files: `tests/latex/project` numbered as pdflatex
//! numbers it (`main.labels`, from the `.aux` files of `main.tex`), and
//! the root document found from each file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use latex_model::project::{Disk, Files, ProjectCache, find_root};

fn dir() -> PathBuf {
    std::fs::canonicalize(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/latex/project"
    ))
    .unwrap()
}

#[test]
fn numbered_across_files() {
    let root = dir().join("main.tex");
    let p = ProjectCache::default().load(&root, &Disk);
    let m = &p.model;
    let expected = std::fs::read_to_string(dir().join("main.labels")).unwrap();
    for line in expected.lines() {
        let (key, number) = line.split_once(' ').unwrap();
        let got = m.label(key).and_then(|l| l.number.clone());
        assert_eq!(got.as_deref(), Some(number), "{key}");
    }
    let files: Vec<String> = m
        .files
        .iter()
        .map(|f| {
            let rel = f.strip_prefix(dir()).unwrap();
            let parts: Vec<_> = rel.iter().map(|c| c.to_string_lossy()).collect();
            parts.join("/")
        })
        .collect();
    assert_eq!(
        files,
        [
            "main.tex",
            "chapters/one.tex",
            "chapters/two.tex",
            "chapters/three.tex",
            "parts/sub.tex",
            "parts/deep/leaf.tex",
            "parts/imported.tex",
            "parts/deep/leaf2.tex",
        ]
    );
    // Each label knows its file; the subfile's preamble is not the
    // document's.
    let leaf = m.label("sec:leaf").unwrap();
    assert!(m.files[leaf.file].ends_with("parts/deep/leaf.tex"));
    assert_eq!(m.class.as_ref().unwrap().name, "report");
    assert_eq!(m.graphics_paths, ["figs/", "images/"]);
    assert!(m.includes.iter().all(|i| i.resolved.is_some()));
    assert_eq!(m.references.len(), 2);
}

#[test]
fn roots() {
    let root = dir().join("main.tex");
    let read = |p: &Path| std::fs::read_to_string(p).unwrap();
    for f in [
        "main.tex",
        "chapters/one.tex",
        "chapters/two.tex",
        "parts/sub.tex",
    ] {
        let file = dir().join(f);
        assert_eq!(
            find_root(&file, &read(&file), &Disk, None, Some(&dir())),
            root,
            "{f}"
        );
    }
    // A setting names it for files that do not say.
    let file = dir().join("chapters/two.tex");
    let other = dir().join("other.tex");
    assert_eq!(
        find_root(&file, &read(&file), &Disk, Some(&other), Some(&dir())),
        other
    );
}

/// Files in memory.
#[derive(Debug, Default)]
struct Memory(HashMap<PathBuf, String>);

impl Files for Memory {
    fn read(&self, path: &Path) -> Option<String> {
        self.0.get(path).cloned()
    }
    fn list(&self, dir: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self
            .0
            .keys()
            .filter(|p| p.parent() == Some(dir))
            .cloned()
            .collect();
        v.sort();
        v
    }
}

#[test]
fn includeonly_cycles_and_markers() {
    let mut fs = Memory::default();
    let put = |fs: &mut Memory, p: &str, t: &str| {
        fs.0.insert(PathBuf::from(p), t.to_string());
    };
    put(
        &mut fs,
        "/p/main.tex",
        "\\documentclass{book}\\includeonly{b}\\begin{document}\\include{a}\\include{b}\\input{loop}\\end{document}",
    );
    put(&mut fs, "/p/a.tex", "\\chapter{A}\\label{a}");
    put(&mut fs, "/p/b.tex", "\\chapter{B}\\label{b}");
    put(&mut fs, "/p/loop.tex", "\\input{loop2}");
    put(
        &mut fs,
        "/p/loop2.tex",
        "\\input{loop}\\chapter{C}\\label{c}",
    );
    let p = ProjectCache::default().load(Path::new("/p/main.tex"), &fs);
    let m = &p.model;
    // Left out by `\includeonly`, still numbered (from its `.aux`).
    assert!(m.includes[0].excluded && !m.includes[1].excluded);
    assert_eq!(m.label("b").unwrap().number.as_deref(), Some("2"));
    // The cycle is read once.
    assert_eq!(m.label("c").unwrap().number.as_deref(), Some("3"));
    assert_eq!(
        m.includes.iter().filter(|i| i.resolved.is_none()).count(),
        1
    );
    // `NAME.tex.latexmain` marks the root.
    put(&mut fs, "/q/thesis.tex.latexmain", "");
    put(&mut fs, "/q/ch/x.tex", "\\section{X}");
    assert_eq!(
        find_root(Path::new("/q/ch/x.tex"), "\\section{X}", &fs, None, None),
        PathBuf::from("/q/thesis.tex")
    );
}

#[test]
fn a_package_beside_the_document() {
    // `\usepackage{mymacros}` with `mymacros.sty` beside the document:
    // its theorems and commands are the document's; a package of TeX's
    // own is not read, and neither is listed as an included file.
    let mut fs = Memory::default();
    fs.0.insert(
        PathBuf::from("/p/main.tex"),
        "\\documentclass{article}\\usepackage{amsmath,mymacros}\\begin{document}\n\
         \\begin{proposition}\\label{p}A\\end{proposition}\n\\end{document}\n"
            .to_string(),
    );
    fs.0.insert(
        PathBuf::from("/p/mymacros.sty"),
        "\\ProvidesPackage{mymacros}\n\\newtheorem{proposition}{Proposition}\n\\newcommand{\\R}{\\mathbb{R}}\n"
            .to_string(),
    );
    let p = ProjectCache::default().load(Path::new("/p/main.tex"), &fs);
    let m = &p.model;
    assert!(m.theorem_kinds.iter().any(|k| k.env == "proposition"));
    assert_eq!(m.label("p").unwrap().number.as_deref(), Some("1"));
    assert!(m.macros.iter().any(|x| x.name == "\\R"));
    assert!(m.includes.is_empty());
}

/// A file in another encoding than UTF-8 (`\usepackage[latin1]{inputenc}`)
/// is read: its commands, its theorems with them.
#[test]
fn files_not_in_utf8() {
    let dir = std::env::temp_dir().join(format!("kalem-latin1-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let root = dir.join("main.tex");
    let mut bytes = b"\\documentclass{article}\n\\usepackage[latin1]{inputenc}\n\\newtheorem{definition}{Definition}\n\\begin{document}\nCaf".to_vec();
    bytes.push(0xe9);
    bytes.extend_from_slice(b".\n\\begin{definition}A\\end{definition}\n\\end{document}\n");
    std::fs::write(&root, bytes).unwrap();
    let text = latex_model::project::Files::read(&latex_model::project::Disk, &root).unwrap();
    assert!(text.contains("Caf\u{fffd}."));
    let project =
        latex_model::project::ProjectCache::default().load(&root, &latex_model::project::Disk);
    assert_eq!(project.model.theorems.len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}
