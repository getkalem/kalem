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

/// Kalem: a fast editor for plain-text documents, shown as they read and
/// kept byte for byte: Org, LaTeX, CSV, BibTeX and code.
#[derive(Debug, Parser)]
#[command(
    name = "kalem",
    version,
    about = "Kalem: a fast, text-first editor that opens every plain-text format as itself and keeps it byte for byte: Org, Markdown, LaTeX, CSV, BibTeX and code",
    long_about = None,
    // The editors, which `kalem` starts before these commands are read.
    override_usage = "kalem [FILE | FOLDER]      the editor: graphical where there is a display, else in the terminal
       kalem gui [FILE]           the graphical editor
       kalem tui [FILE]           the terminal editor (also kalem -t [FILE])
       kalem tui --detect         what the terminal can do
       kalem <COMMAND>            a command-line tool"
)]
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
enum LspAction {
    /// The language plugins, and what serves FILE: its language, root and
    /// server, or why there is none.
    Status {
        /// A code file.
        file: Option<PathBuf>,
    },
    /// The diagnostics of the files' language servers, once they settle;
    /// exits 1 when there are errors.
    Check {
        /// Code files.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        /// Seconds to wait for the server.
        #[arg(long, default_value_t = 120)]
        wait: u64,
        /// Print the server's log (its messages and standard error) to
        /// standard error.
        #[arg(long)]
        log: bool,
    },
    /// Asks the server about a place: `hover`, `definition`,
    /// `references`, `symbols` or `format` (prints the formatted text).
    Ask {
        /// The request.
        request: String,
        /// A code file.
        file: PathBuf,
        /// LINE:COLUMN, from 1.
        #[arg(default_value = "1:1")]
        place: String,
        /// Seconds to wait for the server.
        #[arg(long, default_value_t = 120)]
        wait: u64,
    },
}

