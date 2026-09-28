//! Rich text lines for terminal editors on [ratatui]: a document is lines
//! of styled graphemes, each mapped back to a range of the source text.
//! This crate wraps them into rows with hanging indents, scrolls through
//! lines of any height, moves the cursor up and down at a kept column,
//! draws the cursor, a selection, marked ranges and OSC 8 links, and turns
//! mouse positions back into source offsets.
//!
//! The document side implements [`Lines`]: which lines exist and show, and
//! the glyphs of each. Glyphs that stand for no source (decorations,
//! indentation) have an empty source range. Each glyph can carry data of
//! the caller's type, such as the widget it draws.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::num::NonZeroU16;
use std::ops::Range;

use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use unicode_width::UnicodeWidthStr;

/// One grapheme on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph<D = ()> {
    /// The text drawn (a tab is `"\t"`, widened when wrapped).
    pub text: String,
    /// Its width in cells.
    pub width: u16,
    /// The style.
    pub style: Style,
    /// The start of the source it stands for.
    pub src: usize,
    /// The end of that source; equal to `src` for decorations.
    pub src_end: usize,
    /// The OSC 8 target of a link.
    pub link: Option<String>,
    /// The caller's data.
    pub data: Option<D>,
}

impl<D> Glyph<D> {
    /// A decoration at source offset `at`: text that stands for no source.
    pub fn decoration(text: &str, style: Style, at: usize) -> Glyph<D> {
        Glyph {
            text: text.to_string(),
            width: text.width() as u16,
            style,
            src: at,
            src_end: at,
            link: None,
            data: None,
        }
    }

    /// Whether the glyph stands for source text.
    pub fn is_source(&self) -> bool {
        self.src_end > self.src
    }
}

/// Wraps glyphs into rows of `width` cells, at spaces where possible;
/// continuation rows start after `hang` cells if that leaves room. Tabs
/// widen to the next multiple of 8.
pub fn wrap<D: Clone>(glyphs: Vec<Glyph<D>>, hang: u16, width: u16) -> Vec<Vec<Glyph<D>>> {
    let width = width.max(1);
    let hang = if hang * 2 < width { hang } else { 0 };
    let mut rows: Vec<Vec<Glyph<D>>> = vec![Vec::new()];
    let mut w = 0u16;
    let mut last_space: Option<usize> = None;
    for mut g in glyphs {
        if g.text == "\t" {
            g.width = 8 - (w % 8);
        }
        let row = rows.last_mut().expect("a row");
        if w + g.width > width && !row.is_empty() && g.text != " " {
            let carry: Vec<Glyph<D>> = match last_space {
                Some(i) if i + 1 < row.len() => row.split_off(i + 1),
                _ => Vec::new(),
            };
            let mut next = Vec::new();
            if hang > 0 {
                next.push(Glyph {
                    text: " ".repeat(hang as usize),
                    width: hang,
                    style: Style::default(),
                    src: g.src,
                    src_end: g.src,
                    link: None,
                    data: None,
                });
            }
            w = hang + carry.iter().map(|g| g.width).sum::<u16>();
            next.extend(carry);
            rows.push(next);
            last_space = None;
        }
        let row = rows.last_mut().expect("a row");
        if g.text == " " {
            last_space = Some(row.len());
        }
        w += g.width;
        row.push(g);
    }
    rows
}

/// Justifies wrapped rows to `width` cells: every row but the last one
/// widens the spaces between its words (not the ones at its ends, nor a
/// hanging indent), the leftmost ones first when the cells do not share
/// out evenly.
pub fn justify<D>(rows: &mut [Vec<Glyph<D>>], width: u16) {
    let n = rows.len();
    for row in rows.iter_mut().take(n.saturating_sub(1)) {
        let is_space = |g: &Glyph<D>| g.text == " " && g.is_source();
        let Some(first) = row.iter().position(|g| !is_space(g) && g.is_source()) else {
            continue;
        };
        let Some(last) = row.iter().rposition(|g| !is_space(g)) else {
            continue;
        };
        let used: u16 = row[..=last].iter().map(|g| g.width).sum();
        let spaces: Vec<usize> = (first..last).filter(|&i| is_space(&row[i])).collect();
        if spaces.is_empty() || used >= width {
            continue;
        }
        let room = width - used;
        let each = room / spaces.len() as u16;
        let more = (room % spaces.len() as u16) as usize;
        for (k, &i) in spaces.iter().enumerate() {
            let w = 1 + each + u16::from(k < more);
            row[i].width = w;
            row[i].text = " ".repeat(w as usize);
        }
    }
}

