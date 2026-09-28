//! `kalem table recalc`: every table with a `#+TBLFM` line recalculated
//! as `org-table-recalculate-buffer-tables` does, then aligned.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use org_syntax::SyntaxKind;

use super::{Result, line_col};

/// The starts of the tables of `text` followed by a `#+TBLFM` line.
fn tables_with_formulas(doc: &org_model::Document) -> Vec<usize> {
    let root = doc.parse().syntax();
    let text = root.to_string();
    root.descendants()
        .filter(|n| n.kind() == SyntaxKind::TABLE)
        .filter_map(|n| {
            let last = n
                .children()
                .filter(|c| c.kind() == SyntaxKind::TABLE_ROW)
                .last()?;
            let end = usize::from(last.text_range().end());
            org_table::tblfm::active_line(&text[end..])?;
            Some(usize::from(n.text_range().start()))
        })
        .collect()
}

/// `kalem table recalc`: recalculates the tables of each file in place
/// (`iterate`: until they no longer change); with `check`, only lists the
/// files whose tables would change and fails if there are any.
pub(crate) fn recalc(files: &[PathBuf], iterate: bool, check: bool) -> Result<ExitCode> {
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr().lock();
    let mut changed = 0;
    let mut failed = false;
    for path in files {
        let (text, meta, _) =
            kalem_core::files::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let settings = Arc::new(org_model::Settings::default());
        let parse = |t: &str| {
            org_model::Document::with_settings(org_syntax::parse(t), settings.clone(), None)
        };
        let mut current = text.clone();
        let count = tables_with_formulas(&parse(&current)).len();
        // Tables are found again after each change: earlier ones may have
        // grown.
        for i in 0..count {
            let doc = parse(&current);
            let Some(&start) = tables_with_formulas(&doc).get(i) else {
                break;
            };
            match org_edit::recalc::recalculate(&doc, start, iterate) {
                Ok(r) => {
                    if !r.lisp.is_empty() {
                        let (line, _) = line_col(&current, start);
                        let _ = writeln!(
                            err,
                            "{}:{line}: Emacs Lisp formulas kept, not computed: {}",
                            path.display(),
                            r.lisp.join(", ")
                        );
                    }
                    current = r.transaction.apply(&current);
                }
                Err(e) => {
                    let (line, _) = line_col(&current, start);
                    let _ = writeln!(err, "{}:{line}: {e}", path.display());
                    failed = true;
                }
            }
        }
        if current == text {
            continue;
        }
        changed += 1;
        if check {
            let _ = writeln!(out, "{}", path.display());
        } else {
            kalem_core::files::write(
                path,
                &kalem_core::files::encode(&current, &meta),
                kalem_core::files::SaveOptions::default(),
            )
            .map_err(|e| format!("{}: {e}", path.display()))?;
            let _ = writeln!(out, "recalculated {}", path.display());
        }
    }
    Ok(if failed || (check && changed > 0) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
