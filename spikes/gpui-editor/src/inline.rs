//! Inline layout of one display line: shaped text pieces and fixed-size
//! widgets (checkboxes, formulas), wrapped into rows.
//!
//! gpui's `shape_text` wraps text but cannot reserve space for inline
//! boxes, so the spike shapes each text piece with `shape_line`, breaks
//! rows itself and paints glyphs, decorations and widgets directly. This is
//! the same approach Zed's editor takes for inline fold placeholders.

use gpui::{App, Bounds, Pixels, Point, ShapedLine, SharedString, Size, TextRun, Window, fill, point, px, size};

/// Input to the layout: a run of styled text or a widget box.
pub enum Piece {
    Text { text: String, runs: Vec<TextRun> },
    /// A widget standing for `len` display bytes; `ascent` is the part of
    /// its height above the text baseline.
    Widget { len: usize, size: Size<Pixels>, ascent: Pixels },
}

enum Item {
    Text { line: ShapedLine, runs: Vec<TextRun> },
    Widget { size: Size<Pixels>, ascent: Pixels },
}

struct Placed {
    start: usize,
    end: usize,
    /// Horizontal position in the unwrapped line.
    x: Pixels,
    item: Item,
}

/// One wrapped row.
#[derive(Debug, Clone, Copy)]
pub struct Row {
    pub start: usize,
    pub end: usize,
    /// Unwrapped x of the row start.
    x0: Pixels,
    pub y: Pixels,
    pub height: Pixels,
    pub baseline: Pixels,
}

struct Atom {
    start: usize,
    end: usize,
    x0: Pixels,
    x1: Pixels,
    space: bool,
    widget: bool,
    cjk: bool,
}

pub struct InlineLayout {
    placed: Vec<Placed>,
    pub rows: Vec<Row>,
    pub width: Pixels,
    pub height: Pixels,
    ascent: Pixels,
    descent: Pixels,
    line_height: Pixels,
    unwrapped_width: Pixels,
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF)
}

impl InlineLayout {
    pub fn new(pieces: &[Piece], font_size: Pixels, line_height: Pixels, wrap: Option<Pixels>, window: &mut Window) -> Self {
        let ts = window.text_system().clone();
        let mut placed = Vec::with_capacity(pieces.len());
        let (mut at, mut x) = (0usize, px(0.));
        let (mut ascent, mut descent) = (px(0.), px(0.));
        for p in pieces {
            match p {
                Piece::Text { text, runs } => {
                    if text.is_empty() {
                        continue;
                    }
                    let line = ts.shape_line(SharedString::from(text.clone()), font_size, runs, None);
                    ascent = ascent.max(line.ascent);
                    descent = descent.max(line.descent);
                    let (len, w) = (line.len(), line.width);
                    placed.push(Placed { start: at, end: at + len, x, item: Item::Text { line, runs: runs.clone() } });
                    at += len;
                    x += w;
                }
                &Piece::Widget { len, size, ascent } => {
                    placed.push(Placed { start: at, end: at + len, x, item: Item::Widget { size, ascent } });
                    at += len;
                    x += size.width;
                }
            }
        }
        if ascent == px(0.) {
            ascent = font_size * 0.8;
            descent = font_size * 0.25;
        }
        let len = at;
        let unwrapped_width = x;

        // Row breaking: greedy, at spaces, around widgets and between CJK
        // characters.
        let mut spans: Vec<(usize, usize, Pixels)> = Vec::new();
        let (mut row_start, mut row_x0) = (0usize, px(0.));
        if let Some(w) = wrap
            && unwrapped_width > w
        {
            let mut brk: Option<(usize, Pixels)> = None;
            for a in atoms(&placed) {
                if (a.widget || a.cjk) && a.start > row_start {
                    brk = Some((a.start, a.x0));
                }
                if !a.space && a.x1 - row_x0 > w && a.start > row_start {
                    let (b, bx) = match brk {
                        Some((b, bx)) if b > row_start => (b, bx),
                        _ => (a.start, a.x0),
                    };
                    spans.push((row_start, b, row_x0));
                    row_start = b;
                    row_x0 = bx;
                    brk = None;
                }
                if a.space || a.widget || a.cjk {
                    brk = Some((a.end, a.x1));
                }
            }
        }
        spans.push((row_start, len, row_x0));

        let pad = (line_height - ascent - descent) / 2.;
        let mut rows = Vec::with_capacity(spans.len());
        let mut y = px(0.);
        for (start, end, x0) in spans {
            let mut above = pad + ascent;
            let mut below = line_height - above;
            for p in &placed {
                if let Item::Widget { size, ascent } = p.item
                    && p.start >= start
                    && (p.start < end || (p.start == end && end == len))
                {
                    above = above.max(ascent + px(2.));
                    below = below.max(size.height - ascent + px(2.));
                }
            }
            rows.push(Row { start, end, x0, y, height: above + below, baseline: above });
            y += above + below;
        }
        InlineLayout {
            placed,
            rows,
            width: wrap.unwrap_or(unwrapped_width),
            height: y,
            ascent,
            descent,
            line_height,
            unwrapped_width,
        }
    }

    /// Unwrapped x of display offset `i`.
    fn x_of(&self, i: usize) -> Pixels {
        let k = self.placed.partition_point(|p| p.end <= i);
        match self.placed.get(k) {
            Some(p) if p.start <= i => match &p.item {
                Item::Text { line, .. } => p.x + line.x_for_index(i - p.start),
                Item::Widget { size, .. } => {
                    if i == p.start {
                        p.x
                    } else {
                        p.x + size.width
                    }
                }
            },
            _ => self.unwrapped_width,
        }
    }

