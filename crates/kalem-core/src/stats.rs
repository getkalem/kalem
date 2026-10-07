//! Word counts for the status bars (T1.5.17): the words a reader sees in
//! the rich view.
//!
//! The title, headline titles, paragraphs, lists, tables, quotes and
//! verse count, as do inline code and links (their description, or their
//! target when they have none). Markup, TODO keywords, tags, bullets, other
//! keywords, drawers,
//! planning and clock lines, comments, source and example blocks, LaTeX,
//! timestamps and footnote labels do not. A word is a run of characters
//! without blanks that has a letter or digit in it; hidden markers join
//! what they separate (`un*bold*ed` is one word).

use std::ops::Range;

use org_edit::Transaction;
use org_syntax::{NodeOrToken, SyntaxKind, SyntaxNode};

/// Elements and objects whose text is not counted.
fn skipped(k: SyntaxKind) -> bool {
    use SyntaxKind::*;
    matches!(
        k,
        PLANNING
            | PROPERTY_DRAWER
            | DRAWER
            | CLOCK
            | COMMENT
            | COMMENT_BLOCK
            | SRC_BLOCK
            | EXAMPLE_BLOCK
            | EXPORT_BLOCK
            | FIXED_WIDTH
            | LATEX_ENVIRONMENT
            | KEYWORD
            | AFFILIATED_KEYWORD
            | BABEL_CALL
            | DIARY_SEXP
            | TIMESTAMP
            | STATISTICS_COOKIE
            | FOOTNOTE_REFERENCE
            | TARGET
            | LATEX_FRAGMENT
            | EXPORT_SNIPPET
            | INLINE_BABEL_CALL
            | INLINE_SRC_BLOCK
            | MACRO
            | CITATION
            | BLOCK_BEGIN
            | BLOCK_END
    )
}

/// `#+TITLE` and `#+SUBTITLE`, which the rich view shows as the title.
fn is_title(n: &SyntaxNode) -> bool {
    n.kind() == SyntaxKind::KEYWORD
        && n.children_with_tokens().any(|c| {
            c.as_token().is_some_and(|t| {
                t.kind() == SyntaxKind::KEY
                    && matches!(t.text().to_ascii_uppercase().as_str(), "TITLE" | "SUBTITLE")
            })
        })
}

fn visible(node: &SyntaxNode, range: &Range<usize>, out: &mut String) {
    let r = node.text_range();
    if usize::from(r.end()) <= range.start || usize::from(r.start()) >= range.end {
        return;
    }
    // A link shows its description, or its target without one.
    let described = node.kind() == SyntaxKind::LINK
        && node.children_with_tokens().any(|c| {
            c.as_token().is_some_and(|t| t.kind() == SyntaxKind::TEXT) || c.as_node().is_some()
        });
    for c in node.children_with_tokens() {
        match c {
            NodeOrToken::Node(n) => {
                if skipped(n.kind()) && !is_title(&n) {
                    out.push(' ');
                } else {
                    visible(&n, range, out);
                }
            }
            NodeOrToken::Token(t) => {
                let tr = t.text_range();
                let (s, e) = (usize::from(tr.start()), usize::from(tr.end()));
                if e <= range.start || s >= range.end {
                    continue;
                }
                match t.kind() {
                    SyntaxKind::TEXT => {
                        let (a, b) = (range.start.max(s) - s, range.end.min(e) - s);
                        out.push_str(&t.text()[a..b]);
                    }
                    SyntaxKind::CODE_TEXT if !described => out.push_str(t.text()),
                    // An entity reads as one character.
                    SyntaxKind::KEY if node.kind() == SyntaxKind::ENTITY => out.push('x'),
                    SyntaxKind::MARKER => {}
                    _ => out.push(' '),
                }
            }
        }
    }
}

/// The words in `range` of the document `root`.
pub fn words(root: &SyntaxNode, range: Range<usize>) -> usize {
    let mut text = String::new();
    visible(root, &range, &mut text);
    text.split_whitespace()
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .count()
}

/// The subtree holding `pos`: its innermost headline's range.
pub fn subtree_at(root: &SyntaxNode, pos: usize) -> Option<Range<usize>> {
    let mut found = None;
    let mut node = root.clone();
    loop {
        let next = node.children().find(|c| {
            let r = c.text_range();
            usize::from(r.start()) <= pos && pos < usize::from(r.end())
        });
        let Some(n) = next else { break };
        if n.kind() == SyntaxKind::HEADLINE {
            let r = n.text_range();
            found = Some(usize::from(r.start())..usize::from(r.end()));
        }
        node = n;
    }
    found
}

