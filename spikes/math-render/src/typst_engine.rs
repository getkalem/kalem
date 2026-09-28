//! Option A: LaTeX → Typst math with MiTeX, laid out by the typst library,
//! exported with typst-svg.

use std::collections::HashMap;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::layout::{Abs, Frame, FrameItem, Point};
use typst_layout::PagedDocument;
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

use super::{Engine, Formula, Rendered};

const SPECS: &[(&str, &str)] = &[
    ("specs/mod.typ", include_str!("../mitex-specs/mod.typ")),
    ("specs/prelude.typ", include_str!("../mitex-specs/prelude.typ")),
    ("specs/latex/standard.typ", include_str!("../mitex-specs/latex/standard.typ")),
];

/// Fonts a math renderer needs: the math font and the text fonts for
/// `\text`, `\mathrm` and friends.
const FONTS: &[&str] = &["NewCMMath-Regular", "NewCM10-Regular", "NewCM10-Italic", "NewCM10-Bold", "NewCM10-BoldItalic"];

pub struct Typst {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    main: FileId,
    source: Source,
    files: HashMap<FileId, Source>,
}

fn file_id(path: &str) -> FileId {
    FileId::new(RootedPath::new(VirtualRoot::Project, VirtualPath::new(path).expect("path")))
}

impl Typst {
    pub fn new() -> Self {
        let fonts: Vec<Font> = typst_assets::fonts()
            .flat_map(|data| Font::iter(Bytes::new(data)))
            .filter(|f| {
                let name = f.info().family.replace(' ', "");
                let style = format!("{:?}{:?}", f.info().variant.weight, f.info().variant.style);
                // Keep New Computer Modern (math and text) only.
                let _ = style;
                name.starts_with("NewComputerModern")
            })
            .collect();
        let _ = FONTS;
        let book = FontBook::from_fonts(&fonts);
        let main = file_id("main.typ");
        let files = SPECS.iter().map(|(p, s)| (file_id(p), Source::new(file_id(p), s.to_string()))).collect();
        Typst {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book),
            fonts,
            main,
            source: Source::new(main, String::new()),
            files,
        }
    }
}

impl World for Typst {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }
    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }
    fn main(&self) -> FileId {
        self.main
    }
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main {
            return Ok(self.source.clone());
        }
        self.files.get(&id).cloned().ok_or_else(|| FileError::NotFound(std::path::PathBuf::from(format!("{id:?}"))))
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.source(id).map(|s| Bytes::from_string(s.text().to_string()))
    }
    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }
    fn today(&self, _: Option<typst::foundations::Duration>) -> Option<Datetime> {
        None
    }
}

/// Escapes `s` as a Typst string literal body.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}

/// The baseline of the first line in `frame`, relative to its top.
fn baseline(frame: &Frame, at: Point) -> Option<Abs> {
    for (pos, item) in frame.items() {
        let p = at + *pos;
        match item {
            FrameItem::Group(g) => {
                if g.frame.has_baseline() {
                    return Some(p.y + g.frame.baseline());
                }
                if let Some(b) = baseline(&g.frame, p) {
                    return Some(b);
                }
            }
            FrameItem::Text(_) => return Some(p.y),
            _ => {}
        }
    }
    None
}

impl Engine for Typst {
    fn name(&self) -> &'static str {
        "typst"
    }

    fn render(&mut self, f: &Formula, font_size: f64) -> Result<Rendered, String> {
        let converted = mitex::convert_math(&f.latex, None)?;
        let pt = font_size * 0.75;
        let text = format!(
            "#import \"/specs/mod.typ\": mitex-scope\n\
             #set page(width: auto, height: auto, margin: 0pt, fill: none)\n\
             #set text(size: {pt}pt, top-edge: \"bounds\", bottom-edge: \"bounds\")\n\
             #math.equation(block: {}, eval(\"$\" + \"{}\" + \"$\", scope: mitex-scope))\n",
            f.display,
            escape(&converted)
        );
        self.source = Source::new(self.main, text);
        let result = typst::compile::<PagedDocument>(self);
        let doc = result.output.map_err(|errs| errs.iter().map(|e| e.message.to_string()).collect::<Vec<_>>().join("; "))?;
        let page = doc.pages().first().ok_or("no page")?;
        let svg = typst_svg::svg(page, &Default::default());
        let size = page.frame.size();
        let scale = font_size / pt; // px per pt
        let b = baseline(&page.frame, Point::zero()).unwrap_or(size.y);
        Ok(Rendered { svg, width: size.x.to_pt() * scale, height: size.y.to_pt() * scale, baseline: b.to_pt() * scale })
    }
}
