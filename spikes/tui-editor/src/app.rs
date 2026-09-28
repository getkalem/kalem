//! Editor state and the terminal event loop.

use std::collections::HashMap;
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use org_syntax::{Parse, TextEdit, TextRange, TextSize};
use ratatui::Frame;
use ratatui::buffer::CellDiffOption;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui_image::StatefulImage;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use unicode_segmentation::UnicodeSegmentation;

use crate::caps::Caps;
use crate::render::{self, Glyph};
use crate::view::{self, Widget};

struct RowHit {
    y: u16,
    x0: u16,
    line: usize,
    glyphs: Vec<Glyph>,
}

#[derive(Default)]
pub struct Stats {
    pub frame: Vec<Duration>,
    pub reparse: Vec<Duration>,
}

pub struct App {
    pub text: String,
    pub parse: Parse,
    starts: Vec<usize>,
    pub cursor: usize,
    pub top: usize,
    pub path: PathBuf,
    pub caps: Caps,
    pub picker: Option<Picker>,
    images: HashMap<String, Option<(StatefulProtocol, u16, u16)>>,
    rows: Vec<RowHit>,
    pub status: String,
    pub quit: bool,
    pub stats: Stats,
    modified: bool,
    /// Text area size of the last frame.
    size: (u16, u16),
    last_cursor: Option<usize>,
}

fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0).chain(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1)).collect()
}

impl App {
    pub fn new(path: PathBuf, text: String, caps: Caps, picker: Option<Picker>) -> Self {
        let parse = org_syntax::parse(&text);
        App {
            starts: line_starts(&text),
            text,
            parse,
            cursor: 0,
            top: 0,
            path,
            caps,
            picker,
            images: HashMap::new(),
            rows: Vec::new(),
            status: "ctrl-s save · ctrl-q quit".into(),
            quit: false,
            stats: Stats::default(),
            modified: false,
            size: (80, 24),
            last_cursor: None,
        }
    }

    pub fn line_count(&self) -> usize {
        self.starts.len()
    }

    fn line_of(&self, o: usize) -> usize {
        self.starts.partition_point(|&s| s <= o).saturating_sub(1)
    }

    fn line_range(&self, line: usize) -> (usize, usize) {
        let ls = self.starts[line];
        let le = self.starts.get(line + 1).map_or(self.text.len(), |&n| n - 1);
        (ls, le.max(ls))
    }

    pub fn edit(&mut self, a: usize, b: usize, insert: &str) {
        let edit = TextEdit { range: TextRange::new(TextSize::from(a as u32), TextSize::from(b as u32)), insert: insert.into() };
        let new_text = edit.apply(&self.text);
        let t = Instant::now();
        self.parse = self.parse.reparse(&new_text, &edit);
        self.stats.reparse.push(t.elapsed());
        self.text = new_text;
        self.starts = line_starts(&self.text);
        self.cursor = a + insert.len();
        self.modified = true;
    }

    fn prev(&self, o: usize) -> usize {
        self.text[..o].grapheme_indices(true).next_back().map_or(0, |(i, _)| i)
    }

    fn next(&self, o: usize) -> usize {
        self.text[o..].graphemes(true).next().map_or(o, |g| o + g.len())
    }

    fn vertical(&mut self, delta: isize) {
        let line = self.line_of(self.cursor);
        let target = (line as isize + delta).clamp(0, self.starts.len() as isize - 1) as usize;
        let col = self.cursor - self.starts[line];
        let (ls, le) = self.line_range(target);
        let mut o = (ls + col).min(le);
        while !self.text.is_char_boundary(o) {
            o -= 1;
        }
        self.cursor = o;
    }

