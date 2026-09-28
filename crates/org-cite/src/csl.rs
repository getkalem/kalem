//! CSL rendering, with `hayagriva`: the citations of a document and its
//! bibliography in a Citation Style Language style (`#+CITE_EXPORT: csl
//! apa`), as Org's `csl` processor renders them with `citeproc-el`.
//!
//! The rendering comes back as [`Span`]s, formatted text that an exporter
//! turns into its own markup; each cited item's part of a citation is an
//! [`Span::Entry`], so that the Org prefixes and suffixes of references go
//! around it.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use hayagriva::citationberg::{
    self, FontStyle, FontVariant, FontWeight, IndependentStyle, Locale, LocaleCode, Style,
    StyleClass, TextDecoration, VerticalAlign,
};
use hayagriva::{
    BibliographyDriver, BibliographyRequest, CitationItem, CitationRequest, CitePurpose, ElemChild,
    ElemChildren, ElemMeta, Entry, LocatorPayload, SpecificLocator,
};

/// The style Org uses when `#+CITE_EXPORT: csl` names none.
pub const DEFAULT_STYLE: &str = "chicago-author-date";

/// The entries of the bibliography files, for CSL.
#[derive(Debug, Default)]
pub struct Library {
    entries: Vec<Entry>,
}

impl Library {
    /// Reads `files`: BibTeX and BibLaTeX (`.bib`, `.bibtex`), CSL-JSON
    /// (`.json`) and hayagriva's YAML (`.yaml`, `.yml`). A key seen before
    /// keeps its first entry. The files that cannot be read come back with
    /// their errors.
    pub fn load(files: &[PathBuf]) -> (Library, Vec<(PathBuf, String)>) {
        let mut lib = Library::default();
        let mut errors = Vec::new();
        for f in files {
            match read(f) {
                Ok(entries) => {
                    for e in entries {
                        if lib.get(e.key()).is_none() {
                            lib.entries.push(e);
                        }
                    }
                }
                Err(e) => errors.push((f.clone(), e)),
            }
        }
        (lib, errors)
    }

    /// The entry with `key`.
    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.key() == key)
    }
}

fn read(file: &Path) -> Result<Vec<Entry>, String> {
    let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let lib = match ext.as_str() {
        "json" => {
            let yaml = csl_json_to_yaml(&text)?;
            hayagriva::io::from_yaml_str(&yaml).map_err(|e| e.to_string())?
        }
        "yaml" | "yml" => hayagriva::io::from_yaml_str(&text).map_err(|e| e.to_string())?,
        _ => hayagriva::io::from_biblatex_str(&text).map_err(|errs| {
            errs.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        })?,
    };
    Ok(lib.into_iter().collect())
}

