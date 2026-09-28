//! A minimal WYSIWYG Org editor on gpui: the spike's subject.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    App, AvailableSpace, Bounds, ClipboardItem, Context, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, FontStyle, FontWeight, GlobalElementId, Hsla, InspectorElementId,
    IntoElement, LayoutId, ListAlignment, ListState, PathBuilder, Pixels, Point, SharedString, Size,
    StrikethroughStyle, Style, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div, fill, hsla, list, point,
    prelude::*, px, quad, relative, rgb, size,
};
use org_syntax::{Parse, SyntaxKind, TextEdit, TextSize};

use crate::inline::{InlineLayout, Piece};
use crate::math::{self, MathImage};
use crate::view::{self, BlockKind, LineView, Widget};

actions!(editor, [Left, Right, Up, Down, Backspace, Delete, Enter, Paste, Copy, Home, End, Open, SaveAs]);

pub struct Editor {
    pub path: Option<std::path::PathBuf>,
    pub text: String,
    pub parse: Parse,
    pub line_starts: Vec<usize>,
    pub cursor: usize,
    marked: Option<Range<usize>>,
    pub list: ListState,
    focus: FocusHandle,
    /// Last painted layout of each visible line, for hit testing and IME.
    layouts: Rc<RefCell<HashMap<usize, Painted>>>,
    /// Start offsets of folded headlines.
    folded: BTreeSet<usize>,
    /// Hidden source lines, as sorted, disjoint half-open ranges.
    hidden: Vec<(usize, usize)>,
    math: HashMap<(String, u32), MathState>,
    pub bench: Option<Bench>,
    pub stats: Stats,
}

#[derive(Clone)]
struct Painted {
    bounds: Bounds<Pixels>,
    layout: Rc<InlineLayout>,
    view: Rc<LineView>,
    /// Widget boxes (window coordinates) with their source ranges.
    widgets: Vec<(Bounds<Pixels>, Range<usize>, Widget)>,
    /// The fold arrow of a headline line and the headline's start.
    fold: Option<(Bounds<Pixels>, usize)>,
}

/// A formula image: rendered on a background thread.
enum MathState {
    Pending(Instant),
    Ready(Option<Rc<MathImage>>),
}

#[derive(Default)]
pub struct Stats {
    /// Main-thread work per frame: from the start of render to the last
    /// painted line. Unlike frame intervals, it excludes time the system
    /// was not asking for frames (for example while the window is hidden).
    pub work: Vec<Duration>,
    frame_start: Option<Instant>,
    last_paint: Option<Instant>,
    pub reparse: Vec<Duration>,
    pub view: Duration,
    pub views_built: usize,
    pub math: Duration,
    pub math_rendered: usize,
    /// Time from request to a finished formula image.
    pub math_latency: Vec<Duration>,
}

pub enum Bench {
    /// Scripted interaction that prints the painted layout between steps.
    Script { step: usize },
    Scroll { frames: usize, done: usize, times: Vec<Instant> },
    /// Jump a page (40 lines) per frame: fresh content every frame.
    Jump { frames: usize, done: usize, times: Vec<Instant> },
    Type { chars: usize, done: usize, times: Vec<Instant> },
}

fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0).chain(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1)).collect()
}

impl Editor {
    pub fn new(text: String, cx: &mut Context<Self>) -> Self {
        let parse = org_syntax::parse(&text);
        let starts = line_starts(&text);
        let list = ListState::new(starts.len(), ListAlignment::Top, px(400.));
        Editor {
            path: None,
            text,
            parse,
            line_starts: starts,
            cursor: 0,
            marked: None,
            list,
            focus: cx.focus_handle(),
            layouts: Rc::new(RefCell::new(Default::default())),
            folded: BTreeSet::new(),
            hidden: Vec::new(),
            math: HashMap::new(),
            bench: None,
            stats: Stats::default(),
        }
    }

    pub fn line_of(&self, offset: usize) -> usize {
        self.line_starts.partition_point(|&s| s <= offset).saturating_sub(1)
    }

    pub fn line_range(&self, line: usize) -> (usize, usize) {
        let ls = self.line_starts[line];
        let le = self.line_starts.get(line + 1).map_or(self.text.len(), |&n| n - 1);
        (ls, le.max(ls))
    }

    // Folding: list items are the visible source lines.

    /// Number of visible lines before `line`, which is also the item index
    /// of the first visible line at or after `line`.
    fn visible_before(&self, line: usize) -> usize {
        let mut hidden = 0;
        for &(a, b) in &self.hidden {
            if a >= line {
                break;
            }
            hidden += b.min(line) - a;
        }
        line - hidden
    }

    fn is_hidden(&self, line: usize) -> bool {
        self.hidden.iter().any(|&(a, b)| a <= line && line < b)
    }

