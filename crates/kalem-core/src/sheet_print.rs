//! A spreadsheet's sheet as a LaTeX document, to print it or make it a PDF
//! as Excel's page layout says: each sheet's paper, orientation and
//! margins, the print area, the rows and columns repeated on every page,
//! the header and the footer (their pictures too), the row and column
//! breaks, the scale or the pages to fit, the gridlines and headings; the
//! cells with their heights, widths, fills, font colors and styles,
//! alignment, borders and merges; the pictures, shapes and charts over
//! them. Compiled with LuaLaTeX, which writes any text.

use kalem_viewer::{Align, Chart, GridCell, PageSetup};

/// A picture, shape or chart printed over the cells.
#[derive(Debug, Clone)]
pub struct PrintDrawing {
    /// The cells it covers: first row, first column, last row, last
    /// column.
    pub anchor: [u32; 4],
    /// Its size in inches at full scale, width and height.
    pub size: (f32, f32),
    /// What it is.
    pub what: PrintWhat,
}

/// What a printed drawing is.
#[derive(Debug, Clone)]
pub enum PrintWhat {
    /// A picture's bytes (PNG or JPEG).
    Picture(Vec<u8>),
    /// A shape: its preset geometry, fill, outline and text.
    Shape {
        /// `rect`, `ellipse`, `roundRect`…
        preset: String,
        /// Its fill.
        fill: Option<[u8; 3]>,
        /// Its outline.
        line: Option<[u8; 3]>,
        /// Its text.
        text: String,
    },
    /// A chart as it reads now.
    Chart(Box<Chart>),
}

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
    /// The columns repeated at the left of every page and their widths,
    /// hidden ones left out.
    pub title_columns: Vec<(u32, f32)>,
    /// The rows not printed (hidden).
    pub hidden_rows: Vec<u32>,
    /// The rows' heights in points, where not the default.
    pub heights: std::collections::HashMap<u32, f32>,
    /// The default row height in points.
    pub default_height: f32,
    /// The cells of the area (and of its title rows and columns).
    pub cells: std::collections::HashMap<(u32, u32), GridCell>,
    /// The merged ranges.
    pub merged: Vec<[u32; 4]>,
    /// The pictures, shapes and charts.
    pub drawings: Vec<PrintDrawing>,
    /// The page setup.
    pub setup: PageSetup,
}

impl SheetPrint {
    fn height(&self, r: u32) -> f32 {
        self.heights.get(&r).copied().unwrap_or(self.default_height)
    }
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
/// (`&F`), the sheet (`&A`) and each part's picture (`&G`, from
/// `pictures`); fonts and sizes (`&"Arial,Bold"`, `&12`) left out.
pub fn header_footer(
    code: &str,
    sheet: &str,
    file: &str,
    date: &str,
    time: &str,
    pictures: &[Option<String>; 3],
) -> [String; 3] {
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
            Some('G') => {
                if let Some(p) = &pictures[part] {
                    parts[part].push_str(p);
                }
            }
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
        11 => (5.83, 8.27, "a5paper"),
        _ => (8.27, 11.69, "a4paper"),
    }
}

/// A column's width in inches: Excel's characters as pixels at 96 to the
/// inch (`7 × width + 5`).
fn inches(chars: f32) -> f32 {
    (chars * 7.0 + 5.0) / 96.0
}

/// The width of the row headings' column, in inches at full scale.
const HEADING_WIDTH: f32 = 0.4;

