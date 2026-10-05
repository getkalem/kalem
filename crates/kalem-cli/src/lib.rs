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
#[cfg(feature = "plugins")]
mod extensions;

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
enum PluginAction {
    /// The plugins of the index (`plugins.index`), and which are installed.
    Browse,
    /// Installs a plugin: a name from the index (`elixir`), a GitHub
    /// folder or repository link, an archive link, or a folder or archive
    /// on disk. Shows what it is and asks first.
    Install {
        /// The plugin.
        source: String,
        /// Install without asking.
        #[arg(long, short)]
        yes: bool,
    },
    /// The installed plugins.
    List,
    /// Starts every installed component viewer once, as opening a file
    /// would; exits 1 when one cannot run with this Kalem (built for
    /// another version of the plugin API).
    Check,
    /// Starts a plugin: in a checkout of getkalem/plugins its template as
    /// `plugins/NAME`, elsewhere a crate of its own in `NAME/`.
    New {
        /// Its name: lower-case letters, digits and hyphens.
        name: String,
    },
    /// Builds the plugin in DIR (the current folder by default): Cargo
    /// compiles it for wasm32-unknown-unknown, and the module becomes the
    /// component `main` names in plugin.json.
    Build {
        /// The plugin's folder.
        dir: Option<std::path::PathBuf>,
    },
    /// Removes an installed plugin.
    Remove {
        /// Its ID.
        id: String,
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
    /// `references`, `symbols`, `signature`, `completion` (as typing there would) or
    /// `format` (prints the formatted text).
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
        /// Instead: the chapters `book/chapters.toml` maps to code changed
        /// since this Git revision, which have to change too.
        #[arg(long, value_name = "BASE")]
        changed: Option<String>,
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
    /// Align tables and tags, and blank lines as each file has them.
    Fmt {
        /// Org and LaTeX files, and code whose language has a formatter, to
        /// format in place; other files are left as they are, with a note.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Change nothing; list the files that would change and fail if
        /// there are any (for CI).
        #[arg(long)]
        check: bool,
        /// LaTeX: line up the `&` of tables and alignments.
        #[arg(long)]
        align: bool,
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
    /// Plugins: `kalem plugin browse`, `install NAME|URL|PATH`, `list`,
    /// `check`, `remove ID`, `new NAME`, `build [DIR]`.
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
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
    /// Strict Org, without what earlier versions of Kalem added to it.
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

/// Installs the plugins bundled into the binary (D28): the viewers of
/// getkalem/plugins for pictures, workbooks and PDF files; then the
/// component viewers the user installed, which take the place of a
/// bundled viewer of the same name, and the extension plugins installed.
pub fn bundled_plugins() {
    #[cfg(feature = "viewers")]
    {
        kalem_core::viewer::register(std::sync::Arc::new(kalem_plugin_image_viewer::ImageViewer));
        kalem_core::viewer::register(std::sync::Arc::new(kalem_plugin_xlsx::XlsxViewer));
        kalem_core::viewer::register(std::sync::Arc::new(kalem_plugin_pdf_viewer::PdfViewer));
    }
    #[cfg(feature = "plugins")]
    {
        component_viewers();
        extensions::load();
    }
}

/// The plugin host the component viewers share, made on first need.
#[cfg(feature = "plugins")]
fn viewer_host() -> Option<std::sync::Arc<kalem_script::Host>> {
    static HOST: std::sync::OnceLock<Option<std::sync::Arc<kalem_script::Host>>> =
        std::sync::OnceLock::new();
    HOST.get_or_init(|| {
        let cache = kalem_core::logging::state_dir().map(|d| d.join("plugin-cache"));
        kalem_script::Host::new(cache).ok().map(std::sync::Arc::new)
    })
    .clone()
}

/// The bundled plugins built into Kalem as components (wasm_todo W5), each
/// with its manifest: none without the feature `components`.
#[cfg(feature = "plugins")]
pub fn embedded_components() -> Vec<(serde_json::Value, &'static [u8])> {
    kalem_components::components()
        .iter()
        .map(|c| (c.manifest_json(), c.bytes))
        .collect()
}

/// The embedded components as viewers, each in the place of the native
/// viewer of the same name, which opens its files should it not run.
#[cfg(feature = "plugins")]
pub(crate) fn embedded_viewers() -> Vec<std::sync::Arc<kalem_script::viewer::ComponentViewer>> {
    let Some(host) = viewer_host() else {
        return Vec::new();
    };
    kalem_components::components()
        .iter()
        .map(|c| {
            let id = c.id.rsplit('.').next().unwrap_or(c.id);
            let bundled = kalem_core::viewer::viewers()
                .into_iter()
                .find(|b| b.id() == id);
            std::sync::Arc::new(c.viewer(host.clone()).with_fallback(
                bundled,
                std::sync::Arc::new(|text| kalem_core::jobs::notice(text, true)),
            ))
        })
        .collect()
}

/// The component viewers installed (`kalem plugin install` of a built
/// viewer), registered from their manifests: a plugin with a `main`
/// component that `opens` files. Nothing is compiled here: a thread
/// compiles them, or reads them from the cache in the state directory, so
/// the first file they open does not wait; without any, no engine starts.
#[cfg(feature = "plugins")]
fn component_viewers() {
    let mut loaded = Vec::new();
    // The bundled components first, then the installed ones, which take
    // their place only when newer.
    let embedded = embedded_viewers();
    for v in &embedded {
        kalem_core::viewer::register(v.clone());
        loaded.push(v.clone());
    }
    for (p, v) in installed_viewers() {
        // `kalem plugin list` says why it is not used.
        if embedded_is_newer(&p).is_some() {
            continue;
        }
        match v {
            Ok(v) => {
                kalem_core::viewer::register(v.clone());
                loaded.push(v);
            }
            // Built for another API: not tried, the bundled viewer of the
            // same name opens its files, and the user is told.
            Err(why) => kalem_core::jobs::notice(why, true),
        }
    }
    if !loaded.is_empty() {
        let _ = std::thread::Builder::new()
            .name("kalem-plugins-load".into())
            .spawn(move || {
                for v in loaded {
                    let _ = v.plugin();
                }
            });
    }
}

/// When Kalem has `p` built in at the same or a later version: why the
/// installed copy is not used.
#[cfg(feature = "plugins")]
pub(crate) fn embedded_is_newer(p: &kalem_core::plugin_store::Installed) -> Option<String> {
    let (m, _) = embedded_components()
        .into_iter()
        .find(|(m, _)| m["id"].as_str() == Some(p.id.as_str()))?;
    let built_in = m["version"].as_str().unwrap_or_default().to_string();
    (!kalem_core::plugin_store::newer(&p.version, &built_in)).then(|| {
        kalem_core::tr!(
            "plugin-built-in-newer",
            name = p.name.clone(),
            version = p.version.clone(),
            built_in = built_in
        )
    })
}

/// The installed plugins that are component viewers, each with its
/// viewer (not compiled yet), which falls back to the bundled viewer of
/// the same name when it cannot run; or why it is not tried (its manifest
/// names another version of the plugin API).
#[cfg(feature = "plugins")]
pub(crate) fn installed_viewers() -> Vec<(
    kalem_core::plugin_store::Installed,
    Result<std::sync::Arc<kalem_script::viewer::ComponentViewer>, String>,
)> {
    use kalem_script::viewer::ComponentViewer;
    let mut out = Vec::new();
    for p in kalem_core::plugin_store::installed() {
        let Ok(text) = std::fs::read_to_string(p.dir.join("plugin.json")) else {
            continue;
        };
        let Ok(m) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(main) = m["main"].as_str() else {
            continue;
        };
        let opens = kalem_components::opens(&m);
        if opens.is_empty() {
            continue;
        }
        let api = m["api"].as_str();
        if !kalem_script::api_compatible(api) {
            let why = kalem_core::tr!(
                "plugin-api-mismatch",
                name = p.name.clone(),
                version = p.version.clone(),
                api = api.unwrap_or_default().to_string(),
                ours = kalem_script::API_VERSION
            );
            out.push((p, Err(why)));
            continue;
        }
        let Some(h) = viewer_host() else {
            return out;
        };
        let limits = kalem_components::limits(&m);
        // `org.kalem.pdf-viewer` is the viewer `pdf-viewer`, replacing
        // the bundled one.
        let id = p.id.rsplit('.').next().unwrap_or(&p.id).to_string();
        // The bundled viewer it replaces opens the files when the
        // component cannot run (built for another version of the API).
        let bundled = kalem_core::viewer::viewers()
            .into_iter()
            .find(|v| v.id() == id);
        let v = std::sync::Arc::new(
            ComponentViewer::new(h, p.dir.join(main), id, &p.name, &opens, limits).with_fallback(
                bundled,
                std::sync::Arc::new(|text| kalem_core::jobs::notice(text, true)),
            ),
        );
        out.push((p, Ok(v)));
    }
    out
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
        } => commands::fmt(&files, check, align),
        Command::Complete { place } => commands::complete(&place),
        Command::Commands { text_type } => commands::list_commands(text_type.as_deref()),
        Command::Book {
            action: BookAction::Build { dir, out },
        } => commands::book::build(&dir, &out),
        Command::Book {
            action: BookAction::Check { dir, changed },
        } => match changed {
            Some(base) => commands::book::check_changed(&dir, &base),
            None => commands::book::check(&dir),
        },
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
        Command::Plugin { action } => match action {
            PluginAction::Browse => commands::plugin::browse(),
            PluginAction::Install { source, yes } => commands::plugin::install(&source, yes),
            PluginAction::List => commands::plugin::list(),
            PluginAction::Check => commands::plugin::check(),
            PluginAction::New { name } => commands::plugin::new(&name),
            PluginAction::Build { dir } => commands::plugin::build(dir.as_deref()),
            PluginAction::Remove { id } => commands::plugin::remove(&id),
        },
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
