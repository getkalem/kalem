//! Subcommand implementations.

mod diff_emacs;
mod diff_model;
mod export;
mod fmt;
mod table;

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

pub(crate) use diff_emacs::{DiffOptions, diff_emacs};
pub(crate) use diff_model::diff_model;
pub(crate) use export::{Target, export};
pub(crate) use fmt::{fmt, query};
pub(crate) use table::recalc;

pub(crate) type Result<T> = std::result::Result<T, String>;

pub(crate) fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
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

pub(crate) fn check(
    files: &[std::path::PathBuf],
    json: bool,
    deny_warnings: bool,
) -> Result<ExitCode> {
    let mut failed = false;
    let mut results = Vec::new();
    let mut out = std::io::stdout().lock();
    for f in files {
        let text = read(f)?;
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

pub(crate) fn dump(file: &Path) -> Result<ExitCode> {
    let text = read(file)?;
    let ctx = org_syntax::ParseContext::for_file(&text, file, &org_syntax::ParseContext::default());
    println!("{}", org_syntax::debug::emacs_json(&text, &ctx));
    Ok(ExitCode::SUCCESS)
}
