//! `kalem fmt` and `kalem query`.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use super::Result;

/// `kalem fmt`: tables and tags aligned, blank lines as each document has
/// them (`org_edit::format`); code through the formatter of its type
/// (`kalem_core::formatters`: the one set in `formatters.TYPE`, else the
/// language's usual program); with `check`, only lists the files that
/// would change and fails if there are any.
pub(crate) fn fmt(files: &[PathBuf], check: bool, align: bool) -> Result<ExitCode> {
    // The user's `formatters` table, and a workspace's.
    let user = kalem_core::settings::config_dir().map(|d| d.join("settings.toml"));
    let workspace = files
        .first()
        .and_then(|p| std::path::absolute(p).ok())
        .and_then(|p| {
            let dir = if p.is_dir() {
                Some(p.as_path())
            } else {
                p.parent()
            };
            dir.and_then(kalem_core::settings::find_workspace_settings)
        });
    kalem_core::settings::Config::load(user.as_deref(), workspace.as_deref())
        .apply_process_settings();
    // A folder stands for its Org and LaTeX files.
    let files = super::expand_files_of(
        files,
        &|p| {
            matches!(
                kalem_core::DocumentMode::detect(Some(p), b""),
                kalem_core::DocumentMode::Org | kalem_core::DocumentMode::Latex
            )
        },
        "Org or LaTeX files",
    )?;
    let mut out = std::io::stdout().lock();
    let mut changed = 0;
    let mut refused = 0;
    for path in &files {
        let (text, meta, _) =
            kalem_core::files::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mode = kalem_core::DocumentMode::detect(Some(path), text.as_bytes());
        let first = text.lines().next();
        // The formatter the user set for the type comes first; then a
        // language pack's; then the type's usual program.
        let user = kalem_core::formatters::user_for_file(path, &mode, first);
        // A language pack's formatter (T2.7a.7); a syntax error refuses.
        let pack = match &mode {
            kalem_core::DocumentMode::Text { language: Some(l) } => {
                kalem_core::packs::for_language(l).and_then(|p| p.format(&text))
            }
            _ => None,
        };
        if let Some(kalem_core::packs::Formatted::Refused(d)) =
            pack.as_ref().filter(|_| user.is_none())
        {
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
        let external = user
            .or_else(|| {
                pack.is_none()
                    .then(|| kalem_core::formatters::default_for_file(path, &mode, first))
                    .flatten()
            })
            .map(|f| kalem_core::formatters::format_now(&f, &text));
        // As the editors decide: `.tex`, `.latex`, `.ltx`, or a mode line.
        let formatted = if let Some(result) = external {
            match result {
                Ok(t) => t,
                Err(why) => {
                    refused += 1;
                    let _ = writeln!(out, "{}: not formatted: {why}", path.display());
                    continue;
                }
            }
        } else if let Some(kalem_core::packs::Formatted::Text(t)) = pack {
            t
        } else if mode == kalem_core::DocumentMode::Latex {
            kalem_core::latex_fmt::format_file(path, &text, align)
        } else if mode == kalem_core::DocumentMode::Org {
            // The setup files' keywords too, as the editors read them.
            org_edit::format::format(&org_model::Document::new(org_syntax::parse_file(
                &text, path,
            )))
        } else {
            // Markdown, CSV, BibTeX, code without a formatter: Kalem has
            // none for them, and Org's would rewrite them (a GFM table's
            // `|---|` as `|---+---|`).
            let _ = writeln!(
                std::io::stderr(),
                "{}: not formatted: Kalem formats Org and LaTeX files, and code whose language has a formatter",
                path.display()
            );
            continue;
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

/// `kalem query FILE... MATCH`: the headlines matching an Org match string
/// (`org-map-entries`), as `FILE:LINE: HEADLINE` lines or JSON.
pub(crate) fn query(files: &[std::path::PathBuf], m: &str, json: bool) -> Result<ExitCode> {
    let now = jiff::Zoned::now().datetime();
    // What Emacs leaves out of a match string, it says nothing of; this
    // says it, and matches as Emacs does.
    for part in org_model::Matcher::new(m, now).ignored() {
        eprintln!("kalem query: {part:?} in the match string is no term and is left out");
    }
    // A folder stands for its Org files.
    let files = super::expand_files_of(
        files,
        &|p| kalem_core::DocumentMode::detect(Some(p), b"") == kalem_core::DocumentMode::Org,
        "Org files",
    )?;
    let mut found = Vec::new();
    let mut unreadable = false;
    for path in &files {
        let f = path.to_string_lossy().into_owned();
        // A file that cannot be read is reported, and the others read.
        let text = match super::read(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{e}");
                unreadable = true;
                continue;
            }
        };
        // `#+SETUPFILE`'s keywords and tags too, as export reads them.
        let doc = org_model::Document::with_settings(
            org_syntax::parse_file(&text, path),
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
    Ok(if unreadable {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    })
}
