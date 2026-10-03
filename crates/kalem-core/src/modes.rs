//! The document mode contract (design §11.11, T2.7c.10): what a mode
//! gives the core, the same for built-in modes and plugins. A mode
//! returns ranges into the text, never text, so it cannot break the
//! round trip; the tree is flat data (kinds, ranges and parents), so it
//! crosses a plugin boundary as arrays. The core builds views, the grid,
//! the outline and the batch commands from it.

use std::ops::Range;

/// What a node is: the fixed vocabulary every mode maps its syntax to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A heading, from level 1.
    Heading(u8),
    /// A paragraph.
    Paragraph,
    /// A list (its items are its children).
    List {
        /// Numbered.
        ordered: bool,
    },
    /// A list item, with its checkbox when it has one.
    ListItem {
        /// `Some(true)` checked, `Some(false)` empty, `None` none.
        checkbox: Option<bool>,
    },
    /// A quotation.
    Quote,
    /// Verbatim code, with its language.
    Code {
        /// The language, when given.
        language: Option<String>,
    },
    /// A table (its rows are its children).
    Table,
    /// A table row.
    TableRow,
    /// A table cell.
    TableCell,
    /// Displayed mathematics.
    MathBlock,
    /// A horizontal rule.
    Rule,
    /// Emphasis (italic).
    Emphasis,
    /// Strong emphasis (bold).
    Strong,
    /// Inline code.
    InlineCode,
    /// A link, with the range of its target.
    Link {
        /// Where the target is written.
        target: Option<Range<usize>>,
    },
    /// A picture, with the range of its source.
    Image {
        /// Where the source is written.
        target: Option<Range<usize>>,
    },
    /// Inline mathematics.
    Math,
    /// A footnote reference.
    FootnoteRef,
    /// Markup the view hides away from the cursor (delimiters, brackets).
    HiddenMarker,
    /// Anything else, by the mode's own name, shown as source.
    Other(String),
}

impl Kind {
    /// A block, as against inline content.
    pub fn is_block(&self) -> bool {
        matches!(
            self,
            Kind::Heading(_)
                | Kind::Paragraph
                | Kind::List { .. }
                | Kind::ListItem { .. }
                | Kind::Quote
                | Kind::Code { .. }
                | Kind::Table
                | Kind::TableRow
                | Kind::TableCell
                | Kind::MathBlock
                | Kind::Rule
        )
    }
}

/// A node: its kind, its bytes and its parent (an index before it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// What it is.
    pub kind: Kind,
    /// Its bytes in the text.
    pub range: Range<usize>,
    /// The enclosing node, `None` at the top.
    pub parent: Option<u32>,
}

/// A parse: the nodes in document order, each after its parent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tree {
    /// The nodes, in pre-order.
    pub nodes: Vec<Node>,
}

impl Tree {
    /// Adds a node; its index.
    pub fn push(&mut self, kind: Kind, range: Range<usize>, parent: Option<u32>) -> u32 {
        self.nodes.push(Node {
            kind,
            range,
            parent,
        });
        (self.nodes.len() - 1) as u32
    }

    /// The children of node `i` (`None`: the top-level nodes).
    pub fn children(&self, i: Option<u32>) -> impl Iterator<Item = (u32, &Node)> {
        self.nodes
            .iter()
            .enumerate()
            .filter(move |(_, n)| n.parent == i)
            .map(|(j, n)| (j as u32, n))
    }

    /// The headings, as an outline: level, title (the heading's text,
    /// trimmed) and start.
    pub fn headings(&self, text: &str) -> Vec<crate::view::OutlineItem> {
        self.nodes
            .iter()
            .filter_map(|n| match n.kind {
                Kind::Heading(level) => Some(crate::view::OutlineItem {
                    level: usize::from(level),
                    todo: None,
                    title: text.get(n.range.clone()).unwrap_or("").trim().to_string(),
                    start: n.range.start,
                    file: None,
                }),
                _ => None,
            })
            .collect()
    }
}

/// How a mode is chosen for a file.
#[derive(Debug, Clone, Copy, Default)]
pub struct Detect {
    /// File extensions, without the dot, lower case.
    pub extensions: &'static [&'static str],
    /// A test of the first bytes, for files without a known extension.
    pub sniff: Option<fn(&[u8]) -> bool>,
}

impl Detect {
    /// Whether a file called `name` starting with `head` is this mode's.
    pub fn matches(&self, name: Option<&str>, head: &[u8]) -> bool {
        let ext = name
            .and_then(|n| n.rsplit_once('.'))
            .map(|(_, e)| e.to_lowercase());
        ext.is_some_and(|e| self.extensions.contains(&e.as_str()))
            || self.sniff.is_some_and(|f| f(head))
    }
}

