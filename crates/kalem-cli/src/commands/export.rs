//! `kalem export FILE... --to html|md|gfm|latex|txt|utf8|org`: Org's export, without Emacs.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::{Result, read};

/// An export back-end the command line offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    /// HTML (`ox-html`).
    Html,
    /// Markdown (`ox-md`).
    Markdown,
    /// GitHub Flavored Markdown.
    Gfm,
    /// Strict Org, Kalem's additions taken out.
    Org,
    /// LaTeX (`ox-latex`).
    Latex,
    /// LaTeX with `%% org:LINE` comments.
    LatexLines,
    /// Plain text (`ox-ascii`).
    Text,
    /// Plain text with UTF-8 characters.
    Utf8,
    /// PDF: LaTeX with `%% org:LINE` comments, compiled.
    Pdf,
    /// A format pandoc writes.
    Pandoc(kalem_core::pandoc::Format),
}

/// The LaTeX back-end.
const LATEX: org_export::Latex = org_export::Latex {
    source_lines: false,
};

/// The LaTeX back-end, with the lines of the Org file marked so that
/// LaTeX's errors can be traced back.
const LATEX_LINES: org_export::Latex = org_export::Latex { source_lines: true };

impl Target {
    fn backend(self) -> &'static dyn org_export::Backend {
        match self {
            Target::Html => &org_export::Html,
            Target::Markdown => &org_export::Markdown,
            Target::Gfm => &org_export::Gfm,
            Target::Latex => &LATEX,
            Target::LatexLines | Target::Pdf => &LATEX_LINES,
            // Not an exporter of Kalem's: pandoc reads the Org text.
            Target::Pandoc(_) => &org_export::Markdown,
            Target::Text => &org_export::Text { utf8: false },
            Target::Utf8 => &org_export::Text { utf8: true },
            // Not an Org exporter: `export` writes the stripped text.
            Target::Org => &org_export::Markdown,
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Target::Html => ".html",
            Target::Markdown | Target::Gfm => ".md",
            Target::Org => ".org",
            Target::Latex | Target::LatexLines | Target::Pdf => ".tex",
            Target::Text | Target::Utf8 => ".txt",
            Target::Pandoc(f) => f.extension(),
        }
    }
}

/// The byte offset of the first headline titled `name` (or, for `#ID`,
/// whose `CUSTOM_ID` is `ID`).
fn find_headline(text: &str, name: &str) -> Option<usize> {
    use org_syntax::ast::{AstNode, Headline};
    let parse = org_syntax::parse(text);
    let root = parse.syntax();
    root.descendants()
        .filter_map(Headline::cast)
        .find(|h| match name.strip_prefix('#') {
            Some(id) => h
                .properties()
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("CUSTOM_ID") && v.trim() == id),
            None => h.raw_value().trim() == name.trim(),
        })
        .map(|h| usize::from(h.syntax().text_range().start()))
}

