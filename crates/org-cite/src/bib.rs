//! Bibliography files: BibTeX and BibLaTeX (`.bib`, `.bibtex`) read as
//! `bibtex-parse-entry` reads them, with `@string` abbreviations and the
//! month names expanded, and CSL-JSON (`.json`) as `oc-basic` reads it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A bibliography entry: its key, its type as written and its fields,
/// names in lower case, values with runs of blanks as one space
/// (`org-cite-basic--parse-bibtex`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The citation key.
    pub key: String,
    /// The entry type (`book`, `Article`).
    pub kind: String,
    /// The fields, in order.
    pub fields: Vec<(String, String)>,
}

impl Entry {
    /// A field's value.
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// The entries of the bibliography files of a document.
#[derive(Debug, Clone, Default)]
pub struct Bibliography {
    entries: Vec<Entry>,
    index: HashMap<String, usize>,
    skipped: Vec<(PathBuf, String)>,
}

impl Bibliography {
    /// The entries, file after file.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry with `key` (the first file that has it wins).
    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.index.get(key).map(|&i| &self.entries[i])
    }

    /// Adds entries; a key seen before keeps its first entry.
    pub fn extend(&mut self, entries: Vec<Entry>) {
        for e in entries {
            if self.index.contains_key(&e.key) {
                continue;
            }
            self.index.insert(e.key.clone(), self.entries.len());
            self.entries.push(e);
        }
    }

    /// Reads `files`, by their extensions (`.bib` and `.bibtex` as BibTeX,
    /// `.json` as CSL-JSON); the files that cannot be read are listed
    /// with the reason.
    pub fn load(files: &[PathBuf]) -> (Bibliography, Vec<(PathBuf, String)>) {
        let mut bib = Bibliography::default();
        let mut errors = Vec::new();
        for f in files {
            match read_tolerant(f) {
                Ok((entries, skipped)) => {
                    bib.extend(entries);
                    bib.skipped
                        .extend(skipped.into_iter().map(|e| (f.clone(), e)));
                }
                Err(e) => errors.push((f.clone(), e)),
            }
        }
        (bib, errors)
    }

    /// The entries of the files read that were malformed and left out,
    /// with the reason, as BibTeX reports an entry and goes on.
    pub fn skipped(&self) -> &[(PathBuf, String)] {
        &self.skipped
    }
}

/// The text of bibliography `file`, decoded as the editors decode it:
/// UTF-8 (its byte order mark left out), UTF-16 with its mark, else
/// Windows-1252, the Latin-1 that older BibTeX files are written in.
fn read_text(file: &Path) -> Result<String, String> {
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    let text = if let Some((encoding, bom)) = encoding_rs::Encoding::for_bom(&bytes) {
        encoding
            .decode_without_bom_handling(&bytes[bom..])
            .0
            .into_owned()
    } else {
        match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(e) => encoding_rs::WINDOWS_1252
                .decode_without_bom_handling(e.as_bytes())
                .0
                .into_owned(),
        }
    };
    // Windows' line endings as one, as BibTeX and Emacs read them: a
    // value over two lines has no carriage return in it.
    Ok(if text.contains('\r') {
        text.replace("\r\n", "\n")
    } else {
        text
    })
}

/// The entries of one file, a malformed BibTeX entry left out with its
/// reason (the rest of the file is read); `Err` when the file cannot be
/// read at all.
pub fn read_tolerant(file: &Path) -> Result<(Vec<Entry>, Vec<String>), String> {
    let lower = file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if matches!(lower.as_deref(), Some("bib" | "bibtex")) {
        let text = read_text(file)?;
        return Ok(parse_bibtex_tolerant(&text));
    }
    read(file).map(|e| (e, Vec::new()))
}

