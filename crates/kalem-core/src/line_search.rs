//! The live search of lines (Doom's `SPC s b`, consult-line; T2.7i.4):
//! every line of one or more documents that holds all the words typed,
//! in any order, in the documents' order, recomputed on each keystroke.
//! Lower-case words match either case; a word with a capital matches
//! exactly (smart case).

use std::sync::Arc;

/// A document searched: the frontend's index for it, its name and text.
#[derive(Debug, Clone)]
pub struct Source {
    /// The frontend's index of the document.
    pub doc: usize,
    /// Its name, shown with each line when several are searched.
    pub name: String,
    /// Its text.
    pub text: Arc<str>,
}

/// Which lines take part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lines {
    /// Every line.
    All,
    /// Headings only (Org, Markdown, LaTeX sections): `SPC s i`.
    Headings,
}

/// A matching line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineHit {
    /// The index in [`LineSearch::sources`].
    pub source: usize,
    /// The line, from 0.
    pub line: usize,
    /// Where the line starts, and where the first word matched (bytes).
    pub start: usize,
    /// The first match's byte offset in the document.
    pub at: usize,
    /// The line's text, trimmed and cut when very long.
    pub text: String,
}

/// At most this many lines are listed.
const MAX_HITS: usize = 2000;

/// A search through the lines of some documents.
#[derive(Debug, Clone)]
pub struct LineSearch {
    /// The documents.
    pub sources: Vec<Source>,
    /// Which lines.
    pub lines: Lines,
    /// The matching lines, in order.
    pub hits: Vec<LineHit>,
    query: String,
    /// Line starts of each source, and which lines are headings.
    index: Vec<Vec<(usize, bool)>>,
}

fn is_heading(line: &str) -> bool {
    let t = line.trim_start();
    let stars = line.bytes().take_while(|b| *b == b'*').count();
    (stars > 0 && line.as_bytes().get(stars) == Some(&b' '))
        || (t.starts_with('#') && t.trim_start_matches('#').starts_with(' '))
        || [
            "\\part",
            "\\chapter",
            "\\section",
            "\\subsection",
            "\\subsubsection",
            "\\paragraph",
        ]
        .iter()
        .any(|c| {
            t.strip_prefix(c)
                .is_some_and(|r| r.starts_with(['{', '*', '[']))
        })
}

impl LineSearch {
    /// A search of `sources`, with `query` typed already (the word at
    /// point, say).
    pub fn new(sources: Vec<Source>, lines: Lines, query: &str) -> LineSearch {
        let index = sources
            .iter()
            .map(|s| {
                let mut v = Vec::new();
                let mut start = 0;
                for l in s.text.split_inclusive('\n') {
                    v.push((start, is_heading(l)));
                    start += l.len();
                }
                v
            })
            .collect();
        let mut s = LineSearch {
            sources,
            lines,
            hits: Vec::new(),
            query: String::new(),
            index,
        };
        s.query = "\u{0}".into();
        s.set_text(query);
        s
    }

    /// Several documents are searched, so lines name theirs.
    pub fn several(&self) -> bool {
        self.sources.len() > 1
    }

