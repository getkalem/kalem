//! Debugging and differential testing helpers.
//!
//! [`emacs_json`] writes the parse tree in the JSON format produced by
//! `tests/emacs/dump.el`, so Kalem and Emacs can be compared node by node,
//! including properties. It is built on the public tree and the [`ast`]
//! accessors, so the comparison also checks the typed API.
//!
//! [`ast`]: crate::ast

use std::fmt::Write;

use rowan::NodeOrToken;

use crate::SyntaxKind::{self, *};
use crate::ast;
use crate::context::ParseContext;
use crate::{SyntaxNode, parse_with};

/// A minimal JSON value, enough for the dump.
enum J {
    Null,
    True,
    Num(i64),
    Str(String),
    Arr(Vec<J>),
}

impl J {
    fn opt_str(s: Option<String>) -> J {
        s.map_or(J::Null, J::Str)
    }
    fn flag(b: bool) -> J {
        if b { J::True } else { J::Null }
    }
    fn write(&self, out: &mut String) {
        match self {
            J::Null => out.push_str("null"),
            J::True => out.push_str("true"),
            J::Num(n) => write!(out, "{n}").unwrap(),
            J::Str(s) => write_str(s, out),
            J::Arr(v) => {
                out.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    x.write(out);
                }
                out.push(']');
            }
        }
    }
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => write!(out, "\\u{:04x}", c as u32).unwrap(),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Parses `text` and returns the tree in `dump.el`'s JSON format.
pub fn emacs_json(text: &str, ctx: &ParseContext) -> String {
    let parse = parse_with(text, ctx);
    let root = parse.syntax();
    let mut out = String::new();
    write!(out, "{{\"size\":{},\"children\":", text.len()).unwrap();
    nodes(
        root.children().filter(|c| is_dumped(c.kind())).collect(),
        ctx,
        &mut out,
    );
    out.push('}');
    out
}

fn is_dumped(kind: SyntaxKind) -> bool {
    kind.org_element_type().is_some() && kind != DOCUMENT
}

fn nodes(list: Vec<SyntaxNode>, ctx: &ParseContext, out: &mut String) {
    out.push('[');
    for (i, n) in list.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        node(n, ctx, out);
    }
    out.push(']');
}

fn opt(v: Option<u32>) -> String {
    v.map_or_else(|| "null".to_string(), |v| v.to_string())
}

