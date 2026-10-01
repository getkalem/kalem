//! The Kalem format on the mode contract (T2.13.3): `klm-syntax`'s tree
//! as the contract's kinds and ranges, its canonical form for Format
//! Document and `kalem fmt`, its diagnostics and HTML. An edit reparses
//! from the enclosing top-level block when the last parse is the text
//! before it.

use std::sync::Mutex;

use klm_syntax::{Attr, Body, Command, Document, Inline, Node};

use crate::modes::{Detect, Kind, ModeDiagnostic, ModeSpec, TextEdit, Tree};

/// The Kalem format (`.klm`).
#[derive(Debug, Default)]
pub struct KlmMode {
    /// The last text parsed and its tree, for the next edit.
    last: Mutex<Option<(String, Document)>>,
}

impl KlmMode {
    /// The `klm-syntax` tree of `text`, reparsed from the last one when
    /// `edit` turns the last text into it.
    pub fn document(&self, text: &str, edit: Option<&TextEdit>) -> Document {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        let doc = match (last.take(), edit) {
            (Some((old, doc)), Some(e))
                if e.range.end <= old.len()
                    && old.len() - (e.range.end - e.range.start) + e.len == text.len()
                    && text.as_bytes().get(..e.range.start)
                        == old.as_bytes().get(..e.range.start)
                    && text.as_bytes()[e.range.start + e.len..]
                        == old.as_bytes()[e.range.end..] =>
            {
                klm_syntax::reparse(doc, &old, (e.range.start, e.range.end), text)
            }
            _ => klm_syntax::parse(text),
        };
        *last = Some((text.to_string(), doc.clone()));
        doc
    }
}

/// Whether `doc` is a Kalem format file (`.klm`, or text starting with
/// `\klm[`).
pub fn is_klm_file(doc: &crate::DocumentState) -> bool {
    doc.meta
        .path
        .as_deref()
        .and_then(|p| p.extension())
        .is_some_and(|e| e.eq_ignore_ascii_case("klm"))
        || doc.text().as_str().starts_with("\\klm[")
}

/// The contract's kind of command `c`.
fn kind_of(c: &Command, inline: bool) -> Kind {
    let attr = |key: &str| {
        c.attrs
            .iter()
            .zip(&c.attr_ranges)
            .find_map(|(a, r)| match a {
                Attr::Key(k, v) if k == key => Some((v.clone(), *r)),
                _ => None,
            })
    };
    let positional = || {
        c.attrs
            .iter()
            .zip(&c.attr_ranges)
            .find_map(|(a, r)| matches!(a, Attr::Positional(_)).then_some(r.0..r.1))
    };
    let style = |s: &str| {
        c.attrs
            .iter()
            .any(|a| matches!(a, Attr::Style(x) if x == s))
    };
    match c.name.as_str() {
        h if h.len() == 2 && h.starts_with('h') && h.as_bytes()[1].is_ascii_digit() => {
            Kind::Heading(h.as_bytes()[1] - b'0')
        }
        "p" => Kind::Paragraph,
        "ul" => Kind::List { ordered: false },
        "ol" => Kind::List { ordered: true },
        "li" => Kind::ListItem {
            checkbox: attr("state").map(|(v, _)| v == "done"),
        },
        "block" if style("quote") => Kind::Quote,
        "code" if inline => Kind::InlineCode,
        "code" => Kind::Code {
            language: attr("lang").map(|(v, _)| v),
        },
        "eq" => Kind::MathBlock,
        "table" => Kind::Table,
        "tr" => Kind::TableRow,
        "td" | "th" => Kind::TableCell,
        "hr" => Kind::Rule,
        "b" => Kind::Strong,
        "i" => Kind::Emphasis,
        "link" => Kind::Link {
            target: positional(),
        },
        "img" => Kind::Image {
            target: positional(),
        },
        "fn" | "fnref" => Kind::FootnoteRef,
        other => Kind::Other(other.to_string()),
    }
}

/// `c` and its content as nodes under `parent`: the delimiters (the name,
/// the attributes and `{`, and `}`) as hidden markers.
fn command(tree: &mut Tree, c: &Command, inline: bool, parent: Option<u32>) {
    let me = tree.push(kind_of(c, inline), c.range.0..c.range.1, parent);
    let open_end = c
        .body_range
        .map(|b| b.0)
        .or(c.attrs_range.map(|a| a.1))
        .unwrap_or(c.name_range.1);
    tree.push(Kind::HiddenMarker, c.range.0..open_end, Some(me));
    match &c.body {
        Body::Blocks(b) => blocks(tree, b, Some(me)),
        Body::Inline(i) => inlines(tree, i, Some(me)),
        _ => {}
    }
    if let Some(b) = c.body_range
        && b.1 < c.range.1
    {
        tree.push(Kind::HiddenMarker, b.1..c.range.1, Some(me));
    }
}

