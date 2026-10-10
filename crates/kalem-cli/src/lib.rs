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
       kalem <COMMAND>            a command-line tool",
    after_help = "Environment:
  KALEM_CONFIG_DIR  the settings, keys and plugins (~/.config/kalem; %APPDATA%\\kalem on Windows)
  KALEM_STATE_DIR   the log, crash reports and caches (~/.local/state/kalem; %LOCALAPPDATA%\\kalem)
  KALEM_LOG         what the log keeps: `debug`, or per module (`kalem_core=debug,info`)"
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
    /// Builds the plugin in DIR (the current folder by default) with its
    /// functions' names kept, installs it, and builds and installs it
    /// again whenever its sources change, until stopped: a Kalem running
    /// opens files with the new build.
    Dev {
        /// The plugin's folder.
        dir: Option<std::path::PathBuf>,
    },
    /// Removes an installed plugin.
    Remove {
        /// Its ID.
        id: String,
    },
    /// Turns on again a plugin Kalem turned off after it stopped three
    /// times (a trap, its time or its memory spent).
    Enable {
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
        /// The engine (`pdflatex`, `xelatex`, `lualatex`, `tectonic`),
        /// instead of the one the document asks for.
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
    /// Show a file that is not text through the viewer that opens it: a
    /// unit as PNG, or its text and information.
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
        /// The Org, Markdown or LaTeX file to parse.
        file: PathBuf,
    },
    /// Check files: syntax diagnostics and round-trip verification (Org,
    /// Markdown, LaTeX, CSV and BibTeX files).
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
    #[command(hide = true)]
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
    /// source, and the most frequent commands and environments
    /// (development).
    #[command(hide = true)]
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
    /// `check`, `remove ID`, `enable ID`, `new NAME`, `build [DIR]`,
    /// `dev [DIR]`.
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    /// Language servers: `kalem lsp status`, `kalem lsp check FILE`,
    /// `kalem lsp ask REQUEST FILE [LINE:COLUMN]`.
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
        /// Org files.
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,
        /// The match string, as Emacs's tags and property matches write it.
        #[arg(value_name = "MATCH")]
        matcher: String,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Dump the parse tree in the JSON of `tests/emacs/dump.el`
    /// (development).
    #[command(hide = true)]
    Dump {
        /// The Org file to dump.
        file: PathBuf,
        /// Output format.
        #[arg(long, value_enum, default_value_t = DumpFormat::EmacsJson)]
        format: DumpFormat,
    },
    /// Compare the parse with Emacs's org-element (development).
    #[command(hide = true)]
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

/// What `kalem view` writes.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum ViewFormat {
    /// The unit's text and the file's information.
    Txt,
    /// The unit as a PNG picture.
    Png,
}

/// Installs the plugins built into the binary: the components
/// getkalem/plugins released for pictures, workbooks and PDF files
/// (wasm_todo W9); then the component viewers the user installed, used in
/// place of a built-in one when newer, and the extension plugins
/// installed.
pub fn bundled_plugins() {
    #[cfg(feature = "plugins")]
    {
        choose_viewers(true);
        extensions::load();
    }
}

/// Chooses the viewers again ([`choose_viewers`]): after a plugin was
/// installed, updated or removed, or turned off or on again.
pub fn plugins_changed() {
    #[cfg(feature = "plugins")]
    choose_viewers(false);
}

/// For an editor: watches the installed plugins while it runs
/// ([`kalem_core::plugin_store::stamp`], once a second) and chooses the
/// viewers again once a change has settled, so that a plugin installed or
/// updated while Kalem runs, from it or from a terminal, opens its files
/// without a restart.
pub fn watch_plugins() {
    #[cfg(feature = "plugins")]
    {
        static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let mut chosen_at = kalem_core::plugin_store::stamp();
        let mut seen = chosen_at;
        let _ = std::thread::Builder::new()
            .name("kalem-plugins-watch".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    // The same for a second: an install is a rename, a
                    // record and a removal, not to be chosen from halfway.
                    let now = kalem_core::plugin_store::stamp();
                    if now == seen && now != chosen_at {
                        chosen_at = now;
                        choose_viewers(false);
                    }
                    seen = now;
                }
            });
    }
}

/// The plugin host the component viewers share, made on first need.
#[cfg(feature = "plugins")]
fn viewer_host() -> Option<std::sync::Arc<kalem_script::Host>> {
    static HOST: std::sync::OnceLock<Option<std::sync::Arc<kalem_script::Host>>> =
        std::sync::OnceLock::new();
    HOST.get_or_init(|| {
        // Compiled components are kept in the state folder; or in the
        // folder `KALEM_COMPONENT_CACHE` names, which processes share, as
        // `kalem_components::viewer`'s: CI runs every test in a process of
        // its own, and a test with a state folder of its own compiled the
        // workbook's component again, taking the runner's cores from the
        // other tests (docs/ci_todo.md, C4).
        let cache = std::env::var_os("KALEM_COMPONENT_CACHE")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| kalem_core::logging::state_dir().map(|d| d.join("plugin-cache")));
        kalem_script::Host::new(cache).ok().map(std::sync::Arc::new)
    })
    .clone()
}

