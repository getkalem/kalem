//! CSV mode's model (§2.6.2, T2.7d): the dialect of a file (delimiter,
//! quote, header row, line endings), found from its text and kept; the
//! records with the byte range of every field, found lazily from the top;
//! and edits that change only the fields they touch, quoting only where
//! RFC 4180 needs it, as transactions on the text (so undo, saving and the
//! other views work as for any document).
//!
//! The scanner is Kalem's own rather than the `csv` crate's reader: edits
//! need the byte range of each field in the source, quotes included, which
//! a reader that yields unquoted values does not give.

use std::borrow::Cow;
use std::ops::Range;

use org_edit::{Selection, Transaction};

/// How a CSV file writes its records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dialect {
    /// The field delimiter: `,`, `;`, a tab or `|`.
    pub delimiter: u8,
    /// The quote character.
    pub quote: u8,
    /// Whether the first record names the columns.
    pub header: bool,
    /// Records end with CR LF.
    pub crlf: bool,
}

impl Default for Dialect {
    fn default() -> Self {
        Dialect {
            delimiter: b',',
            quote: b'"',
            header: true,
            crlf: false,
        }
    }
}

impl Dialect {
    /// The delimiter as a character.
    pub fn delimiter_char(&self) -> char {
        self.delimiter as char
    }

    /// The line ending records get.
    pub fn line_ending(&self) -> &'static str {
        if self.crlf { "\r\n" } else { "\n" }
    }
}

/// A field in the text: its source range (quotes included) and whether it
/// is quoted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The bytes of the field in the text.
    pub range: Range<usize>,
    /// Written in quotes.
    pub quoted: bool,
}

/// A record in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Its fields.
    pub fields: Vec<Field>,
    /// Its bytes, without the line ending.
    pub range: Range<usize>,
    /// Where the next record starts.
    pub next: usize,
}

/// Whether a record ends at `i`: a line feed, or a carriage return
/// before one. A carriage return alone is data, as lines split only at
/// line feeds (a file of carriage returns alone is read with them turned
/// into line feeds).
fn line_end(b: &[u8], i: usize) -> bool {
    b[i] == b'\n' || (b[i] == b'\r' && b.get(i + 1) == Some(&b'\n'))
}

/// The record starting at `start`.
pub fn scan(text: &str, start: usize, d: &Dialect) -> Record {
    let b = text.as_bytes();
    let mut fields = Vec::new();
    let mut i = start;
    loop {
        let fs = i;
        let quoted = b.get(i) == Some(&d.quote);
        if quoted {
            i += 1;
            while i < b.len() {
                if b[i] == d.quote {
                    if b.get(i + 1) == Some(&d.quote) {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            // Anything after the closing quote belongs to the field.
            while i < b.len() && b[i] != d.delimiter && !line_end(b, i) {
                i += 1;
            }
        } else {
            while i < b.len() && b[i] != d.delimiter && !line_end(b, i) {
                i += 1;
            }
        }
        fields.push(Field {
            range: fs..i,
            quoted,
        });
        if i < b.len() && b[i] == d.delimiter {
            i += 1;
            continue;
        }
        let end = i;
        let next = if b.get(i) == Some(&b'\r') && b.get(i + 1) == Some(&b'\n') {
            i + 2
        } else if i < b.len() {
            i + 1
        } else {
            i
        };
        return Record {
            fields,
            range: start..end,
            next,
        };
    }
}

/// The value of a field: unquoted, doubled quotes as one.
pub fn value<'a>(text: &'a str, f: &Field, d: &Dialect) -> Cow<'a, str> {
    let s = &text[f.range.clone()];
    if !f.quoted {
        return Cow::Borrowed(s);
    }
    let q = d.quote as char;
    let inner = s.strip_prefix(q).unwrap_or(s);
    // The closing quote is the first one not doubled, as the scanner reads
    // it; what follows is kept as written (`"a"b"c` is `ab"c`).
    let close = closing_quote(inner.as_bytes(), d.quote).unwrap_or(inner.len());
    let (body, rest) = (&inner[..close], &inner[(close + 1).min(inner.len())..]);
    let dq = format!("{q}{q}");
    Cow::Owned(format!("{}{rest}", body.replace(&dq, &q.to_string())))
}

/// The offset of the quote closing a quoted field's contents `b` (after
/// its opening quote): the first quote not followed by another.
fn closing_quote(b: &[u8], quote: u8) -> Option<usize> {
    let mut i = 0;
    while i < b.len() {
        if b[i] == quote {
            if b.get(i + 1) == Some(&quote) {
                i += 2;
                continue;
            }
            return Some(i);
        }
        i += 1;
    }
    None
}

/// The problems of one record (see [`problems`]).
pub fn record_problems(text: &str, r: &Record, d: &Dialect) -> Vec<Problem> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    for f in &r.fields {
        let bytes = &b[f.range.clone()];
        if f.quoted {
            match closing_quote(&bytes[1..], d.quote) {
                None => out.push(Problem {
                    range: f.range.start..f.range.start + 1,
                    code: "csv-unterminated-quote",
                    message: crate::l10n::tr("csv-unterminated-quote"),
                }),
                Some(c) if c + 2 < bytes.len() => out.push(Problem {
                    range: f.range.start + c + 2..f.range.end,
                    code: "csv-text-after-quote",
                    message: crate::l10n::tr("csv-text-after-quote"),
                }),
                Some(_) => {}
            }
        } else if let Some(i) = bytes.iter().position(|&c| c == d.quote) {
            out.push(Problem {
                range: f.range.start + i..f.range.start + i + 1,
                code: "csv-bare-quote",
                message: crate::l10n::tr("csv-bare-quote"),
            });
        }
    }
    out
}

/// What is wrong with a CSV file, read leniently by [`scan`]: its byte
/// range, a code and a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Where.
    pub range: Range<usize>,
    /// `csv-unterminated-quote`, `csv-text-after-quote` or `csv-bare-quote`.
    pub code: &'static str,
    /// What, in the interface language.
    pub message: String,
}

/// The malformed fields of `text`: a quoted field with no closing quote
/// (it runs to the end of the file), text after a closing quote, and a
/// quote inside an unquoted field. At most `limit` problems.
pub fn problems(text: &str, d: &Dialect, limit: usize) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut at = match sep_line(text) {
        Some((_, skip)) => skip,
        None => 0,
    };
    while at < text.len() && out.len() < limit {
        let r = scan(text, at, d);
        out.extend(record_problems(text, &r, d));
        if r.next <= at {
            break;
        }
        at = r.next;
    }
    out.truncate(limit);
    out
}

/// `value` as a field: quoted when it has the delimiter, the quote or a
/// line break, or blanks at its ends.
pub fn encode(value: &str, d: &Dialect) -> String {
    let q = d.quote as char;
    let needs = value.contains(d.delimiter_char())
        || value.contains(q)
        || value.contains(['\n', '\r'])
        || value.starts_with([' ', '\t'])
        || value.ends_with([' ', '\t']);
    if needs {
        format!("{q}{}{q}", value.replace(q, &format!("{q}{q}")))
    } else {
        value.to_string()
    }
}

/// Text typed at `pos` in a field of record `rec`, as the grid types it:
/// the value gets the characters, so a delimiter, a quote or a line break
/// typed in a field quotes it (quotes inside doubled), rather than
/// splitting it. `None` when the text needs no quoting (typed as it is),
/// or `pos` is outside the field's quotes.
pub fn typed(
    text: &str,
    rec: &Record,
    pos: usize,
    typed: &str,
    d: &Dialect,
) -> Option<Transaction> {
    let q = d.quote as char;
    let qq = format!("{q}{q}");
    if !(typed.contains(d.delimiter_char()) || typed.contains(q) || typed.contains(['\n', '\r'])) {
        return None;
    }
    let f = rec
        .fields
        .iter()
        .find(|f| f.range.start <= pos && pos <= f.range.end)?;
    let mut tx = Transaction::new("Typing");
    let caret = if f.quoted {
        // Between the quotes: the characters, quotes doubled.
        if pos <= f.range.start || pos >= f.range.end {
            return None;
        }
        let ins = typed.replace(q, &qq);
        tx.replace(pos..pos, &ins).ok()?;
        pos + ins.len()
    } else {
        let raw = &text[f.range.clone()];
        let rel = pos - f.range.start;
        let before = format!("{}{typed}", &raw[..rel]).replace(q, &qq);
        let after = raw[rel..].replace(q, &qq);
        tx.replace(f.range.clone(), format!("{q}{before}{after}{q}"))
            .ok()?;
        f.range.start + 1 + before.len()
    };
    Some(tx.select(Selection::caret(caret)))
}

/// Finds the dialect of `text` from its first records: the delimiter that
/// gives the most records with the same number of fields (more than one),
/// a header when the first record's values are all text where later ones
/// have numbers, or all different and not empty.
/// Excel's first line naming the delimiter, `sep=;`: the delimiter and
/// the line's length with its line ending.
pub fn sep_line(text: &str) -> Option<(u8, usize)> {
    let rest = text.strip_prefix("sep=")?;
    let d = *rest.as_bytes().first()?;
    let after = &rest[1..];
    let ending = if after.starts_with("\r\n") {
        2
    } else if after.starts_with('\n') {
        1
    } else if after.is_empty() {
        0
    } else {
        return None;
    };
    Some((d, 4 + 1 + ending))
}

/// Whether a field reads as a number, with `.` or `,` as the decimal
/// separator.
fn numeric(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty() && (s.parse::<f64>().is_ok() || s.replace(',', ".").parse::<f64>().is_ok())
}

pub fn detect(text: &str) -> Dialect {
    if let Some((delimiter, skip)) = sep_line(text) {
        let mut d = Dialect {
            delimiter,
            crlf: text.contains("\r\n"),
            ..Dialect::default()
        };
        d.header = looks_like_header(&text[skip..], &d);
        return d;
    }
    let sample_end = text
        .char_indices()
        .map(|(i, _)| i)
        .nth(64 * 1024)
        .unwrap_or(text.len());
    let sample = &text[..sample_end];
    let crlf = sample.contains("\r\n");
    // Double quotes, unless fields are quoted with single quotes and none
    // with double ones (`'a, b','c'`).
    let (key, delimiter) = best_delimiter(sample, crlf, b'"');
    let mut quote = b'"';
    if quoted_fields(sample, crlf, delimiter, b'"') == 0 {
        let (key1, delimiter1) = best_delimiter(sample, crlf, b'\'');
        if key1.0 >= key.0 && quoted_fields(sample, crlf, delimiter1, b'\'') > 0 {
            quote = b'\'';
            let mut d = Dialect {
                delimiter: delimiter1,
                quote,
                crlf,
                ..Dialect::default()
            };
            d.header = looks_like_header(sample, &d);
            return d;
        }
    }
    let mut d = Dialect {
        delimiter,
        quote,
        crlf,
        ..Dialect::default()
    };
    d.header = looks_like_header(sample, &d);
    d
}