    fn row_of(&self, i: usize) -> &Row {
        let r = self.rows.partition_point(|r| r.start <= i).saturating_sub(1);
        &self.rows[r]
    }

    /// The caret box for display offset `i`, relative to the line origin.
    pub fn caret(&self, i: usize) -> Bounds<Pixels> {
        let r = self.row_of(i);
        let pad = (self.line_height - self.ascent - self.descent) / 2.;
        let top = r.y + r.baseline - self.ascent - pad;
        Bounds::new(point(self.x_of(i) - r.x0, top), size(px(2.), self.line_height))
    }

    /// The display offset closest to `p`, relative to the line origin.
    pub fn index_for_position(&self, p: Point<Pixels>) -> usize {
        let r = self.rows.iter().find(|r| p.y < r.y + r.height).or(self.rows.last()).copied();
        let Some(r) = r else { return 0 };
        let x = r.x0 + p.x.max(px(0.));
        let mut best = r.start;
        for pl in &self.placed {
            if pl.end <= r.start || pl.start >= r.end {
                continue;
            }
            let (a, b) = (pl.start.max(r.start), pl.end.min(r.end));
            let (xa, xb) = (self.x_of(a), self.x_of(b));
            if x < xa {
                return a;
            }
            if x <= xb {
                return match &pl.item {
                    Item::Text { line, .. } => (pl.start + line.closest_index_for_x(x - pl.x)).clamp(a, b),
                    Item::Widget { .. } => {
                        if x < (xa + xb) / 2. {
                            a
                        } else {
                            b
                        }
                    }
                };
            }
            best = b;
        }
        best
    }

    /// Widget boxes relative to the line origin, with their display offsets.
    pub fn widgets(&self) -> impl Iterator<Item = (usize, Bounds<Pixels>)> + '_ {
        self.placed.iter().filter_map(|p| match p.item {
            Item::Widget { size, ascent } => {
                let r = self.row_of(p.start);
                Some((p.start, Bounds::new(point(p.x - r.x0, r.y + r.baseline - ascent), size)))
            }
            Item::Text { .. } => None,
        })
    }

    pub fn paint(&self, origin: Point<Pixels>, window: &mut Window, _cx: &mut App) {
        for row in &self.rows {
            let base_y = origin.y + row.y + row.baseline;
            for pl in &self.placed {
                if pl.end <= row.start || pl.start >= row.end {
                    continue;
                }
                let Item::Text { line, runs } = &pl.item else { continue };
                // Backgrounds and decorations, run by run.
                let mut rs = pl.start;
                for run in runs {
                    let re = rs + run.len;
                    let (a, b) = (rs.max(row.start), re.min(row.end));
                    if a < b {
                        let xa = origin.x + self.x_of(a) - row.x0;
                        let xb = origin.x + self.x_of(b) - row.x0;
                        if let Some(bg) = run.background_color {
                            let top = point(xa, base_y - self.ascent - px(1.));
                            let bottom = point(xb, base_y + self.descent + px(1.));
                            window.paint_quad(fill(Bounds::from_corners(top, bottom), bg));
                        }
                        if let Some(u) = &run.underline {
                            window.paint_underline(point(xa, base_y + self.descent * 0.618), xb - xa, u);
                        }
                        if let Some(s) = &run.strikethrough {
                            window.paint_strikethrough(point(xa, base_y - self.ascent * 0.3), xb - xa, s);
                        }
                    }
                    rs = re;
                }
                // Glyphs.
                let mut run_ix = 0;
                let mut run_end = pl.start + runs.first().map_or(0, |r| r.len);
                for sr in line.runs.iter() {
                    for g in &sr.glyphs {
                        let i = pl.start + g.index;
                        if i < row.start || i >= row.end {
                            continue;
                        }
                        while i >= run_end && run_ix + 1 < runs.len() {
                            run_ix += 1;
                            run_end += runs[run_ix].len;
                        }
                        let color = runs.get(run_ix).map_or(gpui::black(), |r| r.color);
                        let o = point(origin.x + pl.x + g.position.x - row.x0, base_y);
                        let _ = if g.is_emoji {
                            window.paint_emoji(o, sr.font_id, g.id, line.font_size)
                        } else {
                            window.paint_glyph(o, sr.font_id, g.id, line.font_size, color)
                        };
                    }
                }
            }
        }
    }
}

fn atoms(placed: &[Placed]) -> Vec<Atom> {
    let mut out = Vec::new();
    for p in placed {
        match &p.item {
            Item::Text { line, .. } => {
                let glyphs: Vec<_> = line.runs.iter().flat_map(|r| r.glyphs.iter()).collect();
                let mut k = 0;
                while k < glyphs.len() {
                    let g = glyphs[k];
                    // Merge glyphs of one cluster.
                    let mut n = k + 1;
                    while n < glyphs.len() && glyphs[n].index == g.index {
                        n += 1;
                    }
                    let next_index = glyphs.get(n).map_or(line.len(), |g| g.index);
                    let x1 = glyphs.get(n).map_or(line.width, |g| g.position.x);
                    let c = line.text[g.index..].chars().next().unwrap_or(' ');
                    out.push(Atom {
                        start: p.start + g.index,
                        end: p.start + next_index,
                        x0: p.x + g.position.x,
                        x1: p.x + x1,
                        space: c == ' ' || c == '\t',
                        widget: false,
                        cjk: is_cjk(c),
                    });
                    k = n;
                }
            }
            Item::Widget { size, .. } => out.push(Atom {
                start: p.start,
                end: p.end,
                x0: p.x,
                x1: p.x + size.width,
                space: false,
                widget: true,
                cjk: false,
            }),
        }
    }
    out
}
