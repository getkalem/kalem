//! BibTeX files as a grid (T2.7h.19): each entry one row (key, type,
//! authors, title, year) away from the cursor, its source where the cursor
//! is; the rows sorted in the view without touching the file; a field set
//! by the smallest edit. The scanner returns ranges into the file, so what
//! the grid does not touch stays byte for byte as written.

use std::ops::Range;

use org_edit::Transaction;

/// A field of an entry: `name = value`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The name.
    pub name: Range<usize>,
    /// The value as written: braces, quotes, `#` concatenations.
    pub value: Range<usize>,
}

/// An entry: `@type{key, fields}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// From the `@` to the closing brace.
    pub range: Range<usize>,
    /// The type, as written (`article`, `Book`).
    pub kind: Range<usize>,
    /// The citation key.
    pub key: Range<usize>,
    /// The fields in order.
    pub fields: Vec<Field>,
    /// The closing brace or parenthesis.
    pub close: usize,
    /// What the scanner could not read in it, which BibTeX reports.
    pub mistakes: Vec<(Range<usize>, Mistake)>,
}

/// Text of an entry the scanner skips, as BibTeX stops at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mistake {
    /// Not a field (`name = value`): skipped to the next comma.
    Unreadable,
    /// No comma between a value and the next field.
    MissingComma,
}

impl Entry {
    /// The field named `name` (case-insensitive).
    pub fn field<'a>(&'a self, text: &str, name: &str) -> Option<&'a Field> {
        self.fields
            .iter()
            .find(|f| text[f.name.clone()].eq_ignore_ascii_case(name))
    }
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// Whether an entry starts at `j`: `@` at the start of a line (where an
/// unbalanced value is taken to end).
fn entry_start(b: &[u8], j: usize) -> bool {
    b[j] == b'@' && j > 0 && b[j - 1] == b'\n'
}

/// Past a braced group opening at `i`.
fn skip_braces(b: &[u8], i: usize) -> usize {
    let mut depth = 0usize;
    let mut j = i;
    while j < b.len() {
        if j > i && entry_start(b, j) {
            return j;
        }
        match b[j] {
            // A backslash escapes nothing: BibTeX counts every brace.
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return j + 1;
                }
            }
            _ => {}
        }
        j += 1;
    }
    b.len()
}

/// Past a quoted string opening at `i` (braces inside protect quotes).
fn skip_quoted(b: &[u8], i: usize) -> usize {
    let mut depth = 0usize;
    let mut j = i + 1;
    while j < b.len() {
        if entry_start(b, j) {
            return j;
        }
        match b[j] {
            // `\"` ends the string too, as in BibTeX (`{\"o}` does not).
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'"' if depth == 0 => return j + 1,
            _ => {}
        }
        j += 1;
    }
    b.len()
}

fn ident_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len()
        && !b[i].is_ascii_whitespace()
        && !matches!(b[i], b'{' | b'}' | b'(' | b')' | b',' | b'=' | b'#' | b'"')
    {
        i += 1;
    }
    i
}

/// The entries of `text`; `@string`, `@preamble` and `@comment` are skipped,
/// as is text outside entries (BibTeX's comments).
pub fn entries(text: &str) -> Vec<Entry> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(at) = text[i..].find('@').map(|k| i + k) {
        let kind = at + 1..ident_end(b, at + 1);
        let mut j = skip_ws(b, kind.end);
        let (open, close) = match b.get(j) {
            Some(b'{') => (b'{', b'}'),
            Some(b'(') => (b'(', b')'),
            _ => {
                i = at + 1;
                continue;
            }
        };
        let name = text[kind.clone()].to_ascii_lowercase();
        if matches!(name.as_str(), "string" | "preamble" | "comment") {
            i = if open == b'{' {
                skip_braces(b, j)
            } else {
                text[j..].find(')').map_or(b.len(), |k| j + k + 1)
            };
            continue;
        }
        j = skip_ws(b, j + 1);
        let key_start = j;
        while j < b.len() && b[j] != b',' && b[j] != close && !b[j].is_ascii_whitespace() {
            j += 1;
        }
        let key = key_start..j;
        let mut fields = Vec::new();
        let mut mistakes = Vec::new();
        let mut end = None;
        // Past what cannot be read: to the next comma or the entry's end
        // at the top level.
        let recover = |mut k: usize| -> usize {
            let mut depth = 0usize;
            while k < b.len() {
                match b[k] {
                    b'{' => depth += 1,
                    b'}' if depth > 0 => depth -= 1,
                    c if depth == 0 && (c == b',' || c == close) => return k,
                    // A new entry at the start of a line.
                    b'@' if depth == 0 && (k == 0 || b[k - 1] == b'\n') => return k,
                    _ => {}
                }
                k += 1;
            }
            k
        };
        let mut next_entry = None;
        loop {
            j = skip_ws(b, j);
            while j < b.len() && b[j] == b',' {
                j = skip_ws(b, j + 1);
            }
            if j >= b.len() {
                break;
            }
            if b[j] == close {
                end = Some(j);
                break;
            }
            // An entry left open: it ends where the next one starts.
            if b[j] == b'@' && (j == 0 || b[j - 1] == b'\n') {
                next_entry = Some(j);
                break;
            }
            let n = j..ident_end(b, j);
            if n.is_empty() {
                // Not a field: skipped.
                let from = j;
                j = recover(j + 1);
                mistakes.push((from..j, Mistake::Unreadable));
                continue;
            }
            j = skip_ws(b, n.end);
            if b.get(j) != Some(&b'=') {
                let from = n.start;
                j = recover(j);
                mistakes.push((from..j, Mistake::Unreadable));
                continue;
            }
            j = skip_ws(b, j + 1);
            let v_start = j;
            let mut v_end = j;
            loop {
                let part_end = match b.get(j) {
                    Some(b'{') => skip_braces(b, j),
                    Some(b'"') => skip_quoted(b, j),
                    Some(_) => ident_end(b, j),
                    None => j,
                };
                if part_end == j {
                    break;
                }
                v_end = part_end;
                j = skip_ws(b, part_end);
                if b.get(j) == Some(&b'#') {
                    j = skip_ws(b, j + 1);
                } else {
                    break;
                }
            }
            fields.push(Field {
                name: n,
                value: v_start..v_end,
            });
            j = v_end;
            // What follows a value: a comma, the entry's end, or the next
            // entry; another field (`name =`) there lacks its comma.
            let next = skip_ws(b, j);
            let name_end = ident_end(b, next);
            if v_end > v_start && name_end > next && b.get(skip_ws(b, name_end)) == Some(&b'=') {
                mistakes.push((v_end..next, Mistake::MissingComma));
            }
        }
        let Some(close_at) = end else {
            // An entry left open: what was read is kept, to the next entry
            // or the end.
            let stop = next_entry.unwrap_or(b.len());
            let last = text[..stop].trim_end().len().max(at + 1);
            // A value left open runs on past the entry's last text (to
            // the next entry, or the end): it ends with the entry.
            for f in &mut fields {
                f.value.end = f.value.end.min(last);
                f.value.start = f.value.start.min(f.value.end);
                f.name.end = f.name.end.min(last);
                f.name.start = f.name.start.min(f.name.end);
            }
            mistakes.retain(|(r, _): &(Range<usize>, Mistake)| r.start < last);
            out.push(Entry {
                range: at..last,
                kind,
                key,
                fields,
                close: last,
                mistakes,
            });
            match next_entry {
                Some(n) => {
                    i = n;
                    continue;
                }
                None => break,
            }
        };
        out.push(Entry {
            range: at..close_at + 1,
            kind,
            key,
            fields,
            close: close_at,
            mistakes,
        });
        i = close_at + 1;
    }
    out
}

