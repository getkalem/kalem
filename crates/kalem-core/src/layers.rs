//! Layers over a mode's view (plugin API 0.2.10): what a plugin changes in
//! the way a core mode draws a document, without parsing it again.
//!
//! A layer serves the documents of some modes under a folder holding one
//! of its root markers (a Logseq graph's `logseq/config.edn`, an Obsidian
//! vault's `.obsidian`). For such a document it gives [`Overlays`], by
//! source range: spans hidden away from the cursor, spans shown as other
//! text away from the cursor, spans drawn in a style, and whole lines
//! hidden or folded to their first line away from the cursor. The core
//! draws the line as the mode draws it ([`crate::mode_view`]), then
//! applies the overlays to its runs, which map to source ranges, and to
//! its blocks, which decide the lines shown. The source view shows the
//! text as it is. Every "Markdown plus" (Logseq's blocks and properties,
//! Obsidian's callouts and comments, Pandoc's and MyST's extensions) is a
//! layer, the core's Markdown drawing kept whole.
//!
//! The overlays of a document are asked of the layer's provider (the
//! plugins' bridge) once per version of its text and per
//! [`generation`] of the layers, which a layer moves when what it shows
//! depends on other files (a block reference's text) and those changed.
//! The plugins' bridge is asked on a thread of its own
//! ([`set_background_provider`]), never as the editors draw: they draw
//! the overlays last given meanwhile, moved with the edits made since
//! ([`DocLayer`]), and again when the answer comes
//! ([`crate::extensions::shown`]).

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::view::{Block, BlockKind, LineView, Run, Style};

/// What a span of the text becomes.
#[derive(Debug, Clone, PartialEq)]
pub enum SpanEffect {
    /// Hidden away from the cursor, as a mode's markers are.
    Hide,
    /// Shown as `text` in `style` away from the cursor.
    Replace {
        /// What is shown.
        text: String,
        /// How.
        style: Style,
    },
    /// Drawn in `style` (its set parts added to the mode's), always.
    Style(Style),
}

/// A span of the text and what it becomes.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    /// Its bytes.
    pub range: Range<usize>,
    /// What it becomes.
    pub effect: SpanEffect,
}

/// What whole lines become.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEffect {
    /// Not shown away from the cursor.
    Hidden,
    /// Folded to their first line away from the cursor, as a drawer.
    Folded,
}

/// Whole lines of the text and what they become.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lines {
    /// From a line's start to the start of the line after the last.
    pub range: Range<usize>,
    /// What they become.
    pub effect: LineEffect,
}

/// The text of a file a line under a line shows (API 0.2.15): in file
/// `path`, line `line` (from 0) from byte `column` of it to its end, its
/// line feed left out, which held `text` when the layer was asked. The
/// line is typed into where it is drawn and what is typed written there
/// ([`crate::row_edit`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineSource {
    /// The file, a whole path.
    pub path: std::path::PathBuf,
    /// Its line, from 0.
    pub line: usize,
    /// The byte of the line the text starts at.
    pub column: usize,
    /// The text, to the line's end.
    pub text: String,
    /// The block of the file's outline the line starts, when the layer
    /// gives it (API 0.2.18): Enter, Tab and Shift+Tab there change the
    /// file's outline as they would in it.
    pub outline: Option<Outline>,
}

/// The block of a file's outline a line under a line starts (API 0.2.18,
/// [`crate::row_edit`]): Enter splits it at the cursor into a new block
/// where the layer says, Tab indents it with the blocks under it, Shift+Tab
/// takes a step of indentation off them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outline {
    /// The line after the block's last, the blocks under it included,
    /// from 0: Tab and Shift+Tab move the lines from the block's to it.
    pub end: usize,
    /// The line Enter puts a new block at, from 0: `end` for a block after
    /// it, the line after the block's own for a first child.
    pub next: usize,
    /// What a new block's line starts with: the block's indentation and
    /// bullet (`\t- `).
    pub bullet: String,
    /// A step of indentation (`\t`, two spaces): put before each of the
    /// block's lines by Tab, taken off them by Shift+Tab.
    pub step: String,
    /// Whether Tab may indent it (a block before it at its level).
    pub indent: bool,
    /// Whether Shift+Tab may take it out a step.
    pub outdent: bool,
}