/// How many of the first records' fields are quoted with `quote` in
/// `sample` read with `delimiter`.
fn quoted_fields(sample: &str, crlf: bool, delimiter: u8, quote: u8) -> usize {
    let d = Dialect {
        delimiter,
        quote,
        crlf,
        ..Dialect::default()
    };
    let mut n = 0;
    let mut at = 0;
    for _ in 0..50 {
        let r = scan(sample, at, &d);
        if r.next == at {
            break;
        }
        n += r.fields.iter().filter(|f| f.quoted).count();
        at = r.next;
    }
    n
}

type DelimiterKey = (
    std::cmp::Reverse<usize>,
    usize,
    usize,
    std::cmp::Reverse<usize>,
    usize,
);

/// The delimiter for `sample` with `quote` as its quote, and how well it
/// splits it.
fn best_delimiter(sample: &str, crlf: bool, quote: u8) -> (DelimiterKey, u8) {
    // The delimiter that reads the fewest fields as malformed, then that
    // splits the most records into the same number of
    // fields (more than one), a first line of one field (a title) left
    // aside; on a tie, the one whose fields read as numbers more often
    // (`1,5;2,5` splits at `;`), then `,` `;` tab `|` in that order.
    let mut best = (
        (
            std::cmp::Reverse(usize::MAX),
            0usize,
            0usize,
            std::cmp::Reverse(0usize),
            0usize,
        ),
        b',',
    );
    for delim in *b",;\t|" {
        let d = Dialect {
            delimiter: delim,
            quote,
            crlf,
            ..Dialect::default()
        };
        let mut counts = Vec::new();
        // Malformed fields read so: a wrong delimiter puts quotes inside
        // fields (`a\t"b,c"` read at tabs is fine, at commas is not).
        let mut malformed = 0;
        let mut at = 0;
        while at < sample.len() && counts.len() < 50 {
            let r = scan(sample, at, &d);
            if r.next == at {
                break;
            }
            malformed += record_problems(sample, &r, &d).len();
            if !(r.range.is_empty() && r.next >= sample.len()) {
                counts.push(r.fields.len());
            }
            at = r.next;
        }
        let Some(&first) = counts.first() else {
            continue;
        };
        // The most common count among the records.
        let mut freq: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        for &n in &counts {
            *freq.entry(n).or_insert(0) += 1;
        }
        let (modal, same) = freq
            .iter()
            .filter(|(n, _)| **n > 1)
            .max_by_key(|(n, f)| (**f, **n))
            .map_or((first, 0), |(n, f)| (*n, *f));
        if modal < 2 {
            continue;
        }
        // Fields that read as numbers, in the first records, and fields
        // holding another delimiter that are not numbers (`elma;1` split
        // at the comma).
        let mut numbers = 0;
        let mut mixed = 0;
        let mut at = 0;
        for _ in 0..counts.len().min(20) {
            let r = scan(sample, at, &d);
            if r.next == at {
                break;
            }
            for f in &r.fields {
                let v = value(sample, f, &d);
                if numeric(&v) {
                    numbers += 1;
                } else if v.bytes().any(|b| b != delim && b",;\t|".contains(&b)) {
                    mixed += 1;
                }
            }
            at = r.next;
        }
        let key = (
            std::cmp::Reverse(malformed),
            same,
            numbers,
            std::cmp::Reverse(mixed),
            modal,
        );
        if key > best.0 {
            best = (key, delim);
        }
    }
    best
}

fn looks_like_header(text: &str, d: &Dialect) -> bool {
    let first = scan(text, 0, d);
    if first.next >= text.len() {
        return false;
    }
    let names: Vec<Cow<'_, str>> = first.fields.iter().map(|f| value(text, f, d)).collect();
    // The reader of the statistics and of sorting.
    let number = |s: &str| number(s, d.delimiter == b';').is_some();
    if names.iter().any(|n| number(n)) {
        return false;
    }
    // Later records with numbers where the first has text: a header.
    let mut at = first.next;
    for _ in 0..20 {
        if at >= text.len() {
            break;
        }
        let r = scan(text, at, d);
        if r.fields.iter().any(|f| number(&value(text, f, d))) {
            return true;
        }
        at = r.next;
    }
    let mut seen = std::collections::HashSet::new();
    names
        .iter()
        .all(|n| !n.trim().is_empty() && seen.insert(n.to_string()))
}

/// The starts of the records of a text, found as far as asked (a large
/// file is not scanned whole to show its first screen).
#[derive(Debug, Clone, Default)]
pub struct Index {
    starts: Vec<usize>,
    complete: bool,
    len: usize,
}

impl Index {
    /// An index of `text`, nothing scanned yet; Excel's `sep=;` first
    /// line is not a record.
    pub fn new(text: &str) -> Index {
        let skip = sep_line(text).map_or(0, |(_, n)| n);
        Index {
            starts: vec![skip],
            complete: skip >= text.len(),
            len: text.len(),
        }
    }

    /// Scans until record `row` is known (or the end).
    pub fn ensure(&mut self, text: &str, row: usize, d: &Dialect) {
        while !self.complete && self.starts.len() <= row + 1 {
            let at = *self.starts.last().expect("a start");
            let r = scan(text, at, d);
            if r.next >= text.len() || r.next == at {
                self.complete = true;
                // A final line feed does not start an empty record.
                if r.next > at && r.next < text.len() {
                    self.starts.push(r.next);
                }
            } else {
                self.starts.push(r.next);
            }
        }
    }

    /// The number of records, scanning the whole text.
    pub fn count(&mut self, text: &str, d: &Dialect) -> usize {
        self.ensure(text, usize::MAX - 1, d);
        self.starts.len()
    }

    /// Record `row`, if the text has it.
    pub fn record(&mut self, text: &str, row: usize, d: &Dialect) -> Option<Record> {
        self.ensure(text, row, d);
        let at = *self.starts.get(row)?;
        (at < text.len() || (row == 0 && text.is_empty())).then(|| scan(text, at, d))
    }

    /// The record holding byte `pos`, and its row.
    pub fn row_at(&mut self, text: &str, pos: usize, d: &Dialect) -> usize {
        while !self.complete && *self.starts.last().expect("a start") <= pos {
            let n = self.starts.len();
            self.ensure(text, n, d);
        }
        self.starts.partition_point(|&s| s <= pos).saturating_sub(1)
    }

    /// Whether the index was made for a text of `len` bytes.
    pub fn fits(&self, len: usize) -> bool {
        self.len == len
    }
}

/// All records of `text` as values, for tests and conversions.
pub fn rows(text: &str, d: &Dialect) -> Vec<Vec<String>> {
    let mut idx = Index::new(text);
    let n = idx.count(text, d);
    (0..n)
        .filter_map(|i| idx.record(text, i, d))
        .map(|r| {
            r.fields
                .iter()
                .map(|f| value(text, f, d).into_owned())
                .collect()
        })
        .collect()
}

/// Sets field `col` of the record at `rec` to `v`: only that field's bytes
/// change; a record with fewer fields gets empty ones first.
pub fn set_cell(_text: &str, rec: &Record, col: usize, v: &str, d: &Dialect) -> Transaction {
    let enc = encode(v, d);
    let mut tx = Transaction::new("Edit Cell");
    let caret = match rec.fields.get(col) {
        Some(f) => {
            tx.replace(f.range.clone(), &enc).expect("one edit");
            f.range.start + enc.len()
        }
        None => {
            let pad = d
                .delimiter_char()
                .to_string()
                .repeat(col + 1 - rec.fields.len());
            let at = rec.range.end;
            tx.replace(at..at, format!("{pad}{enc}")).expect("one edit");
            at + pad.len() + enc.len()
        }
    };
    tx.select(Selection::caret(caret))
}

/// A new empty record of `columns` fields after the record `rec`.
pub fn insert_row(text: &str, rec: &Record, columns: usize, d: &Dialect) -> Transaction {
    let blank = d
        .delimiter_char()
        .to_string()
        .repeat(columns.saturating_sub(1));
    let mut tx = Transaction::new("Insert Row");
    let at = rec.range.end;
    let nl = if text[at..].starts_with("\r\n") || (d.crlf && at >= text.len()) {
        "\r\n"
    } else {
        "\n"
    };
    tx.replace(at..at, format!("{nl}{blank}"))
        .expect("one edit");
    tx.select(Selection::caret(at + nl.len()))
}

/// Deletes the record `rec`, its line ending with it.
pub fn delete_row(text: &str, rec: &Record) -> Transaction {
    let mut tx = Transaction::new("Delete Row");
    let r = if rec.next > rec.range.end || rec.range.start == 0 {
        rec.range.start..rec.next
    } else {
        // The last record: the line ending before it goes.
        let before = if text[..rec.range.start].ends_with("\r\n") {
            2
        } else {
            1
        };
        rec.range.start.saturating_sub(before)..rec.range.end
    };
    tx.replace(r.clone(), "").expect("one edit");
    tx.select(Selection::caret(r.start.min(text.len() - r.len())))
}

/// Swaps two records, `a` before `b`, keeping each one's bytes.
pub fn swap_rows(text: &str, a: &Record, b: &Record) -> Transaction {
    let mut tx = Transaction::new("Move Row");
    let (ta, tb) = (&text[a.range.clone()], &text[b.range.clone()]);
    tx.replace(a.range.clone(), tb).expect("apart");
    tx.replace(b.range.clone(), ta).expect("apart");
    let shift = tb.len() as isize - ta.len() as isize;
    tx.select(Selection::caret((b.range.start as isize + shift) as usize))
}