/// A value as a reader reads it: outer braces or quotes off, TeX's
/// accents and escapes as characters, blanks collapsed.
pub fn plain(value: &str) -> String {
    let v = value.trim();
    let v = if (v.starts_with('{') && v.ends_with('}')) || (v.starts_with('"') && v.ends_with('"'))
    {
        &v[1..v.len() - 1]
    } else {
        v
    };
    let mut out = String::new();
    let mut chars = v.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '~' => out.push(' '),
            // A formula: its Unicode approximation (`$\alpha$-helix`).
            '$' => {
                let formula: String = chars.by_ref().take_while(|&c| c != '$').collect();
                out.push_str(&crate::math::unicode(&format!("${formula}$")));
            }
            '\\' => {
                let Some(&n) = chars.peek() else { break };
                if crate::latex_view::accent_mark(&n.to_string()).is_some()
                    && !(n.is_ascii_alphabetic()
                        && chars
                            .clone()
                            .nth(1)
                            .is_some_and(|c| c.is_ascii_alphabetic()))
                {
                    chars.next();
                    // `\c c`, `\c{c}`: the letter after blanks or a brace.
                    while chars
                        .peek()
                        .is_some_and(|x| *x == '{' || (n.is_alphabetic() && *x == ' '))
                    {
                        chars.next();
                    }
                    let Some(mut l) = chars.next() else { break };
                    // Accents on an accented letter (Vietnamese
                    // `Nguy{\~{\^e}}n`): the inner one's marks first.
                    let mut marks = vec![n];
                    while l == '\\'
                        && let Some(&m) = chars.peek()
                        && m != 'i'
                        && crate::latex_view::accent_mark(&m.to_string()).is_some()
                    {
                        chars.next();
                        marks.insert(0, m);
                        while chars.peek() == Some(&'{') {
                            chars.next();
                        }
                        match chars.next() {
                            Some(next) => l = next,
                            None => break,
                        }
                    }
                    // `\'{\i}`: the dotless i carries the accent (and TeX
                    // eats the blanks after `\i`).
                    if l == '\\' && chars.peek() == Some(&'i') {
                        chars.next();
                        l = 'i';
                        while chars.peek() == Some(&' ') {
                            chars.next();
                        }
                    }
                    out.push_str(&accents(&marks, l));
                } else if n.is_ascii_alphabetic() {
                    // A command: its name is dropped (`\textit`), the
                    // special letters kept.
                    let mut name = String::new();
                    while chars.peek().is_some_and(char::is_ascii_alphabetic) {
                        name.push(chars.next().unwrap_or(' '));
                    }
                    // TeX eats the blanks after a control word
                    // (`Stra\ss e` is Straße).
                    while chars.peek() == Some(&' ') {
                        chars.next();
                    }
                    match name.as_str() {
                        "TeX" | "LaTeX" | "BibTeX" | "LaTeXe" => out.push_str(&name),
                        n => {
                            if let Some(w) = crate::latex_view::word(n) {
                                out.push_str(w);
                            }
                        }
                    }
                } else {
                    chars.next();
                    out.push(n);
                }
            }
            c => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `letter` with the accents `marks` (innermost first), composed as far
/// as Unicode has the characters.
fn accents(marks: &[char], letter: char) -> String {
    use unicode_normalization::UnicodeNormalization;
    let mut s = String::from(letter);
    for m in marks {
        match crate::latex_view::accent_mark(&m.to_string()) {
            Some(c) => s.push(c),
            None => return letter.to_string(),
        }
    }
    s.nfc().collect()
}

/// The authors as a grid shows them: family names as BibTeX reads them,
/// "and" between two, "et al." after the first of more.
pub fn short_authors(authors: &str) -> String {
    crate::bibstyle::surnames(authors)
}

/// The fields BibTeX's standard styles require of an entry type: each
/// item is one field, or alternatives (`author|editor`); biblatex's names
/// (`date` for `year`, `journaltitle` for `journal`) count too.
fn required(kind: &str) -> &'static [&'static str] {
    match kind {
        "article" => &["author", "title", "journal|journaltitle", "year|date"],
        "book" => &["author|editor", "title", "publisher", "year|date"],
        "inbook" => &[
            "author|editor",
            "title",
            "chapter|pages",
            "publisher",
            "year|date",
        ],
        "incollection" => &["author", "title", "booktitle", "publisher", "year|date"],
        "inproceedings" | "conference" => &["author", "title", "booktitle", "year|date"],
        "mastersthesis" | "phdthesis" | "thesis" => {
            &["author", "title", "school|institution", "year|date"]
        }
        "techreport" | "report" => &["author", "title", "institution", "year|date"],
        "proceedings" => &["title", "year|date"],
        "unpublished" => &["author", "title", "note"],
        "manual" => &["title"],
        "booklet" => &["title"],
        _ => &[],
    }
}