/// Exports `files`; `output` (only with one file) names the result, `-`
/// for standard output; `subtree` names the headline whose subtree only
/// is exported.
pub(crate) fn export(
    files: &[PathBuf],
    to: Target,
    output: Option<&Path>,
    body_only: bool,
    subtree: Option<&str>,
) -> Result<ExitCode> {
    if output.is_some() && files.len() > 1 {
        return Err("--output takes one input file".into());
    }
    let mut failed = false;
    for file in files {
        let text = read(file)?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        // The input is read as Org whatever it is: said when the editors
        // would open it as something else.
        let mode = kalem_core::DocumentMode::detect(Some(file), text.as_bytes());
        if matches!(
            mode,
            kalem_core::DocumentMode::Markdown
                | kalem_core::DocumentMode::Csv
                | kalem_core::DocumentMode::Latex
        ) {
            eprintln!(
                "{}: warning: read as Org, not as {} (`kalem export` reads Org files{})",
                file.display(),
                mode.title(),
                if mode == kalem_core::DocumentMode::Markdown {
                    "; `kalem import` converts Markdown to Org"
                } else {
                    ""
                }
            );
        }
        let at = match subtree {
            Some(name) => match find_headline(text, name) {
                Some(at) => Some(at),
                None => {
                    eprintln!("{}: no headline {name:?}", file.display());
                    failed = true;
                    continue;
                }
            },
            None => None,
        };
        let settings = org_export::Settings {
            body_only,
            input_file: Some(std::path::absolute(file).unwrap_or_else(|_| file.clone())),
            now: None,
            subtree: at,
            math: Some(kalem_core::math::export_renderer()),
            options: None,
        };
        if let Target::Pandoc(format) = to {
            if !export_pandoc(file, text, format, output, at) {
                failed = true;
            }
            continue;
        }
        let out = if to == Target::Org {
            let (out, counts) = kalem_core::kinds::strip_markup(text);
            eprintln!(
                "{}: {}",
                file.display(),
                kalem_core::kinds::dropped_summary(counts)
            );
            Ok(out)
        } else {
            org_export::export(text, to.backend(), &settings)
        };
        let out = match out {
            Ok(out) => out,
            Err(e) => {
                eprintln!("{}: {e}", file.display());
                failed = true;
                continue;
            }
        };
        match output {
            Some(p) if p == Path::new("-") => {
                std::io::stdout()
                    .lock()
                    .write_all(out.as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            _ => {
                let target = match output {
                    Some(p) => p.to_path_buf(),
                    None => org_export::output_file_name_for(text, file, to.extension(), at),
                };
                if let Some(dir) = target.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                }
                std::fs::write(&target, out).map_err(|e| format!("{}: {e}", target.display()))?;
                if to == Target::Pdf {
                    if !compile_pdf(file, text, &target) {
                        failed = true;
                    }
                    continue;
                }
                println!("{}", target.display());
            }
        }
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Compiles the LaTeX file `tex` exported from `file` to PDF, printing
/// LaTeX's problems at their Org lines; whether it worked.
fn compile_pdf(file: &Path, text: &str, tex: &Path) -> bool {
    use kalem_core::pdf;
    let engine = pdf::Engine::from_keyword(
        org_syntax::parse(text)
            .keywords()
            .iter()
            .rev()
            .find(|(k, _)| k.eq_ignore_ascii_case("LATEX_COMPILER"))
            .map(|(_, v)| v.as_str()),
    );
    let search = std::env::var_os("PATH").unwrap_or_default();
    let Some(tool) = pdf::detect(engine, &search) else {
        eprintln!(
            "{}: {}",
            file.display(),
            kalem_core::l10n::tr("msg-no-latex")
        );
        return false;
    };
    match pdf::compile(&tool, engine, tex) {
        Ok(c) => {
            if !c.problems.is_empty() {
                eprintln!("{}", pdf::report(file, &c.problems));
            }
            match c.pdf {
                Some(p) if !c.problems.iter().any(|p| p.error) => {
                    println!("{}", p.display());
                    true
                }
                _ => false,
            }
        }
        Err(e) => {
            eprintln!("{}: {e}", file.display());
            false
        }
    }
}

/// Writes `file` as `format` through pandoc; whether it worked.
fn export_pandoc(
    file: &Path,
    text: &str,
    format: kalem_core::pandoc::Format,
    output: Option<&Path>,
    subtree: Option<usize>,
) -> bool {
    let search = std::env::var_os("PATH").unwrap_or_default();
    let Some(pandoc) = kalem_core::pandoc::find(&search) else {
        eprintln!(
            "{}: {}",
            file.display(),
            kalem_core::l10n::tr("msg-no-pandoc")
        );
        return false;
    };
    if subtree.is_some() {
        eprintln!("{}: --subtree is not supported with pandoc", file.display());
        return false;
    }
    let file = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    let target = match output {
        Some(p) => std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()),
        None => org_export::output_file_name_for(text, &file, format.extension(), None),
    };
    match kalem_core::pandoc::export(&pandoc, text, &file, format, &target) {
        Ok(()) => {
            println!("{}", target.display());
            true
        }
        Err(e) => {
            eprintln!("{}: {e}", file.display());
            false
        }
    }
}

/// `kalem import`: Word, OpenDocument, Markdown, HTML, EPUB or RTF files
/// to Org through pandoc.
pub(crate) fn import(files: &[PathBuf], output: Option<&Path>, force: bool) -> Result<ExitCode> {
    if output.is_some() && files.len() > 1 {
        return Err("--output takes one input file".into());
    }
    let search = std::env::var_os("PATH").unwrap_or_default();
    let Some(pandoc) = kalem_core::pandoc::find(&search) else {
        return Err(kalem_core::l10n::tr("msg-no-pandoc"));
    };
    let mut failed = false;
    for file in files {
        let target = output.map_or_else(|| file.with_extension("org"), Path::to_path_buf);
        let to_stdout = target == Path::new("-");
        if !to_stdout && target.exists() && !force {
            eprintln!("{}: exists (use --force to replace it)", target.display());
            failed = true;
            continue;
        }
        let stem = file
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let media = PathBuf::from(format!("{stem}_assets"));
        match kalem_core::pandoc::import(&pandoc, file, Some(&media)) {
            Ok(org) if to_stdout => {
                std::io::stdout()
                    .lock()
                    .write_all(org.as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            Ok(org) => {
                std::fs::write(&target, org).map_err(|e| format!("{}: {e}", target.display()))?;
                println!("{}", target.display());
            }
            Err(e) => {
                eprintln!("{}: {e}", file.display());
                failed = true;
            }
        }
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
