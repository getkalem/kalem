//! SyncTeX (T2.7h.24): where a source line is typeset in the PDF, and
//! which source line a point of the PDF comes from, read from the
//! `.synctex.gz` file `pdflatex -synctex=1` writes beside the PDF.
//!
//! The file lists the input files by number, then each page's boxes and
//! points with the file and line they come from and their position in
//! scaled points from the page's top left corner (the 1 inch margin
//! included). Positions are given here in PDF points (big points), as a
//! viewer draws the page.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// One record of the file: a box (`[` `(` and the void `v` `h`) or a
/// point (`x` `k` `g` `$`), in PDF points.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub page: usize,
    /// `(` or `h` (horizontal boxes, lines of text), `[` or `v`
    /// (vertical boxes), or the point kinds.
    pub kind: char,
    /// The input file's number.
    pub tag: u32,
    pub line: usize,
    /// The left edge.
    pub h: f64,
    /// The baseline.
    pub v: f64,
    pub width: f64,
    pub height: f64,
    pub depth: f64,
    /// The innermost box holding it, by index (the page's boxes only).
    pub parent: Option<usize>,
}

impl Record {
    fn is_line(&self) -> bool {
        matches!(self.kind, '(' | 'h')
    }

    /// The vertical distance from `y` to the box (0 inside it).
    fn dy(&self, y: f64) -> f64 {
        let (top, bottom) = (self.v - self.height, self.v + self.depth);
        if y < top {
            top - y
        } else if y > bottom {
            y - bottom
        } else {
            0.0
        }
    }

    /// The horizontal distance from `x` to the box (0 inside it).
    fn dx(&self, x: f64) -> f64 {
        if x < self.h {
            self.h - x
        } else if x > self.h + self.width {
            x - self.h - self.width
        } else {
            0.0
        }
    }
}

/// Where a source line is typeset: a page and the rectangle of its lines
/// there, in PDF points from the page's top left corner.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// Counted from 1, as SyncTeX counts.
    pub page: usize,
    pub x: f64,
    /// The top of the rectangle.
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A SyncTeX file read.
#[derive(Debug, Clone, Default)]
pub struct Synctex {
    /// The input files by number, as written (absolute, or relative to
    /// the PDF's folder).
    pub inputs: HashMap<u32, PathBuf>,
    /// In page order, as the file has them.
    pub records: Vec<Record>,
    /// The records of each input file, by index, in line order.
    by_tag: HashMap<u32, Vec<usize>>,
}

/// Scaled points to PDF points.
fn bp(sp: i64, unit: f64) -> f64 {
    sp as f64 * unit / 65536.0 * 72.0 / 72.27
}

impl Synctex {
    /// The SyncTeX file of `pdf` (`name.synctex.gz`, or `name.synctex`).
    pub fn for_pdf(pdf: &Path) -> Option<PathBuf> {
        ["synctex.gz", "synctex"]
            .iter()
            .map(|ext| pdf.with_extension(ext))
            .find(|p| p.is_file())
    }

    /// The SyncTeX file of `pdf`, read once for each version of it: the
    /// last one read is kept until the build writes it again.
    pub fn cached(pdf: &Path) -> Option<std::sync::Arc<Synctex>> {
        type Kept = Option<(PathBuf, std::time::SystemTime, std::sync::Arc<Synctex>)>;
        static LAST: std::sync::Mutex<Kept> = std::sync::Mutex::new(None);
        let file = Synctex::for_pdf(pdf)?;
        let modified = std::fs::metadata(&file).and_then(|m| m.modified()).ok()?;
        let mut last = LAST.lock().ok()?;
        if let Some((f, m, st)) = last.as_ref()
            && *f == file
            && *m == modified
        {
            return Some(st.clone());
        }
        let st = std::sync::Arc::new(Synctex::load(&file).ok()?);
        *last = Some((file, modified, st.clone()));
        Some(st)
    }

