//! Syntax highlighting shared by Org source blocks and plain text mode
//! (design §4, D16): Sublime Text syntax definitions through syntect with
//! pure Rust regular expressions. Spans carry a [`Kind`], not colors;
//! frontends color kinds with their theme.

use std::ops::Range;
use std::sync::LazyLock;

use syntect::parsing::{ParseState, Scope, ScopeStack, ScopeStackOp, SyntaxReference, SyntaxSet};

/// What a piece of code is, from its scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Comments.
    Comment,
    /// String and character literals.
    String,
    /// Numbers.
    Number,
    /// Language constants (`true`, `nil`) and escapes.
    Constant,
    /// Keywords and storage modifiers.
    Keyword,
    /// Operators.
    Operator,
    /// Function names.
    Function,
    /// Types and classes.
    Type,
    /// Variables and parameters.
    Variable,
    /// Markup tags and attribute names.
    Tag,
    /// Preprocessor and macro invocations.
    Macro,
    /// Invalid code.
    Invalid,
}

/// A highlighted range of one line, in bytes from the line's start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// The bytes.
    pub range: Range<usize>,
    /// What they are.
    pub kind: Kind,
}

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

/// Org's language names (`org-src-lang-modes`) that differ from the syntax
/// names or extensions.
const ALIASES: &[(&str, &str)] = &[
    ("sh", "Bourne Again Shell (bash)"),
    ("shell", "Bourne Again Shell (bash)"),
    ("bash", "Bourne Again Shell (bash)"),
    ("zsh", "Bourne Again Shell (bash)"),
    ("elisp", "Lisp"),
    ("emacs-lisp", "Lisp"),
    ("lisp", "Lisp"),
    ("scheme", "Lisp"),
    ("clojure", "Clojure"),
    ("js", "JavaScript"),
    ("javascript", "JavaScript"),
    ("ts", "JavaScript"),
    ("typescript", "JavaScript"),
    ("python", "Python"),
    ("py", "Python"),
    ("rust", "Rust"),
    ("c", "C"),
    ("cpp", "C++"),
    ("c++", "C++"),
    ("latex", "LaTeX"),
    ("tex", "LaTeX"),
    ("sql", "SQL"),
    ("sqlite", "SQL"),
    ("R", "R"),
    ("haskell", "Haskell"),
    ("ocaml", "OCaml"),
    ("java", "Java"),
    ("go", "Go"),
    ("ruby", "Ruby"),
    ("perl", "Perl"),
    ("php", "PHP"),
    ("html", "HTML"),
    ("css", "CSS"),
    ("xml", "XML"),
    ("yaml", "YAML"),
    ("json", "JSON"),
    ("markdown", "Markdown"),
    ("makefile", "Makefile"),
    ("dot", "Graphviz (DOT)"),
    ("diff", "Diff"),
    ("lua", "Lua"),
    ("matlab", "MATLAB"),
    ("scala", "Scala"),
    ("erlang", "Erlang"),
];

/// A language the highlighter knows.
#[derive(Debug, Clone, Copy)]
pub struct Language(&'static SyntaxReference);

impl Language {
    /// The language for an Org source block language (`sh`, `emacs-lisp`,
    /// `python`) or a file extension (`rs`), if known.
    pub fn find(name: &str) -> Option<Language> {
        let set = &*SYNTAXES;
        let lower = name.to_ascii_lowercase();
        let by_alias = ALIASES
            .iter()
            .find(|(a, _)| a.eq_ignore_ascii_case(&lower))
            .and_then(|(_, s)| set.find_syntax_by_name(s));
        by_alias
            .or_else(|| set.find_syntax_by_token(&lower))
            .or_else(|| set.find_syntax_by_extension(&lower))
            .filter(|s| s.name != "Plain Text")
            .map(Language)
    }

    /// The language's name.
    pub fn name(self) -> &'static str {
        &self.0.name
    }
}

/// The kind of the innermost scope that has one.
fn kind(stack: &ScopeStack) -> Option<Kind> {
    static PREFIXES: LazyLock<Vec<(Scope, Kind)>> = LazyLock::new(|| {
        [
            ("comment", Kind::Comment),
            ("string", Kind::String),
            ("constant.numeric", Kind::Number),
            ("constant.character.escape", Kind::Constant),
            ("constant", Kind::Constant),
            ("keyword.operator", Kind::Operator),
            ("keyword", Kind::Keyword),
            ("storage", Kind::Keyword),
            ("entity.name.function", Kind::Function),
            ("support.function", Kind::Function),
            ("variable.function", Kind::Function),
            ("entity.name.type", Kind::Type),
            ("entity.name.class", Kind::Type),
            ("support.type", Kind::Type),
            ("support.class", Kind::Type),
            ("entity.name.tag", Kind::Tag),
            ("entity.other.attribute-name", Kind::Tag),
            ("meta.preprocessor", Kind::Macro),
            ("support.macro", Kind::Macro),
            ("variable.parameter", Kind::Variable),
            ("variable", Kind::Variable),
            ("invalid", Kind::Invalid),
        ]
        .into_iter()
        .map(|(s, k)| (Scope::new(s).expect("a valid scope"), k))
        .collect()
    });
    stack.as_slice().iter().rev().find_map(|scope| {
        PREFIXES
            .iter()
            .find(|(p, _)| p.is_prefix_of(*scope))
            .map(|(_, k)| *k)
    })
}

