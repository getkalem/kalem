//! Org on the document mode contract (§11.11, T2.7c.10, roadmap R3.1):
//! the tree of `org-syntax` in the contract's kinds, the outline of the
//! view, the formatter of `org-edit`, the diagnostics of `org-lint` and
//! the keys Enter and Tab, all from the code the Org mode already has.

use std::ops::Range;

use org_syntax::SyntaxKind as K;
use org_syntax::ast::{self, AstNode};
use org_syntax::{ParseContext, SyntaxNode};

use crate::modes::{Detect, EditKey, Kind, ModeDiagnostic, ModeSpec, TextEdit, Tree};

/// Org on the contract.
#[derive(Debug, Default)]
pub struct OrgMode;

impl ModeSpec for OrgMode {
    fn id(&self) -> &'static str {
        "org"
    }

    fn detect(&self) -> Detect {
        Detect {
            extensions: &["org", "org_archive"],
            sniff: None,
        }
    }

    fn parse(&self, text: &str, _edit: Option<&TextEdit>, _previous: Option<&Tree>) -> Tree {
        to_tree(text)
    }

    fn edit(&self, text: &str, at: usize, key: EditKey) -> Option<Vec<(Range<usize>, String)>> {
        let doc = org_model::Document::new(org_syntax::parse(text));
        let tx = match key {
            EditKey::Enter => crate::input::enter(&doc, at, None).ok()?,
            EditKey::Tab => org_edit::table::next_field(&doc, at).ok()?,
            EditKey::BackTab => org_edit::table::previous_field(&doc, at).ok()?,
        };
        // A key that only moves the cursor is the editor's.
        (!tx.edits.is_empty()).then(|| tx.edits.into_iter().map(|e| (e.range, e.insert)).collect())
    }

    fn outline(&self, text: &str, _tree: &Tree) -> Vec<crate::view::OutlineItem> {
        crate::view::outline_items(&org_model::Document::new(org_syntax::parse(text)))
    }

    fn format(&self, text: &str) -> Option<String> {
        Some(org_edit::format::format(&org_model::Document::new(
            org_syntax::parse(text),
        )))
    }

    fn diagnostics(&self, text: &str) -> Vec<ModeDiagnostic> {
        org_syntax::parse(text)
            .diagnostics()
            .into_iter()
            .map(|d| ModeDiagnostic {
                range: usize::from(d.range.start())..usize::from(d.range.end()),
                code: d.code.into(),
                message: d.message,
            })
            .collect()
    }
}

fn span(n: &SyntaxNode) -> Range<usize> {
    let r = n.text_range();
    usize::from(r.start())..usize::from(r.end())
}

/// The tree of an Org document in the contract's kinds.
pub fn to_tree(text: &str) -> Tree {
    let parse = org_syntax::parse(text);
    let mut tree = Tree::default();
    walk(&parse.syntax(), None, parse.context(), &mut tree);
    tree
}

/// Pushes the nodes under `n` with `parent`.
fn walk(n: &SyntaxNode, parent: Option<u32>, ctx: &ParseContext, tree: &mut Tree) {
    for c in n.children() {
        let kind = kind_of(&c, ctx);
        let id = kind
            .clone()
            .map(|k| tree.push(k, span(&c), parent))
            .or(parent);
        match kind {
            // Verbatim: nothing inside.
            Some(Kind::Code { .. } | Kind::InlineCode | Kind::Math | Kind::MathBlock) => {}
            _ => walk(&c, id, ctx, tree),
        }
    }
}

/// Picture files a link without a description shows (`org-html-inline-image-rules`).
const PICTURES: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "tif", "tiff",
];

/// The contract's kind of node `n`, when it has one; `None` for the
/// syntax that only holds others (the document, sections, drawers).
fn kind_of(n: &SyntaxNode, ctx: &ParseContext) -> Option<Kind> {
    Some(match n.kind() {
        K::HEADLINE => {
            let level = ast::Headline::cast(n.clone())?.level(ctx);
            Kind::Heading(level.clamp(1, usize::from(u8::MAX)) as u8)
        }
        K::PARAGRAPH => Kind::Paragraph,
        K::PLAIN_LIST => {
            let first = n.children().find_map(ast::Item::cast);
            let ordered = first.is_some_and(|i| {
                i.bullet()
                    .trim_start()
                    .starts_with(|c: char| c.is_ascii_alphanumeric())
            });
            Kind::List { ordered }
        }
        K::ITEM => Kind::ListItem {
            checkbox: ast::Item::cast(n.clone())
                .and_then(|i| i.checkbox())
                .map(|c| c == ast::Checkbox::On),
        },
        K::QUOTE_BLOCK | K::VERSE_BLOCK => Kind::Quote,
        K::SRC_BLOCK => Kind::Code {
            language: ast::SrcBlock::cast(n.clone()).and_then(|s| s.language()),
        },
        K::EXAMPLE_BLOCK | K::FIXED_WIDTH => Kind::Code { language: None },
        K::TABLE => Kind::Table,
        K::TABLE_ROW => Kind::TableRow,
        K::TABLE_CELL => Kind::TableCell,
        K::LATEX_ENVIRONMENT => Kind::MathBlock,
        K::HORIZONTAL_RULE => Kind::Rule,
        K::BOLD => Kind::Strong,
        K::ITALIC => Kind::Emphasis,
        K::CODE | K::VERBATIM => Kind::InlineCode,
        K::LATEX_FRAGMENT => Kind::Math,
        K::FOOTNOTE_REFERENCE => Kind::FootnoteRef,
        K::LINK => link(n),
        // Containers: their children are the content.
        K::DOCUMENT | K::SECTION | K::HEADLINE_TITLE | K::ITEM_TAG => return None,
        k => Kind::Other(format!("{k:?}").to_lowercase()),
    })
}

