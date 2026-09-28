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