/// The row a cursor at `cursor` is on, and its column.
pub fn cursor_in<D>(rows: &[Vec<Glyph<D>>], cursor: usize) -> (usize, u16) {
    for (ri, row) in rows.iter().enumerate() {
        let mut x = 0;
        for g in row {
            if g.is_source() && g.src >= cursor {
                return (ri, x);
            }
            if g.is_source() && g.src < cursor && cursor < g.src_end && g.data.is_none() {
                return (ri, x);
            }
            x += g.width;
        }
    }
    let last = rows.len().saturating_sub(1);
    (
        last,
        rows.get(last)
            .map_or(0, |r| r.iter().map(|g| g.width).sum()),
    )
}

/// The source offset at column `x` of `row`; past its end, the end of the
/// row (`line_end` for a line's last row).
pub fn offset_at<D>(row: &[Glyph<D>], x: u16, last_row: bool, line_end: usize) -> usize {
    let mut at = 0;
    for g in row {
        if x < at + g.width.max(1) && g.is_source() {
            return g.src;
        }
        at += g.width;
    }
    if last_row {
        line_end
    } else {
        row.iter()
            .rev()
            .find(|g| g.is_source())
            .map_or(line_end, |g| g.src)
    }
}

/// A document as lines of glyphs.
pub trait Lines {
    /// The data glyphs carry.
    type Data: Clone;

    /// The line holding source offset `offset`.
    fn line_of(&self, offset: usize) -> usize;

    /// Where line `line` starts.
    fn line_start(&self, line: usize) -> usize;

    /// Where line `line` ends (before its line break).
    fn line_end(&self, line: usize) -> usize;

    /// The shown line after `line`.
    fn next_line(&self, line: usize) -> Option<usize>;

    /// The shown line before `line`.
    fn prev_line(&self, line: usize) -> Option<usize>;

    /// Whether line `line` shows (lines can be folded away).
    fn is_visible(&self, _line: usize) -> bool {
        true
    }

    /// The rows of line `line` for a text `width` cells wide.
    fn rows(&self, line: usize, width: u16) -> Vec<Vec<Glyph<Self::Data>>>;

    /// A background for the whole width of the line's rows.
    fn background(&self, _line: usize) -> Option<Color> {
        None
    }

    /// Columns at the start of the line's rows that its background leaves
    /// out (an indentation drawn before the text).
    fn background_start(&self, _line: usize) -> u16 {
        0
    }
}

/// Where the view starts and the column vertical motion keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Viewport {
    /// The start of the first shown line.
    pub top: usize,
    /// Rows of that line above the view (for lines taller than it).
    pub top_row: usize,
    /// The column vertical motion aims for.
    pub goal_x: Option<u16>,
}

impl Viewport {
    /// Scrolls so that the row of the cursor at `cursor` shows in a view
    /// `height` rows high.
    pub fn scroll_to<L: Lines>(&mut self, lines: &L, cursor: usize, width: u16, height: u16) {
        let cl = lines.line_of(cursor);
        let (crow, _) = cursor_in(&lines.rows(cl, width), cursor);
        let mut top_line = lines.line_of(self.top);
        if !lines.is_visible(top_line) {
            self.top = lines.line_start(cl);
            self.top_row = crow;
            top_line = cl;
        }
        if cl < top_line || (cl == top_line && crow < self.top_row) {
            self.top = lines.line_start(cl);
            self.top_row = crow;
            return;
        }
        // Rows from the top to the cursor's row.
        let mut count = 0usize;
        let mut line = top_line;
        loop {
            let n = if line == cl {
                crow + 1
            } else {
                lines.rows(line, width).len()
            };
            count += if line == top_line {
                n.saturating_sub(self.top_row)
            } else {
                n
            };
            if count > height as usize || line == cl {
                break;
            }
            match lines.next_line(line) {
                Some(n) => line = n,
                None => break,
            }
        }
        if count <= height as usize {
            return;
        }
        // Fill the view upwards from the cursor's row.
        let mut sum = crow + 1;
        let mut top = cl;
        while let Some(p) = lines.prev_line(top) {
            let n = lines.rows(p, width).len();
            if sum + n > height as usize {
                break;
            }
            sum += n;
            top = p;
        }
        self.top = lines.line_start(top);
        self.top_row = if top == cl {
            (crow + 1).saturating_sub(height as usize)
        } else {
            0
        };
    }

