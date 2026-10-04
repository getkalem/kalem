//! A spreadsheet's sheet as a LaTeX document, to print it or make it a PDF
//! as Excel's page layout says: the paper, its orientation and margins,
//! the print area, the rows repeated on every page, the header and the
//! footer, the page breaks, fitting one page wide; the cells with their
//! widths, fills, font colors and styles, alignment, borders and merges.
//! Compiled with LuaLaTeX, which writes any text.

use kalem_viewer::{Align, GridCell, PageSetup};

/// A sheet to print: its name, its column widths (in characters), its
/// cells by row and column in the area, the merges, and how it prints.
#[derive(Debug, Clone)]
pub struct SheetPrint {
    /// The sheet's name (`&A`).
    pub name: String,
    /// The area printed: first row, first column, last row, last column.
    pub area: [u32; 4],
    /// The columns printed (the area's, hidden ones left out), in order.
    pub columns: Vec<u32>,
    /// Each printed column's width in characters.
    pub widths: Vec<f32>,
    /// The rows not printed (hidden).
    pub hidden_rows: Vec<u32>,
    /// The cells of the area.
    pub cells: std::collections::HashMap<(u32, u32), GridCell>,
    /// The merged ranges.
    pub merged: Vec<[u32; 4]>,
    /// The page setup.
    pub setup: PageSetup,
}

/// Text made safe for LaTeX.
pub fn escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' | '}' | '$' | '&' | '#' | '_' | '%' => {
                out.push('\\');
                out.push(c);
            }
            '^' => out.push_str("\\textasciicircum{}"),
            '~' => out.push_str("\\textasciitilde{}"),
            '\n' => out.push_str("\\newline{}"),
            _ => out.push(c),
        }
    }
    out
}

/// A header or footer in Excel's codes as LaTeX: its left, center and
/// right parts (`&L`, `&C`, `&R`; the center without one), with the page
/// (`&P`), the pages (`&N`), the date (`&D`), the time (`&T`), the file
/// (`&F`) and the sheet (`&A`); fonts and sizes (`&"Arial,Bold"`, `&12`)
/// left out.
pub fn header_footer(code: &str, sheet: &str, file: &str, date: &str, time: &str) -> [String; 3] {
    let mut parts = [String::new(), String::new(), String::new()];
    let mut part = 1;
    let mut chars = code.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '&' {
            parts[part].push_str(&escape(&c.to_string()));
            continue;
        }
        match chars.next() {
            Some('L') => part = 0,
            Some('C') => part = 1,
            Some('R') => part = 2,
            Some('P') => parts[part].push_str("\\thepage{}"),
            Some('N') => parts[part].push_str("\\pageref{kalemlastpage}"),
            Some('D') => parts[part].push_str(&escape(date)),
            Some('T') => parts[part].push_str(&escape(time)),
            Some('F') => parts[part].push_str(&escape(file)),
            Some('A') => parts[part].push_str(&escape(sheet)),
            Some('&') => parts[part].push_str("\\&"),
            Some('"') => {
                for d in chars.by_ref() {
                    if d == '"' {
                        break;
                    }
                }
            }
            Some(d) if d.is_ascii_digit() => {
                while chars.peek().is_some_and(char::is_ascii_digit) {
                    chars.next();
                }
            }
            _ => {}
        }
    }
    parts
}

/// The paper's width and height in inches, by Excel's code (A4 unknown).
fn paper(code: u32) -> (f32, f32, &'static str) {
    match code {
        1 => (8.5, 11.0, "letterpaper"),
        5 => (8.5, 14.0, "legalpaper"),
        8 => (11.69, 16.54, "a3paper"),
        _ => (8.27, 11.69, "a4paper"),
    }
}

/// A column's width in inches: Excel's characters as pixels at 96 to the
/// inch (`7 × width + 5`).
fn inches(chars: f32) -> f32 {
    (chars * 7.0 + 5.0) / 96.0
}

/// The columns each printed page holds, left to right, none wider than
/// `room` inches unless it is alone.
pub fn column_pages(widths: &[f32], room: f32) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut used = 0.0;
    for (i, w) in widths.iter().enumerate() {
        if i > start && used + w > room {
            out.push(start..i);
            start = i;
            used = 0.0;
        }
        used += w;
    }
    if start < widths.len() {
        out.push(start..widths.len());
    }
    out
}