/// A line drawn under a line of the text and not part of it (API
/// 0.2.13): an embedded block's child, a query's result.
#[derive(Debug, Clone, PartialEq)]
pub struct VirtualLine {
    /// Columns in from where the text of the line it is under starts.
    pub indent: usize,
    /// Its text, in pieces and their styles.
    pub pieces: Vec<(String, Style)>,
    /// The text of a file it shows, which may be typed into in place (API
    /// 0.2.15, [`crate::row_edit`]); none for a line only shown.
    pub source: Option<LineSource>,
}

impl VirtualLine {
    /// Its text, without the styles.
    pub fn text(&self) -> String {
        self.pieces.iter().map(|(t, _)| t.as_str()).collect()
    }
}

/// Text a layer shows at a place of a line, not part of the text (API
/// 0.2.20, `layer-5`): a block's reference count at its line's end, a
/// line's author. Shown wherever the cursor is, in every view but the
/// source, as a language server's inlay hints are; the cursor steps over
/// it.
#[derive(Debug, Clone, PartialEq)]
pub struct Insert {
    /// The byte of the text it is shown at, after what ends there.
    pub at: usize,
    /// What is shown, on one line.
    pub text: String,
    /// How.
    pub style: Style,
}

/// Lines drawn under the line starting at byte `line`.
#[derive(Debug, Clone, PartialEq)]
pub struct Below {
    /// The start of the line they are under.
    pub line: usize,
    /// The lines, in order.
    pub lines: Vec<VirtualLine>,
}

/// The lines under lines a document may have in all, and the columns one
/// may be indented: a layer's answer kept to what can be drawn.
const MAX_BELOW: usize = 5000;
const MAX_INDENT: usize = 200;
/// The inserts a document may have in all, and the characters of one.
const MAX_INSERTS: usize = 20_000;
const MAX_INSERT_CHARS: usize = 200;

/// What a layer changes in a document's view.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overlays {
    /// Spans, in order, not overlapping.
    pub spans: Vec<Span>,
    /// Lines, in order, not overlapping.
    pub lines: Vec<Lines>,
    /// Lines under lines, in order of their line, one for a line at most.
    pub below: Vec<Below>,
    /// Text at places of the lines, in order of their place (API 0.2.20).
    pub inserts: Vec<Insert>,
}

impl Overlays {
    /// Moves the overlays with edit `tx` of the text they are of: a span
    /// keeps out what is typed at its edges, lines under a line and text
    /// at a place go with the text after them; [`Overlays::fit`] makes
    /// them whole again.
    fn map(&mut self, tx: &org_edit::Transaction) {
        use org_edit::Assoc::{After, Before};
        let m = tx.mapper();
        for s in &mut self.spans {
            s.range = m.map(s.range.start, After)..m.map(s.range.end, Before);
        }
        for l in &mut self.lines {
            l.range = m.map(l.range.start, After)..m.map(l.range.end, Before);
        }
        for b in &mut self.below {
            b.line = m.map(b.line, After);
        }
        for i in &mut self.inserts {
            i.at = m.map(i.at, After);
        }
    }

    /// The overlays moved with edits ([`Overlays::map`]) made whole for
    /// `text`: spans and places inside it on characters' boundaries,
    /// lines of whole lines, lines under a line at its start; those come
    /// to nothing, or to overlap one before them, left out.
    fn fit(mut self, text: &crate::text::Text) -> Overlays {
        let s = text.as_str();
        let len = s.len();
        let ok = |p: usize| p <= len && s.is_char_boundary(p);
        self.spans
            .retain(|x| x.range.start < x.range.end && ok(x.range.start) && ok(x.range.end));
        let start_of = |p: usize| text.line_start(text.line_of(p.min(len)));
        for l in &mut self.lines {
            let start = start_of(l.range.start);
            let end = l.range.end.min(len);
            let end = if end == len || end == start_of(end) {
                end
            } else {
                text.after_line(text.line_of(end)).min(len)
            };
            l.range = start..end.max(start);
        }
        let mut end = 0;
        self.lines.retain(|l| {
            let keep = l.range.start < l.range.end && l.range.start >= end;
            if keep {
                end = l.range.end;
            }
            keep
        });
        let mut below: Vec<Below> = Vec::with_capacity(self.below.len());
        for mut b in std::mem::take(&mut self.below) {
            b.line = start_of(b.line);
            match below.last_mut() {
                Some(last) if last.line == b.line => last.lines.append(&mut b.lines),
                _ => below.push(b),
            }
        }
        self.below = below;
        self.inserts.retain(|i| ok(i.at));
        self
    }
}