/// Word counts kept for a text version, so status bars count only after
/// edits, and while typing goes on only now and then: counting a large
/// document takes milliseconds a keystroke does not have.
#[derive(Debug, Clone, Default)]
pub struct WordCounts {
    version: Option<u64>,
    document: usize,
    section: Option<(Range<usize>, usize)>,
    counted: Option<std::time::Instant>,
    targets: Targets,
}

/// Word targets: the document's (`#+PROPERTY: WORD_TARGET 80000`) and the
/// section's (its heading's `WORD_TARGET` property).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Targets {
    /// The document's target.
    pub document: Option<usize>,
    /// The target of the section holding the cursor.
    pub section: Option<usize>,
}

/// A word target as written: `80000`, `80,000` or `80k`.
pub fn parse_target(v: &str) -> Option<usize> {
    let v = v.trim().replace([',', '_', '.', ' '], "");
    let (digits, k) = match v.strip_suffix(['k', 'K']) {
        Some(d) => (d.to_string(), 1000),
        None => (v, 1),
    };
    let n: usize = digits.parse().ok()?;
    (n > 0).then_some(n * k)
}

/// The `#+PROPERTY: WORD_TARGET` lines of the document.
fn target_lines(root: &SyntaxNode) -> Vec<(SyntaxNode, String)> {
    use org_syntax::ast::AstNode;
    root.descendants()
        .filter_map(org_syntax::ast::Keyword::cast)
        .filter(|k| k.key().eq_ignore_ascii_case("PROPERTY"))
        .filter_map(|k| {
            let v = k.value();
            let (name, value) = v.trim().split_once([' ', '\t'])?;
            name.eq_ignore_ascii_case("WORD_TARGET")
                .then(|| (k.syntax().clone(), value.trim().to_string()))
        })
        .collect()
}

/// The document's word target: its `#+PROPERTY: WORD_TARGET 80000`, a
/// property of the whole file as Org has them (the last line wins).
pub fn document_target(root: &SyntaxNode) -> Option<usize> {
    target_lines(root).last().and_then(|(_, v)| parse_target(v))
}

/// Sets the document's word target (`None` takes it away): its
/// `#+PROPERTY: WORD_TARGET` line changed, or one added after the
/// keywords at the top.
pub fn set_document_target(root: &SyntaxNode, text: &str, target: Option<usize>) -> Transaction {
    let mut tx = Transaction::new("Word Target");
    let lines = target_lines(root);
    let line = |n: &SyntaxNode| {
        let s = usize::from(n.text_range().start());
        let e = text[s..].find('\n').map_or(text.len(), |i| s + i + 1);
        s..e
    };
    match (lines.last(), target) {
        (Some((n, _)), Some(t)) => {
            let r = line(n);
            let nl = if text[r.clone()].ends_with('\n') {
                "\n"
            } else {
                ""
            };
            let _ = tx.replace(r, format!("#+PROPERTY: WORD_TARGET {t}{nl}"));
        }
        (Some(_), None) => {
            for (n, _) in lines.iter().rev() {
                let _ = tx.delete(line(n));
            }
        }
        (None, Some(t)) => {
            let mut at = 0;
            for l in text.split_inclusive('\n') {
                let t = l.trim_start().to_ascii_lowercase();
                if t.starts_with("#+") && !t.starts_with("#+begin") {
                    at += l.len();
                } else {
                    break;
                }
            }
            let _ = tx.insert(at, format!("#+PROPERTY: WORD_TARGET {t}\n"));
        }
        (None, None) => {}
    }
    tx
}

/// The word target of the headline starting at `start`: its own
/// `WORD_TARGET` property.
fn heading_target(root: &SyntaxNode, start: usize) -> Option<usize> {
    use org_syntax::ast::AstNode;
    let h = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::HEADLINE)
        .find(|n| usize::from(n.text_range().start()) == start)?;
    let h = org_syntax::ast::Headline::cast(h)?;
    h.properties()
        .into_iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("WORD_TARGET"))
        .and_then(|(_, v)| parse_target(&v))
}