    pub fn item_to_line(&self, ix: usize) -> usize {
        let mut line = ix;
        for &(a, b) in &self.hidden {
            if a <= line {
                line += b - a;
            } else {
                break;
            }
        }
        line
    }

    fn item_count(&self) -> usize {
        self.visible_before(self.line_starts.len())
    }

    /// The headline node starting at `offset`, if any.
    fn headline_at(&self, offset: usize) -> Option<org_syntax::SyntaxNode> {
        let root = self.parse.syntax();
        if offset >= self.text.len() {
            return None;
        }
        let tok = root.token_at_offset(TextSize::from(offset as u32)).right_biased()?;
        tok.parent_ancestors()
            .find(|a| matches!(a.kind(), SyntaxKind::HEADLINE) && usize::from(a.text_range().start()) == offset)
    }

    fn compute_hidden(&mut self) {
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        let folded: Vec<usize> = self.folded.iter().copied().collect();
        for start in folded {
            let Some(h) = self.headline_at(start) else {
                self.folded.remove(&start);
                continue;
            };
            let end = usize::from(h.text_range().end());
            let a = self.line_of(start) + 1;
            let b = self.line_of(end.saturating_sub(1).max(start)) + 1;
            if a < b {
                ranges.push((a, b));
            }
        }
        ranges.sort();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (a, b) in ranges {
            match merged.last_mut() {
                Some(last) if a <= last.1 => last.1 = last.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        self.hidden = merged;
    }

    fn toggle_fold(&mut self, start: usize, cx: &mut Context<Self>) {
        let Some(h) = self.headline_at(start) else { return };
        let line = self.line_of(start);
        let end_line = self.line_of(usize::from(h.text_range().end()).saturating_sub(1).max(start)) + 1;
        let old = self.visible_before(line + 1)..self.visible_before(end_line);
        if !self.folded.remove(&start) {
            self.folded.insert(start);
        }
        self.compute_hidden();
        let new = self.visible_before(line + 1)..self.visible_before(end_line);
        // Keep the cursor visible.
        let cl = self.line_of(self.cursor);
        if self.is_hidden(cl) {
            self.cursor = start;
        }
        let item = self.visible_before(line);
        self.list.splice(item..item + 1, 1);
        self.list.splice(old, new.len());
        cx.notify();
    }

    /// Replaces `range` with `new`, reparsing incrementally.
    pub fn edit(&mut self, range: Range<usize>, new: &str, cx: &mut Context<Self>) {
        let first_line = self.line_of(range.start);
        let last_line = self.line_of(range.end);
        let edit = TextEdit { range: view::range(range.start, range.end), insert: new.to_string() };
        let new_text = edit.apply(&self.text);
        let t = Instant::now();
        self.parse = self.parse.reparse(&new_text, &edit);
        self.stats.reparse.push(t.elapsed());
        self.text = new_text;
        self.line_starts = line_starts(&self.text);
        let new_last = self.line_of(range.start + new.len());
        // Lines around the edit can change their display (markers appear or
        // disappear); remeasure a small window.
        let lo = first_line.saturating_sub(1);
        let hi_old = (last_line + 2).min(self.line_starts.len() + last_line - new_last);
        let hi_new = (new_last + 2).min(self.line_starts.len());
        let old_items = self.visible_before(lo)..self.visible_before(hi_old);
        if !self.folded.is_empty() {
            let delta = new.len() as isize - (range.end - range.start) as isize;
            let old_hidden = std::mem::take(&mut self.hidden);
            self.folded = self
                .folded
                .iter()
                .filter(|&&f| f < range.start || f >= range.end)
                .map(|&f| if f >= range.end { (f as isize + delta) as usize } else { f })
                .collect();
            self.compute_hidden();
            // Hidden ranges away from the edit must only have shifted;
            // otherwise the edit changed the outline: rebuild the list.
            let ld = new_last as isize - last_line as isize;
            let shifted: Vec<(usize, usize)> = old_hidden
                .iter()
                .map(|&(a, b)| if a >= hi_old { ((a as isize + ld) as usize, (b as isize + ld) as usize) } else { (a, b) })
                .collect();
            if shifted != self.hidden {
                self.list.reset(self.item_count());
                self.cursor = range.start + new.len();
                cx.notify();
                return;
            }
        }
        let new_items = self.visible_before(lo)..self.visible_before(hi_new);
        self.list.splice(old_items, new_items.len());
        self.cursor = range.start + new.len();
        cx.notify();
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let old_line = self.line_of(self.cursor);
        self.cursor = offset.min(self.text.len());
        let new_line = self.line_of(self.cursor);
        // Reveal and hide markers on the lines the cursor left and entered.
        for l in [old_line, new_line] {
            if !self.is_hidden(l) {
                let i = self.visible_before(l);
                self.list.splice(i..i + 1, 1);
            }
        }
        cx.notify();
    }

    fn prev_boundary(&self, o: usize) -> usize {
        self.text[..o].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self, o: usize) -> usize {
        self.text[o..].chars().next().map_or(o, |c| o + c.len_utf8())
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.prev_boundary(self.cursor), cx);
    }
    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.next_boundary(self.cursor), cx);
    }
    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        let (ls, _) = self.line_range(self.line_of(self.cursor));
        self.move_to(ls, cx);
    }
    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        let (_, le) = self.line_range(self.line_of(self.cursor));
        self.move_to(le, cx);
    }
    fn vertical(&mut self, delta: isize, cx: &mut Context<Self>) {
        let line = self.line_of(self.cursor);
        let last = self.line_starts.len() as isize - 1;
        let mut target = (line as isize + delta).clamp(0, last) as usize;
        while self.is_hidden(target) && (target as isize + delta.signum()).clamp(0, last) as usize != target {
            target = (target as isize + delta.signum()) as usize;
        }
        if self.is_hidden(target) {
            return;
        }
        let col = self.cursor - self.line_starts[line];
        let (ls, le) = self.line_range(target);
        let mut o = (ls + col).min(le);
        while !self.text.is_char_boundary(o) {
            o -= 1;
        }
        self.move_to(o, cx);
        self.list.scroll_to_reveal_item(self.visible_before(target));
    }
    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(-1, cx);
    }
    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(1, cx);
    }
    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.cursor > 0 {
            let p = self.prev_boundary(self.cursor);
            self.edit(p..self.cursor, "", cx);
        }
    }
    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.cursor < self.text.len() {
            let n = self.next_boundary(self.cursor);
            self.edit(self.cursor..n, "", cx);
        }
    }
    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        self.edit(self.cursor..self.cursor, "\n", cx);
    }
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(t) = cx.read_from_clipboard().and_then(|i| i.text()) {
            self.edit(self.cursor..self.cursor, &t, cx);
        }
    }
    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let (ls, le) = self.line_range(self.line_of(self.cursor));
        cx.write_to_clipboard(ClipboardItem::new_string(self.text[ls..le].to_string()));
    }

    pub fn build_view(&mut self, line: usize) -> LineView {
        let (ls, le) = self.line_range(line);
        let t = Instant::now();
        let v = view::line_view(&self.parse.syntax(), ls, le, Some(self.cursor));
        self.stats.view += t.elapsed();
        self.stats.views_built += 1;
        v
    }

    /// The formula image for `src`, or `None` while it renders. Rendering
    /// runs on the background executor; when it finishes, the lines showing
    /// the formula are measured again.
    fn math_image(&mut self, src: &str, font_size: Pixels, scale: f32, cx: &mut Context<Self>) -> Option<Option<Rc<MathImage>>> {
        let key = (src.to_string(), (f32::from(font_size) * scale * 16.) as u32);
        match self.math.get(&key) {
            Some(MathState::Ready(m)) => return Some(m.clone()),
            Some(MathState::Pending(_)) => return None,
            None => {}
        }
        self.math.insert(key.clone(), MathState::Pending(Instant::now()));
        let (src, fs) = (src.to_string(), f32::from(font_size));
        let task = cx.background_executor().spawn(async move {
            let t = Instant::now();
            let m = math::render(&src, fs, scale);
            (m, t.elapsed())
        });
        cx.spawn(async move |this, cx| {
            let (m, took) = task.await;
            let _ = this.update(cx, |e, cx| {
                if let Some(MathState::Pending(asked)) = e.math.get(&key) {
                    e.stats.math_latency.push(asked.elapsed());
                }
                e.stats.math += took;
                e.stats.math_rendered += 1;
                e.math.insert(key.clone(), MathState::Ready(m.map(Rc::new)));
                e.remeasure_formula(&key.0);
                cx.notify();
            });
        })
        .detach();
        None
    }

    /// Measures the painted lines showing formula `src` again.
    fn remeasure_formula(&mut self, src: &str) {
        let lines: Vec<usize> = self
            .layouts
            .borrow()
            .iter()
            .filter(|(_, p)| p.widgets.iter().any(|(_, _, w)| matches!(w, Widget::Math(m) if m == src)))
            .map(|(l, _)| *l)
            .collect();
        for l in lines {
            if l < self.line_starts.len() && !self.is_hidden(l) {
                let i = self.visible_before(l);
                self.list.splice(i..i + 1, 1);
            }
        }
    }

    /// The IME works on the cursor's line: offsets are UTF-16 units from
    /// the line start.
    fn ime_line(&self) -> (usize, usize) {
        self.line_range(self.line_of(self.cursor))
    }

    fn utf16_to_offset(&self, ls: usize, le: usize, u: usize) -> usize {
        let mut n = 0;
        for (i, c) in self.text[ls..le].char_indices() {
            if n >= u {
                return ls + i;
            }
            n += c.len_utf16();
        }
        le
    }

    fn offset_to_utf16(&self, ls: usize, o: usize) -> usize {
        self.text[ls..o.max(ls)].chars().map(char::len_utf16).sum()
    }

    /// Source offset under a window position, from the last painted lines.
    fn offset_at(&self, pos: Point<Pixels>) -> Option<usize> {
        let layouts = self.layouts.borrow();
        let (line, p) = layouts.iter().find(|(_, p)| p.bounds.contains(&pos))?;
        let (_, le) = self.line_range(*line);
        let d = p.layout.index_for_position(pos - p.bounds.origin);
        Some(p.view.source_offset(d, le))
    }

    fn on_mouse_down(&mut self, ev: &gpui::MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        self.click(ev.position, cx);
    }

    fn click(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let ev = gpui::MouseDownEvent { position, ..Default::default() };
        // Fold arrows and widgets first.
        let hit = {
            let layouts = self.layouts.borrow();
            let mut hit = None;
            for p in layouts.values() {
                if let Some((b, start)) = p.fold
                    && b.contains(&ev.position)
                {
                    hit = Some((None, Some(start)));
                }
                for (b, r, w) in &p.widgets {
                    if b.contains(&ev.position) {
                        hit = Some((Some((r.clone(), w.clone())), None));
                    }
                }
            }
            hit
        };
        match hit {
            Some((_, Some(start))) => return self.toggle_fold(start, cx),
            Some((Some((r, Widget::Checkbox(state))), _)) => {
                let new = if state == b'X' { "[ ]" } else { "[X]" };
                let keep = self.cursor;
                self.edit(r, new, cx);
                self.move_to(keep, cx);
                return;
            }
            Some((Some((r, Widget::Math(_))), _)) => {
                // Clicking a formula reveals its source.
                return self.move_to(r.start + 1, cx);
            }
            _ => {}
        }
        if let Some(o) = self.offset_at(ev.position) {
            self.move_to(o, cx);
        }
    }

    /// Replaces the document.
    pub fn load(&mut self, path: std::path::PathBuf, text: String, cx: &mut Context<Self>) {
        self.parse = org_syntax::parse(&text);
        self.line_starts = line_starts(&text);
        self.text = text;
        self.path = Some(path);
        self.cursor = 0;
        self.folded.clear();
        self.hidden.clear();
        self.list.reset(self.line_starts.len());
        cx.notify();
    }

    fn open(&mut self, _: &Open, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            if let Ok(text) = std::fs::read_to_string(&path) {
                let _ = this.update(cx, |e, cx| e.load(path, text, cx));
            }
        })
        .detach();
    }

    fn save_as(&mut self, _: &SaveAs, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self.path.as_ref().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_else(std::env::temp_dir);
        let name = self.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
        let rx = cx.prompt_for_new_path(&dir, name.as_deref());
        let text = self.text.clone();
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else { return };
            if std::fs::write(&path, text).is_ok() {
                let _ = this.update(cx, |e, _| e.path = Some(path));
            }
        })
        .detach();
    }

    fn on_drop(&mut self, paths: &gpui::ExternalPaths, _: &mut Window, cx: &mut Context<Self>) {
        let links: Vec<String> = paths.paths().iter().map(|p| format!("[[file:{}]]", p.display())).collect();
        let at = self.cursor;
        self.edit(at..at, &links.join(" "), cx);
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(&mut self, r: Range<usize>, actual: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let (ls, le) = self.ime_line();
        let (a, b) = (self.utf16_to_offset(ls, le, r.start), self.utf16_to_offset(ls, le, r.end));
        actual.replace(self.offset_to_utf16(ls, a)..self.offset_to_utf16(ls, b));
        Some(self.text[a..b].to_string())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let (ls, _) = self.ime_line();
        let c = self.offset_to_utf16(ls, self.cursor);
        Some(UTF16Selection { range: c..c, reversed: false })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let (ls, _) = self.ime_line();
        self.marked.as_ref().map(|m| self.offset_to_utf16(ls, m.start)..self.offset_to_utf16(ls, m.end))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(&mut self, r: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        let (ls, le) = self.ime_line();
        let range = r
            .map(|r| self.utf16_to_offset(ls, le, r.start)..self.utf16_to_offset(ls, le, r.end))
            .or(self.marked.take())
            .unwrap_or(self.cursor..self.cursor);
        self.marked = None;
        self.edit(range, text, cx);
    }

    fn replace_and_mark_text_in_range(&mut self, r: Option<Range<usize>>, text: &str, _sel: Option<Range<usize>>, _: &mut Window, cx: &mut Context<Self>) {
        let (ls, le) = self.ime_line();
        let range = r
            .map(|r| self.utf16_to_offset(ls, le, r.start)..self.utf16_to_offset(ls, le, r.end))
            .or(self.marked.clone())
            .unwrap_or(self.cursor..self.cursor);
        let start = range.start;
        self.edit(range, text, cx);
        self.marked = (!text.is_empty()).then(|| start..start + text.len());
    }

    fn bounds_for_range(&mut self, r: Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let line = self.line_of(self.cursor);
        let (ls, le) = self.line_range(line);
        let layouts = self.layouts.borrow();
        let p = layouts.get(&line)?;
        let a = self.utf16_to_offset(ls, le, r.start);
        let caret = p.layout.caret(p.view.display_offset(a));
        Some(Bounds::new(p.bounds.origin + caret.origin, caret.size))
    }

    fn character_index_for_point(&mut self, pt: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let line = self.line_of(self.cursor);
        let (ls, le) = self.line_range(line);
        let layouts = self.layouts.borrow();
        let p = layouts.get(&line)?;
        let d = p.layout.index_for_position(pt - p.bounds.origin);
        Some(self.offset_to_utf16(ls, p.view.source_offset(d, le)))
    }
}

/// Text style for a line view segment.
fn run(seg: &view::Seg, len: usize, base: &gpui::Font, color: Hsla, mono_family: &SharedString) -> TextRun {
    let s = &seg.sty;
    let mut font = base.clone();
    if s.mono {
        font.family = mono_family.clone();
    }
    if s.bold || s.title || s.todo.is_some() {
        font.weight = FontWeight::BOLD;
    }
    if s.italic {
        font.style = FontStyle::Italic;
    }
    let color = if s.link {
        hsla(0.6, 0.8, 0.45, 1.)
    } else if let Some(done) = s.todo {
        if done { hsla(0.33, 0.7, 0.35, 1.) } else { hsla(0.0, 0.75, 0.5, 1.) }
    } else if s.tag || s.dim {
        hsla(0., 0., 0.55, 1.)
    } else if s.timestamp {
        hsla(0.78, 0.5, 0.45, 1.)
    } else if s.priority {
        hsla(0.08, 0.9, 0.5, 1.)
    } else {
        color
    };
    TextRun {
        len,
        font,
        color,
        background_color: s.code_bg.then(|| hsla(0., 0., 0.93, 1.)),
        underline: (s.underline || s.link).then(|| UnderlineStyle { color: Some(color), thickness: px(1.), wavy: false }),
        strikethrough: s.strike.then(|| StrikethroughStyle { color: Some(color), thickness: px(1.) }),
    }
}

/// What a widget paints.
#[derive(Clone)]
enum WidgetPaint {
    Checkbox(u8),
    Math(Rc<MathImage>),
    /// A formula that failed to render.
    Broken,
    /// A formula still rendering.
    Pending,
}

struct Prepared {
    view: Rc<LineView>,
    pieces: Vec<Piece>,
    /// Widgets by display offset.
    widgets: Vec<(usize, Range<usize>, Widget, WidgetPaint)>,
    font_size: Pixels,
    folded: Option<bool>,
}

/// Turns a line view into layout pieces, rendering formulas as needed.
fn prepare(editor: &mut Editor, line: usize, base: Pixels, font: &gpui::Font, color: Hsla, scale: f32, cx: &mut Context<Editor>) -> Prepared {
    let view = editor.build_view(line);
    let font_size = font_size_for(&view, base);
    let mono: SharedString = "Menlo".into();
    let mut pieces = Vec::new();
    let mut widgets = Vec::new();
    let (mut text, mut runs) = (String::new(), Vec::new());
    let mut at = 0;
    for seg in &view.segs {
        let paint = match &seg.widget {
            Some(Widget::Checkbox(s)) => Some(WidgetPaint::Checkbox(*s)),
            Some(Widget::Math(src)) => Some(match editor.math_image(src, font_size, scale, cx) {
                Some(Some(m)) => WidgetPaint::Math(m),
                Some(None) => WidgetPaint::Broken,
                None => WidgetPaint::Pending,
            }),
            None => None,
        };
        match paint {
            Some(p) => {
                if !text.is_empty() {
                    pieces.push(Piece::Text { text: std::mem::take(&mut text), runs: std::mem::take(&mut runs) });
                }
                let (sz, ascent) = match &p {
                    WidgetPaint::Checkbox(_) => (size(font_size * 0.95, font_size * 0.95), font_size * 0.8),
                    WidgetPaint::Math(m) => (m.size, m.ascent),
                    WidgetPaint::Broken => (size(font_size, font_size), font_size * 0.8),
                    // Estimated from the source length.
                    WidgetPaint::Pending => {
                        let n = seg.widget.as_ref().map_or(1, |w| match w {
                            Widget::Math(src) => math::body(src).map_or(1, |b| b.chars().count()),
                            _ => 1,
                        });
                        (size(font_size * 0.45 * n as f32, font_size * 1.15), font_size * 0.85)
                    }
                };
                pieces.push(Piece::Widget { len: seg.text.len(), size: sz, ascent });
                widgets.push((at, seg.src_start..seg.src_end, seg.widget.clone().expect("widget"), p));
            }
            None => {
                runs.push(run(seg, seg.text.len(), font, color, &mono));
                text.push_str(&seg.text);
            }
        }
        at += seg.text.len();
    }
    let (ls, _) = editor.line_range(line);
    let folded = (view.heading > 0).then(|| editor.folded.contains(&ls));
    if folded == Some(true) {
        let seg = view::Seg { src_start: 0, src_end: 0, text: String::new(), verbatim: false, sty: view::Sty { dim: true, ..Default::default() }, widget: None };
        runs.push(run(&seg, " …".len(), font, color, &mono));
        text.push_str(" …");
    }
    if !text.is_empty() {
        pieces.push(Piece::Text { text, runs });
    }
    Prepared { view: Rc::new(view), pieces, widgets, font_size, folded }
}

/// One display line: shaped and wrapped text with widgets and the cursor.
pub struct LineElement {
    editor: Entity<Editor>,
    line: usize,
}

type Shaped = (Rc<InlineLayout>, Rc<LineView>, Vec<(usize, Range<usize>, Widget, WidgetPaint)>, Option<bool>, Pixels);

pub struct LineState {
    shaped: Rc<RefCell<Option<Shaped>>>,
}

impl IntoElement for LineElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

fn font_size_for(view: &LineView, base: Pixels) -> Pixels {
    if view.segs.iter().any(|s| s.sty.title) {
        return base * 1.8;
    }
    match view.heading {
        1 => base * 1.6,
        2 => base * 1.35,
        3 => base * 1.2,
        _ if view.block == BlockKind::Meta => base * 0.85,
        _ => base,
    }
}

impl Element for LineElement {
    type RequestLayoutState = LineState;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, LineState) {
        let text_style = window.text_style();
        let base_size = text_style.font_size.to_pixels(window.rem_size());
        let font = text_style.font();
        let color = text_style.color;
        let scale = window.scale_factor();
        let line = self.line;
        let prepared = self.editor.update(cx, |e, cx| prepare(e, line, base_size, &font, color, scale, cx));
        let shaped: Rc<RefCell<Option<Shaped>>> = Rc::new(RefCell::new(None));
        let slot = shaped.clone();
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let prepared = Rc::new(RefCell::new(Some(prepared)));
        let layout_id = window.request_measured_layout(style, move |_known, available, window, _cx| {
            let wrap = match available.width {
                AvailableSpace::Definite(w) => Some(w),
                _ => None,
            };
            // Measuring can run more than once; reuse the layout when the
            // width did not change.
            if let Some((layout, .., line_height)) = slot.borrow().as_ref()
                && wrap.is_none_or(|w| layout.width == w)
            {
                return Size { width: layout.width, height: layout.height.max(*line_height) };
            }
            let p = prepared.borrow();
            let Some(p) = p.as_ref() else { return Size::default() };
            let line_height = p.font_size * 1.45;
            let layout = InlineLayout::new(&p.pieces, p.font_size, line_height, wrap, window);
            let sz = Size { width: layout.width, height: layout.height.max(line_height) };
            *slot.borrow_mut() = Some((Rc::new(layout), p.view.clone(), p.widgets.clone(), p.folded, line_height));
            sz
        });
        (layout_id, LineState { shaped })
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut LineState, _: &mut Window, _: &mut App) {}

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, state: &mut LineState, _: &mut (), window: &mut Window, cx: &mut App) {
        let Some((layout, view, widgets, folded, line_height)) = state.shaped.borrow().clone() else { return };
        let editor = self.editor.read(cx);
        let focus = editor.focus.clone();
        let cursor = editor.cursor;
        let (ls, le) = editor.line_range(self.line);
        let layouts = editor.layouts.clone();
        if view.block == BlockKind::Code {
            window.paint_quad(fill(bounds, rgb(0xf5f5f7)));
        }
        layout.paint(bounds.origin, window, cx);

        // Widgets.
        let boxes: HashMap<usize, Bounds<Pixels>> = layout.widgets().collect();
        let mut hit = Vec::new();
        for (d, src, w, paint) in &widgets {
            let Some(b) = boxes.get(d) else { continue };
            let b = Bounds::new(bounds.origin + b.origin, b.size);
            match paint {
                WidgetPaint::Checkbox(s) => paint_checkbox(b, *s, window),
                WidgetPaint::Math(m) => {
                    let _ = window.paint_image(b, Default::default(), m.image.clone(), 0, false);
                }
                WidgetPaint::Broken => {
                    window.paint_quad(quad(b, px(2.), hsla(0., 0.8, 0.95, 1.), px(1.), hsla(0., 0.8, 0.5, 1.), Default::default()));
                }
                WidgetPaint::Pending => {
                    window.paint_quad(quad(b, px(3.), hsla(0., 0., 0.95, 1.), px(0.), hsla(0., 0., 0.95, 1.), Default::default()));
                }
            }
            hit.push((b, src.clone(), w.clone()));
        }

        // Fold arrow in the left margin.
        let fold = folded.map(|f| {
            let caret = layout.caret(0);
            let s = px(9.);
            let c = point(bounds.origin.x - px(20.), bounds.origin.y + caret.origin.y + line_height / 2.);
            let mut path = PathBuilder::fill();
            if f {
                path.add_polygon(&[point(c.x - s / 3., c.y - s / 2.), point(c.x + s / 2., c.y), point(c.x - s / 3., c.y + s / 2.)], true);
            } else {
                path.add_polygon(&[point(c.x - s / 2., c.y - s / 3.), point(c.x + s / 2., c.y - s / 3.), point(c.x, c.y + s / 2.)], true);
            }
            if let Ok(p) = path.build() {
                window.paint_path(p, hsla(0., 0., 0.6, 1.));
            }
            (Bounds::new(point(c.x - px(10.), c.y - px(10.)), size(px(20.), px(20.))), ls)
        });

        if ls <= cursor && cursor <= le {
            window.handle_input(&focus, ElementInputHandler::new(bounds, self.editor.clone()), cx);
            let caret = layout.caret(view.display_offset(cursor));
            window.paint_quad(fill(Bounds::new(bounds.origin + caret.origin, caret.size), rgb(0x3060ff)));
        }
        layouts.borrow_mut().insert(self.line, Painted { bounds, layout, view, widgets: hit, fold });
        self.editor.update(cx, |e, _| e.stats.last_paint = Some(Instant::now()));
    }
}

