//! Subcommand implementations.

pub(crate) mod book;
mod coverage;
mod diff_emacs;
mod diff_model;
mod diff_pandoc;
mod export;
mod fmt;
mod table;

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

pub(crate) use coverage::latex_coverage;
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
    use kalem_core::DocumentMode;
    let text = read(file)?;
    let mut out = std::io::stdout().lock();
    // As the editors and `kalem check` decide what the file is.
    let mode = DocumentMode::detect(Some(file), text.as_bytes());
    let tree = if fmt::is_klm(file, &text) {
        format!("{:#?}", klm_syntax::parse(&text))
    } else {
        match mode {
            DocumentMode::Org => format!("{:#?}", org_syntax::parse_file(&text, file).syntax()),
            DocumentMode::Latex => format!("{:#?}", latex_syntax::parse(&text).syntax()),
            DocumentMode::Markdown => markdown_tree(&text),
            m => {
                return Err(format!(
                    "{}: kalem parse reads Org, Markdown, LaTeX and Kalem files, not {}",
                    file.display(),
                    m.title()
                ));
            }
        }
    };
    writeln!(out, "{}", tree.trim_end()).map_err(|e| e.to_string())?;
    Ok(ExitCode::SUCCESS)
}

/// A Markdown document's nodes as a tree, one a line, indented by depth,
/// as the syntax trees of the other formats print: `KIND@START..END`,
/// with the text of text and code.
fn markdown_tree(text: &str) -> String {
    let md = kalem_core::markdown::Md::parse(text);
    let mut depth: Vec<usize> = Vec::with_capacity(md.nodes.len());
    let mut out = format!("Document@0..{}\n", text.len());
    for n in &md.nodes {
        let d = n.parent.map_or(0, |p| depth[p as usize] + 1);
        depth.push(d);
        let kind = format!("{:?}", n.kind);
        out.push_str(&format!(
            "{}{kind}@{}..{}",
            "  ".repeat(d + 1),
            n.range.start,
            n.range.end
        ));
        if matches!(
            n.kind,
            kalem_core::markdown::MdKind::Text | kalem_core::markdown::MdKind::Code
        ) {
            out.push_str(&format!(" {:?}", &text[n.content.clone()]));
        }
        out.push('\n');
    }
    out
}

/// The files `paths` name: a folder stands for the Org, Markdown, LaTeX,
/// Kalem, CSV and BibTeX files under it (hidden files and folders,
/// `target` and `node_modules` left out), in order.
fn expand_files(paths: &[std::path::PathBuf]) -> Result<Vec<std::path::PathBuf>> {
    use kalem_core::DocumentMode;
    let wanted = |p: &Path| {
        matches!(
            DocumentMode::detect(Some(p), b""),
            DocumentMode::Org | DocumentMode::Markdown | DocumentMode::Latex | DocumentMode::Csv
        ) || p.extension().is_some_and(|e| e.eq_ignore_ascii_case("bib"))
    };
    let mut out = Vec::new();
    for p in paths {
        if !p.is_dir() {
            out.push(p.clone());
            continue;
        }
        let mut found = Vec::new();
        let mut stack = vec![p.clone()];
        while let Some(d) = stack.pop() {
            let rd = std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?;
            for e in rd.flatten() {
                let path = e.path();
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                if path.is_dir() {
                    if !matches!(name.as_str(), "target" | "node_modules") {
                        stack.push(path);
                    }
                } else if wanted(&path) {
                    found.push(path);
                }
            }
        }
        if found.is_empty() {
            return Err(format!(
                "{}: no Org, Markdown, LaTeX, Kalem, CSV or BibTeX files in it",
                p.display()
            ));
        }
        found.sort();
        out.extend(found);
    }
    Ok(out)
}

