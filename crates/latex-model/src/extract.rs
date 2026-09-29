//! What each paragraph says to the model, as events in document order,
//! kept per paragraph: after an edit only the paragraphs whose trees
//! changed are read again (their green nodes are shared otherwise), and
//! numbering runs over the events.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use latex_syntax::{SyntaxElement, SyntaxKind::*, SyntaxNode, signatures};
use rowan::{GreenNode, NodeOrToken};

/// An argument of a command, in order.
#[derive(Debug, Clone)]
pub(crate) enum Arg {
    Star,
    Opt(String),
    Mand(String),
}

#[derive(Debug, Clone)]
pub(crate) enum Event {
    Class {
        name: String,
        options: Vec<String>,
        range: Range<usize>,
    },
    Package {
        name: String,
        options: Vec<String>,
        range: Range<usize>,
    },
    Section {
        level: i8,
        command: String,
        starred: bool,
        title: String,
        short: Option<String>,
        range: Range<usize>,
    },
    Appendix,
    FrontMatter,
    MainMatter,
    BackMatter,
    Label {
        name: String,
        range: Range<usize>,
    },
    Ref {
        command: String,
        keys: Vec<String>,
        range: Range<usize>,
    },
    Cite {
        command: String,
        keys: Vec<String>,
        notes: Vec<String>,
        range: Range<usize>,
    },
    Caption {
        text: String,
        short: Option<String>,
        range: Range<usize>,
    },
    Footnote {
        explicit: Option<String>,
        range: Range<usize>,
    },
    Macro {
        name: String,
        command: String,
        args: usize,
        default: Option<String>,
        body: String,
        range: Range<usize>,
    },
    NewEnvironment {
        name: String,
        args: usize,
        default: Option<String>,
        begin: String,
        end: String,
        range: Range<usize>,
    },
    TheoremDef {
        env: String,
        title: String,
        shared: Option<String>,
        within: Option<String>,
        numbered: bool,
    },
    Bibliography {
        command: String,
        files: Vec<String>,
        range: Range<usize>,
    },
    BibliographyStyle(String),
    SetCounter {
        counter: String,
        value: i64,
        add: bool,
    },
    NumberWithin {
        counter: String,
        within: Option<String>,
    },
    EnvEnter {
        name: String,
        range: Range<usize>,
        body: Range<usize>,
        note: Option<String>,
    },
    FootnoteEnd,
    EnvExit,
    LineBreak {
        at: usize,
    },
    NoNumber,
    Tag {
        text: String,
    },
}

/// Events of a paragraph, with those of the paragraphs inside it by
/// reference.
#[derive(Debug, Clone)]
pub(crate) enum Item {
    Event(Event),
    /// A paragraph inside, at this offset from the paragraph's start.
    Nested(usize, Arc<Vec<Item>>),
}

/// Events per paragraph, by the address of its green node (kept alive
/// with it).
#[derive(Debug, Default)]
pub(crate) struct Cache {
    map: HashMap<usize, (GreenNode, Arc<Vec<Item>>)>,
    used: HashMap<usize, (GreenNode, Arc<Vec<Item>>)>,
}

impl Cache {
    /// The events of the whole document, with positions from 0.
    pub(crate) fn document(&mut self, root: &SyntaxNode) -> Vec<Item> {
        let mut out = Vec::new();
        self.children(root, 0, &mut out);
        // Only what this version uses stays.
        self.map = std::mem::take(&mut self.used);
        out
    }

    /// The events of the paragraphs among `container`'s children.
    fn children(&mut self, container: &SyntaxNode, base: usize, out: &mut Vec<Item>) {
        for p in container.children().filter(|c| c.kind() == PARAGRAPH) {
            let start = usize::from(p.text_range().start());
            let items = self.paragraph(&p);
            out.push(Item::Nested(start - base, items));
        }
    }

    fn paragraph(&mut self, p: &SyntaxNode) -> Arc<Vec<Item>> {
        let green = p.green();
        let key = green as *const rowan::GreenNodeData as usize;
        if let Some(hit) = self.map.get(&key).or_else(|| self.used.get(&key)) {
            let hit = hit.clone();
            self.used.insert(key, hit.clone());
            return hit.1;
        }
        let green = green.to_owned();
        let base = usize::from(p.text_range().start());
        let mut items = Vec::new();
        self.walk(p, base, &mut items);
        let items = Arc::new(items);
        self.used.insert(key, (green, items.clone()));
        items
    }