/// Where pages begin after the first, each with whether a manual break
/// starts it.
pub type Breaks = Vec<(u32, bool)>;

/// Where pages begin after the first, along rows or columns of `sizes`
/// (each one's place and size in inches) in `room` inches a page; each
/// with whether a manual break (`manual`) starts it.
pub fn page_starts(sizes: &[(u32, f32)], room: f32, manual: &[u32]) -> Breaks {
    let mut out = Vec::new();
    let mut used = 0.0;
    for (k, &(i, size)) in sizes.iter().enumerate() {
        if k > 0 && manual.contains(&i) {
            out.push((i, true));
            used = 0.0;
        } else if k > 0 && used > 0.0 && used + size > room + 0.001 {
            out.push((i, false));
            used = 0.0;
        }
        used += size;
    }
    out
}

/// A sheet's pages as Page Break Preview shows them: where pages begin
/// along the rows and the columns of the area printed, each with whether
/// a manual break starts it. Row heights are in points, column widths in
/// characters, as a grid's layout gives them.
pub fn page_breaks(
    setup: &PageSetup,
    rows: &[(u32, f32)],
    cols: &[(u32, f32)],
) -> (Breaks, Breaks) {
    let (pw, ph, _) = paper(setup.paper);
    let (pw, ph) = if setup.landscape { (ph, pw) } else { (pw, ph) };
    let [l, r, t, b] = setup.margins;
    let (room_w, room_h) = ((pw - l - r).max(1.0), (ph - t - b).max(1.0));
    let widths: Vec<(u32, f32)> = cols.iter().map(|&(c, w)| (c, inches(w))).collect();
    let total: f32 = widths.iter().map(|x| x.1).sum();
    let scale = if setup.fit_width && total > room_w {
        room_w / total
    } else {
        1.0
    };
    let widths: Vec<(u32, f32)> = widths.into_iter().map(|(c, w)| (c, w * scale)).collect();
    let heights: Vec<(u32, f32)> = rows.iter().map(|&(i, h)| (i, h / 72.0 * scale)).collect();
    (
        page_starts(&heights, room_h, &setup.row_breaks),
        page_starts(&widths, room_w, &[]),
    )
}