/// CSL-JSON items as hayagriva's YAML (written as JSON, which YAML
/// reads): the types, names, dates and the fields most styles use.
fn csl_json_to_yaml(text: &str) -> Result<String, String> {
    use serde_json::{Map, Value, json};
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let items = v.as_array().ok_or("a CSL-JSON file holds a list")?;
    let mut out = Map::new();
    let s = |v: Option<&Value>| -> Option<String> {
        match v? {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    };
    let names = |v: Option<&Value>| -> Option<Value> {
        let a = v?.as_array()?;
        let list: Vec<Value> = a
            .iter()
            .filter_map(|n| {
                if let Some(l) = n.get("literal").and_then(Value::as_str) {
                    return Some(json!({ "name": l }));
                }
                let family = n.get("family").and_then(Value::as_str)?;
                let mut p = Map::new();
                p.insert("name".into(), family.into());
                if let Some(g) = n.get("given").and_then(Value::as_str) {
                    p.insert("given-name".into(), g.into());
                }
                if let Some(x) = n.get("non-dropping-particle").and_then(Value::as_str) {
                    p.insert("prefix".into(), x.into());
                }
                if let Some(x) = n.get("suffix").and_then(Value::as_str) {
                    p.insert("suffix".into(), x.into());
                }
                Some(Value::Object(p))
            })
            .collect();
        (!list.is_empty()).then_some(Value::Array(list))
    };
    let date = |v: Option<&Value>| -> Option<String> {
        let v = v?;
        if let Some(parts) = v
            .get("date-parts")
            .and_then(|d| d.get(0))
            .and_then(Value::as_array)
        {
            let p: Vec<i64> = parts
                .iter()
                .filter_map(|x| x.as_i64().or_else(|| x.as_str()?.parse().ok()))
                .collect();
            return match p.as_slice() {
                [y] => Some(format!("{y:04}")),
                [y, m] => Some(format!("{y:04}-{m:02}")),
                [y, m, d, ..] => Some(format!("{y:04}-{m:02}-{d:02}")),
                [] => None,
            };
        }
        v.get("raw")
            .or_else(|| v.get("literal"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    for item in items {
        let Some(key) = s(item.get("id")) else {
            continue;
        };
        let csl_type = s(item.get("type")).unwrap_or_default();
        let (kind, parent_kind) = match csl_type.as_str() {
            "article-journal" => ("article", Some("periodical")),
            "article-magazine" | "article-newspaper" => ("article", Some("newspaper")),
            "article" => ("article", None),
            "book" => ("book", None),
            "chapter" | "entry-encyclopedia" | "entry-dictionary" => ("chapter", Some("book")),
            "paper-conference" => ("article", Some("proceedings")),
            "thesis" => ("thesis", None),
            "report" => ("report", None),
            "webpage" | "post-weblog" | "post" => ("web", None),
            "motion_picture" | "broadcast" => ("video", None),
            "software" => ("repository", None),
            "patent" => ("patent", None),
            "legislation" | "legal_case" => ("case", None),
            "manuscript" => ("manuscript", None),
            _ => ("misc", None),
        };
        let mut e = Map::new();
        e.insert("type".into(), kind.into());
        let fields: [(&str, &str); 11] = [
            ("title", "title"),
            ("publisher", "publisher"),
            ("publisher-place", "location"),
            ("volume", "volume"),
            ("issue", "issue"),
            ("page", "page-range"),
            ("edition", "edition"),
            ("DOI", "doi"),
            ("ISBN", "isbn"),
            ("URL", "url"),
            ("language", "language"),
        ];
        let mut parent = Map::new();
        for (from, to) in fields {
            if let Some(v) = s(item.get(from)) {
                // The issue and volume of an article are its journal's.
                if parent_kind.is_some() && matches!(to, "volume" | "issue" | "publisher") {
                    parent.insert(to.into(), v.into());
                } else {
                    e.insert(to.into(), v.into());
                }
            }
        }
        if let Some(a) = names(item.get("author")) {
            e.insert("author".into(), a);
        }
        if let Some(a) = names(item.get("editor")) {
            e.insert("editor".into(), a);
        }
        if let Some(d) = date(item.get("issued")) {
            e.insert("date".into(), d.into());
        }
        if let Some(n) = s(item.get("note")) {
            e.insert("note".into(), n.into());
        }
        if let Some(pk) = parent_kind {
            parent.insert("type".into(), pk.into());
            if let Some(t) = s(item.get("container-title")) {
                parent.insert("title".into(), t.into());
            }
            e.insert("parent".into(), Value::Object(parent));
        }
        out.insert(key, Value::Object(e));
    }
    serde_json::to_string(&Value::Object(out)).map_err(|e| e.to_string())
}

/// A CSL style with its locales.
#[derive(Debug)]
pub struct Processor {
    style: IndependentStyle,
    locales: Vec<Locale>,
    locale: Option<LocaleCode>,
}

/// How an item is cited (Org's citation styles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The style's citation.
    Normal,
    /// In the text: Author (Year).
    Text,
    /// The authors only.
    Author,
    /// The year only.
    Year,
    /// The whole bibliography entry.
    Full,
}

/// An item of a citation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRequest {
    /// The key.
    pub key: String,
    /// The locator: its CSL label (`page`, `chapter`) and value.
    pub locator: Option<(String, String)>,
    /// How it is cited.
    pub mode: Mode,
}

/// A citation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiteRequest {
    /// Its items.
    pub items: Vec<ItemRequest>,
    /// `nocite`: only in the bibliography.
    pub hidden: bool,
    /// The number of the footnote it is in, for note styles.
    pub note_number: Option<usize>,
}

/// How rendered text looks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Format {
    /// Italic.
    pub italic: bool,
    /// Bold.
    pub bold: bool,
    /// Small capitals.
    pub small_caps: bool,
    /// Underlined.
    pub underline: bool,
    /// Superscript.
    pub superscript: bool,
    /// Subscript.
    pub subscript: bool,
}

