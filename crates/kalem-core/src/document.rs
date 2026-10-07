//! The state of an open document: text, parse, model, selection, history
//! and metadata (T1.3.1).
//!
//! Edits reparse incrementally at once. An edit that needs a full parse
//! (an in-buffer setting changed, for example) starts it on a background
//! thread; until it arrives the old tree stays available together with the
//! edits made since, so a view can map its positions (T1.3.1b).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Instant;

use org_edit::{ChangeKind, EditError, History, Selection, Transaction};
use org_model::{Document, ModelCache, Settings};
use org_syntax::{Parse, ParseContext, ReparseLevel, SetupFileLoader};

use crate::files::{self, DiskChange, DiskState, OpenError, SaveOptions};
use crate::mode::DocumentMode;
use crate::text::Text;
use crate::when::{Context, Value};
use unicode_segmentation::UnicodeSegmentation;

/// Line endings of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    /// `\n`
    Lf,
    /// `\r\n`
    CrLf,
    /// `\r` alone (classic Mac OS): the text holds `\n`, each written as
    /// `\r`, as Emacs decodes the `mac` end-of-line type.
    Cr,
}

/// What is known about a document besides its text.
#[derive(Debug, Clone)]
pub struct Metadata {
    /// The file, if the document has one.
    pub path: Option<PathBuf>,
    /// The editing mode.
    pub mode: DocumentMode,
    /// The file's line endings, kept on save.
    pub line_ending: LineEnding,
    /// The file started with a byte order mark (UTF-8, or UTF-16).
    pub bom: bool,
    /// The file's character encoding, kept on save: UTF-8, UTF-16 (with a
    /// byte order mark) or a legacy encoding such as Windows-1254.
    pub encoding: &'static encoding_rs::Encoding,
    /// Some bytes could not be read in the encoding and became U+FFFD:
    /// saving writes U+FFFD in their place.
    pub lossy: bool,
}

/// Starts a full parse of `text` (version `version`) in the background,
/// with the configuration of `base`.
fn spawn_parse(base: &Parse, text: &str, version: u64) -> Pending {
    let (send, result) = std::sync::mpsc::channel();
    let base = base.clone();
    let text = text.to_string();
    tracing::debug!(version, bytes = text.len(), "background parse started");
    std::thread::spawn(move || {
        let _ = send.send(base.parse_again(&text));
    });
    Pending { version, result }
}

/// A full parse running in the background.
#[derive(Debug)]
struct Pending {
    version: u64,
    result: Receiver<Parse>,
}

/// The Org side of a document.
#[derive(Debug)]
struct OrgState {
    parse: Parse,
    /// The text version the parse is for.
    parse_version: u64,
    /// Edits applied since, when the parse is behind the text, with the
    /// version each one made.
    since: Vec<(u64, Transaction)>,
    pending: Option<Pending>,
    cache: Arc<ModelCache>,
    model: Option<(u64, Arc<Document>)>,
    last_level: Option<ReparseLevel>,
}

/// A cell of a CSV grid being typed into or edited (`csv_edit`): its row
/// and column, and its record as it was, for Escape to put back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellEdit {
    /// The cell's row.
    pub row: usize,
    /// The cell's column.
    pub col: usize,
    /// The record's text before.
    pub record: String,
    /// Edit mode (F2): the arrows move in the cell; else Enter mode,
    /// where they go to another cell.
    pub edit: bool,
    /// The undo steps there were before the entry's first change: Escape
    /// undoes those after, so that Undo does not bring the entry back.
    pub steps: usize,
}

/// The mode of a CSV grid, as Excel's status bar names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellMode {
    /// A cell is selected: typing replaces it, the arrows move between
    /// cells.
    Ready,
    /// Typing into a cell: the arrows end it and move.
    Enter,
    /// Editing a cell's text (F2): the arrows move in it.
    Edit,
}

/// An open document.
#[derive(Debug)]
pub struct DocumentState {
    /// The saved version from before the file was deleted on disk: the
    /// document counts as unsaved meanwhile (its text is the only copy),
    /// and is saved again if the file comes back as it was.
    deleted_saved: Option<u64>,
    /// The user agreed to save a converted file in its own format
    /// ([`SaveError::Converted`]): asked once a document.
    pub conversion_accepted: bool,
    /// A number no other document of this run has: services that keep
    /// something per document (a language server's copy) follow it when
    /// its path changes.
    serial: u64,
    /// Edits are refused (Doom's `SPC t r`); the cursor still moves.
    pub read_only: bool,
    text: Text,
    /// Incremented by every change.
    version: u64,
    /// The version the file was last saved (or opened) at.
    saved_version: u64,
    org: Option<OrgState>,
    /// The parse of a LaTeX document.
    latex: Option<crate::latex_view::LatexState>,
    settings: Arc<Settings>,
    /// The selection.
    pub selection: Selection,
    /// More cursors and selections (multiple cursors, column selection),
    /// besides the primary [`DocumentState::selection`]: typing, deleting
    /// and pasting act at each (see `crate::cursors`).
    pub extra: Vec<Selection>,
    /// Expand Selection's steps, to go back with Shrink Selection: the
    /// selection before each, and the one it made.
    pub(crate) expansions: Vec<(Selection, Selection)>,
    history: History,
    /// File and mode information.
    pub meta: Metadata,
    /// The narrowed part of the text, if any (view state; commands see only
    /// this part).
    pub narrowing: Option<std::ops::Range<usize>>,
    /// Positions that move with the text: Vim's marks, jump list and
    /// change list.
    pub marks: crate::marks::Marks,
    /// The file as last read or written.
    disk: Option<DiskState>,
    /// Edits applied since the frontend last took them.
    changes: Vec<Transaction>,
    /// The file manager's state, in a folder listing
    /// ([`DocumentMode::Directory`]).
    pub dired: Option<Box<crate::dired::DirState>>,
    /// A file that is not text, opened by a viewer plugin
    /// ([`DocumentMode::Viewer`]).
    pub viewer: Option<Box<crate::viewer::ViewerState>>,
    /// A CSV document's filter (view state): only the rows with a field
    /// holding this text show (`crate::csv::filtered`).
    pub csv_filter: Option<String>,
    /// The filter matches the whole value of this column (a Frequency
    /// Table's choice), not text in any field.
    pub csv_filter_column: Option<usize>,
    /// A CSV document's filtered or sorted view as last worked out, kept
    /// through the edits since (`crate::csv::Frozen`).
    pub csv_frozen: std::cell::RefCell<Option<crate::csv::Frozen>>,
    /// A CSV document's view sorted by a column (view state), descending
    /// when `true`: the file keeps its order (`crate::csv::shown_lines`).
    pub csv_sort: Option<(usize, bool)>,
    /// A CSV document's dialect, detected when it is first laid out and
    /// kept (edits do not change it), or set by hand.
    pub csv_dialect: std::cell::Cell<Option<crate::csv::Dialect>>,
    /// The dialect was detected on one record or none, and is detected
    /// again once there are two (`crate::csv::settled`); one set by hand
    /// is not.
    pub csv_dialect_provisional: std::cell::Cell<bool>,
    /// How the grid shows a CSV document (view state): alignment,
    /// rainbow columns, the coordinate grid.
    pub csv_view: crate::csv::View,
    /// A CSV document's columns as the grid shows them (view state):
    /// hidden ones, widths set by hand, the first one frozen.
    pub csv_columns: crate::csv::Columns,
    /// The next paste into this CSV document writes over the cells from
    /// the cursor's down and to the right (Paste as Block), once.
    pub csv_paste_block: bool,
    /// A cell of a CSV grid selected past the end of its record (a short
    /// record's missing cell, an empty column right of the data): the
    /// cursor at the record's end, the version of the text and the
    /// column. It holds while neither moves (`csv_virtual_col`); typing
    /// there adds the fields up to it.
    pub csv_virtual: Option<(usize, u64, usize)>,
    /// The column of a selection's anchor in a CSV grid when it is such a
    /// cell past its record's end (Shift with an arrow from one): the
    /// anchor, the version of the text and the column (the selection's
    /// rectangle reached column A).
    pub csv_virtual_anchor: Option<(usize, u64, usize)>,
    /// A run of Tabs (and Shift+Tabs) along a CSV row: the cell it got to
    /// (row and column) and the column it started from, where Enter goes
    /// back to on the row below, as in Excel.
    pub csv_tab_run: Option<(usize, usize, usize)>,
    /// The CSV grid's cell being typed into or edited, as Excel's Enter
    /// and Edit modes; none in its Ready mode, where the cursor is a cell
    /// (`csv_mode`).
    pub csv_edit: Option<CellEdit>,
    /// Vim's keys edit this document (the editor's Vim layer is on): a
    /// CSV grid has no Excel modes then, typing inserts at the cursor as
    /// Vim's insert mode does (a delimiter or a quote still going into
    /// the value) and the cursor shows in the cell.
    pub csv_vim: bool,
    /// A BibTeX grid's sort: the column (`bibtex::COLUMNS`) and whether
    /// descending; the file keeps its order.
    pub bib_sort: Option<(usize, bool)>,
}

/// Why saving failed.
#[derive(Debug)]
pub enum SaveError {
    /// The document has no file; use [`DocumentState::save_as`].
    NoPath,
    /// Another program changed the file since it was read or saved; saving
    /// again with `force` overwrites it.
    ChangedOnDisk,
    /// Writing failed.
    Io(std::io::Error),
    /// A character of the text has no place in the file's encoding; Save
    /// with Encoding (UTF-8) writes it.
    Unencodable {
        /// The character.
        ch: char,
        /// The encoding's name.
        encoding: &'static str,
    },
    /// The file was read with replacement characters (some bytes are not
    /// in its encoding): saving over it would write � in their place.
    /// Save with Encoding and Save As write it as it shows.
    Lossy {
        /// The encoding's name.
        encoding: &'static str,
    },
    /// The file was converted as it opened (an `.ods` file edited as a
    /// workbook): saving it in its own format writes a new file of what
    /// the conversion keeps, so the user is asked first; saving again
    /// after [`DocumentState::conversion_accepted`] is set writes it.
    Converted {
        /// The file's format (`ods`).
        format: String,
    },
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::NoPath => f.write_str("The document has no file name"),
            SaveError::Converted { format } => write!(
                f,
                "Saving writes a new .{format} file without what the conversion did not keep"
            ),
            SaveError::ChangedOnDisk => f.write_str("The file was changed by another program"),
            SaveError::Lossy { encoding } => {
                f.write_str(&crate::tr!("msg-save-lossy", encoding = *encoding))
            }
            SaveError::Io(e) => write!(f, "{e}"),
            SaveError::Unencodable { ch, encoding } => f.write_str(&crate::tr!(
                "msg-unencodable",
                ch = ch.to_string(),
                encoding = *encoding
            )),
        }
    }
}

impl std::error::Error for SaveError {}

/// What a change on disk did to a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalChange {
    /// Nothing: the contents are the same.
    None,
    /// The document had no unsaved changes and was reloaded.
    Reloaded,
    /// The document has unsaved changes: the user decides
    /// ([`DocumentState::reload`] or saving with `force`).
    Conflict,
    /// The file was deleted.
    Deleted,
    /// A folder listing was read again.
    Listing,
}

impl DocumentState {
    /// A document with `text` (already decoded, line endings as in the
    /// file) and metadata.
    pub fn new(text: impl Into<String>, meta: Metadata, settings: Arc<Settings>) -> DocumentState {
        DocumentState::with_base(text, meta, settings, &ParseContext::default())
    }

    /// A document whose parse starts from `base` (see
    /// [`crate::Config::parse_base`]); a document with a path reads its
    /// `#+SETUPFILE` files relative to it.
    pub fn with_base(
        text: impl Into<String>,
        meta: Metadata,
        settings: Arc<Settings>,
        base: &ParseContext,
    ) -> DocumentState {
        let text = Text::new(text);
        let loader = meta.path.as_ref().map(|p| {
            let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
            Arc::new(org_syntax::FsSetupFiles { base: dir })
                as Arc<dyn SetupFileLoader + Send + Sync>
        });
        let org = (meta.mode == DocumentMode::Org).then(|| OrgState {
            parse: org_syntax::parse_with_base(text.as_str(), base, loader),
            parse_version: 0,
            since: Vec::new(),
            pending: None,
            cache: ModelCache::new(),
            model: None,
            last_level: None,
        });
        let latex = (meta.mode == DocumentMode::Latex).then(|| {
            let mut l = crate::latex_view::LatexState::new(text.as_str());
            if let Some(p) = &meta.path {
                l.find_project(p, text.as_str());
            }
            l
        });
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        DocumentState {
            serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            conversion_accepted: false,
            deleted_saved: None,
            read_only: false,
            text,
            version: 0,
            saved_version: 0,
            org,
            latex,
            settings,
            selection: Selection::caret(0),
            extra: Vec::new(),
            expansions: Vec::new(),
            history: History::new(),
            meta,
            narrowing: None,
            marks: Default::default(),
            disk: None,
            changes: Vec::new(),
            dired: None,
            viewer: None,
            csv_filter: None,
            csv_filter_column: None,
            csv_frozen: std::cell::RefCell::new(None),
            csv_sort: None,
            csv_dialect: std::cell::Cell::new(None),
            csv_dialect_provisional: std::cell::Cell::new(false),
            csv_view: crate::csv::View::default(),
            csv_columns: crate::csv::Columns::default(),
            csv_paste_block: false,
            csv_virtual: None,
            csv_virtual_anchor: None,
            csv_tab_run: None,
            csv_edit: None,
            csv_vim: false,
            bib_sort: None,
        }
    }