fn color(c: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// The document for sheets printed one after another, each as its setup
/// says (the first's paper for all), with `file` for `&F`, `date` and
/// `time` for `&D` and `&T`.
pub fn document(sheets: &[SheetPrint], file: &str, date: &str, time: &str) -> String {
    let first = sheets.first().map(|s| s.setup.clone()).unwrap_or_default();
    let (pw, ph, paper_name) = paper(first.paper);
    let mut out = String::new();
    out.push_str("\\documentclass[10pt]{article}\n");
    out.push_str(&format!(
        "\\usepackage[{paper_name}{}]{{geometry}}\n",
        if first.landscape { ",landscape" } else { "" }
    ));
    out.push_str("\\usepackage{fontspec}\n\\usepackage[table]{xcolor}\n\\usepackage{longtable,array,fancyhdr}\n\\usepackage[normalem]{ulem}\n");
    out.push_str("\\setlength{\\tabcolsep}{2pt}\n\\renewcommand{\\headrulewidth}{0pt}\n\\setlength{\\LTleft}{0pt}\n\\setlength{\\LTright}{0pt plus 1fill}\n\\setlength{\\LTpre}{0pt}\n\\setlength{\\LTpost}{0pt}\n\\setlength{\\parindent}{0pt}\n");
    out.push_str("\\renewcommand{\\familydefault}{\\sfdefault}\n\\begin{document}\n");
    for (k, s) in sheets.iter().enumerate() {
        let setup = &s.setup;
        let [l, r, t, b] = setup.margins;
        if k > 0 {
            out.push_str("\\clearpage\n");
        }
        out.push_str(&format!(
            "\\newgeometry{{left={l}in,right={r}in,top={t}in,bottom={b}in,headsep=0.15in,footskip=0.3in}}\n"
        ));
        let head = header_footer(&setup.header, &s.name, file, date, time);
        let foot = header_footer(&setup.footer, &s.name, file, date, time);
        out.push_str(&format!(
            "\\fancypagestyle{{sheet{k}}}{{\\fancyhf{{}}\\lhead{{{}}}\\chead{{{}}}\\rhead{{{}}}\\lfoot{{{}}}\\cfoot{{{}}}\\rfoot{{{}}}}}\n\\pagestyle{{sheet{k}}}\n",
            head[0], head[1], head[2], foot[0], foot[1], foot[2]
        ));
        let (pw, _) = if first.landscape { (ph, pw) } else { (pw, ph) };
        let room = (pw - l - r).max(1.0);
        let mut widths: Vec<f32> = s.widths.iter().map(|w| inches(*w)).collect();
        let total: f32 = widths.iter().sum();
        // Fit to one page wide: the columns, and the text, made smaller.
        let scale = if setup.fit_width && total > room {
            room / total
        } else {
            1.0
        };
        for w in &mut widths {
            *w *= scale;
        }
        let size = 10.0 * scale.max(0.3);
        out.push_str(&format!(
            "\\fontsize{{{size:.1}pt}}{{{:.1}pt}}\\selectfont\n",
            size * 1.2
        ));
        let pages = column_pages(&widths, room + 0.01);
        for (j, cols) in pages.iter().enumerate() {
            if j > 0 {
                out.push_str("\\clearpage\n");
            }
            table(&mut out, s, cols.clone(), &widths);
        }
    }
    // The last page, for `&N`.
    out.push_str("\\label{kalemlastpage}\n\\end{document}\n");
    out
}

/// One longtable: the area's rows, over columns `cols` of it.
fn table(out: &mut String, s: &SheetPrint, cols: std::ops::Range<usize>, widths: &[f32]) {
    let a = s.area;
    let spec: String = cols.clone().map(|_| "l").collect();
    out.push_str(&format!("\\begin{{longtable}}{{@{{}}{spec}@{{}}}}\n"));
    let row = |out: &mut String, r: u32| {
        let mut c = cols.start;
        let mut cells = Vec::new();
        let mut rules = Vec::new();
        while c < cols.end {
            let col = s.columns[c];
            // A merge from here over the printed columns it covers.
            let span = s
                .merged
                .iter()
                .find(|m| m[0] == r && m[1] == col)
                .map_or(1, |m| {
                    s.columns[c..cols.end]
                        .iter()
                        .take_while(|x| **x <= m[3])
                        .count()
                })
                .max(1);
            let covered = s.merged.iter().any(|m| {
                (m[0]..=m[2]).contains(&r)
                    && (m[1]..=m[3]).contains(&col)
                    && (m[0], m[1]) != (r, col)
            });
            let width: f32 = widths[c..c + span].iter().sum::<f32>() - 4.0 / 72.27;
            let cell = s.cells.get(&(r, col));
            let mut text = String::new();
            let mut left = String::new();
            let mut right = String::new();
            let mut align = "\\raggedright";
            let mut fill = String::new();
            if let Some(g) = cell.filter(|_| !covered) {
                let mut t = escape(&g.text);
                if g.bold {
                    t = format!("\\textbf{{{t}}}");
                }
                if g.italic {
                    t = format!("\\textit{{{t}}}");
                }
                if g.underline {
                    t = format!("\\uline{{{t}}}");
                }
                if g.strike {
                    t = format!("\\sout{{{t}}}");
                }
                if let Some(c) = g.color {
                    t = format!("\\textcolor[HTML]{{{}}}{{{t}}}", color(c));
                }
                text = t;
                align = match g.align {
                    Align::Right => "\\raggedleft",
                    Align::Center => "\\centering",
                    Align::Left => "\\raggedright",
                    _ if g.numeric => "\\raggedleft",
                    _ => "\\raggedright",
                };
                if let Some(f) = g.fill {
                    fill = format!("\\cellcolor[HTML]{{{}}}", color(f));
                }
                if g.borders[3].is_some() {
                    left.push('|');
                }
                if g.borders[1].is_some() {
                    right.push('|');
                }
                if g.borders[2].is_some() {
                    rules.push((c, c + span));
                }
            }
            cells.push(format!(
                "\\multicolumn{{{span}}}{{{left}@{{\\hspace{{2pt}}}}>{{{align}\\arraybackslash}}p{{{width:.3}in}}@{{\\hspace{{2pt}}}}{right}}}{{{fill}{text}}}"
            ));
            c += span;
        }
        out.push_str(&cells.join(" & "));
        out.push_str(" \\\\");
        for (from, to) in rules {
            out.push_str(&format!(
                "\\cline{{{}-{}}}",
                from - cols.start + 1,
                to - cols.start
            ));
        }
        out.push('\n');
    };
    // The rows repeated on every page, then the others.
    let titles = s.setup.title_rows;
    if let Some((t0, t1)) = titles {
        for r in t0..=t1 {
            row(out, r);
        }
        out.push_str("\\endhead\n");
    }
    for r in a[0]..=a[2] {
        if titles.is_some_and(|(t0, t1)| (t0..=t1).contains(&r)) || s.hidden_rows.contains(&r) {
            continue;
        }
        if r > a[0] && s.setup.row_breaks.contains(&r) {
            out.push_str("\\pagebreak\n");
        }
        row(out, r);
    }
    out.push_str("\\end{longtable}\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_and_text() {
        assert_eq!(escape("50% & $5_a"), "50\\% \\& \\$5\\_a");
        let h = header_footer(
            "&LKalem&C&A&RPage &P of &N",
            "Bütçe",
            "b.xlsx",
            "4.10.2026",
            "10:00",
        );
        assert_eq!(h[0], "Kalem");
        assert_eq!(h[1], "Bütçe");
        assert_eq!(h[2], "Page \\thepage{} of \\pageref{kalemlastpage}");
        let h = header_footer("&\"Arial,Bold\"&14Rapor &F", "S", "b.xlsx", "", "");
        assert_eq!(h[1], "Rapor b.xlsx");
    }

    #[test]
    fn columns_over_pages() {
        assert_eq!(column_pages(&[3.0, 3.0, 3.0], 7.0), vec![0..2, 2..3]);
        assert_eq!(column_pages(&[9.0, 1.0], 7.0), vec![0..1, 1..2]);
        assert_eq!(column_pages(&[], 7.0), Vec::<std::ops::Range<usize>>::new());
        // Rows: a page every two, a manual break before the third row.
        let sizes = [(0, 3.0), (1, 3.0), (2, 3.0), (3, 3.0), (4, 3.0)];
        assert_eq!(page_starts(&sizes, 7.0, &[]), vec![(2, false), (4, false)]);
        assert_eq!(page_starts(&sizes, 7.0, &[1]), vec![(1, true), (3, false)]);
    }

    #[test]
    fn a_sheet_as_latex() {
        let mut cells = std::collections::HashMap::new();
        cells.insert(
            (0, 0),
            GridCell {
                text: "Gelir & Gider".into(),
                bold: true,
                fill: Some([0x44, 0x72, 0xC4]),
                ..GridCell::default()
            },
        );
        cells.insert(
            (1, 1),
            GridCell {
                text: "1,200.00".into(),
                numeric: true,
                ..GridCell::default()
            },
        );
        let s = SheetPrint {
            name: "Bütçe".into(),
            area: [0, 0, 1, 1],
            columns: vec![0, 1],
            widths: vec![20.0, 10.0],
            hidden_rows: vec![],
            cells,
            merged: vec![],
            setup: PageSetup {
                landscape: true,
                title_rows: Some((0, 0)),
                footer: "&CPage &P".into(),
                ..PageSetup::default()
            },
        };
        let tex = document(&[s], "b.xlsx", "", "");
        if let Ok(dir) = std::env::var("KALEM_TEX_OUT") {
            std::fs::write(format!("{dir}/sheet.tex"), &tex).unwrap();
        }
        assert!(tex.contains("[a4paper,landscape]{geometry}"), "{tex}");
        assert!(
            tex.contains("\\cellcolor[HTML]{4472C4}\\textbf{Gelir \\& Gider}"),
            "{tex}"
        );
        assert!(tex.contains("\\raggedleft"), "{tex}");
        assert!(tex.contains("\\endhead"), "{tex}");
        assert!(tex.contains("\\cfoot{Page \\thepage{}}"), "{tex}");
    }
}
