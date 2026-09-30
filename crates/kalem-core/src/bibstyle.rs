//! Citations as BibTeX styles print them: the labels of the standard
//! styles (`plain`, `abbrv`, `unsrt`, `ieeetr` and the other numeric
//! ones; `alpha`) and of natbib's author-year styles (`plainnat`,
//! `abbrvnat`, `unsrtnat`), computed the way their `.bst` files compute
//! them: the sort keys of `presort`, `format.lab.names`, the extra
//! letters of `forward.pass`. `tests/latex/citations` holds what
//! pdflatex and bibtex print for each.

use std::collections::HashMap;

use org_cite::{Bibliography, Entry};

/// How a document's citations are labeled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Numbers in the order of the sorted bibliography (`plain`,
    /// `abbrv`), or of first citation (`unsrt`, `ieeetr`).
    Numeric { sorted: bool, abbreviated: bool },
    /// `alpha`: `Knu84a`.
    Alpha,
    /// natbib's author-year styles: `Knuth [1984a]`.
    AuthorYear,
}

/// The kind of `\bibliographystyle{style}`, or `None` for a style Kalem
/// does not label (biblatex, the rest).
pub fn kind(style: &str) -> Option<Kind> {
    Some(match style.trim().to_ascii_lowercase().as_str() {
        "plain" | "siam" | "amsplain" | "acm" => Kind::Numeric {
            sorted: true,
            abbreviated: false,
        },
        "abbrv" => Kind::Numeric {
            sorted: true,
            abbreviated: true,
        },
        "unsrt" | "ieeetr" | "ieeetran" => Kind::Numeric {
            sorted: false,
            abbreviated: false,
        },
        "alpha" | "amsalpha" => Kind::Alpha,
        "plainnat" | "abbrvnat" | "unsrtnat" => Kind::AuthorYear,
        _ => return None,
    })
}

/// A cited entry's label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// What a numeric or alpha citation shows: `5`, `Knu84a`.
    pub mark: String,
    /// The names of an author-year label: `Knuth`, `Lamport and Lynch`,
    /// `Ex et al.`.
    pub names: String,
    /// The year with its extra letter: `1984a`.
    pub year: String,
}

/// A BibTeX name's parts (`format.name$`'s `ff`, `vv`, `ll`, `jj`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Name {
    first: Vec<String>,
    von: Vec<String>,
    last: Vec<String>,
    jr: Vec<String>,
    others: bool,
}