#[derive(Debug, Subcommand)]
enum LatexAction {
    /// Build the PDF of a LaTeX document: its project's root document,
    /// with `latexmk` or the engine it names, and the problems of the log
    /// at their files and lines; exits 1 when LaTeX reports an error.
    Build {
        /// A LaTeX file of the project.
        file: PathBuf,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        /// The engine (`pdflatex`, `xelatex`, `lualatex`), instead of the
        /// one the document asks for.
        #[arg(long)]
        engine: Option<String>,
        /// Where the output goes, relative to the root document.
        #[arg(long)]
        outdir: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum BookAction {
    /// Build the Book as a static site: every chapter of `index.org`
    /// exported to HTML in the theme's page, with a search index.
    Build {
        /// The Book's folder.
        #[arg(default_value = "book")]
        dir: PathBuf,
        /// Where the site goes.
        #[arg(long, default_value = "target/book")]
        out: PathBuf,
    },
    /// Check the Book: every chapter exports, and every link inside it
    /// leads to one of its pages.
    Check {
        /// The Book's folder.
        #[arg(default_value = "book")]
        dir: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show a file that is not text through the viewer that opens it
    /// (design §11.13): a unit as PNG, or its text and information.
    View {
        /// The file.
        file: PathBuf,
        /// The unit (page, frame), from 1.
        #[arg(long, default_value_t = 1)]
        unit: usize,
        /// What to write.
        #[arg(long, value_enum, default_value_t = ViewFormat::Txt)]
        to: ViewFormat,
        /// Where to write the PNG (standard output otherwise).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Print the syntax tree of a file.
    Parse {
        /// The Org, Markdown, LaTeX or Kalem file to parse.
        file: PathBuf,
    },
    /// Check files: syntax diagnostics and round-trip verification (Org,
    /// Markdown, LaTeX, Kalem, CSV and BibTeX files).
    Check {
        /// Files to check; a folder stands for those under it.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        /// Exit with status 1 when there are warnings.
        #[arg(long)]
        deny_warnings: bool,
        /// For LaTeX files, also list the commands and environments the
        /// editor shows as source, most frequent first.
        #[arg(long)]
        unrendered: bool,
    },
    /// List the commands, one a line: ID, title, scope and keys; with
    /// `--type`, those that serve that text type (`org`, `python`, `csv`).
    Commands {
        /// A text type.
        #[arg(long = "type")]
        text_type: Option<String>,
    },
    /// Print the completions at a place in a file, one a line: label,
    /// kind and the completer (`kalem complete notes.org:12:5`).
    Complete {
        /// `FILE:LINE:COLUMN` (1-based; the column in characters).
        place: String,
    },
    /// Align tables and tags, and blank lines as each file has them; the
    /// Kalem format's canonical form.
    Fmt {
        /// Org, LaTeX or Kalem format files to format in place.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Change nothing; list the files that would change and fail if
        /// there are any (for CI).
        #[arg(long)]
        check: bool,
        /// LaTeX: line up the `&` of tables and alignments.
        #[arg(long)]
        align: bool,
        /// Kalem format: format ill-formed files too, as the parser
        /// recovers them (RFC 0003 §15), and print what changed.
        #[arg(long)]
        repair: bool,
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
    /// Convert Word, OpenDocument, Markdown, HTML, EPUB or RTF files to
    /// Org through pandoc, cleaned up: `kalem import report.docx` writes
    /// `report.org` beside it, its pictures in `report_assets`.
    Import {
        /// Files to convert.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Where to write (one input file only); `-` for standard output.
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// Replace an Org file that exists.
        #[arg(long)]
        force: bool,
    },
    /// Compares the structure Kalem reads in LaTeX files with pandoc's
    /// LaTeX reader: headings, formulas, citations, footnotes, figures,
    /// tables, code blocks and list items (development).
    DiffPandoc {
        /// LaTeX files to compare.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Print how many files agree in each category.
        #[arg(long)]
        summary: bool,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// How much of a corpus of LaTeX sources the rendered view covers:
    /// by field (the folders under DIR), the share of the body shown as
    /// source, and the most frequent commands and environments.
    LatexCoverage {
        /// Folders of sources: DIR/FIELD/PAPER/*.tex.
        #[arg(required = true)]
        dirs: Vec<PathBuf>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        /// How many commands and environments to list.
        #[arg(long, default_value_t = 200)]
        top: usize,
    },
    /// Language servers: `kalem lsp status`, `kalem lsp check FILE`.
    Lsp {
        #[command(subcommand)]
        action: LspAction,
    },
    /// LaTeX documents: `kalem latex build FILE`.
    Latex {
        #[command(subcommand)]
        action: LatexAction,
    },
    /// The Book: `kalem book build`, `kalem book check`.
    Book {
        #[command(subcommand)]
        action: BookAction,
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
    /// Word, through pandoc.
    Docx,
    /// OpenDocument text, through pandoc.
    Odt,
    /// EPUB, through pandoc.
    Epub,
    /// Rich Text Format, through pandoc.
    Rtf,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum DumpFormat {
    /// The JSON format of `tests/emacs/dump.el`.
    EmacsJson,
}

/// Runs the command line with the given arguments.
/// What `kalem view` writes.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum ViewFormat {
    /// The unit's text and the file's information.
    Txt,
    /// The unit as a PNG picture.
    Png,
}

/// Installs the plugins bundled into the binary (D28): the image viewer
/// of getkalem/plugins, until components load (T3.1.12).
pub fn bundled_plugins() {
    kalem_core::viewer::register(std::sync::Arc::new(kalem_plugin_image_viewer::ImageViewer));
}

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
        Command::View {
            file,
            unit,
            to,
            output,
        } => commands::view(
            &file,
            unit,
            matches!(to, ViewFormat::Png),
            output.as_deref(),
        ),
        Command::Check {
            files,
            format,
            deny_warnings,
            unrendered,
        } => commands::check(
            &files,
            matches!(format, Format::Json),
            deny_warnings,
            unrendered,
        ),
        Command::Dump {
            file,
            format: DumpFormat::EmacsJson,
        } => commands::dump(&file),
        Command::Fmt {
            files,
            check,
            align,
            repair,
        } => commands::fmt(&files, check, align, repair),
        Command::Complete { place } => commands::complete(&place),
        Command::Commands { text_type } => commands::list_commands(text_type.as_deref()),
        Command::Book {
            action: BookAction::Build { dir, out },
        } => commands::book::build(&dir, &out),
        Command::Book {
            action: BookAction::Check { dir },
        } => commands::book::check(&dir),
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
                ExportTo::Docx => commands::Target::Pandoc(kalem_core::pandoc::Format::Docx),
                ExportTo::Odt => commands::Target::Pandoc(kalem_core::pandoc::Format::Odt),
                ExportTo::Epub => commands::Target::Pandoc(kalem_core::pandoc::Format::Epub),
                ExportTo::Rtf => commands::Target::Pandoc(kalem_core::pandoc::Format::Rtf),
            };
            commands::export(&files, to, output.as_deref(), body_only, subtree.as_deref())
        }
        Command::Import {
            files,
            output,
            force,
        } => commands::import(&files, output.as_deref(), force),
        Command::Table {
            action:
                TableAction::Recalc {
                    files,
                    iterate,
                    check,
                },
        } => commands::recalc(&files, iterate, check),
        Command::Query { args, format } => commands::query(&args, matches!(format, Format::Json)),
        Command::DiffPandoc {
            files,
            summary,
            format,
        } => commands::diff_pandoc(&files, summary, matches!(format, Format::Json)),
        Command::LatexCoverage { dirs, format, top } => {
            commands::latex_coverage(&dirs, matches!(format, Format::Json), top)
        }
        Command::Lsp { action } => match action {
            LspAction::Status { file } => commands::lsp::status(file.as_deref()),
            LspAction::Check {
                files,
                format,
                wait,
                log,
            } => commands::lsp::check(&files, matches!(format, Format::Json), wait, log),
            LspAction::Ask {
                request,
                file,
                place,
                wait,
            } => commands::lsp::at(&request, &file, &place, wait),
        },
        Command::Latex {
            action:
                LatexAction::Build {
                    file,
                    format,
                    engine,
                    outdir,
                },
        } => commands::latex_build(
            &file,
            matches!(format, Format::Json),
            engine.as_deref(),
            outdir.as_deref(),
        ),
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

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    #[test]
    fn help_names_the_editors_and_the_program() {
        let mut cmd = super::Cli::command();
        let help = cmd.render_long_help().to_string();
        for usage in [
            "kalem [FILE | FOLDER]",
            "kalem gui",
            "kalem tui",
            "kalem -t",
        ] {
            assert!(help.contains(usage), "{usage}");
        }
        cmd.build();
        let parse = cmd.find_subcommand_mut("parse").expect("parse");
        assert!(
            parse
                .render_long_help()
                .to_string()
                .contains("Usage: kalem parse")
        );
    }
}
