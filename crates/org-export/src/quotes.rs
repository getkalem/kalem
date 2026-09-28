//! Smart quotes (`org-export-activate-smart-quotes`): straight quotes
//! turned into the quotation marks of the document's language, with
//! `#+OPTIONS: ':t`.

use crate::export::Exporter;
use crate::tree::Id;

/// What a quote character stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quote {
    /// `"` opening.
    PrimaryOpening,
    /// `"` closing.
    PrimaryClosing,
    /// `'` opening.
    SecondaryOpening,
    /// `'` closing.
    SecondaryClosing,
    /// `'` in a word.
    Apostrophe,
}

/// The output format of the marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// HTML entities.
    Html,
    /// UTF-8 characters.
    Utf8,
    /// LaTeX.
    Latex,
}

/// `org-export-smart-quotes-alist`: (opening, closing, single opening,
/// single closing, apostrophe) by language and encoding.
fn marks(lang: &str, enc: Encoding) -> Option<[&'static str; 5]> {
    use Encoding::*;
    Some(match (lang, enc) {
        ("en" | "it", Html) => ["&ldquo;", "&rdquo;", "&lsquo;", "&rsquo;", "&rsquo;"],
        ("en" | "it", Utf8) => ["“", "”", "‘", "’", "’"],
        ("en" | "it", Latex) => ["``", "''", "`", "'", "'"],
        ("de", Html) => ["&bdquo;", "&ldquo;", "&sbquo;", "&lsquo;", "&rsquo;"],
        ("de", Utf8) => ["„", "“", "‚", "‘", "’"],
        ("de", Latex) => ["\"`", "\"'", "\\glq{}", "\\grq{}", "'"],
        ("es", Html) => ["&laquo;", "&raquo;", "&ldquo;", "&rdquo;", "&rsquo;"],
        ("es", Utf8) => ["«", "»", "“", "”", "’"],
        ("es", Latex) => ["\\guillemotleft{}", "\\guillemotright{}", "``", "''", "'"],
        ("fr", Html) => [
            "&laquo;&nbsp;",
            "&nbsp;&raquo;",
            "&ldquo;",
            "&rdquo;",
            "&rsquo;",
        ],
        ("fr", Utf8) => ["« ", " »", "“", "”", "’"],
        ("fr", Latex) => ["\\og ", "\\fg{}", "``", "''", "'"],
        ("no" | "nb" | "nn", Html) => ["&laquo;", "&raquo;", "&lsquo;", "&rsquo;", "&rsquo;"],
        ("no" | "nb" | "nn", Utf8) => ["«", "»", "‘", "’", "’"],
        ("no" | "nb" | "nn", Latex) => ["\\guillemotleft{}", "\\guillemotright{}", "`", "'", "'"],
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Quote,
    Space,
    Open,
    Close,
    Symbol,
    Word,
    Punct,
}

/// A character's syntax class in an Org buffer.
fn class(c: char) -> Class {
    match c {
        '"' => Class::Quote,
        ' ' | '\t' | '\n' | '\r' | '\x0c' => Class::Space,
        '(' | '[' | '{' | '<' => Class::Open,
        ')' | ']' | '}' | '>' => Class::Close,
        '_' | '\\' | '~' => Class::Symbol,
        '\'' => Class::Word,
        c if c.is_alphanumeric() => Class::Word,
        c if c.is_whitespace() => Class::Space,
        _ => Class::Punct,
    }
}

/// The text before and after a quote: a character, or `blank`,
/// `no-blank` or nothing at the edges of the text.
#[derive(Clone, Copy)]
enum Side {
    Char(char),
    Blank,
    NoBlank,
    Nothing,
}

impl Exporter<'_> {
    /// The quote statuses of the plain text `id`, one per quote in it.
    fn quote_status(&mut self, id: Id) -> Vec<Option<Quote>> {
        let Some(parent) = self.tree.parent(id) else {
            return Vec::new();
        };
        let siblings: Vec<Id> = {
            let n = &self.tree.nodes[parent];
            if n.children.contains(&id) {
                n.children.clone()
            } else {
                n.secondary
                    .iter()
                    .find(|(_, v)| v.contains(&id))
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            }
        };
        // Statuses of every text among the siblings.
        let mut full: Vec<(Id, Vec<Option<Quote>>)> = Vec::new();
        let mut level1_open = false;
        for &t in &siblings {
            if !self.tree.is_text(t) || self.info.ignore.contains(&t) {
                continue;
            }
            let text: Vec<char> = self.tree.nodes[t].text.chars().collect();
            let mut status = Vec::new();
            for (i, &c) in text.iter().enumerate() {
                if c != '\'' && c != '"' {
                    continue;
                }
                let q = if c == '"' {
                    level1_open = !level1_open;
                    if level1_open {
                        Quote::PrimaryOpening
                    } else {
                        Quote::PrimaryClosing
                    }
                } else if !level1_open {
                    Quote::Apostrophe
                } else {
                    let previous = if i > 0 {
                        Side::Char(text[i - 1])
                    } else {
                        match self.previous_element(t) {
                            None => Side::Nothing,
                            Some(p) if self.tree.is_text(p) => self.tree.nodes[p]
                                .text
                                .chars()
                                .last()
                                .map_or(Side::Nothing, Side::Char),
                            Some(p) if self.tree.nodes[p].post_blank == 0 => Side::NoBlank,
                            Some(_) => Side::Blank,
                        }
                    };
                    let next = if i + 1 < text.len() {
                        Side::Char(text[i + 1])
                    } else {
                        match self.next_element(t) {
                            None => Side::Nothing,
                            Some(n) if self.tree.is_text(n) => self.tree.nodes[n]
                                .text
                                .chars()
                                .next()
                                .map_or(Side::Nothing, Side::Char),
                            Some(_) => Side::NoBlank,
                        }
                    };
                    let allow_open = (match previous {
                        Side::Char(p) => {
                            matches!(class(p), Class::Quote | Class::Space | Class::Open)
                        }
                        Side::Blank | Side::Nothing => true,
                        Side::NoBlank => false,
                    }) && (match next {
                        Side::Char(n) => {
                            matches!(class(n), Class::Word | Class::Punct | Class::Symbol)
                        }
                        Side::NoBlank => true,
                        _ => false,
                    });
                    let allow_close = (match previous {
                        Side::Char(p) => {
                            matches!(class(p), Class::Word | Class::Punct | Class::Symbol)
                        }
                        Side::NoBlank => true,
                        _ => false,
                    }) && (match next {
                        Side::Char(n) => {
                            matches!(
                                class(n),
                                Class::Space | Class::Close | Class::Punct | Class::Quote
                            )
                        }
                        Side::Blank | Side::Nothing => true,
                        Side::NoBlank => false,
                    });
                    match (allow_open, allow_close) {
                        (true, false) => Quote::SecondaryOpening,
                        (false, true) => Quote::SecondaryClosing,
                        _ => Quote::Apostrophe,
                    }
                };
                status.push(Some(q));
            }
            if !status.is_empty() {
                full.push((t, status));
            }
        }
        // Unbalanced quotes become apostrophes.
        let mut primary: Vec<(usize, usize)> = Vec::new();
        let mut secondary: Vec<(usize, usize)> = Vec::new();
        for i in 0..full.len() {
            for j in 0..full[i].1.len() {
                match full[i].1[j] {
                    Some(Quote::PrimaryOpening) => primary.push((i, j)),
                    Some(Quote::SecondaryOpening) => secondary.push((i, j)),
                    Some(Quote::SecondaryClosing) => {
                        if secondary.pop().is_none() {
                            full[i].1[j] = Some(Quote::Apostrophe);
                        }
                    }
                    Some(Quote::PrimaryClosing) => {
                        for (a, b) in secondary.drain(..) {
                            full[a].1[b] = Some(Quote::Apostrophe);
                        }
                        primary.pop();
                    }
                    _ => {}
                }
            }
        }
        if let Some(&(pi, pj)) = primary.last() {
            // A trailing unclosed `"` stays; single quotes after it are
            // apostrophes.
            full[pi].1[pj] = None;
            let mut after = false;
            for (i, (_, st)) in full.iter_mut().enumerate() {
                for (j, q) in st.iter_mut().enumerate() {
                    if (i, j) == (pi, pj) {
                        after = true;
                    }
                    if after && matches!(q, Some(Quote::SecondaryOpening | Quote::SecondaryClosing))
                    {
                        *q = Some(Quote::Apostrophe);
                    }
                }
            }
        }
        full.into_iter()
            .find(|(t, _)| *t == id)
            .map(|(_, s)| s)
            .unwrap_or_default()
    }

    /// `s` (text `id` after the back-end's escaping) with its quotes
    /// replaced.
    pub fn smart_quotes(&mut self, id: Option<Id>, s: &str, enc: Encoding) -> String {
        let Some(id) = id else {
            return s.to_string();
        };
        let lang = self.string("language").unwrap_or("en").to_string();
        let Some(m) = marks(&lang, enc) else {
            return s.to_string();
        };
        let status = self.quote_status(id);
        let mut it = status.into_iter();
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            if c == '\'' || c == '"' {
                let r = match it.next().flatten() {
                    Some(Quote::PrimaryOpening) => Some(m[0]),
                    Some(Quote::PrimaryClosing) => Some(m[1]),
                    Some(Quote::SecondaryOpening) => Some(m[2]),
                    Some(Quote::SecondaryClosing) => Some(m[3]),
                    Some(Quote::Apostrophe) => Some(m[4]),
                    None => None,
                };
                match r {
                    Some(r) => out.push_str(r),
                    None => out.push(c),
                }
            } else {
                out.push(c);
            }
        }
        out
    }
}
