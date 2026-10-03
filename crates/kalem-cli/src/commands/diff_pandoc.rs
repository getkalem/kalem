//! `kalem diff-pandoc` (T2.7h.32): the structure Kalem reads in a LaTeX
//! file against pandoc's LaTeX reader (`pandoc -f latex -t json`), the
//! most widely used structural reading of LaTeX: headings (and their
//! levels, ranked), inline and displayed formulas, cited keys, footnotes,
//! figures, tables, code blocks and list items, counted on both sides.
//! Deliberate differences are in `book/part-2/latex-known-differences.org`.

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
        in_figure: bool,
    ) {
        match v {
            Value::Object(o) => {
                if let Some(Value::String(t)) = o.get("t") {
                    // A subfigure is a `Figure` in a `Figure`: one figure.
                    if t != "Figure" || !in_figure {
                        *n.entry(t.clone()).or_insert(0) += 1;
                    }
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
                let inside = in_figure || o.get("t").and_then(Value::as_str) == Some("Figure");
                for x in o.values() {
                    walk(x, n, levels, items, cites, inside);
                }
            }
            Value::Array(a) => {
                for x in a {
                    walk(x, n, levels, items, cites, in_figure);
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
        false,
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
    // `\thanks` in a title: a note the reader sees, as pandoc counts it.
    let mut thanks = 0;
    for path in &model.files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let parse = latex_syntax::parse(&text);
        thanks += parse
            .syntax()
            .descendants()
            .filter(|n| {
                n.kind() == K::COMMAND && latex_syntax::name(n).as_deref() == Some("thanks")
            })
            .count();
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
        // Not `\nocite`'s: it prints nothing in the text.
        model
            .citations
            .iter()
            .filter(|c| !c.command.starts_with("nocite"))
            .map(|c| c.keys.len())
            .sum::<usize>()
            .to_string(),
    );
    c.insert("footnotes", (model.footnotes.len() + thanks).to_string());
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
/// Whether `n` is in the text a reader reads: not inside an index,
/// glossary or nomenclature entry, the second argument of
/// `\texorpdfstring`, or a picture (TikZ, `picture`, feynmf), which is
/// drawn, not read.
fn in_the_text(n: &latex_syntax::SyntaxNode) -> bool {
    let mut child = n.clone();
    for a in n.ancestors().skip(1) {
        if a.kind() == K::ENVIRONMENT
            && latex_syntax::name(&a).is_some_and(|e| {
                matches!(
                    e.trim_end_matches('*'),
                    "tikzpicture"
                        | "picture"
                        | "pgfpicture"
                        | "circuitikz"
                        | "axis"
                        | "pspicture"
                        | "fmffile"
                        | "fmfgraph"
                        | "feynman"
                        | "xy"
                )
            })
        {
            return false;
        }
        if a.kind() == K::COMMAND {
            let name = latex_syntax::name(&a).unwrap_or_default();
            if matches!(
                name.as_str(),
                "index" | "indexsee" | "glossary" | "nomenclature"
            ) {
                return false;
            }
            if name == "texorpdfstring"
                && a.children()
                    .filter(|c| c.kind() == K::GROUP)
                    .nth(1)
                    .as_ref()
                    == Some(&child)
            {
                return false;
            }
        }
        child = a;
    }
    true
}

fn syntax_counts(root: &latex_syntax::SyntaxNode, body: &std::ops::Range<usize>) -> [usize; 5] {
    let mut out = [0usize; 5];
    for n in root
        .descendants()
        .filter(|n| body.contains(&usize::from(n.text_range().start())))
    {
        match n.kind() {
            // Formulas in commands that typeset nothing where they are
            // (index and nomenclature entries), or in the PDF string of
            // `\texorpdfstring`, are not in the text pandoc reads.
            K::INLINE_MATH if !in_the_text(&n) => {}
            K::INLINE_MATH => out[0] += 1,
            K::DISPLAY_MATH if !in_the_text(&n) => {}
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
                // Every displayed one (breqn's, empheq's, `xalignat`), not
                // `math`, which is inline.
                let math = latex_syntax::signatures::is_math(&name) && base != "math";
                let in_formula = n
                    .ancestors()
                    .skip(1)
                    .any(|a| matches!(a.kind(), K::INLINE_MATH | K::DISPLAY_MATH));
                if math && !nested && !in_formula && in_the_text(&n) {
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
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("pandoc: {e}"))?;
    // Read on threads, so a full pipe never blocks pandoc.
    let read = |r: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut buf);
            }
            buf
        })
    };
    let stdout = read(
        child
            .stdout
            .take()
            .map(|r| Box::new(r) as Box<dyn std::io::Read + Send>),
    );
    let stderr = read(
        child
            .stderr
            .take()
            .map(|r| Box::new(r) as Box<dyn std::io::Read + Send>),
    );
    // A file pandoc takes too long over is left out rather than stalling
    // a corpus run.
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| format!("pandoc: {e}"))? {
            break s;
        }
        if start.elapsed() > PANDOC_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "{}: pandoc took more than {} s",
                file.display(),
                PANDOC_TIMEOUT.as_secs()
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let (out, err) = (
        stdout.join().unwrap_or_default(),
        stderr.join().unwrap_or_default(),
    );
    if !status.success() {
        return Err(format!(
            "{}: pandoc: {}",
            file.display(),
            String::from_utf8_lossy(&err).trim()
        ));
    }
    serde_json::from_slice(&out).map_err(|e| format!("{}: {e}", file.display()))
}

/// How long pandoc may take over one file.
const PANDOC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// `kalem diff-pandoc FILE... [--summary] [--format json]`.
pub(crate) fn diff_pandoc(files: &[PathBuf], summary: bool, json: bool) -> Result<ExitCode> {
    let search = std::env::var_os("PATH").unwrap_or_default();
    let pandoc = kalem_core::pandoc::find(&search).ok_or("pandoc not found")?;
    let mut out = std::io::stdout().lock();
    let mut agree: BTreeMap<&str, usize> = BTreeMap::new();
    let mut results = Vec::new();
    // Files pandoc cannot read are reported and left out; the others are
    // still compared.
    let mut compared = 0;
    let mut failed = 0;
    for f in files {
        // A file Kalem cannot read (not UTF-8): reported, left out.
        if let Err(e) = read(f) {
            eprintln!("{e}");
            failed += 1;
            continue;
        }
        let theirs = match pandoc_json(&pandoc, f) {
            Ok(j) => pandoc_counts(&j),
            Err(e) => {
                eprintln!(
                    "{}: pandoc cannot read it: {}",
                    f.display(),
                    e.lines().next().unwrap_or("")
                );
                failed += 1;
                continue;
            }
        };
        compared += 1;
        let ours = kalem_counts(f);
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
                "{cat}: {} of {compared} files agree",
                agree.get(cat).copied().unwrap_or(0),
            )
            .map_err(|e| e.to_string())?;
        }
        if failed > 0 {
            writeln!(out, "{failed} files Kalem or pandoc could not read")
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
