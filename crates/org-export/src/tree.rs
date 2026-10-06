//! The export tree: the parse tree as `ox.el` sees it, with plain text
//! between objects, contents and post-blank as `org-element` gives them,
//! and room for the changes export makes (pruning, installed footnote
//! definitions, strings kept in place of removed objects).

use std::collections::HashMap;

use org_syntax::SyntaxKind::{self, *};
use org_syntax::{NodeOrToken, SyntaxNode, TextRange, ast};

/// A node's index in the tree.
pub type Id = usize;

/// What a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// An element or object of this type.
    Node(SyntaxKind),
    /// Plain text.
    Text,
    /// Text inserted as it is in the output (`raw` in `ox.el`).
    Raw,
}

/// Secondary strings of a node (`org-element-secondary-value-alist`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Secondary {
    /// A headline's or inlinetask's title.
    Title,
    /// A descriptive item's tag.
    Tag,
    /// The `n`th `#+CAPTION` of an element (its long part).
    Caption(usize),
    /// The short part of the `n`th `#+CAPTION[short]:`.
    ShortCaption(usize),
}

/// A node of the export tree.
#[derive(Debug, Clone)]
pub struct Node {
    /// What it is.
    pub kind: Kind,
    /// Its syntax node, for its properties.
    pub syntax: Option<SyntaxNode>,
    /// Plain or raw text.
    pub text: String,
    /// Its contents.
    pub children: Vec<Id>,
    /// Its parent (for secondary strings, their owner).
    pub parent: Option<Id>,
    /// Blank lines (elements) or spaces (objects) after it.
    pub post_blank: usize,
    /// Its secondary strings.
    pub secondary: Vec<(Secondary, Vec<Id>)>,
    /// Values export sets (footnote numbers, references).
    pub props: HashMap<&'static str, String>,
}

/// The tree.
#[derive(Debug, Clone)]
pub struct Tree {
    /// Every node; removed ones stay, unreachable.
    pub nodes: Vec<Node>,
    /// The document.
    pub root: Id,
}

fn contains_objects(k: SyntaxKind) -> bool {
    matches!(
        k,
        PARAGRAPH
            | VERSE_BLOCK
            | TABLE_CELL
            | BOLD
            | ITALIC
            | UNDERLINE
            | STRIKE_THROUGH
            | SUBSCRIPT
            | SUPERSCRIPT
            | LINK
            | FOOTNOTE_REFERENCE
            | RADIO_TARGET
            | HEADLINE_TITLE
            | ITEM_TAG
            | KEYWORD_VALUE
    )
}

impl Tree {
    /// The export tree of a parse.
    pub fn build(root: &SyntaxNode) -> Tree {
        let mut t = Tree {
            nodes: Vec::new(),
            root: 0,
        };
        t.root = t.node(root, None);
        t
    }

