//! The outline: headlines and inlinetasks in document order, with their
//! place in the tree and the data Org reads from their first lines and
//! property drawers.

use org_syntax::ast::{AstNode, Headline, Inlinetask};
use std::sync::Arc;

use org_syntax::{ParseContext, SyntaxKind, SyntaxNode, TextRange, TextSize};

/// An entry of the outline: an index into [`Outline::entries`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId(pub usize);

/// A drawer property as org-element stores it on its node: the key
/// upcased; a later duplicate replaces the value, while repeated `KEY+`
/// entries collect all their values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeProperty {
    /// The upcased key, `+` included.
    pub key: String,
    /// One value, or several for repeated `KEY+`.
    pub values: Vec<String>,
}

/// A headline or an inlinetask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The whole node.
    pub range: TextRange,
    /// Whether this is an inlinetask.
    pub inlinetask: bool,
    /// `:level`.
    pub level: usize,
    /// The enclosing headline.
    pub parent: Option<EntryId>,
    /// Headlines and inlinetasks directly below.
    pub children: Vec<EntryId>,
    /// `:todo-keyword`.
    pub todo: Option<String>,
    /// `:priority`.
    pub priority: Option<char>,
    /// `:tags`: the entry's own tags.
    pub local_tags: Vec<String>,
    /// `:raw-value`.
    pub raw_title: String,
    /// `:commentedp`.
    pub commented: bool,
    /// `:archivedp`.
    pub archived: bool,
    /// The headline line, without its line ending.
    pub line: String,
    /// The line after the headline line, if any.
    pub next_line: Option<String>,
    /// Drawer properties as written (key, value), in order.
    pub drawer: Vec<(String, String)>,
    /// Drawer properties as org-element stores them.
    pub node_properties: Vec<NodeProperty>,
}

/// Drawer properties in org-element's form (`org-element--get-node-properties`).
pub(crate) fn node_properties(drawer: &[(String, String)]) -> Vec<NodeProperty> {
    let mut out: Vec<NodeProperty> = Vec::new();
    for (k, v) in drawer {
        let key = k.to_uppercase();
        match out.iter_mut().find(|p| p.key == key) {
            Some(p) if key.ends_with('+') => p.values.push(v.clone()),
            Some(p) => p.values = vec![v.clone()],
            None => out.push(NodeProperty {
                key,
                values: vec![v.clone()],
            }),
        }
    }
    out
}

/// All entries of a document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outline {
    /// Headlines and inlinetasks in document order.
    pub entries: Vec<Entry>,
    /// Top-level headlines.
    pub roots: Vec<EntryId>,
}

/// The first line of `node` (without its line ending) and the line after
/// it, read from the tokens so that large subtrees are not copied.
fn first_lines(node: &SyntaxNode) -> (String, Option<String>) {
    let mut first = String::new();
    let mut second: Option<String> = None;
    for t in node
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
    {
        let text = t.text();
        let target = match &mut second {
            Some(s) => s,
            None => &mut first,
        };
        match text.find('\n') {
            None => target.push_str(text),
            Some(i) => {
                target.push_str(&text[..i]);
                if second.is_some() {
                    break;
                }
                // The rest of this token starts the second line.
                let rest = &text[i + 1..];
                match rest.find('\n') {
                    Some(j) => {
                        second = Some(rest[..j].to_string());
                        break;
                    }
                    None => second = Some(rest.to_string()),
                }
            }
        }
    }
    let trim = |s: String| s.strip_suffix('\r').map(str::to_string).unwrap_or(s);
    // A node that ends right after its first line has no second line in it;
    // the line after is then outside the node.
    (
        trim(first),
        second
            .map(trim)
            .filter(|s| !s.is_empty() || node.text_range().len() > 0.into()),
    )
}

/// Headlines and inlinetasks under `root`, without visiting paragraphs,
/// tables, blocks or objects (they cannot contain either).
fn entries_of(root: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut out = Vec::new();
    let mut stack: Vec<SyntaxNode> = vec![root.clone()];
    while let Some(n) = stack.pop() {
        if matches!(n.kind(), SyntaxKind::HEADLINE | SyntaxKind::INLINETASK) {
            out.push(n.clone());
        }
        let children: Vec<SyntaxNode> = n
            .children()
            .filter(|c| c.kind().is_greater_element() && c.kind() != SyntaxKind::TABLE)
            .collect();
        stack.extend(children.into_iter().rev());
    }
    out
}

/// The data of one headline or inlinetask; `range` is relative to `base`.
fn entry_data(node: &SyntaxNode, ctx: &ParseContext, base: TextSize) -> Entry {
    let kind = node.kind();
    let (level, todo, priority, tags, raw, commented, archived, drawer) =
        if kind == SyntaxKind::HEADLINE {
            let h = Headline::cast(node.clone()).expect("headline");
            (
                h.level(ctx),
                h.todo_keyword().map(|t| t.text().to_string()),
                h.priority(),
                h.tags(),
                h.raw_value(),
                h.is_commented(),
                h.is_archived(),
                h.properties(),
            )
        } else {
            let h = Inlinetask::cast(node.clone()).expect("inlinetask");
            (
                h.level(ctx),
                h.todo_keyword().map(|t| t.text().to_string()),
                h.priority(),
                h.tags(),
                h.raw_value(),
                h.is_commented(),
                h.is_archived(),
                h.properties(),
            )
        };
    let (line, next_line) = first_lines(node);
    let r = node.text_range();
    Entry {
        range: TextRange::new(r.start() - base, r.end() - base),
        inlinetask: kind == SyntaxKind::INLINETASK,
        level,
        parent: None,
        children: Vec::new(),
        todo,
        priority,
        local_tags: tags,
        raw_title: raw,
        commented,
        archived,
        line,
        next_line,
        node_properties: node_properties(&drawer),
        drawer,
    }
}

