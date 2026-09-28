//! Style inference (design §6.2): the conventions a document already
//! follows, so that commands can produce new content in the same style.
//!
//! Emacs does not infer these; it reads variables such as
//! `org-adapt-indentation` and `org-tags-column`. [`Style::default`] holds
//! the values of `emacs -Q`, and [`Style::infer`] replaces each with what
//! most of the document does. A convention the document does not show
//! keeps its default.

use std::collections::HashMap;

use org_syntax::{NodeOrToken, Parse, SyntaxKind, SyntaxNode};

use crate::buffer::{column_at, string_width};

/// Letter case of `#+` keywords or block delimiters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Case {
    /// `#+title:`, `#+begin_src`.
    Lower,
    /// `#+TITLE:`, `#+BEGIN_SRC`.
    Upper,
}

/// How text under a headline is indented (`org-adapt-indentation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indentation {
    /// Nothing is indented (`nil`, the default since Org 9.5).
    Flat,
    /// Planning lines and drawers are indented to the headline's text, the
    /// body is not (`headline-data`).
    HeadlineData,
    /// Everything is indented to the headline's text (`t`).
    Full,
}

/// The conventions of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style {
    /// Indentation of text under headlines.
    pub indentation: Indentation,
    /// `#+STARTUP: indent`: the view indents text visually
    /// (`org-indent-mode`); the text itself is not indented.
    pub startup_indented: bool,
    /// Headlines are preceded by a blank line.
    pub blank_before_heading: bool,
    /// Case of keywords such as `#+title:`.
    pub keyword_case: Case,
    /// Case of block delimiters such as `#+begin_src`.
    pub block_case: Case,
    /// Indentation of source block contents relative to `#+begin_src`
    /// (`org-edit-src-content-indentation`).
    pub src_content_indentation: usize,
    /// Tags alignment (`org-tags-column`): tags end at this column when
    /// negative, start at it when positive.
    pub tags_column: isize,
    /// The first keyword of the first TODO sequence.
    pub todo_keyword: String,
    /// The first done keyword of the first TODO sequence.
    pub done_keyword: String,
    /// Lines end with `\r\n`.
    pub crlf: bool,
}

impl Default for Style {
    /// The conventions of `emacs -Q` with Org 9.7.
    fn default() -> Style {
        Style {
            indentation: Indentation::Flat,
            startup_indented: false,
            blank_before_heading: false,
            keyword_case: Case::Lower,
            block_case: Case::Lower,
            src_content_indentation: 2,
            tags_column: crate::headline::TAGS_COLUMN,
            todo_keyword: "TODO".into(),
            done_keyword: "DONE".into(),
            crlf: false,
        }
    }
}

/// Counts of what a document does, by value.
#[derive(Debug)]
struct Votes<T>(HashMap<T, usize>);

impl<T: std::hash::Hash + Eq + Copy + Ord> Votes<T> {
    fn new() -> Self {
        Votes(HashMap::new())
    }

    fn add(&mut self, v: T) {
        *self.0.entry(v).or_default() += 1;
    }

    /// The most frequent value and its count; ties go to the smallest
    /// value, so that the result does not depend on hashing.
    fn winner(&self) -> Option<(T, usize)> {
        self.0
            .iter()
            .map(|(v, n)| (*v, *n))
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
    }

    /// The majority value, if one value has more votes than every other.
    fn majority(&self) -> Option<T> {
        let (v, n) = self.winner()?;
        (self.0.iter().filter(|(_, m)| **m == n).count() == 1).then_some(v)
    }
}

fn case_of(s: &str) -> Option<Case> {
    let letters = s.chars().filter(|c| c.is_alphabetic());
    let (mut lower, mut upper) = (false, false);
    for c in letters {
        lower |= c.is_lowercase();
        upper |= c.is_uppercase();
    }
    match (lower, upper) {
        (true, false) => Some(Case::Lower),
        (false, true) => Some(Case::Upper),
        _ => None,
    }
}

fn key_token(node: &SyntaxNode) -> Option<String> {
    node.children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|t| t.kind() == SyntaxKind::KEY)
        .map(|t| t.text().to_string())
}

/// Width of the spaces and tabs at the start of the line at `bol`.
fn indent_at(text: &str, bol: usize) -> usize {
    let n = text[bol..]
        .bytes()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .count();
    column_at(text, bol + n) - column_at(text, bol)
}