    fn push(&mut self, node: Node) -> Id {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    /// A text node.
    pub fn text_node(&mut self, text: String, parent: Option<Id>) -> Id {
        self.push(Node {
            kind: Kind::Text,
            syntax: None,
            text,
            children: Vec::new(),
            parent,
            post_blank: 0,
            secondary: Vec::new(),
            props: HashMap::new(),
        })
    }

    /// A raw node.
    pub fn raw_node(&mut self, text: String, parent: Option<Id>) -> Id {
        let id = self.text_node(text, parent);
        self.nodes[id].kind = Kind::Raw;
        id
    }

    /// A node of kind `k` made by export (a citation's emphasis, the
    /// footnote wrapping it), holding `children`.
    pub fn made_node(&mut self, k: SyntaxKind, children: Vec<Id>, parent: Option<Id>) -> Id {
        let id = self.push(Node {
            kind: Kind::Node(k),
            syntax: None,
            text: String::new(),
            children: children.clone(),
            parent,
            post_blank: 0,
            secondary: Vec::new(),
            props: HashMap::new(),
        });
        for c in children {
            self.nodes[c].parent = Some(id);
        }
        id
    }

    /// The objects and plain text of syntax node `n` (a citation's prefix
    /// or suffix), outside the tree.
    pub fn objects_of(&mut self, n: &SyntaxNode) -> Vec<Id> {
        let root = self.root;
        let ids = self.objects(n, Some(n.text_range()), root);
        for &i in &ids {
            self.nodes[i].parent = None;
        }
        ids
    }

    /// Adds `n`, from another parse, and what it holds under `parent`
    /// (not among its children: the caller places it).
    pub fn graft(&mut self, n: &SyntaxNode, parent: Id) -> Id {
        self.node(n, Some(parent))
    }

    fn node(&mut self, n: &SyntaxNode, parent: Option<Id>) -> Id {
        crate::deep(|| self.node_unguarded(n, parent))
    }

    fn node_unguarded(&mut self, n: &SyntaxNode, parent: Option<Id>) -> Id {
        let kind = n.kind();
        let id = self.push(Node {
            kind: Kind::Node(kind),
            syntax: Some(n.clone()),
            text: String::new(),
            children: Vec::new(),
            parent,
            post_blank: if kind.is_element() || kind.is_object() {
                ast::post_blank(n)
            } else {
                0
            },
            secondary: Vec::new(),
            props: HashMap::new(),
        });
        let children = match kind {
            HEADLINE_TITLE | ITEM_TAG | KEYWORD_VALUE => self.objects(n, Some(n.text_range()), id),
            _ if contains_objects(kind) => match ast::contents_range(n) {
                Some(r) => self.objects(n, Some(r), id),
                None => Vec::new(),
            },
            TABLE_ROW => n
                .children()
                .filter(|c| c.kind() == TABLE_CELL)
                .map(|c| self.node(&c, Some(id)))
                .collect(),
            _ => match ast::contents_range(n) {
                Some(r) => n
                    .children()
                    .filter(|c| {
                        (c.kind().is_element() || c.kind() == TABLE_ROW)
                            && r.contains_range(c.text_range())
                    })
                    .map(|c| self.node(&c, Some(id)))
                    .collect(),
                None => Vec::new(),
            },
        };
        self.nodes[id].children = children;
        // Secondary strings.
        let mut sec = Vec::new();
        if matches!(kind, HEADLINE | INLINETASK)
            && let Some(t) = n.children().find(|c| c.kind() == HEADLINE_TITLE)
        {
            let ids = self.objects(&t, Some(t.text_range()), id);
            sec.push((Secondary::Title, ids));
        }
        if kind == ITEM
            && let Some(t) = n.children().find(|c| c.kind() == ITEM_TAG)
        {
            let ids = self.objects(&t, Some(t.text_range()), id);
            sec.push((Secondary::Tag, ids));
        }
        if kind.is_element() {
            let mut i = 0;
            for k in n.children().filter(|c| c.kind() == AFFILIATED_KEYWORD) {
                let key = ast::AstNode::cast(k.clone())
                    .map(|a: ast::AffiliatedKeyword| a.key())
                    .unwrap_or_default();
                if key != "CAPTION" {
                    continue;
                }
                let values: Vec<SyntaxNode> =
                    k.children().filter(|c| c.kind() == KEYWORD_VALUE).collect();
                // `#+CAPTION[short]: long`: the short value comes first.
                if let Some(long) = values.last() {
                    let ids = self.objects(long, Some(long.text_range()), id);
                    sec.push((Secondary::Caption(i), ids));
                }
                if values.len() > 1 {
                    let ids = self.objects(&values[0], Some(values[0].text_range()), id);
                    sec.push((Secondary::ShortCaption(i), ids));
                }
                i += 1;
            }
        }
        self.nodes[id].secondary = sec;
        id
    }

    /// The objects and plain text of `n` (within `range`, if given).
    fn objects(&mut self, n: &SyntaxNode, range: Option<TextRange>, parent: Id) -> Vec<Id> {
        let Some(range) = range.or(Some(n.text_range())) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut text = String::new();
        for c in n.children_with_tokens() {
            let r = c.text_range();
            if r.end() <= range.start() && !(r.is_empty() && r.start() == range.start()) {
                continue;
            }
            if r.start() >= range.end() && !r.is_empty() {
                break;
            }
            if r.start() < range.start() || r.end() > range.end() {
                // A token cut by the range.
                if let NodeOrToken::Token(t) = &c {
                    let s = usize::from(range.start().max(r.start()) - r.start());
                    let e = usize::from(range.end().min(r.end()) - r.start());
                    text.push_str(&t.text()[s..e]);
                }
                continue;
            }
            match c {
                NodeOrToken::Token(t) => text.push_str(t.text()),
                NodeOrToken::Node(child) if child.kind().is_object() => {
                    if !text.is_empty() {
                        out.push(self.text_node(std::mem::take(&mut text), Some(parent)));
                    }
                    out.push(self.node(&child, Some(parent)));
                }
                NodeOrToken::Node(child) => text.push_str(&child.text().to_string()),
            }
        }
        if !text.is_empty() {
            out.push(self.text_node(text, Some(parent)));
        }
        out
    }

    /// The node's element or object type.
    pub fn kind(&self, id: Id) -> Option<SyntaxKind> {
        match self.nodes[id].kind {
            Kind::Node(k) => Some(k),
            _ => None,
        }
    }

    /// Whether the node is plain text.
    pub fn is_text(&self, id: Id) -> bool {
        self.nodes[id].kind == Kind::Text
    }

    /// The node's syntax node.
    pub fn syntax(&self, id: Id) -> Option<&SyntaxNode> {
        self.nodes[id].syntax.as_ref()
    }

    /// The node's contents.
    pub fn children(&self, id: Id) -> &[Id] {
        &self.nodes[id].children
    }

    /// The node's parent.
    pub fn parent(&self, id: Id) -> Option<Id> {
        self.nodes[id].parent
    }

    /// A secondary string of the node.
    pub fn secondary(&self, id: Id, which: Secondary) -> Option<&[Id]> {
        self.nodes[id]
            .secondary
            .iter()
            .find(|(s, _)| *s == which)
            .map(|(_, v)| v.as_slice())
    }

    /// Whether `id` is an object (or plain text), not an element.
    pub fn is_object(&self, id: Id) -> bool {
        match self.nodes[id].kind {
            Kind::Node(k) => k.is_object(),
            _ => true,
        }
    }

    /// The node's ancestors, nearest first.
    pub fn ancestors(&self, id: Id) -> impl Iterator<Item = Id> + '_ {
        std::iter::successors(self.parent(id), |p| self.parent(*p))
    }