/// A headline and what is below it, as cached across versions: ranges
/// are relative to the headline's start, and child headlines are shared,
/// so a new version builds only the headlines on the path to an edit and
/// copies each entry once, into its outline.
#[derive(Debug)]
pub(crate) struct Subtree {
    /// The headline, without parent or children.
    head: Entry,
    /// Its section's inlinetasks and its child headlines, in order.
    below: Vec<Below>,
    /// Entries in all, the headline included.
    len: usize,
}

/// A part of a [`Subtree`], or of the document's top level.
#[derive(Debug)]
enum Below {
    /// An entry of a section, without parent or children.
    Entry(Box<Entry>),
    /// A headline, at its offset.
    Headline(TextSize, Arc<Subtree>),
}

impl Below {
    fn len(&self) -> usize {
        match self {
            Below::Entry(_) => 1,
            Below::Headline(_, s) => s.len,
        }
    }

    /// Appends the entries, shifted by `shift`, under `parent`.
    fn flatten(&self, dst: &mut Vec<Entry>, shift: TextSize, parent: Option<EntryId>) {
        match self {
            Below::Entry(e) => dst.push(Entry {
                range: e.range + shift,
                parent,
                ..(**e).clone()
            }),
            Below::Headline(offset, sub) => {
                let shift = shift + *offset;
                let id = dst.len();
                let mut next = id + 1;
                let children = sub
                    .below
                    .iter()
                    .map(|b| {
                        let c = EntryId(next);
                        next += b.len();
                        c
                    })
                    .collect();
                dst.push(Entry {
                    range: sub.head.range + shift,
                    parent,
                    children,
                    ..sub.head.clone()
                });
                for b in &sub.below {
                    b.flatten(dst, shift, Some(EntryId(id)));
                }
            }
        }
    }
}

type Pass<'a> = crate::cache::Pass<'a, Subtree>;

/// The headline `node` and its subtree, reusing cached subtrees.
fn subtree(node: &SyntaxNode, ctx: &ParseContext, pass: Option<&Pass<'_>>) -> Arc<Subtree> {
    if let Some(v) = pass.and_then(|p| p.get(node)) {
        return v;
    }
    let base = node.text_range().start();
    let mut below = Vec::new();
    for child in node.children() {
        match child.kind() {
            SyntaxKind::SECTION => below.extend(
                entries_of(&child)
                    .iter()
                    .map(|n| Below::Entry(Box::new(entry_data(n, ctx, base)))),
            ),
            SyntaxKind::HEADLINE => below.push(Below::Headline(
                child.text_range().start() - base,
                subtree(&child, ctx, pass),
            )),
            _ => {}
        }
    }
    let v = Arc::new(Subtree {
        head: entry_data(node, ctx, base),
        len: 1 + below.iter().map(Below::len).sum::<usize>(),
        below,
    });
    if let Some(p) = pass {
        p.put(node, v.clone());
    }
    v
}

impl Outline {
    /// Builds the outline of `root`.
    pub fn new(root: &SyntaxNode, ctx: &ParseContext) -> Outline {
        Outline::build(root, ctx, None)
    }

    pub(crate) fn build(
        root: &SyntaxNode,
        ctx: &ParseContext,
        cache: Option<&crate::cache::ModelCache>,
    ) -> Outline {
        let pass = cache.map(|c| c.subtrees.pass());
        let mut top: Vec<Below> = Vec::new();
        for child in root.children() {
            match child.kind() {
                SyntaxKind::SECTION => top.extend(
                    entries_of(&child)
                        .iter()
                        .map(|n| Below::Entry(Box::new(entry_data(n, ctx, TextSize::from(0))))),
                ),
                SyntaxKind::HEADLINE => top.push(Below::Headline(
                    child.text_range().start(),
                    subtree(&child, ctx, pass.as_ref()),
                )),
                _ => {}
            }
        }
        // Ends the pass: what this version no longer uses leaves the cache.
        drop(pass);
        let mut entries = Vec::with_capacity(top.iter().map(Below::len).sum());
        for b in &top {
            b.flatten(&mut entries, TextSize::from(0), None);
        }
        let roots = (0..entries.len())
            .filter(|&i| entries[i].parent.is_none())
            .map(EntryId)
            .collect();
        Outline { entries, roots }
    }

    /// The entry with the given id.
    pub fn get(&self, id: EntryId) -> &Entry {
        &self.entries[id.0]
    }

    /// The ancestors of `id`, nearest first.
    pub fn ancestors(&self, id: EntryId) -> impl Iterator<Item = EntryId> + '_ {
        std::iter::successors(self.entries[id.0].parent, move |p| self.entries[p.0].parent)
    }

    /// The innermost entry containing `offset`.
    pub fn entry_at(&self, offset: usize) -> Option<EntryId> {
        let o = TextSize::from(offset as u32);
        // Entries are in document order and nest: the last one starting at
        // or before `offset` whose range contains it.
        let i = self.entries.partition_point(|e| e.range.start() <= o);
        self.entries[..i]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, e)| {
                e.range.contains_inclusive(o) && (o < e.range.end() || e.range.end() == o)
            })
            .map(|(i, _)| EntryId(i))
    }
}