/// The entries of one file.
pub fn read(file: &Path) -> Result<Vec<Entry>, String> {
    let text = read_text(file)?;
    match file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("json") => parse_csl_json(&text),
        Some("bib" | "bibtex") => parse_bibtex(&text),
        Some("yaml" | "yml") => parse_yaml(&text),
        Some(e) => Err(format!("Unknown bibliography extension: {e:?}")),
        None => Err("Unknown bibliography extension".into()),
    }
}

/// `bibtex-predefined-month-strings`.
const MONTHS: [(&str, &str); 12] = [
    ("jan", "January"),
    ("feb", "February"),
    ("mar", "March"),
    ("apr", "April"),
    ("may", "May"),
    ("jun", "June"),
    ("jul", "July"),
    ("aug", "August"),
    ("sep", "September"),
    ("oct", "October"),
    ("nov", "November"),
    ("dec", "December"),
];

struct Parser<'a> {
    s: &'a str,
    pos: usize,
    strings: HashMap<String, String>,
}

impl Parser<'_> {
    fn skip_blank(&mut self) {
        while let Some(c) = self.s[self.pos..].chars().next() {
            if c.is_whitespace() {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<char> {
        self.s[self.pos..].chars().next()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    /// A name: letters, digits and `-_:.+/'!` and the like, up to a
    /// delimiter.
    fn name(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_whitespace() || matches!(c, '=' | ',' | '{' | '}' | '(' | ')' | '"' | '#' | '%')
            {
                break;
            }
            self.pos += c.len_utf8();
        }
        self.s[start..self.pos].to_string()
    }

    /// A braced text, its braces balanced, without the outer ones.
    fn braced(&mut self) -> Result<String, String> {
        let start = self.pos;
        let mut depth = 1;
        while let Some(c) = self.peek() {
            self.pos += c.len_utf8();
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(self.s[start..self.pos - 1].to_string());
                    }
                }
                _ => {}
            }
        }
        Err(format!("unbalanced braces at byte {start}"))
    }

    /// A quoted text: up to a `"` outside braces.
    fn quoted(&mut self) -> Result<String, String> {
        let start = self.pos;
        let mut depth = 0;
        while let Some(c) = self.peek() {
            self.pos += c.len_utf8();
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                '"' if depth == 0 => return Ok(self.s[start..self.pos - 1].to_string()),
                _ => {}
            }
        }
        Err(format!("unterminated string at byte {start}"))
    }

    /// A field value: parts joined with `#`.
    fn value(&mut self) -> Result<String, String> {
        let mut out = String::new();
        loop {
            self.skip_blank();
            match self.peek() {
                Some('{') => {
                    self.pos += 1;
                    out.push_str(&self.braced()?);
                }
                Some('"') => {
                    self.pos += 1;
                    out.push_str(&self.quoted()?);
                }
                Some(_) => {
                    let n = self.name();
                    if n.is_empty() {
                        return Err(format!("a value expected at byte {}", self.pos));
                    }
                    if n.chars().all(|c| c.is_ascii_digit()) {
                        out.push_str(&n);
                    } else {
                        let key = n.to_lowercase();
                        match self.strings.get(&key) {
                            Some(v) => out.push_str(v),
                            None => match MONTHS.iter().find(|(m, _)| *m == key) {
                                Some((_, full)) => out.push_str(full),
                                None => out.push_str(&n),
                            },
                        }
                    }
                }
                None => return Err("unexpected end".into()),
            }
            self.skip_blank();
            if !self.eat('#') {
                return Ok(out);
            }
        }
    }
}

/// Runs of spaces and line feeds as one space (`[ \n]+`).
fn squeeze(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank = false;
    for c in s.chars() {
        if c == ' ' || c == '\n' {
            if !blank {
                out.push(' ');
            }
            blank = true;
        } else {
            out.push(c);
            blank = false;
        }
    }
    out
}