fn paint_checkbox(b: Bounds<Pixels>, state: u8, window: &mut Window) {
    let inset = Bounds::new(b.origin + point(px(1.), px(1.)), size(b.size.width - px(2.), b.size.height - px(2.)));
    let blue = hsla(0.6, 0.75, 0.5, 1.);
    let (bg, border) = if state == b' ' { (hsla(0., 0., 1., 1.), hsla(0., 0., 0.55, 1.)) } else { (blue, blue) };
    window.paint_quad(quad(inset, px(3.), bg, px(1.5), border, Default::default()));
    let w = inset.size.width;
    let o = inset.origin;
    match state {
        b'X' => {
            let mut p = PathBuilder::stroke(px(2.));
            p.move_to(o + point(w * 0.22, w * 0.52));
            p.line_to(o + point(w * 0.42, w * 0.72));
            p.line_to(o + point(w * 0.78, w * 0.3));
            if let Ok(path) = p.build() {
                window.paint_path(path, hsla(0., 0., 1., 1.));
            }
        }
        b'-' => {
            let bar = Bounds::new(o + point(w * 0.22, w * 0.45), size(w * 0.56, w * 0.1));
            window.paint_quad(fill(bar, hsla(0., 0., 1., 1.)));
        }
        _ => {}
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.bench_step(window, cx);
        self.layouts.borrow_mut().clear();
        let entity = cx.entity();
        div()
            .key_context("Editor")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::save_as))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_drop(cx.listener(Self::on_drop))
            .size_full()
            .bg(rgb(0xffffff))
            .text_color(rgb(0x1f2328))
            .text_size(px(16.))
            .px(px(48.))
            .py(px(24.))
            .child(
                list(self.list.clone(), move |ix, _window, cx| {
                    let line = entity.read(cx).item_to_line(ix);
                    LineElement { editor: entity.clone(), line }.into_any_element()
                })
                .size_full(),
            )
    }
}