/// How a group of rendered text is displayed (CSL's `display`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    /// On its own line.
    Block,
    /// In the left margin (the label of a numbered entry).
    LeftMargin,
    /// To the right of the margin.
    RightInline,
    /// Indented.
    Indent,
}

/// Rendered text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    /// Text.
    Text(String, Format),
    /// A group, displayed some way.
    Group(Option<Display>, Vec<Span>),
    /// The part of a citation for its `n`th item.
    Entry(usize, Vec<Span>),
    /// A link.
    Link(String, Vec<Span>),
}

/// A rendered bibliography.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bibliography {
    /// Entries after the first line are indented.
    pub hanging_indent: bool,
    /// The first field (a number or label) is aligned apart.
    pub second_field_align: bool,
    /// The longest first field, in characters.
    pub max_label: usize,
    /// Extra lines between entries.
    pub entry_spacing: i16,
    /// The entries: their keys, first fields and contents.
    pub items: Vec<(String, Option<Vec<Span>>, Vec<Span>)>,
}

/// What rendering gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The citations, in the order asked; a hidden one is empty.
    pub citations: Vec<Vec<Span>>,
    /// The bibliography, if the style has one.
    pub bibliography: Option<Bibliography>,
}

impl Processor {
    /// The style `name`: a `.csl` file (absolute, or relative to `dir`),
    /// else one of the styles Kalem ships (`apa`, `ieee`,
    /// `chicago-author-date`, `chicago-notes`, `mla`…); `None` is
    /// [`DEFAULT_STYLE`]. `language` is the document's (`en`, `tr-TR`).
    pub fn new(
        name: Option<&str>,
        dir: Option<&Path>,
        language: Option<&str>,
    ) -> Result<Processor, String> {
        let name = name.unwrap_or(DEFAULT_STYLE);
        let path = Path::new(name);
        let file = if path.is_absolute() {
            Some(path.to_path_buf())
        } else {
            dir.map(|d| d.join(name))
                .filter(|p| p.is_file())
                .or_else(|| path.is_file().then(|| path.to_path_buf()))
        };
        let style = match file {
            Some(f) => {
                let xml =
                    std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
                Style::from_xml(&xml).map_err(|e| format!("{}: {e}", f.display()))?
            }
            None => {
                let stem = name.strip_suffix(".csl").unwrap_or(name);
                hayagriva::archive::ArchivedStyle::by_name(stem)
                    .ok_or_else(|| format!("CSL style file not found: {name:?}"))?
                    .get()
            }
        };
        let style = match style {
            Style::Independent(s) => s,
            Style::Dependent(d) => {
                // A dependent style names its parent by URL; the parent's
                // name is the URL's last part.
                let parent = d
                    .parent_link
                    .href
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .to_string();
                match hayagriva::archive::ArchivedStyle::by_name(&parent).map(|s| s.get()) {
                    Some(Style::Independent(s)) => s,
                    _ => return Err(format!("CSL parent style not found: {parent:?}")),
                }
            }
        };
        let locale = language
            .filter(|l| !l.trim().is_empty())
            .map(|l| LocaleCode(l.replace('_', "-")));
        Ok(Processor {
            style,
            locales: hayagriva::archive::locales(),
            locale,
        })
    }

    /// Whether citations are notes (`class="note"`).
    pub fn note_style(&self) -> bool {
        self.style.settings.class == StyleClass::Note
    }

    /// Whether citations are superscript numbers.
    pub fn superscript(&self) -> bool {
        self.style.citation.layout.vertical_align == Some(VerticalAlign::Sup)
    }

    /// The text around a citation (`(` and `)` in author–date styles).
    pub fn affixes(&self) -> (Option<&str>, Option<&str>) {
        let l = &self.style.citation.layout;
        (l.prefix.as_deref(), l.suffix.as_deref())
    }