/// An edit of the text: `range` of the old text replaced by `len` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    /// What was replaced, in the old text.
    pub range: Range<usize>,
    /// How long the new text is.
    pub len: usize,
}

/// A grid of a table-like format: rows of cell ranges.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grid {
    /// Each row's cells.
    pub rows: Vec<Vec<Range<usize>>>,
    /// The first row names the columns.
    pub header: bool,
}

/// A key a mode may handle while typing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKey {
    /// Enter: continue a list, a quote.
    Enter,
    /// Tab: indent, the next cell.
    Tab,
    /// Shift+Tab.
    BackTab,
}

/// A problem a mode reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeDiagnostic {
    /// Where.
    pub range: Range<usize>,
    /// Its rule.
    pub code: String,
    /// For people.
    pub message: String,
}

/// A document mode (§11.11). Only `id`, `detect` and `parse` are
/// required; the rest have neutral defaults.
pub trait ModeSpec: Send + Sync {
    /// Its name: `klm`, `csv`.
    fn id(&self) -> &'static str;

    /// Which files it takes.
    fn detect(&self) -> Detect;

    /// The tree of `text`. With `edit` and the `previous` tree, a mode may
    /// reparse from the enclosing top-level block; the result must equal
    /// a full parse.
    fn parse(&self, text: &str, edit: Option<&TextEdit>, previous: Option<&Tree>) -> Tree;

    /// Rows and cells, for table-like formats.
    fn grid(&self, _text: &str) -> Option<Grid> {
        None
    }

    /// What `key` does at byte `at`: replacements in the text, one undo
    /// step; `None` leaves the key to the editor.
    fn edit(&self, _text: &str, _at: usize, _key: EditKey) -> Option<Vec<(Range<usize>, String)>> {
        None
    }

    /// The outline; the headings of the tree by default.
    fn outline(&self, text: &str, tree: &Tree) -> Vec<crate::view::OutlineItem> {
        tree.headings(text)
    }

    /// The canonical form of `text` (Format Document, `kalem fmt`).
    fn format(&self, _text: &str) -> Option<String> {
        None
    }

    /// Problems for `kalem check` and the editor.
    fn diagnostics(&self, _text: &str) -> Vec<ModeDiagnostic> {
        Vec::new()
    }

    /// HTML of `text`, when the mode exports it itself.
    fn to_html(&self, _text: &str) -> Option<String> {
        None
    }
}

/// The modes on the contract.
pub struct Modes {
    specs: Vec<Box<dyn ModeSpec>>,
}

impl std::fmt::Debug for Modes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.specs.iter().map(|s| s.id()))
            .finish()
    }
}

impl Default for Modes {
    fn default() -> Self {
        Modes::with_builtins()
    }
}

impl Modes {
    /// The built-in modes written against the contract.
    pub fn with_builtins() -> Modes {
        let mut m = Modes { specs: Vec::new() };
        m.register(Box::new(CsvMode));
        m.register(Box::new(crate::klm::KlmMode::default()));
        m.register(Box::new(crate::markdown::MarkdownMode));
        m.register(Box::new(crate::latex_mode::LatexMode));
        m
    }

    /// Adds a mode; a later one with the same id replaces it.
    pub fn register(&mut self, spec: Box<dyn ModeSpec>) {
        self.specs.retain(|s| s.id() != spec.id());
        self.specs.push(spec);
    }

    /// The mode called `id`.
    pub fn get(&self, id: &str) -> Option<&dyn ModeSpec> {
        self.specs.iter().find(|s| s.id() == id).map(|s| &**s)
    }

    /// The mode for a file called `name` starting with `head`.
    pub fn detect(&self, name: Option<&str>, head: &[u8]) -> Option<&dyn ModeSpec> {
        self.specs
            .iter()
            .rev()
            .find(|s| s.detect().matches(name, head))
            .map(|s| &**s)
    }

    /// Their ids.
    pub fn ids(&self) -> Vec<&'static str> {
        self.specs.iter().map(|s| s.id()).collect()
    }
}