    /// The nearest ancestor of kind `k`.
    pub fn ancestor(&self, id: Id, k: SyntaxKind) -> Option<Id> {
        self.ancestors(id).find(|a| self.kind(*a) == Some(k))
    }

    /// The contents list holding `id`: its parent's contents, or the
    /// secondary string it belongs to.
    fn siblings(&self, id: Id) -> Option<&[Id]> {
        let p = self.parent(id)?;
        let n = &self.nodes[p];
        if n.children.contains(&id) {
            return Some(&n.children);
        }
        n.secondary
            .iter()
            .find(|(_, v)| v.contains(&id))
            .map(|(_, v)| v.as_slice())
    }

    /// The sibling before `id`.
    pub fn previous(&self, id: Id) -> Option<Id> {
        let s = self.siblings(id)?;
        let i = s.iter().position(|x| *x == id)?;
        i.checked_sub(1).map(|j| s[j])
    }

    /// The sibling after `id`.
    pub fn next(&self, id: Id) -> Option<Id> {
        let s = self.siblings(id)?;
        let i = s.iter().position(|x| *x == id)?;
        s.get(i + 1).copied()
    }

    /// Takes `id` out of its parent's contents (or secondary string).
    pub fn extract(&mut self, id: Id) {
        let Some(p) = self.parent(id) else { return };
        let n = &mut self.nodes[p];
        n.children.retain(|x| *x != id);
        for (_, v) in &mut n.secondary {
            v.retain(|x| *x != id);
        }
    }