    fn walk(&mut self, node: &SyntaxNode, base: usize, out: &mut Vec<Item>) {
        for child in node.children() {
            match child.kind() {
                COMMAND => {
                    if command(&child, base, out) {
                        self.walk(&child, base, out);
                    }
                    // A footnote's text is a group: what it numbers is
                    // forgotten after it.
                    if latex_syntax::name(&child).as_deref() == Some("footnote") {
                        out.push(Item::Event(Event::FootnoteEnd));
                    }
                }
                ENVIRONMENT => self.environment(&child, base, out),
                VERB => {}
                _ => self.walk(&child, base, out),
            }
        }
    }

    fn environment(&mut self, env: &SyntaxNode, base: usize, out: &mut Vec<Item>) {
        let name = latex_syntax::name(env).unwrap_or_default();
        let range = rel(env.text_range(), base);
        let body = env.children().find(|c| c.kind() == BODY);
        let body_range = body
            .as_ref()
            .map_or(range.end..range.end, |b| rel(b.text_range(), base));
        let begin = env.children().find(|c| c.kind() == BEGIN);
        let note = begin
            .as_ref()
            .and_then(|b| b.children().find(|c| c.kind() == OPT_ARG))
            .map(|o| inner(&o))
            .or_else(|| body.as_ref().and_then(leading_note));
        out.push(Item::Event(Event::EnvEnter {
            name: name.clone(),
            range,
            body: body_range,
            note,
        }));
        if let Some(b) = &body
            && !signatures::is_verbatim(&name)
        {
            let mut nested = Vec::new();
            self.children(b, base, &mut nested);
            out.extend(nested);
        }
        out.push(Item::Event(Event::EnvExit));
    }
}

fn rel(r: rowan::TextRange, base: usize) -> Range<usize> {
    usize::from(r.start()) - base..usize::from(r.end()) - base
}

/// The text inside a group or an optional argument, without its
/// delimiters.
pub(crate) fn inner(node: &SyntaxNode) -> String {
    let mut s = String::new();
    let kids: Vec<SyntaxElement> = node.children_with_tokens().collect();
    for (i, k) in kids.iter().enumerate() {
        let delim = matches!(k.kind(), L_BRACE | L_BRACKET) && i == 0
            || matches!(k.kind(), R_BRACE | R_BRACKET) && i + 1 == kids.len();
        if !delim {
            match k {
                NodeOrToken::Node(n) => s.push_str(&n.text().to_string()),
                NodeOrToken::Token(t) => s.push_str(t.text()),
            }
        }
    }
    s
}

/// The arguments of a command, in order.
pub(crate) fn args(cmd: &SyntaxNode) -> Vec<Arg> {
    cmd.children_with_tokens()
        .skip(1)
        .filter_map(|c| match c {
            NodeOrToken::Node(n) if n.kind() == OPT_ARG => Some(Arg::Opt(inner(&n))),
            NodeOrToken::Node(n) if n.kind() == GROUP => Some(Arg::Mand(inner(&n))),
            NodeOrToken::Node(n) => Some(Arg::Mand(n.text().to_string())),
            NodeOrToken::Token(t) if t.kind() == STAR => Some(Arg::Star),
            NodeOrToken::Token(t) if matches!(t.kind(), WHITESPACE | NEWLINE | COMMENT) => None,
            NodeOrToken::Token(t) => Some(Arg::Mand(t.text().to_string())),
        })
        .collect()
}

fn mands(args: &[Arg]) -> Vec<&str> {
    args.iter()
        .filter_map(|a| match a {
            Arg::Mand(s) => Some(s.as_str()),
            _ => None,
        })
        .collect()
}

fn opts(args: &[Arg]) -> Vec<&str> {
    args.iter()
        .filter_map(|a| match a {
            Arg::Opt(s) => Some(s.as_str()),
            _ => None,
        })
        .collect()
}