/// Children dumped as secondary strings, keyed by property name.
fn secondary(n: &SyntaxNode) -> Vec<(&'static str, Vec<SyntaxNode>)> {
    let mut out: Vec<(&'static str, Vec<SyntaxNode>)> = Vec::new();
    let mut push = |k: &'static str, v: Vec<SyntaxNode>| match out.iter_mut().find(|(x, _)| *x == k)
    {
        Some((_, e)) => e.extend(v),
        None => out.push((k, v)),
    };
    let mut last_key: Option<String> = None;
    for e in n.children_with_tokens() {
        match e {
            NodeOrToken::Token(t) if t.kind() == KEY => last_key = Some(t.text().to_string()),
            NodeOrToken::Token(_) => {}
            NodeOrToken::Node(c) => match (n.kind(), c.kind()) {
                (HEADLINE | INLINETASK, HEADLINE_TITLE) => push("title", c.children().collect()),
                (ITEM, ITEM_TAG) => push("tag", c.children().collect()),
                (CITATION | CITATION_REFERENCE, CITATION_PREFIX) => {
                    push("prefix", c.children().collect())
                }
                (CITATION | CITATION_REFERENCE, CITATION_SUFFIX) => {
                    push("suffix", c.children().collect())
                }
                (CLOCK, TIMESTAMP) => push("value", vec![c]),
                (PLANNING, TIMESTAMP) => {
                    let k = match last_key.as_deref() {
                        Some("SCHEDULED:") => "scheduled",
                        Some("DEADLINE:") => "deadline",
                        _ => "closed",
                    };
                    push(k, vec![c]);
                }
                _ => {}
            },
        }
    }
    out
}

fn is_secondary_child(parent: SyntaxKind, child: SyntaxKind) -> bool {
    matches!(
        (parent, child),
        (HEADLINE | INLINETASK, HEADLINE_TITLE)
            | (ITEM, ITEM_TAG)
            | (
                CITATION | CITATION_REFERENCE,
                CITATION_PREFIX | CITATION_SUFFIX
            )
            | (CLOCK | PLANNING, TIMESTAMP)
    )
}

fn node(n: &SyntaxNode, ctx: &ParseContext, out: &mut String) {
    crate::deep(|| node_inner(n, ctx, out))
}

fn node_inner(n: &SyntaxNode, ctx: &ParseContext, out: &mut String) {
    let kind = n.kind();
    let ty = kind.org_element_type().expect("dumped kinds have a type");
    let r = n.text_range();
    let c = ast::contents_range(n);
    let pa = if kind.is_element() {
        opt(Some(u32::from(ast::post_affiliated(n))))
    } else {
        "null".into()
    };
    write!(
        out,
        "{{\"type\":\"{ty}\",\"begin\":{},\"end\":{},\"cb\":{},\"ce\":{},\"pa\":{pa},\"pb\":{},\"props\":{{",
        u32::from(r.start()),
        u32::from(r.end()),
        opt(c.map(|c| u32::from(c.start()))),
        opt(c.map(|c| u32::from(c.end()))),
        ast::post_blank(n)
    )
    .unwrap();
    for (i, (k, v)) in props(n, ctx).into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_str(k, out);
        out.push(':');
        v.write(out);
    }
    out.push('}');
    let contents: Vec<SyntaxNode> = n
        .children()
        .filter(|c| is_dumped(c.kind()) && !is_secondary_child(kind, c.kind()))
        .collect();
    if !contents.is_empty() {
        out.push_str(",\"children\":");
        nodes(contents, ctx, out);
    }
    let sec = secondary(n);
    if !sec.is_empty() {
        out.push_str(",\"secondary\":{");
        for (i, (k, v)) in sec.into_iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            write!(out, "\"{k}\":").unwrap();
            nodes(
                v.into_iter().filter(|c| is_dumped(c.kind())).collect(),
                ctx,
                out,
            );
        }
        out.push('}');
    }
    out.push('}');
}

fn s(v: impl Into<String>) -> J {
    J::Str(v.into())
}

fn arr(v: Vec<String>) -> J {
    if v.is_empty() {
        J::Null
    } else {
        J::Arr(v.into_iter().map(J::Str).collect())
    }
}