/// 1-based line and column (in characters) of a byte offset.
pub(crate) fn line_col(text: &str, offset: usize) -> (usize, usize) {
    // The line starts of the text last asked about, so that reporting
    // many diagnostics is not quadratic.
    thread_local! {
        static STARTS: std::cell::RefCell<(usize, usize, Vec<usize>)> =
            const { std::cell::RefCell::new((0, 0, Vec::new())) };
    }
    let offset = offset.min(text.len());
    let start = STARTS.with(|c| {
        let mut c = c.borrow_mut();
        if (c.0, c.1) != (text.as_ptr() as usize, text.len()) {
            let mut v = vec![0];
            v.extend(text.match_indices('\n').map(|(i, _)| i + 1));
            *c = (text.as_ptr() as usize, text.len(), v);
        }
        let i = c.2.partition_point(|&s| s <= offset);
        (i, c.2[i - 1])
    });
    let (line, bol) = start;
    // Counted on from the last column asked for on the same line.
    thread_local! {
        static LAST: std::cell::Cell<(usize, usize, usize, usize)> =
            const { std::cell::Cell::new((usize::MAX, 0, 0, 0)) };
    }
    let key = text.as_ptr() as usize ^ text.len();
    let (k, lbol, loff, lcol) = LAST.get();
    let col = if k == key && lbol == bol && loff <= offset {
        lcol + text[loff..offset].chars().count()
    } else {
        text[bol..offset].chars().count() + 1
    };
    LAST.set((key, bol, offset, col));
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
    let files = expand_files(files)?;
    for f in &files {
        let f = f.as_path();
        let text = read(f)?;
        // As the editors decide: `.tex`, `.latex`, `.ltx`, or a mode line.
        let latex = kalem_core::DocumentMode::detect(Some(f), text.as_bytes())
            == kalem_core::DocumentMode::Latex;
        if latex {
            let (ok, result) = check_latex(f, &text, json, deny_warnings, unrendered, &mut out)?;
            failed |= !ok;
            results.extend(result);
            continue;
        }
        if fmt::is_klm(f, &text) {
            let (ok, result) = check_klm(f, &text, json, deny_warnings, &mut out)?;
            failed |= !ok;
            results.extend(result);
            continue;
        }
        let mode = kalem_core::DocumentMode::detect(Some(f), text.as_bytes());
        // A language pack's diagnostics (T2.7a.7).
        if let kalem_core::DocumentMode::Text { language: Some(l) } = &mode
            && let Some(pack) = kalem_core::packs::for_language(l)
        {
            let (ok, result) = check_pack(f, &text, &*pack, json, &mut out)?;
            failed |= !ok;
            results.extend(result);
            continue;
        }
        let markdown = mode == kalem_core::DocumentMode::Markdown;
        let parse = org_syntax::parse_file(if markdown { "" } else { &text }, f);
        let (roundtrip, mut diags) = if markdown {
            // Markdown: every text is a document; links to files that
            // are not there.
            let diags = kalem_core::markdown::missing_files(&text, f)
                .into_iter()
                .map(|(r, path)| org_syntax::Diagnostic {
                    range: org_syntax::TextRange::new(
                        org_syntax::TextSize::from(r.start as u32),
                        org_syntax::TextSize::from(r.end as u32),
                    ),
                    severity: org_syntax::Severity::Warning,
                    code: "markdown-missing-file",
                    message: kalem_core::tr!("msg-md-missing-file", path = path),
                })
                .collect();
            (true, diags)
        } else if mode == kalem_core::DocumentMode::Csv {
            // CSV: its malformed fields, not an Org parse.
            let d = kalem_core::csv::detect(&text);
            let diags = kalem_core::csv::problems(&text, &d, usize::MAX)
                .into_iter()
                .map(|p| org_syntax::Diagnostic {
                    range: org_syntax::TextRange::new(
                        org_syntax::TextSize::from(p.range.start as u32),
                        org_syntax::TextSize::from(p.range.end as u32),
                    ),
                    severity: org_syntax::Severity::Warning,
                    code: p.code,
                    message: p.message,
                })
                .collect();
            (true, diags)
        } else {
            (parse.syntax().to_string() == text, parse.diagnostics())
        };
        // Formatting an earlier Kalem wrote into an Org file (T2.13.13).
        let org = mode == kalem_core::DocumentMode::Org
            && f.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("org") || e.eq_ignore_ascii_case("klm"));
        if org {
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
        if mode != kalem_core::DocumentMode::Csv && !markdown {
            diags.extend(citation_diagnostics(&text, f));
        }
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
/// `kalem check` of a Kalem format file: what the parser recovered from
/// (RFC 0003 §15) as errors, an unknown command as a warning, and whether
/// the file is in canonical form.
fn check_klm(
    f: &Path,
    text: &str,
    json: bool,
    deny_warnings: bool,
    out: &mut impl Write,
) -> Result<(bool, Option<serde_json::Value>)> {
    let doc = klm_syntax::parse(text);
    let severity = |code: &str| {
        if code == "unknown-command" {
            "warning"
        } else {
            "error"
        }
    };
    let canonical = klm_syntax::well_formed(&doc) && klm_syntax::fmt(&doc) == text;
    let ok = doc
        .diagnostics
        .iter()
        .all(|d| severity(d.code) == "warning" && !deny_warnings);
    if json {
        let list: Vec<serde_json::Value> = doc
            .diagnostics
            .iter()
            .map(|d| {
                let (line, col) = line_col(text, d.range.0);
                serde_json::json!({
                    "code": d.code,
                    "severity": severity(d.code),
                    "message": d.message,
                    "start": d.range.0,
                    "end": d.range.1,
                    "line": line,
                    "column": col,
                })
            })
            .collect();
        let v = serde_json::json!({
            "file": f.display().to_string(),
            "canonical": canonical,
            "diagnostics": list,
        });
        return Ok((ok, Some(v)));
    }
    for d in &doc.diagnostics {
        let (line, col) = line_col(text, d.range.0);
        writeln!(
            out,
            "{}:{line}:{col}: {}[{}]: {}",
            f.display(),
            severity(d.code),
            d.code,
            d.message
        )
        .map_err(|e| e.to_string())?;
    }
    if klm_syntax::well_formed(&doc) && !canonical {
        writeln!(
            out,
            "{}: info: not in canonical form (`kalem fmt` writes it)",
            f.display()
        )
        .map_err(|e| e.to_string())?;
    }
    Ok((ok, None))
}

/// `kalem check` on a file a language pack serves: its diagnostics, each
/// an error, and whether its formatter would change it.
fn check_pack(
    f: &Path,
    text: &str,
    pack: &dyn kalem_core::packs::LanguagePack,
    json: bool,
    out: &mut impl Write,
) -> Result<(bool, Option<serde_json::Value>)> {
    let diags = pack.diagnostics(text);
    let formatted = match pack.format(text) {
        Some(kalem_core::packs::Formatted::Text(t)) => Some(t == text),
        _ => None,
    };
    if json {
        let list: Vec<serde_json::Value> = diags
            .iter()
            .map(|d| {
                let (line, col) = line_col(text, d.range.start);
                serde_json::json!({
                    "code": d.code,
                    "severity": "error",
                    "message": d.message,
                    "start": d.range.start,
                    "end": d.range.end,
                    "line": line,
                    "column": col,
                })
            })
            .collect();
        let v = serde_json::json!({
            "file": f.display().to_string(),
            "pack": pack.id(),
            "formatted": formatted,
            "diagnostics": list,
        });
        return Ok((diags.is_empty(), Some(v)));
    }
    for d in &diags {
        let (line, col) = line_col(text, d.range.start);
        writeln!(
            out,
            "{}:{line}:{col}: error[{}]: {}",
            f.display(),
            d.code,
            d.message
        )
        .map_err(|e| e.to_string())?;
    }
    if formatted == Some(false) {
        writeln!(
            out,
            "{}: info: not formatted (`kalem fmt` formats it)",
            f.display()
        )
        .map_err(|e| e.to_string())?;
    }
    Ok((diags.is_empty(), None))
}

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
    let root = kalem_core::latex_view::find_root(&file, &text);
    let project = latex_model::project::ProjectCache::default().load(&root, &disk);
    let root_text = std::fs::read_to_string(&root).map_err(|e| e.to_string())?;
    // `--engine` wins over `% !TEX program`, which wins over the packages.
    let engine = match engine.filter(|e| *e != "auto") {
        Some(e) => kalem_core::pdf::Engine::from_keyword(Some(e)),
        None => latex_build::engine(&root_text, &project.model, "auto"),
    };
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

/// `kalem view`: a unit of a file a viewer opens, as PNG or as text.
pub(crate) fn view(file: &Path, unit: usize, png: bool, output: Option<&Path>) -> Result<ExitCode> {
    let viewer = {
        let head = std::fs::read(file)
            .map(|b| b[..b.len().min(8192)].to_vec())
            .map_err(|e| format!("{}: {e}", file.display()))?;
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        kalem_core::viewer::find(&name, &head)
            .ok_or_else(|| format!("{}: no viewer opens this file", file.display()))?
    };
    let mut v = kalem_core::viewer::ViewerState::open(viewer, file)
        .map_err(|e| format!("{}: {e}", file.display()))?;
    let n = v.structure().units.len();
    if unit == 0 || unit > n {
        return Err(format!("{}: no unit {unit} (it has {n})", file.display()));
    }
    v.go_to(unit - 1);
    if png {
        let bytes = v
            .bitmap()
            .and_then(|b| kalem_core::viewer::png(&b))
            .map_err(|e| format!("{}: {e}", file.display()))?;
        match output {
            Some(o) => std::fs::write(o, bytes).map_err(|e| format!("{}: {e}", o.display()))?,
            None => std::io::stdout()
                .write_all(&bytes)
                .map_err(|e| e.to_string())?,
        }
        return Ok(ExitCode::SUCCESS);
    }
    let mut out = String::new();
    out.push_str(&v.text());
    out.push('\n');
    for f in v.info_fields() {
        out.push_str(&format!("{}: {}\n", f.label, f.value));
    }
    match output {
        Some(o) => std::fs::write(o, out).map_err(|e| format!("{}: {e}", o.display()))?,
        None => std::io::stdout()
            .write_all(out.as_bytes())
            .map_err(|e| e.to_string())?,
    }
    Ok(ExitCode::SUCCESS)
}