fn list(s: &str) -> Vec<String> {
    s.split(',')
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect()
}

/// The level of a sectioning command.
pub(crate) fn level(name: &str) -> Option<i8> {
    Some(match name {
        "part" => -1,
        "chapter" => 0,
        "section" => 1,
        "subsection" => 2,
        "subsubsection" => 3,
        "paragraph" => 4,
        "subparagraph" => 5,
        _ => return None,
    })
}

fn is_cite(name: &str) -> bool {
    signatures::command(name) == "*oom"
}

/// The events of a command; whether its arguments hold more (not for
/// definitions, whose bodies are templates).
fn command(cmd: &SyntaxNode, base: usize, out: &mut Vec<Item>) -> bool {
    let Some(name) = latex_syntax::name(cmd) else {
        return true;
    };
    let range = rel(cmd.text_range(), base);
    let a = args(cmd);
    let m = mands(&a);
    let o = opts(&a);
    let starred = matches!(a.first(), Some(Arg::Star));
    let mut push = |e: Event| out.push(Item::Event(e));
    match name.as_str() {
        "documentclass" => {
            if let Some(n) = m.first() {
                push(Event::Class {
                    name: n.trim().to_string(),
                    options: o.first().map(|s| list(s)).unwrap_or_default(),
                    range,
                });
            }
        }
        "usepackage" | "RequirePackage" => {
            let options = o.first().map(|s| list(s)).unwrap_or_default();
            for n in m.first().map(|s| list(s)).unwrap_or_default() {
                push(Event::Package {
                    name: n,
                    options: options.clone(),
                    range: range.clone(),
                });
            }
        }
        "appendix" => push(Event::Appendix),
        "frontmatter" => push(Event::FrontMatter),
        "mainmatter" => push(Event::MainMatter),
        "backmatter" => push(Event::BackMatter),
        "label" => {
            if let Some(n) = m.first() {
                push(Event::Label {
                    name: n.trim().to_string(),
                    range,
                });
            }
        }
        "ref" | "eqref" | "pageref" | "autoref" | "cref" | "Cref" | "nameref" | "vref" | "Vref"
        | "cpageref" => {
            if let Some(k) = m.first() {
                push(Event::Ref {
                    command: name.clone(),
                    keys: list(k),
                    range,
                });
            }
        }
        "caption" => push(Event::Caption {
            text: m.first().unwrap_or(&"").to_string(),
            short: o.first().map(|s| s.to_string()),
            range,
        }),
        "footnote" => push(Event::Footnote {
            explicit: o.first().map(|s| s.trim().to_string()),
            range,
        }),
        "newcommand" | "renewcommand" | "providecommand" => {
            if let (Some(n), Some(body)) = (m.first(), m.get(1)) {
                push(Event::Macro {
                    name: n.trim().to_string(),
                    command: name.clone(),
                    args: o.first().and_then(|s| s.trim().parse().ok()).unwrap_or(0),
                    default: o.get(1).map(|s| s.to_string()),
                    body: body.to_string(),
                    range,
                });
            }
            return false;
        }
        "DeclareMathOperator" => {
            if let (Some(n), Some(body)) = (m.first(), m.get(1)) {
                push(Event::Macro {
                    name: n.trim().to_string(),
                    command: name.clone(),
                    args: 0,
                    default: None,
                    body: body.to_string(),
                    range,
                });
            }
            return false;
        }
        "def" | "gdef" | "edef" | "xdef" => {
            // `\def`, the name, the parameters, the body.
            let mut toks = cmd
                .children_with_tokens()
                .skip(1)
                .filter(|c| !matches!(c.kind(), WHITESPACE | NEWLINE | COMMENT));
            let n = toks.next().map(|t| t.to_string()).unwrap_or_default();
            let mut params = 0;
            let mut body = String::new();
            for t in toks {
                match &t {
                    NodeOrToken::Node(g) if g.kind() == GROUP => body = inner(g),
                    NodeOrToken::Token(t) if t.kind() == HASH => params += 1,
                    _ => {}
                }
            }
            push(Event::Macro {
                name: n,
                command: name.clone(),
                args: params,
                default: None,
                body,
                range,
            });
            return false;
        }
        "newenvironment" | "renewenvironment" => {
            if let (Some(n), Some(b), Some(e)) = (m.first(), m.get(1), m.get(2)) {
                push(Event::NewEnvironment {
                    name: n.trim().to_string(),
                    args: o.first().and_then(|s| s.trim().parse().ok()).unwrap_or(0),
                    default: o.get(1).map(|s| s.to_string()),
                    begin: b.to_string(),
                    end: e.to_string(),
                    range,
                });
            }
            return false;
        }
        "newtheorem" => {
            // `{name}[shared]{Title}` or `{name}{Title}[within]`.
            let mut env = None;
            let mut title = None;
            let (mut shared, mut within) = (None, None);
            for x in &a {
                match x {
                    Arg::Mand(s) if env.is_none() => env = Some(s.trim().to_string()),
                    Arg::Mand(s) => title = Some(s.trim().to_string()),
                    Arg::Opt(s) if title.is_none() => shared = Some(s.trim().to_string()),
                    Arg::Opt(s) => within = Some(s.trim().to_string()),
                    Arg::Star => {}
                }
            }
            if let (Some(env), Some(title)) = (env, title) {
                push(Event::TheoremDef {
                    env,
                    title,
                    shared,
                    within,
                    numbered: !starred,
                });
            }
            return false;
        }
        "bibliography" | "addbibresource" => {
            if let Some(f) = m.first() {
                let files = list(f)
                    .into_iter()
                    .map(|f| {
                        if f.ends_with(".bib") {
                            f
                        } else {
                            format!("{f}.bib")
                        }
                    })
                    .collect();
                push(Event::Bibliography {
                    command: name.clone(),
                    files,
                    range,
                });
            }
        }
        "bibliographystyle" => {
            if let Some(s) = m.first() {
                push(Event::BibliographyStyle(s.trim().to_string()));
            }
        }
        "setcounter" | "addtocounter" => {
            if let (Some(c), Some(v)) = (m.first(), m.get(1))
                && let Ok(value) = v.trim().parse()
            {
                push(Event::SetCounter {
                    counter: c.trim().to_string(),
                    value,
                    add: name == "addtocounter",
                });
            }
        }
        "numberwithin" | "counterwithin" | "counterwithout" => {
            if let (Some(c), Some(w)) = (m.first(), m.get(1)) {
                push(Event::NumberWithin {
                    counter: c.trim().to_string(),
                    within: (name != "counterwithout").then(|| w.trim().to_string()),
                });
            }
        }
        "\\" => push(Event::LineBreak { at: range.start }),
        "nonumber" | "notag" => push(Event::NoNumber),
        "tag" => {
            if let Some(t) = m.first() {
                push(Event::Tag {
                    text: t.to_string(),
                });
            }
        }
        n if is_cite(n) => {
            if let Some(k) = m.first() {
                push(Event::Cite {
                    command: name.clone(),
                    keys: list(k),
                    notes: o.iter().map(|s| s.to_string()).collect(),
                    range,
                });
            }
        }
        n => {
            if let Some(level) = level(n) {
                push(Event::Section {
                    level,
                    command: name.clone(),
                    starred,
                    title: m.first().unwrap_or(&"").to_string(),
                    short: o.first().map(|s| s.to_string()),
                    range,
                });
            }
        }
    }
    true
}

/// `[note]` at the start of a body (theorems declared by `\newtheorem`
/// take it though the parser does not know them).
fn leading_note(body: &SyntaxNode) -> Option<String> {
    let mut toks = body
        .descendants_with_tokens()
        .filter_map(|t| t.into_token())
        .skip_while(|t| matches!(t.kind(), WHITESPACE | NEWLINE));
    if toks.next()?.kind() != L_BRACKET {
        return None;
    }
    let mut s = String::new();
    let mut depth = 0i32;
    for t in toks.take(300) {
        match t.kind() {
            R_BRACKET if depth == 0 => return Some(s),
            L_BRACE => depth += 1,
            R_BRACE => depth -= 1,
            PAR_BREAK => return None,
            _ => {}
        }
        s.push_str(t.text());
    }
    None
}
