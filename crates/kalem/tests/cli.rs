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
fn kalem_markup_and_file_kinds() {
    let dir = std::env::temp_dir().join(format!("kalem-cli-kinds-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let body = "#+KALEM: size=12\nSome @@kalem:color=red@@red@@kalem:end@@ text.\n\n#+ATTR_KALEM: :align right\n| a | bb |\n| ccc |\n";
    // In a .klm file the additions are fine; in a .org file they warn.
    let klm = dir.join("doc.klm");
    std::fs::write(&klm, body).unwrap();
    let (code, out, _) = kalem(&["check", "--deny-warnings", klm.to_str().unwrap()]);
    assert_eq!((code, out.as_str()), (0, ""));
    let org = dir.join("doc.org");
    std::fs::write(&org, body).unwrap();
    let (code, out, _) = kalem(&["check", "--deny-warnings", org.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert_eq!(out.matches("kalem-markup-in-org").count(), 4, "{out}");
    let (code, _, _) = kalem(&["check", org.to_str().unwrap()]);
    assert_eq!(code, 0);
    // Opted in: no warnings.
    std::fs::write(&org, format!("#+KALEM: markup=yes\n{body}")).unwrap();
    let (code, out, _) = kalem(&["check", "--deny-warnings", org.to_str().unwrap()]);
    assert_eq!((code, out.as_str()), (0, ""), "{out}");
    // `kalem export --to org` writes strict Org and says what went.
    let (code, out, err) = kalem(&["export", klm.to_str().unwrap(), "--to", "org", "-o", "-"]);
    assert_eq!(code, 0);
    assert_eq!(out, "Some red text.\n\n| a | bb |\n| ccc |\n");
    assert!(
        err.contains("2 formatted spans, 1 paragraph attribute, 1 document option line"),
        "{err}"
    );
    // `kalem fmt` aligns the table and leaves the additions alone.
    let (code, _, _) = kalem(&["fmt", klm.to_str().unwrap()]);
    assert_eq!(code, 0);
    let after = std::fs::read_to_string(&klm).unwrap();
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
         tests/fixtures/sample.tex: unrendered: \\bf (1)\n\
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