/// The entries of a BibTeX or BibLaTeX text. Text outside entries is a
/// comment; `@comment` and `@preamble` are skipped, `@string` defines
/// abbreviations.
/// [`parse_bibtex`] going on after a malformed entry, from the next `@`
/// at the start of a line: the entries read and the errors met.
pub fn parse_bibtex_tolerant(text: &str) -> (Vec<Entry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    let mut strings = HashMap::new();
    let mut at = 0;
    while at < text.len() {
        // The next entry, and where the one after starts.
        let next = text[at + 1..]
            .match_indices("\n@")
            .map(|(i, _)| at + 1 + i + 1)
            .next()
            .unwrap_or(text.len());
        let chunk = &text[at..next];
        match parse_bibtex_with(chunk, &mut strings) {
            Ok(e) => entries.extend(e),
            Err(e) => errors.push(e),
        }
        at = next;
    }
    (entries, errors)
}

/// The entries of a hayagriva YAML file, as BibTeX-like fields: `author`
/// and `editor` as `Family, Given` joined with `and`, `title`, `year`.
pub fn parse_yaml(text: &str) -> Result<Vec<Entry>, String> {
    let lib = hayagriva::io::from_yaml_str(text).map_err(|e| e.to_string())?;
    let names = |ps: &[hayagriva::types::Person]| {
        ps.iter()
            .map(|p| {
                let family = match &p.prefix {
                    Some(v) => format!("{v} {}", p.name),
                    None => p.name.clone(),
                };
                match &p.given_name {
                    Some(g) => format!("{family}, {g}"),
                    None => family,
                }
            })
            .collect::<Vec<_>>()
            .join(" and ")
    };
    Ok(lib
        .iter()
        .map(|e| {
            let mut fields = Vec::new();
            if let Some(a) = e.authors() {
                fields.push(("author".to_string(), names(a)));
            }
            if let Some(a) = e.editors() {
                fields.push(("editor".to_string(), names(a)));
            }
            if let Some(t) = e.title() {
                fields.push(("title".to_string(), t.to_string()));
            }
            if let Some(d) = e.date() {
                fields.push(("year".to_string(), d.year.to_string()));
            }
            Entry {
                key: e.key().to_string(),
                kind: format!("{:?}", e.entry_type()).to_lowercase(),
                fields,
            }
        })
        .collect())
}

pub fn parse_bibtex(text: &str) -> Result<Vec<Entry>, String> {
    parse_bibtex_with(text, &mut HashMap::new())
}

/// [`parse_bibtex`] with the `@string` abbreviations defined so far, which
/// it adds to.
fn parse_bibtex_with(
    text: &str,
    strings: &mut HashMap<String, String>,
) -> Result<Vec<Entry>, String> {
    let mut p = Parser {
        s: text,
        pos: 0,
        strings: std::mem::take(strings),
    };
    let out = parse_entries(&mut p);
    *strings = std::mem::take(&mut p.strings);
    out
}

fn parse_entries(p: &mut Parser<'_>) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    while let Some(at) = p.s[p.pos..].find('@') {
        p.pos += at + 1;
        let kind = p.name();
        p.skip_blank();
        let close = if p.eat('{') {
            '}'
        } else if p.eat('(') {
            ')'
        } else {
            continue;
        };
        match kind.to_lowercase().as_str() {
            "comment" => {
                if close == '}' {
                    p.braced()?;
                }
                continue;
            }
            "preamble" => {
                p.value()?;
                p.skip_blank();
                p.eat(close);
                continue;
            }
            "string" => {
                p.skip_blank();
                let name = p.name().to_lowercase();
                p.skip_blank();
                if !p.eat('=') {
                    return Err(format!("`=' expected in @string at byte {}", p.pos));
                }
                let v = p.value()?;
                p.strings.insert(name, v);
                p.skip_blank();
                p.eat(close);
                continue;
            }
            _ => {}
        }
        p.skip_blank();
        let key = p.name();
        let mut fields = Vec::new();
        loop {
            p.skip_blank();
            if p.eat(close) {
                break;
            }
            if !p.eat(',') {
                return Err(format!("`,' expected in entry {key:?} at byte {}", p.pos));
            }
            p.skip_blank();
            if p.eat(close) {
                break;
            }
            let name = p.name().to_lowercase();
            p.skip_blank();
            if !p.eat('=') {
                return Err(format!("`=' expected in entry {key:?} at byte {}", p.pos));
            }
            let v = p.value()?;
            fields.push((name, squeeze(&v)));
        }
        out.push(Entry { key, kind, fields });
    }
    Ok(out)
}