fn props(n: &SyntaxNode, ctx: &ParseContext) -> Vec<(&'static str, J)> {
    use ast::*;
    let mut p: Vec<(&'static str, J)> = Vec::new();
    macro_rules! heading {
        ($h:expr) => {{
            let h = $h;
            p.push(("level", J::Num(h.level(ctx) as i64)));
            p.push((
                "todo-keyword",
                J::opt_str(h.todo_keyword().map(|t| t.text().to_string())),
            ));
            p.push((
                "todo-type",
                match h.todo_type(ctx) {
                    Some(TodoType::Todo) => s("todo"),
                    Some(TodoType::Done) => s("done"),
                    None => J::Null,
                },
            ));
            p.push((
                "priority",
                h.priority().map_or(J::Null, |c| J::Num(c as i64)),
            ));
            p.push(("tags", arr(h.tags())));
            p.push(("raw-value", s(h.raw_value())));
        }};
    }
    match n.kind() {
        HEADLINE => {
            let h = Headline::cast(n.clone()).expect("kind");
            heading!(&h);
            p.push(("commentedp", J::flag(h.is_commented())));
            p.push(("archivedp", J::flag(h.is_archived())));
            p.push(("footnote-section-p", J::flag(h.is_footnote_section(ctx))));
            p.push(("pre-blank", J::Num(h.pre_blank() as i64)));
        }
        INLINETASK => heading!(&Inlinetask::cast(n.clone()).expect("kind")),
        KEYWORD => {
            let k = Keyword::cast(n.clone()).expect("kind");
            p.push(("key", s(k.key())));
            p.push(("value", s(k.value())));
        }
        BABEL_CALL => {
            let b = BabelCall::cast(n.clone()).expect("kind");
            p.push(("call", J::opt_str(b.call())));
            p.push(("inside-header", J::opt_str(b.inside_header())));
            p.push(("arguments", J::opt_str(b.arguments())));
            p.push(("end-header", J::opt_str(b.end_header())));
            p.push(("value", s(b.value())));
        }
        SRC_BLOCK => {
            let b = SrcBlock::cast(n.clone()).expect("kind");
            p.push(("language", J::opt_str(b.language())));
            p.push(("switches", J::opt_str(b.switches())));
            p.push(("parameters", J::opt_str(b.parameters())));
            p.push(("value", s(b.value())));
        }
        EXAMPLE_BLOCK => {
            let b = ExampleBlock::cast(n.clone()).expect("kind");
            p.push(("switches", J::opt_str(b.switches())));
            p.push(("value", s(b.value())));
        }
        EXPORT_BLOCK => {
            let b = ExportBlock::cast(n.clone()).expect("kind");
            p.push(("type", J::opt_str(b.backend())));
            p.push(("value", s(b.value())));
        }
        SPECIAL_BLOCK => p.push((
            "type",
            s(SpecialBlock::cast(n.clone()).expect("kind").block_type()),
        )),
        COMMENT_BLOCK => p.push((
            "value",
            s(CommentBlock::cast(n.clone()).expect("kind").value()),
        )),
        DYNAMIC_BLOCK => {
            let b = DynamicBlock::cast(n.clone()).expect("kind");
            p.push(("block-name", s(b.block_name())));
            p.push(("arguments", J::opt_str(b.arguments())));
        }
        DRAWER => p.push((
            "drawer-name",
            s(Drawer::cast(n.clone()).expect("kind").name()),
        )),
        NODE_PROPERTY => {
            let x = NodeProperty::cast(n.clone()).expect("kind");
            p.push(("key", s(x.key())));
            p.push(("value", s(x.value())));
        }
        PLAIN_LIST => p.push((
            "type",
            s(
                match PlainList::cast(n.clone()).expect("kind").list_type() {
                    ListType::Ordered => "ordered",
                    ListType::Unordered => "unordered",
                    ListType::Descriptive => "descriptive",
                },
            ),
        )),
        ITEM => {
            let i = Item::cast(n.clone()).expect("kind");
            p.push(("bullet", s(i.bullet())));
            p.push((
                "checkbox",
                match i.checkbox() {
                    Some(Checkbox::On) => s("on"),
                    Some(Checkbox::Off) => s("off"),
                    Some(Checkbox::Partial) => s("trans"),
                    None => J::Null,
                },
            ));
            p.push(("counter", i.counter().map_or(J::Null, |c| J::Num(c as i64))));
            p.push(("pre-blank", J::Num(i.pre_blank() as i64)));
        }
        TABLE => {
            let t = Table::cast(n.clone()).expect("kind");
            p.push((
                "type",
                s(if t.table_type() == TableType::Org {
                    "org"
                } else {
                    "table.el"
                }),
            ));
            p.push(("tblfm", arr(t.tblfm())));
            p.push(("value", J::opt_str(t.table_el_value())));
        }
        TABLE_ROW => p.push((
            "type",
            s(if TableRow::cast(n.clone()).expect("kind").is_rule() {
                "rule"
            } else {
                "standard"
            }),
        )),
        CLOCK => {
            let c = Clock::cast(n.clone()).expect("kind");
            p.push((
                "status",
                s(if c.status() == ClockStatus::Closed {
                    "closed"
                } else {
                    "running"
                }),
            ));
            p.push(("duration", J::opt_str(c.duration())));
        }
        FIXED_WIDTH => p.push((
            "value",
            s(FixedWidth::cast(n.clone()).expect("kind").value()),
        )),
        COMMENT => p.push(("value", s(Comment::cast(n.clone()).expect("kind").value()))),
        DIARY_SEXP => p.push((
            "value",
            s(DiarySexp::cast(n.clone()).expect("kind").value()),
        )),
        LATEX_ENVIRONMENT => p.push((
            "value",
            s(LatexEnvironment::cast(n.clone()).expect("kind").value()),
        )),
        FOOTNOTE_DEFINITION => {
            let f = FootnoteDefinition::cast(n.clone()).expect("kind");
            p.push(("label", s(f.label())));
            p.push(("pre-blank", J::Num(f.pre_blank() as i64)));
        }
        LINK => {
            let info = Link::cast(n.clone()).expect("kind").info(ctx);
            p.push(("type", s(info.link_type)));
            p.push(("path", s(info.path)));
            p.push(("raw-link", s(info.raw_link)));
            p.push((
                "format",
                s(match info.format {
                    LinkFormat::Bracket => "bracket",
                    // Emacs reports radio links as plain.
                    LinkFormat::Plain | LinkFormat::Radio => "plain",
                    LinkFormat::Angle => "angle",
                }),
            ));
            p.push(("search-option", J::opt_str(info.search_option)));
            p.push(("application", J::opt_str(info.application)));
        }
        TIMESTAMP => {
            let t = Timestamp::cast(n.clone()).expect("kind");
            p.push((
                "type",
                s(match t.timestamp_type() {
                    TimestampType::Active => "active",
                    TimestampType::ActiveRange => "active-range",
                    TimestampType::Inactive => "inactive",
                    TimestampType::InactiveRange => "inactive-range",
                    TimestampType::Diary => "diary",
                }),
            ));
            p.push(("raw-value", s(t.raw_value())));
            p.push((
                "range-type",
                match t.range_type() {
                    Some(RangeType::DateRange) => s("daterange"),
                    Some(RangeType::TimeRange) => s("timerange"),
                    None => J::Null,
                },
            ));
            let r = t.repeater();
            p.push((
                "repeater-type",
                r.map_or(J::Null, |r| {
                    s(match r.kind {
                        RepeaterType::Cumulate => "cumulate",
                        RepeaterType::CatchUp => "catch-up",
                        RepeaterType::Restart => "restart",
                    })
                }),
            ));
            p.push((
                "repeater-value",
                r.map_or(J::Null, |r| J::Num(r.value as i64)),
            ));
            p.push(("repeater-unit", r.map_or(J::Null, |r| s(r.unit.name()))));
            let w = t.warning();
            p.push((
                "warning-type",
                w.map_or(J::Null, |w| s(if w.first_only { "first" } else { "all" })),
            ));
            p.push((
                "warning-value",
                w.map_or(J::Null, |w| J::Num(w.value as i64)),
            ));
            p.push(("warning-unit", w.map_or(J::Null, |w| s(w.unit.name()))));
        }
        ENTITY => {
            let e = Entity::cast(n.clone()).expect("kind");
            p.push(("name", s(e.name())));
            p.push(("use-brackets-p", J::flag(e.uses_brackets())));
        }
        LATEX_FRAGMENT => p.push((
            "value",
            s(LatexFragment::cast(n.clone()).expect("kind").value()),
        )),
        EXPORT_SNIPPET => {
            let e = ExportSnippet::cast(n.clone()).expect("kind");
            p.push(("back-end", s(e.backend())));
            p.push(("value", s(e.value())));
        }
        FOOTNOTE_REFERENCE => {
            let f = FootnoteReference::cast(n.clone()).expect("kind");
            p.push(("label", J::opt_str(f.label())));
            p.push(("type", s(if f.is_inline() { "inline" } else { "standard" })));
        }
        INLINE_BABEL_CALL => {
            let c = InlineBabelCall::cast(n.clone()).expect("kind");
            p.push(("call", s(c.call())));
            p.push(("inside-header", J::opt_str(c.inside_header())));
            p.push(("arguments", J::opt_str(c.arguments())));
            p.push(("end-header", J::opt_str(c.end_header())));
        }
        INLINE_SRC_BLOCK => {
            let c = InlineSrcBlock::cast(n.clone()).expect("kind");
            p.push(("language", s(c.language())));
            p.push(("parameters", J::opt_str(c.parameters())));
            p.push(("value", s(c.value())));
        }
        MACRO => {
            let m = Macro::cast(n.clone()).expect("kind");
            p.push(("key", s(m.key())));
            let a = m.args();
            p.push((
                "args",
                if a.is_empty() {
                    J::Null
                } else {
                    J::Arr(a.into_iter().map(J::Str).collect())
                },
            ));
        }
        STATISTICS_COOKIE => p.push((
            "value",
            s(StatisticsCookie::cast(n.clone()).expect("kind").value()),
        )),
        TARGET => p.push(("value", s(Target::cast(n.clone()).expect("kind").value()))),
        RADIO_TARGET => p.push((
            "value",
            s(RadioTarget::cast(n.clone()).expect("kind").value()),
        )),
        CODE => p.push(("value", s(Code::cast(n.clone()).expect("kind").value()))),
        VERBATIM => p.push(("value", s(Verbatim::cast(n.clone()).expect("kind").value()))),
        SUBSCRIPT => p.push((
            "use-brackets-p",
            J::flag(Subscript::cast(n.clone()).expect("kind").uses_brackets()),
        )),
        SUPERSCRIPT => p.push((
            "use-brackets-p",
            J::flag(Superscript::cast(n.clone()).expect("kind").uses_brackets()),
        )),
        CITATION => p.push((
            "style",
            J::opt_str(Citation::cast(n.clone()).expect("kind").style()),
        )),
        CITATION_REFERENCE => p.push((
            "key",
            s(CitationReference::cast(n.clone()).expect("kind").key()),
        )),
        _ => {}
    }
    if n.kind().is_element()
        && let Some(name) = ast::element_name(n)
    {
        p.push(("name", s(name)));
    }
    p
}

/// A node's kind, begin, end, contents, post-affiliated and post-blank.
#[cfg(test)]
pub(crate) type Shape = (SyntaxKind, u32, u32, Option<(u32, u32)>, u32, u32);

/// The raw parser tree in the same format, for consistency tests.
#[cfg(test)]
pub(crate) fn raw_structure(text: &str, ctx: &ParseContext) -> Vec<Shape> {
    let raw = crate::parse_raw(text, ctx).0;
    let mut out = Vec::new();
    fn walk(r: &crate::raw::Raw, out: &mut Vec<Shape>) {
        if r.kind.org_element_type().is_some() && r.kind != DOCUMENT {
            let aff_end = r
                .children
                .iter()
                .filter(|c| c.kind == AFFILIATED_KEYWORD)
                .map(|c| c.end)
                .max();
            let pa = if r.kind.is_element() {
                aff_end.unwrap_or(r.pa).max(r.pa)
            } else {
                0
            };
            let c = match (r.cb, r.ce) {
                (Some(a), Some(b)) => Some((a as u32, b as u32)),
                _ => None,
            };
            out.push((
                r.kind,
                r.begin as u32,
                r.end as u32,
                c,
                pa as u32,
                r.pb as u32,
            ));
        }
        for c in &r.children {
            walk(c, out);
        }
    }
    walk(&raw, &mut out);
    out.sort_by_key(|x| (x.1, x.2, x.0));
    out
}

/// The same structure derived from the public tree.
#[cfg(test)]
pub(crate) fn tree_structure(text: &str, ctx: &ParseContext) -> Vec<Shape> {
    let root = parse_with(text, ctx).syntax();
    let mut out = Vec::new();
    for n in root.descendants() {
        let k = n.kind();
        if k.org_element_type().is_none() || k == DOCUMENT {
            continue;
        }
        let r = n.text_range();
        let c = ast::contents_range(&n).map(|c| (u32::from(c.start()), u32::from(c.end())));
        let pa = if k.is_element() {
            u32::from(ast::post_affiliated(&n))
        } else {
            0
        };
        out.push((
            k,
            u32::from(r.start()),
            u32::from(r.end()),
            c,
            pa,
            ast::post_blank(&n) as u32,
        ));
    }
    out.sort_by_key(|x| (x.1, x.2, x.0));
    out
}
