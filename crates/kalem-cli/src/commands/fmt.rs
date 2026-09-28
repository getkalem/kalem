//! `kalem fmt` and `kalem query`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use super::Result;

/// `kalem fmt`: tables and tags aligned, blank lines as each document has
/// them (`org_edit::format`); with `check`, only lists the files that
/// would change and fails if there are any.
pub(crate) fn fmt(files: &[PathBuf], check: bool) -> Result<ExitCode> {
    let mut out = std::io::stdout().lock();
    let mut changed = 0;
    for path in files {
        let (text, meta, _) =
            kalem_core::files::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let doc = org_model::Document::new(org_syntax::parse(&text));
        let formatted = org_edit::format::format(&doc);
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
    Ok(if check && changed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
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