    /// A file manager document showing `place` (design §2.7): read-only
    /// text listing the folder's entries or the projects.
    pub fn directory(
        place: crate::dired::Place,
        options: kalem_fs::ListOptions,
        details: bool,
        settings: Arc<Settings>,
    ) -> DocumentState {
        let meta = Metadata {
            path: match &place {
                crate::dired::Place::Dir(d) => Some(d.clone()),
                crate::dired::Place::Projects => None,
            },
            mode: DocumentMode::Directory,
            line_ending: LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        let mut d = DocumentState::new("", meta, settings);
        let mut state = crate::dired::DirState::new(place, options, details);
        state.load();
        d.dired = Some(Box::new(state));
        d.show_listing(None);
        d
    }

    /// Writes the listing again from the file manager's state, the cursor
    /// on `select` if given, else on the same entry (or line) as before.
    /// The listing is not an edit: there is nothing to undo or save.
    pub fn show_listing(&mut self, select: Option<&Path>) {
        let Some(state) = self.dired.as_mut() else {
            return;
        };
        if state.wdired.is_some() {
            return;
        }
        let line = self.text.line_of(self.selection.head);
        let current = state.path_at(line);
        let text = state.render();
        let target = select
            .map(Path::to_path_buf)
            .or(current)
            .and_then(|p| state.line_of(&p))
            .or_else(|| (select.is_none() && line > 0).then_some(line))
            .or_else(|| state.first_entry_line())
            .unwrap_or(0);
        let column = state.name_range(target).map_or(0, |r| r.start);
        self.meta.path = state.dir().map(Path::to_path_buf);
        let old = self.text.as_str();
        if old != text {
            let (a, b) = (old.as_bytes(), text.as_bytes());
            let mut pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
            while !old.is_char_boundary(pre) || !text.is_char_boundary(pre) {
                pre -= 1;
            }
            let max = a.len().min(b.len()) - pre;
            let mut suf = a
                .iter()
                .rev()
                .zip(b.iter().rev())
                .take(max)
                .take_while(|(x, y)| x == y)
                .count();
            while !old.is_char_boundary(a.len() - suf) || !text.is_char_boundary(b.len() - suf) {
                suf -= 1;
            }
            let mut tx = Transaction::new("Listing");
            tx.edit(pre..a.len() - suf, &text[pre..b.len() - suf]);
            self.apply_raw(&tx);
        }
        self.history = History::new();
        self.mark_saved();
        let lines = self.text.line_count();
        let target = target.min(lines.saturating_sub(1));
        let r = self.text.line_range(target);
        self.selection = Selection::caret((r.start + column).min(r.end));
    }

    /// Reads the file manager's folder again, keeping marks and the cursor.
    pub fn refresh_listing(&mut self) {
        // Not while its names are edited.
        if let Some(s) = self.dired.as_mut() {
            if s.wdired.is_some() {
                return;
            }
            s.load();
        }
        self.show_listing(None);
    }

    /// Shows `place` in the file manager, the cursor on `select` (or the
    /// first entry). Marks stay behind.
    pub fn visit(&mut self, place: crate::dired::Place, select: Option<&Path>) {
        let Some(s) = self.dired.as_mut() else {
            return;
        };
        // A project's root remembers the projects view for one step only:
        // any other place forgets it (T2.7e.19).
        if s.via_projects
            .as_ref()
            .is_some_and(|root| place != crate::dired::Place::Dir(root.clone()))
        {
            s.via_projects = None;
        }
        s.place = place;
        s.filter.clear();
        s.find = None;
        s.subdirs.clear();
        s.load();
        // A new place starts at its first entry.
        self.selection = Selection::caret(0);
        self.show_listing(select);
    }

    /// Edits the document in `mode` from now on: an Org document gets a
    /// parse, others lose theirs.
    pub fn set_mode(&mut self, mode: DocumentMode, base: &ParseContext) {
        if mode == DocumentMode::Org && self.org.is_none() {
            let loader = self.meta.path.as_ref().map(|p| {
                let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
                Arc::new(org_syntax::FsSetupFiles { base: dir })
                    as Arc<dyn SetupFileLoader + Send + Sync>
            });
            self.org = Some(OrgState {
                parse: org_syntax::parse_with_base(self.text.as_str(), base, loader),
                parse_version: self.version,
                since: Vec::new(),
                pending: None,
                cache: ModelCache::new(),
                model: None,
                last_level: None,
            });
        } else if mode != DocumentMode::Org {
            self.org = None;
            self.narrowing = None;
        }
        if mode != DocumentMode::Latex {
            self.latex = None;
        } else if self.latex.is_none() {
            let mut l = crate::latex_view::LatexState::new(self.text.as_str());
            if let Some(p) = &self.meta.path {
                l.find_project(p, self.text.as_str());
            }
            self.latex = Some(l);
        }
        self.meta.mode = mode;
    }

    /// The parse and model of a LaTeX document.
    pub fn latex(&self) -> Option<&crate::latex_view::LatexState> {
        self.latex.as_ref()
    }

    /// Opens a file (§2.3, §2.6), starting its parse from `base`.
    pub fn open(
        path: &Path,
        settings: Arc<Settings>,
        base: &ParseContext,
    ) -> Result<DocumentState, OpenError> {
        if path.is_dir() {
            let dir = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
            return Ok(DocumentState::directory(
                crate::dired::Place::Dir(dir),
                kalem_fs::ListOptions::default(),
                true,
                settings,
            ));
        }
        if let Some(viewer) = crate::viewer::for_file(path) {
            return DocumentState::viewed(path, viewer, settings);
        }
        let (text, meta, disk) = files::read(path)?;
        let mut d = DocumentState::with_base(text, meta, settings, base);
        d.disk = Some(disk);
        Ok(d)
    }

    /// The file at `path` opened by `viewer` (design §11.13): no text, the
    /// viewer's units.
    pub fn viewed(
        path: &Path,
        viewer: std::sync::Arc<dyn kalem_viewer::Viewer>,
        settings: Arc<Settings>,
    ) -> Result<DocumentState, OpenError> {
        let path = dunce::canonicalize(path)
            .or_else(|_| std::path::absolute(path))
            .unwrap_or_else(|_| path.to_path_buf());
        let state = crate::viewer::ViewerState::open(viewer, &path).map_err(|e| {
            if e == kalem_viewer::NEEDS_PASSWORD {
                OpenError::NeedsPassword
            } else {
                OpenError::Viewer(e)
            }
        })?;
        let meta = Metadata {
            path: Some(path.clone()),
            mode: DocumentMode::Viewer,
            line_ending: LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        let mut d = DocumentState::new("", meta, settings);
        d.read_only = true;
        d.viewer = Some(Box::new(state));
        d.disk = files::stat(&path).ok();
        Ok(d)
    }

    /// Saves a viewer's edits: the bytes the plugin writes, written
    /// atomically as text is.
    fn save_viewed(
        &mut self,
        path: &Path,
        options: SaveOptions,
        force: bool,
    ) -> Result<(), SaveError> {
        let Some(v) = self.viewer.as_deref_mut() else {
            return Ok(());
        };
        // Save As (no file known yet) writes even without edits.
        if !v.modified() && self.disk.is_some() {
            return Ok(());
        }
        let ext = path
            .extension()
            .map_or(String::new(), |e| e.to_string_lossy().to_ascii_lowercase());
        // Written back in the format it was converted from: only once the
        // user has agreed to what that keeps.
        if !self.conversion_accepted && v.converted_from.as_deref() == Some(ext.as_str()) {
            return Err(SaveError::Converted { format: ext });
        }
        if !force && let Some(known) = self.disk {
            match files::check(path, &known).map_err(SaveError::Io)? {
                DiskChange::Modified => return Err(SaveError::ChangedOnDisk),
                DiskChange::Unchanged | DiskChange::Touched(_) | DiskChange::Deleted => {}
            }
        }
        // A workbook the viewer only showed, written as a new `.xlsx`:
        // opened again from it, to edit.
        let converted = v.is_workbook()
            && !v.grid_editable()
            && matches!(ext.as_str(), "xlsx" | "xlsm" | "xltx" | "xltm");
        let bytes = if v.is_workbook() {
            v.save_as_format(&ext)
                .map_err(|e| SaveError::Io(std::io::Error::other(e)))?
        } else {
            let out = v
                .save()
                .map_err(|e| SaveError::Io(std::io::Error::other(e)))?;
            for loss in &out.losses {
                tracing::warn!(path = %path.display(), loss, "lost on save");
            }
            out.bytes
        };
        self.disk = Some(files::write(path, &bytes, options).map_err(SaveError::Io)?);
        if converted && let Some(old) = self.viewer.as_deref() {
            let mut state = crate::viewer::ViewerState::open(old.viewer.clone(), path)
                .map_err(|e| SaveError::Io(std::io::Error::other(e)))?;
            state.set_area(old.area().0, old.area().1);
            state.go_to(old.unit);
            self.viewer = Some(Box::new(state));
        }
        Ok(())
    }

    /// Shows the file `delta` places after this one among the files of
    /// its folder the same viewer opens, wrapping around (a viewer's next
    /// and previous file). Refused while the file has unsaved edits.
    pub fn viewer_step_file(&mut self, delta: i64) -> Result<(), String> {
        let Some(v) = self.viewer.as_deref() else {
            return Ok(());
        };
        if v.modified() {
            return Err(crate::l10n::tr("msg-viewer-unsaved"));
        }
        let Some(path) = self.meta.path.clone() else {
            return Ok(());
        };
        let viewer = v.viewer.clone();
        let files = crate::viewer::siblings(&path, viewer.as_ref());
        let at = files.iter().position(|p| *p == path).unwrap_or(0) as i64;
        let n = files.len() as i64;
        if n < 2 {
            return Ok(());
        }
        let mut i = at;
        // A file that does not open is passed over.
        for _ in 1..n {
            i = (i + delta).rem_euclid(n);
            let next = &files[i as usize];
            match crate::viewer::ViewerState::open(viewer.clone(), next) {
                Ok(mut state) => {
                    let Some(old) = self.viewer.as_deref() else {
                        return Ok(());
                    };
                    state.info = old.info;
                    state.set_area(old.area().0, old.area().1);
                    self.viewer = Some(Box::new(state));
                    self.meta.path = Some(next.clone());
                    self.disk = files::stat(next).ok();
                    return Ok(());
                }
                Err(e) => tracing::info!(path = %next.display(), error = %e, "passed over"),
            }
        }
        Ok(())
    }

    /// Saves to the document's file. Unless `force`, fails if another
    /// program changed the file since it was read or saved.
    pub fn save(&mut self, options: SaveOptions, force: bool) -> Result<(), SaveError> {
        if self.dired.is_some() {
            return Ok(());
        }
        let path = self.meta.path.clone().ok_or(SaveError::NoPath)?;
        if self.viewer.is_some() {
            return self.save_viewed(&path, options, force);
        }
        // Read with replacement characters: written over the file it came
        // from, its undecodable bytes would become �.
        if self.meta.lossy && self.disk.is_some() {
            return Err(SaveError::Lossy {
                encoding: self.meta.encoding.name(),
            });
        }
        if !force && let Some(known) = self.disk {
            match files::check(&path, &known).map_err(SaveError::Io)? {
                DiskChange::Modified => return Err(SaveError::ChangedOnDisk),
                DiskChange::Unchanged | DiskChange::Touched(_) | DiskChange::Deleted => {}
            }
        }
        if let Some(ch) = files::unencodable(self.text.as_str(), self.meta.encoding) {
            return Err(SaveError::Unencodable {
                ch,
                encoding: self.meta.encoding.name(),
            });
        }
        let bytes = files::encode(self.text.as_str(), &self.meta);
        self.disk = Some(files::write(&path, &bytes, options).map_err(SaveError::Io)?);
        self.mark_saved();
        Ok(())
    }

    /// Saves to `path`, which becomes the document's file.
    /// Takes the mode the document's name and text say, after Save As
    /// gave it a new name (a new document typed as Python and saved as
    /// `x.py` stayed Org until reopened); `true` when it changed.
    pub fn mode_for_name(&mut self, base: &ParseContext) -> bool {
        if self.dired.is_some() || self.viewer.is_some() {
            return false;
        }
        let mode = DocumentMode::detect(self.meta.path.as_deref(), self.text().as_str().as_bytes());
        if mode == self.meta.mode {
            return false;
        }
        self.set_mode(mode, base);
        true
    }

    pub fn save_as(&mut self, path: &Path, options: SaveOptions) -> Result<(), SaveError> {
        self.meta.path = Some(path.to_path_buf());
        self.disk = None;
        self.save(options, true)?;
        // The new file holds the text as it shows.
        self.meta.lossy = false;
        Ok(())
    }

    /// Checks the file after a `workspace:file-changed` event: reloads the
    /// document if it has no unsaved changes, or reports a conflict.
    pub fn external_change(&mut self, now: Instant) -> Result<ExternalChange, OpenError> {
        if self.dired.is_some() {
            self.refresh_listing();
            return Ok(ExternalChange::Listing);
        }
        let (Some(path), Some(known)) = (self.meta.path.clone(), self.disk) else {
            return Ok(ExternalChange::None);
        };
        let change = files::check(&path, &known)?;
        // Back as it was after a deletion: saved again.
        if matches!(change, DiskChange::Unchanged | DiskChange::Touched(_))
            && let Some(v) = self.deleted_saved.take()
        {
            self.saved_version = v;
        }
        match change {
            DiskChange::Unchanged => Ok(ExternalChange::None),
            DiskChange::Touched(state) => {
                self.disk = Some(state);
                Ok(ExternalChange::None)
            }
            DiskChange::Deleted => {
                // The text is now the only copy: unsaved, so closing asks
                // and Save writes the file again.
                if self.viewer.is_none() && self.deleted_saved.is_none() {
                    self.deleted_saved = Some(self.saved_version);
                    self.saved_version = u64::MAX;
                }
                Ok(ExternalChange::Deleted)
            }
            DiskChange::Modified if self.is_modified() => {
                tracing::info!(path = %path.display(), "file changed on disk with unsaved changes");
                Ok(ExternalChange::Conflict)
            }
            DiskChange::Modified => {
                self.reload(now)?;
                Ok(ExternalChange::Reloaded)
            }
        }
    }

    /// Reads the file again in `encoding` (Reopen with Encoding), as one
    /// undo step; refused while there are unsaved changes.
    pub fn reopen_with(
        &mut self,
        encoding: &'static encoding_rs::Encoding,
        now: Instant,
    ) -> Result<(), OpenError> {
        let path = self.meta.path.clone().ok_or_else(|| {
            OpenError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "no file"))
        })?;
        let bytes = std::fs::read(&path)?;
        let modified = std::fs::metadata(&path)?.modified().ok();
        let disk = files::DiskState::of(&bytes, modified);
        let (text, meta) = files::decode_as(Some(&path), &bytes, encoding);
        self.replace_from_disk(&text, meta, disk, now);
        Ok(())
    }

