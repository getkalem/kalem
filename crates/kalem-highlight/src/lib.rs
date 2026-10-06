//! Syntax highlighting shared by Org source blocks and plain text mode
//! (design §4, D16): Sublime Text syntax definitions through syntect with
//! pure Rust regular expressions. Spans carry a [`Kind`], not colors;
//! frontends color kinds with their theme.

use std::ops::Range;
use std::sync::{LazyLock, RwLock};

mod plugins;

pub use plugins::{Files, Registered, SyntaxSource, flatten, register, register_cached};

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

/// syntect's syntaxes with bat's added (`two-face`): TOML, INI,
/// TypeScript, and others syntect lacks; then the plugins' ([`register`]).
/// A set is never changed once built: a registration builds a new one and
/// leaks it, so a [`Language`] found before keeps working with its own
/// set (registrations happen a few times per run, when plugins load).
static SYNTAXES: LazyLock<RwLock<&'static SyntaxSet>> = LazyLock::new(|| RwLock::new(defaults()));

/// The built-in syntaxes.
fn defaults() -> &'static SyntaxSet {
    static DEFAULTS: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);
    &DEFAULTS
}

/// How many times the set changed: frontends that keep a highlighter per
/// document make it again when this moves (a plugin installed).
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The set's generation ([`GENERATION`]).
pub fn generation() -> u64 {
    GENERATION.load(std::sync::atomic::Ordering::Relaxed)
}

fn replace_set(set: &'static SyntaxSet) {
    *SYNTAXES.write().expect("syntaxes") = set;
    GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Names and extensions that language plugins map to a syntax by its name
/// (`eex` to `HTML (EEx)`, whose own extensions are `html.eex`), looked up
/// before the built-in table.
static PLUGIN_ALIASES: RwLock<Vec<(String, String)>> = RwLock::new(Vec::new());

/// Sets the plugins' names and extensions for syntaxes: pairs of a name
/// or extension and a syntax's name.
pub fn set_aliases(aliases: Vec<(String, String)>) {
    *PLUGIN_ALIASES.write().expect("aliases") = aliases;
}

/// Back to the built-in syntaxes (the last plugin removed).
pub fn reset() {
    if !std::ptr::eq(current(), defaults()) {
        replace_set(defaults());
    }
}

fn current() -> &'static SyntaxSet {
    *SYNTAXES.read().expect("syntaxes")
}

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
    ("ts", "TypeScript"),
    ("typescript", "TypeScript"),
    ("tsx", "TypeScriptReact"),
    // `c-or-c++-mode` in Emacs; C here.
    ("h", "C"),
    ("toml", "TOML"),
    ("ini", "INI"),
    ("conf", "INI"),
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

/// A language the highlighter knows. Two are equal when they are the
/// same syntax of the same set: a plugin's Elixir is not the built-in one.
#[derive(Debug, Clone, Copy)]
pub struct Language {
    set: &'static SyntaxSet,
    syntax: &'static SyntaxReference,
}

impl PartialEq for Language {
    fn eq(&self, other: &Language) -> bool {
        std::ptr::eq(self.syntax, other.syntax)
    }
}

impl Eq for Language {}

impl Language {
    /// The language for an Org source block language (`sh`, `emacs-lisp`,
    /// `python`) or a file extension (`rs`), if known.
    pub fn find(name: &str) -> Option<Language> {
        let set = current();
        let lower = name.to_ascii_lowercase();
        let plugin = PLUGIN_ALIASES
            .read()
            .ok()
            .and_then(|a| a.iter().find(|(n, _)| *n == lower).map(|(_, s)| s.clone()))
            .and_then(|s| set.find_syntax_by_name(&s));
        if let Some(syntax) = plugin {
            return Some(Language { set, syntax });
        }
        let by_alias = ALIASES
            .iter()
            .find(|(a, _)| a.eq_ignore_ascii_case(&lower))
            .and_then(|(_, s)| set.find_syntax_by_name(s));
        by_alias
            .or_else(|| set.find_syntax_by_token(&lower))
            .or_else(|| set.find_syntax_by_extension(&lower))
            .filter(|s| s.name != "Plain Text")
            .map(|syntax| Language { set, syntax })
    }