impl Overlays {
    /// The overlays made safe for a text of `len` bytes: inside it, on
    /// character boundaries, in order, overlapping ones dropped (the
    /// first kept), lines widened to whole lines.
    pub fn normalized(mut self, text: &str) -> Overlays {
        let len = text.len();
        let ok = |r: &Range<usize>| {
            r.start < r.end
                && r.end <= len
                && text.is_char_boundary(r.start)
                && text.is_char_boundary(r.end)
        };
        self.spans.retain(|s| ok(&s.range));
        self.spans.sort_by_key(|s| (s.range.start, s.range.end));
        let mut end = 0;
        self.spans.retain(|s| {
            let keep = s.range.start >= end;
            if keep {
                end = s.range.end;
            }
            keep
        });
        let line_start = |p: usize| text[..p].rfind('\n').map_or(0, |i| i + 1);
        let next_line = |p: usize| {
            if p > 0 && text.as_bytes().get(p - 1) == Some(&b'\n') {
                p
            } else {
                text[p..].find('\n').map_or(len, |i| p + i + 1)
            }
        };
        for l in &mut self.lines {
            let start = line_start(l.range.start.min(len));
            let end = next_line(l.range.end.min(len).max(start));
            l.range = start..end;
        }
        self.lines.retain(|l| l.range.start < l.range.end);
        self.lines.sort_by_key(|l| l.range.start);
        let mut end = 0;
        self.lines.retain(|l| {
            let keep = l.range.start >= end;
            if keep {
                end = l.range.end;
            }
            keep
        });
        // Lines under lines: at their line's start, those of one line
        // together, a line feed or a control character in a piece a space.
        let mut below: Vec<Below> = Vec::new();
        let mut count = 0;
        for mut b in std::mem::take(&mut self.below) {
            let mut at = b.line.min(len);
            while !text.is_char_boundary(at) {
                at -= 1;
            }
            b.line = line_start(at);
            for l in &mut b.lines {
                l.indent = l.indent.min(MAX_INDENT);
                // A source in a file of a whole path, of one line.
                if l.source
                    .as_ref()
                    .is_some_and(|s| !s.path.is_absolute() || s.text.contains(['\n', '\r']))
                {
                    l.source = None;
                }
                for (t, _) in &mut l.pieces {
                    if t.contains(char::is_control) {
                        *t = t.replace(char::is_control, " ");
                    }
                }
            }
            b.lines.truncate(MAX_BELOW - count);
            count += b.lines.len();
            if b.lines.is_empty() {
                continue;
            }
            match below.iter_mut().find(|x| x.line == b.line) {
                Some(x) => x.lines.append(&mut b.lines),
                None => below.push(b),
            }
        }
        below.sort_by_key(|b| b.line);
        self.below = below;
        // Text at places: in the text, on a character's boundary, of one
        // line and so many characters, so many in all, in order (those at
        // one place in the order given).
        self.inserts
            .retain(|i| i.at <= len && text.is_char_boundary(i.at) && !i.text.is_empty());
        for i in &mut self.inserts {
            if i.text.contains(char::is_control) {
                i.text = i.text.replace(char::is_control, " ");
            }
            if let Some((cut, _)) = i.text.char_indices().nth(MAX_INSERT_CHARS) {
                i.text.truncate(cut);
            }
        }
        self.inserts.truncate(MAX_INSERTS);
        self.inserts.sort_by_key(|i| i.at);
        self
    }

    /// Nothing to change.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
            && self.lines.is_empty()
            && self.below.is_empty()
            && self.inserts.is_empty()
    }

    /// The lines under the line starting at byte `line`.
    pub fn below(&self, line: usize) -> &[VirtualLine] {
        self.below
            .binary_search_by_key(&line, |b| b.line)
            .map_or(&[], |i| &self.below[i].lines)
    }
}