/// Inserts an empty column before column `col` in every record (`col`
/// past the last: after it).
pub fn insert_column(text: &str, d: &Dialect, col: usize) -> Transaction {
    let mut tx = Transaction::new("Insert Column");
    let delim = d.delimiter_char().to_string();
    let mut idx = Index::new(text);
    let n = idx.count(text, d);
    for i in 0..n {
        let Some(r) = idx.record(text, i, d) else {
            continue;
        };
        // A blank line stays blank.
        if r.range.is_empty() {
            continue;
        }
        match r.fields.get(col) {
            Some(f) => {
                let _ = tx.replace(f.range.start..f.range.start, delim.clone());
            }
            None => {
                let _ = tx.replace(
                    r.range.end..r.range.end,
                    delim.repeat(col + 1 - r.fields.len()),
                );
            }
        }
    }
    tx
}

/// Deletes column `col` from every record.
pub fn delete_column(text: &str, d: &Dialect, col: usize) -> Transaction {
    let mut tx = Transaction::new("Delete Column");
    let mut idx = Index::new(text);
    let n = idx.count(text, d);
    for i in 0..n {
        let Some(r) = idx.record(text, i, d) else {
            continue;
        };
        let Some(f) = r.fields.get(col) else { continue };
        // The field with the delimiter after it (before it, for the last).
        let range = if col + 1 < r.fields.len() {
            f.range.start..r.fields[col + 1].range.start
        } else if col > 0 {
            r.fields[col - 1].range.end..f.range.end
        } else {
            f.range.clone()
        };
        let _ = tx.replace(range, "");
    }
    tx
}

/// Swaps columns `a` and `a + 1` in every record, keeping each field's
/// bytes.
pub fn swap_columns(text: &str, d: &Dialect, a: usize) -> Transaction {
    let mut tx = Transaction::new("Move Column");
    let mut idx = Index::new(text);
    let n = idx.count(text, d);
    for i in 0..n {
        let Some(r) = idx.record(text, i, d) else {
            continue;
        };
        let (Some(x), Some(y)) = (r.fields.get(a), r.fields.get(a + 1)) else {
            continue;
        };
        let _ = tx.replace(x.range.clone(), &text[y.range.clone()]);
        let _ = tx.replace(y.range.clone(), &text[x.range.clone()]);
    }
    tx
}

/// How rows compare for sorting by a column: as numbers when both are.
/// The order of two values in a column: numbers (read as the column
/// statistics read them) before text, numbers by value, text without
/// case. A total order, as sorting needs, whatever the column mixes.
pub(crate) fn compare(a: &str, b: &str, comma_decimal: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (number(a, comma_decimal), number(b, comma_decimal)) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.to_lowercase().cmp(&b.to_lowercase()),
    }
}

/// The order of the data rows (the header stays first) sorted by column
/// `col` (`reverse`: descending), for a view that sorts without changing
/// the file.
pub fn sorted_order(text: &str, d: &Dialect, col: usize, reverse: bool) -> Vec<usize> {
    let rows = rows(text, d);
    let first = usize::from(d.header);
    // Blank lines go last, in file order, whichever the direction.
    let blank = |i: usize| rows[i].len() == 1 && rows[i][0].is_empty();
    let (mut order, blanks): (Vec<usize>, Vec<usize>) =
        (first..rows.len()).partition(|&i| !blank(i));
    let key = |i: usize| rows[i].get(col).map_or("", String::as_str);
    order.sort_by(|&a, &b| {
        let o = compare(key(a), key(b), d.delimiter == b';');
        if reverse { o.reverse() } else { o }
    });
    let mut out: Vec<usize> = (0..first).collect();
    out.extend(order);
    out.extend(blanks);
    out
}

/// Sort File: the records rewritten in the order of column `col` (the
/// header stays first), each record's bytes kept.
pub fn sort_file(text: &str, d: &Dialect, col: usize, reverse: bool) -> Transaction {
    let order = sorted_order(text, d, col, reverse);
    let mut idx = Index::new(text);
    let n = idx.count(text, d);
    let recs: Vec<Record> = (0..n).filter_map(|i| idx.record(text, i, d)).collect();
    let body: Vec<&str> = order
        .iter()
        .map(|&i| &text[recs[i].range.clone()])
        .collect();
    let nl = d.line_ending();
    // From the first record: a `sep=` line stays.
    let start = recs.first().map_or(0, |r| r.range.start);
    let end = recs.last().map_or(0, |r| r.range.end);
    let mut tx = Transaction::new("Sort File");
    tx.replace(start..end, body.join(nl)).expect("one edit");
    tx
}

