//! One pass before parsing: which `{` closes where and which `\begin`
//! ends where (the nearest balanced partner, as TeX pairs them), and where
//! the sectioning commands are. The parser uses them to decide whether a
//! group or an environment is closed without scanning ahead again, which
//! keeps parsing linear on unbalanced input.

use std::collections::HashMap;

use crate::lexer::{self, Tok};
use crate::signatures;

#[derive(Debug, Default)]
pub(crate) struct Tables {
    /// `{` to its `}`.
    pub(crate) braces: HashMap<usize, usize>,
    /// `\begin` to its `\end`.
    pub(crate) envs: HashMap<usize, usize>,
    /// Sectioning commands, in order.
    pub(crate) sections: Vec<usize>,
    /// `\makeatletter` and `\makeatother`, in order.
    pub(crate) toggles: Vec<(usize, bool)>,
    /// A macro that opens a displayed formula (`\be`, defined as
    /// `\begin{equation}`) to the one that closes it (`\ee`).
    pub(crate) aliases: HashMap<usize, usize>,
    /// Environments the text defines as a displayed formula
    /// (`\newenvironment{eqn}{\begin{equation}}{\end{equation}}`).
    pub(crate) math_envs: Vec<String>,
    /// Closing macros (`\\ee`) that end an environment opened by `\\begin`
    /// (`\\begin{equation} … \\ee`): their positions.
    pub(crate) alias_ends: std::collections::HashSet<usize>,
}

impl Tables {
    /// The first sectioning command at or after `pos`.
    pub(crate) fn section_after(&self, pos: usize) -> Option<usize> {
        let i = self.sections.partition_point(|&s| s < pos);
        self.sections.get(i).copied()
    }
}

/// What a text defines that changes how it parses: macros for an
/// equation's opening and closing (`\\be`, `\\ee`) and environments that
/// are displayed formulas.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Definitions {
    pub(crate) openers: Vec<String>,
    pub(crate) closers: Vec<String>,
    pub(crate) math_envs: Vec<String>,
}

impl Definitions {
    /// Those of `src`.
    pub(crate) fn of(src: &str) -> Definitions {
        let (openers, closers) = math_aliases(src);
        Definitions {
            openers,
            closers,
            math_envs: math_environments(src),
        }
    }
}

/// Whether `s` holds a definition that [`Definitions`] reads.
pub(crate) fn defines(s: &str) -> bool {
    [
        "\\def",
        "\\newcommand",
        "\\renewcommand",
        "\\providecommand",
        "\\newenvironment",
        "\\renewenvironment",
    ]
    .iter()
    .any(|d| s.contains(d))
}