/// The fields biblatex requires, for an entry written for it (with a
/// `date`, a `journaltitle`, an `@inbook` with its `booktitle`): no
/// publisher, `booktitle` for a part of a book.
fn required_biblatex(kind: &str) -> &'static [&'static str] {
    match kind {
        "article" => &["author", "title", "journal|journaltitle", "year|date"],
        "book" | "mvbook" | "collection" | "proceedings" | "manual" | "online" => {
            &["title", "year|date"]
        }
        "inbook" | "incollection" | "inproceedings" | "conference" => {
            &["author", "title", "booktitle", "year|date"]
        }
        "thesis" | "mastersthesis" | "phdthesis" => {
            &["author", "title", "school|institution", "year|date"]
        }
        "report" | "techreport" => &["author", "title", "institution", "year|date"],
        "unpublished" => &["author", "title", "year|date"],
        _ => &[],
    }
}

/// The entry types BibTeX's standard styles and biblatex know.
const KNOWN_TYPES: &[&str] = &[
    "article",
    "book",
    "booklet",
    "conference",
    "inbook",
    "incollection",
    "inproceedings",
    "manual",
    "mastersthesis",
    "misc",
    "phdthesis",
    "proceedings",
    "techreport",
    "unpublished",
    // biblatex's.
    "bookinbook",
    "collection",
    "dataset",
    "electronic",
    "inreference",
    "mvbook",
    "mvcollection",
    "mvproceedings",
    "mvreference",
    "online",
    "patent",
    "periodical",
    "reference",
    "report",
    "set",
    "software",
    "suppbook",
    "suppcollection",
    "suppperiodical",
    "thesis",
    "www",
    "xdata",
    "artwork",
    "audio",
    "bibnote",
    "commentary",
    "image",
    "jurisdiction",
    "legislation",
    "legal",
    "letter",
    "movie",
    "music",
    "performance",
    "review",
    "standard",
    "video",
    "unknown",
];

/// The month abbreviations BibTeX defines.
const MONTHS: &[&str] = &[
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// The bare parts of a value (macro names and numbers), not its braced or
/// quoted ones: `tug # { 1}` has `tug`.
fn bare_parts(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut quoted, mut start) = (0i32, false, 0);
    for (i, c) in value.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            '"' if depth == 0 => quoted = !quoted,
            '#' if depth == 0 && !quoted => {
                parts.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&value[start..]);
    parts
        .into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty() && !p.starts_with(['{', '"']))
        .collect()
}

