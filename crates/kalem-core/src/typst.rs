//! Typst beside LaTeX (T2.7h.25): `.typ` files are text with Typst's
//! highlighting, an outline of their headings, and Build PDF through
//! `typst compile`, its problems shown in the text as LaTeX's are. Never
//! a translation of LaTeX.

use std::path::{Path, PathBuf};

use crate::latex_build::{Problem, Severity};
use crate::view::OutlineItem;

/// Whether the document is a Typst file.
pub fn is_typst(doc: &crate::DocumentState) -> bool {
    matches!(&doc.meta.mode, crate::DocumentMode::Text { language: Some(l) }
        if crate::command::canonical_type(l) == "typst")
}

/// The headings of a Typst text (`= Title`, `== Section`), outside raw
/// blocks and comments, in text order.
pub fn outline(text: &str) -> Vec<OutlineItem> {
    let mut out = Vec::new();
    let mut raw: Option<usize> = None;
    let mut comment = false;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let start = at;
        at += line.len();
        let trimmed = line.trim_start();
        // A raw block's fence: three or more backticks, closed by as many.
        let ticks = trimmed.bytes().take_while(|b| *b == b'`').count();
        match raw {
            Some(n) => {
                if ticks >= n {
                    raw = None;
                }
                continue;
            }
            None if ticks >= 3 && !trimmed[ticks..].contains(&"`".repeat(ticks)) => {
                raw = Some(ticks);
                continue;
            }
            None => {}
        }
        if comment {
            if line.contains("*/") {
                comment = false;
            }
            continue;
        }
        if trimmed.starts_with("/*") && !line.contains("*/") {
            comment = true;
            continue;
        }
        let level = trimmed.bytes().take_while(|b| *b == b'=').count();
        let rest = &trimmed[level..];
        if level == 0 || !(rest.starts_with(' ') || rest.starts_with('\t')) {
            continue;
        }
        // The title without a trailing comment or label.
        let mut title = rest.trim();
        if let Some(i) = title.find(" //") {
            title = title[..i].trim_end();
        }
        if title.ends_with('>')
            && let Some(i) = title.rfind(" <")
        {
            title = title[..i].trim_end();
        }
        out.push(OutlineItem {
            level,
            todo: None,
            title: title.to_string(),
            start: start + (line.len() - trimmed.len()),
            file: None,
        });
    }
    out
}

/// The outline of a Typst document.
pub fn outline_items(doc: &crate::DocumentState) -> Option<Vec<OutlineItem>> {
    is_typst(doc).then(|| outline(doc.text().as_str()))
}

/// The problems `typst compile --diagnostic-format short` prints:
/// `file:line:col: error: message`.
pub fn problems(output: &str) -> Vec<Problem> {
    output
        .lines()
        .filter_map(|l| {
            let (place, rest) = [": error: ", ": warning: "]
                .iter()
                .find_map(|sep| l.split_once(sep).map(|(p, r)| (p, (sep, r))))?;
            let mut parts = place.rsplitn(3, ':');
            let _col = parts.next()?.parse::<usize>().ok()?;
            let line = parts.next()?.parse::<usize>().ok()?;
            let file = parts.next()?.to_string();
            Some(Problem {
                file: Some(file),
                line: Some(line),
                message: rest.1.trim().to_string(),
                severity: if rest.0.contains("error") {
                    Severity::Error
                } else {
                    Severity::Warning
                },
            })
        })
        .collect()
}

/// What a build of a Typst file gave.
#[derive(Debug, Clone)]
pub struct Built {
    pub pdf: Option<PathBuf>,
    pub problems: Vec<Problem>,
}

/// The `typst` program on `search` (the `PATH`).
pub fn find(search: &std::ffi::OsStr) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "typst.exe" } else { "typst" };
    std::env::split_paths(search)
        .map(|d| d.join(exe))
        .find(|p| p.is_file())
}

/// Compiles `file` into the PDF beside it (or in `out_dir`, relative to
/// its folder) with `typst compile`.
pub fn build(file: &Path, out_dir: Option<&Path>) -> Result<Built, String> {
    let search = std::env::var_os("PATH").unwrap_or_default();
    let typst = find(&search).ok_or_else(|| crate::l10n::tr("msg-no-typst"))?;
    let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let out = out_dir.map_or(dir.clone(), |d| dir.join(d));
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let stem = file.file_stem().map(PathBuf::from).unwrap_or_default();
    let pdf = out.join(stem).with_extension("pdf");
    let output = std::process::Command::new(typst)
        .arg("compile")
        .args(["--diagnostic-format", "short"])
        .arg(file)
        .arg(&pdf)
        .current_dir(&dir)
        .output()
        .map_err(|e| e.to_string())?;
    let mut problems = problems(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() && problems.is_empty() {
        problems.push(Problem {
            file: None,
            line: None,
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            severity: Severity::Error,
        });
    }
    Ok(Built {
        pdf: (output.status.success() && pdf.is_file()).then_some(pdf),
        problems,
    })
}

/// The problems of the last build in a Typst document, for its status
/// line and marks: each on its line.
pub fn diagnostics(doc: &crate::DocumentState) -> Vec<crate::modes::ModeDiagnostic> {
    let Some(path) = doc.meta.path.as_deref().filter(|_| is_typst(doc)) else {
        return Vec::new();
    };
    let text = doc.text();
    crate::latex_build::problems_in(path)
        .into_iter()
        .map(|p| {
            let line = p
                .line
                .unwrap_or(1)
                .saturating_sub(1)
                .min(text.line_count().saturating_sub(1));
            crate::modes::ModeDiagnostic {
                range: text.line_range(line),
                code: "typst-build".into(),
                message: p.message,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings() {
        let text = "#set page(width: 10cm)\n= Introduction\nText.\n== Background <bg>\n```\n= not a heading\n```\n/* = nor\nthis */\n// = nor this\n=no space\n  === Deep // note\n";
        let o = outline(text);
        let got: Vec<(usize, &str)> = o.iter().map(|i| (i.level, i.title.as_str())).collect();
        assert_eq!(got, [(1, "Introduction"), (2, "Background"), (3, "Deep")]);
        assert_eq!(&text[o[0].start..o[0].start + 1], "=");
        assert_eq!(&text[o[2].start..o[2].start + 3], "===");
    }

    #[test]
    fn short_diagnostics() {
        let out = "main.typ:3:2: error: unknown variable: foo\nchapters/a.typ:10:1: warning: unused label\nhelp: something\n";
        let p = problems(out);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].file.as_deref(), Some("main.typ"));
        assert_eq!(p[0].line, Some(3));
        assert_eq!(p[0].message, "unknown variable: foo");
        assert_eq!(p[0].severity, Severity::Error);
        assert_eq!(p[1].file.as_deref(), Some("chapters/a.typ"));
        assert_eq!(p[1].severity, Severity::Warning);
        // A Windows path keeps its drive.
        let p = problems("C:\\d\\main.typ:1:1: error: x\n");
        assert_eq!(p[0].file.as_deref(), Some("C:\\d\\main.typ"));
    }
}