/// The entries of a CSL-JSON text, as `oc-basic` reads them: names as
/// `Family Given` joined with `and`, the year of `issued`.
pub fn parse_csl_json(text: &str) -> Result<Vec<Entry>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let items = v.as_array().ok_or("a CSL-JSON file holds a list")?;
    let mut out = Vec::new();
    for item in items {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let key = obj.get("id").map(json_string).unwrap_or_default();
        let kind = obj.get("type").map(json_string).unwrap_or_default();
        let mut fields = Vec::new();
        for (k, val) in obj {
            match k.as_str() {
                "id" | "type" => {}
                "author" | "editor" => {
                    let names: Vec<String> = val
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|n| {
                                    let f = n.get("family").map(json_string).unwrap_or_default();
                                    let g = n.get("given").map(json_string).unwrap_or_default();
                                    format!("{f} {g}")
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    fields.push((k.clone(), names.join(" and ")));
                }
                "issued" => {
                    let year = val
                        .get("date-parts")
                        .and_then(|d| d.get(0))
                        .and_then(|d| d.get(0))
                        .map(json_string)
                        .or_else(|| {
                            let s = val
                                .get("literal")
                                .or_else(|| val.get("raw"))
                                .map(json_string)?;
                            // The first four digits in a row.
                            let b = s.as_bytes();
                            (0..b.len().saturating_sub(3))
                                .find(|&i| b[i..i + 4].iter().all(u8::is_ascii_digit))
                                .map(|i| s[i..i + 4].to_string())
                        });
                    if let Some(y) = year {
                        fields.push(("year".into(), y));
                    }
                }
                _ => fields.push((k.clone(), json_string(val))),
            }
        }
        out.push(Entry { key, kind, fields });
    }
    Ok(out)
}