    pub fn key(&mut self, k: KeyEvent) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Char('q') | KeyCode::Char('c') if ctrl => self.quit = true,
            KeyCode::Char('s') if ctrl => self.save(),
            KeyCode::Char(c) if !ctrl => {
                let at = self.cursor;
                self.edit(at, at, c.encode_utf8(&mut [0; 4]));
            }
            KeyCode::Enter => {
                let at = self.cursor;
                self.edit(at, at, "\n");
            }
            KeyCode::Backspace if self.cursor > 0 => {
                let p = self.prev(self.cursor);
                self.edit(p, self.cursor, "");
            }
            KeyCode::Delete if self.cursor < self.text.len() => {
                let n = self.next(self.cursor);
                self.edit(self.cursor, n, "");
            }
            KeyCode::Left => self.cursor = self.prev(self.cursor),
            KeyCode::Right => self.cursor = self.next(self.cursor),
            KeyCode::Up => self.vertical(-1),
            KeyCode::Down => self.vertical(1),
            KeyCode::PageUp => self.vertical(-(self.size.1 as isize)),
            KeyCode::PageDown => self.vertical(self.size.1 as isize),
            KeyCode::Home => self.cursor = self.line_range(self.line_of(self.cursor)).0,
            KeyCode::End => self.cursor = self.line_range(self.line_of(self.cursor)).1,
            _ => {}
        }
    }

    pub fn event(&mut self, e: Event) {
        match e {
            Event::Key(k) => self.key(k),
            Event::Mouse(m) => match m.kind {
                MouseEventKind::Down(MouseButton::Left) => self.click(m.column, m.row),
                MouseEventKind::ScrollDown => self.top = (self.top + 3).min(self.starts.len().saturating_sub(1)),
                MouseEventKind::ScrollUp => self.top = self.top.saturating_sub(3),
                _ => {}
            },
            _ => {}
        }
    }

    pub fn click(&mut self, col: u16, row: u16) {
        let Some(hit) = self.rows.iter().find(|r| r.y == row) else { return };
        let mut x = hit.x0;
        for g in &hit.glyphs {
            if col < x + g.width.max(1) {
                if let Some((Widget::Checkbox(s), a, b)) = &g.widget {
                    let new = if *s == b'X' { "[ ]" } else { "[X]" };
                    let keep = self.cursor;
                    self.edit(*a, *b, new);
                    self.cursor = keep;
                    return;
                }
                self.cursor = g.src;
                return;
            }
            x += g.width;
        }
        self.cursor = self.line_range(hit.line).1;
    }

    fn save(&mut self) {
        match std::fs::write(&self.path, &self.text) {
            Ok(()) => {
                self.modified = false;
                self.status = format!("saved {}", self.path.display());
            }
            Err(e) => self.status = format!("save failed: {e}"),
        }
    }

    /// An image shown instead of the line: a display formula or an image link.
    fn image_key(&self, ls: usize, le: usize) -> Option<String> {
        let t = self.text[ls..le].trim();
        if (t.starts_with("\\[") && t.ends_with("\\]") || t.starts_with("$$") && t.ends_with("$$")) && t.len() > 4 {
            return Some(format!("math:{t}"));
        }
        let inner = t.strip_prefix("[[")?.strip_suffix("]]")?;
        if inner.contains("][") {
            return None;
        }
        let p = inner.strip_prefix("file:").unwrap_or(inner);
        let lower = p.to_lowercase();
        [".png", ".jpg", ".jpeg", ".gif"].iter().any(|e| lower.ends_with(e)).then(|| format!("file:{p}"))
    }

    /// Loads an image and returns its size in cells.
    fn ensure_image(&mut self, key: &str, max_cols: u16) -> Option<(u16, u16)> {
        let picker = self.picker.as_ref()?;
        if !self.images.contains_key(key) {
            let (cw, ch) = { let f = picker.font_size(); (f.width.max(1) as f32, f.height.max(1) as f32) };
            let img = if let Some(src) = key.strip_prefix("math:") {
                crate::math::image(src, ch * 0.95, [0.85, 0.9, 0.95])
            } else {
                let p = key.strip_prefix("file:").unwrap_or(key);
                let base = self.path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
                image::open(base.join(p)).ok()
            };
            let entry = img.map(|img| {
                let cols = ((img.width() as f32 / cw).ceil() as u16).clamp(1, max_cols.max(1));
                let rows = ((img.height() as f32 / ch).ceil() as u16).clamp(1, 16);
                (picker.new_resize_protocol(img), rows, cols)
            });
            self.images.insert(key.to_string(), entry);
        }
        self.images.get(key)?.as_ref().map(|(_, r, c)| (*r, *c))
    }

    fn line_height(&mut self, line: usize, width: u16) -> u16 {
        let (ls, le) = self.line_range(line);
        let on_line = self.line_of(self.cursor) == line;
        if !on_line
            && self.picker.is_some()
            && let Some(key) = self.image_key(ls, le)
            && let Some((rows, _)) = self.ensure_image(&key, width)
        {
            return rows;
        }
        let root = self.parse.syntax();
        let v = view::line_view(&root, ls, le, Some(self.cursor));
        render::wrap(render::glyphs(&v, &root, ls, on_line, &self.caps), width).len() as u16
    }

    /// Scrolls so that the cursor's line is visible. Walks up from the
    /// cursor line at most one screen, so the cost does not depend on the
    /// distance to the top line.
    fn ensure_visible(&mut self, width: u16, height: u16) {
        let cl = self.line_of(self.cursor);
        if cl < self.top {
            self.top = cl;
            return;
        }
        let (mut sum, mut l) = (0u32, cl);
        loop {
            let h = self.line_height(l, width) as u32;
            if sum + h > height as u32 && l < cl {
                self.top = self.top.max(l + 1);
                return;
            }
            sum += h;
            if l <= self.top {
                return;
            }
            l -= 1;
        }
    }

    pub fn draw(&mut self, f: &mut Frame) {
        let t = Instant::now();
        let area = f.area();
        let text_area = Rect { height: area.height.saturating_sub(1), ..area };
        let x0 = text_area.x + 1;
        let width = text_area.width.saturating_sub(2).max(1);
        self.size = (width, text_area.height);
        // Follow the cursor only when it moved, so wheel scrolling sticks.
        if self.last_cursor != Some(self.cursor) {
            self.ensure_visible(width, text_area.height);
            self.last_cursor = Some(self.cursor);
        }
        self.rows.clear();
        let root = self.parse.syntax();
        let cursor_line = self.line_of(self.cursor);
        let mut cursor_pos = None;
        let (mut y, mut line) = (text_area.y, self.top);
        while y < text_area.bottom() && line < self.starts.len() {
            let (ls, le) = self.line_range(line);
            let on_line = line == cursor_line;
            if !on_line
                && self.picker.is_some()
                && let Some(key) = self.image_key(ls, le)
                && let Some((rows, cols)) = self.ensure_image(&key, width)
            {
                let h = rows.min(text_area.bottom() - y);
                let rect = Rect::new(x0, y, cols.min(width), h);
                if let Some(Some((proto, _, _))) = self.images.get_mut(&key) {
                    f.render_stateful_widget(StatefulImage::default(), rect, proto);
                }
                for dy in 0..h {
                    self.rows.push(RowHit { y: y + dy, x0, line, glyphs: Vec::new() });
                }
                y += h;
                line += 1;
                continue;
            }
            let v = view::line_view(&root, ls, le, Some(self.cursor));
            let code = render::is_code(&v);
            let rows = render::wrap(render::glyphs(&v, &root, ls, on_line, &self.caps), width);
            let nrows = rows.len();
            for (ri, row) in rows.into_iter().enumerate() {
                if y >= text_area.bottom() {
                    break;
                }
                let buf = f.buffer_mut();
                if code && !self.caps.no_color {
                    for x in text_area.x..text_area.right() {
                        buf[(x, y)].set_bg(render::code_bg(&self.caps));
                    }
                }
                let mut x = x0;
                for (gi, g) in row.iter().enumerate() {
                    if x + g.width > text_area.right() {
                        break;
                    }
                    if on_line && cursor_pos.is_none() && g.src >= self.cursor && g.src_end > g.src {
                        cursor_pos = Some((x, y));
                    }
                    match (&g.link, self.caps.hyperlinks) {
                        (Some(url), true) => {
                            let first = gi == 0 || row[gi - 1].link.as_ref() != Some(url);
                            let last = gi + 1 == row.len() || row[gi + 1].link.as_ref() != Some(url);
                            let mut sym = String::new();
                            if first {
                                sym.push_str(&format!("\x1b]8;;{url}\x1b\\"));
                            }
                            sym.push_str(&g.text);
                            if last {
                                sym.push_str("\x1b]8;;\x1b\\");
                            }
                            let cell = &mut buf[(x, y)];
                            cell.set_symbol(&sym).set_style(g.style);
                            cell.set_diff_option(CellDiffOption::ForcedWidth(NonZeroU16::new(g.width.max(1)).unwrap()));
                        }
                        _ => {
                            buf.set_stringn(x, y, &g.text, g.width as usize, g.style);
                        }
                    }
                    x += g.width;
                }
                if on_line && cursor_pos.is_none() && ri + 1 == nrows {
                    cursor_pos = Some((x.min(text_area.right().saturating_sub(1)), y));
                }
                self.rows.push(RowHit { y, x0, line, glyphs: row });
                y += 1;
            }
            line += 1;
        }
        // Status line.
        let name = self.path.file_name().map_or("".into(), |n| n.to_string_lossy().into_owned());
        let (cl, col) = (cursor_line + 1, self.text[self.starts[cursor_line]..self.cursor].graphemes(true).count() + 1);
        let frame_ms = self.stats.frame.last().map_or(0., |d| d.as_secs_f64() * 1000.);
        let status = format!(
            " {name}{}  {cl}:{col}  {}  {}  {frame_ms:.1} ms",
            if self.modified { " •" } else { "" },
            self.caps.graphics,
            self.status
        );
        let sy = area.bottom().saturating_sub(1);
        let st = Style::default().add_modifier(Modifier::REVERSED);
        let buf = f.buffer_mut();
        for x in area.x..area.right() {
            buf[(x, sy)].set_symbol(" ").set_style(st);
        }
        buf.set_stringn(area.x, sy, &status, area.width as usize, st);
        if let Some((x, y)) = cursor_pos {
            f.set_cursor_position((x, y));
        }
        self.stats.frame.push(t.elapsed());
    }
}
