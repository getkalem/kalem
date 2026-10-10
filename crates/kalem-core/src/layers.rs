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

/// What a layer changes in a document's view.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overlays {
    /// Spans, in order, not overlapping.
    pub spans: Vec<Span>,
    /// Lines, in order, not overlapping.
    pub lines: Vec<Lines>,
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
        self
    }

    /// Nothing to change.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty() && self.lines.is_empty()
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
    /// Whether a folder holds a marker: `(folder, marker)`.
    found: BTreeMap<(PathBuf, String), bool>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    layers: Vec::new(),
    provider: None,
    found: BTreeMap::new(),
});

static GENERATION: AtomicU64 = AtomicU64::new(0);

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
        refresh();
    }
}

/// The layers.
pub fn layers() -> Vec<LayerSpec> {
    registry().layers.clone()
}

/// Sets what asks the layers for overlays.
pub fn set_provider(provider: Arc<Provider>) {
    registry().provider = Some(provider);
    refresh();
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

/// The overlays of a document, asked of its layer's provider.
pub fn compute(spec: &LayerSpec, path: Option<&Path>, text: &str) -> Outcome {
    let provider = registry().provider.clone();
    match provider {
        Some(p) => match p(spec, path, text) {
            Outcome::Done(o) => Outcome::Done(o.normalized(text)),
            other => other,
        },
        None => Outcome::Failed,
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

/// Applies `overlays`' spans on the line of `v` to its runs: the cursor
/// at `cursor`.
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