    /// Reads a SyncTeX file, compressed or not.
    pub fn load(path: &Path) -> std::io::Result<Synctex> {
        let bytes = std::fs::read(path)?;
        let text = if bytes.starts_with(&[0x1f, 0x8b]) {
            let mut s = String::new();
            flate2::read::MultiGzDecoder::new(&bytes[..]).read_to_string(&mut s)?;
            s
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        };
        Ok(Synctex::parse(&text))
    }

    /// Reads a SyncTeX file's text; records it cannot read are left out.
    pub fn parse(text: &str) -> Synctex {
        let mut st = Synctex::default();
        let mut unit = 1.0;
        let mut mag = 1.0;
        let (mut x_off, mut y_off) = (0i64, 0i64);
        let mut page = 0;
        let mut stack: Vec<usize> = Vec::new();
        let mut content = false;
        for line in text.lines() {
            if !content {
                if let Some(rest) = line.strip_prefix("Input:") {
                    st.input(rest);
                } else if let Some(v) = line.strip_prefix("Unit:") {
                    unit = v.trim().parse().unwrap_or(1.0);
                } else if let Some(v) = line.strip_prefix("Magnification:") {
                    mag = v.trim().parse::<f64>().unwrap_or(1000.0) / 1000.0;
                } else if let Some(v) = line.strip_prefix("X Offset:") {
                    x_off = v.trim().parse().unwrap_or(0);
                } else if let Some(v) = line.strip_prefix("Y Offset:") {
                    y_off = v.trim().parse().unwrap_or(0);
                } else if line.starts_with("Content:") {
                    content = true;
                }
                continue;
            }
            let Some(kind) = line.chars().next() else {
                continue;
            };
            let rest = &line[kind.len_utf8()..];
            match kind {
                'I' => {
                    if let Some(rest) = line.strip_prefix("Input:") {
                        st.input(rest);
                    }
                }
                '{' => {
                    page = rest.trim().parse().unwrap_or(page + 1);
                    stack.clear();
                }
                '}' => stack.clear(),
                ']' | ')' => {
                    stack.pop();
                }
                '[' | '(' | 'v' | 'h' | 'x' | 'k' | 'g' | '$' => {
                    let Some(mut r) = record(rest, kind, page, unit * mag, x_off, y_off) else {
                        continue;
                    };
                    r.parent = stack.last().copied();
                    st.records.push(r);
                    if matches!(kind, '[' | '(') {
                        stack.push(st.records.len() - 1);
                    }
                }
                'P' if line.starts_with("Postamble:") => break,
                _ => {}
            }
        }
        for (i, r) in st.records.iter().enumerate() {
            st.by_tag.entry(r.tag).or_default().push(i);
        }
        // By line, page order kept within a line.
        for v in st.by_tag.values_mut() {
            v.sort_by_key(|&i| st.records[i].line);
        }
        st
    }

    /// The records of page `page`.
    fn page(&self, page: usize) -> std::ops::Range<usize> {
        let start = self.records.partition_point(|r| r.page < page);
        let end = self.records.partition_point(|r| r.page <= page);
        start..end
    }

    fn input(&mut self, rest: &str) {
        if let Some((n, path)) = rest.split_once(':')
            && let Ok(n) = n.trim().parse()
        {
            self.inputs.insert(n, PathBuf::from(path));
        }
    }

    /// The numbers of the inputs that are `file`: the same path once both
    /// are made whole (`./` dropped), else the same file on disk.
    fn tags_of(&self, file: &Path) -> Vec<u32> {
        let want = clean(file);
        let same = |p: &Path| {
            let p = clean(p);
            p == want
                || std::fs::canonicalize(&p)
                    .ok()
                    .zip(std::fs::canonicalize(&want).ok())
                    .is_some_and(|(a, b)| a == b)
        };
        let mut tags: Vec<u32> = self
            .inputs
            .iter()
            .filter(|(_, p)| same(p))
            .map(|(t, _)| *t)
            .collect();
        tags.sort_unstable();
        tags
    }

