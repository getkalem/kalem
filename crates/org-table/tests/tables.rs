//! Tables recalculated here and by Emacs (`tests/corpus/tables/*.org`,
//! with `tests/emacs/table.el` giving `tests/tables/*.expected`): every
//! field must agree, and so must the tables Emacs refuses.

#![allow(clippy::print_stderr)]

use org_table::formula::{DurationCustom, Env, NoRemote, Remote};
use org_table::recalc;
use org_table::table::{Row, Table};
use org_table::tblfm;

/// The tables of an Org file with their `#+NAME` and the text after them.
fn tables(text: &str) -> Vec<(Option<String>, Table, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_start().starts_with('|') {
            let start = i;
            while i < lines.len() && lines[i].trim_start().starts_with('|') {
                i += 1;
            }
            let name = start
                .checked_sub(1)
                .and_then(|p| {
                    let l = lines[p].trim();
                    let low = l.to_ascii_lowercase();
                    low.strip_prefix("#+name:")
                        .or_else(|| low.strip_prefix("#+tblname:"))
                        .map(|_| l.split_once(':').map(|(_, v)| v.trim().to_string()))
                })
                .flatten();
            let table = Table::parse(&lines[start..i].join("\n"));
            let after = lines[i..].join("\n");
            out.push((name, table, after));
        } else {
            i += 1;
        }
    }
    out
}

struct Named(Vec<(String, Table)>);

impl Remote for Named {
    fn table(&self, name: &str) -> Option<Table> {
        self.0
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, t)| t.clone())
    }
}

fn constants(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for l in text.lines() {
        let t = l.trim();
        if t.len() > 12 && t.as_bytes()[..12].eq_ignore_ascii_case(b"#+constants:") {
            for pair in t[12..].split_whitespace() {
                if let Some((k, v)) = pair.split_once('=') {
                    out.push((k.to_string(), v.to_string()));
                }
            }
        }
    }
    out
}

fn dump(t: &Table) -> Vec<String> {
    t.rows
        .iter()
        .map(|r| match r {
            Row::Rule => "RULE".to_string(),
            Row::Data(f) => f.join("\t"),
        })
        .collect()
}

fn run(name: &str) -> (usize, Vec<String>) {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let text = std::fs::read_to_string(format!("{root}/tests/corpus/tables/{name}.org")).unwrap();
    let expected = std::fs::read_to_string(format!(
        "{}/tests/tables/{name}.expected",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let expected: Vec<Vec<String>> = expected
        .split("\n\n")
        .filter(|b| !b.trim().is_empty())
        .map(|b| b.lines().map(str::to_string).collect())
        .collect();
    let all = tables(&text);
    let remote = Named(
        all.iter()
            .filter_map(|(n, t, _)| n.clone().map(|n| (n, t.clone())))
            .collect(),
    );
    let consts = constants(&text);
    let no_property = |_: &str| None;
    let env = Env {
        remote: &remote,
        constants: &consts,
        property: &no_property,
        duration_custom: DurationCustom::default(),
    };
    let _ = NoRemote;
    let mut got = Vec::new();
    // Fields of Emacs Lisp formulas, which Kalem leaves as they were.
    let mut lisp: Vec<Vec<(usize, usize)>> = Vec::new();
    for (_, table, after) in &all {
        let Some((_, value)) = tblfm::active_line(after) else {
            continue;
        };
        let eqs = tblfm::parse(value).equations;
        match recalc::recalculate(table, &eqs, &env) {
            Ok((t, report)) => {
                let a = org_table::table::Analysis::of(&t);
                let mut cells = Vec::new();
                for l in &report.lisp {
                    if let Some(c) = l.strip_prefix('$').and_then(|c| c.parse::<usize>().ok()) {
                        cells.extend((0..t.rows.len()).map(|r| (r, c)));
                    } else if let Some((r, c)) = l.strip_prefix('@').and_then(|x| x.split_once('$'))
                        && let (Ok(r), Ok(c)) = (r.parse::<usize>(), c.parse::<usize>())
                    {
                        cells.push((a.dlines[r], c));
                    }
                }
                // Kalem kept them: they must be as in the input.
                for &(r, c) in &cells {
                    assert_eq!(
                        t.field(r, c),
                        table.field(r, c),
                        "{name}: a Lisp formula changed a field"
                    );
                }
                lisp.push(cells);
                got.push(dump(&t));
            }
            Err(e) => {
                lisp.push(Vec::new());
                got.push(vec![format!("ERROR {e}")]);
            }
        }
    }
    let mut wrong = Vec::new();
    for (i, (g, e)) in got.iter().zip(&expected).enumerate() {
        let masked = |rows: &[String]| -> Vec<String> {
            rows.iter()
                .enumerate()
                .map(|(r, l)| {
                    l.split('\t')
                        .enumerate()
                        .map(|(c, f)| {
                            if lisp[i].contains(&(r, c + 1)) {
                                "LISP"
                            } else {
                                f
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\t")
                })
                .collect()
        };
        let same = if e.first().is_some_and(|l| l.starts_with("ERROR")) {
            g.first().is_some_and(|l| l.starts_with("ERROR"))
        } else {
            masked(g) == masked(e)
        };
        let known = KNOWN.iter().any(|(n, t, _)| *n == name && *t == i + 1);
        if !same && !known {
            wrong.push(format!(
                "{name} table {}:\n  emacs {e:?}\n  kalem {g:?}",
                i + 1
            ));
        }
    }
    if got.len() != expected.len() {
        wrong.push(format!(
            "{name}: {} tables, Emacs {}",
            got.len(),
            expected.len()
        ));
    }
    (got.len(), wrong)
}

/// Tables whose results differ, with the reason.
const KNOWN: &[(&str, usize, &str)] = &[(
    "random-3",
    64,
    "the standard deviation of dates goes through dates near year 0, which Calc writes as <+4-01-02 Fri>",
)];

#[test]
fn agree_with_emacs() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/tables");
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            e.ok()?
                .file_name()
                .to_str()?
                .strip_suffix(".expected")
                .map(str::to_string)
        })
        .collect();
    names.sort();
    let mut total = 0;
    let mut wrong = Vec::new();
    for n in &names {
        let (count, w) = run(n);
        total += count;
        wrong.extend(w);
    }
    for w in &wrong {
        eprintln!("{w}");
    }
    eprintln!("{} of {total} tables differ", wrong.len());
    assert!(wrong.is_empty());
}