/// A heading's words and target, for the list of chapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chapter {
    /// Its level.
    pub level: usize,
    /// Its title.
    pub title: String,
    /// Where it starts.
    pub start: usize,
    /// The words of its subtree.
    pub words: usize,
    /// Its `WORD_TARGET`.
    pub target: Option<usize>,
}

/// The headings of the first two levels (parts and chapters, or chapters
/// and sections) with their words and targets.
pub fn chapters(root: &SyntaxNode) -> Vec<Chapter> {
    use org_syntax::ast::AstNode;
    let heads: Vec<SyntaxNode> = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::HEADLINE)
        .collect();
    let stars = |n: &SyntaxNode| {
        n.first_token()
            .map_or(1, |t| t.text().bytes().take_while(|b| *b == b'*').count())
    };
    let levels: Vec<usize> = heads.iter().map(stars).collect();
    let top = levels.iter().copied().min().unwrap_or(1);
    heads
        .into_iter()
        .filter_map(|n| {
            let h = org_syntax::ast::Headline::cast(n.clone())?;
            let level = stars(&n);
            if level > top + 1 {
                return None;
            }
            let r = n.text_range();
            let (start, end) = (usize::from(r.start()), usize::from(r.end()));
            Some(Chapter {
                level: level - top + 1,
                title: h.raw_value(),
                start,
                words: words(root, start..end),
                target: heading_target(root, start),
            })
        })
        .collect()
}

/// `n` words against a target: `1,200 of 5,000 (24%)`.
pub fn progress(n: usize, target: usize) -> String {
    crate::tr!(
        "status-words-progress",
        shown = thousands(n),
        target = thousands(target),
        percent = n * 100 / target.max(1)
    )
}

/// How long typing goes on before the counts catch up.
const PAUSE: std::time::Duration = std::time::Duration::from_millis(300);

impl WordCounts {
    /// Whether the counts are behind the text and due: the frontend
    /// should draw the status bar again. Never for a document [`get`]
    /// does not count (one without an Org parse: Markdown, code), which
    /// would otherwise be drawn again at every tick.
    ///
    /// [`get`]: WordCounts::get
    pub fn due(&self, doc: &crate::DocumentState) -> bool {
        doc.parse().is_some()
            && self.version != Some(doc.version())
            && self.counted.is_none_or(|t| t.elapsed() >= PAUSE)
    }

    /// The words of `doc` and of the section holding its cursor, counted
    /// again when the text changed (at most every 300 ms while typing).
    /// Until a parse catches up with edits, the last counts; `None` before
    /// the first parse.
    pub fn get(&mut self, doc: &crate::DocumentState) -> Option<(usize, Option<usize>)> {
        if let Some((parse, true)) = doc.parse() {
            let root = parse.syntax();
            let changed = self.version != Some(doc.version())
                && (self.version.is_none() || self.counted.is_none_or(|t| t.elapsed() >= PAUSE));
            if changed {
                self.counted = Some(std::time::Instant::now());
                self.document = words(&root, 0..root.text_range().end().into());
                self.version = Some(doc.version());
            }
            let range = subtree_at(&root, doc.selection.head);
            let same = self.section.as_ref().map(|s| &s.0) == range.as_ref();
            if changed || (!same && self.version == Some(doc.version())) {
                self.section = range.clone().map(|r| (r.clone(), words(&root, r)));
                self.targets = Targets {
                    document: document_target(&root),
                    section: range.and_then(|r| heading_target(&root, r.start)),
                };
            }
        }
        self.version?;
        Some((self.document, self.section.as_ref().map(|s| s.1)))
    }

    /// The targets for the counts [`WordCounts::get`] gave.
    pub fn targets(&self) -> Targets {
        self.targets
    }
}

/// `1234567` as `1,234,567` (digit groups of the interface language).
pub fn thousands(n: usize) -> String {
    crate::l10n::number(n)
}