impl Editor {
    fn bench_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.bench.is_none() {
            return;
        }
        let now = Instant::now();
        if let (Some(a), Some(b)) = (self.stats.frame_start, self.stats.last_paint)
            && b > a
        {
            self.stats.work.push(b - a);
        }
        self.stats.frame_start = Some(now);
        let Some(bench) = self.bench.as_mut() else { return };
        match bench {
            Bench::Script { step } => {
                *step += 1;
                let step = *step;
                match step {
                    2 => {
                        self.dump("initial");
                        let fold = self.layouts.borrow().values().filter_map(|p| p.fold).find(|(_, s)| self.text[*s..].starts_with("* Folded"));
                        if let Some((b, _)) = fold {
                            self.click(b.center(), cx);
                        }
                    }
                    4 => {
                        self.dump("after folding the second section");
                        let cb = self.layouts.borrow().values().flat_map(|p| p.widgets.clone()).find(|(_, _, w)| *w == Widget::Checkbox(b' '));
                        if let Some((b, _, _)) = cb {
                            self.click(b.center(), cx);
                        }
                        let f = self.layouts.borrow().values().flat_map(|p| p.widgets.clone()).find(|(_, _, w)| matches!(w, Widget::Math(_)));
                        if let Some((b, _, _)) = f {
                            self.click(b.center(), cx);
                        }
                    }
                    6 => {
                        self.dump("after clicking the empty checkbox and the first formula");
                        cx.quit();
                        return;
                    }
                    _ => {}
                }
            }
            Bench::Scroll { frames, done, times } => {
                times.push(now);
                *done += 1;
                if *done >= *frames {
                    report("scroll", times, &self.stats);
                    println!("  scrolled to item {}", self.list.logical_scroll_top().item_ix);
                    cx.quit();
                    return;
                }
                // Items have no height until the first layout; scrolling
                // before it would jump to the end.
                if *done > 3 {
                    self.list.scroll_by(px(37.));
                }
            }
            Bench::Jump { frames, done, times } => {
                times.push(now);
                *done += 1;
                if *done >= *frames {
                    report("page jumps", times, &self.stats);
                    println!("  scrolled to item {}", self.list.logical_scroll_top().item_ix);
                    cx.quit();
                    return;
                }
                let ix = (*done * 40) % self.list.item_count().max(1);
                self.list.scroll_to(gpui::ListOffset { item_ix: ix, offset_in_item: px(0.) });
            }
            Bench::Type { chars, done, times } => {
                times.push(now);
                *done += 1;
                if *done >= *chars {
                    report("typing", times, &self.stats);
                    cx.quit();
                    return;
                }
                let c = if *done % 7 == 0 { " " } else { "x" };
                let at = self.cursor;
                self.edit(at..at, c, cx);
            }
        }
        window.request_animation_frame();
    }
}