    /// Puts `new` before `id` in its contents.
    pub fn insert_before(&mut self, id: Id, new: Id) {
        let Some(p) = self.parent(id) else { return };
        self.nodes[new].parent = Some(p);
        let n = &mut self.nodes[p];
        if let Some(i) = n.children.iter().position(|x| *x == id) {
            n.children.insert(i, new);
            return;
        }
        for (_, v) in &mut n.secondary {
            if let Some(i) = v.iter().position(|x| *x == id) {
                v.insert(i, new);
                return;
            }
        }
    }

    /// The nodes under `id` in the order of `org-element-map`: `id`, its
    /// secondary strings, then its contents, depth first.
    pub fn descendants(&self, id: Id) -> Vec<Id> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(x) = stack.pop() {
            out.push(x);
            let n = &self.nodes[x];
            // Secondary strings come before contents in `org-element-map`.
            let mut next: Vec<Id> = Vec::new();
            for (_, v) in &n.secondary {
                next.extend(v);
            }
            next.extend(&n.children);
            stack.extend(next.into_iter().rev());
        }
        out
    }

    /// The first of `descendants(id)` for which `f` holds, without
    /// walking past it (`org-element-map` with FIRST-MATCH).
    pub fn find_descendant(&self, id: Id, f: impl Fn(Id) -> bool) -> Option<Id> {
        if f(id) {
            return Some(id);
        }
        // Each node being walked, with the list it is in (its secondary
        // strings, then its contents) and the place in that list, so that
        // a node of many children costs nothing until they are reached.
        let mut stack: Vec<(Id, usize, usize)> = vec![(id, 0, 0)];
        while let Some(top) = stack.last_mut() {
            let (x, list, i) = *top;
            let n = &self.nodes[x];
            let items: &[Id] = match list.cmp(&n.secondary.len()) {
                std::cmp::Ordering::Less => &n.secondary[list].1,
                std::cmp::Ordering::Equal => &n.children,
                std::cmp::Ordering::Greater => {
                    stack.pop();
                    continue;
                }
            };
            let Some(&c) = items.get(i) else {
                *top = (x, list + 1, 0);
                continue;
            };
            top.2 += 1;
            if f(c) {
                return Some(c);
            }
            stack.push((c, 0, 0));
        }
        None
    }

    /// The text of a node's plain text and objects, as written.
    pub fn source(&self, id: Id) -> String {
        match &self.nodes[id].syntax {
            Some(s) => s.text().to_string(),
            None => self.nodes[id].text.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_descendant_walks_as_descendants() {
        let text = "#+TITLE: T\n* Head *bold* [[a][b]] :t:\n:PROPERTIES:\n:X: 1\n:END:\nSee [[x]] and *[[y][z]]*.[fn:1]\n- item [[w]]\n  | [[v]] | c |\n\n[fn:1] Note [[u]].\n** Sub [[t]]\n";
        let parse = org_syntax::parse(text);
        let tree = Tree::build(&parse.syntax());
        let kinds: Vec<Option<SyntaxKind>> = (0..tree.nodes.len()).map(|i| tree.kind(i)).collect();
        for id in 0..tree.nodes.len() {
            for want in &kinds {
                assert_eq!(
                    tree.find_descendant(id, |d| tree.kind(d) == *want),
                    tree.descendants(id)
                        .into_iter()
                        .find(|&d| tree.kind(d) == *want),
                    "{id} {want:?}"
                );
            }
        }
    }
}