/// Splits at `sep` outside braces.
fn split_top(s: &str, is_sep: impl Fn(&str, usize) -> Option<usize>) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start, mut i) = (0i32, 0, 0);
    let b = s.as_bytes();
    while i < b.len() {
        match b[i] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ if depth == 0 => {
                if let Some(len) = is_sep(s, i) {
                    out.push(&s[start..i]);
                    i += len;
                    start = i;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&s[start..]);
    out
}

/// A word starts in lower case (a von part), looking through braces
/// only for a special character (`{\"u}ber`).
fn lower(word: &str) -> bool {
    let w = word.trim_start_matches('{');
    let w = w.strip_prefix('\\').map_or(w, |r| {
        r.trim_start_matches(|c: char| c.is_ascii_alphabetic())
            .trim_start_matches([' ', '{'])
    });
    if word.starts_with('{') && !word.starts_with("{\\") {
        return false;
    }
    w.chars()
        .find(|c| c.is_alphabetic())
        .is_some_and(char::is_lowercase)
}

/// The names of a BibTeX `author` or `editor` field.
fn names(field: &str) -> Vec<Name> {
    let and = |s: &str, i: usize| {
        let rest = &s[i..];
        (rest.len() > 5
            && rest.as_bytes()[0].is_ascii_whitespace()
            && rest[1..4].eq_ignore_ascii_case("and")
            && rest.as_bytes()[4].is_ascii_whitespace())
        .then_some(5)
    };
    split_top(field, and)
        .into_iter()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(|n| {
            if n == "others" {
                return Name {
                    others: true,
                    ..Name::default()
                };
            }
            let words = |s: &str| -> Vec<String> {
                split_top(s, |s, i| {
                    matches!(s.as_bytes()[i], b' ' | b'\t' | b'\n' | b'~').then_some(1)
                })
                .into_iter()
                .filter(|w| !w.is_empty())
                .map(str::to_string)
                .collect()
            };
            let parts: Vec<&str> = split_top(n, |s, i| (s.as_bytes()[i] == b',').then_some(1));
            // `von Last` of the part before a comma.
            let von_last = |ws: Vec<String>| -> (Vec<String>, Vec<String>) {
                let k = ws.len();
                let end = (0..k.saturating_sub(1)).rev().find(|&i| lower(&ws[i]));
                match end {
                    Some(e) => {
                        let start = (0..=e).find(|&i| lower(&ws[i])).unwrap_or(0);
                        (ws[start..=e].to_vec(), ws[e + 1..].to_vec())
                    }
                    None => (Vec::new(), ws),
                }
            };
            match parts.len() {
                1 => {
                    let ws = words(parts[0]);
                    let k = ws.len();
                    match (0..k.saturating_sub(1)).find(|&i| lower(&ws[i])) {
                        Some(start) => {
                            let end = (start..k - 1)
                                .rev()
                                .find(|&i| lower(&ws[i]))
                                .unwrap_or(start);
                            Name {
                                first: ws[..start].to_vec(),
                                von: ws[start..=end].to_vec(),
                                last: ws[end + 1..].to_vec(),
                                ..Name::default()
                            }
                        }
                        None => Name {
                            first: ws[..k.saturating_sub(1)].to_vec(),
                            last: ws[k.saturating_sub(1)..].to_vec(),
                            ..Name::default()
                        },
                    }
                }
                2 => {
                    let (von, last) = von_last(words(parts[0]));
                    Name {
                        first: words(parts[1]),
                        von,
                        last,
                        ..Name::default()
                    }
                }
                _ => {
                    let (von, last) = von_last(words(parts[0]));
                    Name {
                        first: words(parts[2]),
                        von,
                        last,
                        jr: words(parts[1]),
                        others: false,
                    }
                }
            }
        })
        .collect()
}

/// A word without its braces and control sequences' backslashes, as it
/// prints: `{\TeX}book` is `TeXbook`, `van~den` `van den`.
fn plain_text(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '~' => out.push(' '),
            '\\' => {
                let word: String =
                    std::iter::from_fn(|| chars.next_if(|c| c.is_ascii_alphabetic())).collect();
                match word.as_str() {
                    // An accent before a letter: the letter stays.
                    "" => {
                        chars.next();
                    }
                    "c" | "u" | "v" | "H" | "r" | "k" | "d" | "b" | "t" | "i" | "j" => {
                        if word == "i" || word == "j" {
                            out.push_str(&word);
                        }
                    }
                    // A named letter or a logo: its name (`\TeX`, `\ss`).
                    w => out.push_str(w),
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// The first letter of a word (`format.name$`'s abbreviation).
fn initial(word: &str) -> String {
    plain_text(word)
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_string())
        .unwrap_or_default()
}

/// `purify$` and lower case (`sortify`): letters, digits and spaces;
/// hyphens and ties as spaces; control sequences dropped.
fn sortify(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                while chars.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                    chars.next();
                }
            }
            '-' | '~' => out.push(' '),
            c if c.is_alphanumeric() || c.is_whitespace() => out.extend(c.to_lowercase()),
            _ => {}
        }
    }
    out
}