    /// Scrolls by `delta` rows.
    pub fn scroll<L: Lines>(&mut self, lines: &L, delta: isize, width: u16) {
        let mut line = lines.line_of(self.top);
        let mut row = self.top_row;
        if delta > 0 {
            for _ in 0..delta {
                if row + 1 < lines.rows(line, width).len() {
                    row += 1;
                } else if let Some(n) = lines.next_line(line) {
                    line = n;
                    row = 0;
                }
            }
        } else {
            for _ in 0..-delta {
                if row > 0 {
                    row -= 1;
                } else if let Some(p) = lines.prev_line(line) {
                    line = p;
                    row = lines.rows(p, width).len() - 1;
                }
            }
        }
        self.top = lines.line_start(line);
        self.top_row = row;
    }

    /// The source offset `delta` rows below (above, if negative) the
    /// cursor at `cursor`, at the kept column.
    pub fn vertical<L: Lines>(
        &mut self,
        lines: &L,
        cursor: usize,
        delta: isize,
        width: u16,
    ) -> usize {
        let mut line = lines.line_of(cursor);
        let mut rows = lines.rows(line, width);
        let (mut row, x) = cursor_in(&rows, cursor);
        let goal = *self.goal_x.get_or_insert(x);
        for _ in 0..delta.unsigned_abs() {
            if delta > 0 {
                if row + 1 < rows.len() {
                    row += 1;
                } else if let Some(n) = lines.next_line(line) {
                    line = n;
                    rows = lines.rows(n, width);
                    row = 0;
                } else {
                    return lines.line_end(line);
                }
            } else if row > 0 {
                row -= 1;
            } else if let Some(p) = lines.prev_line(line) {
                line = p;
                rows = lines.rows(p, width);
                row = rows.len() - 1;
            } else {
                return lines.line_start(line);
            }
        }
        offset_at(
            &rows[row],
            goal,
            row + 1 == rows.len(),
            lines.line_end(line),
        )
    }
}

/// How to draw.
#[derive(Debug, Clone, Default)]
pub struct Options<'a> {
    /// The cursor.
    pub cursor: usize,
    /// The selected source range (empty for none).
    pub selection: Range<usize>,
    /// Ranges to mark, sorted (search matches).
    pub marks: &'a [Range<usize>],
    /// The style added to marked text.
    pub mark_style: Style,
    /// Columns left free before the text.
    pub margin: u16,
    /// Columns scrolled out at the left, for lines that do not wrap.
    pub hscroll: u16,
}

/// A drawn row, for mouse hits.
#[derive(Debug, Clone)]
pub struct RowHit<D> {
    /// The screen row.
    pub y: u16,
    /// The screen column of the first glyph.
    pub x0: u16,
    /// Columns of the row scrolled out at the left.
    pub scrolled: u16,
    /// The line.
    pub line: usize,
    /// Whether it is the line's last row.
    pub last: bool,
    /// The glyphs.
    pub glyphs: Vec<Glyph<D>>,
}

/// Where a line was drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawnLine {
    /// The line.
    pub line: usize,
    /// The screen row of its first drawn row.
    pub y: u16,
    /// Its rows above the view.
    pub skipped: usize,
    /// Its rows in total.
    pub rows: usize,
}

/// The result of drawing.
#[derive(Debug, Clone)]
pub struct Drawn<D> {
    /// The cursor's cell, if it shows.
    pub cursor: Option<(u16, u16)>,
    /// The rows drawn.
    pub rows: Vec<RowHit<D>>,
    /// The lines drawn.
    pub lines: Vec<DrawnLine>,
}

impl<D: Clone> Drawn<D> {
    /// The source offset under screen cell (`col`, `row`), and the data of
    /// the glyph there. Past a row's end, the end of the row.
    pub fn hit<L: Lines<Data = D>>(
        &self,
        lines: &L,
        col: u16,
        row: u16,
    ) -> Option<(usize, Option<D>)> {
        let hit = self.rows.iter().find(|r| r.y == row)?;
        let mut x = i32::from(hit.x0) - i32::from(hit.scrolled);
        for g in &hit.glyphs {
            if i32::from(col) < x + i32::from(g.width.max(1)) {
                return Some((g.src, g.data.clone()));
            }
            x += i32::from(g.width);
        }
        Some((
            offset_at(&hit.glyphs, u16::MAX, hit.last, lines.line_end(hit.line)),
            None,
        ))
    }
}