impl Editor {
    fn dump(&self, title: &str) {
        println!("== {title} (cursor {})", self.cursor);
        let layouts = self.layouts.borrow();
        let mut lines: Vec<_> = layouts.iter().collect();
        lines.sort_by_key(|(l, _)| **l);
        for (l, p) in lines {
            let (ls, le) = self.line_range(*l);
            let rows: Vec<String> = p
                .layout
                .rows
                .iter()
                .map(|r| format!("[{}..{} y{:.0} h{:.0}]", r.start, r.end, f32::from(r.y), f32::from(r.height)))
                .collect();
            let widgets: Vec<String> = p
                .widgets
                .iter()
                .map(|(b, _, w)| {
                    let k = match w {
                        Widget::Checkbox(s) => format!("checkbox'{}'", *s as char),
                        Widget::Math(m) => format!("math{m:?}"),
                    };
                    format!("{k}@({:.0},{:.0} {:.0}x{:.0})", f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height))
                })
                .collect();
            let fold = p.fold.map_or(String::new(), |_| if self.folded.contains(&ls) { " folded".into() } else { " open".into() });
            println!(
                "line {l:>2} y{:>4.0} h{:>3.0}{fold} rows {} {} | {:?}",
                f32::from(p.bounds.origin.y),
                f32::from(p.bounds.size.height),
                rows.join(""),
                widgets.join(" "),
                &self.text[ls..le]
            );
        }
    }
}

