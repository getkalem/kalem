//! Every extension point of the design document (§11.10 to §11.13) is in
//! the WIT definition, or named with the open task that will put it there.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The points of `coverage.toml`: name, interface or planned task.
fn coverage() -> Vec<(String, Option<String>, Option<String>)> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("coverage.toml"))
        .unwrap();
    let doc: toml_edit::DocumentMut = text.parse().unwrap();
    doc["point"]
        .as_array_of_tables()
        .expect("[[point]] tables")
        .iter()
        .map(|t| {
            let s = |k: &str| t.get(k).and_then(|v| v.as_str()).map(str::to_string);
            (s("name").expect("a name"), s("interface"), s("planned"))
        })
        .collect()
}

/// The interfaces the WIT files define.
fn interfaces() -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("wit");
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let text = std::fs::read_to_string(e.path()).unwrap();
        for line in text.lines() {
            if let Some(rest) = line.trim_start().strip_prefix("interface ") {
                out.push(rest.trim_end_matches(['{', ' ']).to_string());
            }
        }
    }
    out
}

#[test]
fn every_point_is_defined_or_planned() {
    let wit = interfaces();
    let todo = std::fs::read_to_string(root().join("docs/todo.md")).unwrap();
    for (name, interface, planned) in coverage() {
        match (interface, planned) {
            (Some(i), None) => assert!(wit.contains(&i), "{name}: no interface `{i}` in wit/"),
            (None, Some(t)) => assert!(
                todo.contains(&format!("- [ ] {t} ")) || todo.contains(&format!("- [~] {t} ")),
                "{name}: {t} is not an open task of docs/todo.md"
            ),
            _ => panic!("{name}: one of `interface` or `planned`"),
        }
    }
}

#[test]
fn every_point_of_the_design_is_listed() {
    let design = std::fs::read_to_string(root().join("docs/design_document.md")).unwrap();
    let start = design.find("### 11.10 Extension points").expect("§11.10");
    let end = design[start..]
        .find("**Example")
        .map(|i| start + i)
        .unwrap();
    let listed: Vec<String> = coverage().into_iter().map(|(n, _, _)| n).collect();
    let rows: Vec<&str> = design[start..end]
        .lines()
        .filter_map(|l| l.strip_prefix("| "))
        .map(|l| l.split(" |").next().unwrap_or("").trim())
        .filter(|n| !n.is_empty() && *n != "Extension point" && !n.starts_with("---"))
        .collect();
    assert!(rows.len() >= 18, "the table of §11.10: {rows:?}");
    for row in rows {
        assert!(
            listed.iter().any(|l| l == row),
            "§11.10's `{row}` is not in coverage.toml"
        );
    }
    // §11.13.
    assert!(listed.iter().any(|l| l.starts_with("Viewers and editors")));
}