    /// The document's text replaced by `text` read from the file, as one
    /// undo step that changes only the part that differs.
    fn replace_from_disk(
        &mut self,
        text: &str,
        meta: Metadata,
        disk: files::DiskState,
        now: Instant,
    ) {
        let old = self.text.as_str();
        let (a, b) = (old.as_bytes(), text.as_bytes());
        let mut pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
        while !old.is_char_boundary(pre) || !text.is_char_boundary(pre) {
            pre -= 1;
        }
        let max = a.len().min(b.len()) - pre;
        let mut suf = a
            .iter()
            .rev()
            .zip(b.iter().rev())
            .take(max)
            .take_while(|(x, y)| x == y)
            .count();
        while !old.is_char_boundary(a.len() - suf) || !text.is_char_boundary(b.len() - suf) {
            suf -= 1;
        }
        let mut tx = Transaction::new("Reload from disk");
        tx.edit(pre..a.len() - suf, &text[pre..b.len() - suf]);
        self.break_undo_group();
        // Read-only keeps the user from editing, not the file from
        // changing: the text follows it (it stayed old, and was taken for
        // the file's).
        let read_only = std::mem::replace(&mut self.read_only, false);
        self.apply(&tx, ChangeKind::Command, now);
        self.read_only = read_only;
        self.meta.line_ending = meta.line_ending;
        self.meta.bom = meta.bom;
        self.meta.encoding = meta.encoding;
        self.meta.lossy = meta.lossy;
        self.disk = Some(disk);
        self.mark_saved();
    }