    /// Renders `cites` from `lib`: keys not in it are left out of their
    /// citation (a citation of none of them is `??`).
    pub fn render(&self, lib: &Library, cites: &[CiteRequest]) -> Rendered {
        let mut driver: BibliographyDriver<'_, Entry> = BibliographyDriver::new();
        // The citations given to the driver, and where each one's is.
        let mut asked: Vec<Option<usize>> = Vec::new();
        let mut n = 0;
        for c in cites {
            let items: Vec<CitationItem<'_, Entry>> = c
                .items
                .iter()
                .filter_map(|i| {
                    let entry = lib.get(&i.key)?;
                    let locator = i.locator.as_ref().map(|(label, value)| {
                        let l = citationberg::taxonomy::Locator::from_str(label)
                            .unwrap_or(citationberg::taxonomy::Locator::Page);
                        SpecificLocator(l, LocatorPayload::Str(value.as_str()))
                    });
                    let purpose = match i.mode {
                        Mode::Normal => None,
                        Mode::Text => Some(CitePurpose::Prose),
                        Mode::Author => Some(CitePurpose::Author),
                        Mode::Year => Some(CitePurpose::Year),
                        Mode::Full => Some(CitePurpose::Full),
                    };
                    Some(CitationItem::new(entry, locator, None, c.hidden, purpose))
                })
                .collect();
            if items.is_empty() {
                asked.push(None);
                continue;
            }
            driver.citation(CitationRequest::new(
                items,
                &self.style,
                self.locale.clone(),
                &self.locales,
                c.note_number,
            ));
            asked.push(Some(n));
            n += 1;
        }
        let out = driver.finish(BibliographyRequest::new(
            &self.style,
            self.locale.clone(),
            &self.locales,
        ));
        let citations = cites
            .iter()
            .zip(asked)
            .map(|(c, a)| match a {
                _ if c.hidden => Vec::new(),
                Some(i) => out
                    .citations
                    .get(i)
                    .map(|r| spans(&r.citation))
                    .unwrap_or_default(),
                None => vec![Span::Text("??".into(), Format::default())],
            })
            .collect();
        let bibliography = out.bibliography.map(|b| {
            let items: Vec<(String, Option<Vec<Span>>, Vec<Span>)> = b
                .items
                .iter()
                .map(|i| {
                    (
                        i.key.clone(),
                        i.first_field
                            .as_ref()
                            .map(|f| spans_of(std::slice::from_ref(f))),
                        spans(&i.content),
                    )
                })
                .collect();
            let max_label = items
                .iter()
                .filter_map(|(_, f, _)| f.as_ref().map(|f| plain(f).chars().count()))
                .max()
                .unwrap_or(0);
            Bibliography {
                hanging_indent: b.hanging_indent,
                second_field_align: b.second_field_align.is_some(),
                max_label,
                entry_spacing: b.entry_spacing,
                items,
            }
        });
        Rendered {
            citations,
            bibliography,
        }
    }
}

fn spans(c: &ElemChildren) -> Vec<Span> {
    spans_of(&c.0)
}

fn spans_of(children: &[ElemChild]) -> Vec<Span> {
    let mut out = Vec::new();
    for c in children {
        match c {
            ElemChild::Text(t) => out.push(Span::Text(t.text.clone(), format(&t.formatting))),
            ElemChild::Elem(e) => {
                let inner = spans(&e.children);
                match (&e.meta, e.display) {
                    (Some(ElemMeta::Entry(i)), _) => out.push(Span::Entry(*i, inner)),
                    (_, None) => out.extend(inner),
                    (_, Some(d)) => out.push(Span::Group(
                        Some(match d {
                            hayagriva::citationberg::Display::Block => Display::Block,
                            hayagriva::citationberg::Display::LeftMargin => Display::LeftMargin,
                            hayagriva::citationberg::Display::RightInline => Display::RightInline,
                            hayagriva::citationberg::Display::Indent => Display::Indent,
                        }),
                        inner,
                    )),
                }
            }
            ElemChild::Markup(m) => out.push(Span::Text(m.clone(), Format::default())),
            ElemChild::Link { text, url } => out.push(Span::Link(
                url.clone(),
                vec![Span::Text(text.text.clone(), format(&text.formatting))],
            )),
            ElemChild::Transparent { .. } => {}
        }
    }
    out
}

fn format(f: &hayagriva::Formatting) -> Format {
    Format {
        italic: f.font_style == FontStyle::Italic,
        bold: f.font_weight == FontWeight::Bold,
        small_caps: f.font_variant == FontVariant::SmallCaps,
        underline: f.text_decoration == TextDecoration::Underline,
        superscript: f.vertical_align == VerticalAlign::Sup,
        subscript: f.vertical_align == VerticalAlign::Sub,
    }
}

