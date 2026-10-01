//! `kalem fmt` and `kalem query`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use super::Result;

/// `kalem fmt`: tables and tags aligned, blank lines as each document has
/// them (`org_edit::format`); with `check`, only lists the files that
/// would change and fails if there are any.
pub(crate) fn fmt(files: &[PathBuf], check: bool, align: bool, repair: bool) -> Result<ExitCode> {
    let mut out = std::io::stdout().lock();
    let mut changed = 0;
    let mut refused = 0;
    for path in files {
        let (text, meta, _) =
            kalem_core::files::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        // The Kalem format: its canonical form, for well-formed files only
        // (RFC 0003 §15); `--repair` formats the recovered tree and shows
        // the change.
        if is_klm(path, &text) {
            let doc = klm_syntax::parse(&text);
            if !klm_syntax::well_formed(&doc) && !repair {
                refused += 1;
                let _ = writeln!(
                    out,
                    "{}: not formatted: {} problems (`kalem check` lists them, `kalem fmt --repair` fixes them)",
                    path.display(),
                    doc.diagnostics.len()
                );
                continue;
            }
            let formatted = klm_syntax::fmt(&doc);
            if formatted == text {
                continue;
            }
            changed += 1;
            if repair {
                let _ = write!(
                    out,
                    "{}",
                    kalem_core::lines::unified_diff(&text, &formatted, &path.display().to_string())
                );
            }
            if check {
                if !repair {
                    let _ = writeln!(out, "{}", path.display());
                }
            } else {
                kalem_core::files::write(
                    path,
                    &kalem_core::files::encode(&formatted, &meta),
                    kalem_core::files::SaveOptions::default(),
                )
                .map_err(|e| format!("{}: {e}", path.display()))?;
                let _ = writeln!(out, "formatted {}", path.display());
            }
            continue;
        }
        let mode = kalem_core::DocumentMode::detect(Some(path), text.as_bytes());
        // A language pack's formatter (T2.7a.7); a syntax error refuses.
        let pack = match &mode {
            kalem_core::DocumentMode::Text { language: Some(l) } => {
                kalem_core::packs::for_language(l).and_then(|p| p.format(&text))
            }
            _ => None,
        };
        if let Some(kalem_core::packs::Formatted::Refused(d)) = &pack {
            refused += 1;
            let line = text[..d.range.start.min(text.len())].matches('\n').count() + 1;
            let _ = writeln!(
                out,
                "{}:{line}: not formatted: {}",
                path.display(),
                d.message
            );
            continue;
        }
        // As the editors decide: `.tex`, `.latex`, `.ltx`, or a mode line.
        let latex = mode == kalem_core::DocumentMode::Latex;
        let formatted = if let Some(kalem_core::packs::Formatted::Text(t)) = pack {
            t
        } else if latex {
            kalem_core::latex_fmt::format(&text, align)
        } else {
            org_edit::format::format(&org_model::Document::new(org_syntax::parse(&text)))
        };
        if formatted == text {
            continue;
        }
        changed += 1;
        if check {
            let _ = writeln!(out, "{}", path.display());
        } else {
            kalem_core::files::write(
                path,
                &kalem_core::files::encode(&formatted, &meta),
                kalem_core::files::SaveOptions::default(),
            )
            .map_err(|e| format!("{}: {e}", path.display()))?;
            let _ = writeln!(out, "formatted {}", path.display());
        }
    }
    Ok(if (check && changed > 0) || refused > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Whether `path` is in the Kalem format: `.klm`, or text that starts
/// with `\klm[`.
pub(crate) fn is_klm(path: &Path, text: &str) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("klm"))
        || text.starts_with("\\klm[")
}

/// `kalem query FILE... MATCH`: the headlines matching an Org match string
/// (`org-map-entries`), as `FILE:LINE: HEADLINE` lines or JSON.
pub(crate) fn query(args: &[String], json: bool) -> Result<ExitCode> {
    let (m, files) = args.split_last().ok_or("A match string is needed")?;
    let now = jiff::Zoned::now().datetime();
    let mut found = Vec::new();
    for f in files {
        let path = Path::new(f);
        let text = super::read(path)?;
        let doc = org_model::Document::with_settings(
            org_syntax::parse(&text),
            Arc::new(org_model::Settings::default()),
            Some(f.clone()),
        );
        for id in doc.matching(m, now) {
            let e = doc.entry(id);
            let start = usize::from(e.range.start());
            let line = text[..start].matches('\n').count() + 1;
            found.push(serde_json::json!({
                "file": f,
                "line": line,
                "level": e.level,
                "todo": e.todo,
                "priority": e.priority.map(String::from),
                "title": e.raw_title,
                "tags": doc.tags(id),
                "localTags": e.local_tags,
                "headline": e.line,
            }));
        }
    }
    let mut out = std::io::stdout().lock();
    if json {
        let _ = writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&found).map_err(|e| e.to_string())?
        );
    } else {
        for h in &found {
            let _ = writeln!(
                out,
                "{}:{}: {}",
                h["file"].as_str().unwrap_or(""),
                h["line"],
                h["headline"].as_str().unwrap_or("")
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}