/// The problems of a BibTeX file, in text order: an entry not closed
/// before the next one, an entry without a key, a key two entries use,
/// and a field the entry's type requires missing (publish_todo 3.4).
pub fn problems(text: &str) -> Vec<crate::modes::ModeDiagnostic> {
    use crate::modes::ModeDiagnostic;
    let mut out = Vec::new();
    let mut seen: std::collections::HashMap<String, Range<usize>> =
        std::collections::HashMap::new();
    let all = entries(text);
    let strings = strings(text);
    let by_key: std::collections::HashMap<String, &Entry> = all
        .iter()
        .map(|e| (text[e.key.clone()].trim().to_lowercase(), e))
        .collect();
    for e in &all {
        let at = e.range.start..e.kind.end;
        let closer = text.as_bytes().get(e.close).copied();
        if !matches!(closer, Some(b'}' | b')')) {
            out.push(ModeDiagnostic {
                range: at.clone(),
                code: "bibtex-unclosed".into(),
                message: crate::l10n::tr("bibtex-unclosed"),
            });
        }
        let key = text[e.key.clone()].trim();
        if key.is_empty() {
            out.push(ModeDiagnostic {
                range: at.clone(),
                code: "bibtex-no-key".into(),
                message: crate::l10n::tr("bibtex-no-key"),
            });
        } else if seen.insert(key.to_lowercase(), e.key.clone()).is_some() {
            out.push(ModeDiagnostic {
                range: e.key.clone(),
                code: "bibtex-duplicate-key".into(),
                message: crate::tr!("bibtex-duplicate-key", key = key),
            });
        }
        // What BibTeX stops at: text that is not a field, a missing comma.
        for (r, m) in &e.mistakes {
            let (code, key) = match m {
                Mistake::Unreadable => ("bibtex-syntax", "bibtex-syntax"),
                Mistake::MissingComma => ("bibtex-missing-comma", "bibtex-missing-comma"),
            };
            out.push(ModeDiagnostic {
                range: if r.is_empty() {
                    r.start..r.start + 1
                } else {
                    r.clone()
                },
                code: code.into(),
                message: crate::l10n::tr(key),
            });
        }
        // A field given twice: BibTeX keeps the first.
        let mut names: Vec<String> = Vec::new();
        for f in &e.fields {
            let n = text[f.name.clone()].to_ascii_lowercase();
            if names.contains(&n) {
                out.push(ModeDiagnostic {
                    range: f.name.clone(),
                    code: "bibtex-duplicate-field".into(),
                    message: crate::tr!("bibtex-duplicate-field", field = n.as_str()),
                });
            } else {
                names.push(n);
            }
            // An abbreviation no `@string` defines.
            for part in bare_parts(&text[f.value.clone()]) {
                let k = part.to_ascii_lowercase();
                if !k.chars().all(|c| c.is_ascii_digit())
                    && !strings.contains_key(&k)
                    && !MONTHS.contains(&k.as_str())
                {
                    out.push(ModeDiagnostic {
                        range: f.value.clone(),
                        code: "bibtex-undefined-string".into(),
                        message: crate::tr!("bibtex-undefined-string", name = part),
                    });
                }
            }
        }
        let kind = text[e.kind.clone()].to_ascii_lowercase();
        if !KNOWN_TYPES.contains(&kind.as_str()) {
            out.push(ModeDiagnostic {
                range: e.kind.clone(),
                code: "bibtex-unknown-type".into(),
                message: crate::tr!("bibtex-unknown-type", kind = kind.as_str()),
            });
        }
        // A `crossref`'s entry gives the fields this one lacks, as BibTeX
        // reads it; one in another file is not known here.
        let parent = match e.field(text, "crossref") {
            Some(f) => {
                let key = text[f.value.clone()]
                    .trim_matches(['{', '}', '"'])
                    .trim()
                    .to_lowercase();
                match by_key.get(&key) {
                    Some(p) => Some(*p),
                    None => continue,
                }
            }
            None => None,
        };
        // An empty value (`author = {}`, as New Entry writes it) is as
        // good as none.
        let given = |en: &Entry, f: &str| {
            en.field(text, f).is_some_and(|v| {
                !text[v.value.clone()]
                    .trim_matches(|c: char| c == '{' || c == '}' || c == '"' || c.is_whitespace())
                    .is_empty()
            })
        };
        let has = |f: &str| given(e, f) || parent.is_some_and(|p| given(p, f));
        let biblatex = ["date", "journaltitle", "location", "maintitle"]
            .iter()
            .any(|f| has(f))
            || (kind == "inbook" && has("booktitle"));
        let needs = if biblatex {
            required_biblatex(&kind)
        } else {
            required(&kind)
        };
        for need in needs {
            if !need.split('|').any(has) {
                out.push(ModeDiagnostic {
                    range: at.clone(),
                    code: "bibtex-missing-field".into(),
                    message: crate::tr!(
                        "bibtex-missing-field",
                        kind = kind.as_str(),
                        field = need.replace('|', " / ")
                    ),
                });
            }
        }
    }
    out
}

/// BibTeX's checks as a language pack: what `kalem check` runs on a
/// `.bib` file and the status bar says, when no plugin serves BibTeX.
#[derive(Debug)]
pub struct Pack;

impl crate::packs::LanguagePack for Pack {
    fn id(&self) -> &'static str {
        "bibtex"
    }

    fn languages(&self) -> &[&str] {
        &["bib"]
    }

    fn diagnostics(&self, text: &str) -> Vec<crate::modes::ModeDiagnostic> {
        problems(text)
    }
}

/// Whether `doc` is a BibTeX file.
pub fn is_bib(doc: &crate::DocumentState) -> bool {
    matches!(&doc.meta.mode, crate::DocumentMode::Text { language: Some(l) } if l.eq_ignore_ascii_case("bib"))
}

/// The grid's columns.
pub const COLUMNS: [&str; 5] = ["key", "type", "author", "title", "year"];

/// The widest each column is laid out, in characters.
const MAX: [usize; 5] = [24, 13, 24, 48, 4];

/// The `@string` abbreviations of `text`, by lower-case name.
pub fn strings(text: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("@string") {
        let at = from + i + "@string".len();
        from = at;
        let rest = text[at..].trim_start();
        let open = at + (text[at..].len() - rest.len());
        let close = match rest.chars().next() {
            Some('{') => '}',
            Some('(') => ')',
            _ => continue,
        };
        // The matching close, outside braces.
        let mut depth = 0i32;
        let mut end = None;
        for (j, c) in text[open + 1..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' if depth > 0 => depth -= 1,
                c if c == close && depth == 0 => {
                    end = Some(open + 1 + j);
                    break;
                }
                _ => {}
            }
        }
        let Some(end) = end else { continue };
        if let Some((name, value)) = text[open + 1..end].split_once('=') {
            let v = expand(value.trim(), &out);
            // The text, without the braces or quotes around it.
            let inner = v
                .strip_prefix(['{', '"'])
                .and_then(|x| x.strip_suffix(['}', '"']))
                .unwrap_or(&v)
                .to_string();
            out.insert(name.trim().to_ascii_lowercase(), inner);
        }
        from = end;
    }
    out
}

