//! `kalem export FILE... --to html|md`: Org's export, without Emacs.

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
}

impl Target {
    fn backend(self) -> &'static dyn org_export::Backend {
        match self {
            Target::Html => &org_export::Html,
            Target::Markdown => &org_export::Markdown,
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Target::Html => ".html",
            Target::Markdown => ".md",
        }
    }
}

/// Exports `files`; `output` (only with one file) names the result, `-`
/// for standard output.
pub(crate) fn export(
    files: &[PathBuf],
    to: Target,
    output: Option<&Path>,
    body_only: bool,
) -> Result<ExitCode> {
    if output.is_some() && files.len() > 1 {
        return Err("--output takes one input file".into());
    }
    let mut failed = false;
    for file in files {
        let text = read(file)?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let settings = org_export::Settings {
            body_only,
            input_file: Some(std::path::absolute(file).unwrap_or_else(|_| file.clone())),
            now: None,
        };
        let out = match org_export::export(text, to.backend(), &settings) {
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
                    None => org_export::output_file_name(text, file, to.extension()),
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