fn inlines(tree: &mut Tree, inl: &[Inline], parent: Option<u32>) {
    for i in inl {
        match i {
            Inline::Text(..) => {}
            Inline::Math(_, r) => {
                let m = tree.push(Kind::Math, r.0..r.1, parent);
                tree.push(Kind::HiddenMarker, r.0..r.0 + 1, Some(m));
                if r.1 > r.0 + 1 && r.1 > 0 {
                    tree.push(Kind::HiddenMarker, r.1 - 1..r.1, Some(m));
                }
            }
            Inline::Command(c) => command(tree, c, true, parent),
        }
    }
}

fn blocks(tree: &mut Tree, nodes: &[Node], parent: Option<u32>) {
    for n in nodes {
        match n {
            Node::Paragraph(inl, r) => {
                let p = tree.push(Kind::Paragraph, r.0..r.1, parent);
                inlines(tree, inl, Some(p));
            }
            Node::Block(c) => command(tree, c, false, parent),
        }
    }
}

/// The contract's tree of a `klm-syntax` document.
pub fn tree(text: &str, doc: &Document) -> Tree {
    let mut t = Tree::default();
    if doc.version.is_some() {
        let end = text.find('\n').unwrap_or(text.len());
        t.push(Kind::Other("klm".into()), 0..end, None);
    }
    blocks(&mut t, &doc.blocks, None);
    t
}

impl ModeSpec for KlmMode {
    fn id(&self) -> &'static str {
        "klm"
    }

    fn detect(&self) -> Detect {
        Detect {
            extensions: &["klm"],
            sniff: Some(|head| head.starts_with(b"\\klm[")),
        }
    }

    fn parse(&self, text: &str, edit: Option<&TextEdit>, _previous: Option<&Tree>) -> Tree {
        tree(text, &self.document(text, edit))
    }

    fn outline(&self, text: &str, _tree: &Tree) -> Vec<crate::view::OutlineItem> {
        let doc = self.document(text, None);
        let mut out = Vec::new();
        klm_syntax::visit_commands(&doc.blocks, &mut |c| {
            if let Kind::Heading(level) = kind_of(c, false)
                && let Some(b) = c.body_range
            {
                out.push(crate::view::OutlineItem {
                    level: usize::from(level),
                    todo: None,
                    title: text[b.0..b.1].trim().to_string(),
                    start: c.range.0,
                });
            }
        });
        out
    }

    fn format(&self, text: &str) -> Option<String> {
        Some(klm_syntax::fmt(&self.document(text, None)))
    }

    fn diagnostics(&self, text: &str) -> Vec<ModeDiagnostic> {
        self.document(text, None)
            .diagnostics
            .iter()
            .map(|d| ModeDiagnostic {
                range: d.range.0..d.range.1,
                code: d.code.to_string(),
                message: d.message.clone(),
            })
            .collect()
    }

    fn to_html(&self, text: &str) -> Option<String> {
        Some(klm_syntax::html(&self.document(text, None)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<String> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/klm-spec");
        let mut out = Vec::new();
        for sub in ["samples", "spec"] {
            for e in std::fs::read_dir(dir.join(sub)).unwrap() {
                let p = e.unwrap().path();
                if p.extension().is_some_and(|x| x == "klm") {
                    out.push(std::fs::read_to_string(p).unwrap());
                }
            }
        }
        out
    }

    #[test]
    fn the_kalem_format_on_the_contract() {
        let modes = crate::modes::Modes::with_builtins();
        let klm = modes.detect(Some("thesis.klm"), b"").unwrap();
        assert_eq!(klm.id(), "klm");
        assert_eq!(modes.detect(None, b"\\klm[1.0]\n").unwrap().id(), "klm");
        for text in samples() {
            let mid = text.len() / 2;
            let mid = (0..=mid).rev().find(|&i| text.is_char_boundary(i)).unwrap();
            crate::modes::check(
                klm,
                &text,
                &[
                    (mid..mid, "x"),
                    (mid..mid, "\n\n"),
                    (mid..mid, "}"),
                    (0..0, "\\h1{A}\n"),
                ],
            )
            .unwrap();
        }
        // A heading: its kind, the title in the outline, its markers.
        let text = "\\klm[1.0]\n\n\\h2[#a]{Results}\n\nSome \\b{bold} $x$.\n";
        let tree = klm.parse(text, None, None);
        let heading = tree
            .nodes
            .iter()
            .find(|n| n.kind == Kind::Heading(2))
            .unwrap();
        assert_eq!(&text[heading.range.clone()], "\\h2[#a]{Results}");
        let markers: Vec<&str> = tree
            .nodes
            .iter()
            .filter(|n| n.kind == Kind::HiddenMarker)
            .map(|n| &text[n.range.clone()])
            .collect();
        assert_eq!(markers, ["\\h2[#a]{", "}", "\\b{", "}", "$", "$"]);
        let outline = klm.outline(text, &tree);
        assert_eq!(outline[0].title, "Results");
        assert_eq!(
            klm.format("\\h1{A}\n\n\n\nText\nhere.\n").unwrap(),
            "\\h1{A}\n\nText here.\n"
        );
    }
}