/// What every mode must satisfy on `text` (the conformance suite of
/// §11.11): ranges inside the text and on character boundaries, each
/// node inside its parent and after it, the parse the same twice, and an
/// incremental parse after each of `edits` (a range and its new text)
/// the same as a full one.
pub fn check(
    spec: &dyn ModeSpec,
    text: &str,
    edits: &[(Range<usize>, &str)],
) -> Result<(), String> {
    let tree = spec.parse(text, None, None);
    check_tree(text, &tree)?;
    if spec.parse(text, None, None) != tree {
        return Err("two parses differ".into());
    }
    for (range, new) in edits {
        if range.end > text.len()
            || !text.is_char_boundary(range.start)
            || !text.is_char_boundary(range.end)
        {
            continue;
        }
        let mut after = text.to_string();
        after.replace_range(range.clone(), new);
        let edit = TextEdit {
            range: range.clone(),
            len: new.len(),
        };
        let inc = spec.parse(&after, Some(&edit), Some(&tree));
        let full = spec.parse(&after, None, None);
        check_tree(&after, &full)?;
        if inc != full {
            return Err(format!(
                "an incremental parse after replacing {range:?} by {new:?} differs from a full parse"
            ));
        }
    }
    Ok(())
}

fn check_tree(text: &str, tree: &Tree) -> Result<(), String> {
    for (i, n) in tree.nodes.iter().enumerate() {
        let r = &n.range;
        if r.start > r.end || r.end > text.len() {
            return Err(format!(
                "node {i} ({:?}) is outside the text: {r:?}",
                n.kind
            ));
        }
        if !text.is_char_boundary(r.start) || !text.is_char_boundary(r.end) {
            return Err(format!("node {i} ({:?}) cuts a character: {r:?}", n.kind));
        }
        if let Some(p) = n.parent {
            let p = p as usize;
            if p >= i {
                return Err(format!("node {i} comes before its parent {p}"));
            }
            let pr = &tree.nodes[p].range;
            if r.start < pr.start || r.end > pr.end {
                return Err(format!("node {i} {r:?} is outside its parent {p} {pr:?}"));
            }
        }
    }
    Ok(())
}

/// CSV on the contract: the grid of `crate::csv`, rows and cells as the
/// tree.
#[derive(Debug)]
pub struct CsvMode;

impl ModeSpec for CsvMode {
    fn id(&self) -> &'static str {
        "csv"
    }

    fn detect(&self) -> Detect {
        Detect {
            extensions: &["csv", "tsv", "tab"],
            sniff: None,
        }
    }

    fn parse(&self, text: &str, _edit: Option<&TextEdit>, _previous: Option<&Tree>) -> Tree {
        let mut tree = Tree::default();
        if text.is_empty() {
            return tree;
        }
        let table = tree.push(Kind::Table, 0..text.len(), None);
        if let Some(g) = self.grid(text) {
            for row in g.rows {
                let start = row.first().map_or(0, |c| c.start);
                let end = row.last().map_or(start, |c| c.end);
                let r = tree.push(Kind::TableRow, start..end, Some(table));
                for c in row {
                    tree.push(Kind::TableCell, c, Some(r));
                }
            }
        }
        tree
    }

    fn grid(&self, text: &str) -> Option<Grid> {
        let d = crate::csv::detect(text);
        let mut rows = Vec::new();
        let mut at = 0;
        while at < text.len() {
            let r = crate::csv::scan(text, at, &d);
            rows.push(r.fields.iter().map(|f| f.range.clone()).collect());
            if r.next <= at {
                break;
            }
            at = r.next;
        }
        Some(Grid {
            rows,
            header: d.header,
        })
    }

    fn diagnostics(&self, text: &str) -> Vec<ModeDiagnostic> {
        let d = crate::csv::detect(text);
        crate::csv::problems(text, &d, 1000)
            .into_iter()
            .map(|p| ModeDiagnostic {
                range: p.range.clone(),
                code: p.code.into(),
                message: p.message.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_on_the_contract() {
        let modes = Modes::with_builtins();
        let csv = modes.detect(Some("data.CSV"), b"").unwrap();
        assert_eq!(csv.id(), "csv");
        let text = "a,b\n1,\"x,y\"\n2,3\n";
        let tree = csv.parse(text, None, None);
        let cells: Vec<&str> = tree
            .nodes
            .iter()
            .filter(|n| n.kind == Kind::TableCell)
            .map(|n| &text[n.range.clone()])
            .collect();
        assert_eq!(cells, ["a", "b", "1", "\"x,y\"", "2", "3"]);
        check(csv, text, &[(0..1, "z"), (4..5, "10\n4,5\n")]).unwrap();
    }

    #[test]
    fn a_bad_tree_is_caught() {
        struct Bad;
        impl ModeSpec for Bad {
            fn id(&self) -> &'static str {
                "bad"
            }
            fn detect(&self) -> Detect {
                Detect::default()
            }
            fn parse(&self, text: &str, _: Option<&TextEdit>, _: Option<&Tree>) -> Tree {
                let mut t = Tree::default();
                t.push(Kind::Paragraph, 0..text.len() + 1, None);
                t
            }
        }
        assert!(check(&Bad, "abc", &[]).is_err());
    }
}