/// An OSC 8 link id: the same for the cells of one link on a line.
fn link_id(url: &str, line: usize) -> u64 {
    let mut h = DefaultHasher::new();
    (url, line).hash(&mut h);
    h.finish() % 1_000_000_007
}

/// Draws `lines` from the viewport's top into `area` of `buf`.
pub fn draw<L: Lines>(
    lines: &L,
    viewport: &Viewport,
    buf: &mut Buffer,
    area: Rect,
    opt: &Options<'_>,
) -> Drawn<L::Data> {
    let mut drawn = Drawn {
        cursor: None,
        rows: Vec::new(),
        lines: Vec::new(),
    };
    let x0 = area.x + opt.margin;
    let width = area.width.saturating_sub(2 * opt.margin).max(1);
    let (sa, sb) = (opt.selection.start, opt.selection.end);
    let mut line = lines.line_of(viewport.top);
    if !lines.is_visible(line) {
        line = lines.next_line(line).unwrap_or(line);
    }
    let mut skip = viewport.top_row;
    let mut y = area.y;
    while y < area.bottom() {
        let rows = lines.rows(line, width);
        let background = lines.background(line);
        let range = lines.line_start(line)..lines.line_end(line);
        let on_line = range.start <= opt.cursor && opt.cursor <= range.end;
        let (crow, cx) = if on_line {
            cursor_in(&rows, opt.cursor)
        } else {
            (usize::MAX, 0)
        };
        drawn.lines.push(DrawnLine {
            line,
            y,
            skipped: skip.min(rows.len()),
            rows: rows.len(),
        });
        let n = rows.len();
        for (ri, row) in rows.into_iter().enumerate() {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            if y >= area.bottom() {
                break;
            }
            if let Some(bg) = background {
                let from = if lines.background_start(line) > 0 {
                    x0 + lines.background_start(line)
                } else {
                    area.x
                };
                for x in from..area.right() {
                    buf[(x, y)].set_bg(bg);
                }
            }
            let mut x = x0;
            let mut col = 0u16;
            for g in &row {
                // Scrolled out at the left.
                if col < opt.hscroll {
                    col = col.saturating_add(g.width);
                    continue;
                }
                if x + g.width > area.right() {
                    break;
                }
                col = col.saturating_add(g.width);
                let mut style = g.style;
                if g.is_source() && !opt.marks.is_empty() {
                    let i = opt.marks.partition_point(|m| m.end <= g.src);
                    if opt.marks.get(i).is_some_and(|m| m.start < g.src_end) {
                        style = style.patch(opt.mark_style);
                    }
                }
                if g.is_source() && g.src < sb && g.src_end > sa && sa != sb {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                let text = if g.text == "\t" {
                    " ".repeat(g.width as usize)
                } else {
                    g.text.clone()
                };
                match &g.link {
                    Some(url) => {
                        // Each cell opens and closes the link, so a cell
                        // redrawn alone keeps it; the id joins the cells.
                        let id = link_id(url, line);
                        let sym = format!("\x1b]8;id={id};{url}\x1b\\{text}\x1b]8;;\x1b\\");
                        let cell = &mut buf[(x, y)];
                        cell.set_symbol(&sym).set_style(style);
                        if let Some(w) = NonZeroU16::new(g.width) {
                            cell.set_diff_option(CellDiffOption::ForcedWidth(w));
                        }
                    }
                    None => {
                        buf.set_stringn(x, y, &text, g.width as usize, style);
                    }
                }
                x += g.width;
            }
            if ri == crow {
                let cx = cx.saturating_sub(opt.hscroll);
                drawn.cursor = Some(((x0 + cx).min(area.right().saturating_sub(1)), y));
            }
            drawn.rows.push(RowHit {
                y,
                x0,
                scrolled: opt.hscroll,
                line,
                last: ri + 1 == n,
                glyphs: row,
            });
            y += 1;
        }
        match lines.next_line(line) {
            Some(n) => line = n,
            None => break,
        }
    }
    drawn
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;

    #[test]
    fn justified_rows() {
        let text = "aa bb cc dd ee ff";
        let glyphs: Vec<Glyph> = text
            .char_indices()
            .map(|(i, c)| Glyph {
                text: c.to_string(),
                width: 1,
                style: Style::default(),
                src: i,
                src_end: i + 1,
                link: None,
                data: None,
            })
            .collect();
        let mut rows = wrap(glyphs, 0, 10);
        justify(&mut rows, 10);
        let shown: Vec<String> = rows
            .iter()
            .map(|r| r.iter().map(|g| g.text.as_str()).collect())
            .collect();
        // `aa bb cc` fills 10 cells as `aa  bb  cc`; the last row stays.
        assert_eq!(shown[0].trim_end(), "aa  bb  cc");
        assert_eq!(shown.last().unwrap(), "dd ee ff");
        let widths: u16 = rows[0].iter().map(|g| g.width).sum();
        assert!(widths >= 10);
        // The cursor still finds its source offsets.
        assert_eq!(cursor_in(&rows, 3), (0, 4));
    }

    /// Plain lines of a string, one glyph per character.
    struct Plain(String);

    impl Plain {
        fn starts(&self) -> Vec<usize> {
            std::iter::once(0)
                .chain(self.0.match_indices('\n').map(|(i, _)| i + 1))
                .collect()
        }
    }

    impl Lines for Plain {
        type Data = ();
        fn line_of(&self, offset: usize) -> usize {
            self.starts().partition_point(|s| *s <= offset) - 1
        }
        fn line_start(&self, line: usize) -> usize {
            self.starts()[line]
        }
        fn line_end(&self, line: usize) -> usize {
            self.starts().get(line + 1).map_or(self.0.len(), |s| s - 1)
        }
        fn next_line(&self, line: usize) -> Option<usize> {
            (line + 1 < self.starts().len()).then_some(line + 1)
        }
        fn prev_line(&self, line: usize) -> Option<usize> {
            line.checked_sub(1)
        }
        fn rows(&self, line: usize, width: u16) -> Vec<Vec<Glyph>> {
            let s = self.line_start(line);
            let glyphs = self.0[s..self.line_end(line)]
                .char_indices()
                .map(|(i, c)| Glyph {
                    text: c.to_string(),
                    width: 1,
                    style: Style::default(),
                    src: s + i,
                    src_end: s + i + c.len_utf8(),
                    link: None,
                    data: None,
                })
                .collect();
            wrap(glyphs, 0, width)
        }
    }

    #[test]
    fn wrapping_scrolling_and_motion() {
        let doc = Plain("aaaa bbbb cccc\nd\ne\nf\n".into());
        let rows = doc.rows(0, 6);
        let text: Vec<String> = rows
            .iter()
            .map(|r| r.iter().map(|g| g.text.as_str()).collect())
            .collect();
        assert_eq!(text, ["aaaa ", "bbbb ", "cccc"]);
        let mut v = Viewport::default();
        // Down from `aa|aa` keeps the column through the wrapped rows.
        assert_eq!(v.vertical(&doc, 2, 1, 6), 7);
        assert_eq!(v.vertical(&doc, 7, 1, 6), 12);
        assert_eq!(v.vertical(&doc, 12, 1, 6), 16);
        // A view three rows high follows the cursor.
        v.scroll_to(&doc, 19, 6, 3);
        assert_eq!((v.top, v.top_row), (15, 0));
        // Two rows up: the last, then the middle row of the first line.
        v.scroll(&doc, -2, 6);
        assert_eq!((v.top, v.top_row), (0, 1));
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 3));
        // One marked range.
        #[allow(clippy::single_range_in_vec_init)]
        let marks = [5..9];
        let opt = Options {
            hscroll: 0,
            cursor: 15,
            selection: 0..2,
            marks: &marks,
            mark_style: Style::default().add_modifier(Modifier::UNDERLINED),
            margin: 1,
        };
        // Rows `bbbb `, `cccc`, `d`: the cursor on `d`.
        let d = draw(&doc, &v, &mut buf, Rect::new(0, 0, 8, 3), &opt);
        assert_eq!(d.cursor, Some((1, 2)));
        assert_eq!(buf[(1, 0)].symbol(), "b");
        assert!(buf[(1, 0)].modifier.contains(Modifier::UNDERLINED));
        assert_eq!(d.hit(&doc, 1, 2), Some((15, None)));
        // Past the end of a wrapped row: its last character; of a line's
        // last row: the line's end.
        assert_eq!(d.hit(&doc, 7, 0), Some((9, None)));
        assert_eq!(d.hit(&doc, 7, 1), Some((14, None)));
    }
}