/// A field's value with its `@string` abbreviations, month names and `#`
/// concatenations written out (a braced or quoted part as it is).
pub fn expand(value: &str, strings: &std::collections::HashMap<String, String>) -> String {
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
    // The parts between `#` outside braces and quotes.
    let mut parts = Vec::new();
    let (mut depth, mut quoted, mut start) = (0i32, false, 0);
    for (i, c) in value.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            '"' if depth == 0 => quoted = !quoted,
            '#' if depth == 0 && !quoted => {
                parts.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&value[start..]);
    if parts.len() == 1 && (value.starts_with('{') || value.starts_with('"')) {
        return value.to_string();
    }
    let mut out = String::new();
    for p in parts {
        let p = p.trim();
        if (p.starts_with('{') && p.ends_with('}')) || (p.starts_with('"') && p.ends_with('"')) {
            out.push_str(&p[1..p.len() - 1]);
        } else if p.chars().all(|c| c.is_ascii_digit()) {
            out.push_str(p);
        } else {
            let k = p.to_ascii_lowercase();
            match strings.get(&k) {
                Some(v) => out.push_str(v),
                None => match MONTHS.iter().find(|(m, _)| *m == k) {
                    Some((_, m)) => out.push_str(m),
                    None => out.push_str(p),
                },
            }
        }
    }
    format!("{{{out}}}")
}

/// An entry's cells, as the grid shows them.
pub fn cells(text: &str, e: &Entry) -> [String; 5] {
    cells_with(text, e, &strings(text))
}

/// [`cells`] with the file's `@string` abbreviations.
pub fn cells_with(
    text: &str,
    e: &Entry,
    strings: &std::collections::HashMap<String, String>,
) -> [String; 5] {
    let get = |n: &str| {
        e.field(text, n)
            .map(|f| plain(&expand(&text[f.value.clone()], strings)))
            .unwrap_or_default()
    };
    // The names as BibTeX reads them, before their braces go.
    let raw = |n: &str| {
        e.field(text, n)
            .map(|f| {
                let v = expand(&text[f.value.clone()], strings);
                let inner = v
                    .strip_prefix('{')
                    .and_then(|v| v.strip_suffix('}'))
                    .or_else(|| v.strip_prefix('"').and_then(|v| v.strip_suffix('"')));
                inner.map_or_else(|| v.clone(), str::to_string)
            })
            .unwrap_or_default()
    };
    let author = {
        let a = raw("author");
        if a.trim().is_empty() {
            raw("editor")
        } else {
            a
        }
    };
    let year = {
        let y = get("year");
        if y.is_empty() {
            get("date").chars().take(4).collect()
        } else {
            y
        }
    };
    [
        text[e.key.clone()].to_string(),
        text[e.kind.clone()].to_ascii_lowercase(),
        crate::bibstyle::surnames(&author),
        get("title"),
        year,
    ]
}

/// The grid of a BibTeX document: its entries, their cells, the column
/// widths.
#[derive(Debug)]
pub struct Grid {
    /// The entries in file order.
    pub entries: Vec<Entry>,
    /// Their cells.
    pub cells: Vec<[String; 5]>,
    /// The columns' widths, in characters.
    pub widths: [usize; 5],
}

/// The last grid: the document (its serial: two documents both start at
/// version 0) and its version, and the grid.
type GridMemo = ((u64, u64), std::rc::Rc<Grid>);

thread_local! {
    static GRID: std::cell::RefCell<Option<GridMemo>> =
        const { std::cell::RefCell::new(None) };
}

/// The grid of the BibTeX document `doc`, for its text version.
pub fn grid(doc: &crate::DocumentState) -> std::rc::Rc<Grid> {
    use unicode_width::UnicodeWidthStr;
    // The document and its version: two documents both start at 0.
    let key = (doc.serial(), doc.version());
    GRID.with(|g| {
        if let Some((k, v)) = &*g.borrow()
            && *k == key
        {
            return v.clone();
        }
        let text = doc.text().as_str();
        let entries = entries(text);
        let abbreviations = strings(text);
        let cells: Vec<[String; 5]> = entries
            .iter()
            .map(|e| cells_with(text, e, &abbreviations))
            .collect();
        let mut widths = [0; 5];
        for c in &cells {
            for (i, s) in c.iter().enumerate() {
                widths[i] = widths[i].max(s.width().min(MAX[i]));
            }
        }
        let v = std::rc::Rc::new(Grid {
            entries,
            cells,
            widths,
        });
        *g.borrow_mut() = Some((key, v.clone()));
        v
    })
}