/// The manifests of the plugins built into Kalem as components (wasm_todo
/// W9): none without the feature `components`.
#[cfg(feature = "plugins")]
pub fn embedded_components() -> Vec<serde_json::Value> {
    kalem_components::components()
        .iter()
        .map(kalem_components::Component::manifest_json)
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
            let id = short(c.id);
            let bundled = native_viewers().iter().find(|b| b.id() == id).cloned();
            let m = c.manifest_json();
            let hook = on_stop(
                c.id,
                m["name"].as_str().unwrap_or(c.id),
                m["version"].as_str().unwrap_or_default(),
            );
            std::sync::Arc::new(
                c.viewer(host.clone())
                    .with_fallback(
                        bundled,
                        std::sync::Arc::new(|text| kalem_core::jobs::notice(text, true)),
                    )
                    .with_on_stop(hook),
            )
        })
        .collect()
}

/// What a component viewer of plugin `id` (`name` at `version`) calls
/// when one of its documents stops (wasm_todo W8): the stop counted and
/// logged with the plugin's version; at the third, the plugin turned off
/// until it is updated, the user told, and its files given at once to
/// what opens them next ([`choose_viewers`]): a newer copy installed, the
/// copy built in, the native viewer, or nothing.
#[cfg(feature = "plugins")]
fn on_stop(id: &str, name: &str, version: &str) -> kalem_script::viewer::OnStop {
    let (id, name, version) = (id.to_string(), name.to_string(), version.to_string());
    std::sync::Arc::new(move |why| {
        let n = kalem_core::plugin_store::record_stop(&id, &version, &format!("{why:?}"));
        tracing::error!(plugin = %id, version = %version, stops = n, why = ?why, "a plugin stopped");
        if n == kalem_core::plugin_store::STOPS_TO_TURN_OFF {
            kalem_core::jobs::notice(turned_off(&id, &name, &version, n), true);
            choose_viewers(false);
        }
    })
}

/// What the user is told of plugin `id` turned off after `n` stops.
#[cfg(feature = "plugins")]
pub(crate) fn turned_off(id: &str, name: &str, version: &str, n: u32) -> String {
    kalem_core::tr!(
        "plugin-turned-off",
        plugin = name,
        version = version,
        count = n,
        id = id
    )
}

/// A plugin's viewer by its id: `org.kalem.xlsx` is the viewer `xlsx`.
#[cfg(feature = "plugins")]
fn short(id: &str) -> &str {
    id.rsplit('.').next().unwrap_or(id)
}

/// The viewers registered before any component: the native ones a
/// component takes the place of, which open its files when it cannot.
/// Read once, before the first component is registered.
#[cfg(feature = "plugins")]
fn native_viewers() -> &'static [std::sync::Arc<dyn kalem_viewer::Viewer>] {
    static NATIVE: std::sync::OnceLock<Vec<std::sync::Arc<dyn kalem_viewer::Viewer>>> =
        std::sync::OnceLock::new();
    NATIVE.get_or_init(kalem_core::viewer::viewers)
}

/// The components built in, each with its viewer, made once.
#[cfg(feature = "plugins")]
fn embedded() -> &'static [(
    &'static kalem_components::Component,
    std::sync::Arc<kalem_script::viewer::ComponentViewer>,
)] {
    static EMBEDDED: std::sync::OnceLock<
        Vec<(
            &'static kalem_components::Component,
            std::sync::Arc<kalem_script::viewer::ComponentViewer>,
        )>,
    > = std::sync::OnceLock::new();
    EMBEDDED.get_or_init(|| {
        kalem_components::components()
            .iter()
            .zip(embedded_viewers())
            .collect()
    })
}

/// [`installed_viewers`], a copy made before kept as it was (with what it
/// compiled), so that choosing again costs nothing.
#[cfg(feature = "plugins")]
fn installed_kept() -> Vec<(
    kalem_core::plugin_store::Installed,
    Result<std::sync::Arc<kalem_script::viewer::ComponentViewer>, String>,
)> {
    type Made = (
        String,
        String,
        PathBuf,
        std::sync::Arc<kalem_script::viewer::ComponentViewer>,
    );
    static MADE: std::sync::Mutex<Vec<Made>> = std::sync::Mutex::new(Vec::new());
    let mut made = MADE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    installed_viewers()
        .into_iter()
        .map(|(p, v)| {
            let v = v.map(|v| {
                match made
                    .iter()
                    .find(|m| m.0 == p.id && m.1 == p.version && m.2 == p.dir)
                {
                    Some(m) => m.3.clone(),
                    None => {
                        made.push((p.id.clone(), p.version.clone(), p.dir.clone(), v.clone()));
                        v
                    }
                }
            });
            (p, v)
        })
        .collect()
}

