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

/// An open document.
#[derive(Debug)]
pub struct DocumentState {
    text: Text,
    /// Incremented by every change.
    version: u64,
    /// The version the file was last saved (or opened) at.
    saved_version: u64,
    org: Option<OrgState>,
    settings: Arc<Settings>,
    /// The selection.
    pub selection: Selection,
    /// More cursors and selections (multiple cursors, column selection),
    /// besides the primary [`DocumentState::selection`]: typing, deleting
    /// and pasting act at each (see `crate::cursors`).
    pub extra: Vec<Selection>,
    history: History,
    /// File and mode information.
    pub meta: Metadata,
    /// The narrowed part of the text, if any (view state; commands see only
    /// this part).
    pub narrowing: Option<std::ops::Range<usize>>,
    /// The file as last read or written.
    disk: Option<DiskState>,
    /// Edits applied since the frontend last took them.
    changes: Vec<Transaction>,
    /// The file manager's state, in a folder listing
    /// ([`DocumentMode::Directory`]).
    pub dired: Option<Box<crate::dired::DirState>>,
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
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::NoPath => f.write_str("The document has no file name"),
            SaveError::ChangedOnDisk => f.write_str("The file was changed by another program"),
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
        DocumentState {
            text,
            version: 0,
            saved_version: 0,
            org,
            settings,
            selection: Selection::caret(0),
            extra: Vec::new(),
            history: History::new(),
            meta,
            narrowing: None,
            disk: None,
            changes: Vec::new(),
            dired: None,
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
            tx.replace(pre..a.len() - suf, &text[pre..b.len() - suf])
                .expect("one edit");
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
        if let Some(s) = self.dired.as_mut() {
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
        if let crate::dired::Place::Dir(d) = &place
            && s.via_projects
                .as_ref()
                .is_some_and(|root| !d.starts_with(root))
        {
            s.via_projects = None;
        }
        s.place = place;
        s.filter.clear();
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
        self.meta.mode = mode;
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
        let (text, meta, disk) = files::read(path)?;
        let mut d = DocumentState::with_base(text, meta, settings, base);
        d.disk = Some(disk);
        Ok(d)
    }

    /// Saves to the document's file. Unless `force`, fails if another
    /// program changed the file since it was read or saved.
    pub fn save(&mut self, options: SaveOptions, force: bool) -> Result<(), SaveError> {
        if self.dired.is_some() {
            return Ok(());
        }
        let path = self.meta.path.clone().ok_or(SaveError::NoPath)?;
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
    pub fn save_as(&mut self, path: &Path, options: SaveOptions) -> Result<(), SaveError> {
        self.meta.path = Some(path.to_path_buf());
        self.disk = None;
        self.save(options, true)
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
        match files::check(&path, &known)? {
            DiskChange::Unchanged => Ok(ExternalChange::None),
            DiskChange::Touched(state) => {
                self.disk = Some(state);
                Ok(ExternalChange::None)
            }
            DiskChange::Deleted => Ok(ExternalChange::Deleted),
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
        tx.replace(pre..a.len() - suf, &text[pre..b.len() - suf])
            .expect("one edit");
        self.break_undo_group();
        self.apply(&tx, ChangeKind::Command, now);
        self.meta.line_ending = meta.line_ending;
        self.meta.bom = meta.bom;
        self.meta.encoding = meta.encoding;
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

    /// The version of the text, incremented by every change.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Whether there are changes since the last save.
    pub fn is_modified(&self) -> bool {
        self.version != self.saved_version
    }

    /// Records that the current text was saved.
    pub fn mark_saved(&mut self) {
        self.saved_version = self.version;
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
        if self.dired.is_some() {
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
            Selection::caret(p)
        };
        self.break_undo_group();
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
        let pictures = (!plain && self.meta.mode == DocumentMode::Org)
            .then(|| crate::images::pasted_paths(text))
            .flatten();
        if let (Some(files), Some(doc)) = (pictures, self.meta.path.clone())
            && let Ok(links) = crate::images::import_all(&doc, &files)
        {
            return self.paste(&links, None, true, now);
        }
        let text = text.replace("\r\n", "\n");
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
        tx.replace(ins.range.clone(), new).expect("one edit");
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
        if self.meta.mode != DocumentMode::Org {
            return Err(crate::l10n::tr("msg-not-org"));
        }
        let doc = self
            .meta
            .path
            .clone()
            .ok_or_else(|| crate::l10n::tr("msg-picture-needs-file"))?;
        let link = crate::images::save(&doc, data, extension)?;
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
        let links = crate::images::import_all(&doc, files)?;
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
                    tx.replace(start..start + n, "").expect("apart");
                }
            } else if !line.trim().is_empty() {
                tx.replace(start..start, step.clone()).expect("apart");
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
        tx.replace(a..b, text).expect("one edit");
        let tx = tx.select(Selection::caret(a + text.len()));
        self.apply(&tx, ChangeKind::Typing, now);
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
    pub fn type_text(&mut self, text: &str, blank_field: bool, now: Instant) {
        if !self.extra.is_empty() {
            self.insert_text(text, now);
            return;
        }
        let s = self.selection;
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
            // The character before a formatting snippet, not the snippet.
            let head = self.past_markers(s.head, true);
            if head == 0 {
                return;
            }
            self.grapheme_before(head)..head
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
            let head = self.past_markers(s.head, false);
            if head >= self.text.len() {
                return;
            }
            head..self.grapheme_after(head)
        } else {
            return;
        };
        self.delete(range, now);
    }

    fn delete(&mut self, range: std::ops::Range<usize>, now: Instant) {
        let start = range.start;
        let parts = self.kalem_deletion(range);
        let before: usize = parts
            .iter()
            .map(|r| r.end.min(start).saturating_sub(r.start))
            .sum();
        let mut tx = Transaction::new("Delete");
        for r in parts {
            tx.replace(r, "").expect("separate ranges");
        }
        let tx = tx.select(Selection::caret(start - before));
        self.apply(&tx, ChangeKind::Typing, now);
    }

    /// What deleting `range` deletes: Kalem's formatting snippets stay
    /// (see [`crate::rich::deletion`]).
    fn kalem_deletion(&self, range: std::ops::Range<usize>) -> Vec<std::ops::Range<usize>> {
        let text = self.text.as_str();
        let near = &text[range.start.saturating_sub(2)..(range.end + 2).min(text.len())];
        if self.meta.mode != DocumentMode::Org || !near.contains("@@") {
            return vec![range];
        }
        match self.parse() {
            Some((p, true)) => crate::rich::deletion(&p.syntax(), text, range),
            _ => vec![range],
        }
    }

    /// `pos` moved out of Kalem's snippets: before them (`back`) or after.
    fn past_markers(&self, pos: usize, back: bool) -> usize {
        let text = self.text.as_str();
        let at_snippet = if back {
            text[..pos].ends_with("@@")
        } else {
            text[pos..].starts_with("@@")
        };
        if self.meta.mode != DocumentMode::Org || !at_snippet {
            return pos;
        }
        let Some((p, true)) = self.parse() else {
            return pos;
        };
        let root = p.syntax();
        let mut pos = pos;
        loop {
            let probe = if back { pos.checked_sub(1) } else { Some(pos) };
            match probe.and_then(|q| crate::rich::marker_at(&root, q)) {
                Some(m) => pos = if back { m.start } else { m.end },
                None => return pos,
            }
        }
    }

    /// Ends the current typing group, so the next typing is a new undo step.
    pub fn break_undo_group(&mut self) {
        self.history.break_group();
    }

    /// Undoes the last step; returns its label.
    pub fn undo(&mut self) -> Option<String> {
        let replay = self.history.undo()?;
        for t in &replay.transactions {
            self.apply_raw(t);
        }
        self.selection = replay.selection;
        self.extra.clear();
        Some(replay.label)
    }

    /// Redoes the last undone step; returns its label.
    pub fn redo(&mut self) -> Option<String> {
        let replay = self.history.redo()?;
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
        let edit = tx.covering_edit(self.text.as_str());
        if let Some(r) = &self.narrowing {
            self.narrowing = Some(
                tx.map(r.start, org_edit::Assoc::Before)..tx.map(r.end, org_edit::Assoc::After),
            );
        }
        for e in tx.edits.iter().rev() {
            self.text.replace(e.range.clone(), &e.insert);
        }
        self.version += 1;
        let version = self.version;
        let text = self.text.as_str();
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

    /// Installs a finished background parse, if any. Returns whether the
    /// parse changed.
    pub fn poll(&mut self) -> bool {
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
    /// `onHeadline` (on a heading line), `inTable`, `inList`, `inBlock` and
    /// `inSrcBlock`. While a full reparse runs, the Org keys come from the
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
        c.flag("hasSelection", self.selection.anchor != self.selection.head);
        c.flag("narrowed", self.narrowing.is_some());
        c.flag("modified", self.is_modified());
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

    #[test]
    fn contexts() {
        let text = "* H\n- a\n  | x |\n#+begin_src sh\nls\n#+end_src\n";
        let meta = Metadata {
            path: None,
            mode: DocumentMode::Org,
            line_ending: LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
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
