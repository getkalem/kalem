//! Golden tests for the command line: output and exit codes.

use std::process::Command;

fn kalem(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_kalem"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run kalem");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

#[test]
fn version_and_help() {
    let (code, out, _) = kalem(&["--version"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("kalem "));
    let (code, out, _) = kalem(&["--help"]);
    assert_eq!(code, 0);
    assert!(out.contains("check"));
}

#[test]
fn editor_help_and_no_terminal() {
    // `kalem gui --help` prints the usage instead of opening a file named
    // `--help` (T1.8.12).
    for args in [
        ["gui", "--help"],
        ["gui", "-h"],
        ["tui", "--help"],
        ["tui", "-h"],
    ] {
        let (code, out, err) = kalem(&args);
        if out.is_empty() {
            // A build without that editor says so.
            assert_eq!(code, 2, "{args:?}: {err}");
        } else {
            assert_eq!(code, 0, "{args:?}");
            assert!(out.starts_with("Usage: kalem"), "{args:?}: {out}");
        }
    }
    // The terminal editor without a terminal says it needs one (the
    // test's output is a pipe).
    let (code, _, err) = kalem(&["tui", "--detect"]);
    assert_eq!(code, 2);
    assert!(err.contains("terminal"), "{err}");
}

#[test]
fn check_reports_diagnostics() {
    let (code, out, _) = kalem(&["check", "tests/fixtures/sample.org"]);
    assert_eq!(code, 0, "warnings alone do not fail");
    insta::assert_snapshot!("check_text", out);
    let (code, _, _) = kalem(&["check", "--deny-warnings", "tests/fixtures/sample.org"]);
    assert_eq!(code, 1);
    let (code, out, _) = kalem(&["check", "--deny-warnings", "tests/fixtures/clean.org"]);
    assert_eq!(code, 0);
    assert!(out.is_empty());
}

#[test]
fn check_json() {
    let (code, out, _) = kalem(&["check", "--format", "json", "tests/fixtures/sample.org"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    // Key order depends on serde_json's `preserve_order`, which gpui turns
    // on for the whole build.
    insta::with_settings!({ sort_maps => true }, {
        insta::assert_json_snapshot!("check_json", v);
    });
}

#[test]
fn parse_prints_the_tree() {
    let (code, out, _) = kalem(&["parse", "tests/fixtures/clean.org"]);
    assert_eq!(code, 0);
    insta::assert_snapshot!("parse_tree", out);
}

#[test]
fn dump_is_valid_json() {
    let (code, out, _) = kalem(&["dump", "tests/fixtures/clean.org"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["children"][0]["type"], "headline");
}

#[test]
fn missing_file_is_an_error() {
    let (code, _, err) = kalem(&["check", "tests/fixtures/does-not-exist.org"]);
    assert_eq!(code, 2);
    assert!(err.contains("does-not-exist.org"));
}

#[test]
fn bad_arguments_exit_with_2() {
    let (code, _, _) = kalem(&["no-such-command"]);
    assert_eq!(code, 2);
}

#[test]
fn kalem_markup_in_org() {
    let dir = std::env::temp_dir().join(format!("kalem-cli-markup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let body = "#+KALEM: size=12\nSome @@kalem:color=red@@red@@kalem:end@@ text.\n\n#+ATTR_KALEM: :align right\n| a | bb |\n| ccc |\n";
    // Formatting an earlier Kalem wrote into Org is listed (T2.13.13);
    // only `--deny-warnings` fails on it.
    let org = dir.join("doc.org");
    std::fs::write(&org, body).unwrap();
    {
        let f = &org;
        let (code, out, _) = kalem(&["check", "--deny-warnings", f.to_str().unwrap()]);
        assert_eq!(code, 1);
        assert_eq!(out.matches("kalem-markup-in-org").count(), 4, "{out}");
        let (code, _, _) = kalem(&["check", f.to_str().unwrap()]);
        assert_eq!(code, 0);
    }
    // `kalem export --to org` writes strict Org and says what went.
    let (code, out, err) = kalem(&["export", org.to_str().unwrap(), "--to", "org", "-o", "-"]);
    assert_eq!(code, 0);
    assert_eq!(out, "Some red text.\n\n| a | bb |\n| ccc |\n");
    assert!(
        err.contains("2 formatted spans, 1 paragraph attribute, 1 document option line"),
        "{err}"
    );
    // `kalem fmt` aligns the table and leaves the additions alone.
    let (code, _, _) = kalem(&["fmt", org.to_str().unwrap()]);
    assert_eq!(code, 0);
    let after = std::fs::read_to_string(&org).unwrap();
    assert_eq!(
        after,
        body.replace("| a | bb |\n| ccc |\n", "| a   | bb |\n| ccc |    |\n")
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn fmt_aligns_and_checks() {
    let dir = std::env::temp_dir().join(format!("kalem-cli-fmt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("t.org");
    std::fs::write(&file, "* A\n| a | bb |\n| ccc |\n").unwrap();
    let path = file.to_str().unwrap();
    let (code, out, _) = kalem(&["fmt", "--check", path]);
    assert_eq!(code, 1);
    assert_eq!(out.trim(), path);
    let (code, out, _) = kalem(&["fmt", path]);
    assert_eq!(code, 0);
    assert!(out.contains("formatted"));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "* A\n| a   | bb |\n| ccc |    |\n"
    );
    let (code, out, _) = kalem(&["fmt", "--check", path]);
    assert_eq!((code, out.as_str()), (0, ""));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn query_matches_headlines() {
    let (code, out, _) = kalem(&["query", "tests/fixtures/tasks.org", "TODO=\"NEXT\"+work"]);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "tests/fixtures/tasks.org:3: ** NEXT Write the report\n"
    );
    let (code, out, _) = kalem(&[
        "query",
        "tests/fixtures/tasks.org",
        "/NEXT",
        "--format",
        "json",
    ]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let titles: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Write the report", "Fix the door"]);
    assert_eq!(v[0]["tags"], serde_json::json!(["work"]));
}

#[test]
fn export_writes_html_and_markdown() {
    let dir = std::env::temp_dir().join(format!("kalem-cli-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("notes.org");
    std::fs::write(&file, "#+TITLE: Notes\n* One\nSome *bold* text.\n").unwrap();
    let f = file.to_str().unwrap();
    let (code, out, err) = kalem(&["export", f]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.trim(), dir.join("notes.html").display().to_string());
    let html = std::fs::read_to_string(dir.join("notes.html")).unwrap();
    assert!(
        html.contains("<title>Notes</title>") && html.contains("<b>bold</b>"),
        "{html}"
    );
    let (code, out, _) = kalem(&["export", "--to", "md", "--body-only", "-o", "-", f]);
    assert_eq!(code, 0);
    assert!(out.contains("# One") && out.contains("**bold**"), "{out}");
    // A file that cannot be exported: an error, and status 1.
    std::fs::write(dir.join("bad.org"), "{{{undefined}}}\n").unwrap();
    let (code, _, err) = kalem(&["export", dir.join("bad.org").to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(err.contains("undefined"), "{err}");
}

/// `kalem export --to pdf` with a stand-in for `pdflatex` that writes a
/// PDF, and a log with an error at the `.tex` line of the second
/// paragraph: the error is given at its Org line.
#[cfg(unix)]
#[test]
fn pdf_export() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("kalem-cli-pdf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    // Shell built-ins only: `PATH` has nothing else.
    let script = "#!/bin/sh\nfor a; do f=$a; done\nb=${f%.tex}\nprintf '%%PDF-1.4\\n' > $b.pdf\nn=0\nwhile read -r line; do n=$((n+1)); case $line in *Second*) m=$n;; esac; done < $f\nprintf './%s.tex:%s: Undefined control sequence.\\n' $b $m > $b.log\n";
    let engine = bin.join("pdflatex");
    std::fs::write(&engine, script).unwrap();
    std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
    let org = dir.join("doc.org");
    std::fs::write(&org, "#+TITLE: T\n\nFirst.\n\nSecond \\foo.\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_kalem"))
        .args(["export", "--to", "pdf", org.to_str().unwrap()])
        .env("PATH", &bin)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(
        err.contains(&format!(
            "{}:5: error: Undefined control sequence.",
            org.display()
        )),
        "{err}"
    );
    assert!(dir.join("doc.pdf").is_file());
    let tex = std::fs::read_to_string(dir.join("doc.tex")).unwrap();
    assert!(tex.contains("%% org:5\nSecond"), "{tex}");
    // Without LaTeX: guidance.
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_kalem"))
        .args(["export", "--to", "pdf", org.to_str().unwrap()])
        .env("PATH", &empty)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("No LaTeX found"));
}

/// Word and HTML through pandoc, when it is installed.
#[test]
fn pandoc_bridge() {
    let found = std::process::Command::new("pandoc")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !found {
        return;
    }
    let dir = std::env::temp_dir().join(format!("kalem-cli-pandoc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let org = dir.join("doc.org");
    std::fs::write(&org, "#+TITLE: Doc\n\n* Part\nText with /emphasis/.\n").unwrap();
    let (code, out, err) = kalem(&["export", "--to", "docx", org.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert!(out.trim().ends_with("doc.docx"));
    let (code, back, err) = kalem(&["import", dir.join("doc.docx").to_str().unwrap(), "-o", "-"]);
    assert_eq!(code, 0, "{err}");
    assert!(back.contains("* Part\nText with /emphasis/."), "{back}");
    // HTML beside: `page.org`, cleaned up.
    let html = dir.join("page.html");
    std::fs::write(
        &html,
        "<h1 id=\"a\">A</h1><p>x <a href=\"https://k.l\">https://k.l</a></p>",
    )
    .unwrap();
    let (code, _, err) = kalem(&["import", html.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    let page = std::fs::read_to_string(dir.join("page.org")).unwrap();
    assert!(page.contains("* A\nx [[https://k.l]]"), "{page}");
    // Not over an existing file.
    let (code, _, err) = kalem(&["import", html.to_str().unwrap()]);
    assert_eq!(code, 1, "{err}");
}

/// `kalem check` reads the bibliography and warns about unknown keys.
#[test]
fn citation_checks() {
    let dir = std::env::temp_dir().join(format!("kalem-cli-cite-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("refs.bib"),
        "@book{knuth84, title = {The {\\TeX}book}, year = 1984}\n",
    )
    .unwrap();
    let org = dir.join("doc.org");
    std::fs::write(
        &org,
        "#+bibliography: refs.bib\n#+bibliography: gone.bib\n\nSee [cite:@knuth84; @missing].\n",
    )
    .unwrap();
    let (code, out, _) = kalem(&["check", org.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(
        out.contains(":2:1: warning[bibliography-unreadable]"),
        "{out}"
    );
    assert!(
        out.contains(":4:5: warning[cite-unknown-key]: No bibliography has the key @missing"),
        "{out}"
    );
    assert!(!out.contains("knuth84"), "{out}");
}

#[test]
fn check_latex() {
    let (code, out, _) = kalem(&["check", "--unrendered", "tests/fixtures/sample.tex"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        out,
        "tests/fixtures/sample.tex:4:17: warning[latex-undefined-reference]: No label b\n\
         tests/fixtures/sample.tex:4:27: info[latex-deprecated]: \\bf is deprecated in LaTeX 2ε; use the \\text… command or the declaration (\\bfseries, \\itshape)\n\
         tests/fixtures/sample.tex:5:1: warning[latex-syntax]: \\begin{itemize} is not closed\n\
         tests/fixtures/sample.tex: rendered: 89.3%\n\
         tests/fixtures/sample.tex: unrendered: \\tikzset (1)\n"
    );
    let (code, _, _) = kalem(&["check", "--deny-warnings", "tests/fixtures/sample.tex"]);
    assert_eq!(code, 1);
}

#[test]
fn latex_build() {
    let search = std::env::var_os("PATH").unwrap_or_default();
    if !std::env::split_paths(&search).any(|d| d.join("pdflatex").is_file()) {
        return;
    }
    let dir = std::env::temp_dir().join(format!("kalem-cli-latex-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("ch")).unwrap();
    std::fs::write(
        dir.join("main.tex"),
        "\\documentclass{article}\\begin{document}\\input{ch/one}\\end{document}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("ch/one.tex"),
        "% !TEX root = ../main.tex\nHello \\undefinedcommand.\n",
    )
    .unwrap();
    let one = dir.join("ch/one.tex");
    let (code, out, _) = kalem(&["latex", "build", "--format", "json", one.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["root"].as_str().unwrap().ends_with("main.tex"));
    let first = &v["problems"][0];
    assert_eq!(
        (first["file"].as_str(), first["line"].as_u64()),
        (Some("./ch/one.tex"), Some(2))
    );
    assert_eq!(first["severity"], "error");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fmt_latex() {
    let dir = std::env::temp_dir().join(format!("kalem-cli-fmt-tex-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("a.tex");
    std::fs::write(
        &f,
        "\\begin{itemize}\n  \\item a   \n\\item b\n\\end{itemize}\n\n\n\nx\n",
    )
    .unwrap();
    let path = f.to_str().unwrap();
    let (code, out, _) = kalem(&["fmt", "--check", path]);
    assert_eq!((code, out.trim()), (1, path));
    let (code, _, _) = kalem(&["fmt", path]);
    assert_eq!(code, 0);
    assert_eq!(
        std::fs::read_to_string(&f).unwrap(),
        "\\begin{itemize}\n  \\item a\n  \\item b\n\\end{itemize}\n\nx\n"
    );
    let (code, _, _) = kalem(&["fmt", "--check", path]);
    assert_eq!(code, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn diff_pandoc() {
    let search = std::env::var_os("PATH").unwrap_or_default();
    if !std::env::split_paths(&search).any(|d| d.join("pandoc").is_file()) {
        return;
    }
    let (code, out, _) = kalem(&["diff-pandoc", "../../tests/latex/model/book.tex"]);
    assert_eq!(code, 0);
    assert_eq!(out, "", "every category agrees on the book");
    let (code, out, _) = kalem(&["diff-pandoc", "../../tests/latex/model/article.tex"]);
    assert_eq!(code, 0);
    // The one deliberate difference (book/part-2/latex-known-differences.org).
    assert_eq!(
        out.trim(),
        "../../tests/latex/model/article.tex: figures: kalem 2, pandoc 1"
    );
    // Counted as a reader reads: a subfigure in its figure, no `\nocite`.
    let (code, out, _) = kalem(&[
        "diff-pandoc",
        "../../tests/latex/model/subfigures.tex",
        "../../tests/latex/model/caption-above.tex",
        "../../tests/latex/citations/plain.tex",
    ]);
    assert_eq!((code, out.as_str()), (0, ""));
    // A file that is not UTF-8 is left out, not the end of the run.
    let dir = std::env::temp_dir().join(format!("kalem-pandoc-utf16-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bad = dir.join("utf16.tex");
    std::fs::write(&bad, [0xff, 0xfe, b'x', 0, 0xd8, 0x00]).unwrap();
    let (code, out, _) = kalem(&[
        "diff-pandoc",
        "--summary",
        bad.to_str().unwrap(),
        "../../tests/latex/model/book.tex",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("headings: 1 of 1 files agree"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_book_builds() {
    // Every chapter exports and every link inside the Book leads to a page.
    let book = concat!(env!("CARGO_MANIFEST_DIR"), "/../../book");
    let (code, out, err) = kalem(&["book", "check", book]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("0 problems"), "{out}");
    // The site: pages, the theme, the search index.
    let site = std::env::temp_dir().join(format!("kalem-book-{}", std::process::id()));
    let (code, out, err) = kalem(&["book", "build", book, "--out", site.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}{err}");
    for f in [
        "index.html",
        "part-1/installing.html",
        "part-2/latex.html",
        "appendices/commands.html",
        "theme/book.css",
        "search-index.js",
    ] {
        assert!(site.join(f).is_file(), "{f}");
    }
    let index = std::fs::read_to_string(site.join("search-index.js")).unwrap();
    assert!(index.contains("bib.sortView"));
    let _ = std::fs::remove_dir_all(&site);
}

#[test]
fn check_csv() {
    // A CSV file is checked as CSV: its malformed fields, not Org syntax.
    let (code, out, _) = kalem(&["check", "--deny-warnings", "tests/fixtures/malformed.csv"]);
    assert_eq!(code, 1);
    assert_eq!(
        out.lines()
            .map(|l| l.split(": ").next().unwrap_or(""))
            .collect::<Vec<_>>(),
        [
            "tests/fixtures/malformed.csv:2:8",
            "tests/fixtures/malformed.csv:3:1"
        ],
        "{out}"
    );
    assert!(
        out.contains("warning[csv-bare-quote]") && out.contains("warning[csv-unterminated-quote]"),
        "{out}"
    );
}

/// Markdown exports to HTML as Markdown (it was read as Org, with a
/// warning); to another format it is refused, not exported as Org.
#[test]
fn export_reads_markdown_as_markdown() {
    let dir = std::env::temp_dir().join(format!("kalem-cli-mdx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let md = dir.join("notes.md");
    std::fs::write(&md, "# Some Title\n\n- one\n").unwrap();
    let (code, out, err) = kalem(&["export", "--to", "html", "-o", "-", md.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("<h1>Some Title</h1>") && out.contains("<li>one</li>"),
        "{out}"
    );
    assert!(err.is_empty(), "{err}");
    let (code, _, err) = kalem(&["export", "--to", "latex", "-o", "-", md.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(err.contains("not exported"), "{err}");
    let org = dir.join("notes.org");
    std::fs::write(&org, "* Title\n").unwrap();
    let (_, _, err) = kalem(&["export", "--to", "html", "-o", "-", org.to_str().unwrap()]);
    assert!(err.is_empty(), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn markdown_exports_to_org() {
    // `kalem export FILE.md --to org` without pandoc (T2.7c.7).
    let dir = std::env::temp_dir().join(format!("kalem-cli-md-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let md = dir.join("README.md");
    std::fs::write(&md, "# Title\n\nSome *text* and `code`.\n\n- [ ] task\n").unwrap();
    let (code, out, err) = kalem(&["export", md.to_str().unwrap(), "--to", "org", "-o", "-"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, "* Title\n\nSome /text/ and ~code~.\n\n- [ ] task\n");
    assert!(err.is_empty(), "{err}");
    let (code, out, _) = kalem(&["export", md.to_str().unwrap(), "--to", "org"]);
    assert_eq!(code, 0);
    assert!(out.trim().ends_with("README.org"), "{out}");
    assert!(dir.join("README.org").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn parse_and_check_by_kind_and_folder() {
    let dir = std::env::temp_dir().join(format!("kalem-cli-folder-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::create_dir_all(dir.join(".hidden")).unwrap();
    std::fs::write(
        dir.join("a.md"),
        "# Hi\n\nSee [b](sub/b.org) and [x](gone.md).\n",
    )
    .unwrap();
    std::fs::write(dir.join("sub/b.org"), "* B\n").unwrap();
    std::fs::write(dir.join(".hidden/c.org"), "* C\n").unwrap();
    std::fs::write(dir.join("d.csv"), "a,b\n").unwrap();
    let p = |n: &str| dir.join(n).display().to_string();
    // A Markdown file is parsed as Markdown, not as Org.
    let (code, out, _) = kalem(&["parse", &p("a.md")]);
    assert_eq!(code, 0);
    assert!(
        out.contains("Heading { level: 1") && out.contains("Link { url: \"gone.md\" }"),
        "{out}"
    );
    let (code, _, err) = kalem(&["parse", &p("d.csv")]);
    assert_eq!(code, 2);
    assert!(err.contains("not CSV"), "{err}");
    // A folder stands for its files; links to missing files are reported.
    let (code, out, _) = kalem(&["check", "--format", "json", &dir.display().to_string()]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let files: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["file"].as_str().unwrap())
        .collect();
    assert_eq!(files.len(), 3, "{files:?}");
    assert!(!files.iter().any(|f| f.contains(".hidden")));
    let md = v
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["file"].as_str().unwrap().ends_with("a.md"))
        .unwrap();
    let codes: Vec<&str> = md["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["markdown-missing-file"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn org_footnotes_and_files_not_there() {
    let dir = std::env::temp_dir().join(format!("kalem-cli-org-lint-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("doc.org");
    std::fs::write(
        &f,
        "#+INCLUDE: \"gone.org\"\nSee [fn:9] and [[file:gone.png]].\n\n[fn:1] One.\n\n[fn:1] Two.\n",
    )
    .unwrap();
    let f = f.display().to_string();
    let (code, out, _) = kalem(&["check", "--format", "json", &f]);
    assert_eq!(code, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let codes: Vec<&str> = v[0]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    assert_eq!(
        codes,
        [
            "missing-include-file",
            "undefined-footnote-reference",
            "missing-linked-file",
            "duplicate-footnote-definition",
            "duplicate-footnote-definition"
        ]
    );
    // The export stops there, as Emacs's does.
    std::fs::write(dir.join("doc.org"), "See [fn:9].\n").unwrap();
    let (code, _, err) = kalem(&["export", "--to", "md", "-o", "-", &f]);
    assert_ne!(code, 0);
    assert!(err.contains("Definition not found for footnote 9"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn folders_for_fmt_export_and_query() {
    let dir = std::env::temp_dir().join(format!("kalem-cli-folders-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::create_dir_all(dir.join(".hidden")).unwrap();
    std::fs::write(dir.join("a.org"), "* TODO A :work:\n| a | bb |\n|-|-|\n").unwrap();
    std::fs::write(dir.join("sub/b.org"), "* B :work:\n").unwrap();
    std::fs::write(dir.join(".hidden/c.org"), "* C :work:\n").unwrap();
    std::fs::write(dir.join("r.md"), "# R\n").unwrap();
    let d = dir.display().to_string();
    // A folder stands for its Org files (and LaTeX's, for fmt).
    let (code, out, _) = kalem(&["fmt", "--check", &d]);
    assert_eq!(code, 1);
    assert_eq!(out.lines().count(), 2, "{out}");
    assert!(!out.contains(".hidden") && !out.contains("r.md"), "{out}");
    let (code, out, _) = kalem(&["query", &d, "work"]);
    assert_eq!(code, 0);
    assert_eq!(out.lines().count(), 2, "{out}");
    assert!(!out.contains(".hidden"), "{out}");
    let (code, _, _) = kalem(&["export", "--to", "md", &d]);
    assert_eq!(code, 0);
    assert!(dir.join("a.md").exists() && dir.join("sub/b.md").exists());
    let (code, _, err) = kalem(&["export", "--to", "md", "-o", "-", &d]);
    assert_eq!(code, 2);
    assert!(err.contains("--output takes one input file"), "{err}");
    // What the match string has that is no term is said, and left out
    // as Emacs leaves it out.
    let (code, out, err) = kalem(&["query", &d, "work=\""]);
    assert_eq!(code, 0);
    assert_eq!(out.lines().count(), 2, "{out}");
    assert!(
        err.contains("\"=\\\"\" in the match string is no term"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn latex_coverage_by_field() {
    let dir = std::env::temp_dir().join(format!("kalem-cli-coverage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (paper, body) in [
        (
            "math/p1",
            "\\documentclass{article}\n\\begin{document}\n\\section{A} Text $x+y$.\n\\foo{bar}\n\\end{document}\n",
        ),
        (
            "cs/p2",
            "\\documentclass{article}\n\\begin{document}\nPlain \\emph{words}.\n\\end{document}\n",
        ),
    ] {
        std::fs::create_dir_all(dir.join(paper)).unwrap();
        std::fs::write(dir.join(paper).join("main.tex"), body).unwrap();
    }
    let (code, out, _) = kalem(&[
        "latex-coverage",
        "--format",
        "json",
        &dir.display().to_string(),
    ]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["total"]["papers"], 2);
    assert!(v["fields"]["math"]["source_bytes"].as_u64().unwrap() > 0);
    assert_eq!(v["fields"]["cs"]["source_bytes"], 0);
    assert!(v["fields"]["math"]["math_bytes"].as_u64().unwrap() > 0);
    let names: Vec<&str> = v["most_frequent_unrendered"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["\\foo"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_plugin_turned_off_is_listed_and_enabled_again() {
    // Kalem turned org.example.stops off after three stops (wasm_todo W8):
    // `kalem plugin list` says so of the installed plugin, and `kalem
    // plugin enable` forgets the stops.
    let dir = std::env::temp_dir().join(format!("kalem-cli-turned-off-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let plugin = dir.join("config/plugins/stops");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(
        plugin.join("plugin.json"),
        r#"{"id": "org.example.stops", "name": "Stops", "version": "1.0.0"}"#,
    )
    .unwrap();
    // In English, whatever the system's language.
    std::fs::write(
        dir.join("config/settings.toml"),
        "[ui]\nlanguage = \"en\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("state")).unwrap();
    std::fs::write(
        dir.join("state/plugin-failures.json"),
        r#"{"org.example.stops": {"version": "1.0.0", "count": 3, "last": "trap"}}"#,
    )
    .unwrap();
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_kalem"))
            .args(args)
            .env("KALEM_CONFIG_DIR", dir.join("config"))
            .env("KALEM_STATE_DIR", dir.join("state"))
            .env("LANG", "en_US.UTF-8")
            .output()
            .expect("run kalem");
        assert!(out.status.success(), "{args:?}");
        String::from_utf8(out.stdout).unwrap()
    };
    let list = run(&["plugin", "list"]);
    assert!(
        list.contains("turned off: stopped 3 times (kalem plugin enable org.example.stops)"),
        "{list}"
    );
    assert!(
        run(&["plugin", "enable", "org.example.stops"]).contains("org.example.stops is on again")
    );
    assert!(!run(&["plugin", "list"]).contains("turned off"));
    assert!(
        run(&["plugin", "enable", "org.example.stops"])
            .contains("org.example.stops was not turned off")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_build_that_cannot_start_says_so_in_json() {
    // `--engine tectonic` where Tectonic is not installed: the build does
    // not start, and `--format json` says why as JSON (it printed text).
    let path = std::env::var_os("PATH").unwrap_or_default();
    if std::env::split_paths(&path).any(|d| d.join("tectonic").is_file()) {
        return;
    }
    let dir = std::env::temp_dir().join(format!("kalem-cli-no-tectonic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.tex");
    std::fs::write(
        &file,
        "\\documentclass{article}\n\\begin{document}\nx\n\\end{document}\n",
    )
    .unwrap();
    let (code, out, _) = kalem(&[
        "latex",
        "build",
        "--format",
        "json",
        "--engine",
        "tectonic",
        file.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    let v: serde_json::Value = serde_json::from_str(out.trim()).expect(&out);
    assert!(v["pdf"].is_null(), "{v}");
    assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()), "{v}");
    let _ = std::fs::remove_dir_all(&dir);
}
