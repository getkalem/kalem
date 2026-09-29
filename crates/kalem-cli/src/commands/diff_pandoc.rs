//! `kalem diff-pandoc` (T2.7h.32): the structure Kalem reads in a LaTeX
//! file against pandoc's LaTeX reader (`pandoc -f latex -t json`), the
//! most widely used structural reading of LaTeX: headings (and their
//! levels, ranked), inline and displayed formulas, cited keys, footnotes,
//! figures, tables, code blocks and list items, counted on both sides.
//! Deliberate differences are in `docs/known-differences-latex.org`.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use latex_syntax::SyntaxKind as K;
use serde_json::Value;

use super::{Result, read};

/// What is compared, in order.
const CATEGORIES: &[&str] = &[
    "headings",
    "heading-levels",
    "inline-math",
    "display-math",
    "citations",
    "footnotes",
    "figures",
    "tables",
    "code-blocks",
    "list-items",
];

/// The counts of one side.
type Counts = BTreeMap<&'static str, String>;

/// Pandoc's element types in a tree, with how often.
fn pandoc_counts(v: &Value) -> Counts {
    let mut n: BTreeMap<String, usize> = BTreeMap::new();
    let mut levels: Vec<u64> = Vec::new();
    let mut items = 0usize;
    let mut cites = 0usize;
    fn walk(
        v: &Value,
        n: &mut BTreeMap<String, usize>,
        levels: &mut Vec<u64>,
        items: &mut usize,
        cites: &mut usize,
    ) {
        match v {
            Value::Object(o) => {
                if let Some(Value::String(t)) = o.get("t") {
                    *n.entry(t.clone()).or_insert(0) += 1;
                    let c = o.get("c");
                    match t.as_str() {
                        "Header" => {
                            if let Some(l) = c.and_then(|c| c.get(0)).and_then(Value::as_u64) {
                                levels.push(l);
                            }
                        }
                        "BulletList" => *items += c.and_then(Value::as_array).map_or(0, Vec::len),
                        "OrderedList" | "DefinitionList" => {
                            let list = if t == "OrderedList" {
                                c.and_then(|c| c.get(1))
                            } else {
                                c
                            };
                            *items += list.and_then(Value::as_array).map_or(0, Vec::len);
                        }
                        "Cite" => {
                            *cites += c
                                .and_then(|c| c.get(0))
                                .and_then(Value::as_array)
                                .map_or(0, Vec::len)
                        }
                        _ => {}
                    }
                }
                for x in o.values() {
                    walk(x, n, levels, items, cites);
                }
            }
            Value::Array(a) => {
                for x in a {
                    walk(x, n, levels, items, cites);
                }
            }
            _ => {}
        }
    }
    walk(
        v.get("blocks").unwrap_or(&Value::Null),
        &mut n,
        &mut levels,
        &mut items,
        &mut cites,
    );
    let get = |k: &str| n.get(k).copied().unwrap_or(0);
    let mut c = Counts::new();
    c.insert("headings", get("Header").to_string());
    c.insert(
        "heading-levels",
        ranked(&levels.iter().map(|l| *l as i64).collect::<Vec<_>>()),
    );
    c.insert("inline-math", get("InlineMath").to_string());
    c.insert("display-math", get("DisplayMath").to_string());
    c.insert("citations", cites.to_string());
    c.insert("footnotes", get("Note").to_string());
    c.insert("figures", get("Figure").to_string());
    c.insert("tables", get("Table").to_string());
    c.insert("code-blocks", get("CodeBlock").to_string());
    c.insert("list-items", items.to_string());
    c
}