/// The tables of `src[start..end]`, with `@` a letter at the start when
/// `at_letter`, and the definitions `defs` of the whole text.
pub(crate) fn build(
    src: &str,
    start: usize,
    end: usize,
    mut at_letter: bool,
    defs: &Definitions,
) -> Tables {
    let b = src.as_bytes();
    let mut t = Tables::default();
    let mut braces: Vec<usize> = Vec::new();
    let mut envs: Vec<(usize, &str)> = Vec::new();
    let (openers, closers) = (&defs.openers, &defs.closers);
    t.math_envs = defs.math_envs.clone();
    let mut alias_open: Option<usize> = None;
    let mut pos = start;
    while pos < end {
        let (tok, e) = lexer::next(src, pos, end, at_letter);
        match tok {
            Tok::LBrace => braces.push(pos),
            Tok::RBrace => {
                if let Some(open) = braces.pop() {
                    t.braces.insert(open, pos);
                }
            }
            Tok::ControlWord => {
                let name = &src[pos + 1..e];
                match name {
                    "verb" | "lstinline" => {
                        if let Some(v) = lexer::verb_end(b, e, end, name == "lstinline") {
                            pos = v;
                            continue;
                        }
                    }
                    "url" | "href" => {
                        if let Some((_, close)) = lexer::raw_braces(b, e, end) {
                            pos = close + 1;
                            continue;
                        }
                    }
                    "makeatletter" | "makeatother" => {
                        at_letter = name == "makeatletter";
                        t.toggles.push((pos, at_letter));
                    }
                    "begin" => {
                        if let Some((_, s, k)) = lexer::env_name(b, e, end) {
                            let env = &src[s..k];
                            if signatures::is_verbatim(env) {
                                let close = format!("\\end{{{env}}}");
                                if let Some(i) = src[k..end].find(&close) {
                                    t.envs.insert(pos, k + i);
                                    pos = k + i + close.len();
                                    continue;
                                }
                            }
                            envs.push((pos, env));
                            pos = k + 1;
                            continue;
                        }
                    }
                    "end" => {
                        if let Some((_, s, k)) = lexer::env_name(b, e, end) {
                            let env = &src[s..k];
                            if let Some(i) = envs.iter().rposition(|(_, n)| *n == env) {
                                // Environments opened inside it and not
                                // closed stay open.
                                t.envs.insert(envs[i].0, pos);
                                envs.truncate(i);
                            }
                            pos = k + 1;
                            continue;
                        }
                    }
                    n if signatures::is_sectioning(n) => t.sections.push(pos),
                    n if openers.iter().any(|o| o == n) && !in_definition(src, pos) => {
                        alias_open = Some(pos);
                    }
                    n if closers.iter().any(|o| o == n) && !in_definition(src, pos) => {
                        if let Some(o) = alias_open.take() {
                            t.aliases.insert(o, pos);
                        } else if let Some((open, env)) = envs.last()
                            && signatures::is_math(env)
                            && !matches!(
                                env.trim_end_matches('*'),
                                "split" | "aligned" | "gathered" | "alignedat" | "math"
                            )
                        {
                            // `\\begin{equation} … \\ee`.
                            t.envs.insert(*open, pos);
                            t.alias_ends.insert(pos);
                            envs.pop();
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        pos = e;
    }
    t
}

/// Names (without the backslash) papers give to the opening and closing
/// of an equation, taken as such when the text does not define them (a
/// file of a project whose root does).
const USUAL_ALIASES: &[(&str, &str)] = &[
    ("be", "ee"),
    ("beq", "eeq"),
    ("beqn", "eeqn"),
    ("bea", "eea"),
    ("beqa", "eeqa"),
    ("beqna", "eeqna"),
    ("bes", "ees"),
    ("ben", "een"),
    ("begeq", "endeq"),
];

/// The macros `src` defines as the opening and the closing of a displayed
/// formula's environment (`\def\be{\begin{equation}}`), with the usual
/// names it does not define.
pub(crate) fn math_aliases(src: &str) -> (Vec<String>, Vec<String>) {
    let (mut open, mut close) = (Vec::new(), Vec::new());
    let mut defined: Vec<String> = Vec::new();
    for cmd in [
        "\\def",
        "\\newcommand",
        "\\renewcommand",
        "\\providecommand",
    ] {
        for (i, _) in src.match_indices(cmd) {
            let rest = &src[i + cmd.len()..];
            let rest = rest.strip_prefix('*').unwrap_or(rest).trim_start();
            let (name, rest) = match rest.strip_prefix('{') {
                Some(r) => match r.split_once('}') {
                    Some((n, r)) => (n.trim(), r),
                    None => continue,
                },
                None => {
                    let n = rest
                        .char_indices()
                        .skip(1)
                        .find(|(_, c)| !c.is_ascii_alphabetic())
                        .map_or(rest.len(), |(k, _)| k);
                    (&rest[..n], &rest[n..])
                }
            };
            let Some(name) = name.strip_prefix('\\') else {
                continue;
            };
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphabetic()) {
                continue;
            }
            defined.push(name.to_string());
            let Some(body) = rest.trim_start().strip_prefix('{') else {
                continue;
            };
            let Some((body, _)) = body.split_once('}') else {
                continue;
            };
            // `{\begin{equation` up to its first `}`: the environment.
            let body = body.trim();
            let display = |env: &str| {
                let e = env.trim_end_matches('*');
                signatures::is_math(env)
                    && !matches!(e, "split" | "aligned" | "gathered" | "alignedat" | "math")
            };
            if let Some(env) = body.strip_prefix("\\begin{")
                && display(env)
            {
                open.push(name.to_string());
            } else if let Some(env) = body.strip_prefix("\\end{")
                && display(env)
            {
                close.push(name.to_string());
            }
        }
    }
    for (o, c) in USUAL_ALIASES {
        if !defined.iter().any(|d| d == o || d == c) {
            open.push(o.to_string());
            close.push(c.to_string());
        }
    }
    (open, close)
}

/// Whether the control word at `pos` is the name a definition defines
/// (`\def\be`, `\newcommand{\be}`), not a use.
fn in_definition(src: &str, pos: usize) -> bool {
    let before = src[..pos].trim_end();
    let before = before.strip_suffix('{').unwrap_or(before).trim_end();
    let before = before.strip_suffix('*').unwrap_or(before);
    [
        "\\def",
        "\\gdef",
        "\\edef",
        "\\xdef",
        "\\let",
        "\\newcommand",
        "\\renewcommand",
        "\\providecommand",
    ]
    .iter()
    .any(|d| before.ends_with(d))
}

/// The environments `src` defines whose begin code opens a displayed
/// formula (`\newenvironment{eqn}{\begin{equation}}{\end{equation}}`,
/// `{\[}{\]}`).
pub(crate) fn math_environments(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for cmd in ["\\newenvironment", "\\renewenvironment"] {
        for (i, _) in src.match_indices(cmd) {
            let rest = &src[i + cmd.len()..];
            let rest = rest.strip_prefix('*').unwrap_or(rest).trim_start();
            let Some((name, rest)) = rest.strip_prefix('{').and_then(|r| r.split_once('}')) else {
                continue;
            };
            let mut rest = rest.trim_start();
            // `[n]` and `[default]`.
            while rest.starts_with('[') {
                match rest.find(']') {
                    Some(k) => rest = rest[k + 1..].trim_start(),
                    None => break,
                }
            }
            let Some(code) = rest.strip_prefix('{') else {
                continue;
            };
            let code = code.trim_start();
            let display = if let Some(env) = code.strip_prefix("\\begin{") {
                let env = env.split('}').next().unwrap_or("");
                signatures::is_math(env)
                    && !matches!(
                        env.trim_end_matches('*'),
                        "split" | "aligned" | "gathered" | "alignedat" | "math"
                    )
            } else {
                code.starts_with("\\[") || code.starts_with("$$")
            };
            if display && !signatures::is_math(name) {
                out.push(name.trim().to_string());
            }
        }
    }
    out
}