    /// The language's name.
    pub fn name(self) -> &'static str {
        &self.syntax.name
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

/// The longest line colored, in bytes, as VS Code's
/// `editor.maxTokenizationLineLength`: a longer one (a minified script, a
/// line of data) shows plain, its state passed on as it came, since
/// parsing it took seconds at each keystroke (3.6 s for a 1 MB line).
pub const MAX_LINE: usize = 20_000;

/// Highlights one line (with its line feed) from `state` and `stack`,
/// which it moves on to the next line; a line longer than [`MAX_LINE`]
/// gets no spans and leaves them as they are.
fn highlight_line(
    set: &SyntaxSet,
    state: &mut ParseState,
    stack: &mut ScopeStack,
    line: &str,
) -> Vec<Span> {
    if line.len() > MAX_LINE {
        return Vec::new();
    }
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
            start.unwrap_or_else(|| (ParseState::new(self.language.syntax), ScopeStack::new()));
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
            let spans = highlight_line(self.language.set, &mut state.0, &mut state.1, &text[s..e]);
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

/// Highlighting for texts too large to highlight whole (§15, T2.7a.3):
/// the lines asked for are highlighted in a window, from a fresh state a
/// little before it, and kept until the text changes or another window is
/// asked for. A construct that opens more than [`Windowed::LOOKBACK`] lines
/// before the window (a long block comment) may be colored as code.
#[derive(Debug, Clone, Default)]
pub struct Windowed {
    /// The language, when the text has one.
    pub language: Option<Language>,
    version: u64,
    first: usize,
    lines: Vec<Vec<Span>>,
}

impl Windowed {
    /// How many lines before a window are highlighted first.
    pub const LOOKBACK: usize = 200;
    /// How many lines a window has.
    pub const SIZE: usize = 400;

    /// Highlighting in `language`, for texts of version `version`.
    pub fn new(language: Option<Language>) -> Windowed {
        Windowed {
            language,
            ..Windowed::default()
        }
    }

    /// The spans of line `i` of `text` (version `version`; `range(n)` is
    /// line `n`'s byte range, without its line feed, and `count` the
    /// number of lines).
    pub fn line(
        &mut self,
        version: u64,
        i: usize,
        text: &str,
        range: impl Fn(usize) -> Range<usize>,
        count: usize,
    ) -> &[Span] {
        let Some(language) = self.language else {
            return &[];
        };
        let cached =
            self.version == version && i >= self.first && i < self.first + self.lines.len();
        if !cached {
            let first = i.saturating_sub(Self::SIZE / 4);
            let from = first.saturating_sub(Self::LOOKBACK);
            let last = (first + Self::SIZE).min(count.max(1)) - 1;
            let (a, b) = (range(from).start, range(last).end);
            let mut lines = highlight(language, &text[a..b]);
            lines.drain(..(first - from).min(lines.len()));
            self.version = version;
            self.first = first;
            self.lines = lines;
        }
        self.lines.get(i - self.first).map_or(&[], Vec::as_slice)
    }
}

/// [`highlight`] of a block, kept for the blocks highlighted last: a
/// block's lines are drawn one at a time, each with the state of the lines
/// before it, so the whole block is highlighted once and its lines taken
/// from that.
pub fn highlight_block(language: Language, text: &str) -> std::sync::Arc<Vec<Vec<Span>>> {
    use std::hash::{Hash, Hasher};
    type Cache = Vec<(u64, std::sync::Arc<Vec<Vec<Span>>>)>;
    thread_local! {
        static CACHE: std::cell::RefCell<Cache> = const { std::cell::RefCell::new(Vec::new()) };
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    language.syntax.name.hash(&mut h);
    std::ptr::from_ref(language.set).hash(&mut h);
    text.hash(&mut h);
    let key = h.finish();
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((_, v)) = c.iter().find(|(k, _)| *k == key) {
            return v.clone();
        }
        let v = std::sync::Arc::new(highlight(language, text));
        c.insert(0, (key, v.clone()));
        c.truncate(32);
        v
    })
}

/// Highlights `text` line by line; the spans of each line are in order
/// and do not overlap. Lines are split at `\n`.
pub fn highlight(language: Language, text: &str) -> Vec<Vec<Span>> {
    let set = language.set;
    let mut state = ParseState::new(language.syntax);
    let mut stack = ScopeStack::new();
    let mut out = Vec::new();
    for line in text.split_inclusive('\n') {
        // Too long to color: plain, the state as it came.
        if line.len() > MAX_LINE {
            out.push(Vec::new());
            continue;
        }
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
mod window_tests {
    use super::*;

    #[test]
    fn a_line_too_long_shows_plain() {
        // A minified script: the long line plain, the lines after it
        // colored as before, at once (3.6 s a keystroke on 1 MB).
        let lang = Language::find("js").expect("js");
        let long = "var a=1;".repeat(MAX_LINE / 8 + 10);
        let text = format!("var x = \"s\";\n{long}\nvar y = 2;\n");
        let started = std::time::Instant::now();
        let h = Highlighter::new(lang, &text);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert!(!h.line(0).is_empty());
        assert!(h.line(1).is_empty());
        assert!(!h.line(2).is_empty());
        let whole = highlight(lang, &text);
        assert!(whole[1].is_empty() && !whole[2].is_empty());
    }

    #[test]
    fn languages_beyond_syntect() {
        // bat's syntaxes: TOML, INI and TypeScript are their own, `.h` C.
        let name = |n: &str| Language::find(n).map(Language::name);
        assert_eq!(name("toml"), Some("TOML"));
        assert_eq!(name("ini"), Some("INI"));
        assert_eq!(name("ts"), Some("TypeScript"));
        assert_eq!(name("h"), Some("C"));
        assert_eq!(name("m"), Some("Objective-C"));
        // Their regular expressions work with fancy-regex.
        let toml = highlight(Language::find("toml").unwrap(), "# c\n[a]\nk = \"v\"\n");
        assert!(toml[0].iter().any(|s| s.kind == Kind::Comment), "{toml:?}");
        assert!(toml[2].iter().any(|s| s.kind == Kind::String), "{toml:?}");
        let ts = highlight(
            Language::find("ts").unwrap(),
            "interface P { x: number }\nconst p: P = { x: 1 };\n",
        );
        assert!(ts[0].iter().any(|s| s.kind == Kind::Keyword), "{ts:?}");
        let ini = highlight(Language::find("ini").unwrap(), "; c\n[s]\nk=v\n");
        assert!(ini[0].iter().any(|s| s.kind == Kind::Comment), "{ini:?}");
    }

    #[test]
    fn windows_of_a_large_text() {
        let rust = Language::find("rs").unwrap();
        let text: String = (0..2000)
            .map(|i| format!("let x{i} = \"s\"; // c\n"))
            .collect();
        let starts: Vec<usize> = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        let range = |n: usize| starts[n]..starts.get(n + 1).map_or(text.len(), |e| e - 1);
        let whole = highlight(rust, &text);
        let mut w = Windowed::new(Some(rust));
        for i in [0, 1, 999, 1500, 1999] {
            assert_eq!(
                w.line(1, i, &text, range, 2000),
                whole[i].as_slice(),
                "line {i}"
            );
        }
        assert!(
            Windowed::new(None)
                .line(1, 3, &text, range, 2000)
                .is_empty()
        );
    }
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