    /// Where line `line` (counted from 1) of `file` is typeset: the first
    /// page with a line of text from it, and the rectangle of those lines
    /// there. A line that typesets nothing (a blank line, a comment) goes
    /// to the next one that does.
    pub fn forward(&self, file: &Path, line: usize) -> Option<Place> {
        let lists: Vec<&Vec<usize>> = self
            .tags_of(file)
            .iter()
            .filter_map(|t| self.by_tag.get(t))
            .collect();
        let line_of = |i: usize| self.records[i].line;
        // The nearest line at or after it that has records.
        let target = lists
            .iter()
            .filter_map(|v| v.get(v.partition_point(|&i| line_of(i) < line)))
            .map(|&i| line_of(i))
            .min()?;
        let hits: Vec<&Record> = lists
            .iter()
            .flat_map(|v| {
                let from = v.partition_point(|&i| line_of(i) < target);
                let to = v.partition_point(|&i| line_of(i) <= target);
                v[from..to].iter().map(|&i| &self.records[i])
            })
            .collect();
        let page = hits.iter().map(|r| r.page).min()?;
        // The lines of text holding the line's material, or the material
        // itself.
        let boxes: Vec<&Record> = hits
            .iter()
            .filter(|r| r.page == page)
            .map(|r| match r.parent.map(|p| &self.records[p]) {
                Some(p) if !r.is_line() && p.is_line() => p,
                _ => *r,
            })
            .collect();
        let x = boxes.iter().map(|r| r.h).fold(f64::INFINITY, f64::min);
        let right = boxes
            .iter()
            .map(|r| r.h + r.width)
            .fold(f64::NEG_INFINITY, f64::max);
        let top = boxes
            .iter()
            .map(|r| r.v - r.height)
            .fold(f64::INFINITY, f64::min);
        let bottom = boxes
            .iter()
            .map(|r| r.v + r.depth)
            .fold(f64::NEG_INFINITY, f64::max);
        Some(Place {
            page,
            x,
            y: top,
            width: (right - x).max(0.0),
            height: (bottom - top).max(0.0),
        })
    }

