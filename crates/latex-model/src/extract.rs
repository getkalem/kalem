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
        /// `\captionof{figure}`: the float type outside a float.
        of: Option<String>,
        /// `\subfloat[caption]` (subfig): a sub-caption.
        sub: bool,
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
    /// `\addcontentsline{toc}{level}{title}`.
    ContentsLine {
        level: i8,
        title: String,
        range: Range<usize>,
    },
    /// `\newcounter{counter}[within]`: reset by `within`, printed alone.
    ResetWithin {
        counter: String,
        within: String,
    },
    /// `\stepcounter`, or `\refstepcounter` (`refer`), which `\label`
    /// then points at.
    Step {
        counter: String,
        refer: bool,
    },
    /// `\item`, with a label of its own (`explicit`) or numbered by its
    /// list.
    Item {
        explicit: bool,
    },
    Include {
        command: String,
        args: Vec<String>,
        range: Range<usize>,
    },
    IncludeOnly(Vec<String>),
    GraphicsPath(Vec<String>),
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
                // `\[…\]` is amsmath's `equation*`: a `\tag` there numbers
                // it (not in `$$…$$`, where amsmath's `\tag` fails).
                DISPLAY_MATH if child.text().to_string().starts_with("\\[") => {
                    let range = rel(child.text_range(), base);
                    let body = (range.start + 2).min(range.end)
                        ..range.end.saturating_sub(2).max(range.start);
                    out.push(Item::Event(Event::EnvEnter {
                        name: "displaymath".into(),
                        range,
                        body,
                        note: None,
                    }));
                    self.walk(&child, base, out);
                    out.push(Item::Event(Event::EnvExit));
                }
                VERB => {}
                _ => self.walk(&child, base, out),
            }
        }
    }

    fn environment(&mut self, env: &SyntaxNode, base: usize, out: &mut Vec<Item>) {
        let mut name = latex_syntax::name(env).unwrap_or_default();
        // `\begin{empheq}{align}` numbers as the environment it names.
        if name == "empheq"
            && let Some(inner) = env
                .children()
                .find(|c| c.kind() == BEGIN)
                .and_then(|b| b.children().filter(|c| c.kind() == GROUP).nth(1))
        {
            name = crate::extract::inner(&inner).trim().to_string();
        }
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

/// The keys of a citation or reference, without a macro's parameters
/// (`\cite{#1}` in a definition cites nothing).
fn keys(s: &str) -> Vec<String> {
    let mut k = list(s);
    k.retain(|k| !k.starts_with('#'));
    k
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
            if let Some(n) = m.first().filter(|n| !n.trim().starts_with('#')) {
                push(Event::Label {
                    name: n.trim().to_string(),
                    range,
                });
            }
        }
        "ref" | "eqref" | "pageref" | "autoref" | "cref" | "Cref" | "nameref" | "vref" | "Vref"
        | "cpageref" => {
            if let Some(k) = m.first().map(|k| keys(k)).filter(|k| !k.is_empty()) {
                push(Event::Ref {
                    command: name.clone(),
                    keys: k,
                    range,
                });
            }
        }
        "caption" => push(Event::Caption {
            text: m.first().unwrap_or(&"").to_string(),
            short: o.first().map(|s| s.to_string()),
            range,
            of: None,
            sub: false,
        }),
        "captionof" => {
            if let Some(kind) = m.first() {
                push(Event::Caption {
                    text: m.get(1).unwrap_or(&"").to_string(),
                    short: o.first().map(|s| s.to_string()),
                    range,
                    of: Some(kind.trim().to_string()),
                    sub: false,
                });
            }
        }
        // subfig's `\subfloat[list][caption]{body}`, before its body.
        "subfloat" => push(Event::Caption {
            text: o.last().unwrap_or(&"").to_string(),
            short: (o.len() > 1).then(|| o[0].to_string()),
            range,
            of: None,
            sub: true,
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
        // thmtools: `\declaretheorem[key=value,…]{name}` (or the options
        // after the name).
        "declaretheorem" => {
            let env = m.first().map(|s| s.trim().to_string());
            let mut title = None;
            let (mut shared, mut within, mut numbered) = (None, None, true);
            for opts in &o {
                for kv in opts.split(',') {
                    let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                    let v = v.trim().trim_matches(['{', '}']).trim().to_string();
                    match k.trim() {
                        "name" | "title" => title = Some(v),
                        "numberwithin" | "within" | "parent" => within = Some(v),
                        "sibling" | "sharenumber" | "numberlike" | "sharecounter" => {
                            shared = Some(v)
                        }
                        "numbered" => numbered = v != "no",
                        _ => {}
                    }
                }
            }
            if let Some(env) = env.filter(|e| !e.is_empty()) {
                let title = title.unwrap_or_else(|| {
                    let mut c = env.chars();
                    c.next()
                        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                        .unwrap_or_default()
                });
                push(Event::TheoremDef {
                    env,
                    title,
                    shared,
                    within,
                    numbered,
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
                // `\bibliography{refs}` names `refs.bib`; `\addbibresource`
                // names the file with its extension (`refs.bib`,
                // `refs.json`), which biber reads as it is.
                let files = list(f)
                    .into_iter()
                    .map(|f| {
                        if f.ends_with(".bib") || (name == "addbibresource" && f.contains('.')) {
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
        "addcontentsline" => {
            if let [list, lvl, title] = m.as_slice()
                && list.trim() == "toc"
                && let Some(level) = level(lvl.trim())
            {
                push(Event::ContentsLine {
                    level,
                    title: title.trim().to_string(),
                    range,
                });
            }
        }
        // `\newcounter{name}[within]`: reset by `within`.
        "newcounter" => {
            if let Some(c) = m.first() {
                push(Event::SetCounter {
                    counter: c.trim().to_string(),
                    value: 0,
                    add: false,
                });
                if let Some(w) = o.first() {
                    push(Event::ResetWithin {
                        counter: c.trim().to_string(),
                        within: w.trim().to_string(),
                    });
                }
            }
        }
        // `\footnotemark` steps the footnote counter; with a number, not.
        "footnotemark" if o.is_empty() => push(Event::SetCounter {
            counter: "footnote".into(),
            value: 1,
            add: true,
        }),
        "stepcounter" | "refstepcounter" => {
            if let Some(c) = m.first() {
                push(Event::Step {
                    counter: c.trim().to_string(),
                    refer: name == "refstepcounter",
                });
            }
        }
        "item" => push(Event::Item {
            explicit: !o.is_empty(),
        }),
        "numberwithin" | "counterwithin" | "counterwithout" => {
            if let (Some(c), Some(w)) = (m.first(), m.get(1)) {
                push(Event::NumberWithin {
                    counter: c.trim().to_string(),
                    within: (name != "counterwithout").then(|| w.trim().to_string()),
                });
            }
        }
        // `\input file` without braces: TeX reads the name up to a blank.
        "input" if !cmd.children().any(|c| c.kind() == GROUP) => {
            let mut file = String::new();
            let mut end = range.end;
            let mut next = cmd.first_token().and_then(|t| t.next_token());
            while let Some(t) = next {
                if matches!(t.kind(), WHITESPACE | NEWLINE) && !file.is_empty() {
                    break;
                }
                if !matches!(t.kind(), TEXT | WHITESPACE) {
                    break;
                }
                if t.kind() == TEXT {
                    file.push_str(t.text());
                    end = rel(t.text_range(), base).end;
                }
                next = t.next_token();
            }
            let file = file.split_whitespace().next().unwrap_or("").to_string();
            if !file.is_empty() {
                push(Event::Include {
                    command: name.clone(),
                    args: vec![file],
                    range: range.start..end,
                });
            }
        }
        "input" | "include" | "subfile" | "import" | "subimport" => {
            if !m.is_empty() {
                push(Event::Include {
                    command: name.clone(),
                    args: m.iter().map(|s| s.trim().to_string()).collect(),
                    range,
                });
            }
        }
        "includeonly" => push(Event::IncludeOnly(
            m.first().map(|s| list(s)).unwrap_or_default(),
        )),
        "graphicspath" => {
            // `{{a/}{b/}}`: the folders in braces.
            let dirs = m
                .first()
                .map(|s| {
                    s.split(['{', '}'])
                        .map(str::trim)
                        .filter(|d| !d.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            push(Event::GraphicsPath(dirs));
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
            if let Some(k) = m.first().map(|k| keys(k)).filter(|k| !k.is_empty()) {
                push(Event::Cite {
                    command: name.clone(),
                    keys: k,
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
    // A definition's body runs where the macro is used, not here.
    !matches!(
        name.as_str(),
        "newcommand"
            | "renewcommand"
            | "providecommand"
            | "newenvironment"
            | "renewenvironment"
            | "DeclareRobustCommand"
            | "NewDocumentCommand"
            | "RenewDocumentCommand"
            | "ProvideDocumentCommand"
            | "DeclareDocumentCommand"
            | "NewDocumentEnvironment"
            | "RenewDocumentEnvironment"
    )
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