/// Levels as ranks from 1 (pandoc and LaTeX number them differently).
fn ranked(levels: &[i64]) -> String {
    let mut distinct: Vec<i64> = levels.to_vec();
    distinct.sort_unstable();
    distinct.dedup();
    levels
        .iter()
        .map(|l| (distinct.iter().position(|d| d == l).unwrap_or(0) + 1).to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Kalem's counts of the same things, over the files the document
/// includes (as pandoc reads them too).
fn kalem_counts(file: &Path) -> Counts {
    let disk = latex_model::project::Disk;
    let project = latex_model::project::ProjectCache::default().load(file, &disk);
    let model = project.model.clone();
    let mut totals = [0usize; 5];
    for path in &model.files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let parse = latex_syntax::parse(&text);
        // A document's body (the root's, a subfile's), an included file
        // whole.
        let body = latex_model::Model::new(&parse)
            .body
            .unwrap_or(0..text.len());
        for (t, n) in totals.iter_mut().zip(syntax_counts(&parse.syntax(), &body)) {
            *t += n;
        }
    }
    let [inline, display, code, tables, items] = totals;
    let mut c = Counts::new();
    c.insert("headings", model.sections.len().to_string());
    c.insert(
        "heading-levels",
        ranked(
            &model
                .sections
                .iter()
                .map(|s| i64::from(s.level))
                .collect::<Vec<_>>(),
        ),
    );
    c.insert("inline-math", inline.to_string());
    c.insert("display-math", display.to_string());
    c.insert(
        "citations",
        model
            .citations
            .iter()
            .map(|c| c.keys.len())
            .sum::<usize>()
            .to_string(),
    );
    c.insert("footnotes", model.footnotes.len().to_string());
    c.insert(
        "figures",
        model
            .floats
            .iter()
            .filter(|f| f.kind == "figure")
            .count()
            .to_string(),
    );
    c.insert("tables", tables.to_string());
    c.insert("code-blocks", code.to_string());
    c.insert("list-items", items.to_string());
    c
}

/// Inline and displayed formulas, code blocks, tables and list items in
/// `body` of a tree.
fn syntax_counts(root: &latex_syntax::SyntaxNode, body: &std::ops::Range<usize>) -> [usize; 5] {
    let mut out = [0usize; 5];
    for n in root
        .descendants()
        .filter(|n| body.contains(&usize::from(n.text_range().start())))
    {
        match n.kind() {
            K::INLINE_MATH => out[0] += 1,
            K::DISPLAY_MATH => out[1] += 1,
            K::ENVIRONMENT => {
                let name = latex_syntax::name(&n).unwrap_or_default();
                let base = name.trim_end_matches('*');
                // A math environment is one displayed formula, not those
                // inside another.
                let nested = n.ancestors().skip(1).any(|a| {
                    a.kind() == K::ENVIRONMENT
                        && latex_syntax::name(&a)
                            .is_some_and(|x| latex_syntax::signatures::is_math(&x))
                });
                let math = matches!(
                    base,
                    "equation"
                        | "align"
                        | "gather"
                        | "multline"
                        | "eqnarray"
                        | "alignat"
                        | "flalign"
                        | "displaymath"
                );
                if math && !nested {
                    out[1] += 1;
                }
                if latex_syntax::signatures::is_verbatim(&name) && name != "comment" {
                    out[2] += 1;
                }
                if matches!(base, "tabular" | "tabularx" | "longtable") {
                    out[3] += 1;
                }
            }
            K::COMMAND if latex_syntax::name(&n).as_deref() == Some("item") => out[4] += 1,
            _ => {}
        }
    }
    out
}

fn pandoc_json(pandoc: &Path, file: &Path) -> Result<Value> {
    let mut cmd = Command::new(pandoc);
    cmd.args(["-f", "latex", "-t", "json"]);
    if let Some(d) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
        cmd.current_dir(d);
    }
    cmd.arg(file.file_name().map(PathBuf::from).unwrap_or_default());
    let out = cmd.output().map_err(|e| format!("pandoc: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{}: pandoc: {}",
            file.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("{}: {e}", file.display()))
}

/// `kalem diff-pandoc FILE... [--summary] [--format json]`.
pub(crate) fn diff_pandoc(files: &[PathBuf], summary: bool, json: bool) -> Result<ExitCode> {
    let search = std::env::var_os("PATH").unwrap_or_default();
    let pandoc = kalem_core::pandoc::find(&search).ok_or("pandoc not found")?;
    let mut out = std::io::stdout().lock();
    let mut agree: BTreeMap<&str, usize> = BTreeMap::new();
    let mut results = Vec::new();
    for f in files {
        read(f)?;
        let ours = kalem_counts(f);
        let theirs = pandoc_counts(&pandoc_json(&pandoc, f)?);
        let mut rows = Vec::new();
        for cat in CATEGORIES {
            let (a, b) = (&ours[cat], &theirs[cat]);
            if a == b {
                *agree.entry(cat).or_insert(0) += 1;
            }
            rows.push((*cat, a.clone(), b.clone()));
        }
        if json {
            results.push(serde_json::json!({
                "file": f.display().to_string(),
                "categories": rows.iter().map(|(c, a, b)| serde_json::json!({"category": c, "kalem": a, "pandoc": b, "agree": a == b})).collect::<Vec<_>>(),
            }));
        } else if !summary {
            for (c, a, b) in &rows {
                if a != b {
                    writeln!(out, "{}: {c}: kalem {a}, pandoc {b}", f.display())
                        .map_err(|e| e.to_string())?;
                }
            }
        }
    }
    if json {
        writeln!(out, "{}", Value::Array(results)).map_err(|e| e.to_string())?;
    } else if summary {
        for cat in CATEGORIES {
            writeln!(
                out,
                "{cat}: {} of {} files agree",
                agree.get(cat).copied().unwrap_or(0),
                files.len()
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks() {
        assert_eq!(ranked(&[1, 2, 3, 1, 2]), "1,2,3,1,2");
        assert_eq!(ranked(&[0, 1, 1, 0]), "1,2,2,1");
    }
}