/// Highlights one line (with its line feed) from `state` and `stack`,
/// which it moves on to the next line.
fn highlight_line(state: &mut ParseState, stack: &mut ScopeStack, line: &str) -> Vec<Span> {
    let set = &*SYNTAXES;
    let ops = state.parse_line(line, set).unwrap_or_default();
    let mut spans: Vec<Span> = Vec::new();
    let mut at = 0;
    let push = |spans: &mut Vec<Span>, stack: &ScopeStack, from: usize, to: usize| {
        if from >= to {
            return;
        }
        if let Some(k) = kind(stack) {
            match spans.last_mut() {
                Some(last) if last.kind == k && last.range.end == from => last.range.end = to,
                _ => spans.push(Span {
                    range: from..to,
                    kind: k,
                }),
            }
        }
    };
    for (pos, op) in ops {
        push(&mut spans, stack, at, pos);
        at = pos;
        if matches!(op, ScopeStackOp::Noop) {
            continue;
        }
        let _ = stack.apply(&op);
    }
    let end = line.trim_end_matches('\n').trim_end_matches('\r').len();
    push(&mut spans, stack, at, end);
    spans.retain(|s| s.range.start < end);
    for s in &mut spans {
        s.range.end = s.range.end.min(end);
    }
    spans
}

/// The parser's state at a line start.
type LineState = (ParseState, ScopeStack);

/// The previous highlighting while updating: states, lines, how many lines
/// were added, and the last changed line.
type Old = (Vec<LineState>, Vec<Vec<Span>>, isize, usize);

/// A highlighted text kept up to date through edits (plain text mode):
/// the parser's state at each line start is kept, so after an edit only
/// the changed lines are highlighted again, and the lines after them until
/// the state is what it was.
#[derive(Debug, Clone)]
pub struct Highlighter {
    language: Language,
    text: String,
    /// The state before each line.
    states: Vec<(ParseState, ScopeStack)>,
    lines: Vec<Vec<Span>>,
}

fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

impl Highlighter {
    /// Highlights `text` in `language`.
    pub fn new(language: Language, text: &str) -> Highlighter {
        let mut h = Highlighter {
            language,
            text: String::new(),
            states: Vec::new(),
            lines: Vec::new(),
        };
        h.run(text, 0, None, None);
        h
    }

    /// The language.
    pub fn language(&self) -> Language {
        self.language
    }

    /// The spans of line `i` (bytes from the line's start).
    pub fn line(&self, i: usize) -> &[Span] {
        self.lines.get(i).map_or(&[], Vec::as_slice)
    }

    /// Highlights from line `first` of `text` on; with `old` (the previous
    /// states and lines, shifted by `delta` lines after `last`, the last
    /// changed line), it stops where the state is as before.
    fn run(
        &mut self,
        text: &str,
        first: usize,
        start: Option<(ParseState, ScopeStack)>,
        old: Option<Old>,
    ) {
        let starts = line_starts(text);
        let mut state =
            start.unwrap_or_else(|| (ParseState::new(self.language.0), ScopeStack::new()));
        self.states.truncate(first);
        self.lines.truncate(first);
        for (i, &s) in starts.iter().enumerate().skip(first) {
            if let Some((old_states, old_lines, delta, last)) = &old {
                let j = i as isize - delta;
                if i > *last
                    && j >= 0
                    && (j as usize) < old_states.len()
                    && old_states[j as usize] == state
                {
                    // As before from here: the old lines, moved.
                    self.states.extend_from_slice(&old_states[j as usize..]);
                    self.lines.extend_from_slice(&old_lines[j as usize..]);
                    break;
                }
            }
            let e = starts.get(i + 1).copied().unwrap_or(text.len());
            self.states.push(state.clone());
            let spans = highlight_line(&mut state.0, &mut state.1, &text[s..e]);
            self.lines.push(spans);
        }
        self.states.truncate(starts.len());
        self.lines.truncate(starts.len());
        self.text = text.to_string();
    }

