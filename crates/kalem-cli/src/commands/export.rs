//! `kalem export FILE... --to html|md|gfm|org`: Org's export, without Emacs.

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
}

impl Target {
    fn backend(self) -> &'static dyn org_export::Backend {
        match self {
            Target::Html => &org_export::Html,
            Target::Markdown => &org_export::Markdown,
            Target::Gfm => &org_export::Gfm,
            // Not an Org exporter: `export` writes the stripped text.
            Target::Org => &org_export::Markdown,
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Target::Html => ".html",
            Target::Markdown | Target::Gfm => ".md",
            Target::Org => ".org",
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