/// The text of `spans`, without formatting.
pub fn plain(spans: &[Span]) -> String {
    let mut out = String::new();
    for s in spans {
        match s {
            Span::Text(t, _) => out.push_str(t),
            Span::Group(_, c) | Span::Entry(_, c) | Span::Link(_, c) => out.push_str(&plain(c)),
        }
    }
    out
}

/// `org-cite-csl--label-alist`: locator names and their CSL labels.
pub const LABELS: &[(&str, &str)] = &[
    ("bk.", "book"),
    ("bks.", "book"),
    ("book", "book"),
    ("chap.", "chapter"),
    ("chaps.", "chapter"),
    ("chapter", "chapter"),
    ("col.", "column"),
    ("cols.", "column"),
    ("column", "column"),
    ("figure", "figure"),
    ("fig.", "figure"),
    ("figs.", "figure"),
    ("folio", "folio"),
    ("fol.", "folio"),
    ("fols.", "folio"),
    ("number", "number"),
    ("no.", "number"),
    ("nos.", "number"),
    ("line", "line"),
    ("l.", "line"),
    ("ll.", "line"),
    ("note", "note"),
    ("n.", "note"),
    ("nn.", "note"),
    ("opus", "opus"),
    ("op.", "opus"),
    ("opp.", "opus"),
    ("page", "page"),
    ("p", "page"),
    ("p.", "page"),
    ("pp.", "page"),
    ("paragraph", "paragraph"),
    ("para.", "paragraph"),
    ("paras.", "paragraph"),
    ("\\P", "paragraph"),
    ("¶", "paragraph"),
    ("\\P\\P", "paragraph"),
    ("¶¶", "paragraph"),
    ("part", "part"),
    ("pt.", "part"),
    ("pts.", "part"),
    ("§", "section"),
    ("\\S", "section"),
    ("§§", "section"),
    ("\\S\\S", "section"),
    ("section", "section"),
    ("sec.", "section"),
    ("secs.", "section"),
    ("sub verbo", "sub verbo"),
    ("s.v.", "sub verbo"),
    ("s.vv.", "sub verbo"),
    ("verse", "verse"),
    ("v.", "verse"),
    ("vv.", "verse"),
    ("volume", "volume"),
    ("vol.", "volume"),
    ("vols.", "volume"),
];