/// A layer a plugin declares (its manifest's `layers`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerSpec {
    /// The plugin, by its ID.
    pub plugin: String,
    /// The layer's name, which the plugin is asked by.
    pub id: String,
    /// Files or folders one of which a document's folder, or a folder
    /// above it, must hold (`logseq/config.edn`, `.obsidian`).
    pub markers: Vec<String>,
    /// The modes it serves, by name (`markdown`, `org`).
    pub modes: Vec<String>,
    /// The plugin's command that opens the link at the cursor in the
    /// documents it serves, in place of the mode's own on a Ctrl-click
    /// (`graph.openLink`, a link followed by the plugin's own rules).
    pub open: Option<String>,
}

/// What asking a layer for overlays gave.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The overlays.
    Done(Overlays),
    /// The layer failed (a trap, its budget spent): the document is drawn
    /// as its mode draws it, until its text or the layers change.
    Failed,
    /// The plugins are busy (a call of theirs is running): asked again at
    /// the next drawing.
    Busy,
}

/// Asks a layer for a document's overlays: the plugins' bridge.
pub type Provider = dyn Fn(&LayerSpec, Option<&Path>, &str) -> Outcome + Send + Sync;

struct Registry {
    layers: Vec<LayerSpec>,
    provider: Option<Arc<Provider>>,
    /// Whether the provider is asked on a thread of its own (the plugins'
    /// bridge, [`set_background_provider`]), not as the editors draw.
    background: bool,
    /// Whether a folder holds a marker: `(folder, marker)`.
    found: BTreeMap<(PathBuf, String), bool>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    layers: Vec::new(),
    provider: None,
    background: false,
    found: BTreeMap::new(),
});

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Changes when a layer is added or removed, not when one shows anew.
static REGISTERED: AtomicU64 = AtomicU64::new(0);

fn registry() -> std::sync::MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Adds a layer; one of the same plugin and ID is replaced.
pub fn register(spec: LayerSpec) {
    let mut r = registry();
    r.layers
        .retain(|l| !(l.plugin == spec.plugin && l.id == spec.id));
    r.layers.push(spec);
    drop(r);
    REGISTERED.fetch_add(1, Ordering::Relaxed);
    refresh();
}

/// Removes plugin `plugin`'s layers.
pub fn remove_plugin(plugin: &str) {
    let mut r = registry();
    let before = r.layers.len();
    r.layers.retain(|l| l.plugin != plugin);
    let changed = r.layers.len() != before;
    drop(r);
    if changed {
        REGISTERED.fetch_add(1, Ordering::Relaxed);
        refresh();
    }
}

/// The layers.
pub fn layers() -> Vec<LayerSpec> {
    registry().layers.clone()
}

/// Sets what asks the layers for overlays, asked as the editors draw: a
/// layer of the editors' own (a test's).
pub fn set_provider(provider: Arc<Provider>) {
    let mut r = registry();
    r.provider = Some(provider);
    r.background = false;
    drop(r);
    refresh();
}

/// Sets what asks the layers for overlays, asked on a thread of its own
/// while the editors draw what was shown before ([`DocLayer`]): the
/// plugins' bridge, whose layers may take their whole budget.
pub fn set_background_provider(provider: Arc<Provider>) {
    let mut r = registry();
    r.provider = Some(provider);
    r.background = true;
    drop(r);
    refresh();
}

/// Changes when a layer is added or removed (a plugin started or stopped
/// while a document is open): the folders a layer serves are looked for
/// again, a notes graph's offered as a project.
pub fn registered() -> u64 {
    REGISTERED.load(Ordering::Relaxed)
}