    /// Brings the highlighting up to date with `text` (the whole new
    /// text).
    pub fn update(&mut self, text: &str) {
        if text == self.text {
            return;
        }
        let (a, b) = (self.text.as_bytes(), text.as_bytes());
        let pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
        let max = a.len().min(b.len()) - pre;
        let suf = a
            .iter()
            .rev()
            .zip(b.iter().rev())
            .take(max)
            .take_while(|(x, y)| x == y)
            .count();
        let first = self.text[..pre].matches('\n').count();
        let last = text[..b.len() - suf].matches('\n').count();
        let delta = line_starts(text).len() as isize - line_starts(&self.text).len() as isize;
        let old = (
            std::mem::take(&mut self.states),
            std::mem::take(&mut self.lines),
            delta,
            last,
        );
        let first = first.min(old.0.len());
        let start = old.0.get(first).cloned();
        self.states = old.0[..first].to_vec();
        self.lines = old.1[..first.min(old.1.len())].to_vec();
        self.run(text, first, start, Some(old));
    }
}

/// Highlights `text` line by line; the spans of each line are in order
/// and do not overlap. Lines are split at `\n`.
pub fn highlight(language: Language, text: &str) -> Vec<Vec<Span>> {
    let set = &*SYNTAXES;
    let mut state = ParseState::new(language.0);
    let mut stack = ScopeStack::new();
    let mut out = Vec::new();
    for line in text.split_inclusive('\n') {
        let ops = state.parse_line(line, set).unwrap_or_default();
        let mut spans: Vec<Span> = Vec::new();
        let mut at = 0;
        let push = |spans: &mut Vec<Span>, stack: &ScopeStack, from: usize, to: usize| {
            if from >= to {
                return;
            }
            if let Some(k) = kind(stack) {
                match spans.last_mut() {
                    Some(last) if last.kind == k && last.range.end == from => last.range.end = to,
                    _ => spans.push(Span {
                        range: from..to,
                        kind: k,
                    }),
                }
            }
        };
        for (pos, op) in ops {
            push(&mut spans, &stack, at, pos);
            at = pos;
            if matches!(op, ScopeStackOp::Noop) {
                continue;
            }
            let _ = stack.apply(&op);
        }
        let end = line.trim_end_matches('\n').len();
        push(&mut spans, &stack, at, end);
        spans.retain(|s| s.range.start < end);
        for s in &mut spans {
            s.range.end = s.range.end.min(end);
        }
        out.push(spans);
    }
    if text.is_empty() || text.ends_with('\n') {
        out.push(Vec::new());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremental() {
        let rust = Language::find("rs").unwrap();
        let text = "fn a() {}\n/* x\ny */\nlet s = \"q\";\n";
        let mut h = Highlighter::new(rust, text);
        assert_eq!(h.lines, highlight(rust, text));
        // Edits: in a line, lines added, a comment opened that runs on.
        for new in [
            "fn a() { 1 }\n/* x\ny */\nlet s = \"q\";\n",
            "fn a() { 1 }\n\n\n/* x\ny */\nlet s = \"q\";\n",
            "fn a() { 1 }\n\n\n/* x\nfn y */\nlet s = \"q\";\n",
            "/* fn a() { 1 }\n\n\n/* x\ny */\nlet s = \"q\";\n",
            "let s = \"q\";\n",
            "",
        ] {
            h.update(new);
            assert_eq!(h.lines, highlight(rust, new), "{new:?}");
        }
    }

    #[test]
    fn languages() {
        for name in [
            "sh",
            "emacs-lisp",
            "python",
            "rust",
            "rs",
            "js",
            "C",
            "latex",
            "sql",
            "dot",
        ] {
            assert!(Language::find(name).is_some(), "{name}");
        }
        assert!(Language::find("no-such-language").is_none());
        assert_eq!(
            Language::find("sh").map(Language::name),
            Some("Bourne Again Shell (bash)")
        );
    }

    #[test]
    fn rust() {
        let code = "fn main() {\n    // hi\n    let x = \"s\"; 42\n}\n";
        let lines = highlight(Language::find("rust").unwrap(), code);
        assert_eq!(lines.len(), 5);
        let at = |l: usize, s: &str| {
            let line = code.split('\n').nth(l).unwrap();
            let i = line.find(s).unwrap();
            lines[l]
                .iter()
                .find(|sp| sp.range.contains(&i))
                .map(|sp| sp.kind)
        };
        assert_eq!(at(0, "fn"), Some(Kind::Keyword));
        assert_eq!(at(0, "main"), Some(Kind::Function));
        assert_eq!(at(1, "hi"), Some(Kind::Comment));
        assert_eq!(at(2, "\"s\""), Some(Kind::String));
        assert_eq!(at(2, "42"), Some(Kind::Number));
    }
}
