//! Rich text lines for editors on [gpui]: the inline layout of one line
//! of styled text with widgets in it.
//!
//! gpui's `shape_text` wraps text but cannot reserve space for inline
//! boxes. Here each text piece is shaped with `shape_line`, and this crate
//! breaks the rows itself (at spaces, around widgets and between CJK
//! characters), with a hanging indent for wrapped rows. It paints glyphs,
//! backgrounds, underlines and strike-throughs, and answers what an editor
//! asks of a line: where the caret goes for an offset, which offset is
//! under the mouse, the rectangles of a selection, and where each widget
//! box is so the caller can paint it.
//!
//! A line is a list of [`Piece`]s: text with style runs, widget boxes of a
//! given size standing for some bytes of the display text (a checkbox, a
//! formula), superscripts and subscripts, spacers that grow to fill a
//! short row (tags pushed to the right edge, centered text), and blocks:
//! boxes of a given width whose own pieces wrap inside them (the cells of
//! a table row). Offsets are byte offsets into the concatenated display
//! text of the pieces.
//!
//! Zed's editor does the same for its inline fold placeholders; this crate
//! makes it reusable. It needs a gpui newer than 0.2.2, such as the
//! `gpui-unofficial` snapshot of Zed's release tags.

use gpui::{
    App, Bounds, Pixels, Point, ShapedLine, SharedString, Size, TextRun, Window, point, px, quad,
    size,
};

/// Input to the layout: a run of styled text or a widget box.
#[derive(Debug, Clone)]
pub enum Piece {
    /// Text with its style runs.
    Text {
        /// The text.
        text: String,
        /// Its runs.
        runs: Vec<TextRun>,
    },
    /// A widget standing for `len` display bytes; `ascent` is the part of
    /// its height above the text baseline.
    Widget {
        /// Display bytes it stands for.
        len: usize,
        /// Its size.
        size: Size<Pixels>,
        /// Its height above the baseline.
        ascent: Pixels,
    },
    /// Smaller text on a raised or lowered baseline: superscripts and
    /// subscripts.
    Script {
        /// The text.
        text: String,
        /// Its runs.
        runs: Vec<TextRun>,
        /// Superscript (else subscript).
        sup: bool,
    },
    /// Text in its own font size on the line's baseline (a larger or
    /// smaller span); rows holding it grow to fit.
    Sized {
        /// The text.
        text: String,
        /// Its runs.
        runs: Vec<TextRun>,
        /// Its font size.
        size: Pixels,
    },
    /// Blank space that grows to fill the row when the line is shorter
    /// than the wrap width (tags at the right edge); several spacers share
    /// the room (centering).
    Spacer {
        /// Display bytes it stands for.
        len: usize,
        /// Its width when the line is full.
        min: Pixels,
    },
    /// A box `width` wide whose pieces are laid out wrapping at that
    /// width (a cell of a table row): the row is as high as its highest
    /// block, and each block's first baseline is on the row's.
    Block {
        /// Its contents.
        pieces: Vec<Piece>,
        /// Its width, at which its contents wrap.
        width: Pixels,
    },
}

enum Item {
    Text {
        line: Box<ShapedLine>,
        runs: Vec<TextRun>,
        /// Baseline shift (negative raises).
        shift: Pixels,
        /// In its own size: the row's height follows it.
        sized: bool,
    },
    Widget {
        size: Size<Pixels>,
        ascent: Pixels,
        /// A spacer's room, not a caller's widget.
        spacer: bool,
    },
    Block {
        layout: Box<InlineLayout>,
        width: Pixels,
        /// Its first baseline, from its top.
        ascent: Pixels,
    },
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
    /// Its first display offset.
    pub start: usize,
    /// Its end.
    pub end: usize,
    /// Unwrapped x of the row start.
    x0: Pixels,
    /// Its top.
    pub y: Pixels,
    /// Its height.
    pub height: Pixels,
    /// Its baseline, from its top.
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

/// A laid out line.
pub struct InlineLayout {
    placed: Vec<Placed>,
    /// The rows.
    pub rows: Vec<Row>,
    /// The width.
    pub width: Pixels,
    /// The height.
    pub height: Pixels,
    ascent: Pixels,
    descent: Pixels,
    line_height: Pixels,
    unwrapped_width: Pixels,
    /// Justified rows (by index): the display offsets of their stretched
    /// spaces, sorted, and the room each space gains.
    justified: Vec<(Vec<usize>, Pixels)>,
    /// Whether any piece is a block.
    blocks: bool,
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF)
}

