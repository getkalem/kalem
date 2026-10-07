//! `kalem export FILE... --to html|md|gfm|latex|txt|utf8|org`: Org's export, without Emacs.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::Result;

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
    // A folder stands for its Org files.
    let given_one = files.len() == 1 && !files[0].is_dir();
    let files = super::expand_files_of(
        files,
        &|p| kalem_core::DocumentMode::detect(Some(p), b"") == kalem_core::DocumentMode::Org,
        "Org files",
    )?;
    if output.is_some() && !given_one {
        return Err("--output takes one input file".into());
    }
    let mut failed = false;
    for file in &files {
        // A file that cannot be read is reported, and the others exported.
        let (text, meta) = match super::read_doc(file) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{e}");
                failed = true;
                continue;
            }
        };
        let text = text.as_str();
        let mode = kalem_core::DocumentMode::detect(Some(file), text.as_bytes());
        // Markdown to Org: written from comrak's tree (T2.7c.7); to HTML:
        // comrak's rendering, as Copy as HTML gives it.
        let markdown = mode == kalem_core::DocumentMode::Markdown;
        let markdown_to_org = markdown && to == Target::Org;
        let markdown_to_html = markdown && to == Target::Html;
        // Anything else is exported from Org: another format read as Org
        // gave broken output (and exit 0), so it is refused.
        let other_format = matches!(
            mode,
            kalem_core::DocumentMode::Markdown
                | kalem_core::DocumentMode::Csv
                | kalem_core::DocumentMode::Latex
                | kalem_core::DocumentMode::Text { language: Some(_) }
        );
        if other_format && !markdown_to_org && !markdown_to_html {
            eprintln!(
                "{}: not exported: `kalem export` exports Org files{}",
                file.display(),
                if markdown {
                    ", and Markdown to HTML (`--to html`) or Org (`--to org`)"
                } else {
                    ""
                }
            );
            failed = true;
            continue;
        }
        // Converting Markdown to Org beside it does not replace an Org file
        // already there, which may be the user's own.
        if markdown_to_org && output.is_none() {
            let target = file.with_extension("org");
            if target.exists() {
                eprintln!(
                    "{}: not converted: {} exists (give --output)",
                    file.display(),
                    target.display()
                );
                failed = true;
                continue;
            }
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
        let out = if markdown_to_org {
            Ok(kalem_core::markdown_org::to_org(text))
        } else if markdown_to_html {
            Ok(if body_only {
                kalem_core::markdown::to_html(text)
            } else {
                let stem = file
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                kalem_core::markdown::to_html_page(text, &stem)
            })
        } else if to == Target::Org {
            let (out, counts) = kalem_core::kinds::strip_markup(text);
            eprintln!(
                "{}: {}",
                file.display(),
                kalem_core::kinds::dropped_summary(counts)
            );
            // The file's byte order mark kept: the Org it writes is the
            // same file without Kalem's additions.
            Ok(if meta.bom {
                format!("\u{feff}{out}")
            } else {
                out
            })
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
        eprintln!("{}: {}", file.display(), pdf::missing(engine, &search));
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