/// Rows as tab-separated values, the way spreadsheets copy them.
pub fn to_tsv(rows: &[Vec<String>]) -> String {
    let d = Dialect {
        delimiter: b'\t',
        ..Dialect::default()
    };
    rows.iter()
        .map(|r| {
            r.iter()
                .map(|v| encode(v, &d))
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The rows as an Org table (Convert to Org Table), a rule under the
/// header, aligned as Org aligns tables.
pub fn to_org_table(text: &str, d: &Dialect) -> String {
    let rows = rows(text, d);
    let mut lines: Vec<Option<Vec<String>>> = rows
        .into_iter()
        .map(|r| {
            Some(
                r.into_iter()
                    .map(|v| crate::paste::field_text(&v))
                    .collect(),
            )
        })
        .collect();
    if d.header && lines.len() > 1 {
        lines.insert(1, None);
    }
    let t = lines
        .iter()
        .map(|r| match r {
            Some(cells) => format!("| {} |", cells.join(" | ")),
            None => "|-".to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    crate::paste::align_tables(&format!("{t}\n"))
}

/// Numbers of column `col` of the data rows: count, sum, average, min, max.
/// A number as spreadsheets write it: `1234.5`, `1,234.5` (English),
/// `1.234,5` (Turkish and most of Europe), `1 234,5`; a comma alone is
/// the decimal point unless it groups thousands (`1,234,567`, or `1,234`
/// outside files that `;` delimits, as European spreadsheets write), and
/// in files that `;` delimits a dot alone before three digits groups them
/// (`1.234` is one thousand…). Not numbers: dates, version numbers, phone
/// numbers (spaces that do not group thousands, `+90 …`) and integers
/// with leading zeros (identifiers such as `05321234567`).
pub(crate) fn number(v: &str, comma_decimal: bool) -> Option<f64> {
    let t = v.trim();
    // Spaces group thousands or they are not a number's.
    let spaced: Vec<&str> = t.split([' ', '\u{a0}', '\u{202f}']).collect();
    if spaced.len() > 1 {
        let head = spaced[0].trim_start_matches(['-', '+']);
        let ok = !head.is_empty()
            && head.len() <= 3
            && head.bytes().all(|b| b.is_ascii_digit())
            && spaced[1..spaced.len() - 1]
                .iter()
                .all(|g| g.len() == 3 && g.bytes().all(|b| b.is_ascii_digit()))
            && spaced.last().is_some_and(|g| {
                g.len() >= 3
                    && g[..3].bytes().all(|b| b.is_ascii_digit())
                    && (g.len() == 3 || matches!(g.as_bytes()[3], b'.' | b','))
            });
        if !ok {
            return None;
        }
    }
    let v = t.replace(['\u{a0}', '\u{202f}', ' ', '\''], "");
    if v.is_empty() {
        return None;
    }
    let digits = v.trim_start_matches(['-', '+']);
    if digits.len() > 1 && digits.starts_with('0') && digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let grouped = |s: &str, sep: char| {
        let s = s.trim_start_matches(['-', '+']);
        let mut parts = s.split(sep);
        let first = parts.next().unwrap_or("");
        !first.is_empty()
            && first.len() <= 3
            && first.bytes().all(|b| b.is_ascii_digit())
            && parts.clone().count() >= 1
            && parts.all(|p| p.len() == 3 && p.bytes().all(|b| b.is_ascii_digit()))
    };
    let (comma, dot) = (v.rfind(','), v.rfind('.'));
    let normal = match (comma, dot) {
        // Both: the last one is the decimal point.
        (Some(c), Some(d)) if c > d => v.replace('.', "").replace(',', "."),
        (Some(_), Some(_)) => v.replace(',', ""),
        (Some(_), None) => {
            if grouped(&v, ',') && (v.matches(',').count() > 1 || !comma_decimal) {
                v.replace(',', "")
            } else {
                v.replace(',', ".")
            }
        }
        (None, Some(_)) if v.matches('.').count() > 1 && grouped(&v, '.') => v.replace('.', ""),
        (None, Some(_)) if comma_decimal && grouped(&v, '.') => v.replace('.', ""),
        _ => v,
    };
    normal.parse::<f64>().ok().filter(|x| x.is_finite())
}

pub fn column_stats(text: &str, d: &Dialect, col: usize) -> Option<(usize, f64, f64, f64, f64)> {
    let rows = rows(text, d);
    let nums: Vec<f64> = rows
        .iter()
        .skip(usize::from(d.header))
        .filter_map(|r| r.get(col))
        .filter_map(|v| number(v, d.delimiter == b';'))
        .collect();
    if nums.is_empty() {
        return None;
    }
    let sum: f64 = nums.iter().sum();
    let min = nums.iter().copied().fold(f64::INFINITY, f64::min);
    let max = nums.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Some((nums.len(), sum, sum / nums.len() as f64, min, max))
}

/// How the grid shows a CSV document (view state, never written to the
/// file; T2.7d.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct View {
    /// Columns of numbers and dates aligned right, as spreadsheets do.
    pub align_numbers: bool,
    /// Each column its color.
    pub rainbow: bool,
    /// Row numbers in the gutter and column letters on the first row, as
    /// `C-c }` shows them in an Org table.
    pub coordinates: bool,
    /// As a spreadsheet looks: row numbers in a shaded gutter, the column
    /// letters in a bar above the grid ([`letters_bar`]), the cell at the
    /// cursor marked, its row number and column letter too.
    pub sheet: bool,
}

/// The columns of a CSV document as the grid shows them (view state,
/// never written to the file; T2.7d.9).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Columns {
    /// Columns not shown.
    pub hidden: std::collections::BTreeSet<usize>,
    /// Widths set by hand (Autosize, Widen, Narrow, Column Width), in
    /// characters; a longer value shows cut, with `…`, but whole in the
    /// cell at the cursor.
    pub widths: std::collections::BTreeMap<usize, usize>,
    /// The first shown column stays at the left edge when the rows scroll
    /// sideways, as a frozen pane does in a spreadsheet.
    pub frozen: bool,
}

/// The view new CSV documents start with: the settings `csv.align_numbers`,
/// `csv.rainbow` and `csv.coordinates`.
static VIEW_DEFAULTS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(1 | 8);

/// Sets the view new CSV documents start with.
pub fn set_view_defaults(v: View) {
    let bits = u8::from(v.align_numbers)
        | u8::from(v.rainbow) << 1
        | u8::from(v.coordinates) << 2
        | u8::from(v.sheet) << 3;
    VIEW_DEFAULTS.store(bits, std::sync::atomic::Ordering::Relaxed);
}

impl Default for View {
    fn default() -> View {
        let bits = VIEW_DEFAULTS.load(std::sync::atomic::Ordering::Relaxed);
        View {
            align_numbers: bits & 1 != 0,
            rainbow: bits & 2 != 0,
            coordinates: bits & 4 != 0,
            sheet: bits & 8 != 0,
        }
    }
}

/// The values of column `col` (the header row left out) with how many
/// rows hold each, the most frequent first, then in text order.
pub fn frequencies(text: &str, d: &Dialect, col: usize) -> Vec<(String, usize)> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (i, row) in rows(text, d).into_iter().enumerate() {
        if i == 0 && d.header {
            continue;
        }
        let v = row.get(col).cloned().unwrap_or_default();
        *counts.entry(v).or_default() += 1;
    }
    let mut v: Vec<(String, usize)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

/// One range of a histogram: values from `low` up to `high` (`high`
/// itself only in the last range), how many rows hold one, and the first
/// such row (counted with the header).
#[derive(Debug, Clone, PartialEq)]
pub struct Bin {
    pub low: f64,
    pub high: f64,
    pub count: usize,
    pub first: usize,
}

/// The numbers of column `col` (the header left out) in ranges of a round
/// width (1, 2 or 5 times a power of ten), about as many ranges as
/// Sturges' rule gives, at most 20; empty when the column holds no
/// numbers. Fields that are not numbers are left out.
pub fn histogram(text: &str, d: &Dialect, col: usize) -> Vec<Bin> {
    let comma = d.delimiter == b';';
    let values: Vec<(usize, f64)> = rows(text, d)
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !(*i == 0 && d.header))
        .filter_map(|(i, row)| {
            number(row.get(col)?, comma)
                .filter(|v| v.is_finite())
                .map(|v| (i, v))
        })
        .collect();
    let Some(min) = values.iter().map(|v| v.1).reduce(f64::min) else {
        return Vec::new();
    };
    let max = values.iter().map(|v| v.1).fold(min, f64::max);
    let wanted = ((values.len() as f64).log2().ceil() as usize + 1).clamp(1, 20);
    let width = round_width((max - min) / wanted as f64);
    let low = (min / width).floor() * width;
    let n = (((max - low) / width).floor() as usize + 1).min(64);
    let mut bins: Vec<Bin> = (0..n)
        .map(|k| Bin {
            low: low + k as f64 * width,
            high: low + (k + 1) as f64 * width,
            count: 0,
            first: usize::MAX,
        })
        .collect();
    for (row, v) in values {
        let k = (((v - low) / width).floor().max(0.0) as usize).min(n - 1);
        bins[k].count += 1;
        bins[k].first = bins[k].first.min(row);
    }
    bins
}

/// The round width (1, 2 or 5 times a power of ten) at or above `w`; 1 for
/// a column of one value.
fn round_width(w: f64) -> f64 {
    if w.is_nan() || w <= 0.0 || !w.is_finite() {
        return 1.0;
    }
    let p = 10f64.powf(w.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * p)
        .find(|&r| r >= w * (1.0 - 1e-9))
        .unwrap_or(10.0 * p)
}

/// A histogram range's bounds as text: `10 – 20`, without the rounding
/// noise of floating point.
pub fn bin_label(b: &Bin) -> String {
    let f = |v: f64| {
        let s = format!("{:.10}", v);
        let s = s.trim_end_matches('0').trim_end_matches('.');
        if s == "-0" {
            "0".to_string()
        } else {
            s.to_string()
        }
    };
    format!("{} – {}", f(b.low), f(b.high))
}

/// A bar of `n` out of `max` in at most `width` cells, for the frequency
/// table's histogram.
pub fn bar(n: usize, max: usize, width: usize) -> String {
    let cells = if max == 0 {
        0
    } else {
        (n * width).div_ceil(max)
    };
    "█".repeat(cells)
}

/// Each field of column `col` (the header left out) with `find` replaced
/// by `replace`: the edits, and how many fields changed.
pub fn replace_in_column(
    text: &str,
    d: &Dialect,
    col: usize,
    find: &str,
    replace: &str,
) -> (Transaction, usize) {
    let mut tx = Transaction::new("Replace in Column");
    let mut n = 0;
    if find.is_empty() {
        return (tx, 0);
    }
    let mut at = 0;
    let mut first = true;
    while at < text.len() {
        let rec = scan(text, at, d);
        let header = first && d.header;
        first = false;
        if !header && let Some(f) = rec.fields.get(col) {
            let v = value(text, f, d);
            if v.contains(find) {
                let new = v.replace(find, replace);
                let _ = tx.replace(f.range.clone(), encode(&new, d));
                n += 1;
            }
        }
        if rec.next <= at {
            break;
        }
        at = rec.next;
    }
    (tx, n)
}

/// The shading of a spreadsheet's row numbers and column letters.
pub const SHEET_GRAY: u32 = 0xbfbfbf55;

/// The cell at the cursor, its row number and column letter, as a
/// spreadsheet marks them (Excel's green).
pub const SHEET_ACTIVE: u32 = 0x9fd18a99;

/// The narrowest a column of the spreadsheet look is, in characters, as
/// Excel's columns are some eight characters wide however little they
/// hold.
pub const SHEET_MIN_WIDTH: usize = 8;

/// The width of each column of the spreadsheet look: its widest value
/// (at most forty characters), its letters, at least
/// [`SHEET_MIN_WIDTH`].
pub fn sheet_widths(layout: &Layout) -> Vec<usize> {
    layout
        .widths
        .iter()
        .enumerate()
        .map(|(j, &w)| {
            let letters = crate::csv_tools::column_letters(j).len();
            if layout.columns.widths.contains_key(&j) {
                // A width set by hand, wide enough for the letters.
                w.max(letters)
            } else {
                w.max(letters).max(SHEET_MIN_WIDTH)
            }
        })
        .collect()
}

/// The bar of column letters above a spreadsheet-looking grid: the text
/// of each piece and whether it is the column at the cursor (`current`).
/// The pieces line up with the rows of [`line_view`]: the gutter and the
/// grid's left edge, then each column's letters centered over the cell
/// and the edge after it.
pub fn letters_bar(layout: &Layout, current: Option<usize>) -> Vec<(String, bool)> {
    let mut out = vec![(" ".repeat(layout.gutter + 2), false), ("│".into(), false)];
    for (j, w) in sheet_widths(layout).into_iter().enumerate() {
        if layout.columns.hidden.contains(&j) {
            continue;
        }
        let letters = crate::csv_tools::column_letters(j);
        let w = w + 2;
        let left = (w - letters.len()) / 2;
        out.push((
            format!(
                "{}{letters}{}",
                " ".repeat(left),
                " ".repeat(w - letters.len() - left)
            ),
            current == Some(j),
        ));
        out.push(("│".into(), false));
    }
    out
}

/// The colors of rainbow columns, readable on light and dark themes.
const RAINBOW: [u32; 6] = [
    0x2e86deff, 0xc0392bff, 0x27ae60ff, 0x8e44adff, 0xd35400ff, 0x16a085ff,
];

/// How a CSV document is laid out as a grid, for a text version: its
/// dialect, the width of each column (from the first thousand records,
/// at most forty characters), which columns hold numbers, and the record
/// index.
#[derive(Debug)]
pub struct Layout {
    /// The dialect.
    pub dialect: Dialect,
    /// Column widths, in characters.
    pub widths: Vec<usize>,
    /// Columns whose values are numbers or dates (most of the non-empty
    /// ones of the first thousand data records).
    pub numeric: Vec<bool>,
    /// How the grid shows the document.
    pub view: View,
    /// Hidden columns, widths set by hand, the frozen column.
    pub columns: Columns,
    /// The width of the row numbers of the coordinate grid.
    pub gutter: usize,
    /// Record starts, found as far as the view needed.
    pub index: std::cell::RefCell<Index>,
}

/// A value right-aligned in a column: a number as spreadsheets write it,
/// or a date (`2026-09-29`, `29.09.2026`, `9/29/2026`).
fn numeric_value(v: &str, comma_decimal: bool) -> bool {
    let v = v.trim();
    if number(v, comma_decimal).is_some() {
        return true;
    }
    let parts: Vec<&str> = v.split(['-', '.', '/']).collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| (1..=4).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The widest a column is laid out.
const MAX_WIDTH: usize = 40;

impl Layout {
    /// The layout of `text`, its dialect detected.
    pub fn new(text: &str) -> Layout {
        Layout::with(text, detect(text))
    }

    /// The layout of `text` in `dialect`.
    pub fn with(text: &str, dialect: Dialect) -> Layout {
        Layout::with_view(text, dialect, View::default())
    }

    /// The layout of `text` in `dialect`, shown as `view` says.
    pub fn with_view(text: &str, dialect: Dialect, view: View) -> Layout {
        Layout::with_columns(text, dialect, view, Columns::default())
    }

    /// The layout of `text` in `dialect`, shown as `view` and `columns`
    /// say.
    pub fn with_columns(text: &str, dialect: Dialect, view: View, columns: Columns) -> Layout {
        use unicode_width::UnicodeWidthStr;
        let mut index = Index::new(text);
        let mut widths: Vec<usize> = Vec::new();
        // Per column: numeric and non-empty data values.
        let mut counts: Vec<(usize, usize)> = Vec::new();
        let comma = dialect.delimiter == b';';
        for i in 0..1000 {
            let Some(r) = index.record(text, i, &dialect) else {
                break;
            };
            for (j, f) in r.fields.iter().enumerate() {
                let w = text[f.range.clone()].width().min(MAX_WIDTH);
                if j >= widths.len() {
                    widths.push(w);
                    counts.push((0, 0));
                } else {
                    widths[j] = widths[j].max(w);
                }
                if i == 0 && dialect.header {
                    continue;
                }
                let v = value(text, f, &dialect);
                if !v.trim().is_empty() {
                    counts[j].1 += 1;
                    if numeric_value(&v, comma) {
                        counts[j].0 += 1;
                    }
                }
            }
        }
        let numeric = counts
            .iter()
            .map(|&(n, all)| all > 0 && n * 5 >= all * 4)
            .collect();
        let gutter = if view.coordinates || view.sheet {
            (memchr_count(text) + 1).to_string().len()
        } else {
            0
        };
        for (&j, &w) in &columns.widths {
            if j < widths.len() {
                widths[j] = w;
            }
        }
        Layout {
            dialect,
            widths,
            numeric,
            view,
            columns,
            gutter,
            index: std::cell::RefCell::new(index),
        }
    }

    /// The row and the record starting at byte `line_start`, when a record
    /// starts there (not inside a quoted field of the one before).
    pub fn record_at(&self, text: &str, line_start: usize) -> Option<(usize, Record)> {
        let mut idx = self.index.borrow_mut();
        let row = idx.row_at(text, line_start, &self.dialect);
        let r = idx.record(text, row, &self.dialect)?;
        (r.range.start == line_start).then_some((row, r))
    }
}

/// The number of line feeds in `text`: an upper bound of its records.
fn memchr_count(text: &str) -> usize {
    text.bytes().filter(|&b| b == b'\n').count()
}

/// What a memo is for: the text's version and a length or column.
type Key = (u64, usize, Dialect);

/// The layout last computed, with what it was computed for.
type LayoutMemo = ((Key, View, Columns), std::rc::Rc<Layout>);

thread_local! {
    static LAYOUT: std::cell::RefCell<Option<LayoutMemo>> =
        const { std::cell::RefCell::new(None) };
}

/// The layout of the CSV document `doc`, for its text version.
pub fn layout(doc: &crate::DocumentState) -> std::rc::Rc<Layout> {
    // The dialect is found once and kept: renaming a header cell to a
    // number or editing the first line does not change it.
    let dialect = match doc.csv_dialect.get() {
        Some(d) => d,
        None => {
            let d = detect(doc.text().as_str());
            doc.csv_dialect.set(Some(d));
            d
        }
    };
    let key = (
        (doc.version(), doc.text().len(), dialect),
        doc.csv_view,
        doc.csv_columns.clone(),
    );
    LAYOUT.with(|l| {
        if let Some((k, v)) = &*l.borrow()
            && *k == key
        {
            return v.clone();
        }
        let v = std::rc::Rc::new(Layout::with_columns(
            doc.text().as_str(),
            dialect,
            doc.csv_view,
            doc.csv_columns.clone(),
        ));
        *l.borrow_mut() = Some((key, v.clone()));
        v
    })
}

/// A line of a CSV document as a row of the grid: each field as written
/// (so that it is edited in place), padded to its column's width (on the
/// left in a column of numbers), with `│` for the delimiters; the header
/// row bold; with the view's options, each column its color, and the row
/// number and column letters of the coordinate grid. A line inside a
/// record that spans lines (a quoted line break) shows as it is.
pub fn line_view(
    layout: &Layout,
    text: &str,
    line: Range<usize>,
    cursor: Option<usize>,
) -> crate::view::LineView {
    use crate::view::{LineView, Run, Style};
    use unicode_width::UnicodeWidthStr;
    let Some((row, rec)) = layout
        .record_at(text, line.start)
        .filter(|(_, r)| r.range.end <= line.end)
    else {
        return crate::view::plain_line_view(text, line, None);
    };
    let header = row == 0 && layout.dialect.header;
    let view = layout.view;
    // The cell at the cursor, in a spreadsheet-looking grid.
    let active = cursor
        .filter(|_| view.sheet)
        .filter(|c| rec.range.start <= *c && *c <= rec.range.end)
        .map(|c| {
            rec.fields
                .iter()
                .position(|f| c <= f.range.end)
                .unwrap_or(rec.fields.len().saturating_sub(1))
        });
    let shade = |color: u32| crate::rich::CharFormat {
        highlight: Some(crate::theme::Color(color)),
        ..Default::default()
    };
    let style_of = |j: usize| Style {
        bold: header,
        rich: crate::rich::CharFormat {
            color: view
                .rainbow
                .then(|| crate::theme::Color(RAINBOW[j % RAINBOW.len()])),
            ..Default::default()
        },
        ..Style::default()
    };
    let deco = |at: usize, t: String, dim: bool| Run {
        src: at..at,
        text: t,
        verbatim: false,
        style: Style {
            dim,
            bold: header && !dim,
            ..Style::default()
        },
        widget: None,
    };
    let mut runs = Vec::new();
    let sheet_w = if view.sheet {
        sheet_widths(layout)
    } else {
        Vec::new()
    };
    let width_of = |j: usize| {
        if view.sheet {
            sheet_w.get(j).copied().unwrap_or(SHEET_MIN_WIDTH)
        } else {
            layout.widths.get(j).copied().unwrap_or(0)
        }
    };
    let bar = |at: usize, t: &str| Run {
        src: at..at,
        text: t.into(),
        verbatim: false,
        style: Style {
            dim: true,
            ..Style::default()
        },
        widget: None,
    };
    if view.sheet {
        // The row number in a shaded gutter, marked on the cursor's row.
        let here = active.is_some();
        runs.push(Run {
            src: line.start..line.start,
            text: format!(" {:<w$} ", row + 1, w = layout.gutter),
            verbatim: false,
            style: Style {
                bold: here,
                rich: shade(if here { SHEET_ACTIVE } else { SHEET_GRAY }),
                ..Style::default()
            },
            widget: None,
        });
        // The grid's left edge.
        runs.push(bar(line.start, "│ "));
    } else if view.coordinates {
        // The row number, in the gutter.
        runs.push(deco(
            line.start,
            format!("{:>w$} ", row + 1, w = layout.gutter),
            true,
        ));
    }
    // The last field that shows: a hidden last column takes the
    // delimiter before it out of sight too.
    let shown_last = (0..rec.fields.len())
        .rev()
        .find(|j| !layout.columns.hidden.contains(j));
    let at_cursor = |f: &Field| cursor.is_some_and(|c| f.range.start <= c && c <= f.range.end);
    for (j, f) in rec.fields.iter().enumerate() {
        if layout.columns.hidden.contains(&j) {
            // The field and the delimiter after it (before it, for the
            // last shown) take no room.
            let end = if Some(j) > shown_last || j + 1 == rec.fields.len() {
                f.range.end
            } else {
                f.range.end + 1
            };
            let start = if Some(j) > shown_last && j > 0 {
                f.range.start - 1
            } else {
                f.range.start
            };
            runs.push(Run {
                src: start..end,
                text: String::new(),
                verbatim: false,
                style: Style::default(),
                widget: None,
            });
            continue;
        }
        let s = &text[f.range.clone()];
        let last = Some(j) == shown_last;
        let on = active == Some(j);
        // A value longer than a width set by hand shows cut, but whole at
        // the cursor.
        let cut = layout
            .columns
            .widths
            .get(&j)
            .filter(|&&w| s.width() > w && !at_cursor(f))
            .map(|&w| truncate(s, w));
        let s_width = cut.as_ref().map_or(s.width(), |c| c.width());
        if view.coordinates && !view.sheet {
            // The column's letters on the first row, the same width of
            // blanks below them.
            let letters = crate::csv_tools::column_letters(j);
            let label = if row == 0 {
                format!("{letters}:")
            } else {
                " ".repeat(letters.len() + 1)
            };
            runs.push(deco(f.range.start, label, true));
        }
        let pad = width_of(j).saturating_sub(s_width);
        let right =
            view.align_numbers && !header && layout.numeric.get(j).copied().unwrap_or(false);
        // The cell at the cursor marked across its width.
        let mark = |mut r: Run| {
            if on {
                r.style.rich.highlight = Some(crate::theme::Color(SHEET_ACTIVE));
            }
            r
        };
        if right && pad > 0 {
            runs.push(mark(deco(f.range.start, " ".repeat(pad), false)));
        }
        if let Some(c) = cut {
            runs.push(mark(Run {
                src: f.range.clone(),
                text: c,
                verbatim: false,
                style: style_of(j),
                widget: None,
            }));
        } else if !s.is_empty() {
            runs.push(mark(Run {
                src: f.range.clone(),
                text: s.to_string(),
                verbatim: true,
                style: style_of(j),
                widget: None,
            }));
        } else if on && pad == 0 {
            runs.push(mark(deco(f.range.start, " ".into(), false)));
        }
        if last && (on || view.sheet) && pad > 0 && !right {
            runs.push(mark(deco(f.range.end, " ".repeat(pad), false)));
        }
        if last && view.sheet {
            // The edge after the last cell, then empty cells to the last
            // column, so the grid goes on.
            runs.push(bar(f.range.end, " │"));
            for k in
                (rec.fields.len()..sheet_w.len()).filter(|k| !layout.columns.hidden.contains(k))
            {
                runs.push(bar(
                    f.range.end,
                    &format!("{} │", " ".repeat(width_of(k) + 1)),
                ));
            }
        }
        if !last {
            // The padding, then the delimiter drawn as a bar.
            if pad > 0 && !right {
                runs.push(mark(deco(f.range.end, " ".repeat(pad), false)));
            }
            runs.push(Run {
                src: f.range.end..f.range.end + 1,
                text: " │ ".into(),
                verbatim: false,
                style: Style {
                    dim: true,
                    ..Style::default()
                },
                widget: None,
            });
        }
    }
    LineView {
        runs,
        range: line,
        mono: true,
        ..LineView::default()
    }
}

/// `s` cut to `w` columns, the last one `…`.
fn truncate(s: &str, w: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if used + cw + 1 > w {
            break;
        }
        used += cw;
        out.push(c);
    }
    out.push('…');
    out
}

/// The widest value of each column, uncut, over the first ten thousand
/// records: what Autosize sets.
pub fn natural_widths(text: &str, d: &Dialect) -> Vec<usize> {
    use unicode_width::UnicodeWidthStr;
    let mut index = Index::new(text);
    let mut widths: Vec<usize> = Vec::new();
    for i in 0..10_000 {
        let Some(r) = index.record(text, i, d) else {
            break;
        };
        for (j, f) in r.fields.iter().enumerate() {
            let w = text[f.range.clone()].width();
            if j >= widths.len() {
                widths.resize(j + 1, 0);
            }
            widths[j] = widths[j].max(w);
        }
    }
    widths
}

/// The width in characters of the frozen first column with what comes
/// before it (the row numbers) and the bar after it, when the view
/// freezes it.
pub fn frozen_width(layout: &Layout) -> Option<usize> {
    if !layout.columns.frozen {
        return None;
    }
    let first = (0..layout.widths.len().max(1)).find(|j| !layout.columns.hidden.contains(j))?;
    let w = if layout.view.sheet {
        sheet_widths(layout)
            .get(first)
            .copied()
            .unwrap_or(SHEET_MIN_WIDTH)
    } else {
        layout.widths.get(first).copied().unwrap_or(0)
    };
    let gutter = if layout.view.sheet {
        layout.gutter + 4
    } else if layout.view.coordinates {
        layout.gutter + 1 + crate::csv_tools::column_letters(first).len() + 1
    } else {
        0
    };
    Some(gutter + w + 2)
}

/// The cell at the cursor of the CSV document `doc`: the layout, the row,
/// its record and the column.
pub fn cell_at(doc: &crate::DocumentState) -> Option<(std::rc::Rc<Layout>, usize, Record, usize)> {
    cell_at_offset(doc, doc.selection.head)
}

/// Rows and columns of a rectangle of cells, each as first and last.
pub type Rectangle = ((usize, usize), (usize, usize));

/// The rows and columns of the rectangle of cells a selection spans in a
/// CSV document: from the anchor's cell to the cursor's, when they are in
/// different rows (a selection within one row stays text).
pub fn cell_rectangle(doc: &crate::DocumentState) -> Option<Rectangle> {
    let sel = doc.selection;
    if sel.anchor == sel.head || !doc.extra.is_empty() {
        return None;
    }
    let (_, r1, _, c1) = cell_at(doc)?;
    let (_, r0, _, c0) = cell_at_offset(doc, sel.anchor)?;
    (r0 != r1).then_some(((r0.min(r1), r0.max(r1)), (c0.min(c1), c0.max(c1))))
}

/// The byte ranges of the cells of [`cell_rectangle`], one per row, from
/// its first column's field to its last (shorter rows to their end), to
/// paint as selected.
pub fn rectangle_ranges(doc: &crate::DocumentState) -> Option<Vec<Range<usize>>> {
    let ((r0, r1), (c0, c1)) = cell_rectangle(doc)?;
    let layout = layout(doc);
    let text = doc.text().as_str();
    let mut idx = layout.index.borrow_mut();
    let mut out = Vec::new();
    for row in r0..=r1 {
        let Some(rec) = idx.record(text, row, &layout.dialect) else {
            break;
        };
        let (Some(first), Some(last)) =
            (rec.fields.get(c0), rec.fields.get(c1).or(rec.fields.last()))
        else {
            continue;
        };
        out.push(first.range.start..last.range.end.max(first.range.start));
    }
    Some(out)
}

/// The cell at byte `pos`, as [`cell_at`] gives the cursor's.
pub fn cell_at_offset(
    doc: &crate::DocumentState,
    pos: usize,
) -> Option<(std::rc::Rc<Layout>, usize, Record, usize)> {
    if doc.meta.mode != crate::DocumentMode::Csv {
        return None;
    }
    let layout = layout(doc);
    let text = doc.text().as_str();
    let pos = pos.min(text.len());
    let (row, rec) = {
        let mut idx = layout.index.borrow_mut();
        let row = idx.row_at(text, pos, &layout.dialect);
        let rec = idx.record(text, row, &layout.dialect)?;
        (row, rec)
    };
    let col = rec
        .fields
        .iter()
        .position(|f| pos <= f.range.end)
        .unwrap_or(rec.fields.len().saturating_sub(1));
    Some((layout, row, rec, col))
}

thread_local! {
    static STATS: std::cell::RefCell<Option<(Key, Option<String>)>> =
        const { std::cell::RefCell::new(None) };
}

/// The rows a filter keeps: the header, the record at `cursor`, and the
/// records with a field containing `needle` (case ignored); the byte
/// ranges they show, merged, each with its line ending (the last one
/// reaching `text.len() + 1`, the line after a final line feed), and how
/// many data rows match out of how many.
pub fn filter_rows(
    text: &str,
    d: &Dialect,
    needle: &str,
    cursor: usize,
) -> (Vec<Range<usize>>, usize, usize) {
    let needle = needle.to_lowercase();
    let mut out: Vec<Range<usize>> = Vec::new();
    let (mut matched, mut total) = (0, 0);
    let mut start = sep_line(text).map_or(0, |(_, n)| n);
    let mut row = 0;
    while start < text.len() {
        let rec = scan(text, start, d);
        let next = if rec.next < text.len() {
            rec.next
        } else {
            text.len() + 1
        };
        let header = row == 0 && d.header;
        // Blank lines are not rows of data.
        let blank = rec.range.is_empty();
        let hit = !header
            && !blank
            && rec
                .fields
                .iter()
                .any(|f| value(text, f, d).to_lowercase().contains(&needle));
        if !header && !blank {
            total += 1;
            matched += usize::from(hit);
        }
        let here = rec.range.start <= cursor && cursor < next;
        if header || hit || here {
            match out.last_mut() {
                Some(r) if r.end == rec.range.start => r.end = next,
                _ => out.push(rec.range.start..next),
            }
        }
        if next <= start {
            break;
        }
        start = next;
        row += 1;
    }
    if out.is_empty() {
        out.push(0..text.len() + 1);
    }
    (out, matched, total)
}

/// What a filter's memo is for: the text's version, the filter, the
/// cursor's line.
type FilterKey = (u64, String, usize);

/// What a CSV document's filter keeps.
#[derive(Debug)]
pub struct Filtered {
    /// The byte ranges shown, merged ([`filter_rows`]).
    pub ranges: Vec<Range<usize>>,
    /// The data rows that match.
    pub matched: usize,
    /// The data rows.
    pub total: usize,
}

thread_local! {
    static FILTERED: std::cell::RefCell<Option<(FilterKey, std::rc::Rc<Filtered>)>> =
        const { std::cell::RefCell::new(None) };
}

/// The rows a CSV document's filter keeps ([`filter_rows`] for its text,
/// filter and cursor), memoized; `None` without a filter.
pub fn filtered(doc: &crate::DocumentState) -> Option<std::rc::Rc<Filtered>> {
    let needle = doc.csv_filter.as_deref().filter(|f| !f.is_empty())?;
    if doc.meta.mode != crate::DocumentMode::Csv {
        return None;
    }
    let text = doc.text().as_str();
    // The record at the cursor stays, so the key is its line.
    let line = doc.text().line_of(doc.selection.head.min(text.len()));
    let key = (doc.version(), needle.to_string(), line);
    FILTERED.with(|m| {
        if let Some((k, v)) = &*m.borrow()
            && *k == key
        {
            return Some(v.clone());
        }
        let layout = layout(doc);
        let (ranges, matched, total) =
            filter_rows(text, &layout.dialect, needle, doc.selection.head);
        let v = std::rc::Rc::new(Filtered {
            ranges,
            matched,
            total,
        });
        *m.borrow_mut() = Some((key, v.clone()));
        Some(v)
    })
}

/// What a shown-lines memo is for: the version, the filter, the sort, the
/// cursor's line.
type ShownKey = (u64, Option<String>, Option<(usize, bool)>, usize);

thread_local! {
    static SHOWN: std::cell::RefCell<Option<(ShownKey, std::rc::Rc<Vec<usize>>)>> =
        const { std::cell::RefCell::new(None) };
}

/// The lines of a CSV document in the order its view shows them, when a
/// filter or a sort is on (`None` otherwise): the header first, then the
/// records the filter keeps, in the order of the sorted column; the file
/// keeps its order.
pub fn shown_lines(doc: &crate::DocumentState) -> Option<std::rc::Rc<Vec<usize>>> {
    if doc.meta.mode != crate::DocumentMode::Csv {
        return None;
    }
    let filter = filtered(doc);
    if filter.is_none() && doc.csv_sort.is_none() {
        return None;
    }
    let t = doc.text();
    let text = t.as_str();
    let line = t.line_of(doc.selection.head.min(text.len()));
    let key = (doc.version(), doc.csv_filter.clone(), doc.csv_sort, line);
    if let Some(v) = SHOWN.with(|m| {
        m.borrow()
            .as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
    }) {
        return Some(v);
    }
    let layout = layout(doc);
    let d = &layout.dialect;
    let mut idx = Index::new(text);
    let n = idx.count(text, d);
    let records: Vec<Record> = (0..n).filter_map(|i| idx.record(text, i, d)).collect();
    let kept = |r: &Record| {
        filter.as_ref().is_none_or(|f| {
            f.ranges
                .iter()
                .any(|x| x.start <= r.range.start && r.range.start < x.end)
        })
    };
    let order: Vec<usize> = match doc.csv_sort {
        Some((col, reverse)) => sorted_order(text, d, col, reverse),
        None => (0..records.len()).collect(),
    };
    let mut out = Vec::new();
    for i in order {
        let Some(r) = records.get(i) else { continue };
        if !kept(r) {
            continue;
        }
        let first = t.line_of(r.range.start);
        let last = t.line_of(r.range.end.max(r.range.start));
        out.extend(first..=last);
    }
    // The empty line after a final line feed, when all rows show.
    let lines = t.line_count();
    if filter.is_none()
        && lines > 0
        && !out.contains(&(lines - 1))
        && t.line_range(lines - 1).is_empty()
    {
        out.push(lines - 1);
    }
    if out.is_empty() {
        out.push(0);
    }
    let v = std::rc::Rc::new(out);
    SHOWN.with(|m| *m.borrow_mut() = Some((key, v.clone())));
    Some(v)
}

/// The status bar's numbers for the column at the cursor of a CSV
/// document: count, sum, average, smallest and largest; after a filter,
/// how many rows it keeps.
pub fn status(doc: &crate::DocumentState) -> Option<String> {
    // A malformed field in the cursor's record comes first.
    if let Some((layout, _, rec, _)) = cell_at(doc)
        && let Some(p) = record_problems(doc.text().as_str(), &rec, &layout.dialect).first()
    {
        return Some(p.message.clone());
    }
    let numbers = column_status(doc);
    let Some(f) = filtered(doc) else {
        return numbers;
    };
    let filter = crate::tr!(
        "status-csv-filter",
        filter = doc.csv_filter.clone().unwrap_or_default(),
        matched = f.matched,
        total = f.total
    );
    Some(match numbers {
        Some(n) => format!("{filter}   {n}"),
        None => filter,
    })
}

fn column_status(doc: &crate::DocumentState) -> Option<String> {
    let (layout, _, _, col) = cell_at(doc)?;
    let key = (doc.version(), col, layout.dialect);
    STATS.with(|s| {
        if let Some((k, v)) = &*s.borrow()
            && *k == key
        {
            return v.clone();
        }
        let v = column_stats(doc.text().as_str(), &layout.dialect, col).map(
            |(n, sum, avg, min, max)| {
                let f = |x: f64| crate::formulas::number(x);
                format!(
                    "{}   {}",
                    crate::tr!("status-table-count", count = n),
                    crate::tr!(
                        "status-table-numbers",
                        sum = f(sum),
                        average = f(avg),
                        min = f(min),
                        max = f(max)
                    )
                )
            },
        );
        *s.borrow_mut() = Some((key, v.clone()));
        v
    })
}

/// Pasted text in a CSV document: tab-separated rows (copied from a
/// spreadsheet) written with the document's delimiter.
pub fn pasted(text: &str, d: &Dialect) -> Option<String> {
    if d.delimiter == b'\t' || !text.contains('\t') {
        return None;
    }
    let tsv = Dialect {
        delimiter: b'\t',
        ..*d
    };
    let rows = rows(text, &tsv);
    let out: Vec<String> = rows
        .iter()
        .map(|r| {
            r.iter()
                .map(|v| encode(v, d))
                .collect::<Vec<_>>()
                .join(&d.delimiter_char().to_string())
        })
        .collect();
    let mut out = out.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_character_detected() {
        let d = detect("'name','note'\n'Ali','a, b'\n'Ayşe','c'\n");
        assert_eq!((d.quote, d.delimiter), (b'\'', b','));
        // Double quotes when both occur, and when none does.
        assert_eq!(detect("\"a\",'b'\n\"c\",'d'\n").quote, b'"');
        assert_eq!(detect("it's,x\nno,y\n").quote, b'"');
    }

    #[test]
    fn malformed_fields() {
        let d = Dialect::default();
        // The value as the scanner reads the field: up to the first
        // undoubled quote, the rest as written.
        let text = "\"a\"b\"c,d\n";
        let r = scan(text, 0, &d);
        assert_eq!(value(text, &r.fields[0], &d), "ab\"c");
        let p = problems(text, &d, 10);
        assert_eq!(p.len(), 1);
        assert_eq!(
            (p[0].code, p[0].range.clone()),
            ("csv-text-after-quote", 3..6)
        );
        let text = "a,b\"c\n\"x\"\"y\",z\n\"open,1\n2,3\n";
        let codes: Vec<_> = problems(text, &d, 10)
            .into_iter()
            .map(|p| (p.code, p.range.start))
            .collect();
        assert_eq!(
            codes,
            vec![("csv-bare-quote", 3), ("csv-unterminated-quote", 15)]
        );
        assert!(problems("a,b\n\"c\"\"d\",e\n", &d, 10).is_empty());
    }

    #[test]
    fn blank_lines() {
        let d = Dialect {
            header: true,
            ..Dialect::default()
        };
        let text = "sep=,\nn,v\nb,2\n\na,1\n";
        // Sorted: blank lines last, the `sep=` line and the header kept.
        let sorted = sort_file(text, &d, 0, false).apply(text);
        assert_eq!(sorted, "sep=,\nn,v\na,1\nb,2\n\n");
        let sorted = sort_file(text, &d, 0, true).apply(text);
        assert_eq!(sorted, "sep=,\nn,v\nb,2\na,1\n\n");
        // Not counted by the filter; a new column leaves them blank.
        let (_, matched, total) = filter_rows(text, &d, "a", 0);
        assert_eq!((matched, total), (1, 2));
        let wide = insert_column(text, &d, 2).apply(text);
        assert_eq!(wide, "sep=,\nn,v,\nb,2,\n\na,1,\n");
    }

    #[test]
    fn a_lone_carriage_return_is_data() {
        let d = Dialect::default();
        let text = "a\rb,c\r\nd,e\n";
        let r = scan(text, 0, &d);
        assert_eq!(value(text, &r.fields[0], &d), "a\rb");
        assert_eq!(r.next, 7);
        assert_eq!(rows(text, &d), vec![vec!["a\rb", "c"], vec!["d", "e"]]);
    }

    #[test]
    fn sorting_a_mixed_column_is_a_total_order() {
        // Numbers and text mixed (and NaN): no panic, numbers first.
        let mut text = String::from("v\n");
        for i in 0..2000 {
            text.push_str(&match i % 4 {
                0 => format!("{}\n", i % 17),
                1 => format!("{}a\n", i % 13),
                2 => "nan\n".to_string(),
                _ => format!("x{}\n", i % 7),
            });
        }
        let d = detect(&text);
        let order = sorted_order(&text, &d, 0, false);
        let rows = rows(&text, &d);
        let first_text = order
            .iter()
            .skip(1)
            .position(|&i| number(&rows[i][0], false).is_none())
            .unwrap();
        assert!(
            order
                .iter()
                .skip(1 + first_text)
                .all(|&i| number(&rows[i][0], false).is_none())
        );
    }

    #[test]
    fn numbers_as_spreadsheets_write_them() {
        assert_eq!(number("1,234.5", false), Some(1234.5));
        assert_eq!(number("1.234,5", false), Some(1234.5));
        assert_eq!(number("1,5", false), Some(1.5));
        assert_eq!(number("1,234", false), Some(1234.0));
        assert_eq!(number("1,234", true), Some(1.234));
        assert_eq!(number("1,234,567", true), Some(1_234_567.0));
        assert_eq!(number("1.234.567", false), Some(1_234_567.0));
        assert_eq!(number("12.5", false), Some(12.5));
        assert_eq!(number("-3", false), Some(-3.0));
        assert_eq!(number("1 234,5", true), Some(1234.5));
        assert_eq!(number("abc", false), None);
        assert_eq!(number("inf", false), None);
        // A Turkish thousand in a file `;` delimits; a decimal elsewhere.
        assert_eq!(number("1.234", true), Some(1234.0));
        assert_eq!(number("1.234", false), Some(1.234));
        assert_eq!(number("12.5", true), Some(12.5));
        assert_eq!(number("1 234 567", false), Some(1_234_567.0));
        assert_eq!(number("0,5", true), Some(0.5));
        assert_eq!(number("0", false), Some(0.0));
        // Dates, versions, phone numbers and identifiers are not numbers.
        for v in [
            "29.09.2026",
            "2026-09-29",
            "9/29/2026",
            "1.2.3",
            "10.0.1",
            "+90 532 123 45 67",
            "0532 123 45 67",
            "(0532) 123 4567",
            "05321234567",
            "007",
            "12:30",
        ] {
            assert_eq!(number(v, false), None, "{v}");
            assert_eq!(number(v, true), None, "{v}");
        }
        // The header test reads numbers as the statistics do: a column of
        // phone numbers under a header is still a header.
        let t = "ad,telefon\nAda,0532 123 45 67\nBob,7\n";
        assert!(detect(t).header);
    }

    fn apply(text: &str, tx: Transaction) -> String {
        tx.apply(text)
    }

    #[test]
    fn dialects() {
        let d = detect("name,age\nAda,36\nAlan,41\n");
        assert_eq!((d.delimiter, d.header, d.crlf), (b',', true, false));
        let d = detect("ad;yaş;şehir\r\nAyşe;30;İzmir\r\nMehmet;41;Ankara\r\n");
        assert_eq!((d.delimiter, d.header, d.crlf), (b';', true, true));
        let d = detect("a\tb\tc\n1\t2\t3\n");
        assert_eq!(d.delimiter, b'\t');
        let d = detect("1,2,3\n4,5,6\n");
        assert!(!d.header);
        // Commas inside numbers do not fool it.
        let d = detect("ürün;fiyat\nelma;1,5\narmut;2,25\n");
        assert_eq!(d.delimiter, b';');
        // Ties: the delimiter that leaves numbers.
        assert_eq!(detect("elma;1,5\n").delimiter, b';');
        assert_eq!(detect("1,5;2,5;3,5\n").delimiter, b';');
        // A title line of one field does not decide.
        let d = detect("My data\nname;qty\napple;3\npear;4\n");
        assert_eq!(d.delimiter, b';');
        // Excel's `sep=` line: the delimiter, and not a record.
        let t = "sep=;\nname;qty\napple;3\n";
        let d = detect(t);
        assert_eq!((d.delimiter, d.header), (b';', true));
        assert_eq!(
            rows(t, &d),
            [
                vec!["name".to_string(), "qty".into()],
                vec!["apple".into(), "3".into()]
            ]
        );
    }

    #[test]
    fn rfc4180() {
        let d = Dialect::default();
        let t = "a,\"b,c\",\"say \"\"hi\"\"\"\n\"multi\nline\",x,\n";
        let r = rows(t, &d);
        assert_eq!(
            r,
            [vec!["a", "b,c", "say \"hi\""], vec!["multi\nline", "x", ""]]
        );
        assert_eq!(encode("plain", &d), "plain");
        assert_eq!(encode("a,b", &d), "\"a,b\"");
        assert_eq!(encode("say \"hi\"", &d), "\"say \"\"hi\"\"\"");
        assert_eq!(encode(" pad", &d), "\" pad\"");
        // No final line feed; an empty file.
        assert_eq!(rows("x,y", &d), [vec!["x", "y"]]);
        assert!(rows("", &d).iter().all(|r| r == &vec![String::new()]));
    }

    #[test]
    fn minimal_edits() {
        let d = Dialect::default();
        let t = "id,name,note\n1,\"Ada\",keep  \n2,Alan,x\n";
        let mut idx = Index::new(t);
        let rec = idx.record(t, 1, &d).unwrap();
        // Only the field changes; the quotes elsewhere stay.
        assert_eq!(
            apply(t, set_cell(t, &rec, 2, "a,b", &d)),
            "id,name,note\n1,\"Ada\",\"a,b\"\n2,Alan,x\n"
        );
        assert_eq!(
            apply(t, set_cell(t, &rec, 4, "e", &d)),
            "id,name,note\n1,\"Ada\",keep  ,,e\n2,Alan,x\n"
        );
        let rec2 = idx.record(t, 2, &d).unwrap();
        assert_eq!(apply(t, insert_row(t, &rec2, 3, &d)), format!("{t},,\n"));
        assert_eq!(apply(t, delete_row(t, &rec)), "id,name,note\n2,Alan,x\n");
        assert_eq!(
            apply(t, swap_rows(t, &rec, &rec2)),
            "id,name,note\n2,Alan,x\n1,\"Ada\",keep  \n"
        );
        assert_eq!(
            apply(t, insert_column(t, &d, 1)),
            "id,,name,note\n1,,\"Ada\",keep  \n2,,Alan,x\n"
        );
        assert_eq!(
            apply(t, delete_column(t, &d, 1)),
            "id,note\n1,keep  \n2,x\n"
        );
        assert_eq!(
            apply(t, delete_column(t, &d, 2)),
            "id,name\n1,\"Ada\"\n2,Alan\n"
        );
        assert_eq!(
            apply(t, swap_columns(t, &d, 0)),
            "name,id,note\n\"Ada\",1,keep  \nAlan,2,x\n"
        );
        assert_eq!(idx.row_at(t, 20, &d), 1);
    }

    #[test]
    fn sorting_and_conversions() {
        let t = "name,n\nb,10\na,9\nc,100\n";
        let d = detect(t);
        assert_eq!(sorted_order(t, &d, 1, false), [0, 2, 1, 3]);
        assert_eq!(
            apply(t, sort_file(t, &d, 0, true)),
            "name,n\nc,100\nb,10\na,9\n"
        );
        assert_eq!(to_tsv(&[vec!["a b".into(), "c".into()]]), "a b\tc");
        assert_eq!(
            to_org_table(t, &d),
            "| name |   n |\n|------+-----|\n| b    |  10 |\n| a    |   9 |\n| c    | 100 |\n"
        );
        let (n, sum, avg, min, max) = column_stats(t, &d, 1).unwrap();
        assert_eq!((n, sum, avg, min, max), (3, 119.0, 119.0 / 3.0, 9.0, 100.0));
        assert!(column_stats(t, &d, 0).is_none());
        // European numbers.
        let e = "ürün;fiyat\nelma;1,5\narmut;1.234,5\n";
        let d = detect(e);
        assert_eq!(column_stats(e, &d, 1).unwrap().1, 1236.0);
    }

    /// The grid without the spreadsheet look.
    const CLASSIC: View = View {
        align_numbers: true,
        rainbow: false,
        coordinates: false,
        sheet: false,
    };

    #[test]
    fn spreadsheet_look() {
        let t = "name,n\nAda,36\nBob,7\n";
        let sheet = View {
            sheet: true,
            ..CLASSIC
        };
        let l = Layout::with_view(t, detect(t), sheet);
        let line = |i: usize| {
            let s: usize = t.split_inclusive('\n').take(i).map(str::len).sum();
            s..s + t.split('\n').nth(i).unwrap().len()
        };
        // The row number shaded in the gutter; no letters in the cells;
        // columns at least eight characters wide, between edges.
        let v = line_view(&l, t, line(1), None);
        assert_eq!(v.display(), " 2 │ Ada      │       36 │");
        assert_eq!(
            v.runs[0].style.rich.highlight,
            Some(crate::theme::Color(SHEET_GRAY))
        );
        // The cell at the cursor and its row number marked.
        let at = t.find("36").unwrap();
        let v = line_view(&l, t, line(1), Some(at));
        let marked: String = v
            .runs
            .iter()
            .filter(|r| r.style.rich.highlight == Some(crate::theme::Color(SHEET_ACTIVE)))
            .map(|r| r.text.as_str())
            .collect();
        assert_eq!(marked, " 2       36");
        // The letters bar lines up with the rows.
        let bar: String = letters_bar(&l, Some(1))
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        assert_eq!(bar, "   │    A     │    B     │");
        assert_eq!(bar.chars().count(), v.display().chars().count());
        assert!(
            letters_bar(&l, Some(1))
                .iter()
                .any(|(s, on)| *on && s.trim() == "B")
        );
        // A short row: its missing cells drawn empty, the grid goes on.
        let t2 = "a,b,c\nx\n";
        let l2 = Layout::with_view(t2, detect(t2), sheet);
        let v = line_view(&l2, t2, 6..7, None);
        assert_eq!(v.display(), " 2 │ x        │          │          │");
    }

    #[test]
    fn grid_rows() {
        let t = "name,n\nAda,36\n\"long, name\",7\n\"two\nlines\",1\n";
        let l = Layout::with_view(t, detect(t), CLASSIC);
        assert_eq!(l.widths, [12, 2]);
        let lines: Vec<Range<usize>> = {
            let mut out = Vec::new();
            let mut s = 0;
            for (i, _) in t.match_indices('\n') {
                out.push(s..i);
                s = i + 1;
            }
            out
        };
        let v = line_view(&l, t, lines[1].clone(), None);
        assert_eq!(v.display(), "Ada          │ 36");
        assert!(v.runs[0].verbatim && !v.runs[0].style.bold);
        assert!(line_view(&l, t, lines[0].clone(), None).runs[0].style.bold);
        // Editing positions map through: after `Ada` is in the field.
        let at = t.find("Ada").unwrap() + 3;
        assert_eq!(v.source_offset(v.display_offset(at)), at);
        // A record over two lines shows as written.
        assert_eq!(line_view(&l, t, lines[3].clone(), None).display(), "\"two");
        assert_eq!(
            line_view(&l, t, lines[4].clone(), None).display(),
            "lines\",1"
        );
    }

    #[test]
    fn grid_views() {
        let t = "name,n,when\nAda,36,2026-09-29\nBob,7,29.09.2026\n";
        let lines: Vec<Range<usize>> = {
            let mut out = Vec::new();
            let mut s = 0;
            for (i, _) in t.match_indices('\n') {
                out.push(s..i);
                s = i + 1;
            }
            out
        };
        let d = detect(t);
        // Numbers and dates right, text left; the header as written.
        let l = Layout::with_view(t, d, CLASSIC);
        assert_eq!(l.numeric, [false, true, true]);
        assert_eq!(
            line_view(&l, t, lines[2].clone(), None).display(),
            "Bob  │  7 │ 29.09.2026"
        );
        assert_eq!(
            line_view(&l, t, lines[0].clone(), None).display(),
            "name │ n  │ when"
        );
        let plain = View {
            align_numbers: false,
            ..CLASSIC
        };
        let l = Layout::with_view(t, d, plain);
        assert_eq!(
            line_view(&l, t, lines[2].clone(), None).display(),
            "Bob  │ 7  │ 29.09.2026"
        );
        // The coordinate grid: row numbers and column letters.
        let grid = View {
            coordinates: true,
            ..CLASSIC
        };
        let l = Layout::with_view(t, d, grid);
        assert_eq!(
            line_view(&l, t, lines[0].clone(), None).display(),
            "1 A:name │ B:n  │ C:when"
        );
        let v = line_view(&l, t, lines[1].clone(), None);
        assert_eq!(v.display(), "2   Ada  │   36 │   2026-09-29");
        // Editing positions still map through the decorations.
        let at = t.find("36").unwrap() + 1;
        assert_eq!(v.source_offset(v.display_offset(at)), at);
        // Rainbow columns: each column its color.
        let l = Layout::with_view(
            t,
            d,
            View {
                rainbow: true,
                ..CLASSIC
            },
        );
        let v = line_view(&l, t, lines[1].clone(), None);
        let colors: Vec<_> = v
            .runs
            .iter()
            .filter(|r| r.verbatim)
            .map(|r| r.style.rich.color)
            .collect();
        assert_eq!(colors.len(), 3);
        assert!(colors.iter().all(Option::is_some));
        assert_ne!(colors[0], colors[1]);
    }

    #[test]
    fn large_files_open_lazily() {
        let t: String = (0..100_000)
            .map(|i| format!("{i},name {i},{}\n", i * 2))
            .collect();
        let d = detect(&t);
        let start = std::time::Instant::now();
        let mut idx = Index::new(&t);
        let r = idx.record(&t, 30, &d).unwrap();
        assert_eq!(value(&t, &r.fields[1], &d), "name 30");
        assert!(idx.starts.len() < 100);
        assert_eq!(idx.count(&t, &d), 100_000);
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }
}

#[cfg(test)]
mod spreadsheet_tests {
    use super::*;

    #[test]
    fn histogram_of_a_numeric_column() {
        let d = Dialect {
            delimiter: b',',
            header: true,
            ..Dialect::default()
        };
        let text = "n,v\na,1\nb,3\nc,12\nd,x\ne,19\nf,20\n";
        let h = histogram(text, &d, 1);
        let labels: Vec<String> = h.iter().map(bin_label).collect();
        assert_eq!(labels, ["0 – 5", "5 – 10", "10 – 15", "15 – 20", "20 – 25"]);
        let counts: Vec<usize> = h.iter().map(|b| b.count).collect();
        assert_eq!(counts, [2, 0, 1, 1, 1]);
        assert_eq!(h[0].first, 1);
        assert_eq!(h[2].first, 3);
        assert!(histogram(text, &d, 0).is_empty());
        // One value: one range.
        let h = histogram("v\n2.5\n2.5\n", &d, 0);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].count, 2);
    }

    #[test]
    fn frequencies_and_column_replace() {
        let t = "name,city\nAli,Ankara\nAyşe,İzmir\nCan,Ankara\n";
        let d = detect(t);
        assert_eq!(
            frequencies(t, &d, 1),
            [("Ankara".to_string(), 2), ("İzmir".to_string(), 1)]
        );
        assert_eq!(bar(1, 2, 10), "█████");
        let (tx, n) = replace_in_column(t, &d, 1, "Ankara", "Bursa");
        assert_eq!(n, 2);
        let mut s = t.to_string();
        for e in tx.edits.iter().rev() {
            s.replace_range(e.range.clone(), &e.insert);
        }
        assert_eq!(s, "name,city\nAli,Bursa\nAyşe,İzmir\nCan,Bursa\n");
        // The header and the other columns stay.
        let (_, n) = replace_in_column(t, &d, 0, "city", "x");
        assert_eq!(n, 0);
    }
}
