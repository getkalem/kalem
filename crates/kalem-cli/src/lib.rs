//! Command-line subcommands and batch mode for the Kalem editor.
//!
//! The `kalem` binary delegates to [`run`]. Every subcommand works without
//! a display, so Kalem can be used in scripts and CI.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

mod commands;

/// Kalem: a lightweight editor for Org mode files.
#[derive(Debug, Parser)]
#[command(name = "kalem", version, about = "Kalem: a lightweight editor for Org mode files", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum TableAction {
    /// Recalculate every table that has a `#+TBLFM` line, as Emacs does
    /// with `org-table-recalculate-buffer-tables`, and align it.
    Recalc {
        /// Org files to recalculate in place.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Recalculate until the tables no longer change (at most ten
        /// times), as `C-u C-u C-c *`.
        #[arg(long)]
        iterate: bool,
        /// Change nothing; list the files whose tables would change and
        /// fail if there are any (for CI).
        #[arg(long)]
        check: bool,
    },
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the syntax tree of a file.
    Parse {
        /// The Org file to parse.
        file: PathBuf,
    },
    /// Check files: syntax diagnostics and round-trip verification.
    Check {
        /// Org files to check.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        /// Exit with status 1 when there are warnings.
        #[arg(long)]
        deny_warnings: bool,
    },
    /// Align tables and tags, and blank lines as each file has them.
    Fmt {
        /// Org files to format in place.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Change nothing; list the files that would change and fail if
        /// there are any (for CI).
        #[arg(long)]
        check: bool,
    },
    /// Export Org files as Emacs's Org exporter does: `kalem export
    /// notes.org --to html` writes `notes.html` beside it (or the file
    /// `#+EXPORT_FILE_NAME` names).
    Export {
        /// Org files to export.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// The format.
        #[arg(long, value_enum, default_value_t = ExportTo::Html)]
        to: ExportTo,
        /// Where to write (one input file only); `-` for standard output.
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// Only the document's body, without the page around it.
        #[arg(long)]
        body_only: bool,
        /// Only the subtree of this headline (its title, or `#` and its
        /// `CUSTOM_ID`), with its `EXPORT_` properties, as `C-c C-e C-s`.
        #[arg(long, value_name = "HEADLINE")]
        subtree: Option<String>,
        /// LaTeX: a `%% org:LINE` comment before each element, giving the
        /// line of the Org file it comes from.
        #[arg(long)]
        source_lines: bool,
    },
    /// Table formulas: `kalem table recalc FILE...`.
    Table {
        #[command(subcommand)]
        action: TableAction,
    },
    /// Print the headlines matching an Org match string, such as
    /// `kalem query notes.org 'TODO="NEXT"+work'`.
    Query {
        /// Org files, then the match string.
        #[arg(required = true, num_args = 2.., value_name = "FILE... MATCH")]
        args: Vec<String>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Dump the parse tree in a machine-readable format.
    Dump {
        /// The Org file to dump.
        file: PathBuf,
        /// Output format.
        #[arg(long, value_enum, default_value_t = DumpFormat::EmacsJson)]
        format: DumpFormat,
    },
    /// Compare the parse with Emacs's org-element (development tool).
    DiffEmacs {
        /// Org files to compare.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Directory with precomputed dumps (`FILE.json`), instead of
        /// running Emacs.
        #[arg(long)]
        emacs_dumps: Option<PathBuf>,
        /// Path to `dump.el` (default: search `tests/emacs/dump.el` upwards
        /// from the current directory).
        #[arg(long)]
        dump_el: Option<PathBuf>,
        /// The Emacs executable.
        #[arg(long, default_value = "emacs")]
        emacs: String,
        /// How many differences to print per file.
        #[arg(long, default_value_t = 20)]
        show: usize,
        /// Only print the summary.
        #[arg(long)]
        summary: bool,
        /// Compare the document model (tags, properties, TODO sets,
        /// matches) through `tests/emacs/model.el` instead of the syntax
        /// tree.
        #[arg(long)]
        model: bool,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ExportTo {
    /// HTML.
    Html,
    /// Markdown, as Emacs's `ox-md` writes it (tables as HTML).
    Md,
    /// GitHub Flavored Markdown: pipe tables, fenced code, `~~strike~~`.
    Gfm,
    /// Strict Org: a Kalem document without Kalem's additions.
    Org,
    /// LaTeX, as Emacs's `ox-latex` writes it.
    Latex,
    /// Plain text, as Emacs's `ox-ascii` writes it.
    Txt,
    /// Plain text with UTF-8 lines, bullets and quotes.
    Utf8,
    /// PDF through LaTeX (`latexmk`, the TeX engine or `tectonic`).
    Pdf,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum DumpFormat {
    /// The JSON format of `tests/emacs/dump.el`.
    EmacsJson,
}

/// Runs the command line with the given arguments.
pub fn run<I, T>(args: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return if e.use_stderr() {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    let result = match cli.command {
        Command::Parse { file } => commands::parse(&file),
        Command::Check {
            files,
            format,
            deny_warnings,
        } => commands::check(&files, matches!(format, Format::Json), deny_warnings),
        Command::Dump {
            file,
            format: DumpFormat::EmacsJson,
        } => commands::dump(&file),
        Command::Fmt { files, check } => commands::fmt(&files, check),
        Command::Export {
            files,
            to,
            output,
            body_only,
            subtree,
            source_lines,
        } => {
            let to = match to {
                ExportTo::Html => commands::Target::Html,
                ExportTo::Md => commands::Target::Markdown,
                ExportTo::Gfm => commands::Target::Gfm,
                ExportTo::Org => commands::Target::Org,
                ExportTo::Latex if source_lines => commands::Target::LatexLines,
                ExportTo::Latex => commands::Target::Latex,
                ExportTo::Txt => commands::Target::Text,
                ExportTo::Utf8 => commands::Target::Utf8,
                ExportTo::Pdf => commands::Target::Pdf,
            };
            commands::export(&files, to, output.as_deref(), body_only, subtree.as_deref())
        }
        Command::Table {
            action:
                TableAction::Recalc {
                    files,
                    iterate,
                    check,
                },
        } => commands::recalc(&files, iterate, check),
        Command::Query { args, format } => commands::query(&args, matches!(format, Format::Json)),
        Command::DiffEmacs {
            files,
            emacs_dumps,
            dump_el,
            emacs,
            show,
            summary,
            model,
            format,
        } => {
            let opts = commands::DiffOptions {
                emacs_dumps,
                dump_el,
                emacs,
                show,
                summary,
                json: matches!(format, Format::Json),
            };
            if model {
                commands::diff_model(&files, &opts)
            } else {
                commands::diff_emacs(&files, &opts)
            }
        }
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("kalem: {e}");
            ExitCode::from(2)
        }
    }
}
