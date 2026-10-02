//! CSV conformance (T2.7d): Kalem's reader and writer against the `csv`
//! crate's, a reference RFC 4180 implementation, on files made at random.
//! Valid files (the reference writer's) must be read field for field;
//! values Kalem quotes and files after Kalem's edits must read back in the
//! reference reader. Malformed text, which RFC 4180 leaves undefined, is
//! measured and reported.

use kalem_core::csv::{Dialect, Index, detect, encode, rows, set_cell};

/// A small deterministic generator (xorshift).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// A value with the characters that need quoting, often.
fn value(rng: &mut Rng, delimiter: u8) -> String {
    const PIECES: [&str; 14] = [
        "a", "bc", " ", "\"", "\"\"", "\n", "\r\n", "é", "日本", "x y", "1.5", "", "\t", "'",
    ];
    let d = (delimiter as char).to_string();
    let mut s = String::new();
    for _ in 0..rng.below(5) {
        if rng.below(8) == 0 {
            s.push_str(&d);
        } else {
            s.push_str(PIECES[rng.below(PIECES.len())]);
        }
    }
    s
}

fn records(rng: &mut Rng, delimiter: u8) -> Vec<Vec<String>> {
    let columns = 1 + rng.below(5);
    (0..1 + rng.below(6))
        .map(|_| (0..columns).map(|_| value(rng, delimiter)).collect())
        .collect()
}

/// The reference writer's file of `recs`.
fn write(recs: &[Vec<String>], delimiter: u8, crlf: bool, all: bool) -> String {
    let mut w = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .terminator(if crlf {
            csv::Terminator::CRLF
        } else {
            csv::Terminator::Any(b'\n')
        })
        .quote_style(if all {
            csv::QuoteStyle::Always
        } else {
            csv::QuoteStyle::Necessary
        })
        .from_writer(Vec::new());
    for r in recs {
        w.write_record(r).unwrap();
    }
    String::from_utf8(w.into_inner().unwrap()).unwrap()
}

/// The reference reader's records of `text`.
fn read(text: &str, delimiter: u8) -> Option<Vec<Vec<String>>> {
    let mut r = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(text.as_bytes());
    r.records()
        .map(|x| x.ok().map(|x| x.iter().map(str::to_string).collect()))
        .collect()
}

fn dialect(delimiter: u8, crlf: bool) -> Dialect {
    Dialect {
        delimiter,
        quote: b'"',
        header: false,
        crlf,
    }
}

/// A record of one empty field is an empty line, which both readers
/// skip: compared without them.
fn without_empty(recs: Vec<Vec<String>>) -> Vec<Vec<String>> {
    recs.into_iter()
        .filter(|r| !(r.len() == 1 && r[0].is_empty()))
        .collect()
}

#[test]
#[allow(clippy::print_stderr)]
fn conformance() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut valid, mut valid_ok) = (0, 0);
    let (mut quoted, mut quoted_ok) = (0, 0);
    let (mut edits, mut edits_ok) = (0, 0);
    let mut bad = Vec::new();
    for case in 0..5000 {
        let delimiter = b",;\t|"[rng.below(4)];
        let crlf = rng.below(2) == 0;
        let recs = records(&mut rng, delimiter);
        let text = write(&recs, delimiter, crlf, rng.below(4) == 0);
        let d = dialect(delimiter, crlf);
        // 1. A valid file, read field for field.
        valid += 1;
        let got = without_empty(rows(&text, &d));
        if got == without_empty(recs.clone()) {
            valid_ok += 1;
        } else if bad.len() < 5 {
            bad.push(format!(
                "read {case}: {text:?}\n  want {recs:?}\n  got  {got:?}"
            ));
        }
        // 2. A value Kalem quotes, as the reference reads it.
        let v = value(&mut rng, delimiter);
        quoted += 1;
        let enc = encode(&v, &d);
        let back = read(&format!("{enc}\n"), delimiter);
        if back
            .as_ref()
            .and_then(|r| r.first())
            .and_then(|r| r.first())
            == Some(&v)
            || (v.is_empty() && back.as_ref().is_some_and(|r| r.is_empty()))
        {
            quoted_ok += 1;
        } else if bad.len() < 5 {
            bad.push(format!("encode {case}: {v:?} -> {enc:?} -> {back:?}"));
        }
        // 3. A cell set by Kalem: the file reads back with it.
        let row = rng.below(recs.len());
        let col = rng.below(recs[row].len());
        let nv = format!("n{}", value(&mut rng, delimiter));
        let mut idx = Index::new(&text);
        if let Some(rec) = idx.record(&text, row, &d) {
            edits += 1;
            let tx = set_cell(&text, &rec, col, &nv, &d);
            let after = tx.apply(&text);
            let mut want = recs.clone();
            want[row][col] = nv.clone();
            match read(&after, delimiter).map(|r| (after.clone(), r)) {
                Some((_, r)) if without_empty(r.clone()) == without_empty(want.clone()) => {
                    edits_ok += 1;
                }
                other => {
                    if bad.len() < 5 {
                        bad.push(format!(
                            "edit {case}: {text:?} [{row},{col}]={nv:?}\n  got {other:?}"
                        ));
                    }
                }
            }
        }
    }
    // 5. The delimiter found from the text alone, in files of two columns
    // or more and two records or more.
    let (mut found, mut found_ok) = (0, 0);
    let mut wrong = Vec::new();
    for _ in 0..5000 {
        let delimiter = b",;\t|"[rng.below(4)];
        let recs = records(&mut rng, delimiter);
        if recs.len() < 2 || recs[0].len() < 2 {
            continue;
        }
        let text = write(&recs, delimiter, false, false);
        found += 1;
        if detect(&text).delimiter == delimiter {
            found_ok += 1;
        } else if wrong.len() < 5 {
            wrong.push(format!(
                "{:?} found {:?}: {text:?}",
                delimiter as char,
                detect(&text).delimiter as char
            ));
        }
    }
    // 4. Arbitrary text, malformed too: agreement, reported.
    let (mut free, mut free_ok) = (0, 0);
    for _ in 0..5000 {
        const BYTES: [&str; 8] = ["a", ",", "\"", "\n", "\r\n", " ", "é", "\"\""];
        let text: String = (0..rng.below(20)).map(|_| BYTES[rng.below(8)]).collect();
        let d = dialect(b',', false);
        if let Some(want) = read(&text, b',') {
            free += 1;
            if without_empty(rows(&text, &d)) == without_empty(want) {
                free_ok += 1;
            }
        }
    }
    let pct = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
    eprintln!(
        "CSV conformance: read {valid_ok}/{valid} ({:.2}%), quote {quoted_ok}/{quoted} ({:.2}%), edit {edits_ok}/{edits} ({:.2}%); any text {free_ok}/{free} ({:.2}%)",
        pct(valid_ok, valid),
        pct(quoted_ok, quoted),
        pct(edits_ok, edits),
        pct(free_ok, free)
    );
    eprintln!(
        "CSV delimiter found: {found_ok}/{found} ({:.2}%)",
        pct(found_ok, found)
    );
    for b in bad.iter().chain(&wrong) {
        eprintln!("{b}");
    }
    assert!(
        valid_ok == valid && quoted_ok == quoted && edits_ok == edits,
        "{} differences",
        bad.len()
    );
    // The delimiter, found from the text alone: 99.9% at least.
    assert!(found_ok * 1000 >= found * 999, "{found_ok}/{found}");
}