fn report(name: &str, times: &[Instant], stats: &Stats) {
    let mut d: Vec<f64> = times.windows(2).map(|w| (w[1] - w[0]).as_secs_f64() * 1000.).collect();
    if d.len() > 10 {
        d.drain(..5); // warm-up frames
    }
    d.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |q: f64| d[((d.len() as f64 - 1.) * q) as usize];
    let slow = d.iter().filter(|&&x| x > 17.).count();
    println!("{name}: {} frames, interval p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms, max {:.2} ms, over 17 ms: {slow}", d.len(), p(0.5), p(0.95), p(0.99), p(1.0));
    let mut w: Vec<f64> = stats.work.iter().map(|x| x.as_secs_f64() * 1000.).collect();
    if w.len() > 10 {
        w.drain(..5);
    }
    w.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if !w.is_empty() {
        let q = |q: f64| w[((w.len() as f64 - 1.) * q) as usize];
        let slow = w.iter().filter(|&&x| x > 8.).count();
        println!("  main-thread work per frame: p50 {:.2} ms, p99 {:.2} ms, max {:.2} ms, over 8 ms: {slow}", q(0.5), q(0.99), q(1.0));
    }
    if !stats.reparse.is_empty() {
        let mut r: Vec<f64> = stats.reparse.iter().map(|x| x.as_secs_f64() * 1e6).collect();
        r.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("  reparse per edit: p50 {:.0} µs, max {:.0} µs", r[r.len() / 2], r[r.len() - 1]);
    }
    println!("  line views built: {} in {:.1} ms", stats.views_built, stats.view.as_secs_f64() * 1000.);
    if stats.math_rendered > 0 {
        println!("  formulas rendered: {} in {:.1} ms of background time", stats.math_rendered, stats.math.as_secs_f64() * 1000.);
        let mut l: Vec<f64> = stats.math_latency.iter().map(|x| x.as_secs_f64() * 1000.).collect();
        l.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if !l.is_empty() {
            println!("  formula latency: p50 {:.1} ms, p99 {:.1} ms", l[l.len() / 2], l[(l.len() - 1) * 99 / 100]);
        }
    }
}