    /// Replaces the text with the file's, as one undo step that changes
    /// only the part that differs, so the cursor and undo history stay.
    pub fn reload(&mut self, now: Instant) -> Result<(), OpenError> {
        if self.dired.is_some() {
            self.refresh_listing();
            return Ok(());
        }
        if let (Some(old), Some(path)) = (self.viewer.as_deref(), self.meta.path.clone()) {
            let mut state = crate::viewer::ViewerState::open(old.viewer.clone(), &path)
                .map_err(OpenError::Viewer)?;
            state.info = old.info;
            state.set_area(old.area().0, old.area().1);
            // A PDF built again: the same page, zoom and place on it.
            state.go_to(old.unit);
            state.zoom = old.zoom;
            state.rotation = old.rotation;
            state.center = old.center;
            self.viewer = Some(Box::new(state));
            self.disk = files::stat(&path).ok();
            return Ok(());
        }
        let path = self.meta.path.clone().ok_or_else(|| {
            OpenError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "no file"))
        })?;
        // The encoding the document has, unless a byte order mark says
        // otherwise now.
        let (text, meta, disk) = files::read_with(&path, Some(self.meta.encoding))?;
        self.replace_from_disk(&text, meta, disk, now);
        tracing::info!(path = %path.display(), "reloaded from disk");
        Ok(())
    }

    /// The text.
    pub fn text(&self) -> &Text {
        &self.text
    }

    /// This document's number in this run ([`DocumentState`]'s `serial`).
    pub fn serial(&self) -> u64 {
        self.serial
    }

    /// The column of the CSV cell selected past the end of its record,
    /// while the cursor and the text stay as they were (`csv_virtual`).
    pub fn csv_virtual_col(&self) -> Option<usize> {
        let (head, version, col) = self.csv_virtual?;
        (self.selection.head == head && self.version == version).then_some(col)
    }

    /// The column of the selection's anchor when it is a CSV cell past its
    /// record's end (`csv_virtual_anchor`).
    pub fn csv_virtual_anchor_col(&self) -> Option<usize> {
        let (anchor, version, col) = self.csv_virtual_anchor?;
        (self.selection.anchor == anchor
            && self.selection.head != anchor
            && self.version == version)
            .then_some(col)
    }

    /// Selects the CSV cell at column `col` of the record at the cursor,
    /// past its end: the cursor goes to the record's end.
    pub fn select_csv_virtual(&mut self, col: usize) {
        let Some((_, _, rec, _)) = crate::csv::cell_at(self) else {
            return;
        };
        if col < rec.fields.len() {
            return;
        }
        self.selection = Selection::caret(rec.range.end);
        self.csv_virtual = Some((rec.range.end, self.version, col));
    }

    /// The CSV grid's cell being typed into or edited, while the cursor
    /// is in it.
    pub fn csv_editing(&self) -> Option<&CellEdit> {
        let e = self.csv_edit.as_ref()?;
        let (_, row, _, col) = crate::csv::cell_at(self)?;
        ((row, col) == (e.row, e.col)).then_some(e)
    }

    /// The CSV grid's mode: Ready, Enter or Edit; `None` outside CSV and
    /// with Vim's keys (`csv_vim`), which edit the cells' text.
    pub fn csv_mode(&self) -> Option<CellMode> {
        if self.meta.mode != DocumentMode::Csv || self.csv_vim {
            return None;
        }
        Some(match self.csv_editing() {
            None => CellMode::Ready,
            Some(e) if e.edit => CellMode::Edit,
            Some(_) => CellMode::Enter,
        })
    }

    /// Ends the cell's entry when the cursor has left it (the entry stays
    /// in the file, as Excel enters it).
    pub fn settle_csv_edit(&mut self) {
        if self.csv_edit.is_some() && self.csv_editing().is_none() {
            self.csv_edit = None;
        }
    }

    /// Starts typing into (`edit` false) or editing the CSV cell at the
    /// cursor, its record kept for Escape.
    pub fn begin_csv_edit(&mut self, edit: bool) {
        let Some((_, row, rec, col)) = crate::csv::cell_at(self) else {
            return;
        };
        let record = self.text.as_str()[rec.range.clone()].to_string();
        // The entry's typing a step of its own.
        self.break_undo_group();
        self.csv_edit = Some(CellEdit {
            row,
            col,
            record,
            edit,
            steps: self.history.depth(),
        });
    }

    /// The number of undo steps.
    pub fn undo_depth(&self) -> usize {
        self.history.depth()
    }

    /// Escape in a cell being typed into or edited: its record as it was,
    /// the cursor on the cell. Whether there was one.
    pub fn cancel_csv_edit(&mut self, now: Instant) -> bool {
        let Some(e) = self.csv_editing().cloned() else {
            return false;
        };
        self.csv_edit = None;
        // The entry's own undo steps undone and not kept for Redo (a
        // "Cancel Entry" step of its own let Undo bring the entry back);
        // with history beyond them, the record put back by an edit.
        if self.history.depth() > e.steps {
            while self.history.depth() > e.steps && self.undo().is_some() {}
            self.history.forget_redo();
        }
        let Some((_, _, rec, _)) = crate::csv::cell_at(self) else {
            return true;
        };
        if self.text.as_str()[rec.range.clone()] != e.record {
            let mut tx = Transaction::new("Cancel Entry");
            tx.edit(rec.range.clone(), e.record.clone());
            let start = rec.range.start;
            self.apply(&tx, ChangeKind::Command, now);
            self.selection = Selection::caret(start);
        }
        self.go_to_csv_cell(e.row, e.col);
        true
    }

    /// Puts the cursor on the CSV cell at `row` and `col`: at its value's
    /// start, or past its record's end (selected without a change) when the
    /// record is too short to have it.
    pub fn go_to_csv_cell(&mut self, row: usize, col: usize) {
        let layout = crate::csv::layout(self);
        let rec = layout
            .index
            .borrow_mut()
            .record(self.text.as_str(), row, &layout.dialect);
        let Some(rec) = rec else { return };
        match rec.fields.get(col) {
            Some(f) => {
                let at = crate::csv::value_range(f).start;
                self.selection = Selection::caret(at);
            }
            None => {
                self.selection = Selection::caret(rec.range.end);
                self.select_csv_virtual(col);
            }
        }
        self.settle_csv_edit();
    }

    /// The column of the CSV cell at the cursor, to keep across a move
    /// up or down (`keep_csv_column`).
    pub fn csv_column(&self) -> Option<usize> {
        crate::csv::cell_at(self).map(|(_, _, _, c)| c)
    }

    /// After a move up or down in a CSV grid from column `col`: on a
    /// record too short to have it, its missing cell in that column rather
    /// than its last field, as a spreadsheet keeps the column.
    pub fn keep_csv_column(&mut self, col: usize) {
        let s = self.selection;
        if s.anchor != s.head {
            return;
        }
        let Some((layout, _, rec, c)) = crate::csv::cell_at(self) else {
            return;
        };
        if c != col && col >= rec.fields.len() && col < layout.widths.len() {
            self.select_csv_virtual(col);
        }
    }

    /// The version of the text, incremented by every change.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Writes the file with line ending `ending` (LF or CR LF) from the
    /// next save: the text's line breaks made so as one edit (undone as
    /// one), and the document modified even when the text stays the same
    /// (a classic Mac file made LF).
    pub fn set_line_ending(&mut self, ending: LineEnding, now: Instant) {
        if self.meta.line_ending == ending {
            return;
        }
        let text = self.text().as_str();
        let mut tx = Transaction::new("Line Endings");
        if ending == LineEnding::CrLf {
            for (i, _) in text.match_indices('\n') {
                if i == 0 || text.as_bytes()[i - 1] != b'\r' {
                    tx.edit(i..i, "\r");
                }
            }
        } else {
            for (i, _) in text.match_indices("\r\n") {
                tx.edit(i..i + 1, "");
            }
        }
        self.meta.line_ending = ending;
        if tx.edits.is_empty() {
            self.version += 1;
        } else {
            self.apply(&tx, ChangeKind::Command, now);
        }
    }

    /// Writes a UTF-8 file with a byte order mark (`bom`) or without one
    /// from the next save; the document is modified.
    pub fn set_bom(&mut self, bom: bool) {
        if self.meta.bom != bom && self.meta.encoding == encoding_rs::UTF_8 {
            self.meta.bom = bom;
            self.version += 1;
        }
    }

    /// Whether there are changes since the last save.
    pub fn is_modified(&self) -> bool {
        self.version != self.saved_version || self.viewer.as_ref().is_some_and(|v| v.modified())
    }

    /// Whether the text was edited since it was saved: [`Self::is_modified`]
    /// without counting a deletion of the file on disk.
    pub fn has_unsaved_edits(&self) -> bool {
        self.version != self.deleted_saved.unwrap_or(self.saved_version)
            || self.viewer.as_ref().is_some_and(|v| v.modified())
    }

    /// Kalem itself removed the file (trash, delete): its deletion is no
    /// unsaved change, so the document closes without a question.
    pub fn accept_removal(&mut self) {
        if let Some(v) = self.deleted_saved.take() {
            self.saved_version = v;
        }
    }

    /// Records that the current text was saved.
    pub fn mark_saved(&mut self) {
        self.saved_version = self.version;
        self.deleted_saved = None;
    }

    /// Applies `tx` as a user change: recorded for undo, grouped with
    /// earlier typing when `kind` is typing.
    pub fn apply(&mut self, tx: &Transaction, kind: ChangeKind, now: Instant) {
        if tx.is_empty() && tx.selection_after.is_none() {
            return;
        }
        // A folder listing is read-only; its commands change it. The
        // selection an edit would leave is not taken either: it may be past
        // the text the edit did not make.
        if self.dired.as_ref().is_some_and(|d| d.wdired.is_none()) {
            return;
        }
        if self.read_only {
            if tx.is_empty()
                && let Some(sel) = tx.selection_after
            {
                self.selection = sel;
            }
            return;
        }
        let before = self.selection;
        let after = tx.selection_after.unwrap_or_else(|| Selection {
            anchor: tx.map(before.anchor, org_edit::Assoc::After),
            head: tx.map(before.head, org_edit::Assoc::After),
        });
        self.history
            .record(tx, self.text.as_str(), before, after, kind, now);
        self.apply_raw(tx);
        self.selection = after;
        if !self.extra.is_empty() {
            let mapped = std::mem::take(&mut self.extra)
                .into_iter()
                .map(|s| Selection {
                    anchor: tx.map(s.anchor, org_edit::Assoc::After),
                    head: tx.map(s.head, org_edit::Assoc::After),
                })
                .collect();
            self.set_extra(mapped);
        }
        self.settle_csv_edit();
    }

    /// The edits applied since the last call (commands, typing, undo,
    /// reloads), in order: for moving view state and for
    /// `document:changed`.
    pub fn take_changes(&mut self) -> Vec<Transaction> {
        std::mem::take(&mut self.changes)
    }

    /// The selected text, if the selection is not empty.
    pub fn selected_text(&self) -> Option<&str> {
        let s = self.selection;
        (s.anchor != s.head)
            .then(|| &self.text.as_str()[s.anchor.min(s.head)..s.anchor.max(s.head)])
    }

    /// Moves the cursor to `pos` (clamped to a character boundary),
    /// extending the selection if `extend`.
    pub fn move_cursor(&mut self, pos: usize, extend: bool) {
        let text = self.text.as_str();
        let mut p = pos.min(text.len());
        while !text.is_char_boundary(p) {
            p -= 1;
        }
        self.selection = if extend {
            Selection {
                anchor: self.selection.anchor,
                head: p,
            }
        } else {
            self.csv_virtual_anchor = None;
            self.csv_tab_run = None;
            Selection::caret(p)
        };
        self.break_undo_group();
        self.settle_csv_edit();
    }

    /// Moves the subtree at `from` before the heading at `to` (or to the
    /// end when `to` is the text's length), its top heading at `level`: an
    /// outline's drag and drop ([`org_edit::headline::move_subtree_to`]).
    pub fn move_subtree(
        &mut self,
        from: usize,
        to: usize,
        level: usize,
        now: Instant,
    ) -> Result<(), EditError> {
        let m = self.model().ok_or_else(|| EditError {
            message: "Not an Org document".into(),
            point: None,
        })?;
        let text = self.text.as_str().to_string();
        let tx = org_edit::headline::move_subtree_to(&text, from, to, level, m.parse().context())?;
        self.apply(&tx, ChangeKind::Command, now);
        Ok(())
    }

    /// Pastes `text` and, when the clipboard has it, `html` over the
    /// selection (`kalem_core::paste`): tab-separated values become a table
    /// and HTML is converted to Org, except in verbatim text and in
    /// documents that are not Org. `plain` inserts the text as it is.
    /// Line breaks become the document's.
    pub fn paste(&mut self, text: &str, html: Option<&str>, plain: bool, now: Instant) {
        // The paths of pictures (a dropped file, as terminals paste it):
        // copied beside the document and linked.
        let style = crate::images::LinkStyle::of(&self.meta.mode);
        let pictures = (!plain && style.is_some())
            .then(|| crate::images::pasted_paths(text))
            .flatten();
        if let (Some(files), Some(doc), Some(style)) = (pictures, self.meta.path.clone(), style)
            && let Ok(links) = crate::images::import_all_as(&doc, &files, style)
        {
            return self.paste(&links, None, true, now);
        }
        let mut text = text.replace("\r\n", "\n");
        // Paste as Block: the cells written over, as a spreadsheet pastes.
        if std::mem::take(&mut self.csv_paste_block)
            && self.meta.mode == DocumentMode::Csv
            && let Some((layout, row, _, col)) = crate::csv::cell_at(self)
        {
            let block = crate::csv_tools::block_rows(&text, &layout.dialect);
            let rows = crate::csv::view_rows_from(self, row, block.len());
            if let Some(tx) = crate::csv_tools::paste_block(
                self.text().as_str(),
                &layout.dialect,
                &rows,
                col,
                &block,
            ) {
                let caret = self.selection.head;
                let tx = tx.select(Selection::caret(caret));
                self.apply(&tx, ChangeKind::Command, now);
            }
            return;
        }
        // Rows copied from a spreadsheet, in a CSV file: its delimiter.
        if !plain
            && self.meta.mode == DocumentMode::Csv
            && let Some(t) = crate::csv::pasted(&text, &crate::csv::layout(self).dialect)
        {
            text = t;
        }
        // BibTeX in a LaTeX document: into its bibliography, cited here.
        if !plain
            && self.meta.mode == DocumentMode::Latex
            && let Some(t) = crate::cite::pasted_bibtex(self, &text)
        {
            text = t;
        }
        if !self.extra.is_empty() {
            self.paste_at_cursors(&text, now);
            return;
        }
        let s = self.selection;
        let sel = s.anchor.min(s.head)..s.anchor.max(s.head);
        let ins = if plain || self.meta.mode != DocumentMode::Org {
            None
        } else {
            self.model()
                .map(|m| crate::paste::plan(&m, sel.clone(), &text, html))
        }
        .unwrap_or_else(|| crate::paste::Insertion::plain(sel, &text));
        let (new, cursor) = if self.meta.line_ending == LineEnding::CrLf {
            let before = ins.text[..ins.cursor].matches('\n').count();
            (ins.text.replace('\n', "\r\n"), ins.cursor + before)
        } else {
            (ins.text, ins.cursor)
        };
        let mut tx = Transaction::new("Paste");
        tx.edit(ins.range.clone(), new);
        let tx = tx.select(Selection::caret(ins.range.start + cursor));
        self.apply(&tx, ChangeKind::Command, now);
    }

    /// Pastes a picture's data (`extension` as `png`): saved beside the
    /// document (`NAME_assets/`) and linked. A document without a file
    /// has nowhere to keep it.
    pub fn paste_picture(
        &mut self,
        data: &[u8],
        extension: &str,
        now: Instant,
    ) -> Result<(), String> {
        let style = crate::images::LinkStyle::of(&self.meta.mode)
            .ok_or_else(|| crate::l10n::tr("msg-not-org"))?;
        let doc = self
            .meta
            .path
            .clone()
            .ok_or_else(|| crate::l10n::tr("msg-picture-needs-file"))?;
        let link = crate::images::save_as(&doc, data, extension, style)?;
        self.paste(&link, None, true, now);
        Ok(())
    }

    /// Links pictures dropped on the document, copied beside it when they
    /// are elsewhere.
    pub fn drop_pictures(&mut self, files: &[PathBuf], now: Instant) -> Result<(), String> {
        let doc = self
            .meta
            .path
            .clone()
            .ok_or_else(|| crate::l10n::tr("msg-picture-needs-file"))?;
        let style = crate::images::LinkStyle::of(&self.meta.mode)
            .ok_or_else(|| crate::l10n::tr("msg-not-org"))?;
        let links = crate::images::import_all_as(&doc, files, style)?;
        self.paste(&links, None, true, now);
        Ok(())
    }

    /// How this document indents: as its lines do, else tabs for
    /// languages that need them (Makefiles, Go) and four spaces otherwise.
    pub fn indent_unit(&self) -> crate::text::Indent {
        use crate::text::Indent;
        let default = match &self.meta.mode {
            DocumentMode::Text { language: Some(l) }
                if matches!(l.as_str(), "makefile" | "make" | "mk" | "go") =>
            {
                Indent::Tabs
            }
            _ => Indent::Spaces(4),
        };
        crate::text::detect_indent(self.text.as_str()).unwrap_or(default)
    }

    /// Tab in plain text (T1.6a.4): with a selection over lines, indents
    /// them (or outdents them) one step in the document's style; otherwise
    /// a tab or the spaces to the next step, or with `outdent` one step
    /// less before the line's text.
    pub fn indent(&mut self, outdent: bool, now: Instant) {
        use crate::text::Indent;
        let unit = self.indent_unit();
        let step = match unit {
            Indent::Tabs => "\t".to_string(),
            Indent::Spaces(n) => " ".repeat(n),
        };
        let s = self.selection;
        let (a, b) = (s.anchor.min(s.head), s.anchor.max(s.head));
        let (l1, l2) = (
            self.text.line_of(a),
            self.text.line_of(b.saturating_sub(1).max(a)),
        );
        if !outdent && l1 == l2 {
            // In a line: to the next step.
            let text = match unit {
                Indent::Tabs => "\t".to_string(),
                Indent::Spaces(n) => {
                    let col = self.text.as_str()[self.text.line_start(l1)..a]
                        .chars()
                        .count();
                    " ".repeat(n - col % n)
                }
            };
            self.insert_text(&text, now);
            return;
        }
        let mut tx = Transaction::new(if outdent { "Outdent" } else { "Indent" });
        for l in l1..=l2 {
            let start = self.text.line_start(l);
            let line = &self.text.as_str()[self.text.line_range(l)];
            if outdent {
                let n = match unit {
                    Indent::Tabs => usize::from(line.starts_with('\t')),
                    Indent::Spaces(n) => line.bytes().take(n).take_while(|c| *c == b' ').count(),
                };
                if n > 0 {
                    tx.edit(start..start + n, "");
                }
            } else if !line.trim().is_empty() {
                tx.edit(start..start, step.clone());
            }
        }
        if tx.is_empty() {
            return;
        }
        let map = |p: usize| tx.map(p, org_edit::Assoc::After);
        let sel = if s.anchor == s.head {
            Selection::caret(map(s.head).max(self.text.line_start(l1)))
        } else {
            Selection {
                anchor: tx.map(s.anchor, org_edit::Assoc::Before),
                head: map(s.head),
            }
        };
        let tx = tx.select(sel);
        self.apply(&tx, ChangeKind::Command, now);
    }

    /// Replaces the selection with `text`, as typing (undone in groups).
    pub fn insert_text(&mut self, text: &str, now: Instant) {
        if !self.extra.is_empty() {
            self.insert_at_cursors(|_| text.to_string(), now);
            return;
        }
        let s = self.selection;
        let (a, b) = (s.anchor.min(s.head), s.anchor.max(s.head));
        let mut tx = Transaction::new("Typing");
        tx.edit(a..b, text);
        // In an environment's name: the other end too (T2.7h.15).
        let mut caret = a + text.len();
        if let Some(m) = self.latex_mirror(a..b) {
            let _ = tx.replace(m.clone(), text);
            if m.start < a {
                caret = (caret as isize + text.len() as isize - m.len() as isize) as usize;
            }
        }
        let tx = tx.select(Selection::caret(caret));
        self.apply(&tx, ChangeKind::Typing, now);
    }

    /// For an edit of `range` in the name of a closed environment of a
    /// LaTeX document: the same place at its other end.
    fn latex_mirror(&self, range: std::ops::Range<usize>) -> Option<std::ops::Range<usize>> {
        let l = self.latex.as_ref()?;
        crate::latex_edit::mirror(&l.parse().syntax(), range)
    }

    /// The start of the grapheme before `pos` (a CR LF pair is one).
    pub fn grapheme_before(&self, pos: usize) -> usize {
        let text = self.text.as_str();
        let mut pos = pos.min(text.len());
        while !text.is_char_boundary(pos) {
            pos -= 1;
        }
        if pos == 0 {
            return 0;
        }
        // Segment a window that starts on the previous character's line, so
        // a line break's carriage return is in it, and is bounded for very
        // long lines.
        let line_start = text.as_bytes()[..pos - 1]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1);
        let mut from = line_start.max(pos.saturating_sub(128));
        while !text.is_char_boundary(from) {
            from -= 1;
        }
        text[from..pos]
            .grapheme_indices(true)
            .next_back()
            .map_or(pos - 1, |(i, _)| from + i)
    }

    /// The end of the grapheme after `pos`.
    pub fn grapheme_after(&self, pos: usize) -> usize {
        let text = self.text.as_str();
        let mut pos = pos.min(text.len());
        while !text.is_char_boundary(pos) {
            pos -= 1;
        }
        let mut to = (pos + 128).min(text.len());
        while !text.is_char_boundary(to) {
            to += 1;
        }
        text[pos..to]
            .graphemes(true)
            .next()
            .map_or(pos, |g| pos + g.len())
    }

    /// Whether typing at `pos` follows Org's rules: in an Org document, on
    /// a table line (columns stay aligned) or a headline with tags (tags
    /// stay aligned). Elsewhere the rules change nothing.
    fn org_typing_at(&self, pos: usize) -> bool {
        if self.meta.mode != DocumentMode::Org {
            return false;
        }
        let text = self.text.as_str();
        let line = self.text.line_range(self.text.line_of(pos));
        let l = &text[line];
        l.trim_start_matches([' ', '\t']).starts_with('|')
            || (l.starts_with('*') && l.trim_end_matches([' ', '\t', '\r']).ends_with(':'))
    }

    /// Types `text` (a character or a grapheme cluster) at the cursor, as
    /// `org-self-insert-command`: in tables the column keeps its width
    /// while there is room, and on headlines the tags stay aligned.
    /// `blank_field`: the previous command moved to a table field, which is
    /// blanked first (`org-table-auto-blank-field`). A selection is
    /// replaced.
    /// Typing in a CSV file's grid (not its source view): a delimiter, a
    /// quote or a line break goes into the field's value, which is quoted
    /// for it. Whether the text was typed so; else `type_text` types it.
    pub fn type_in_grid(&mut self, text: &str, now: Instant) -> bool {
        if self.meta.mode != DocumentMode::Csv || !self.extra.is_empty() {
            return false;
        }
        // Over a selection: within one cell its text goes; over several
        // cells the text goes into the cell the selection started from,
        // as a spreadsheet types into the active cell (it replaced the
        // text between them, merging cells and rows; then it went into
        // the cell the selection was extended to).
        let s = self.selection;
        if s.anchor != s.head {
            let anchor = crate::csv::anchor_cell(self);
            if anchor == crate::csv::cell_at(self).map(|(_, r, _, c)| (r, c)) {
                self.delete_in_grid(false, now);
            } else if let Some((row, col)) = anchor {
                self.selection = Selection::caret(s.anchor);
                self.go_to_csv_cell(row, col);
            }
        }
        let Some((layout, _, rec, col)) = crate::csv::cell_at(self) else {
            return false;
        };
        // Ready mode: the text replaces the cell (past the record's end,
        // the fields up to it first), and typing into it goes on, as in
        // Excel; a new entry, a new undo step (the entries of a row made
        // with Tab were one). Vim's insert mode inserts (it replaced the
        // cell). A cell past the record's end gets the fields up to it in
        // every mode (after F2 the text went into the field before).
        let entry = self.csv_editing().cloned();
        let past = col >= rec.fields.len();
        if (entry.is_none() && !self.csv_vim) || past {
            let record = match &entry {
                Some(e) => e.record.clone(),
                None => self.text.as_str()[rec.range.clone()].to_string(),
            };
            let tx = crate::csv::set_cell(self.text.as_str(), &rec, col, text, &layout.dialect);
            self.csv_virtual = None;
            if entry.is_none() {
                self.break_undo_group();
            }
            let steps = entry
                .as_ref()
                .map_or_else(|| self.history.depth(), |e| e.steps);
            self.apply(&tx, ChangeKind::Typing, now);
            if let Some((_, row, _, col)) = crate::csv::cell_at(self) {
                self.csv_edit = Some(CellEdit {
                    row,
                    col,
                    record,
                    edit: entry.is_some_and(|e| e.edit),
                    steps,
                });
            }
            return true;
        }
        // Right before or after a quoted field's quotes: inside them, so
        // that the field stays one (`"x"Q` is not CSV).
        let mut pos = self.selection.head;
        if let Some(f) = rec
            .fields
            .get(col)
            .filter(|f| f.quoted && f.range.len() >= 2)
        {
            if pos == f.range.start {
                pos += 1;
            } else if pos == f.range.end {
                pos -= 1;
            }
        }
        let Some(tx) = crate::csv::typed(self.text.as_str(), &rec, pos, text, &layout.dialect)
            .or_else(|| {
                (pos != self.selection.head).then(|| {
                    let mut tx = Transaction::new("Typing");
                    tx.edit(pos..pos, text);
                    tx.select(Selection::caret(pos + text.len()))
                })
            })
        else {
            return false;
        };
        self.apply(&tx, ChangeKind::Typing, now);
        true
    }

    /// A paste into a CSV file's grid (not its source view), as a
    /// spreadsheet pastes: lines or tab-separated values are written over
    /// the cells from the cursor's (the selection's top left) down and to
    /// the right, rows added past the end; a single value goes into the
    /// cell, quoted when it needs it. Whether it was pasted so.
    pub fn paste_in_grid(&mut self, text: &str, now: Instant) -> bool {
        if self.meta.mode != DocumentMode::Csv || !self.extra.is_empty() {
            return false;
        }
        let text = text.replace("\r\n", "\n");
        let text = text.strip_suffix('\n').unwrap_or(&text);
        if text.is_empty() {
            return false;
        }
        let Some((layout, row, rec, col)) = crate::csv::cell_at(self) else {
            return false;
        };
        // Paste as Block asked for: this is it.
        self.csv_paste_block = false;
        let s = self.selection;
        let editing = self.csv_editing().is_some();
        // Typing into a cell (Enter or Edit mode): into it at the caret,
        // line breaks and tabs too, as a spreadsheet pastes into a cell
        // being edited (rows of it overwrote the cells below).
        if s.anchor == s.head && editing {
            if !self.type_in_grid(text, now) {
                self.insert_text(text, now);
            }
            return true;
        }
        let (row0, col0) = if s.anchor == s.head {
            (row, col)
        } else {
            let Some(cell) = crate::csv::anchor_cell(self) else {
                return false;
            };
            cell
        };
        // The selection's top row as the grid shows it.
        let top = crate::csv::rows_between(self, row0, row)
            .first()
            .copied()
            .unwrap_or(row0.min(row));
        let left = col0.min(col);
        if text.contains(['\n', '\t']) {
            let block = crate::csv_tools::block_rows(text, &layout.dialect);
            let rows = crate::csv::view_rows_from(self, top, block.len());
            let Some(tx) = crate::csv_tools::paste_block(
                self.text.as_str(),
                &layout.dialect,
                &rows,
                left,
                &block,
            ) else {
                return false;
            };
            self.apply(&tx, ChangeKind::Command, now);
            // The cursor at the block's first cell.
            let layout = crate::csv::layout(self);
            let first = layout
                .index
                .borrow_mut()
                .record(self.text.as_str(), top, &layout.dialect)
                .and_then(|r| r.fields.get(left).map(|f| f.range.start));
            if let Some(at) = first {
                self.selection = Selection::caret(at);
            }
            return true;
        }
        // One value: into the cell, the selection's text in it replaced,
        // a selection over cells collapsed to its first.
        if s.anchor != s.head {
            if (row0, col0) == (row, col) {
                self.delete_in_grid(false, now);
            } else if let Some(f) = crate::csv::layout(self)
                .index
                .borrow_mut()
                .record(self.text.as_str(), top, &layout.dialect)
                .and_then(|r| r.fields.get(left).cloned())
            {
                self.selection = Selection::caret(f.range.end);
            } else {
                self.selection = Selection::caret(rec.range.end);
            }
        }
        if !self.type_in_grid(text, now) {
            self.insert_text(text, now);
        }
        // The cell pasted into stays selected, as in a spreadsheet: Escape
        // no longer takes the paste back, and the next typing replaces it.
        if !editing {
            self.csv_edit = None;
            self.break_undo_group();
        }
        true
    }

    /// Backspace (`forward` false) or Delete in a CSV file's grid (not
    /// its source view): within a cell's value, never its delimiters or
    /// quotes; a selection over more than one cell clears the cells
    /// (rather than merging them, and the rows). Whether the key was
    /// taken so; else `delete_backward` or `delete_forward` deletes.
    pub fn delete_in_grid(&mut self, forward: bool, now: Instant) -> bool {
        if self.meta.mode != DocumentMode::Csv || !self.extra.is_empty() {
            return false;
        }
        let Some((layout, row, rec, col)) = crate::csv::cell_at(self) else {
            return false;
        };
        let s = self.selection;
        if s.anchor != s.head {
            let Some((row0, col0)) = crate::csv::anchor_cell(self) else {
                return false;
            };
            if (row0, col0) == (row, col) {
                // Within one cell: its value only.
                let Some(f) = rec.fields.get(col) else {
                    return false;
                };
                let (a, b) = if f.quoted && f.range.len() >= 2 {
                    (f.range.start + 1, f.range.end - 1)
                } else {
                    (f.range.start, f.range.end)
                };
                let (lo, hi) = (s.anchor.min(s.head).max(a), s.anchor.max(s.head).min(b));
                if lo < hi {
                    let mut tx = Transaction::new("Delete");
                    tx.edit(lo..hi, "");
                    let tx = tx.select(Selection::caret(lo));
                    self.apply(&tx, ChangeKind::Typing, now);
                } else {
                    self.selection = Selection::caret(s.head);
                }
                return true;
            }
            // The cells the grid shows selected (with a filter or a sorted
            // view on, not the file's rows between the corners).
            let rows = crate::csv::rows_between(self, row0, row);
            let cols = (col0.min(col), col0.max(col));
            if let Some(tx) = crate::csv::clear_cells(self.text.as_str(), &layout, &rows, cols) {
                self.apply(&tx, ChangeKind::Command, now);
            }
            return true;
        }
        if let Some(tx) = crate::csv::deleted(
            self.text.as_str(),
            &rec,
            self.selection.head,
            forward,
            &layout.dialect,
        ) {
            self.apply(&tx, ChangeKind::Typing, now);
            self.unquote_entry(now);
        }
        true
    }

    /// After a deletion in the cell being typed into or edited: quotes
    /// the entry put around its value (for a delimiter, a quote or a blank
    /// at an end, typed) go when the value no longer needs them and the
    /// field had none before the entry (a space typed and deleted left
    /// `""` in the file).
    fn unquote_entry(&mut self, now: Instant) {
        let Some(e) = self.csv_editing().cloned() else {
            return;
        };
        let Some((layout, _, rec, col)) = crate::csv::cell_at(self) else {
            return;
        };
        let d = &layout.dialect;
        let Some(f) = rec.fields.get(col).filter(|f| f.quoted) else {
            return;
        };
        if crate::csv::scan(&e.record, 0, d)
            .fields
            .get(col)
            .is_some_and(|f| f.quoted)
        {
            return;
        }
        let text = self.text.as_str();
        let v = crate::csv::value(text, f, d).into_owned();
        if crate::csv::encode(&v, d) != v || text[f.range.clone()].len() != v.len() + 2 {
            return;
        }
        // The caret where it was in the value, the opening quote gone.
        let head = self.selection.head;
        let caret = head.saturating_sub(f.range.start + 1).min(v.len()) + f.range.start;
        let mut tx = Transaction::new("Typing");
        tx.edit(f.range.clone(), v);
        let tx = tx.select(Selection::caret(caret));
        self.apply(&tx, ChangeKind::Typing, now);
    }

    pub fn type_text(&mut self, text: &str, blank_field: bool, now: Instant) {
        if !self.extra.is_empty() {
            self.insert_text(text, now);
            return;
        }
        // LaTeX: `\begin{name}` typed gets its `\end{name}`; `$`, `\(`,
        // `\[` and `\left(` their closing pairs.
        if let Some(l) = &self.latex {
            if let Some(tx) = crate::latex_edit::typed(
                self.text.as_str(),
                self.selection,
                &l.parse().syntax(),
                text,
            ) {
                self.apply(&tx, ChangeKind::Typing, now);
                return;
            }
            self.insert_text(text, now);
            if text == "}"
                && let Some(l) = &self.latex
                && let Some(tx) = crate::latex_edit::complete_begin(
                    self.text.as_str(),
                    self.selection.head,
                    l.parse(),
                )
            {
                self.apply(&tx, ChangeKind::Typing, now);
            }
            return;
        }
        let s = self.selection;
        // In code, a closing bracket alone on its line goes back a level.
        if s.anchor == s.head
            && matches!(self.meta.mode, DocumentMode::Text { language: Some(_) })
            && let Some(r) = crate::code::dedent_for(
                self.text.as_str(),
                s.head,
                text,
                &crate::code::indent_text(self),
            )
        {
            let mut tx = Transaction::new("Typing");
            tx.edit(r.clone(), text);
            let tx = tx.select(Selection::caret(r.start + text.len()));
            self.apply(&tx, ChangeKind::Typing, now);
            return;
        }
        if s.anchor != s.head || !self.org_typing_at(s.head) {
            self.insert_text(text, now);
            return;
        }
        let r = self.run_as(now, ChangeKind::Typing, |d, p, _| {
            org_edit::typing::self_insert(d, p, text, blank_field)
        });
        if r.is_err() {
            self.insert_text(text, now);
        }
    }

    /// The formatting objects (emphasis, links) on the line holding `pos`.
    fn line_objects(&self, pos: usize) -> Vec<org_syntax::SyntaxKind> {
        use org_syntax::SyntaxKind as K;
        let Some((p, true)) = self.parse() else {
            return Vec::new();
        };
        let line = self
            .text
            .line_range(self.text.line_of(pos.min(self.text.len())));
        let root = p.syntax();
        let range =
            org_syntax::TextRange::new((line.start as u32).into(), (line.end as u32).into());
        let mut kinds: Vec<K> = root
            .descendants()
            .filter(|n| {
                matches!(
                    n.kind(),
                    K::BOLD
                        | K::ITALIC
                        | K::UNDERLINE
                        | K::STRIKE_THROUGH
                        | K::CODE
                        | K::VERBATIM
                        | K::LINK
                ) && n
                    .text_range()
                    .intersect(range)
                    .is_some_and(|r| !r.is_empty())
            })
            .map(|n| n.kind())
            .collect();
        kinds.sort_by_key(|k| *k as u16);
        kinds
    }

    /// What a deletion broke: the formatting that is plain text now (§6.3).
    fn broken_formatting(&self, before: &[org_syntax::SyntaxKind], pos: usize) -> Option<String> {
        use org_syntax::SyntaxKind as K;
        let after = self.line_objects(pos);
        let lost = before.iter().find(|k| {
            before.iter().filter(|x| x == k).count() > after.iter().filter(|x| x == k).count()
        })?;
        let name = match lost {
            K::BOLD => "format-bold",
            K::ITALIC => "format-italic",
            K::UNDERLINE => "format-underline",
            K::STRIKE_THROUGH => "format-strike-through",
            K::CODE => "format-code",
            K::VERBATIM => "format-verbatim",
            _ => "format-link",
        };
        Some(crate::tr!(
            "msg-formatting-removed",
            format = crate::l10n::tr(name)
        ))
    }

    /// Deletes the selection, or the grapheme before the cursor (in tables
    /// keeping columns aligned, as `org-delete-backward-char`). Returns a
    /// message when the deletion turned formatting into plain text.
    pub fn delete_backward(&mut self, now: Instant) -> Option<String> {
        if !self.extra.is_empty() {
            self.delete_at_cursors(false, now);
            return None;
        }
        let before = self.line_objects(self.selection.head);
        self.delete_backward_inner(now);
        self.broken_formatting(&before, self.selection.head)
    }

    fn delete_backward_inner(&mut self, now: Instant) {
        let s = self.selection;
        if s.anchor == s.head && s.head > 0 && self.org_typing_at(s.head) {
            let len = s.head - self.grapheme_before(s.head);
            let _ = self.run_as(now, ChangeKind::Typing, |d, p, _| {
                Ok(org_edit::typing::delete_backward(d, p, len))
            });
            return;
        }
        let range = if s.anchor != s.head {
            s.anchor.min(s.head)..s.anchor.max(s.head)
        } else if s.head > 0 {
            self.grapheme_before(s.head)..s.head
        } else {
            return;
        };
        self.delete(range, now);
    }

    /// Deletes the selection, or the grapheme after the cursor (in tables
    /// keeping columns aligned, as `org-delete-char`). Returns a message
    /// when the deletion turned formatting into plain text.
    pub fn delete_forward(&mut self, now: Instant) -> Option<String> {
        if !self.extra.is_empty() {
            self.delete_at_cursors(true, now);
            return None;
        }
        let before = self.line_objects(self.selection.head);
        self.delete_forward_inner(now);
        self.broken_formatting(&before, self.selection.head)
    }

    fn delete_forward_inner(&mut self, now: Instant) {
        let s = self.selection;
        if s.anchor == s.head && s.head < self.text.len() && self.org_typing_at(s.head) {
            let len = self.grapheme_after(s.head) - s.head;
            let _ = self.run_as(now, ChangeKind::Typing, |d, p, _| {
                Ok(org_edit::typing::delete_forward(d, p, len))
            });
            return;
        }
        let range = if s.anchor != s.head {
            s.anchor.min(s.head)..s.anchor.max(s.head)
        } else if s.head < self.text.len() {
            s.head..self.grapheme_after(s.head)
        } else {
            return;
        };
        self.delete(range, now);
    }

    fn delete(&mut self, range: std::ops::Range<usize>, now: Instant) {
        let start = range.start;
        let parts = vec![range];
        let mut tx = Transaction::new("Delete");
        let mut caret = start;
        if let [one] = parts.as_slice()
            && let Some(m) = self.latex_mirror(one.clone())
        {
            let _ = tx.replace(m.clone(), "");
            if m.start < one.start {
                caret -= m.len();
            }
        }
        for r in parts {
            tx.edit(r, "");
        }
        let tx = tx.select(Selection::caret(caret));
        self.apply(&tx, ChangeKind::Typing, now);
    }

    /// Ends the current typing group, so the next typing is a new undo step.
    pub fn break_undo_group(&mut self) {
        self.history.break_group();
    }

    /// Every change from now until [`Self::break_undo_group`] is one undo
    /// step (Vim's command and the insert it starts).
    pub fn begin_undo_join(&mut self) {
        self.history.begin_join();
    }

    /// Undoes the last step; returns its label.
    pub fn undo(&mut self) -> Option<String> {
        if let Some(v) = self.viewer.as_deref_mut() {
            return v.undo().ok().filter(|done| *done).map(|_| String::new());
        }
        if self.read_only {
            return None;
        }
        let replay = self.history.undo()?;
        self.csv_edit = None;
        for t in &replay.transactions {
            self.apply_raw(t);
        }
        self.selection = replay.selection;
        self.extra.clear();
        Some(replay.label)
    }

    /// Redoes the last undone step with the cursor where the step began,
    /// as Vim's redo leaves it; returns its label.
    pub fn redo_from_start(&mut self) -> Option<String> {
        if self.viewer.is_some() {
            return self.redo();
        }
        if self.read_only {
            return None;
        }
        let replay = self.history.redo()?;
        self.csv_edit = None;
        for t in &replay.transactions {
            self.apply_raw(t);
        }
        let len = self.text().len();
        let at = replay.start.head.min(len);
        self.selection = Selection::caret(at);
        self.extra.clear();
        Some(replay.label)
    }

    /// Redoes the last undone step; returns its label.
    pub fn redo(&mut self) -> Option<String> {
        if let Some(v) = self.viewer.as_deref_mut() {
            return v.redo().ok().filter(|done| *done).map(|_| String::new());
        }
        if self.read_only {
            return None;
        }
        let replay = self.history.redo()?;
        self.csv_edit = None;
        for t in &replay.transactions {
            self.apply_raw(t);
        }
        self.selection = replay.selection;
        self.extra.clear();
        Some(replay.label)
    }

    /// Applies the edits of `tx` to the text and the parse.
    fn apply_raw(&mut self, tx: &Transaction) {
        if tx.is_empty() {
            return;
        }
        self.changes.push(tx.clone());
        self.marks.map(tx);
        if let Some(f) = self.csv_frozen.get_mut() {
            f.map(tx);
        }
        let edit = tx.covering_edit(self.text.as_str());
        if let Some(r) = &self.narrowing {
            self.narrowing = Some(
                tx.map(r.start, org_edit::Assoc::Before)..tx.map(r.end, org_edit::Assoc::After),
            );
        }
        self.text.apply(&tx.edits);
        self.version += 1;
        let version = self.version;
        let text = self.text.as_str();
        if let Some(l) = &mut self.latex {
            l.edit(text, edit.as_ref());
            l.diagnostics.map(tx, version);
            if let Some(first) = tx.edits.first() {
                l.check_root(text, first.range.start);
            }
        }
        let Some(org) = &mut self.org else { return };
        let current =
            org.pending.is_none() && org.since.is_empty() && org.parse_version + 1 == version;
        if current
            && let Some(edit) = &edit
            && let Some((parse, level)) = org.parse.try_reparse(text, edit)
        {
            org.parse = parse;
            org.parse_version = version;
            org.last_level = Some(level);
            return;
        }
        // Behind: remember the edit for position mapping and parse the
        // whole text in the background, one parse at a time; a parse that
        // finishes behind the text is followed by another ([`poll`]).
        org.since.push((version, tx.clone()));
        org.last_level = Some(ReparseLevel::Document);
        if org.pending.is_none() {
            org.pending = Some(spawn_parse(&org.parse, text, version));
        }
    }

    /// Installs a finished background parse and LaTeX diagnostics, and
    /// starts LaTeX diagnostics after a pause in typing. Returns whether
    /// what the view shows changed.
    pub fn poll(&mut self) -> bool {
        let latex = match &mut self.latex {
            Some(l) => {
                let project = l.poll_project();
                let d =
                    l.diagnostics
                        .poll(self.version, self.meta.path.as_deref(), self.text.as_str());
                // Pictures TeX finished drawing.
                let done = crate::tex_pictures::finished();
                let pictures = l.pictures_seen.replace(done) != done;
                project || d || pictures
            }
            None => false,
        };
        // A large Markdown document's first parse, done in the background.
        let markdown =
            self.meta.mode == DocumentMode::Markdown && crate::markdown::background_done();
        self.poll_parse() || latex || markdown
    }

    /// The LaTeX diagnostics of the text as it is, when they are known
    /// (they are worked out after a pause in typing).
    pub fn latex_diagnostics(&self) -> Option<&Arc<Vec<crate::latex_check::Diagnostic>>> {
        self.latex.as_ref()?.diagnostics.current(self.version)
    }

    /// Whether a LaTeX document's diagnostics are to be worked out again
    /// (frontends poll sooner meanwhile).
    pub fn latex_diagnostics_due(&self) -> bool {
        self.latex
            .as_ref()
            .is_some_and(|l| l.diagnostics.due(self.version))
    }

    /// Waits for a LaTeX document's project to be found (tests).
    pub fn wait_for_latex_project(&mut self) {
        if let Some(l) = &mut self.latex {
            l.wait_for_project();
        }
    }

    /// Works the LaTeX diagnostics out now rather than in the background
    /// (tests).
    pub fn update_latex_diagnostics(&mut self) {
        if let Some(l) = &mut self.latex {
            l.diagnostics
                .update_now(self.version, self.meta.path.as_deref(), self.text.as_str());
        }
    }

    fn poll_parse(&mut self) -> bool {
        let version = self.version;
        let Some(org) = &mut self.org else {
            return false;
        };
        let Some(p) = &org.pending else { return false };
        match p.result.try_recv() {
            Ok(parse) => {
                let for_version = p.version;
                org.pending = None;
                // The finished tree serves the view better than the older
                // one even if edits came in meanwhile; then parse again.
                org.parse = parse;
                org.parse_version = for_version;
                org.since.retain(|(v, _)| *v > for_version);
                tracing::debug!(
                    version = for_version,
                    behind = org.since.len(),
                    "background parse installed"
                );
                if for_version != version {
                    org.pending = Some(spawn_parse(&org.parse, self.text.as_str(), version));
                }
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                org.pending = None;
                false
            }
        }
    }

    /// Waits for the parse to catch up with the text (commands need a
    /// current tree).
    pub fn wait_for_parse(&mut self) {
        let Some(org) = &mut self.org else { return };
        if org.pending.is_none() && org.since.is_empty() {
            return;
        }
        let parse = match org.pending.take() {
            Some(p) if p.version == self.version => p.result.recv().ok(),
            _ => None,
        };
        org.parse = parse.unwrap_or_else(|| org.parse.parse_again(self.text.as_str()));
        org.parse_version = self.version;
        org.since.clear();
    }

    /// The parse, and whether it is for the current text. When it is not,
    /// [`DocumentState::edits_since_parse`] maps its positions.
    pub fn parse(&self) -> Option<(&Parse, bool)> {
        let org = self.org.as_ref()?;
        Some((
            &org.parse,
            org.parse_version == self.version && org.since.is_empty(),
        ))
    }

    /// Edits made after the text the parse is for, in order.
    pub fn edits_since_parse(&self) -> impl Iterator<Item = &Transaction> {
        self.org.iter().flat_map(|o| o.since.iter().map(|(_, t)| t))
    }

    /// How much the last change reparsed.
    pub fn last_reparse(&self) -> Option<ReparseLevel> {
        self.org.as_ref().and_then(|o| o.last_level)
    }

    /// The type of the document as a whole, wherever the cursor is: `org`,
    /// a plain text file's language (lower case)
    /// or `text`, `markdown`, `latex`, `csv`, `directory`. Menus and
    /// toolbars show the commands that serve it.
    pub fn document_type(&self) -> String {
        match &self.meta.mode {
            DocumentMode::Org => crate::kinds::file_kind(self).unwrap_or("org").to_string(),
            DocumentMode::Text { language: Some(l) } => l.to_lowercase(),
            m => m.name().to_string(),
        }
    }

    /// The when-clause keys that hold for the whole document, wherever the
    /// cursor is: `editorMode`, `fileKind`, `editorLanguage`, and
    /// `textType` as `document_type`. Menus and toolbars offer the
    /// commands whose when-clause can hold with these
    /// (`CommandRegistry::offered`).
    pub fn document_context(&self) -> Context {
        let mut c = Context::default();
        c.set("editorMode", Value::Str(self.meta.mode.name().into()));
        if let Some(kind) = crate::kinds::file_kind(self) {
            c.set("fileKind", Value::Str(kind.into()));
        }
        if let DocumentMode::Text { language: Some(l) } = &self.meta.mode {
            c.set("editorLanguage", Value::Str(l.clone()));
        }
        c.set("textType", Value::Str(self.document_type()));
        c
    }

    /// The type of the text at the cursor (§11.2), innermost first: in an
    /// Org document the language of the source block the cursor is in
    /// (lower case), an export block's back-end, `latex` in a formula, else
    /// `org`; a plain text file's language, or
    /// `text`; `markdown`, `csv`, `directory`.
    pub fn text_type(&self) -> String {
        use org_syntax::SyntaxKind as K;
        match &self.meta.mode {
            DocumentMode::Org => {}
            DocumentMode::Text { language: Some(l) } => {
                return crate::command::canonical_type(l);
            }
            m => return m.name().to_string(),
        }
        let base = crate::kinds::file_kind(self).unwrap_or("org").to_string();
        let Some((parse, _)) = self.parse() else {
            return base;
        };
        let root = parse.syntax();
        let len = usize::from(root.text_range().end());
        let pos = self.selection.head.min(len);
        if len == 0 {
            return base;
        }
        let text = self.text.as_str();
        let Some(tok) = root
            .token_at_offset(org_syntax::TextSize::from(pos.min(len - 1) as u32))
            .right_biased()
        else {
            return base;
        };
        for a in tok.parent_ancestors() {
            match a.kind() {
                K::LATEX_FRAGMENT | K::LATEX_ENVIRONMENT => return "latex".into(),
                K::SRC_BLOCK | K::EXPORT_BLOCK => {
                    // Inside the contents: past the first line, before
                    // the last.
                    let r = a.text_range();
                    let (s, e) = (usize::from(r.start()), usize::from(r.end()));
                    // The parse may be of the text before the last edits
                    // (a reparse running): its ranges sliced with care.
                    let Some(body) = text.get(s..e) else {
                        return base;
                    };
                    let first = body.find('\n').map_or(e, |i| s + i);
                    let end_line = body.trim_end().rfind('\n').map_or(e, |i| s + i + 1);
                    if pos <= first || pos >= end_line {
                        return base;
                    }
                    let head = &text[s..first];
                    let word = head
                        .split_whitespace()
                        .nth(1)
                        .map(crate::command::canonical_type);
                    return word.unwrap_or(base);
                }
                _ => {}
            }
        }
        base
    }

    /// The model [`DocumentState::model`] made for the current parse, if
    /// it has made one.
    pub fn cached_model(&self) -> Option<Arc<Document>> {
        let org = self.org.as_ref()?;
        match &org.model {
            Some((v, m)) if *v == org.parse_version => Some(m.clone()),
            _ => None,
        }
    }

    /// The document model of the current text, computed lazily and reusing
    /// unchanged subtrees across versions.
    pub fn model(&mut self) -> Option<Arc<Document>> {
        self.wait_for_parse();
        let settings = self.settings.clone();
        let file = self
            .meta
            .path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        let org = self.org.as_mut()?;
        if let Some((v, m)) = &org.model
            && *v == org.parse_version
        {
            return Some(m.clone());
        }
        let m = Arc::new(Document::with_cache(
            org.parse.clone(),
            settings,
            file,
            org.cache.clone(),
        ));
        org.model = Some((org.parse_version, m.clone()));
        Some(m)
    }

    /// Runs an `org-edit` command on the current model at the cursor and
    /// applies its result as one undo step.
    pub fn run(
        &mut self,
        now: Instant,
        command: impl FnOnce(&Document, usize, Option<usize>) -> Result<Transaction, EditError>,
    ) -> Result<(), EditError> {
        self.run_as(now, ChangeKind::Command, command)
    }

    /// [`DocumentState::run`] with the undo grouping of `kind`.
    pub fn run_as(
        &mut self,
        now: Instant,
        kind: ChangeKind,
        command: impl FnOnce(&Document, usize, Option<usize>) -> Result<Transaction, EditError>,
    ) -> Result<(), EditError> {
        let Some(model) = self.model() else {
            return Err(EditError {
                message: "Not an Org document".into(),
                point: None,
            });
        };
        let sel = self.selection;
        let mark = (sel.anchor != sel.head).then_some(sel.anchor);
        let result = match self.narrowing.clone() {
            Some(r) => org_edit::narrow::narrowed(&model, r.clone(), sel.head, |sub, p| {
                command(sub, p, mark.map(|m| m.clamp(r.start, r.end) - r.start))
            }),
            None => command(&model, sel.head, mark),
        };
        match result {
            Ok(tx) => {
                self.apply(&tx, kind, now);
                Ok(())
            }
            Err(e) => {
                if let Some(p) = e.point {
                    self.selection = Selection::caret(p);
                }
                Err(e)
            }
        }
    }

    /// The when-clause context at the cursor: `editorMode` (`org`,
    /// `markdown`, `csv`, `text`, `binary`), `editorLanguage`,
    /// `hasSelection`, `narrowed` and `modified`, and in Org documents
    /// `onHeadline` (on a heading line), `inTable`, `inList`, `inBlock`,
    /// `inSrcBlock` and `onTimestamp`. While a full reparse runs, the Org keys come from the
    /// previous tree. Frontends add their own keys, such as `editorFocus`.
    pub fn when_context(&self) -> Context {
        use org_syntax::SyntaxKind as K;
        let mut c = Context::default();
        let mode = self.meta.mode.name();
        c.set("editorMode", Value::Str(mode.into()));
        if let Some(kind) = crate::kinds::file_kind(self) {
            c.set("fileKind", Value::Str(kind.into()));
        }
        if let DocumentMode::Text { language: Some(l) } = &self.meta.mode {
            c.set("editorLanguage", Value::Str(l.clone()));
        }
        if let Some(v) = self.viewer.as_deref() {
            c.flag("viewerAnimated", v.structure().animated());
            c.flag("viewerEditable", !v.edits().is_empty());
            c.flag("viewerGrid", v.is_grid());
        }
        c.flag(
            "wdired",
            self.dired.as_ref().is_some_and(|d| d.wdired.is_some()),
        );
        c.flag("hasSelection", self.selection.anchor != self.selection.head);
        // The text has a comment syntax (Toggle Comment): not a viewer's
        // file, not a listing.
        c.flag(
            "hasComments",
            self.viewer.is_none()
                && self.dired.is_none()
                && crate::code::language_at(self)
                    .and_then(|l| crate::code::comment_style(&l))
                    .is_some(),
        );
        // A plugin's question waits, a plugin has a panel.
        c.flag("pluginAsks", crate::extensions::asking());
        c.flag("pluginPanels", crate::extensions::has_panels());
        c.flag("narrowed", self.narrowing.is_some());
        c.flag("modified", self.is_modified());
        c.flag("hasFile", self.meta.path.is_some());
        c.flag("readOnly", self.read_only);
        c.flag(
            "hasFormatter",
            crate::packs::has_formatter(self)
                || crate::lsp::can(self, crate::lsp::Kind::Format)
                || crate::lsp::has_format_command(self),
        );
        c.flag("hasLanguageServer", crate::lsp::serves(self));
        if self.meta.mode == DocumentMode::Markdown
            && let Some(md) = crate::markdown::ready(self)
        {
            let at = self.selection.head;
            c.flag(
                "inMarkdownTable",
                crate::markdown_table::table_at(&md, self.text.as_str(), at).is_some(),
            );
            c.flag(
                "inMarkdownList",
                crate::markdown::newline(&md, self.text.as_str(), at).is_some(),
            );
            c.flag("inMarkdownItem", crate::markdown::in_item(&md, at));
        }
        c.set("textType", Value::Str(self.text_type()));
        let Some((parse, _)) = self.parse() else {
            return c;
        };
        let root = parse.syntax();
        let pos = self
            .selection
            .head
            .min(usize::from(root.text_range().end()));
        let Some(el) = org_edit::narrow::element_at(&root, pos) else {
            return c;
        };
        let kinds: Vec<K> = el.ancestors().map(|a| a.kind()).collect();
        let block = |k: &K| {
            matches!(
                k,
                K::SRC_BLOCK
                    | K::EXAMPLE_BLOCK
                    | K::EXPORT_BLOCK
                    | K::COMMENT_BLOCK
                    | K::VERSE_BLOCK
                    | K::QUOTE_BLOCK
                    | K::CENTER_BLOCK
                    | K::SPECIAL_BLOCK
                    | K::DYNAMIC_BLOCK
            )
        };
        c.flag(
            "onHeadline",
            matches!(el.kind(), K::HEADLINE | K::INLINETASK),
        );
        // In a table only on one of its rows (`org-at-table-p`): the blank
        // lines after a table, and the end of the text below it, are not.
        let line = self.text.line_range(self.text.line_of(pos));
        let on_row = self.text.as_str()[line]
            .trim_start_matches([' ', '\t'])
            .starts_with('|');
        c.flag("inTable", on_row && kinds.contains(&K::TABLE));
        c.flag("inList", kinds.contains(&K::ITEM));
        c.flag("inBlock", kinds.iter().any(block));
        c.flag("inSrcBlock", el.kind() == K::SRC_BLOCK);
        c.flag(
            "onTimestamp",
            org_edit::timestamp::at_timestamp(self.text.as_str(), pos),
        );
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indenting_plain_text() {
        let meta = |lang: &str| Metadata {
            path: None,
            mode: DocumentMode::Text {
                language: Some(lang.into()),
            },
            line_ending: LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        let now = Instant::now();
        let mut d = DocumentState::new("a\n  b\nc\n", meta("py"), Arc::new(Settings::default()));
        d.selection = Selection::caret(1);
        d.indent(false, now);
        assert_eq!(d.text().as_str(), "a \n  b\nc\n");
        // Lines of a selection, one step (two spaces here), then back.
        d.selection = Selection { anchor: 0, head: 9 };
        d.indent(false, now);
        assert_eq!(d.text().as_str(), "  a \n    b\n  c\n");
        d.indent(true, now);
        assert_eq!(d.text().as_str(), "a \n  b\nc\n");
        // Tabs where the file has them, and in a new Makefile.
        let mut d = DocumentState::new("", meta("makefile"), Arc::new(Settings::default()));
        d.indent(false, now);
        assert_eq!(d.text().as_str(), "\t");
    }

    #[test]
    fn typing_and_deleting() {
        let mut d = org("* A\nx\u{65}\u{301}y\r\nz");
        let now = Instant::now();
        d.move_cursor(4, false);
        d.insert_text("ab", now);
        assert_eq!(d.text().as_str(), "* A\nabxe\u{301}y\r\nz");
        // One grapheme: e with its accent.
        d.move_cursor(10, false);
        d.delete_backward(now);
        assert_eq!(d.text().as_str(), "* A\nabxy\r\nz");
        // CR LF is one grapheme.
        d.move_cursor(8, false);
        d.delete_forward(now);
        assert_eq!(d.text().as_str(), "* A\nabxyz");
        // Backspace at the start of a CRLF line removes the whole break.
        let mut c = org("a\r\nb");
        c.move_cursor(3, false);
        c.delete_backward(now);
        assert_eq!(c.text().as_str(), "ab");
        // Deleting a marker says what formatting is gone.
        let mut b = org("a *bold* b\n");
        b.move_cursor(8, false);
        assert_eq!(
            b.delete_backward(now).as_deref(),
            Some("Bold removed: its marker was deleted")
        );
        assert_eq!(b.delete_backward(now), None);
        d.move_cursor(4, false);
        d.move_cursor(6, true);
        assert_eq!(d.selected_text(), Some("ab"));
        d.insert_text("Q", now);
        assert_eq!(d.text().as_str(), "* A\nQxyz");
        d.undo();
        assert_eq!(d.text().as_str(), "* A\nabxyz");
    }

    #[test]
    fn typing_in_tables() {
        let mut d = org("| ab   | c |\n");
        let now = Instant::now();
        d.move_cursor(4, false);
        d.type_text("x", false, now);
        d.type_text("y", false, now);
        assert_eq!(d.text().as_str(), "| abxy | c |\n");
        d.delete_backward(now);
        assert_eq!(d.text().as_str(), "| abx  | c |\n");
        // One undo step for the typing.
        d.undo();
        assert_eq!(d.text().as_str(), "| ab   | c |\n");
        // After moving to a field, the first key replaces its content.
        d.move_cursor(2, false);
        d.type_text("z", true, now);
        assert_eq!(d.text().as_str(), "| z    | c |\n");
    }

    #[test]
    fn line_endings_and_byte_order_mark_changed() {
        let dir = std::env::temp_dir().join(format!("kalem-eol-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("w.txt");
        std::fs::write(&p, "\u{feff}one\r\ntwo\r\n").unwrap();
        let now = Instant::now();
        let mut d =
            DocumentState::open(&p, Arc::new(Settings::default()), &ParseContext::default())
                .unwrap();
        assert_eq!(
            crate::files::encoding_label(&d.meta).as_deref(),
            Some("BOM CRLF")
        );
        // To LF: the text's breaks too, as one edit; then no mark.
        d.set_line_ending(LineEnding::Lf, now);
        assert_eq!(d.text().as_str(), "one\ntwo\n");
        assert!(d.is_modified());
        d.set_bom(false);
        assert_eq!(crate::files::encoding_label(&d.meta), None);
        d.save(crate::files::SaveOptions::default(), false).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"one\ntwo\n");
        // And back, saved as CR LF with a mark.
        d.set_line_ending(LineEnding::CrLf, now);
        d.set_bom(true);
        assert!(d.is_modified());
        d.save(crate::files::SaveOptions::default(), false).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"\xEF\xBB\xBFone\r\ntwo\r\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files() {
        let dir = std::env::temp_dir().join(format!("kalem-doc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("d.org");
        std::fs::write(&p, "* A\r\nbody\r\n").unwrap();
        let now = Instant::now();
        let mut d =
            DocumentState::open(&p, Arc::new(Settings::default()), &ParseContext::default())
                .unwrap();
        assert_eq!(d.meta.line_ending, LineEnding::CrLf);
        d.selection = Selection::caret(4);
        d.run(now, |m, p, _| {
            org_edit::todo::todo(
                m,
                p,
                &org_edit::todo::TodoOptions {
                    arg: org_edit::todo::TodoArg::Cycle,
                    settings: &org_edit::todo::TodoSettings::default(),
                    now: jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0),
                    remembered_head: None,
                    repeated: false,
                    force_note: false,
                    inhibit_note: false,
                },
            )
            .map(|o| o.transaction)
        })
        .unwrap();
        d.save(SaveOptions::default(), false).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "* TODO A\r\nbody\r\n");
        assert!(!d.is_modified());
        // Another program changes the file: a clean document reloads.
        std::fs::write(&p, "* TODO A\r\nbody, edited\r\n").unwrap();
        assert_eq!(d.external_change(now).unwrap(), ExternalChange::Reloaded);
        assert_eq!(d.text().as_str(), "* TODO A\r\nbody, edited\r\n");
        assert_eq!(d.selection.head, 9);
        assert!(!d.is_modified());
        d.undo();
        assert_eq!(d.text().as_str(), "* TODO A\r\nbody\r\n");
        // With unsaved changes it is a conflict, and saving needs force.
        std::fs::write(&p, "changed again\n").unwrap();
        assert_eq!(d.external_change(now).unwrap(), ExternalChange::Conflict);
        assert!(matches!(
            d.save(SaveOptions::default(), false),
            Err(SaveError::ChangedOnDisk)
        ));
        d.save(SaveOptions::default(), true).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "* TODO A\r\nbody\r\n");
        assert_eq!(d.external_change(now).unwrap(), ExternalChange::None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A clean document whose file is deleted on disk counts as unsaved,
    /// its text being the only copy: closing asks, Save writes it again;
    /// the file back as it was makes it saved again.
    #[test]
    fn a_deleted_file_leaves_an_unsaved_document() {
        let dir = std::env::temp_dir().join(format!("kalem-del-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("note.txt");
        std::fs::write(&p, "keep\n").unwrap();
        let now = Instant::now();
        let mut d =
            DocumentState::open(&p, Arc::new(Settings::default()), &ParseContext::default())
                .unwrap();
        assert!(!d.is_modified());
        std::fs::remove_file(&p).unwrap();
        assert_eq!(d.external_change(now).unwrap(), ExternalChange::Deleted);
        assert!(d.is_modified());
        // Restored by another program (a checkout): saved again.
        std::fs::write(&p, "keep\n").unwrap();
        assert_eq!(d.external_change(now).unwrap(), ExternalChange::None);
        assert!(!d.is_modified());
        // Deleted again, then saved: the file is back.
        std::fs::remove_file(&p).unwrap();
        d.external_change(now).unwrap();
        d.save(SaveOptions::default(), false).unwrap();
        assert!(!d.is_modified());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "keep\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A document made read-only still follows its file: the reload
    /// takes the new text (it reported the reload and kept the old text).
    #[test]
    fn a_read_only_document_reloads() {
        let dir = std::env::temp_dir().join(format!("kalem-ro-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("app.log");
        std::fs::write(&p, "one\n").unwrap();
        let now = Instant::now();
        let mut d =
            DocumentState::open(&p, Arc::new(Settings::default()), &ParseContext::default())
                .unwrap();
        d.read_only = true;
        std::fs::write(&p, "one\ntwo\n").unwrap();
        assert_eq!(d.external_change(now).unwrap(), ExternalChange::Reloaded);
        assert_eq!(d.text().as_str(), "one\ntwo\n");
        assert!(d.read_only && !d.is_modified());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A file read with replacement characters is not saved over: its
    /// bytes would become U+FFFD. Save As writes the text as it shows.
    #[test]
    fn a_lossy_read_is_not_saved_over() {
        let dir = std::env::temp_dir().join(format!("kalem-lossy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("latin1.txt");
        // "café" in Latin-1: 0xE9 is not UTF-8.
        let bytes = b"caf\xe9\n".to_vec();
        std::fs::write(&p, &bytes).unwrap();
        let now = Instant::now();
        let mut d =
            DocumentState::open(&p, Arc::new(Settings::default()), &ParseContext::default())
                .unwrap();
        d.reopen_with(encoding_rs::UTF_8, now).unwrap();
        assert!(d.meta.lossy);
        assert!(matches!(
            d.save(SaveOptions::default(), false),
            Err(SaveError::Lossy { .. })
        ));
        assert!(matches!(
            d.save(SaveOptions::default(), true),
            Err(SaveError::Lossy { .. })
        ));
        assert_eq!(std::fs::read(&p).unwrap(), bytes);
        let copy = dir.join("copy.txt");
        d.save_as(&copy, SaveOptions::default()).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), "caf\u{fffd}\n".as_bytes());
        assert!(!d.meta.lossy);
        assert_eq!(std::fs::read(&p).unwrap(), bytes);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn contexts() {
        let text = "* H\n- a\n  | x |\n#+begin_src sh\nls\n#+end_src\n";
        let meta = Metadata {
            path: None,
            mode: DocumentMode::Org,
            line_ending: LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        let mut d = DocumentState::new(text, meta, Arc::new(Settings::default()));
        d.wait_for_parse();
        let at = |d: &mut DocumentState, p: usize| {
            d.selection = Selection::caret(p);
            let c = d.when_context();
            ["onHeadline", "inList", "inTable", "inBlock", "inSrcBlock"]
                .into_iter()
                .filter(|k| c.get(k) == Some(&Value::Bool(true)))
                .collect::<Vec<_>>()
        };
        assert_eq!(at(&mut d, 1), ["onHeadline"]);
        assert_eq!(at(&mut d, 6), ["inList"]);
        assert_eq!(at(&mut d, 12), ["inList", "inTable"]);
        assert_eq!(at(&mut d, 34), ["inBlock", "inSrcBlock"]);
        // Below a table: not in it.
        for text in ["| a | b |\n", "| a | b |\n\nafter\n"] {
            let mut t = org(text);
            t.wait_for_parse();
            assert_eq!(at(&mut t, 10), Vec::<&str>::new(), "{text:?}");
            assert_eq!(at(&mut t, 9), ["inTable"], "{text:?}");
        }
        assert_eq!(
            d.when_context().get("editorMode"),
            Some(&Value::Str("org".into()))
        );
    }

    fn org(text: &str) -> DocumentState {
        let meta = Metadata {
            path: None,
            mode: DocumentMode::Org,
            line_ending: LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        DocumentState::new(text, meta, Arc::new(Settings::default()))
    }

    fn typing(d: &mut DocumentState, at: usize, s: &str, now: Instant) {
        let mut t = Transaction::new("Type");
        t.replace(at..at, s).unwrap();
        let t = t.select(Selection::caret(at + s.len()));
        d.apply(&t, ChangeKind::Typing, now);
    }

    #[test]
    fn edits_undo_and_parse() {
        let mut d = org("* A\ntext\n");
        let now = Instant::now();
        d.selection = Selection::caret(8);
        typing(&mut d, 8, "s", now);
        typing(&mut d, 9, "!", now);
        assert_eq!(d.text().as_str(), "* A\ntexts!\n");
        assert_eq!(d.parse().unwrap().0.syntax().to_string(), d.text().as_str());
        assert!(d.parse().unwrap().1);
        assert_eq!(d.undo().as_deref(), Some("Type"));
        assert_eq!(d.text().as_str(), "* A\ntext\n");
        assert_eq!(d.selection.head, 8);
        d.redo();
        assert_eq!(d.text().as_str(), "* A\ntexts!\n");
        assert!(d.is_modified());
    }

    #[test]
    fn one_background_parse_at_a_time() {
        let mut d = org("* TODO A\n");
        let now = Instant::now();
        typing(&mut d, 0, "#+TODO: NEXT | DONE\n", now);
        let (_, current) = d.parse().unwrap();
        if current {
            return;
        }
        // More edits while the parse runs join the queue of edits.
        typing(&mut d, 0, "#+TODO: WAIT | DONE\n", now);
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        while !d.parse().unwrap().1 && Instant::now() < deadline {
            d.poll();
            let (p, _) = d.parse().unwrap();
            // The tree and the remembered edits always add up to the text.
            let mut text = p.syntax().to_string();
            for t in d.edits_since_parse() {
                text = t.apply(&text);
            }
            assert_eq!(text, d.text().as_str());
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let (p, current) = d.parse().unwrap();
        assert!(current);
        // Both `#+TODO` lines apply.
        assert_eq!(p.context().todo_keywords, ["WAIT", "NEXT"]);
    }

    #[test]
    fn full_parse_in_background() {
        let mut d = org("* TODO A\n");
        let now = Instant::now();
        // A new TODO keyword needs a full parse.
        typing(&mut d, 0, "#+TODO: NEXT | DONE\n", now);
        let (_, current) = d.parse().unwrap();
        if !current {
            assert_eq!(d.edits_since_parse().count(), 1);
        }
        d.wait_for_parse();
        let (p, current) = d.parse().unwrap();
        assert!(current);
        assert_eq!(p.context().todo_keywords, vec!["NEXT".to_string()]);
        // Commands run on the current model.
        d.selection = Selection::caret(25);
        d.run(now, |m, p, _| {
            let settings = org_edit::todo::TodoSettings::default().for_document(m);
            let opts = org_edit::todo::TodoOptions {
                arg: org_edit::todo::TodoArg::Next,
                settings: &settings,
                now: jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0),
                remembered_head: None,
                repeated: false,
                force_note: false,
                inhibit_note: false,
            };
            org_edit::todo::todo(m, p, &opts).map(|o| o.transaction)
        })
        .unwrap();
        assert_eq!(d.text().as_str(), "#+TODO: NEXT | DONE\n* NEXT TODO A\n");
    }
}