    /// The source file and line (counted from 1) a point of page `page`
    /// (PDF points from its top left corner) comes from: the line of text
    /// nearest the point, and in it the material nearest the point.
    pub fn inverse(&self, page: usize, x: f64, y: f64) -> Option<(PathBuf, usize)> {
        let on_page = self.page(page);
        let line = self.records[on_page.clone()]
            .iter()
            .enumerate()
            .map(|(k, r)| (on_page.start + k, r))
            .filter(|(_, r)| r.is_line())
            .min_by(|(_, a), (_, b)| {
                (a.dy(y), a.dx(x), a.width * (a.height + a.depth))
                    .partial_cmp(&(b.dy(y), b.dx(x), b.width * (b.height + b.depth)))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })?;
        let (i, boxed) = line;
        // The material of that line nearest the point, as `synctex edit`
        // takes it: a boundary (`x`, often the paragraph's last line where
        // TeX broke it) only when the line holds nothing else.
        let pick = self.records[on_page]
            .iter()
            .filter(|r| r.parent == Some(i))
            .min_by(|a, b| {
                (a.kind == 'x', a.dx(x))
                    .partial_cmp(&(b.kind == 'x', b.dx(x)))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(boxed);
        let path = self.inputs.get(&pick.tag)?;
        Some((path.clone(), pick.line))
    }
}

/// A record's fields: `tag,line:h,v` and, for boxes, `:W,H,D` (a kern's
/// one width).
fn record(
    rest: &str,
    kind: char,
    page: usize,
    unit: f64,
    x_off: i64,
    y_off: i64,
) -> Option<Record> {
    let mut parts = rest.split(':');
    let (tag, line) = parts.next()?.split_once(',')?;
    let (h, v) = parts.next()?.split_once(',')?;
    let mut size = parts
        .next()
        .map(|s| {
            s.split(',')
                .map(|n| n.trim().parse::<i64>().unwrap_or(0))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    size.resize(3, 0);
    let (w, ht, d) = if kind == 'k' {
        (size[0], 0, 0)
    } else {
        (size[0], size[1], size[2])
    };
    Some(Record {
        page,
        kind,
        tag: tag.trim().parse().ok()?,
        line: line.trim().parse().ok()?,
        h: bp(h.trim().parse::<i64>().ok()? + x_off, unit),
        v: bp(v.trim().parse::<i64>().ok()? + y_off, unit),
        width: bp(w.abs(), unit),
        height: bp(ht, unit),
        depth: bp(d, unit),
        parent: None,
    })
}

/// A path without `.` components (`/tmp/./a.tex` is `/tmp/a.tex`).
fn clean(p: &Path) -> PathBuf {
    p.components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The records `pdflatex -synctex=1` wrote for a two-page document
    /// (`main.tex` with `\input{part}` on its second page), cut short.
    const SAMPLE: &str = "SyncTeX Version:1
Input:1:/d/./main.tex
Input:2:/texmf/article.cls
Output:pdf
Magnification:1000
Unit:1
X Offset:0
Y Offset:0
Content:
!330
{1
[1,8:4736286,46220574:26673152,41484288,0
[1,8:8799518,44254494:22609920,36044800,0
(1,4:8799518,8865054:22609920,455111,0
h1,3:8799518,8865054:983040,0,0
x1,3:11257121,8865054
x1,3:13243229,8865054
k1,4:31409438,8865054:18166209
)
(1,7:8799518,12467224:22609920,455111,0
x1,6:9000000,12467224
x1,7:20000000,12467224
)
]
]
}1
Input:5:/d/./part.tex
{2
[1,8:4736286,46220574:26673152,41484288,0
(1,10:8799518,8865054:22609920,455111,0
x1,10:9000000,8865054
x5,1:15000000,8865054
)
]
}2
Postamble:
Count:20
";

    #[test]
    fn positions_in_pdf_points() {
        let st = Synctex::parse(SAMPLE);
        assert_eq!(st.inputs.len(), 3);
        let first = &st.records[0];
        assert_eq!(
            (first.page, first.kind, first.tag, first.line),
            (1, '[', 1, 8)
        );
        // 8799518 sp is the 1 inch margin and the text's indent: 133.77 bp,
        // as `synctex view` gives it.
        let line = st.records.iter().find(|r| r.kind == '(').unwrap();
        assert!((line.h - 133.768356).abs() < 1e-4, "{}", line.h);
        assert!((line.width - 343.711060).abs() < 1e-4);
        assert_eq!(line.parent, Some(1));
    }

    #[test]
    fn forward_and_inverse() {
        let st = Synctex::parse(SAMPLE);
        let main = Path::new("/d/main.tex");
        // Line 7: the second line of text on page 1.
        let p = st.forward(main, 7).unwrap();
        assert_eq!(p.page, 1);
        assert!((p.y - (189.531387 - 6.918498)).abs() < 1e-2, "{p:?}");
        assert!((p.height - 6.918498).abs() < 1e-3);
        // A line with nothing typeset: the next one that is.
        assert_eq!(st.forward(main, 5).unwrap().y, p.y);
        // The included file, on page 2.
        assert_eq!(st.forward(Path::new("/d/./part.tex"), 1).unwrap().page, 2);
        assert!(st.forward(Path::new("/d/other.tex"), 1).is_none());
        // A click in the first line of text, right of its last word: line 3.
        let (file, line) = st.inverse(1, 300.0, 132.0).unwrap();
        assert_eq!((file.as_path(), line), (Path::new("/d/./main.tex"), 3));
        // In the second line, left part: line 6; right part: line 7.
        assert_eq!(st.inverse(1, 140.0, 186.0).unwrap().1, 6);
        assert_eq!(st.inverse(1, 320.0, 186.0).unwrap().1, 7);
        // Page 2, on the included text.
        let (file, line) = st.inverse(2, 240.0, 132.0).unwrap();
        assert_eq!((file.as_path(), line), (Path::new("/d/./part.tex"), 1));
    }
}