impl std::fmt::Debug for InlineLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InlineLayout")
            .field("rows", &self.rows)
            .finish_non_exhaustive()
    }
}

impl InlineLayout {
    /// Lays out `pieces`, wrapping at `wrap` if given; wrapped rows start
    /// at the x of display offset `hang_at` (after a list bullet).
    pub fn new(
        pieces: &[Piece],
        font_size: Pixels,
        line_height: Pixels,
        wrap: Option<Pixels>,
        hang_at: Option<usize>,
        window: &mut Window,
    ) -> Self {
        let ts = window.text_system().clone();
        let mut placed = Vec::with_capacity(pieces.len());
        let (mut at, mut x) = (0usize, px(0.));
        let (mut ascent, mut descent) = (px(0.), px(0.));
        let mut spacers: Vec<usize> = Vec::new();
        for p in pieces {
            match p {
                Piece::Text { text, runs } => {
                    if text.is_empty() {
                        continue;
                    }
                    let line =
                        ts.shape_line(SharedString::from(text.clone()), font_size, runs, None);
                    ascent = ascent.max(line.ascent);
                    descent = descent.max(line.descent);
                    let (len, w) = (line.len(), line.width);
                    placed.push(Placed {
                        start: at,
                        end: at + len,
                        x,
                        item: Item::Text {
                            line: Box::new(line),
                            runs: runs.clone(),
                            shift: px(0.),
                            sized: false,
                        },
                    });
                    at += len;
                    x += w;
                }
                Piece::Script { text, runs, sup } => {
                    if text.is_empty() {
                        continue;
                    }
                    let line = ts.shape_line(
                        SharedString::from(text.clone()),
                        font_size * 0.72,
                        runs,
                        None,
                    );
                    let (len, w) = (line.len(), line.width);
                    let shift = if *sup {
                        font_size * -0.38
                    } else {
                        font_size * 0.18
                    };
                    placed.push(Placed {
                        start: at,
                        end: at + len,
                        x,
                        item: Item::Text {
                            line: Box::new(line),
                            runs: runs.clone(),
                            shift,
                            sized: false,
                        },
                    });
                    at += len;
                    x += w;
                }
                Piece::Sized {
                    text,
                    runs,
                    size: fs,
                } => {
                    if text.is_empty() {
                        continue;
                    }
                    let line = ts.shape_line(SharedString::from(text.clone()), *fs, runs, None);
                    let (len, w) = (line.len(), line.width);
                    placed.push(Placed {
                        start: at,
                        end: at + len,
                        x,
                        item: Item::Text {
                            line: Box::new(line),
                            runs: runs.clone(),
                            shift: px(0.),
                            sized: true,
                        },
                    });
                    at += len;
                    x += w;
                }
                &Piece::Widget { len, size, ascent } => {
                    placed.push(Placed {
                        start: at,
                        end: at + len,
                        x,
                        item: Item::Widget {
                            size,
                            ascent,
                            spacer: false,
                        },
                    });
                    at += len;
                    x += size.width;
                }
                Piece::Block { pieces, width } => {
                    let inner = InlineLayout::new(
                        pieces,
                        font_size,
                        line_height,
                        Some(*width),
                        None,
                        window,
                    );
                    let len = inner.rows.last().map_or(0, |r| r.end);
                    let first = inner.rows.first().map_or(px(0.), |r| r.baseline);
                    placed.push(Placed {
                        start: at,
                        end: at + len,
                        x,
                        item: Item::Block {
                            layout: Box::new(inner),
                            width: *width,
                            ascent: first,
                        },
                    });
                    at += len;
                    x += *width;
                }
                &Piece::Spacer { len, min } => {
                    spacers.push(placed.len());
                    placed.push(Placed {
                        start: at,
                        end: at + len,
                        x,
                        item: Item::Widget {
                            size: size(min, px(0.)),
                            ascent: px(0.),
                            spacer: true,
                        },
                    });
                    at += len;
                    x += min;
                }
            }
        }
        // Spacers share what the row leaves.
        if let Some(w) = wrap
            && x < w
            && !spacers.is_empty()
        {
            let extra = (w - x) / spacers.len() as f32;
            for &i in &spacers {
                if let Item::Widget { size, .. } = &mut placed[i].item {
                    size.width += extra;
                }
                for p in &mut placed[i + 1..] {
                    p.x += extra;
                }
            }
            x = w;
        }
        if ascent == px(0.) {
            ascent = font_size * 0.8;
            descent = font_size * 0.25;
        }
        let len = at;
        let unwrapped_width = x;
        // The hanging indent: the x of `hang_at`, if it leaves room.
        let hang = hang_at
            .and_then(|h| {
                let p = placed.iter().find(|p| p.start <= h && h < p.end)?;
                Some(match &p.item {
                    Item::Text { line, .. } => p.x + line.x_for_index(h - p.start),
                    Item::Widget { .. } | Item::Block { .. } => p.x,
                })
            })
            .filter(|h| wrap.is_some_and(|w| *h * 2. < w))
            .unwrap_or(px(0.));

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
                    // Wrapped rows start after the hanging indent.
                    row_x0 = bx - hang;
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
                let in_row = p.start >= start && (p.start < end || (p.start == end && end == len));
                if !in_row {
                    continue;
                }
                match &p.item {
                    Item::Widget { size, ascent, .. } => {
                        above = above.max(*ascent + px(2.));
                        below = below.max(size.height - *ascent + px(2.));
                    }
                    // Text in its own size: room above and below as its
                    // font needs, with the line's spacing.
                    Item::Text {
                        line, sized: true, ..
                    } => {
                        let extra = (line.ascent + line.descent) * 0.15;
                        above = above.max(line.ascent + extra);
                        below = below.max(line.descent + extra);
                    }
                    Item::Text { .. } => {}
                    Item::Block { layout, ascent, .. } => {
                        above = above.max(*ascent);
                        below = below.max(layout.height - *ascent);
                    }
                }
            }
            rows.push(Row {
                start,
                end,
                x0,
                y,
                height: above + below,
                baseline: above,
            });
            y += above + below;
        }
        let blocks = placed.iter().any(|p| matches!(p.item, Item::Block { .. }));
        InlineLayout {
            placed,
            rows,
            width: wrap.unwrap_or(unwrapped_width),
            height: y,
            ascent,
            descent,
            line_height,
            unwrapped_width,
            justified: Vec::new(),
            blocks,
        }
    }

    /// The block holding display offset `i` (its ends included): it,
    /// its row, its layout and its first baseline.
    fn block_at(&self, i: usize) -> Option<(&Placed, usize, &InlineLayout, Pixels)> {
        if !self.blocks {
            return None;
        }
        self.placed.iter().find_map(|p| match &p.item {
            Item::Block { layout, ascent, .. } if p.start <= i && i <= p.end => {
                Some((p, self.row_index(p.start), &**layout, *ascent))
            }
            _ => None,
        })
    }

    /// The top left of block `pl` in row `k`, relative to the line origin.
    fn block_origin(&self, pl: &Placed, k: usize, ascent: Pixels) -> Point<Pixels> {
        let r = &self.rows[k];
        point(
            pl.x - r.x0 + self.stretch(k, pl.start),
            r.y + r.baseline - ascent,
        )
    }

    /// Justifies the wrapped rows: every row but the last one stretches
    /// its spaces (not the ones at its ends) to reach the wrap width.
    pub fn justify(&mut self) {
        if self.rows.len() < 2 {
            return;
        }
        let atoms = atoms(&self.placed);
        let mut justified = vec![(Vec::new(), px(0.)); self.rows.len()];
        for (k, r) in self.rows.iter().enumerate().take(self.rows.len() - 1) {
            let row: Vec<&Atom> = atoms
                .iter()
                .filter(|a| a.start >= r.start && a.end <= r.end)
                .collect();
            // The words' ends: spaces at either end do not stretch.
            let Some(first) = row.iter().position(|a| !a.space) else {
                continue;
            };
            let Some(last) = row.iter().rposition(|a| !a.space) else {
                continue;
            };
            let spaces: Vec<usize> = row[first..=last]
                .iter()
                .filter(|a| a.space)
                .map(|a| a.start)
                .collect();
            if spaces.is_empty() {
                continue;
            }
            let used = row[last].x1 - r.x0;
            let room = self.width - used;
            if room <= px(0.) {
                continue;
            }
            justified[k] = (spaces.clone(), room / spaces.len() as f32);
        }
        self.justified = justified;
    }

    /// The room justification adds before display offset `i` in row `k`.
    fn stretch(&self, k: usize, i: usize) -> Pixels {
        match self.justified.get(k) {
            Some((spaces, extra)) if !spaces.is_empty() => {
                *extra * spaces.partition_point(|s| *s < i) as f32
            }
            _ => px(0.),
        }
    }

    fn row_index(&self, i: usize) -> usize {
        self.rows
            .partition_point(|r| r.start <= i)
            .saturating_sub(1)
    }

    /// The x of display offset `i` in row `k`, from the row's left edge.
    fn row_x(&self, k: usize, i: usize) -> Pixels {
        self.x_of(i) - self.rows[k].x0 + self.stretch(k, i)
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
                Item::Block { width, .. } => {
                    if i == p.start {
                        p.x
                    } else {
                        p.x + *width
                    }
                }
            },
            _ => self.unwrapped_width,
        }
    }

    /// The caret box for display offset `i`, relative to the line origin.
    pub fn caret(&self, i: usize) -> Bounds<Pixels> {
        if let Some((pl, k, layout, ascent)) = self.block_at(i) {
            let c = layout.caret(i - pl.start);
            let o = self.block_origin(pl, k, ascent);
            return Bounds::new(point(c.origin.x + o.x, c.origin.y + o.y), c.size);
        }
        let k = self.row_index(i);
        let r = &self.rows[k];
        let pad = (self.line_height - self.ascent - self.descent) / 2.;
        let top = r.y + r.baseline - self.ascent - pad;
        Bounds::new(point(self.row_x(k, i), top), size(px(2.), self.line_height))
    }

    /// The display offset closest to `p`, relative to the line origin.
    pub fn index_for_position(&self, p: Point<Pixels>) -> usize {
        let k = self
            .rows
            .iter()
            .position(|r| p.y < r.y + r.height)
            .unwrap_or(self.rows.len().saturating_sub(1));
        let Some(r) = self.rows.get(k).copied() else {
            return 0;
        };
        // A justified row: the nearest character boundary.
        if self.justified.get(k).is_some_and(|j| !j.0.is_empty()) {
            let x = p.x.max(px(0.));
            let mut best = (r.start, Pixels::MAX);
            for a in atoms(&self.placed)
                .iter()
                .filter(|a| a.start >= r.start && a.start <= r.end)
            {
                for i in [a.start, a.end.min(r.end)] {
                    let d = (self.row_x(k, i) - x).abs();
                    if d < best.1 {
                        best = (i, d);
                    }
                }
            }
            return best.0;
        }
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
                    Item::Text { line, .. } => {
                        let local = x - pl.x;
                        let mut i = line.closest_index_for_x(local);
                        // gpui gives the end for any x past the start of the
                        // last glyph; its middle decides here.
                        if i == line.len()
                            && let Some((prev, _)) = line.text[..i].char_indices().next_back()
                            && local < (line.x_for_index(prev) + line.width) / 2.
                        {
                            i = prev;
                        }
                        (pl.start + i).clamp(a, b)
                    }
                    Item::Widget { .. } => {
                        if x < (xa + xb) / 2. {
                            a
                        } else {
                            b
                        }
                    }
                    Item::Block { layout, ascent, .. } => {
                        let o = self.block_origin(pl, k, *ascent);
                        pl.start + layout.index_for_position(point(p.x - o.x, p.y - o.y))
                    }
                };
            }
            best = b;
        }
        best
    }

    /// Rectangles covering display offsets `a..b`, relative to the line
    /// origin, one per row (for selections and marks).
    pub fn range_rects(&self, a: usize, b: usize) -> Vec<Bounds<Pixels>> {
        let mut out = Vec::new();
        for (k, r) in self.rows.iter().enumerate() {
            let (s, e) = (a.max(r.start), b.min(r.end));
            if s >= e && !(a == b && a == r.start) {
                continue;
            }
            // The part in each block, as the block's own rows have it.
            let mut from = s;
            let before = out.len();
            if self.blocks {
                for pl in &self.placed {
                    let Item::Block { layout, ascent, .. } = &pl.item else {
                        continue;
                    };
                    if pl.end <= s || pl.start >= e {
                        continue;
                    }
                    if from < pl.start {
                        let (xa, xb) = (self.row_x(k, from), self.row_x(k, pl.start));
                        out.push(Bounds::new(point(xa, r.y), size(xb - xa, r.height)));
                    }
                    let o = self.block_origin(pl, k, *ascent);
                    let (bs, be) = (s.max(pl.start) - pl.start, e.min(pl.end) - pl.start);
                    for q in layout.range_rects(bs, be) {
                        out.push(Bounds::new(
                            point(q.origin.x + o.x, q.origin.y + o.y),
                            q.size,
                        ));
                    }
                    from = pl.end.min(e);
                }
                if from >= e && out.len() > before {
                    continue;
                }
            }
            let xa = self.row_x(k, from);
            let xb = self.row_x(k, e);
            out.push(Bounds::new(point(xa, r.y), size(xb - xa, r.height)));
        }
        out
    }

    /// Widget boxes relative to the line origin, with their display offsets.
    pub fn widgets(&self) -> impl Iterator<Item = (usize, Bounds<Pixels>)> + '_ {
        self.placed.iter().flat_map(|p| match &p.item {
            &Item::Widget {
                size,
                ascent,
                spacer: false,
            } => {
                let k = self.row_index(p.start);
                let r = &self.rows[k];
                vec![(
                    p.start,
                    Bounds::new(
                        point(
                            p.x - r.x0 + self.stretch(k, p.start),
                            r.y + r.baseline - ascent,
                        ),
                        size,
                    ),
                )]
            }
            // A block's own, where it is.
            Item::Block { layout, ascent, .. } => {
                let o = self.block_origin(p, self.row_index(p.start), *ascent);
                layout
                    .widgets()
                    .map(|(i, b)| {
                        (
                            p.start + i,
                            Bounds::new(point(b.origin.x + o.x, b.origin.y + o.y), b.size),
                        )
                    })
                    .collect()
            }
            _ => Vec::new(),
        })
    }

    /// Paints the text at `origin`.
    pub fn paint(&self, origin: Point<Pixels>, window: &mut Window, _cx: &mut App) {
        for (k, row) in self.rows.iter().enumerate() {
            let base_y = origin.y + row.y + row.baseline;
            for pl in &self.placed {
                if pl.end <= row.start || pl.start >= row.end {
                    continue;
                }
                if let Item::Block { layout, ascent, .. } = &pl.item {
                    let o = self.block_origin(pl, k, *ascent);
                    layout.paint(point(origin.x + o.x, origin.y + o.y), window, _cx);
                    continue;
                }
                let Item::Text {
                    line,
                    runs,
                    shift,
                    sized,
                } = &pl.item
                else {
                    continue;
                };
                let base_y = base_y + *shift;
                // Text in its own size has its own ascent and descent.
                let (ascent, descent) = if *sized {
                    (line.ascent, line.descent)
                } else {
                    (self.ascent, self.descent)
                };
                // Backgrounds and decorations, run by run.
                let mut rs = pl.start;
                for run in runs {
                    let re = rs + run.len;
                    let (a, b) = (rs.max(row.start), re.min(row.end));
                    if a < b {
                        let xa = origin.x + self.row_x(k, a);
                        let xb = origin.x + self.row_x(k, b);
                        if let Some(bg) = run.background_color {
                            let top = point(xa, base_y - ascent - px(1.));
                            let bottom = point(xb, base_y + descent + px(1.));
                            window.paint_quad(quad(
                                Bounds::from_corners(top, bottom),
                                px(3.),
                                bg,
                                px(0.),
                                bg,
                                Default::default(),
                            ));
                        }
                        if let Some(u) = &run.underline {
                            window.paint_underline(point(xa, base_y + descent * 0.618), xb - xa, u);
                        }
                        if let Some(s) = &run.strikethrough {
                            window.paint_strikethrough(
                                point(xa, base_y - ascent * 0.3),
                                xb - xa,
                                s,
                            );
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
                        let o = point(
                            origin.x + pl.x + g.position.x - row.x0 + self.stretch(k, i),
                            base_y,
                        );
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
            Item::Block { width, .. } => out.push(Atom {
                start: p.start,
                end: p.end,
                x0: p.x,
                x1: p.x + *width,
                space: false,
                widget: true,
                cjk: false,
            }),
        }
    }
    out
}