/// `sort.format.names`: `{vv{ } }{ll{ }}{  ff{ }}{  jj{ }}` (with `f`
/// for the abbreviated styles), the names three blanks apart, "others"
/// as "et al".
fn sort_names(ns: &[Name], abbreviated: bool) -> String {
    let mut out = String::new();
    for (i, n) in ns.iter().enumerate() {
        if i > 0 {
            out.push_str("   ");
        }
        if n.others {
            out.push_str("et al");
            continue;
        }
        let mut t = String::new();
        if !n.von.is_empty() {
            t.push_str(&n.von.join(" "));
            t.push(' ');
        }
        t.push_str(&n.last.join(" "));
        if !n.first.is_empty() {
            t.push_str("  ");
            let first: Vec<String> = if abbreviated {
                n.first.iter().map(|w| initial(w)).collect()
            } else {
                n.first.clone()
            };
            t.push_str(&first.join(" "));
        }
        if !n.jr.is_empty() {
            t.push_str("  ");
            t.push_str(&n.jr.join(" "));
        }
        out.push_str(&sortify(&t));
    }
    out
}

/// `sort.format.title`: without a leading "A ", "An " or "The ".
fn sort_title(t: &str) -> String {
    let mut t = t;
    for w in ["A ", "An ", "The "] {
        if let Some(r) = t.strip_prefix(w) {
            t = r;
        }
    }
    sortify(t)
}

fn field<'a>(e: &'a Entry, name: &str) -> Option<&'a str> {
    e.field(name).filter(|v| !v.trim().is_empty())
}

/// The names a sort uses: `author.sort`, `author.editor.sort` for books.
fn sort_author(e: &Entry, abbreviated: bool) -> String {
    let kind = e.kind.to_ascii_lowercase();
    let who = match kind.as_str() {
        "book" | "inbook" => field(e, "author").or_else(|| field(e, "editor")),
        "proceedings" => field(e, "editor"),
        _ => field(e, "author"),
    };
    match who {
        Some(w) => sort_names(&names(w), abbreviated),
        None => field(e, "key").map(sortify).unwrap_or_default(),
    }
}

fn year(e: &Entry) -> String {
    field(e, "year").map(plain_text).unwrap_or_default()
}

/// `format.lab.names` of alpha.bst.
fn alpha_names(e: &Entry) -> String {
    let who = field(e, "author").or_else(|| field(e, "editor"));
    let Some(who) = who else {
        return field(e, "key")
            .map(|k| plain_text(k).chars().take(3).collect())
            .unwrap_or_else(|| e.key.chars().take(3).collect());
    };
    let ns = names(who);
    let initials = |n: &Name| -> String {
        n.von
            .iter()
            .chain(n.last.iter())
            .map(|w| initial(w))
            .collect::<String>()
    };
    if ns.len() > 1 {
        let shown = if ns.len() > 4 { 3 } else { ns.len() };
        let mut out = String::new();
        for (i, n) in ns[..shown].iter().enumerate() {
            if n.others && i + 1 == ns.len() {
                out.push('+');
            } else {
                out.push_str(&initials(n));
            }
        }
        if ns.len() > 4 {
            out.push('+');
        }
        out
    } else {
        let n = &ns[0];
        let i = initials(n);
        if i.chars().count() < 2 {
            plain_text(&n.last.join(" ")).chars().take(3).collect()
        } else {
            i
        }
    }
}

/// `format.lab.names` of natbib's styles: `Knuth`, `Lamport and Lynch`,
/// `Ex et al.`, `van den Berg`.
fn natbib_names(e: &Entry) -> String {
    let who = field(e, "author").or_else(|| field(e, "editor"));
    let Some(who) = who else {
        return field(e, "key")
            .map(plain_text)
            .unwrap_or_else(|| e.key.chars().take(3).collect());
    };
    let ns = names(who);
    let last = |n: &Name| -> String {
        let mut v = n.von.clone();
        v.extend(n.last.iter().cloned());
        plain_text(&v.join(" "))
    };
    match ns.len() {
        0 => String::new(),
        1 => last(&ns[0]),
        2 if ns[1].others => format!("{} et al.", last(&ns[0])),
        2 => format!("{} and {}", last(&ns[0]), last(&ns[1])),
        _ => format!("{} et al.", last(&ns[0])),
    }
}