/// The columns each printed page holds, left to right, none wider than
/// `room` inches unless it is alone; a page also begins at each index of
/// `manual`.
pub fn column_pages(widths: &[f32], room: f32, manual: &[usize]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut used = 0.0;
    for (i, w) in widths.iter().enumerate() {
        if i > start && (used + w > room || manual.contains(&i)) {
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

/// The paper's width and height in inches as the sheet is turned.
fn page_size(setup: &PageSetup) -> (f32, f32) {
    let (pw, ph, _) = paper(setup.paper);
    if setup.landscape { (ph, pw) } else { (pw, ph) }
}

/// The room inside the margins, width and height in inches.
fn room(setup: &PageSetup) -> (f32, f32) {
    let (pw, ph) = page_size(setup);
    let [l, r, t, b] = setup.margins;
    ((pw - l - r).max(1.0), (ph - t - b).max(1.0))
}

/// How much the sheet is made smaller (or larger): its scale, or what
/// fits it on the pages asked, for a print `width` by `height` inches.
pub fn scale(setup: &PageSetup, width: f32, height: f32) -> f32 {
    match setup.fit {
        Some((wide, tall)) => {
            let (rw, rh) = room(setup);
            let mut s: f32 = 1.0;
            if wide > 0 && width > 0.0 {
                s = s.min(rw * wide as f32 / width);
            }
            if tall > 0 && height > 0.0 {
                s = s.min(rh * tall as f32 / height);
            }
            s.max(0.1)
        }
        None => (setup.scale.clamp(10, 400) as f32) / 100.0,
    }
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
    let (room_w, room_h) = room(setup);
    let widths: Vec<(u32, f32)> = cols.iter().map(|&(c, w)| (c, inches(w))).collect();
    let heights: Vec<(u32, f32)> = rows.iter().map(|&(i, h)| (i, h / 72.0)).collect();
    let s = scale(
        setup,
        widths.iter().map(|x| x.1).sum(),
        heights.iter().map(|x| x.1).sum(),
    );
    let widths: Vec<(u32, f32)> = widths.into_iter().map(|(c, w)| (c, w * s)).collect();
    let heights: Vec<(u32, f32)> = heights.into_iter().map(|(i, h)| (i, h * s)).collect();
    (
        page_starts(&heights, room_h, &setup.row_breaks),
        page_starts(&widths, room_w, &setup.col_breaks),
    )
}

fn color(c: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// A picture's file extension by its first bytes; `None` for one LaTeX
/// does not take (GIF).
fn picture_ext(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG") {
        Some("png")
    } else if data.starts_with(&[0xFF, 0xD8]) {
        Some("jpg")
    } else {
        None
    }
}

/// The places of a header's or footer's pictures, left to right.
const PLACES: [[&str; 3]; 2] = [["LH", "CH", "RH"], ["LF", "CF", "RF"]];

/// The picture files the document of `sheets` names, to be written beside
/// it: each name with its bytes.
pub fn images(sheets: &[SheetPrint]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for (k, s) in sheets.iter().enumerate() {
        for (i, d) in s.drawings.iter().enumerate() {
            if let PrintWhat::Picture(data) = &d.what
                && let Some(ext) = picture_ext(data)
            {
                out.push((format!("sheet{k}-picture{i}.{ext}"), data.clone()));
            }
        }
        for p in &s.setup.pictures {
            if let Some(ext) = picture_ext(&p.data) {
                out.push((format!("sheet{k}-{}.{ext}", p.place), p.data.clone()));
            }
        }
    }
    out
}

/// The document for sheets printed one after another, each on its own
/// paper as its setup says, with `file` for `&F`, `date` and `time` for
/// `&D` and `&T`. Its pictures are the files [`images`] names.
pub fn document(sheets: &[SheetPrint], file: &str, date: &str, time: &str) -> String {
    let first = sheets.first().map(|s| s.setup.clone()).unwrap_or_default();
    let (_, _, paper_name) = paper(first.paper);
    let mut out = String::new();
    out.push_str("\\documentclass[10pt]{article}\n");
    out.push_str(&format!(
        "\\usepackage[{paper_name}{}]{{geometry}}\n",
        if first.landscape { ",landscape" } else { "" }
    ));
    out.push_str("\\usepackage{fontspec}\n\\usepackage[table]{xcolor}\n\\usepackage{longtable,array,fancyhdr,graphicx,tikz}\n\\usepackage[normalem]{ulem}\n");
    out.push_str("\\setlength{\\tabcolsep}{2pt}\n\\renewcommand{\\headrulewidth}{0pt}\n\\setlength{\\LTleft}{0pt}\n\\setlength{\\LTright}{0pt plus 1fill}\n\\setlength{\\LTpre}{0pt}\n\\setlength{\\LTpost}{0pt}\n\\setlength{\\parindent}{0pt}\n");
    out.push_str("\\renewcommand{\\familydefault}{\\sfdefault}\n\\begin{document}\n");
    let files = images(sheets);
    for (k, s) in sheets.iter().enumerate() {
        let setup = &s.setup;
        let [l, _, t, _] = setup.margins;
        if k > 0 {
            out.push_str("\\clearpage\n");
        }
        // The sheet's own paper, turned as it says.
        let (pw, ph) = page_size(setup);
        let (room_w, room_h) = room(setup);
        out.push_str(&format!(
            "\\paperwidth={pw}in \\paperheight={ph}in \\pagewidth=\\paperwidth \\pageheight=\\paperheight\n"
        ));
        // Room above for the header's pictures.
        let pic_h = setup
            .pictures
            .iter()
            .filter(|p| p.place.ends_with('H'))
            .map(|p| p.size.1)
            .fold(12.0f32, f32::max);
        out.push_str(&format!(
            "\\newgeometry{{left={l}in,top={t}in,textwidth={room_w}in,textheight={room_h}in,headheight={pic_h}pt,headsep=0.15in,footskip=0.3in}}\n\\setlength{{\\headwidth}}{{\\textwidth}}\n"
        ));
        let pictures = |row: usize| -> [Option<String>; 3] {
            PLACES[row].map(|place| {
                let pic = setup.pictures.iter().find(|p| p.place == place)?;
                let name = files
                    .iter()
                    .find(|(n, _)| n.starts_with(&format!("sheet{k}-{place}.")))?
                    .0
                    .clone();
                Some(format!(
                    "\\includegraphics[width={:.1}pt,height={:.1}pt]{{{name}}}",
                    pic.size.0, pic.size.1
                ))
            })
        };
        let head = header_footer(&setup.header, &s.name, file, date, time, &pictures(0));
        let foot = header_footer(&setup.footer, &s.name, file, date, time, &pictures(1));
        out.push_str(&format!(
            "\\fancypagestyle{{sheet{k}}}{{\\fancyhf{{}}\\lhead{{{}}}\\chead{{{}}}\\rhead{{{}}}\\lfoot{{{}}}\\cfoot{{{}}}\\rfoot{{{}}}}}\n\\pagestyle{{sheet{k}}}\n",
            head[0], head[1], head[2], foot[0], foot[1], foot[2]
        ));
        let widths: Vec<f32> = s.widths.iter().map(|w| inches(*w)).collect();
        let titles: f32 = s.title_columns.iter().map(|(_, w)| inches(*w)).sum();
        let heading = if setup.headings { HEADING_WIDTH } else { 0.0 };
        let height: f32 = (s.area[0]..=s.area[2])
            .filter(|r| !s.hidden_rows.contains(r))
            .map(|r| s.height(r) / 72.0)
            .sum();
        let sc = scale(setup, widths.iter().sum::<f32>() + titles + heading, height);
        let widths: Vec<f32> = widths.iter().map(|w| w * sc).collect();
        let size = 10.0 * sc.max(0.3);
        out.push_str(&format!(
            "\\fontsize{{{size:.1}pt}}{{{:.1}pt}}\\selectfont\n",
            size * 1.2
        ));
        let manual: Vec<usize> = setup
            .col_breaks
            .iter()
            .filter_map(|c| s.columns.iter().position(|x| x == c))
            .collect();
        let pages = column_pages(&widths, room_w - (titles + heading) * sc + 0.01, &manual);
        for (j, cols) in pages.iter().enumerate() {
            if j > 0 {
                out.push_str("\\clearpage\n");
            }
            table(&mut out, s, k, cols.clone(), &widths, sc);
        }
    }
    // The last page, for `&N`.
    out.push_str("\\label{kalemlastpage}\n\\end{document}\n");
    out
}

/// A column's letters (`0` A).
fn letters(mut c: u32) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (c % 26) as u8);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

/// A drawing as LaTeX `w` by `h` inches.
fn drawing_tex(d: &PrintDrawing, file: Option<&str>, w: f32, h: f32) -> Option<String> {
    let rgb = |c: [u8; 3]| format!("{{rgb,255:red,{};green,{};blue,{}}}", c[0], c[1], c[2]);
    Some(match &d.what {
        PrintWhat::Picture(_) => {
            format!(
                "\\includegraphics[width={w:.3}in,height={h:.3}in]{{{}}}",
                file?
            )
        }
        PrintWhat::Shape {
            preset,
            fill,
            line,
            text,
        } => {
            let mut opts = Vec::new();
            if let Some(c) = fill {
                opts.push(format!("fill={}", rgb(*c)));
            }
            if let Some(c) = line {
                opts.push(format!("draw={}", rgb(*c)));
            }
            if preset == "roundRect" {
                opts.push("rounded corners=0.08in".into());
            }
            let body = match preset.as_str() {
                "ellipse" => format!(
                    "({:.3},{:.3}) ellipse[x radius={:.3},y radius={:.3}]",
                    w / 2.0,
                    h / 2.0,
                    w / 2.0,
                    h / 2.0
                ),
                _ => format!("(0,0) rectangle ({w:.3},{h:.3})"),
            };
            format!(
                "\\begin{{tikzpicture}}[x=1in,y=1in]\\path[{}] {body};\\node[text width={:.3}in,align=center] at ({:.3},{:.3}) {{{}}};\\end{{tikzpicture}}",
                opts.join(","),
                (w - 0.1).max(0.1),
                w / 2.0,
                h / 2.0,
                escape(text)
            )
        }
        PrintWhat::Chart(c) => crate::sheet_chart::tikz(c, w, h),
    })
}

/// One longtable: the area's rows, over columns `cols` of it (the title
/// columns before them, the row headings first), at scale `sc`.
fn table(
    out: &mut String,
    s: &SheetPrint,
    k: usize,
    cols: std::ops::Range<usize>,
    widths: &[f32],
    sc: f32,
) {
    let a = s.area;
    let setup = &s.setup;
    // The columns of this page: the title ones not in the area's part.
    let mut page: Vec<(u32, f32)> = s
        .title_columns
        .iter()
        .filter(|(c, _)| !s.columns[cols.clone()].contains(c))
        .map(|(c, w)| (*c, inches(*w) * sc))
        .collect();
    page.extend(cols.clone().map(|i| (s.columns[i], widths[i])));
    let heading_w = HEADING_WIDTH * sc;
    let n = page.len() + usize::from(setup.headings);
    let spec: String = (0..n).map(|_| "l").collect();
    out.push_str(&format!("\\begin{{longtable}}{{@{{}}{spec}@{{}}}}\n"));
    let grid = "!{\\color[gray]{0.75}\\vrule}";
    let black = "!{\\color{black}\\vrule}";
    let gray_rule = "\\arrayrulecolor[gray]{0.75}\\hline\\arrayrulecolor{black}";
    // A strut giving a row its height.
    let strut = |r: u32| {
        let h = s.height(r) * sc;
        format!("\\rule[-{:.2}bp]{{0pt}}{{{h:.2}bp}}", h * 0.3)
    };
    let files = images(std::slice::from_ref(s));
    // Each drawing in the last printed row it covers, standing up from
    // that row's bottom, so the rows before it are under it; on the page
    // of its first column (a repeated title column is not its own).
    let printed_rows: Vec<u32> = (a[0]..=a[2])
        .filter(|r| !s.hidden_rows.contains(r))
        .collect();
    let own = page.len() - cols.len();
    let mut overlays: std::collections::HashMap<(u32, usize), String> = Default::default();
    for (i, d) in s.drawings.iter().enumerate() {
        let [r0, c0, r1, c1] = d.anchor;
        // Not past a page break after its first row.
        let first = printed_rows.iter().copied().find(|r| (r0..=r1).contains(r));
        let end = setup
            .row_breaks
            .iter()
            .copied()
            .filter(|b| first.is_some_and(|f| *b > f))
            .min()
            .map_or(r1, |b| r1.min(b - 1));
        let Some(&last) = printed_rows.iter().rev().find(|r| (r0..=end).contains(r)) else {
            continue;
        };
        let Some(at) = (own..page.len()).find(|&x| (c0..=c1).contains(&page[x].0)) else {
            continue;
        };
        // Its first column printed on an earlier page.
        if page[at].0 != c0 && at > own {
            continue;
        }
        if page[at].0 != c0 && s.columns.iter().any(|c| (c0..page[at].0).contains(c)) {
            continue;
        }
        let file = files
            .iter()
            .find(|(n, _)| n.starts_with(&format!("sheet0-picture{i}.")))
            .map(|(n, _)| n.replace("sheet0-", &format!("sheet{k}-")));
        let (w, h) = (d.size.0 * sc, d.size.1 * sc);
        let Some(tex) = drawing_tex(d, file.as_deref(), w, h) else {
            continue;
        };
        // The rows it covers below the last one printed.
        let below: f32 = (last + 1..=r1)
            .filter(|r| !s.hidden_rows.contains(r))
            .map(|r| s.height(r))
            .sum();
        let raise = -(s.height(last) * 0.3 + below) * sc;
        overlays.insert(
            (last, at),
            format!(
                "\\makebox[0pt][l]{{\\hspace*{{-2pt}}\\raisebox{{{raise:.2}bp}}[0pt][0pt]{{{tex}}}}}"
            ),
        );
    }
    let row = |out: &mut String, r: u32| {
        let mut cells = Vec::new();
        let mut rules = Vec::new();
        let first_rule = if setup.gridlines { grid } else { "" };
        if setup.headings {
            cells.push(format!(
                "\\multicolumn{{1}}{{{first_rule}@{{\\hspace{{2pt}}}}>{{\\centering\\arraybackslash}}p{{{:.3}in}}@{{\\hspace{{2pt}}}}{first_rule}}}{{\\cellcolor[HTML]{{F2F2F2}}{}{}}}",
                heading_w - 4.0 / 72.27,
                strut(r),
                r + 1
            ));
        }
        let mut c = 0;
        while c < page.len() {
            let col = page[c].0;
            // A merge from here over the printed columns it covers.
            let span = s
                .merged
                .iter()
                .find(|m| m[0] == r && m[1] == col)
                .map_or(1, |m| {
                    page[c..]
                        .iter()
                        .take_while(|x| (m[1]..=m[3]).contains(&x.0))
                        .count()
                })
                .max(1);
            let covered = s.merged.iter().any(|m| {
                (m[0]..=m[2]).contains(&r)
                    && (m[1]..=m[3]).contains(&col)
                    && (m[0], m[1]) != (r, col)
            });
            let width: f32 = page[c..c + span].iter().map(|x| x.1).sum::<f32>() - 4.0 / 72.27;
            let cell = s.cells.get(&(r, col));
            let mut text = String::new();
            let first = c == 0 && !setup.headings;
            let mut left = if first && setup.gridlines { grid } else { "" };
            let mut right = if setup.gridlines { grid } else { "" };
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
                    left = black;
                }
                if g.borders[1].is_some() {
                    right = black;
                }
                if g.borders[2].is_some() {
                    rules.push((c, c + span));
                }
            }
            let mut lead = String::new();
            if c == 0 && !setup.headings {
                lead.push_str(&strut(r));
            }
            if let Some(o) = (c..c + span).find_map(|i| overlays.get(&(r, i))) {
                lead.push_str(o);
            }
            cells.push(format!(
                "\\multicolumn{{{span}}}{{{left}@{{\\hspace{{2pt}}}}>{{{align}\\arraybackslash}}p{{{width:.3}in}}@{{\\hspace{{2pt}}}}{right}}}{{{fill}{lead}{text}}}"
            ));
            c += span;
        }
        out.push_str(&cells.join(" & "));
        out.push_str(" \\\\");
        if setup.gridlines {
            out.push_str(gray_rule);
        }
        let shift = usize::from(setup.headings);
        for (from, to) in rules {
            out.push_str(&format!("\\cline{{{}-{}}}", from + shift + 1, to + shift));
        }
        out.push('\n');
    };
    // The headings, the rows repeated on every page, then the others.
    if setup.gridlines {
        out.push_str(gray_rule);
        out.push('\n');
    }
    if setup.headings {
        let g = if setup.gridlines { grid } else { "" };
        let mut cells = vec![format!(
            "\\multicolumn{{1}}{{{g}p{{{:.3}in}}{g}}}{{\\cellcolor[HTML]{{F2F2F2}}}}",
            heading_w - 4.0 / 72.27
        )];
        for (c, w) in &page {
            cells.push(format!(
                "\\multicolumn{{1}}{{@{{\\hspace{{2pt}}}}>{{\\centering\\arraybackslash}}p{{{:.3}in}}@{{\\hspace{{2pt}}}}{g}}}{{\\cellcolor[HTML]{{F2F2F2}}{}}}",
                w - 4.0 / 72.27,
                letters(*c)
            ));
        }
        out.push_str(&cells.join(" & "));
        out.push_str(" \\\\");
        if setup.gridlines {
            out.push_str(gray_rule);
        }
        out.push('\n');
    }
    let titles = setup.title_rows;
    if let Some((t0, t1)) = titles {
        for r in t0..=t1 {
            if !s.hidden_rows.contains(&r) {
                row(out, r);
            }
        }
    }
    if titles.is_some() || setup.headings {
        out.push_str("\\endhead\n");
    }
    for r in a[0]..=a[2] {
        if titles.is_some_and(|(t0, t1)| (t0..=t1).contains(&r)) || s.hidden_rows.contains(&r) {
            continue;
        }
        if r > a[0] && setup.row_breaks.contains(&r) {
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
        let none = [None, None, None];
        let h = header_footer(
            "&LKalem&C&A&RPage &P of &N",
            "Bütçe",
            "b.xlsx",
            "4.10.2026",
            "10:00",
            &none,
        );
        assert_eq!(h[0], "Kalem");
        assert_eq!(h[1], "Bütçe");
        assert_eq!(h[2], "Page \\thepage{} of \\pageref{kalemlastpage}");
        let h = header_footer("&\"Arial,Bold\"&14Rapor &F", "S", "b.xlsx", "", "", &none);
        assert_eq!(h[1], "Rapor b.xlsx");
        let pics = [Some("PIC".to_string()), None, None];
        assert_eq!(header_footer("&L&G", "S", "", "", "", &pics)[0], "PIC");
        assert_eq!(letters(0), "A");
        assert_eq!(letters(27), "AB");
    }

    #[test]
    fn columns_over_pages() {
        assert_eq!(column_pages(&[3.0, 3.0, 3.0], 7.0, &[]), vec![0..2, 2..3]);
        assert_eq!(column_pages(&[9.0, 1.0], 7.0, &[]), vec![0..1, 1..2]);
        assert_eq!(column_pages(&[1.0, 1.0, 1.0], 7.0, &[1]), vec![0..1, 1..3]);
        assert_eq!(
            column_pages(&[], 7.0, &[]),
            Vec::<std::ops::Range<usize>>::new()
        );
        // Rows: a page every two, a manual break before the third row.
        let sizes = [(0, 3.0), (1, 3.0), (2, 3.0), (3, 3.0), (4, 3.0)];
        assert_eq!(page_starts(&sizes, 7.0, &[]), vec![(2, false), (4, false)]);
        assert_eq!(page_starts(&sizes, 7.0, &[1]), vec![(1, true), (3, false)]);
        // Fitted two pages tall: half the size of four pages' rows.
        let setup = PageSetup {
            fit: Some((0, 2)),
            margins: [0.0, 0.0, 0.0, 0.0],
            ..PageSetup::default()
        };
        assert!((scale(&setup, 1.0, 11.69 * 4.0) - 0.5).abs() < 0.001);
        let half = PageSetup {
            scale: 50,
            ..PageSetup::default()
        };
        assert_eq!(scale(&half, 100.0, 100.0), 0.5);
    }

    fn sheet() -> SheetPrint {
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
        SheetPrint {
            name: "Bütçe".into(),
            area: [0, 0, 1, 1],
            columns: vec![0, 1],
            widths: vec![20.0, 10.0],
            title_columns: vec![],
            hidden_rows: vec![],
            heights: Default::default(),
            default_height: 15.0,
            cells,
            merged: vec![],
            drawings: vec![],
            setup: PageSetup {
                landscape: true,
                title_rows: Some((0, 0)),
                footer: "&CPage &P".into(),
                ..PageSetup::default()
            },
        }
    }

    #[test]
    fn a_sheet_as_latex() {
        let s = sheet();
        let tex = document(&[s], "b.xlsx", "", "");
        if let Ok(dir) = std::env::var("KALEM_TEX_OUT") {
            std::fs::write(format!("{dir}/sheet.tex"), &tex).unwrap();
        }
        assert!(tex.contains("[a4paper,landscape]{geometry}"), "{tex}");
        assert!(
            tex.contains(
                "\\cellcolor[HTML]{4472C4}\\rule[-4.50bp]{0pt}{15.00bp}\\textbf{Gelir \\& Gider}"
            ),
            "{tex}"
        );
        assert!(tex.contains("\\raggedleft"), "{tex}");
        assert!(tex.contains("\\endhead"), "{tex}");
        assert!(tex.contains("\\cfoot{Page \\thepage{}}"), "{tex}");
    }

    #[test]
    fn sheets_on_their_own_paper_with_drawings() {
        let mut a = sheet();
        a.setup.gridlines = true;
        a.setup.headings = true;
        a.setup.header = "&L&G".into();
        a.setup.pictures = vec![kalem_viewer::HeaderPicture {
            place: "LH".into(),
            data: b"\x89PNG....".to_vec(),
            size: (40.0, 20.0),
        }];
        a.drawings = vec![PrintDrawing {
            anchor: [1, 0, 1, 0],
            size: (1.0, 0.5),
            what: PrintWhat::Shape {
                preset: "ellipse".into(),
                fill: Some([0xFF, 0, 0]),
                line: None,
                text: "Not".into(),
            },
        }];
        let mut b = sheet();
        b.setup.landscape = false;
        b.setup.paper = 1;
        b.setup.title_rows = None;
        b.title_columns = vec![(0, 5.0)];
        b.area = [0, 1, 1, 1];
        b.columns = vec![1];
        b.widths = vec![10.0];
        let tex = document(&[a.clone(), b], "b.xlsx", "", "");
        if let Ok(dir) = std::env::var("KALEM_TEX_OUT") {
            std::fs::write(format!("{dir}/sheets.tex"), &tex).unwrap();
        }
        assert!(
            tex.contains("\\paperwidth=11.69in \\paperheight=8.27in"),
            "{tex}"
        );
        assert!(
            tex.contains("\\paperwidth=8.5in \\paperheight=11in"),
            "{tex}"
        );
        assert!(
            tex.contains("\\includegraphics[width=40.0pt,height=20.0pt]{sheet0-LH.png}"),
            "{tex}"
        );
        assert!(tex.contains("ellipse[x radius"), "{tex}");
        assert!(tex.contains("\\color[gray]{0.75}\\vrule"), "{tex}");
        assert!(tex.contains("{\\cellcolor[HTML]{F2F2F2}B}"), "{tex}");
        // The second sheet's title column A before its column B.
        let second = &tex[tex.find("\\paperwidth=8.5in").unwrap()..];
        assert!(second.contains("Gelir \\& Gider"), "{second}");
        assert_eq!(images(&[a])[0].0, "sheet0-LH.png");
    }
}
