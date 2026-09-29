//! Printing (T2.5.11): the document as a PDF (the LaTeX back-end), handed
//! to the system's print dialog. macOS prints through Preview with its
//! dialog, Windows through the PDF application's Print verb, and Linux
//! opens Evince's print preview when it is installed, otherwise the
//! default viewer, from which the user prints. Without a display (a
//! terminal over SSH) nothing is printed on its own: the message names
//! the PDF and `lp`.

use std::ffi::OsStr;
use std::path::Path;

/// How a PDF reaches a printer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The program run.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Whether it shows a print dialog (otherwise a viewer opens).
    pub dialog: bool,
}

/// The plan for printing `pdf` on `os` (`std::env::consts::OS`), with or
/// without a `display`, the programs looked up in `path`. `None` without
/// a display on Linux.
pub fn plan(pdf: &Path, os: &str, display: bool, path: &OsStr) -> Option<Plan> {
    let file = pdf.display().to_string();
    let p = |program: &str, args: Vec<String>, dialog: bool| Plan {
        program: program.into(),
        args,
        dialog,
    };
    match os {
        "macos" => Some(p(
            "osascript",
            vec![
                "-e".into(),
                format!(
                    "tell application \"Preview\" to print (POSIX file \"{}\") with print dialog",
                    file.replace('\\', "\\\\").replace('"', "\\\"")
                ),
            ],
            true,
        )),
        "windows" => Some(p(
            "powershell",
            vec![
                "-NoProfile".into(),
                "-Command".into(),
                format!(
                    "Start-Process -FilePath '{}' -Verb Print",
                    file.replace('\'', "''")
                ),
            ],
            true,
        )),
        _ if !display => None,
        _ => Some(match crate::pdf::find("evince", path) {
            Some(_) => p("evince", vec!["--preview".into(), file], true),
            None => p("xdg-open", vec![file], false),
        }),
    }
}

/// Prints `pdf` as [`plan`] says for this system: the message for the
/// user, an error when the program could not start.
pub fn run(pdf: &Path) -> Result<String, String> {
    let display = cfg!(any(target_os = "macos", windows))
        || std::env::var_os("DISPLAY").is_some()
        || std::env::var_os("WAYLAND_DISPLAY").is_some();
    let search = std::env::var_os("PATH").unwrap_or_default();
    let path = pdf.display().to_string();
    let Some(plan) = plan(pdf, std::env::consts::OS, display, &search) else {
        return Ok(crate::tr!("msg-print-no-display", path = path));
    };
    std::process::Command::new(&plan.program)
        .args(&plan.args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| crate::tr!("msg-print-failed", error = e.to_string()))?;
    Ok(if plan.dialog {
        crate::tr!("msg-printing", path = path)
    } else {
        crate::tr!("msg-print-viewer", path = path)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans() {
        let pdf = Path::new("/d/my \"notes\".pdf");
        let mac = plan(pdf, "macos", true, OsStr::new("")).unwrap();
        assert_eq!(mac.program, "osascript");
        assert!(mac.args[1].contains("POSIX file \"/d/my \\\"notes\\\".pdf\") with print dialog"));
        let win = plan(Path::new("C:\\it's.pdf"), "windows", true, OsStr::new("")).unwrap();
        assert!(win.args[2].contains("'C:\\it''s.pdf' -Verb Print"));
        let linux = plan(pdf, "linux", true, OsStr::new("/nonexistent")).unwrap();
        assert_eq!((linux.program.as_str(), linux.dialog), ("xdg-open", false));
        assert_eq!(plan(pdf, "linux", false, OsStr::new("")), None);
    }
}