/// A link, or a picture: a link to a picture file without a description.
fn link(n: &SyntaxNode) -> Kind {
    let target = n
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == K::CODE_TEXT)
        .map(|t| usize::from(t.text_range().start())..usize::from(t.text_range().end()));
    let described = ast::Link::cast(n.clone()).is_some_and(|l| l.description().is_some());
    let picture = target.as_ref().is_some_and(|r| {
        let t = n.text().to_string();
        let start = usize::from(n.text_range().start());
        t.get(r.start - start..r.end - start)
            .and_then(|s| s.rsplit_once('.'))
            .is_some_and(|(_, ext)| PICTURES.contains(&ext.to_ascii_lowercase().as_str()))
    });
    if picture && !described {
        Kind::Image { target }
    } else {
        Kind::Link { target }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "#+TITLE: Notes\n* Intro\nSome *bold* and /italic/ text, =code= and $x^2$.\n** TODO Lists\n- [X] One [[https://a.b][site]]\n- [ ] Two\n  1. Inner\n\n| a | b |\n|---+---|\n| c | *d* |\n\n#+begin_src rust\nfn main() {}\n#+end_src\n\n#+begin_quote\nQuoted.\n#+end_quote\n\n[[file:fig.png]] and a note[fn:1].\n-----\n\n[fn:1] The note.\n";

    #[test]
    fn org_on_the_contract() {
        let spec = OrgMode;
        let edits: Vec<(Range<usize>, &str)> = (0..DOC.len())
            .step_by(5)
            .flat_map(|i| [(i..i, "x"), (i..i, "\n* "), (i..(i + 3).min(DOC.len()), "")])
            .collect();
        crate::modes::check(&spec, DOC, &edits).unwrap();
        let tree = spec.parse(DOC, None, None);
        let kinds: Vec<&Kind> = tree.nodes.iter().map(|n| &n.kind).collect();
        assert!(kinds.contains(&&Kind::Heading(1)) && kinds.contains(&&Kind::Heading(2)));
        assert!(kinds.contains(&&Kind::List { ordered: false }));
        assert!(kinds.contains(&&Kind::List { ordered: true }));
        assert!(kinds.contains(&&Kind::ListItem {
            checkbox: Some(true)
        }));
        assert!(kinds.contains(&&Kind::ListItem {
            checkbox: Some(false)
        }));
        assert_eq!(kinds.iter().filter(|k| ***k == Kind::TableCell).count(), 4);
        assert!(kinds.contains(&&Kind::Code {
            language: Some("rust".into())
        }));
        assert!(kinds.contains(&&Kind::Quote) && kinds.contains(&&Kind::Rule));
        assert!(kinds.contains(&&Kind::Strong) && kinds.contains(&&Kind::Emphasis));
        assert!(kinds.contains(&&Kind::InlineCode) && kinds.contains(&&Kind::Math));
        assert!(kinds.contains(&&Kind::FootnoteRef));
        let target = |k: &Kind| match k {
            Kind::Link { target } | Kind::Image { target } => target.clone().map(|r| &DOC[r]),
            _ => None,
        };
        let targets: Vec<&str> = kinds.iter().filter_map(|k| target(k)).collect();
        assert_eq!(targets, ["https://a.b", "file:fig.png"]);
        assert!(kinds.iter().any(|k| matches!(k, Kind::Image { .. })));
        // The bold cell's markup is under its cell, the cell under its row.
        let bold = tree
            .nodes
            .iter()
            .rposition(|n| n.kind == Kind::Strong)
            .unwrap();
        let cell = tree.nodes[bold].parent.unwrap() as usize;
        assert_eq!(tree.nodes[cell].kind, Kind::TableCell);
        // The sub-heading is under the heading.
        let sub = kinds.iter().position(|k| **k == Kind::Heading(2)).unwrap();
        let up = tree.nodes[sub].parent.unwrap() as usize;
        assert_eq!(tree.nodes[up].kind, Kind::Heading(1));
        // The outline is the view's.
        let outline = spec.outline(DOC, &tree);
        let titles: Vec<&str> = outline.iter().map(|o| o.title.as_str()).collect();
        assert_eq!(titles.len(), 2, "{titles:?}");
        assert!(titles[1].contains("Lists"), "{titles:?}");
        // Enter in an item starts the next.
        let at = DOC.find("Two").unwrap() + 3;
        let edits = spec.edit(DOC, at, EditKey::Enter).expect("an item");
        assert!(edits.iter().any(|(_, s)| s.contains("- ")), "{edits:?}");
        // Format Document gives the canonical text.
        assert!(spec.format(DOC).is_some());
        let modes = crate::modes::Modes::with_builtins();
        assert_eq!(
            modes.detect(Some("a.org"), b"").map(|m| m.id()),
            Some("org")
        );
    }

    #[test]
    fn diagnostics_from_org_lint() {
        let d = OrgMode.diagnostics("#+begin_src\nnever closed\n");
        assert!(!d.is_empty());
    }

    /// The conformance check on every Org file of the corpus (R3.1).
    #[test]
    fn the_org_corpus_conforms() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/org-mode");
        let mut files = Vec::new();
        let mut stack = vec![dir];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "org") {
                    files.push(p);
                }
            }
        }
        assert!(files.len() > 10, "{files:?}");
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap();
            let mid = text.len() / 2;
            let mid = (0..=mid)
                .rev()
                .find(|&i| text.is_char_boundary(i))
                .unwrap_or(0);
            crate::modes::check(&OrgMode, &text, &[(mid..mid, "\n* x\n")])
                .unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        }
    }
}