/// Chooses, for each plugin with a viewer component, what opens its
/// files, and registers it: a copy the user installed, when it is newer
/// than the one built in and not turned off; else the copy built in, when
/// not turned off; else the native viewer it took the place of; else
/// nothing. A viewer chosen again is the same one, with what it compiled.
/// At startup the user is told of a copy turned off with nothing in its
/// place; afterwards (an install, an update, a removal, a plugin turned
/// off or on again) of the copy that opens the files now; once, of a copy
/// built for another plugin API. Nothing is compiled here: a thread
/// compiles what is new, or reads it from the cache in the state
/// directory, so the first file it opens does not wait.
#[cfg(feature = "plugins")]
fn choose_viewers(startup: bool) {
    use kalem_core::plugin_store;
    use std::sync::Arc;
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // The copies built for another API the user was told of.
    static TOLD: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let natives = native_viewers();
    let embedded = embedded();
    let installed = installed_kept();
    let mut ids: Vec<&str> = embedded.iter().map(|(c, _)| short(c.id)).collect();
    for (p, _) in &installed {
        if !ids.contains(&short(&p.id)) {
            ids.push(short(&p.id));
        }
    }
    let mut loaded = Vec::new();
    for id in ids {
        // The viewer chosen, its plugin's name and version; and what the
        // user is told when a copy that would open the files is off.
        let mut chosen: Option<(Arc<kalem_script::viewer::ComponentViewer>, String, String)> = None;
        let mut off = None;
        if let Some((p, v)) = installed.iter().find(|(p, _)| short(&p.id) == id)
            && embedded_is_newer(p).is_none()
        {
            if plugin_store::turned_off(&p.id, &p.version) {
                let n = plugin_store::stops(&p.id, &p.version);
                off = Some(turned_off(&p.id, &p.name, &p.version, n));
            } else {
                match v {
                    Ok(v) => chosen = Some((v.clone(), p.name.clone(), p.version.clone())),
                    // Built for another API: the copy built in opens its
                    // files.
                    Err(why) => {
                        let key = (p.id.clone(), p.version.clone());
                        let mut told = TOLD
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if !told.contains(&key) {
                            told.push(key);
                            kalem_core::jobs::notice(why.clone(), true);
                        }
                    }
                }
            }
        }
        if chosen.is_none()
            && let Some((c, v)) = embedded.iter().find(|(c, _)| short(c.id) == id)
        {
            let m = c.manifest_json();
            let name = m["name"].as_str().unwrap_or(c.id);
            let version = m["version"].as_str().unwrap_or_default();
            if plugin_store::turned_off(c.id, version) {
                let n = plugin_store::stops(c.id, version);
                off.get_or_insert_with(|| turned_off(c.id, name, version, n));
            } else {
                chosen = Some((v.clone(), name.to_string(), version.to_string()));
            }
        }
        if startup
            && chosen.is_none()
            && let Some(off) = off
        {
            kalem_core::jobs::notice(off, true);
        }
        let new: Option<Arc<dyn kalem_viewer::Viewer>> = match &chosen {
            Some((v, ..)) => Some(v.clone()),
            None => natives.iter().find(|n| n.id() == id).cloned(),
        };
        let now = kalem_core::viewer::viewers()
            .into_iter()
            .find(|v| v.id() == id);
        let same = match (&now, &new) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if same {
            continue;
        }
        match &new {
            Some(v) => kalem_core::viewer::register(v.clone()),
            None => kalem_core::viewer::unregister(id),
        }
        if let Some((v, name, version)) = chosen {
            if !startup {
                kalem_core::jobs::notice(
                    kalem_core::tr!("plugin-now-used", plugin = name, version = version),
                    false,
                );
            }
            loaded.push(v);
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
    let m = embedded_components()
        .into_iter()
        .find(|m| m["id"].as_str() == Some(p.id.as_str()))?;
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
/// viewer (not compiled yet), which falls back to the built-in component
/// of the same name when it cannot run; or why it is not tried (its manifest
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
        let id = short(&p.id).to_string();
        // What opens the files when the component cannot run: the copy
        // built in, unless it is turned off, else the native viewer.
        let bundled: Option<std::sync::Arc<dyn kalem_viewer::Viewer>> = embedded()
            .iter()
            .find(|(c, _)| {
                short(c.id) == id
                    && !kalem_core::plugin_store::turned_off(
                        c.id,
                        c.manifest_json()["version"].as_str().unwrap_or_default(),
                    )
            })
            .map(|(_, v)| -> std::sync::Arc<dyn kalem_viewer::Viewer> { v.clone() })
            .or_else(|| native_viewers().iter().find(|v| v.id() == id).cloned());
        let v = std::sync::Arc::new(
            ComponentViewer::new(h, p.dir.join(main), id, &p.name, &opens, limits)
                .with_fallback(
                    bundled,
                    std::sync::Arc::new(|text| kalem_core::jobs::notice(text, true)),
                )
                .with_on_stop(on_stop(&p.id, &p.name, &p.version)),
        );
        out.push((p, Ok(v)));
    }
    out
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
        Command::Query {
            files,
            matcher,
            format,
        } => commands::query(&files, &matcher, matches!(format, Format::Json)),
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
            PluginAction::Dev { dir } => commands::plugin::dev(dir.as_deref()),
            PluginAction::Remove { id } => commands::plugin::remove(&id),
            PluginAction::Enable { id } => commands::plugin::enable(&id),
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
