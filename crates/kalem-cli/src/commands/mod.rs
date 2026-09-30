//! Subcommand implementations.

pub(crate) mod book;
mod diff_emacs;
mod diff_model;
mod diff_pandoc;
mod export;
mod fmt;
mod table;

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

pub(crate) use diff_emacs::{DiffOptions, diff_emacs};
pub(crate) use diff_model::diff_model;
pub(crate) use diff_pandoc::diff_pandoc;
pub(crate) use export::{Target, export, import};
pub(crate) use fmt::{fmt, query};
pub(crate) use table::recalc;

pub(crate) type Result<T> = std::result::Result<T, String>;

pub(crate) fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// `kalem commands [--type TYPE]`: every command, or those whose scope
/// serves `text_type`, with their scope and default keys.
pub(crate) fn list_commands(text_type: Option<&str>) -> Result<ExitCode> {
    let reg = kalem_core::CommandRegistry::with_builtins();
    let mut cmds: Vec<_> = reg
        .commands()
        .filter(|c| {
            text_type.is_none_or(|t| {
                c.scope
                    .as_ref()
                    .is_some_and(|s| s.serves(&t.to_lowercase()))
            })
        })
        .collect();
    cmds.sort_by(|a, b| a.id.cmp(&b.id));
    let mut out = std::io::stdout().lock();
    for c in cmds {
        let keys: Vec<String> = c.default_keys.iter().map(ToString::to_string).collect();
        writeln!(
            out,
            "{}\t{}\t{}\t{}",
            c.id,
            c.display_title(),
            c.scope.as_ref().map_or_else(String::new, |s| s.describe()),
            keys.join(" ")
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(ExitCode::SUCCESS)
}

/// `kalem complete FILE:LINE:COL`: the items every completer gives there,
/// as on request (Alt+/), best first.
pub(crate) fn complete(place: &str) -> Result<ExitCode> {
    let mut parts = place.rsplitn(3, ':');
    let (col, line, file) = (parts.next(), parts.next(), parts.next());
    let (Some(col), Some(line), Some(file)) = (col, line, file) else {
        return Err(format!("{place}: expected FILE:LINE:COLUMN"));
    };
    let (line, col): (usize, usize) = (
        line.parse().map_err(|_| format!("{place}: bad line"))?,
        col.parse().map_err(|_| format!("{place}: bad column"))?,
    );
    let path = Path::new(file);
    let mut doc = kalem_core::DocumentState::open(
        path,
        std::sync::Arc::new(org_model::Settings::default()),
        &org_syntax::ParseContext::default(),
    )
    .map_err(|e| format!("{}: {e}", path.display()))?;
    let text = doc.text();
    let l = line.max(1) - 1;
    if l >= text.line_count() {
        return Err(format!("{place}: past the end of the file"));
    }
    let r = text.line_range(l);
    let pos = text.as_str()[r.clone()]
        .char_indices()
        .nth(col.max(1) - 1)
        .map_or(r.end, |(i, _)| r.start + i);
    doc.move_cursor(pos, false);
    let items = kalem_core::completers::Registry::with_builtins().complete(
        &mut doc,
        true,
        std::time::Duration::from_secs(2),
    );
    let mut out = std::io::stdout().lock();
    for i in items {
        writeln!(out, "{}\t{}\t{}", i.label, i.kind.name(), i.source).map_err(|e| e.to_string())?;
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn parse(file: &Path) -> Result<ExitCode> {
    let text = read(file)?;
    let parse = org_syntax::parse_file(&text, file);
    let mut out = std::io::stdout().lock();
    writeln!(out, "{:#?}", parse.syntax()).map_err(|e| e.to_string())?;
    Ok(ExitCode::SUCCESS)
}

/// 1-based line and column (in characters) of a byte offset.
pub(crate) fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset.min(text.len())];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}

/// Warnings about citations: `#+BIBLIOGRAPHY` files that cannot be read,
/// and cited keys none of the files has.
fn citation_diagnostics(text: &str, file: &std::path::Path) -> Vec<org_syntax::Diagnostic> {
    let doc = org_model::Document::new(org_syntax::parse_file(text, file));
    let dir = file.parent().filter(|d| !d.as_os_str().is_empty());
    let files = doc.bibliography(dir);
    let citations = doc.citations();
    if files.is_empty() && citations.is_empty() {
        return Vec::new();
    }
    let range = |r: std::ops::Range<usize>| {
        org_syntax::TextRange::new(
            org_syntax::TextSize::from(r.start as u32),
            org_syntax::TextSize::from(r.end as u32),
        )
    };
    let mut out = Vec::new();
    let (bib, errors) = org_cite::Bibliography::load(&files);
    // A keyword's range: its line.
    let keyword_line = |name: &str| {
        text.lines()
            .scan(0, |at, l| {
                let start = *at;
                *at += l.len() + 1;
                Some((start, l))
            })
            .find(|(_, l)| {
                l.to_ascii_lowercase().starts_with("#+bibliography:") && l.contains(name)
            })
            .map_or(0..0, |(s, l)| s..s + l.len())
    };
    for (path, e) in &errors {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        out.push(org_syntax::Diagnostic {
            range: range(keyword_line(&name)),
            severity: org_syntax::Severity::Warning,
            code: "bibliography-unreadable",
            message: kalem_core::tr!(
                "cite-bibliography-unreadable",
                file = path.display().to_string(),
                error = e.clone()
            ),
        });
    }
    if files.len() > errors.len() {
        for c in citations {
            for k in &c.keys {
                if bib.get(k).is_none() {
                    out.push(org_syntax::Diagnostic {
                        range: range(c.range.clone()),
                        severity: org_syntax::Severity::Warning,
                        code: "cite-unknown-key",
                        message: kalem_core::tr!("cite-unknown-key", key = k.clone()),
                    });
                }
            }
        }
    }
    out
}

pub(crate) fn check(
    files: &[std::path::PathBuf],
    json: bool,
    deny_warnings: bool,
    unrendered: bool,
) -> Result<ExitCode> {
    let mut failed = false;
    let mut results = Vec::new();
    let mut out = std::io::stdout().lock();
    for f in files {
        let text = read(f)?;
        let latex = f
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("tex") || e.eq_ignore_ascii_case("ltx"));
        if latex {
            let (ok, result) = check_latex(f, &text, json, deny_warnings, unrendered, &mut out)?;
            failed |= !ok;
            results.extend(result);
            continue;
        }
        let parse = org_syntax::parse_file(&text, f);
        let roundtrip = parse.syntax().to_string() == text;
        let mut diags = parse.diagnostics();
        // Kalem's additions in a strict `.org` file (design §3.7).
        let org = f.extension().is_some_and(|e| e.eq_ignore_ascii_case("org"));
        let opted_in = kalem_core::rich::kalem_option(&parse.keywords(), "markup")
            .is_some_and(|v| v.eq_ignore_ascii_case("yes"));
        if org && !opted_in {
            for (r, _) in kalem_core::kinds::markup(&parse.syntax()) {
                diags.push(org_syntax::Diagnostic {
                    range: org_syntax::TextRange::new(
                        org_syntax::TextSize::from(r.start as u32),
                        org_syntax::TextSize::from(r.end as u32),
                    ),
                    severity: org_syntax::Severity::Warning,
                    code: "kalem-markup-in-org",
                    message: kalem_core::l10n::tr("kind-markup-in-org"),
                });
            }
            diags.sort_by_key(|d| d.range.start());
        }
        // Citations: the bibliography files, and keys none of them has.
        diags.extend(citation_diagnostics(&text, f));
        diags.sort_by_key(|d| d.range.start());
        if !roundtrip {
            failed = true;
        }
        if deny_warnings
            && diags
                .iter()
                .any(|d| d.severity == org_syntax::Severity::Warning)
        {
            failed = true;
        }
        if json {
            let list: Vec<serde_json::Value> = diags
                .iter()
                .map(|d| {
                    let (line, col) = line_col(&text, usize::from(d.range.start()));
                    serde_json::json!({
                        "code": d.code,
                        "severity": format!("{:?}", d.severity).to_lowercase(),
                        "message": d.message,
                        "start": u32::from(d.range.start()),
                        "end": u32::from(d.range.end()),
                        "line": line,
                        "column": col,
                    })
                })
                .collect();
            results.push(serde_json::json!({ "file": f.display().to_string(), "roundtrip": roundtrip, "diagnostics": list }));
        } else {
            if !roundtrip {
                writeln!(
                    out,
                    "{}: error: the parse tree does not reproduce the file (please report this)",
                    f.display()
                )
                .map_err(|e| e.to_string())?;
            }
            for d in &diags {
                let (line, col) = line_col(&text, usize::from(d.range.start()));
                let sev = match d.severity {
                    org_syntax::Severity::Warning => "warning",
                    org_syntax::Severity::Info => "info",
                };
                writeln!(
                    out,
                    "{}:{line}:{col}: {sev}[{}]: {}",
                    f.display(),
                    d.code,
                    d.message
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }
    if json {
        writeln!(out, "{}", serde_json::Value::Array(results)).map_err(|e| e.to_string())?;
    }
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// `kalem check` of a LaTeX file: whether it passes, and its JSON result.
fn check_latex(
    f: &Path,
    text: &str,
    json: bool,
    deny_warnings: bool,
    unrendered: bool,
    out: &mut impl Write,
) -> Result<(bool, Option<serde_json::Value>)> {
    use kalem_core::latex_check::{self, Severity};
    let roundtrip = latex_syntax::parse(text).syntax().to_string() == text;
    let diags = latex_check::check(f, text);
    let report = unrendered.then(|| latex_check::unrendered_in(text, Some(f)));
    let ok = roundtrip && !(deny_warnings && diags.iter().any(|d| d.severity == Severity::Warning));
    if json {
        let list: Vec<serde_json::Value> = diags
            .iter()
            .map(|d| {
                let (line, col) = line_col(text, d.range.start);
                serde_json::json!({
                    "code": d.code,
                    "severity": format!("{:?}", d.severity).to_lowercase(),
                    "message": d.message,
                    "start": d.range.start,
                    "end": d.range.end,
                    "line": line,
                    "column": col,
                })
            })
            .collect();
        let mut v = serde_json::json!({ "file": f.display().to_string(), "roundtrip": roundtrip, "diagnostics": list });
        if let Some(r) = &report {
            v["unrendered"] = r
                .iter()
                .map(|(n, c)| serde_json::json!({ "construct": n, "count": c }))
                .collect();
            v["rendered"] = serde_json::json!(latex_check::coverage_in(text, Some(f)));
        }
        return Ok((ok, Some(v)));
    }
    if !roundtrip {
        writeln!(
            out,
            "{}: error: the parse tree does not reproduce the file (please report this)",
            f.display()
        )
        .map_err(|e| e.to_string())?;
    }
    for d in &diags {
        let (line, col) = line_col(text, d.range.start);
        let sev = match d.severity {
            Severity::Warning => "warning",
            Severity::Info => "info",
        };
        writeln!(
            out,
            "{}:{line}:{col}: {sev}[{}]: {}",
            f.display(),
            d.code,
            d.message
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(r) = report {
        writeln!(
            out,
            "{}: rendered: {:.1}%",
            f.display(),
            latex_check::coverage_in(text, Some(f)) * 100.0
        )
        .map_err(|e| e.to_string())?;
        for (n, c) in r {
            writeln!(out, "{}: unrendered: {n} ({c})", f.display()).map_err(|e| e.to_string())?;
        }
    }
    Ok((ok, None))
}

/// `kalem latex build`.
pub(crate) fn latex_build(
    file: &Path,
    json: bool,
    engine: Option<&str>,
    outdir: Option<&Path>,
) -> Result<ExitCode> {
    use kalem_core::latex_build::{self, Severity};
    let text = read(file)?;
    let file = std::path::absolute(file).map_err(|e| e.to_string())?;
    let disk = latex_model::project::Disk;
    let root = latex_model::project::find_root(&file, &text, &disk, None, None);
    let project = latex_model::project::ProjectCache::default().load(&root, &disk);
    let root_text = std::fs::read_to_string(&root).map_err(|e| e.to_string())?;
    let engine = latex_build::engine(&root_text, &project.model, engine.unwrap_or("auto"));
    let built = latex_build::build(&root, engine, outdir)?;
    let failed =
        built.problems.iter().any(|p| p.severity == Severity::Error) || built.pdf.is_none();
    let mut out = std::io::stdout().lock();
    if json {
        let problems: Vec<serde_json::Value> = built
            .problems
            .iter()
            .map(|p| {
                serde_json::json!({
                    "file": p.file,
                    "line": p.line,
                    "severity": format!("{:?}", p.severity).to_lowercase(),
                    "message": p.message,
                })
            })
            .collect();
        let v = serde_json::json!({
            "root": root.display().to_string(),
            "pdf": built.pdf.as_ref().map(|p| p.display().to_string()),
            "problems": problems,
        });
        writeln!(out, "{v}").map_err(|e| e.to_string())?;
    } else {
        let report = latex_build::report(&root, &built.problems);
        if !report.is_empty() {
            writeln!(out, "{report}").map_err(|e| e.to_string())?;
        }
        if let Some(pdf) = &built.pdf {
            writeln!(out, "{}", pdf.display()).map_err(|e| e.to_string())?;
        }
    }
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

pub(crate) fn dump(file: &Path) -> Result<ExitCode> {
    let text = read(file)?;
    let ctx = org_syntax::ParseContext::for_file(&text, file, &org_syntax::ParseContext::default());
    println!("{}", org_syntax::debug::emacs_json(&text, &ctx));
    Ok(ExitCode::SUCCESS)
}