    /// Changes the query and finds the lines again.
    pub fn set_text(&mut self, query: &str) {
        if self.query == query {
            return;
        }
        self.query = query.to_string();
        let words: Vec<(String, bool)> = query
            .split_whitespace()
            .map(|w| {
                let exact = w.chars().any(char::is_uppercase);
                (
                    if exact {
                        w.to_string()
                    } else {
                        w.to_lowercase()
                    },
                    exact,
                )
            })
            .collect();
        self.hits.clear();
        let mut lower = String::new();
        'all: for (si, src) in self.sources.iter().enumerate() {
            let text = &*src.text;
            for (li, &(start, heading)) in self.index[si].iter().enumerate() {
                if self.lines == Lines::Headings && !heading {
                    continue;
                }
                let end = self.index[si].get(li + 1).map_or(text.len(), |n| n.0);
                let line = text[start..end].trim_end_matches(['\n', '\r']);
                if words.is_empty() && self.lines == Lines::All {
                    // Nothing typed: nothing listed but headings.
                    continue;
                }
                let mut first = None;
                let mut ok = true;
                lower.clear();
                for (w, exact) in &words {
                    let found = if *exact {
                        line.find(w.as_str())
                    } else {
                        if lower.is_empty() {
                            lower = line.to_lowercase();
                        }
                        // Lower-casing may change lengths: map back only
                        // when it did not.
                        lower
                            .find(w.as_str())
                            .map(|i| if lower.len() == line.len() { i } else { 0 })
                    };
                    match found {
                        Some(i) => first = Some(first.map_or(i, |f: usize| f.min(i))),
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if !ok {
                    continue;
                }
                let mut shown: String = line.trim().chars().take(200).collect();
                if shown.is_empty() {
                    shown.push(' ');
                }
                self.hits.push(LineHit {
                    source: si,
                    line: li,
                    start,
                    at: start + first.unwrap_or(0),
                    text: shown,
                });
                if self.hits.len() >= MAX_HITS {
                    break 'all;
                }
            }
        }
    }

    /// The query.
    pub fn text(&self) -> &str {
        &self.query
    }

    /// A hit as the list shows it: the line, and its number (with the
    /// document's name when several are searched).
    pub fn row(&self, hit: &LineHit) -> (String, String) {
        let n = hit.line + 1;
        let place = if self.several() {
            format!("{}:{n}", self.sources[hit.source].name)
        } else {
            n.to_string()
        };
        (hit.text.clone(), place)
    }

    /// The hit nearest after byte `at` of source `source` (for the list to
    /// start where the cursor is).
    pub fn nearest(&self, source: usize, at: usize) -> usize {
        self.hits
            .iter()
            .position(|h| h.source == source && h.at >= at)
            .or_else(|| self.hits.iter().rposition(|h| h.source == source))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(doc: usize, name: &str, text: &str) -> Source {
        Source {
            doc,
            name: name.into(),
            text: text.into(),
        }
    }

    #[test]
    fn words_in_any_order_smart_case() {
        let text = "* Plans\nbuy milk and bread\nBread is baked\nno match here\n** Milk\n";
        let mut s = LineSearch::new(vec![src(0, "a.org", text)], Lines::All, "");
        assert!(s.hits.is_empty());
        s.set_text("bread milk");
        assert_eq!(s.hits.len(), 1);
        assert_eq!(s.hits[0].line, 1);
        assert_eq!(s.hits[0].at, text.find("milk").unwrap());
        s.set_text("bread");
        assert_eq!(s.hits.iter().map(|h| h.line).collect::<Vec<_>>(), [1, 2]);
        s.set_text("Bread");
        assert_eq!(s.hits.iter().map(|h| h.line).collect::<Vec<_>>(), [2]);
        assert_eq!(s.row(&s.hits[0]), ("Bread is baked".into(), "3".into()));
        // Headings only, all of them with nothing typed.
        let h = LineSearch::new(vec![src(0, "a.org", text)], Lines::Headings, "");
        assert_eq!(h.hits.iter().map(|h| h.line).collect::<Vec<_>>(), [0, 4]);
        // Several documents name theirs.
        let s = LineSearch::new(
            vec![src(0, "a.org", text), src(3, "b.md", "# Milk\nmilk\n")],
            Lines::All,
            "milk",
        );
        assert_eq!(s.hits.len(), 4);
        assert_eq!(s.row(&s.hits[3]).1, "b.md:2");
        assert_eq!(s.nearest(1, 0), 2);
    }

    #[test]
    fn a_megabyte_within_the_keystroke_budget() {
        let line = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do.\n";
        let mut text = line.repeat(1_000_000 / line.len());
        text.push_str("the needle\n");
        let mut s = LineSearch::new(vec![src(0, "big", &text)], Lines::All, "");
        // Every line read to the end: one match, the last line.
        let t = std::time::Instant::now();
        for q in ["n", "ne", "nee", "need", "needl", "needle"] {
            s.set_text(q);
        }
        let each = t.elapsed() / 6;
        assert_eq!(s.hits.len(), 1);
        // The budget is 16 ms in release builds; debug builds get more.
        assert!(each.as_millis() < 200, "{each:?}");
    }
}