/// The status bar text for word counts, with their targets.
pub fn describe(document: usize, section: Option<usize>, targets: Targets) -> String {
    let words = match targets.document {
        Some(t) => crate::tr!("status-words-target", words = progress(document, t)),
        None => crate::tr!(
            "status-words",
            count = document,
            shown = thousands(document)
        ),
    };
    match section {
        Some(s) => crate::tr!(
            "status-words-section",
            words = words,
            section = match targets.section {
                Some(t) => progress(s, t),
                None => thousands(s),
            }
        ),
        None => words,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_documents() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/corpus/org-mode");
        let Some(big) = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "org"))
            .max_by_key(|p| std::fs::metadata(p).map_or(0, |m| m.len()))
        else {
            return;
        };
        let t = std::fs::read_to_string(&big).unwrap();
        let p = org_syntax::parse(&t);
        let start = std::time::Instant::now();
        let n = words(&p.syntax(), 0..t.len());
        let took = start.elapsed();
        // About 5 ms in a release build.
        assert!(took < std::time::Duration::from_secs(2), "{took:?}");
        assert!(n > 0);
    }

    #[test]
    fn documents_not_counted_are_never_due() {
        // A Markdown file was drawn again at every tick, for ever.
        let meta = |mode| crate::Metadata {
            path: None,
            mode,
            line_ending: crate::LineEnding::Lf,
            bom: false,
            encoding: crate::encoding_rs::UTF_8,
            lossy: false,
        };
        let settings = std::sync::Arc::new(org_model::Settings::default());
        let md = crate::DocumentState::new(
            "# Title\n\nSome words.\n",
            meta(crate::DocumentMode::Markdown),
            settings.clone(),
        );
        let mut counts = WordCounts::default();
        assert!(!counts.due(&md));
        assert_eq!(counts.get(&md), None);
        assert!(!counts.due(&md));
        let org = crate::DocumentState::new(
            "* Title\nSome words.\n",
            meta(crate::DocumentMode::Org),
            settings,
        );
        let mut counts = WordCounts::default();
        assert!(counts.due(&org));
        assert!(counts.get(&org).is_some());
        assert!(!counts.due(&org));
    }

    #[test]
    fn counting() {
        let t = "#+AUTHOR: Not counted\n* TODO Head [[https://x.org][the link]] :tag:\nSCHEDULED: <2026-01-01 Thu>\n:PROPERTIES:\n:ID: x\n:END:\nSome *bold* text [[file:a.org]] and \\alpha. un*bold*ed <2026-01-01 Thu> ~co de~ [fn:1] --\n- [ ] item one\n| a b | c |\n#+begin_src sh\nnot counted\n#+end_src\n#+begin_quote\nquoted words\n#+end_quote\n** Sub\none two\n";
        let p = org_syntax::parse(t);
        let root = p.syntax();
        // Head, the, link; Some, bold, text, file:a.org, and, x., unbolded,
        // co, de; item, one; a, b, c; quoted, words; Sub; one, two.
        assert_eq!(words(&root, 0..t.len()), 22);
        let sub = t.find("** Sub").unwrap();
        assert_eq!(subtree_at(&root, sub + 8), Some(sub..t.len()));
        assert_eq!(words(&root, sub..t.len()), 3);
        assert_eq!(subtree_at(&root, 3), None);
        assert_eq!(subtree_at(&root, 25), Some(22..t.len()));
        let none = Targets::default();
        assert_eq!(
            describe(1234567, Some(3), none),
            "1,234,567 words, 3 in section"
        );
        assert_eq!(describe(1, None, none), "1 word");
        let t = Targets {
            document: Some(80_000),
            section: Some(4_000),
        };
        assert_eq!(
            describe(1200, Some(1000), t),
            "1,200 of 80,000 (1%) words, 1,000 of 4,000 (25%) in section"
        );
    }

    #[test]
    fn targets_and_chapters() {
        assert_eq!(parse_target("80,000"), Some(80_000));
        assert_eq!(parse_target("5k"), Some(5_000));
        assert_eq!(parse_target("none"), None);
        let t = "#+PROPERTY: WORD_TARGET 10k\n* Part\n** One\n:PROPERTIES:\n:WORD_TARGET: 100\n:END:\nfour words right here\n*** Deep\nthree more words\n** Two\nx y\n";
        let root = org_syntax::parse(t).syntax();
        assert_eq!(document_target(&root), Some(10_000));
        let cs = chapters(&root);
        assert_eq!(
            cs.iter()
                .map(|c| (c.level, c.title.as_str(), c.words, c.target))
                .collect::<Vec<_>>(),
            [
                (1, "Part", 13, None),
                (2, "One", 9, Some(100)),
                (2, "Two", 3, None)
            ]
        );
    }
}