/// Changes when the layers or what they show change: views kept of an
/// earlier generation are made again.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// What the layers show changed (a layer added, removed, or one telling
/// so): every document asks again, and the markers are looked for again.
pub fn refresh() {
    registry().found.clear();
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Whether `dir` holds `marker`, remembered until [`refresh`].
fn holds(r: &mut Registry, dir: &Path, marker: &str) -> bool {
    let key = (dir.to_path_buf(), marker.to_string());
    if let Some(found) = r.found.get(&key) {
        return *found;
    }
    let found = std::fs::symlink_metadata(dir.join(marker)).is_ok();
    r.found.insert(key, found);
    found
}

/// The folder holding a layer's marker above `path` (or `path`, a folder),
/// whatever the mode, and the layer: a notes graph's folder
/// (`logseq/config.edn`), a project by its nature.
pub fn marker_root(path: &Path) -> Option<(PathBuf, LayerSpec)> {
    let mut r = registry();
    if r.layers.is_empty() {
        return None;
    }
    let layers = r.layers.clone();
    for dir in path.ancestors().skip(usize::from(!path.is_dir())) {
        for l in &layers {
            if l.markers.iter().any(|m| holds(&mut r, dir, m)) {
                return Some((dir.to_path_buf(), l.clone()));
            }
        }
    }
    None
}

/// The layer serving a document at `path` in mode `mode`: the first whose
/// marker the nearest folder holds.
pub fn layer_for(path: Option<&Path>, mode: &str) -> Option<LayerSpec> {
    let path = path?;
    let mut r = registry();
    if r.layers.is_empty() {
        return None;
    }
    let candidates: Vec<LayerSpec> = r
        .layers
        .iter()
        .filter(|l| l.modes.iter().any(|m| m == mode))
        .cloned()
        .collect();
    for dir in path.ancestors().skip(1) {
        for l in &candidates {
            if l.markers.iter().any(|m| holds(&mut r, dir, m)) {
                return Some(l.clone());
            }
        }
    }
    None
}

/// The overlays of a document, asked of its layer's provider here and
/// now, wherever it is asked as the editors draw.
pub fn compute(spec: &LayerSpec, path: Option<&Path>, text: &str) -> Outcome {
    let provider = registry().provider.clone();
    match provider {
        Some(p) => ask(&*p, spec, path, text),
        None => Outcome::Failed,
    }
}

/// Asks `provider` for the overlays, made safe for `text`.
fn ask(provider: &Provider, spec: &LayerSpec, path: Option<&Path>, text: &str) -> Outcome {
    match provider(spec, path, text) {
        Outcome::Done(o) => Outcome::Done(o.normalized(text)),
        other => other,
    }
}

/// Whether any layer is registered (a check cheap enough for every
/// drawing).
pub fn any() -> bool {
    !registry().layers.is_empty()
}

/// A document's overlays as last asked: for which text version, layers'
/// generation and path, and what they were.
#[derive(Debug, Clone)]
pub struct Cached {
    /// The text's version.
    pub version: u64,
    /// The layers' generation.
    pub generation: u64,
    /// The document's file.
    pub path: Option<PathBuf>,
    /// The overlays, `None` when no layer serves it or the layer failed.
    pub overlays: Option<Arc<Overlays>>,
}

/// None for overlays that change nothing.
fn kept(o: Overlays) -> Option<Arc<Overlays>> {
    (!o.is_empty()).then(|| Arc::new(o))
}

/// A document's overlays as the editors draw them
/// ([`crate::DocumentState::overlays`]): its layer's last answer, moved
/// with the edits made since, and the answer waited for. The plugins'
/// layers are asked on a thread ([`set_background_provider`]), for one
/// version at a time: an answer for a version the text has left is
/// moved with the edits since and asked again for the text as it is.
#[derive(Debug, Default)]
pub struct DocLayer {
    /// The overlays shown.
    shown: Option<Cached>,
    /// Whether `shown` is the layer's answer for its version, generation
    /// and path, not one moved with edits.
    exact: bool,
    /// The edits since the overlays shown, or the answer waited for, were
    /// asked: the version each made and the edit, `None` for a version
    /// that changed no text.
    edits: Vec<(u64, Option<org_edit::Transaction>)>,
    /// The version the answer waited for is of, and the answer, which the
    /// thread asking writes.
    waiting: Option<(u64, Arc<Mutex<Waiting>>)>,
    /// Counts the changes of the overlays shown: the views made with
    /// others are made again ([`crate::mode_view::view_key`]).
    revision: u64,
}

/// An answer waited for: what was asked, and the answer once it came.
#[derive(Debug)]
struct Waiting {
    version: u64,
    generation: u64,
    path: Option<PathBuf>,
    answer: Option<Outcome>,
}

/// Writes a thread's answer, or `Failed` for one that never came (a
/// panic, a thread that did not start).
struct Reply {
    to: Arc<Mutex<Waiting>>,
    sent: bool,
}

impl Reply {
    fn send(mut self, answer: Outcome) {
        self.to.lock().unwrap_or_else(|e| e.into_inner()).answer = Some(answer);
        self.sent = true;
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        if !self.sent {
            self.to.lock().unwrap_or_else(|e| e.into_inner()).answer = Some(Outcome::Failed);
        }
    }
}

impl DocLayer {
    /// The overlays to draw version `version` of `text` with, of a
    /// document at `path` in mode `mode`. Asked of the layer when those
    /// shown are not its answer for them and no answer is waited for: at
    /// once of a provider asked as the editors draw, on a thread of the
    /// plugins' (with the text `shared` gives); meanwhile those shown,
    /// moved with the edits since.
    pub fn overlays(
        &mut self,
        version: u64,
        path: Option<&Path>,
        mode: &str,
        text: &crate::text::Text,
        shared: impl FnOnce() -> Arc<str>,
    ) -> Option<Arc<Overlays>> {
        let generation = generation();
        self.take();
        if self.exact
            && let Some(c) = &self.shown
            && c.version == version
            && c.generation == generation
            && c.path.as_deref() == path
        {
            return c.overlays.clone();
        }
        if self.waiting.is_none() {
            let Some(spec) = layer_for(path, mode) else {
                self.show(version, generation, path, None);
                return None;
            };
            let (provider, background) = {
                let r = registry();
                (r.provider.clone(), r.background)
            };
            let Some(provider) = provider else {
                self.show(version, generation, path, None);
                return None;
            };
            if background {
                let w = Arc::new(Mutex::new(Waiting {
                    version,
                    generation,
                    path: path.map(Path::to_path_buf),
                    answer: None,
                }));
                self.waiting = Some((version, w.clone()));
                let text = shared();
                let path = path.map(Path::to_path_buf);
                let reply = Reply { to: w, sent: false };
                crate::extensions::behind(move || {
                    reply.send(ask(&*provider, &spec, path.as_deref(), &text));
                });
            } else {
                match ask(&*provider, &spec, path, text.as_str()) {
                    Outcome::Done(o) => {
                        self.show(version, generation, path, kept(o));
                        return self.shown.as_ref().and_then(|c| c.overlays.clone());
                    }
                    Outcome::Failed => {
                        self.show(version, generation, path, None);
                        return None;
                    }
                    // Asked again at the next drawing.
                    Outcome::Busy => {}
                }
            }
        }
        self.moved(version, path, text)
    }

    /// Takes in the answer waited for when it came; whether the overlays
    /// shown changed.
    pub fn take(&mut self) -> bool {
        let Some((_, w)) = &self.waiting else {
            return false;
        };
        let mut g = w.lock().unwrap_or_else(|e| e.into_inner());
        let Some(answer) = g.answer.take() else {
            return false;
        };
        let (version, generation, path) = (g.version, g.generation, g.path.take());
        drop(g);
        self.waiting = None;
        let overlays = match answer {
            Outcome::Done(o) => kept(o),
            Outcome::Failed => None,
            // Asked again at the next drawing.
            Outcome::Busy => return false,
        };
        self.show(version, generation, path.as_deref(), overlays);
        true
    }

    /// Whether an answer is waited for.
    pub fn waiting(&self) -> bool {
        self.waiting.is_some()
    }

    /// Counts the changes of the overlays shown.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Edit `tx` made version `version` of the text (`None`: a version
    /// that changed no text): kept, to move the overlays shown or the
    /// answer waited for with it.
    pub fn edited(&mut self, version: u64, tx: Option<&org_edit::Transaction>) {
        let shows = self.shown.as_ref().is_some_and(|c| c.overlays.is_some());
        if shows || self.waiting.is_some() {
            self.edits.push((version, tx.cloned()));
        }
    }

    /// Shows the layer's answer for a version, generation and path.
    fn show(
        &mut self,
        version: u64,
        generation: u64,
        path: Option<&Path>,
        overlays: Option<Arc<Overlays>>,
    ) {
        let before = self.shown.as_ref().and_then(|c| c.overlays.as_ref());
        if before.is_some() || overlays.is_some() {
            self.revision += 1;
        }
        self.shown = Some(Cached {
            version,
            generation,
            path: path.map(Path::to_path_buf),
            overlays,
        });
        self.exact = true;
        self.prune();
    }

    /// Those shown, moved with the edits since their version to version
    /// `version` of `text`; none when they are of another path, or an
    /// edit since is not known.
    fn moved(
        &mut self,
        version: u64,
        path: Option<&Path>,
        text: &crate::text::Text,
    ) -> Option<Arc<Overlays>> {
        let c = self.shown.as_ref()?;
        if c.path.as_deref() != path || c.version > version {
            return None;
        }
        if c.version == version || c.overlays.is_none() {
            return c.overlays.clone();
        }
        let from = self.edits.partition_point(|(v, _)| *v <= c.version);
        let n = usize::try_from(version - c.version).unwrap_or(usize::MAX);
        let edits = &self.edits[from..];
        let known = edits.len() >= n
            && edits[..n]
                .iter()
                .zip(c.version + 1..)
                .all(|((v, _), want)| *v == want);
        let overlays = if known {
            c.overlays.as_deref().map(|o| {
                let mut o = o.clone();
                for tx in edits[..n].iter().filter_map(|(_, tx)| tx.as_ref()) {
                    o.map(tx);
                }
                o.fit(text)
            })
        } else {
            None
        };
        let overlays = overlays.and_then(kept);
        let generation = c.generation;
        self.show(version, generation, path, overlays);
        self.exact = false;
        self.shown.as_ref().and_then(|c| c.overlays.clone())
    }

    /// Lets go of the edits neither the overlays shown nor the answer
    /// waited for need.
    fn prune(&mut self) {
        let shown = self.shown.as_ref().map_or(u64::MAX, |c| c.version);
        let waiting = self.waiting.as_ref().map_or(u64::MAX, |w| w.0);
        let keep = shown.min(waiting);
        self.edits.retain(|(v, _)| *v > keep);
    }
}

/// Whether the span `r` is shown as source: the cursor in it, as markers
/// are (`editor.show_source_markers`).
fn revealed(r: &Range<usize>, cursor: Option<usize>) -> bool {
    match crate::view::source_markers() {
        crate::view::Markers::Always => true,
        crate::view::Markers::Never => false,
        crate::view::Markers::Cursor => cursor.is_some_and(|c| r.start <= c && c <= r.end),
    }
}

/// `base` with `extra`'s set parts added.
fn merged(base: &Style, extra: &Style) -> Style {
    let mut s = *base;
    s.bold |= extra.bold;
    s.italic |= extra.italic;
    s.underline |= extra.underline;
    s.strike |= extra.strike;
    s.code |= extra.code;
    s.link |= extra.link;
    s.dim |= extra.dim;
    s.todo = extra.todo.or(s.todo);
    s.tag |= extra.tag;
    s.timestamp |= extra.timestamp;
    s.priority |= extra.priority;
    s.footnote |= extra.footnote;
    s.target |= extra.target;
    s.expansion |= extra.expansion;
    s.cookie |= extra.cookie;
    s
}

/// Splits the runs of `v` at source offset `at`, where a run holding it
/// is verbatim; whether the runs now break there (or no run holds it).
fn split_at(v: &mut LineView, at: usize) -> bool {
    let Some(i) = v
        .runs
        .iter()
        .position(|r| r.src.start < at && at < r.src.end)
    else {
        return true;
    };
    let r = &v.runs[i];
    if !r.verbatim || r.widget.is_some() {
        return false;
    }
    let k = at - r.src.start;
    if k > r.text.len() || !r.text.is_char_boundary(k) {
        return false;
    }
    let right = Run {
        src: at..r.src.end,
        text: r.text[k..].to_string(),
        verbatim: true,
        style: r.style,
        widget: None,
    };
    let left = &mut v.runs[i];
    left.src.end = at;
    left.text.truncate(k);
    v.runs.insert(i + 1, right);
    true
}

/// Applies `overlays`' spans on the line of `v` to its runs, the cursor at
/// `cursor`, then puts its inserts in.
pub fn apply_line(v: &mut LineView, overlays: &Overlays, cursor: Option<usize>) {
    let line = v.range.clone();
    let first = overlays
        .spans
        .partition_point(|s| s.range.end <= line.start);
    for s in overlays.spans[first..]
        .iter()
        .take_while(|s| s.range.start < line.end.max(line.start + 1))
    {
        let r = s.range.start.max(line.start)..s.range.end.min(line.end);
        if r.is_empty() {
            continue;
        }
        let shown_as_source =
            !matches!(s.effect, SpanEffect::Style(_)) && revealed(&s.range, cursor);
        if shown_as_source {
            continue;
        }
        // The span's edges must fall between runs, or inside verbatim ones.
        if !split_at(v, r.start) || !split_at(v, r.end) {
            continue;
        }
        let inside: Vec<usize> = v
            .runs
            .iter()
            .enumerate()
            .filter(|(_, run)| {
                run.src.start >= r.start && run.src.end <= r.end && !run.src.is_empty()
            })
            .map(|(i, _)| i)
            .collect();
        match &s.effect {
            SpanEffect::Style(st) => {
                for i in inside {
                    v.runs[i].style = merged(&v.runs[i].style, st);
                }
            }
            SpanEffect::Hide => {
                for i in inside {
                    let run = &mut v.runs[i];
                    run.text.clear();
                    run.verbatim = false;
                    run.widget = None;
                }
            }
            SpanEffect::Replace { text, style } => {
                if inside.is_empty() {
                    continue;
                }
                for (n, i) in inside.iter().enumerate() {
                    let run = &mut v.runs[*i];
                    run.verbatim = false;
                    run.widget = None;
                    if n == 0 {
                        run.text = text.clone();
                        run.style = *style;
                    } else {
                        run.text.clear();
                    }
                }
            }
        }
    }
    // Text at places of the line (API 0.2.20), after what ends there.
    let from = overlays.inserts.partition_point(|i| i.at < line.start);
    for i in overlays.inserts[from..]
        .iter()
        .take_while(|i| i.at <= line.end)
    {
        crate::view::insert_text(v, i.at, i.text.clone(), i.style);
    }
}

/// `blocks` cut where `overlays` hide or fold whole lines: those lines a
/// block of their own ([`BlockKind::Hidden`] or [`BlockKind::Drawer`]),
/// the block they were in split around them.
pub fn apply_blocks(blocks: Vec<Block>, overlays: &Overlays) -> Vec<Block> {
    if overlays.lines.is_empty() {
        return blocks;
    }
    let mut out = Vec::with_capacity(blocks.len() + overlays.lines.len() * 2);
    for b in blocks {
        let mut rest = b.range.clone();
        let cuts = overlays.lines.iter().filter(|l| {
            l.range.start >= b.range.start
                && l.range.end <= b.range.end
                && l.range.start < l.range.end
        });
        let mut any = false;
        for l in cuts {
            if l.range.start < rest.start {
                continue;
            }
            any = true;
            if l.range.start > rest.start {
                out.push(Block {
                    kind: b.kind.clone(),
                    range: rest.start..l.range.start,
                    content_end: b.content_end.min(l.range.start).max(rest.start),
                    depth: b.depth,
                    headline: b.headline,
                });
            }
            out.push(Block {
                kind: match l.effect {
                    LineEffect::Hidden => BlockKind::Hidden,
                    LineEffect::Folded => BlockKind::Drawer,
                },
                range: l.range.clone(),
                content_end: l.range.end,
                depth: b.depth,
                headline: b.headline,
            });
            rest.start = l.range.end;
        }
        if !any {
            out.push(b);
        } else if rest.start < rest.end {
            out.push(Block {
                kind: b.kind.clone(),
                content_end: b.content_end.max(rest.start),
                range: rest,
                depth: b.depth,
                headline: b.headline,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests;
