//! The `kalem` binary: the graphical editor (`kalem FILE`, `kalem gui
//! FILE`), the terminal editor (`kalem tui FILE`, `kalem -t FILE`) or the
//! command-line tools (`kalem check`, …).

// The command-line entry point reports to the terminal.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::ffi::OsString;
use std::process::ExitCode;

/// The command-line tools' subcommands.
const SUBCOMMANDS: &[&str] = &[
    "parse",
    "check",
    "fmt",
    "complete",
    "commands",
    "export",
    "import",
    "query",
    "table",
    "dump",
    "diff-emacs",
    "help",
];

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    let first = args.get(1).and_then(|a| a.to_str());
    match first {
        Some("tui" | "-t") => tui(&args[2..]),
        Some("gui") => gui(&args[2..]),
        Some(a) if SUBCOMMANDS.contains(&a) || a.starts_with('-') => kalem_cli::run(args),
        // A word that is no file and does not look like one (no directory,
        // no extension) is a mistyped subcommand.
        Some(a) if !looks_like_file(a) => {
            eprintln!(
                "kalem: no command or file `{a}`; see `kalem help` (a new file needs an extension, such as {a}.org)"
            );
            ExitCode::from(2)
        }
        // A file, or nothing: the editor, graphical where there is a
        // display.
        _ if has_display() => gui(&args[1..]),
        _ => tui(&args[1..]),
    }
}

/// Whether `arg` names a file: an existing path, or one with a directory
/// or an extension.
fn looks_like_file(arg: &str) -> bool {
    let p = std::path::Path::new(arg);
    p.exists() || p.extension().is_some() || arg.contains(['/', std::path::MAIN_SEPARATOR])
}

/// Whether a graphical session is available (and this build has the
/// graphical editor).
fn has_display() -> bool {
    if cfg!(not(feature = "gui")) {
        return false;
    }
    if cfg!(any(target_os = "macos", windows)) {
        return true;
    }
    std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn one_path(args: &[OsString]) -> Result<Option<std::path::PathBuf>, ExitCode> {
    match args {
        [] => Ok(None),
        [p] => Ok(Some(p.into())),
        _ => {
            eprintln!("kalem: one file at a time");
            Err(ExitCode::FAILURE)
        }
    }
}

#[cfg(feature = "gui")]
fn gui(args: &[OsString]) -> ExitCode {
    match one_path(args) {
        Ok(path) => {
            kalem_ui::run(path);
            ExitCode::SUCCESS
        }
        Err(code) => code,
    }
}

#[cfg(not(feature = "gui"))]
fn gui(_: &[OsString]) -> ExitCode {
    eprintln!("kalem: this build has no graphical editor; use `kalem tui FILE`");
    ExitCode::from(2)
}

#[cfg(not(feature = "tui"))]
fn tui(_: &[OsString]) -> ExitCode {
    eprintln!("kalem: this build has no terminal editor; use `kalem gui FILE`");
    ExitCode::from(2)
}

#[cfg(feature = "tui")]
fn tui(args: &[OsString]) -> ExitCode {
    if args.first().and_then(|a| a.to_str()) == Some("--detect") {
        return match kalem_tui::detect() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("kalem: {e}");
                ExitCode::FAILURE
            }
        };
    }
    if matches!(args.first().and_then(|a| a.to_str()), Some("-h" | "--help")) {
        println!(
            "Usage: kalem tui [FILE]\n       kalem tui --detect   print the terminal's capabilities"
        );
        return ExitCode::SUCCESS;
    }
    let path = match one_path(args) {
        Ok(p) => p,
        Err(code) => return code,
    };
    match kalem_tui::run(path.as_deref()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kalem: {e}");
            ExitCode::FAILURE
        }
    }
}