fn json_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bibtex() {
        let text = "@string{pub = \"Example Press\"}\n@String{j = {Journal of Tests}}\n% a comment line\n@comment{ignored = {x}}\n@preamble{\"\\newcommand{\\x}{y}\"}\n\n@book{knuth84,\n  author    = {Donald E. Knuth},\n  title     = {The {\\TeX}book},\n  publisher = pub,\n  year      = 1984,\n  month     = jan,\n}\n\n@Article{doe-2020,\n  AUTHOR = \"Jane Doe and Smith, John\",\n  title = {Multi\n     line   title},\n  journal = j # \" (\" # \"Series\" # \")\",\n  date = {2020-05-01},\n  note = {With \"quotes\" and {braces}},\n}\n";
        let e = parse_bibtex(text).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].key, "knuth84");
        assert_eq!(e[0].kind, "book");
        assert_eq!(
            e[0].fields,
            [
                ("author".to_string(), "Donald E. Knuth".to_string()),
                ("title".into(), "The {\\TeX}book".into()),
                ("publisher".into(), "Example Press".into()),
                ("year".into(), "1984".into()),
                ("month".into(), "January".into()),
            ]
        );
        assert_eq!(e[1].kind, "Article");
        assert_eq!(e[1].field("author"), Some("Jane Doe and Smith, John"));
        assert_eq!(e[1].field("title"), Some("Multi line title"));
        assert_eq!(e[1].field("journal"), Some("Journal of Tests (Series)"));
        assert_eq!(e[1].field("note"), Some("With \"quotes\" and {braces}"));
        assert!(parse_bibtex("@book{x, title = {open").is_err());
        // One malformed entry, the others read.
        let (e, errors) = parse_bibtex_tolerant(
            "@book{a, title = {A}}\n@book{b, title {B}}\n@book{c, title = {C}}\n",
        );
        assert_eq!(
            e.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
            ["a", "c"]
        );
        assert_eq!(errors.len(), 1);
        // Abbreviations reach the entries after them.
        let (e, _) =
            parse_bibtex_tolerant("@string{ae = {Addison-Wesley}}\n@book{a, publisher = ae}\n");
        assert_eq!(e[0].field("publisher"), Some("Addison-Wesley"));
        // hayagriva's YAML.
        let y = parse_yaml(
            "knuth:\n  type: book\n  title: The TeXbook\n  author: Knuth, Donald\n  date: 1984\n",
        )
        .unwrap();
        assert_eq!(y[0].key, "knuth");
        assert_eq!(y[0].field("author"), Some("Knuth, Donald"));
        assert_eq!(y[0].field("year"), Some("1984"));
    }

    #[test]
    fn files_in_latin_1_and_utf_16_read() {
        // As the editors read them; `read_to_string` refused them, so the
        // bibliography was "unreadable" and its keys unknown.
        let dir = std::env::temp_dir().join(format!("org-cite-enc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let entry = "@book{m, author = {M\u{fc}ller, J\u{f6}rg}, title = {\u{c7}a\u{11f}}}\n";
        let latin1: Vec<u8> = "@book{m, author = {M\u{fc}ller, J\u{f6}rg}, title = {T}}\n"
            .chars()
            .map(|c| c as u8)
            .collect();
        let mut utf16 = vec![0xff, 0xfe];
        utf16.extend(entry.encode_utf16().flat_map(u16::to_le_bytes));
        for (name, bytes, title) in [
            ("latin1.bib", latin1, "T"),
            ("utf16.bib", utf16, "\u{c7}a\u{11f}"),
            (
                "bom.bib",
                [&[0xef, 0xbb, 0xbf][..], entry.as_bytes()].concat(),
                "\u{c7}a\u{11f}",
            ),
        ] {
            let f = dir.join(name);
            std::fs::write(&f, bytes).unwrap();
            let (e, errors) = read_tolerant(&f).unwrap();
            assert!(errors.is_empty(), "{name}: {errors:?}");
            assert_eq!(e[0].key, "m", "{name}");
            assert_eq!(
                e[0].field("author"),
                Some("M\u{fc}ller, J\u{f6}rg"),
                "{name}"
            );
            assert_eq!(e[0].field("title"), Some(title), "{name}");
            assert_eq!(read(&f).unwrap().len(), 1, "{name}");
        }
        // Windows' line endings: no carriage return in a value.
        let f = dir.join("crlf.bib");
        std::fs::write(&f, "@book{c,\r\n  title = {Multi\r\n Line Title}\r\n}\r\n").unwrap();
        let (e, _) = read_tolerant(&f).unwrap();
        assert!(
            !e[0].field("title").unwrap().contains('\r'),
            "{:?}",
            e[0].field("title")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn csl_json() {
        let text = r#"[{"id": "a", "type": "book", "title": "T", "author": [{"family": "Doe", "given": "Jane"}, {"family": "Roe", "given": "R."}], "issued": {"date-parts": [[2020, 5]]}}, {"id": "b", "issued": {"literal": "circa 1999?"}}]"#;
        let e = parse_csl_json(text).unwrap();
        assert_eq!(e[0].field("author"), Some("Doe Jane and Roe R."));
        assert_eq!(e[0].field("year"), Some("2020"));
        assert_eq!(e[1].field("year"), Some("1999"));
        let mut b = Bibliography::default();
        b.extend(e);
        b.extend(vec![Entry {
            key: "a".into(),
            kind: "x".into(),
            fields: vec![],
        }]);
        assert_eq!(b.get("a").map(|e| e.kind.as_str()), Some("book"));
        assert_eq!(b.entries().len(), 2);
    }
}