/// `s` cut to `width` columns (with `…`) and padded to it.
fn fit(s: &str, width: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    if s.width() <= width {
        return format!("{s}{}", " ".repeat(width - s.width()));
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw + 1 > width {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    w += 1;
    format!("{out}{}", " ".repeat(width.saturating_sub(w)))
}

/// The entry holding byte `pos`.
fn entry_at(grid: &Grid, pos: usize) -> Option<usize> {
    let i = grid.entries.partition_point(|e| e.range.end <= pos);
    grid.entries
        .get(i)
        .filter(|e| e.range.start <= pos && pos <= e.range.end)
        .map(|_| i)
}

/// A line of a BibTeX document: an entry's first line away from the cursor
/// as its row of the grid; other lines as they are.
pub fn line_view(
    doc: &crate::DocumentState,
    line: Range<usize>,
    cursor: Option<usize>,
) -> crate::view::LineView {
    use crate::view::{LineView, Run, Style};
    let text = doc.text().as_str();
    let g = grid(doc);
    let row = g
        .entries
        .iter()
        .position(|e| line.start <= e.range.start && e.range.start < line.end.max(line.start + 1));
    let Some(i) = row.filter(|&i| {
        let e = &g.entries[i];
        cursor.is_none_or(|c| !(e.range.start <= c && c <= e.range.end))
            && text[line.start..e.range.start].trim().is_empty()
    }) else {
        let mut v = crate::view::plain_line_view(text, line, cursor);
        v.mono = true;
        return v;
    };
    let e = &g.entries[i];
    let mut runs = Vec::new();
    let bar = |at: usize| Run {
        src: at..at,
        text: " │ ".into(),
        verbatim: false,
        style: Style {
            dim: true,
            ..Style::default()
        },
        widget: None,
    };
    for (c, s) in g.cells[i].iter().enumerate() {
        if c > 0 {
            runs.push(bar(e.key.end.min(line.end)));
        }
        let (src, style) = if c == 0 {
            // The key stands for the line: a click there opens the entry.
            (
                line.start..line.end,
                Style {
                    bold: true,
                    ..Style::default()
                },
            )
        } else {
            (
                line.end..line.end,
                Style {
                    italic: c == 3,
                    dim: c == 1,
                    ..Style::default()
                },
            )
        };
        let shown = if c + 1 == COLUMNS.len() {
            s.clone()
        } else {
            fit(s, g.widths[c])
        };
        runs.push(Run {
            src,
            text: shown,
            verbatim: false,
            style,
            widget: None,
        });
    }
    LineView {
        runs,
        range: line,
        mono: true,
        ..LineView::default()
    }
}

/// The lines a BibTeX document shows as its grid: each entry's first line,
/// all the lines of the entry holding the cursor, and the text between
/// entries that is not blank (comments, `@string`s), in the order of the
/// view's sort (`DocumentState::bib_sort`).
pub fn shown_lines(doc: &crate::DocumentState) -> Option<std::rc::Rc<Vec<usize>>> {
    if !is_bib(doc) {
        return None;
    }
    let g = grid(doc);
    let t = doc.text();
    let text = t.as_str();
    let cursor = doc.selection.head.min(text.len());
    let open = entry_at(&g, cursor);
    // The lines outside entries: shown unless blank.
    let mut in_entry = vec![false; t.line_count()];
    for e in &g.entries {
        let first = t.line_of(e.range.start);
        let last = t.line_of(e.range.end.saturating_sub(1).max(e.range.start));
        for l in first..=last.min(in_entry.len().saturating_sub(1)) {
            in_entry[l] = true;
        }
    }
    let mut order: Vec<usize> = (0..g.entries.len()).collect();
    if let Some((col, reverse)) = doc.bib_sort {
        let key = |i: &usize| {
            g.cells[*i]
                .get(col)
                .cloned()
                .unwrap_or_default()
                .to_lowercase()
        };
        order.sort_by_key(key);
        if reverse {
            order.reverse();
        }
    }
    let mut out: Vec<usize> = (0..in_entry.len())
        .filter(|&l| !in_entry[l] && !text[t.line_range(l)].trim().is_empty())
        .collect();
    for i in order {
        let e = &g.entries[i];
        let first = t.line_of(e.range.start);
        if open == Some(i) {
            let last = t.line_of(e.range.end.saturating_sub(1).max(e.range.start));
            out.extend(first..=last);
        } else {
            out.push(first);
        }
    }
    if doc.bib_sort.is_none() {
        out.sort_unstable();
        out.dedup();
    }
    if out.is_empty() {
        out.push(t.line_of(cursor));
    }
    Some(std::rc::Rc::new(out))
}

/// One replacement as a transaction.
fn one(range: Range<usize>, insert: String) -> Transaction {
    let mut tx = Transaction::new("Set Field");
    let _ = tx.replace(range, insert);
    tx
}

/// The edit that sets field `name` of entry `e` to `value` (braced), the
/// smallest one: the value replaced where the field is, else a line added
/// before the closing brace in the indentation of the other fields; an
/// empty value removes the field.
pub fn set_field(text: &str, e: &Entry, name: &str, value: &str) -> Transaction {
    let braced = format!("{{{value}}}");
    if let Some(f) = e.field(text, name) {
        if value.is_empty() {
            // The field's line, or the field and its comma.
            let start = text[..f.name.start]
                .rfind('\n')
                .map_or(f.name.start, |n| n + 1);
            let mut end = f.value.end;
            let rest = text.get(end..e.close).unwrap_or("");
            let comma = rest.find(',').filter(|k| rest[..*k].trim().is_empty());
            if let Some(k) = comma {
                end += k + 1;
            }
            let line_start_blank = text[start..f.name.start].trim().is_empty();
            let (from, to) = if line_start_blank && text[end..].starts_with('\n') {
                (start, end + 1)
            } else {
                (f.name.start, end)
            };
            return one(from..to, String::new());
        }
        return one(f.value.clone(), braced);
    }
    if value.is_empty() {
        return Transaction::new("Set Field");
    }
    let indent = e
        .fields
        .first()
        .map(|f| {
            let ls = text[..f.name.start].rfind('\n').map_or(0, |n| n + 1);
            let lead = &text[ls..f.name.start];
            if lead.trim().is_empty() {
                lead.to_string()
            } else {
                "  ".into()
            }
        })
        .unwrap_or_else(|| "  ".into());
    // After the last field (its comma added when it has none), on a line of
    // its own.
    let after = e.fields.last().map_or(e.key.end, |f| f.value.end);
    let rest = text.get(after..e.close).unwrap_or("");
    let has_comma = rest.trim_start().starts_with(',');
    let at = if has_comma {
        after + rest.find(',').map_or(0, |k| k + 1)
    } else {
        after
    };
    let comma = if has_comma { "" } else { "," };
    one(at..at, format!("{comma}\n{indent}{name} = {braced}"))
}

/// The entry at `pos` of the BibTeX document `doc`.
pub fn entry_at_cursor(doc: &crate::DocumentState) -> Option<Entry> {
    let g = grid(doc);
    let pos = doc.selection.head.min(doc.text().len());
    entry_at(&g, pos).map(|i| g.entries[i].clone())
}

/// The column named `name`, or the one whose field the cursor is in.
pub fn column(doc: &crate::DocumentState, name: Option<&str>) -> usize {
    if let Some(n) = name {
        return COLUMNS
            .iter()
            .position(|c| c.eq_ignore_ascii_case(n))
            .unwrap_or(0);
    }
    let text = doc.text().as_str();
    let pos = doc.selection.head;
    entry_at_cursor(doc)
        .and_then(|e| {
            if e.key.start <= pos && pos <= e.key.end {
                return Some(0);
            }
            let f = e
                .fields
                .iter()
                .find(|f| f.name.start <= pos && pos <= f.value.end)?;
            let n = text[f.name.clone()].to_ascii_lowercase();
            COLUMNS
                .iter()
                .position(|c| *c == n || (*c == "author" && n == "editor"))
        })
        .unwrap_or(0)
}

/// The status bar's words for a BibTeX document: how many entries, and
/// the sort.
pub fn status(doc: &crate::DocumentState) -> Option<String> {
    if !is_bib(doc) {
        return None;
    }
    let g = grid(doc);
    let n = g.entries.len();
    Some(match doc.bib_sort {
        Some((c, rev)) => crate::tr!(
            "status-bib-sorted",
            count = n as i64,
            column = COLUMNS[c.min(4)],
            order = if rev { "desc" } else { "asc" }
        ),
        None => crate::tr!("status-bib", count = n as i64),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_bib_files_problems() {
        let text = "@article{knuth84,\n  author = {Knuth},\n  title = {Literate Programming},\n  year = 1984,\n}\n@book{knuth84,\n  editor = {X},\n  title = {T},\n  publisher = {P},\n  date = {2020},\n}\n@article{open,\n  title = {Unclosed {brace},\n@misc{,\n  note = {n}\n}\n";
        let p = super::problems(text);
        let codes: Vec<&str> = p.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(
            codes,
            [
                // knuth84: an article without its journal.
                "bibtex-missing-field",
                // The book reuses the key; editor and date stand in.
                "bibtex-duplicate-key",
                // open: not closed, and neither author, journal nor year.
                "bibtex-unclosed",
                "bibtex-missing-field",
                "bibtex-missing-field",
                "bibtex-missing-field",
                // An entry without a key.
                "bibtex-no-key",
            ],
            "{p:#?}"
        );
        assert!(p[0].message.contains("journal"), "{}", p[0].message);
        // `kalem check` and the status bar get them through the packs.
        let pack = crate::packs::for_language("bib").expect("BibTeX's pack");
        assert_eq!(pack.diagnostics(text).len(), p.len());
        // What BibTeX stops at or warns about.
        let codes =
            |t: &str| -> Vec<String> { super::problems(t).into_iter().map(|d| d.code).collect() };
        let one = |fields: &str| codes(&format!("@misc{{k,\n{fields}\n}}\n"));
        assert_eq!(
            one("  title = {A},\n  note = tug,"),
            ["bibtex-undefined-string"]
        );
        assert_eq!(
            one("  title = {A}\n  note = {B},"),
            ["bibtex-missing-comma"]
        );
        assert_eq!(one("  title {A},"), ["bibtex-syntax"]);
        assert_eq!(
            one("  title = {A},\n  Title = {B},"),
            ["bibtex-duplicate-field"]
        );
        assert_eq!(
            one("  author = \"Erwin Schr\\\"odinger\","),
            ["bibtex-syntax"]
        );
        assert!(one("  author = \"Erwin Schr{\\\"o}dinger\", month = jan,").is_empty());
        assert_eq!(
            codes("@artcle{k,\n  title = {A}\n}\n"),
            ["bibtex-unknown-type"]
        );
        // An empty required field is a missing one; a crossref's entry
        // gives what the entry lacks; biblatex's entries need no
        // publisher.
        assert_eq!(
            codes(
                "@article{k,\n  author = {},\n  title = {T},\n  journal = {J},\n  year = 2000\n}\n"
            ),
            ["bibtex-missing-field"]
        );
        assert!(codes("@inproceedings{a,\n  author = {A},\n  title = {T},\n  crossref = {P}\n}\n@proceedings{p,\n  title = {P},\n  booktitle = {P},\n  year = 2020\n}\n").is_empty());
        assert!(
            codes(
                "@inproceedings{a,\n  author = {A},\n  title = {T},\n  crossref = {elsewhere}\n}\n"
            )
            .is_empty()
        );
        assert!(
            codes("@book{b,\n  author = {A},\n  title = {T},\n  date = {2020}\n}\n").is_empty()
        );
    }

    use super::*;

    const BIB: &str = "% My references\n@string{tug = \"TUGboat\"}\n\n@book{knuth84,\n  author = {Donald E. Knuth},\n  title = {The {\\TeX}book},\n  year = 1984,\n}\n\n@Article{lamport,\n  author = \"Lamport, Leslie and Mittelbach, Frank and Goossens, Michel\",\n  title = {G{\\\"o}del and {\\c C}ay},\n  journal = tug # { 1},\n  year = {1994}\n}\n";

    fn doc(text: &str) -> crate::DocumentState {
        let meta = crate::Metadata {
            path: Some("refs.bib".into()),
            mode: crate::DocumentMode::Text {
                language: Some("bib".into()),
            },
            line_ending: crate::LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        crate::DocumentState::new(
            text,
            meta,
            std::sync::Arc::new(org_model::Settings::default()),
        )
    }

    #[test]
    fn scans_entries() {
        let es = entries(BIB);
        assert_eq!(es.len(), 2);
        assert_eq!(&BIB[es[0].key.clone()], "knuth84");
        assert_eq!(&BIB[es[1].kind.clone()], "Article");
        let j = es[1].field(BIB, "JOURNAL").unwrap();
        assert_eq!(&BIB[j.value.clone()], "tug # { 1}");
        assert_eq!(
            cells(BIB, &es[1]),
            [
                "lamport".to_string(),
                "article".into(),
                "Lamport et al.".into(),
                "Gödel and Çay".into(),
                "1994".into()
            ]
        );
        assert_eq!(cells(BIB, &es[0])[3], "The TeXbook");
        assert_eq!(cells(BIB, &es[0])[2], "Knuth");
    }

    #[test]
    fn values_read_as_tex_prints_them() {
        for (value, shown) in [
            ("{Nguy{\\~{\\^e}}n}", "Nguyễn"),
            ("{$\\alpha$-helix}", "α-helix"),
            ("{Stra\\ss e}", "Straße"),
            ("{na\\\"\\i ve}", "naïve"),
            ("{G\\\"{o}del}", "Gödel"),
            ("{\\c{C}ay}", "Çay"),
        ] {
            assert_eq!(plain(value), shown, "{value}");
        }
    }

    #[test]
    fn abbreviations_expanded() {
        let st = strings(BIB);
        assert_eq!(st.get("tug").map(String::as_str), Some("TUGboat"));
        assert_eq!(expand("tug # { 1}", &st), "{TUGboat 1}");
        assert_eq!(expand("jan", &st), "{January}");
        assert_eq!(expand("{Plain}", &st), "{Plain}");
        let text = "@string{me = {Kalem Team}}\n@misc{a,\n  author = me,\n  year = 2026\n}\n";
        let e = entries(text);
        // The author column shows family names: `me` expanded to "Kalem
        // Team" is Team.
        assert_eq!(cells(text, &e[0])[2], "Team");
        // Names as BibTeX reads them: a von part, a corporate name in
        // braces, `others`, an upper-case `AND`.
        for (author, shown) in [
            ("Jan van den Berg", "van den Berg"),
            ("van den Berg, Jan", "van den Berg"),
            ("{World Health Organization}", "World Health Organization"),
            (
                "{National Aeronautics and Space Administration}",
                "National Aeronautics and Space Administration",
            ),
            ("Knuth, Donald and others", "Knuth et al."),
            ("Alpha, A. AND Beta, B.", "Alpha and Beta"),
        ] {
            let text = format!("@misc{{a,\n  author = {{{author}}}\n}}\n");
            let e = entries(&text);
            assert_eq!(cells(&text, &e[0])[2], shown, "{author}");
        }
    }

    #[test]
    fn grid_rows_and_sorting() {
        let mut d = doc(BIB);
        d.selection = org_edit::Selection::caret(0);
        let lines = shown_lines(&d).unwrap();
        // The comment, the `@string`, and each entry's first line.
        assert_eq!(*lines, vec![0, 1, 3, 9]);
        let row = line_view(&d, d.text().line_range(3), Some(0)).display();
        assert!(row.starts_with("knuth84 │ book    │ Knuth "), "{row}");
        assert!(row.ends_with("│ 1984"), "{row}");
        // The cursor in an entry: all its lines, as source.
        d.selection = org_edit::Selection::caret(BIB.find("Lamport,").unwrap());
        let lines = shown_lines(&d).unwrap();
        assert_eq!(*lines, vec![0, 1, 3, 9, 10, 11, 12, 13, 14]);
        let src = line_view(&d, d.text().line_range(9), Some(d.selection.head)).display();
        assert_eq!(src, "@Article{lamport,");
        // Sorted by year, descending.
        d.selection = org_edit::Selection::caret(0);
        d.bib_sort = Some((4, true));
        let lines = shown_lines(&d).unwrap();
        assert_eq!(*lines, vec![0, 1, 9, 3]);
    }

    #[test]
    fn malformed_entries_do_not_swallow_the_rest() {
        let bib = "@book{a,\n  author = {A},\n  junk here,\n  year = 2000,\n}\n\n@article{b,\n  title = {Open\n\n@misc{c,\n  title = {C}\n}\n";
        let es = entries(bib);
        let keys: Vec<&str> = es.iter().map(|e| &bib[e.key.clone()]).collect();
        assert_eq!(keys, ["a", "b", "c"]);
        // The bad field skipped, the good ones read.
        assert!(es[0].field(bib, "year").is_some());
        assert_eq!(cells(bib, &es[2])[3], "C");
        // Set Field on the entry left open, as while typing it: no field
        // runs past the entry.
        let open = &es[1];
        assert!(open.fields.iter().all(|f| f.value.end <= open.close));
        for (name, value) in [("year", "2020"), ("title", ""), ("title", "T")] {
            let tx = set_field(bib, open, name, value);
            let mut s = bib.to_string();
            for c in tx.edits.iter().rev() {
                s.replace_range(c.range.clone(), &c.insert);
            }
            assert!(s.contains("@misc{c,"), "{s}");
        }
        let end = "@article{z,\n  title = {Open";
        let es = entries(end);
        let _ = set_field(end, &es[0], "year", "2020");
        let _ = set_field(end, &es[0], "title", "");
    }

    #[test]
    fn sets_fields() {
        let es = entries(BIB);
        let apply = |tx: Transaction| {
            let mut s = BIB.to_string();
            for c in tx.edits.iter().rev() {
                s.replace_range(c.range.clone(), &c.insert);
            }
            s
        };
        // A value replaced in place.
        let s = apply(set_field(BIB, &es[0], "year", "1986"));
        assert!(s.contains("  year = {1986},\n}"), "{s}");
        // A new field after the last, the comma added.
        let s = apply(set_field(BIB, &es[1], "doi", "10.1/x"));
        assert!(s.contains("  year = {1994},\n  doi = {10.1/x}\n}"), "{s}");
        // An empty value removes the field's line.
        let s = apply(set_field(BIB, &es[0], "year", ""));
        assert!(s.contains("  title = {The {\\TeX}book},\n}"), "{s}");
    }
}