impl Style {
    /// The style of a document: each convention as most of the document
    /// follows it, or its default.
    pub fn infer(parse: &Parse) -> Style {
        Style::infer_in(parse, &parse.syntax().to_string())
    }

    /// [`Style::infer`] given the text of `parse`.
    pub(crate) fn infer_in(parse: &Parse, text: &str) -> Style {
        use SyntaxKind::*;
        let root = parse.syntax();
        let ctx = parse.context();
        let mut style = Style::default();

        let mut keyword_case = Votes::new();
        let mut block_case = Votes::new();
        let mut data = Votes::new();
        let mut body = Votes::new();
        let mut blank = Votes::new();
        let mut src = Votes::new();
        let (mut tag_end, mut tag_start) = (Votes::new(), Votes::new());
        for node in root.descendants() {
            match node.kind() {
                KEYWORD | AFFILIATED_KEYWORD | BABEL_CALL => {
                    if let Some(key) = key_token(&node) {
                        if let Some(c) = case_of(&key) {
                            keyword_case.add(c);
                        }
                        if key.eq_ignore_ascii_case("startup") {
                            for opt in node.text().to_string().split_whitespace() {
                                match opt {
                                    "indent" => style.startup_indented = true,
                                    "noindent" => style.startup_indented = false,
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                BLOCK_BEGIN => {
                    let key = key_token(&node).unwrap_or_default();
                    if key.len() > 5
                        && key
                            .get(..5)
                            .is_some_and(|b| b.eq_ignore_ascii_case("begin"))
                        && let Some(c) = case_of(&key)
                    {
                        block_case.add(c);
                    }
                }
                SRC_BLOCK => {
                    if let Some(v) = src_indentation(&node, text) {
                        src.add(v);
                    }
                }
                HEADLINE => {
                    let start = usize::from(node.text_range().start());
                    if start > 0 {
                        let prev_bol = text[..start - 1].rfind('\n').map_or(0, |i| i + 1);
                        blank.add(
                            text[prev_bol..start - 1]
                                .trim_matches([' ', '\t', '\r'])
                                .is_empty(),
                        );
                    }
                    let stars = text[start..].bytes().take_while(|b| *b == b'*').count();
                    let section = node.children().find(|c| c.kind() == SECTION);
                    for el in section.iter().flat_map(|s| s.children()) {
                        let bol = usize::from(el.text_range().start());
                        let indent = indent_at(text, bol);
                        let vote = if indent == 0 {
                            false
                        } else if indent > stars {
                            true
                        } else {
                            continue;
                        };
                        match el.kind() {
                            PLANNING | PROPERTY_DRAWER | DRAWER => data.add(vote),
                            _ => body.add(vote),
                        }
                    }
                    if let Some(tags) = node
                        .children_with_tokens()
                        .filter_map(NodeOrToken::into_token)
                        .find(|t| t.kind() == TAGS)
                    {
                        let at = usize::from(tags.text_range().start());
                        let gap = text[start..at].len()
                            - text[start..at].trim_end_matches([' ', '\t']).len();
                        if gap >= 2 {
                            let col = column_at(text, at) - column_at(text, start);
                            tag_start.add(col);
                            tag_end.add(col + string_width(tags.text()));
                        }
                    }
                }
                _ => {}
            }
        }

        if let Some(c) = keyword_case.majority() {
            style.keyword_case = c;
        }
        if let Some(c) = block_case.majority() {
            style.block_case = c;
        }
        if let Some(v) = src.majority() {
            style.src_content_indentation = v;
        }
        if let Some(b) = blank.majority() {
            style.blank_before_heading = b;
        }
        style.indentation = if body.majority() == Some(true) {
            Indentation::Full
        } else if data.majority() == Some(true) {
            Indentation::HeadlineData
        } else {
            Indentation::Flat
        };
        // Aligned tags share an end column (negative `org-tags-column`) or
        // a start column (positive); one headline alone does not tell.
        let (end, start) = (tag_end.winner(), tag_start.winner());
        match (end, start) {
            (Some((e, n)), Some((s, m))) if n.max(m) >= 2 => {
                style.tags_column = if n >= m { -(e as isize) } else { s as isize };
            }
            _ => {}
        }

        if let Some(seq) = ctx.todo_sequences.first() {
            if let Some(k) = seq.keywords.iter().find(|k| !k.done) {
                style.todo_keyword = k.name.clone();
            }
            if let Some(k) = seq.keywords.iter().find(|k| k.done) {
                style.done_keyword = k.name.clone();
            }
        }
        let lines = text.matches('\n').count();
        style.crlf = lines > 0 && 2 * text.matches("\r\n").count() > lines;
        style
    }
}

/// The indentation of a source block's contents relative to its
/// `#+begin_src` line, unless the block preserves indentation (`-i`).
fn src_indentation(node: &SyntaxNode, text: &str) -> Option<usize> {
    let begin = node
        .children()
        .find(|c| c.kind() == SyntaxKind::BLOCK_BEGIN)?;
    let bol = usize::from(begin.text_range().start());
    let params = begin.text().to_string();
    if params.split_whitespace().any(|w| w == "-i") {
        return None;
    }
    let code = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|t| t.kind() == SyntaxKind::CODE_TEXT)?;
    let start = usize::from(code.text_range().start());
    let mut min: Option<usize> = None;
    let mut pos = start;
    for line in code.text().split_inclusive('\n') {
        if !line.trim().is_empty() {
            let i = indent_at(text, pos);
            min = Some(min.map_or(i, |m| m.min(i)));
        }
        pos += line.len();
    }
    min?.checked_sub(indent_at(text, bol))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn infer(text: &str) -> Style {
        Style::infer(&org_syntax::parse(text))
    }

    #[test]
    fn defaults_without_evidence() {
        assert_eq!(infer(""), Style::default());
        assert_eq!(infer("* One\nText.\n"), Style::default());
    }

    #[test]
    fn casing() {
        let s = infer("#+TITLE: T\n#+AUTHOR: A\n#+date: d\n#+BEGIN_SRC sh\nls\n#+END_SRC\n");
        assert_eq!(s.keyword_case, Case::Upper);
        assert_eq!(s.block_case, Case::Upper);
        // A tie keeps the default.
        let s = infer("#+TITLE: T\n#+date: d\n");
        assert_eq!(s.keyword_case, Case::Lower);
    }

    #[test]
    fn indentation() {
        let full = "* A\n  SCHEDULED: <2026-01-01>\n  Body.\n** B\n   Text.\n";
        assert_eq!(infer(full).indentation, Indentation::Full);
        let data =
            "* A\n  :PROPERTIES:\n  :X: 1\n  :END:\nBody.\n* B\n  SCHEDULED: <2026-01-01>\nText.\n";
        assert_eq!(infer(data).indentation, Indentation::HeadlineData);
        assert_eq!(
            infer("* A\nSCHEDULED: <2026-01-01>\nBody.\n").indentation,
            Indentation::Flat
        );
    }

    #[test]
    fn blank_lines_and_startup() {
        let s = infer("#+STARTUP: indent\n\n* A\ntext\n\n* B\n\n* C\n");
        assert!(s.blank_before_heading);
        assert!(s.startup_indented);
        assert!(!infer("#+STARTUP: indent\n#+STARTUP: noindent\n* A\n* B\n").startup_indented);
    }

    #[test]
    fn tags_column() {
        let right = format!(
            "* A{}:x:\n* Longer title{}:yz:\n",
            " ".repeat(71),
            " ".repeat(59)
        );
        assert_eq!(infer(&right).tags_column, -77);
        let left = "* A          :x:\n* Title      :yz:\n";
        assert_eq!(infer(left).tags_column, 13);
        let right60 = format!("* A{}:x:\n* B{}:y:\n", " ".repeat(54), " ".repeat(54));
        assert_eq!(infer(&right60).tags_column, -60);
        // Unaligned tags say nothing.
        assert_eq!(infer("* A :x:\n* B :y:\n").tags_column, -77);
    }

    #[test]
    fn todo_src_and_line_endings() {
        let s = infer("#+TODO: NEXT WAIT | FINISHED\n#+begin_src sh\nls\n#+end_src\n");
        assert_eq!(
            (s.todo_keyword.as_str(), s.done_keyword.as_str()),
            ("NEXT", "FINISHED")
        );
        assert_eq!(s.src_content_indentation, 0);
        assert_eq!(
            infer("  #+begin_src sh\n      ls\n  #+end_src\n").src_content_indentation,
            4
        );
        assert_eq!(
            infer("#+begin_src sh -i\nls\n#+end_src\n").src_content_indentation,
            2
        );
        assert!(infer("* A\r\ntext\r\n").crlf);
    }
}