/// A reference's suffix split as `org-cite-csl--parse-reference` splits
/// it: the text before the locator, the locator's label and value, and
/// the text after. `None` when it holds no locator.
pub fn split_locator(suffix: &str) -> Option<(String, String, String, String)> {
    // A label at the start or after a space, followed by digits and a
    // space or the end (`org-cite-csl--label-regexp`); the longest name
    // wins, as `regexp-opt` makes it.
    let mut found: Option<(usize, &str, &str)> = None;
    for (i, _) in suffix.char_indices() {
        if i > 0 && !suffix[..i].ends_with(char::is_whitespace) {
            continue;
        }
        let rest = &suffix[i..];
        let best = LABELS
            .iter()
            .filter(|(name, _)| {
                rest.starts_with(name) && {
                    let after = rest[name.len()..].trim_start_matches(|c: char| c.is_ascii_digit());
                    let word_end = rest[..name.len()]
                        .chars()
                        .last()
                        .is_some_and(|c| !c.is_alphanumeric())
                        || !after.starts_with(char::is_alphanumeric);
                    after.is_empty() || after.starts_with([' ', '\u{a0}']) || word_end
                }
            })
            .max_by_key(|(name, _)| name.len());
        if let Some((name, label)) = best {
            found = Some((i, name, label));
            break;
        }
    }
    let (location_start, label, locator_start) = match found {
        Some((i, name, label)) => {
            let after = &suffix[i + name.len()..];
            let skip = after.len() - after.trim_start_matches([' ', '\t', '\n', '\u{a0}']).len();
            (i, label.to_string(), i + name.len() + skip)
        }
        None => {
            let i = suffix.find(|c: char| c.is_ascii_digit())?;
            (i, "page".to_string(), i)
        }
    };
    // The locator ends at the last digit, or before the last comma.
    let tail = &suffix[location_start..];
    let end = tail
        .char_indices()
        .rev()
        .find(|(_, c)| *c == ',' || c.is_ascii_digit())
        .map(|(j, c)| {
            if c == ',' {
                location_start + j
            } else {
                location_start + j + 1
            }
        })?;
    let after_start = if suffix[end..].starts_with(',') {
        end + 1
    } else {
        end
    };
    let locator = suffix[locator_start.min(end)..end].trim().to_string();
    Some((
        suffix[..location_start].to_string(),
        label,
        locator,
        suffix[after_start..].to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> Library {
        let dir = std::env::temp_dir().join(format!("org-cite-csl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bib = dir.join("refs.bib");
        std::fs::write(
            &bib,
            "@book{knuth84, author = {Donald E. Knuth}, title = {The {\\TeX}book}, publisher = {Addison-Wesley}, year = 1984}\n\
             @article{doe20, author = {Doe, Jane and Smith, John}, title = {A study}, journal = {Journal}, year = 2020, volume = 3, pages = {1--10}}\n",
        )
        .unwrap();
        let json = dir.join("more.json");
        std::fs::write(
            &json,
            r#"[{"id": "adams05", "type": "article-journal", "title": "From JSON", "author": [{"family": "Adams", "given": "Ada"}], "container-title": "Json Journal", "issued": {"date-parts": [[2005, 3]]}}]"#,
        )
        .unwrap();
        let (lib, errors) = Library::load(&[bib, json]);
        assert!(errors.is_empty(), "{errors:?}");
        lib
    }

    fn item(key: &str, mode: Mode) -> ItemRequest {
        ItemRequest {
            key: key.into(),
            locator: None,
            mode,
        }
    }

    #[test]
    fn author_date() {
        let lib = lib();
        let p = Processor::new(None, None, Some("en")).unwrap();
        assert!(!p.note_style());
        let r = p.render(
            &lib,
            &[
                CiteRequest {
                    items: vec![ItemRequest {
                        locator: Some(("page".into(), "12".into())),
                        ..item("knuth84", Mode::Normal)
                    }],
                    hidden: false,
                    note_number: None,
                },
                CiteRequest {
                    items: vec![item("doe20", Mode::Text)],
                    hidden: false,
                    note_number: None,
                },
                CiteRequest {
                    items: vec![item("adams05", Mode::Normal)],
                    hidden: true,
                    note_number: None,
                },
                CiteRequest {
                    items: vec![item("nope", Mode::Normal)],
                    hidden: false,
                    note_number: None,
                },
            ],
        );
        assert_eq!(plain(&r.citations[0]), "(Knuth 1984, 12)");
        assert_eq!(plain(&r.citations[1]), "Doe and Smith (2020)");
        assert!(r.citations[2].is_empty());
        assert_eq!(plain(&r.citations[3]), "??");
        let b = r.bibliography.unwrap();
        let keys: Vec<&str> = b.items.iter().map(|(k, _, _)| k.as_str()).collect();
        assert_eq!(keys, ["adams05", "doe20", "knuth84"]);
        let adams = plain(&b.items[0].2);
        assert_eq!(adams, "Adams, Ada. 2005. “From Json.” Json Journal, March.");
        assert!(b.hanging_indent);
    }

    #[test]
    fn numeric_and_notes() {
        let lib = lib();
        let ieee = Processor::new(Some("ieee"), None, None).unwrap();
        let r = ieee.render(
            &lib,
            &[CiteRequest {
                items: vec![item("doe20", Mode::Normal), item("knuth84", Mode::Normal)],
                hidden: false,
                note_number: None,
            }],
        );
        assert_eq!(plain(&r.citations[0]), "[1], [2]");
        let b = r.bibliography.unwrap();
        assert!(b.second_field_align);
        assert_eq!(b.max_label, 3);
        let notes = Processor::new(Some("chicago-notes.csl"), None, None).unwrap();
        assert!(notes.note_style());
        assert!(Processor::new(Some("no-such-style"), None, None).is_err());
    }

    #[test]
    fn locators() {
        assert_eq!(
            split_locator(" p. 3"),
            Some((" ".into(), "page".into(), "3".into(), String::new()))
        );
        assert_eq!(
            split_locator(" chap. 2, and more"),
            Some((" ".into(), "chapter".into(), "2".into(), " and more".into()))
        );
        assert_eq!(
            split_locator(" 12-14"),
            Some((" ".into(), "page".into(), "12-14".into(), String::new()))
        );
        assert_eq!(split_locator(" see also"), None);
    }
}