/// The labels of the entries `cited` (keys in the order of their first
/// citation, `\nocite` included; keys the bibliography lacks left out).
pub fn labels(kind: Kind, cited: &[String], bib: &Bibliography) -> HashMap<String, Label> {
    let entries: Vec<&Entry> = cited.iter().filter_map(|k| bib.get(k)).collect();
    let mut out = HashMap::new();
    match kind {
        Kind::Numeric {
            sorted,
            abbreviated,
        } => {
            let mut order: Vec<(String, &Entry)> = entries
                .iter()
                .map(|e| {
                    let key = format!(
                        "{}    {}    {}",
                        sort_author(e, abbreviated),
                        sortify(&year(e)),
                        sort_title(field(e, "title").unwrap_or(""))
                    );
                    (key, *e)
                })
                .collect();
            if sorted {
                order.sort_by(|a, b| a.0.cmp(&b.0));
            }
            for (i, (_, e)) in order.iter().enumerate() {
                out.insert(
                    e.key.clone(),
                    Label {
                        mark: (i + 1).to_string(),
                        names: natbib_names(e),
                        year: year(e),
                    },
                );
            }
        }
        Kind::Alpha => {
            let mut order: Vec<(String, String, &Entry)> = entries
                .iter()
                .map(|e| {
                    let y = sortify(&year(e));
                    let two: String = y
                        .chars()
                        .rev()
                        .take(2)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    let label = format!("{}{two}", alpha_names(e));
                    let key = format!(
                        "{}    {}    {}    {}",
                        sortify(&label),
                        sort_author(e, false),
                        y,
                        sort_title(field(e, "title").unwrap_or(""))
                    );
                    (key, label, *e)
                })
                .collect();
            order.sort_by(|a, b| a.0.cmp(&b.0));
            let same = |i: usize, j: usize| order[i].1 == order[j].1;
            let mut letter = 0u8;
            for i in 0..order.len() {
                let shared = (i > 0 && same(i, i - 1)) || (i + 1 < order.len() && same(i, i + 1));
                letter = if i > 0 && same(i, i - 1) {
                    letter + 1
                } else {
                    0
                };
                let mut mark = order[i].1.clone();
                if shared {
                    mark.push((b'a' + letter) as char);
                }
                out.insert(
                    order[i].2.key.clone(),
                    Label {
                        mark,
                        names: natbib_names(order[i].2),
                        year: year(order[i].2),
                    },
                );
            }
        }
        Kind::AuthorYear => {
            let mut order: Vec<(String, String, &Entry)> = entries
                .iter()
                .map(|e| {
                    let names = natbib_names(e);
                    let key = format!(
                        "{}    {}    {}    {}",
                        sortify(&names),
                        sort_author(e, false),
                        sortify(&year(e)),
                        e.key
                    );
                    (key, names, *e)
                })
                .collect();
            order.sort_by(|a, b| a.0.cmp(&b.0));
            let tag = |i: usize| (order[i].1.clone(), year(order[i].2));
            let mut letter = 0u8;
            for i in 0..order.len() {
                let shared = (i > 0 && tag(i) == tag(i - 1))
                    || (i + 1 < order.len() && tag(i) == tag(i + 1));
                letter = if i > 0 && tag(i) == tag(i - 1) {
                    letter + 1
                } else {
                    0
                };
                let mut y = year(order[i].2);
                if shared {
                    y.push((b'a' + letter) as char);
                }
                out.insert(
                    order[i].2.key.clone(),
                    Label {
                        mark: String::new(),
                        names: order[i].1.clone(),
                        year: y,
                    },
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_parts() {
        let n = &names("Jan van den Berg")[0];
        assert_eq!(
            (n.first.clone(), n.von.clone(), n.last.clone()),
            (
                vec!["Jan".to_string()],
                vec!["van".to_string(), "den".into()],
                vec!["Berg".to_string()]
            )
        );
        let n = &names("Knuth, Donald E.")[0];
        assert_eq!(
            (n.first.len(), n.last.clone()),
            (2, vec!["Knuth".to_string()])
        );
        assert_eq!(names("A. Alpha and B. Beta AND C. Gamma").len(), 3);
        assert!(names("A. Alpha and others")[1].others);
    }
}
