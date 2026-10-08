//! Syntax highlighting shared by Org source blocks and plain text mode
//! (design §4, D16): Sublime Text syntax definitions through syntect with
//! pure Rust regular expressions. Spans carry a [`Kind`], not colors;
//! frontends color kinds with their theme.

use std::ops::Range;
use std::sync::{Condvar, LazyLock, Mutex, PoisonError, RwLock};

mod plugins;

pub use plugins::{
    Files, Registered, SyntaxSource, flatten, register, register_cached, register_cached_later,
};

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
    /// A line a diff adds.
    Inserted,
    /// A line a diff removes.
    Deleted,
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
    let mut building = BUILDING.lock().unwrap_or_else(PoisonError::into_inner);
    *building = 0;
    install(set);
    BUILT.notify_all();
}

fn install(set: &'static SyntaxSet) {
    *SYNTAXES.write().expect("syntaxes") = set;
    GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// The set being built on a thread of its own ([`register_cached_later`]):
/// its number while it is, 0 otherwise. A registration meanwhile makes
/// that set stale: it is dropped when built.
static BUILDING: Mutex<u64> = Mutex::new(0);
/// Told when [`BUILDING`] goes back to 0.
static BUILT: Condvar = Condvar::new();
static BUILDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A set to be built on a thread of its own: its number.
fn start_building() -> u64 {
    let n = BUILDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    *BUILDING.lock().unwrap_or_else(PoisonError::into_inner) = n;
    n
}

/// Puts the set built as number `n` in place and calls `done`, unless a
/// registration came after it. Lookups waiting for it wait for `done` too.
fn finish_building(n: u64, set: SyntaxSet, done: impl FnOnce()) {
    let mut building = BUILDING.lock().unwrap_or_else(PoisonError::into_inner);
    if *building != n {
        return;
    }
    install(Box::leak(Box::new(set)));
    done();
    *building = 0;
    BUILT.notify_all();
}

/// Waits for a set being built on a thread of its own
/// ([`register_cached_later`]); at once when none is.
pub fn wait() {
    let mut building = BUILDING.lock().unwrap_or_else(PoisonError::into_inner);
    while *building != 0 {
        building = BUILT.wait(building).unwrap_or_else(PoisonError::into_inner);
    }
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
    if std::ptr::eq(current(), defaults()) {
        let mut building = BUILDING.lock().unwrap_or_else(PoisonError::into_inner);
        *building = 0;
        BUILT.notify_all();
    } else {
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
    /// A plugin's language waits for a set being built with it
    /// ([`register_cached_later`]); the others are found in the set in
    /// place.
    pub fn find(name: &str) -> Option<Language> {
        let lower = name.to_ascii_lowercase();
        let plugin = PLUGIN_ALIASES
            .read()
            .ok()
            .and_then(|a| a.iter().find(|(n, _)| *n == lower).map(|(_, s)| s.clone()));
        if plugin.is_some() {
            wait();
        }
        let set = current();
        let plugin = plugin.and_then(|s| set.find_syntax_by_name(&s));
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
            // A diff: its added and removed lines, the hunk's range line
            // as a function (its heading), the file headers dimmed.
            ("markup.inserted", Kind::Inserted),
            ("markup.deleted", Kind::Deleted),
            ("meta.diff.range", Kind::Function),
            ("meta.diff.header", Kind::Comment),
            ("meta.diff.index", Kind::Comment),
            ("meta.separator.diff", Kind::Comment),
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

/// A highlighted text kept up to date through edits (plain text mode),
/// colored from the top only as far as its lines are asked for: opening a
/// long file colors the lines shown, not the whole file (a 10 KB Markdown
/// file took 220 ms). The parser's state at each line start is kept, so
/// after an edit only the changed lines are highlighted again, and the
/// lines after them until the state is what it was.
#[derive(Debug, Clone)]
pub struct Highlighter {
    language: Language,
    text: String,
    /// Where each line starts.
    starts: Vec<usize>,
    /// The state before each line colored and before the line after them:
    /// one more than `lines`.
    states: Vec<LineState>,
    /// The spans of the lines colored, from the first.
    lines: Vec<Vec<Span>>,
}

fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

/// How many bytes `a` and `b` start with in common.
fn common_prefix(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut i = 0;
    // Whole chunks first: compared as slices, they are compared fast.
    while i + 64 <= n && a[i..i + 64] == b[i..i + 64] {
        i += 64;
    }
    i + a[i..n]
        .iter()
        .zip(&b[i..n])
        .take_while(|(x, y)| x == y)
        .count()
}

/// How many bytes `a` and `b` end with in common, at most `max`.
fn common_suffix(a: &[u8], b: &[u8], max: usize) -> usize {
    let (mut x, mut y) = (a.len(), b.len());
    let mut n = 0;
    while n + 64 <= max && a[x - 64..x] == b[y - 64..y] {
        (x, y, n) = (x - 64, y - 64, n + 64);
    }
    n + a[..x]
        .iter()
        .rev()
        .zip(b[..y].iter().rev())
        .take(max - n)
        .take_while(|(x, y)| x == y)
        .count()
}

fn line_feeds(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&b| b == b'\n').count()
}

impl Highlighter {
    /// How many lines an edit colors again at most, looking for the state
    /// as it was before a line it left as it was; the lines after are
    /// colored again when they are shown (a comment opened at the top of a
    /// long file).
    const REDO: usize = 1000;

    /// Highlights `text` in `language` (lines are colored when asked for).
    pub fn new(language: Language, text: &str) -> Highlighter {
        Highlighter {
            language,
            text: text.to_string(),
            starts: line_starts(text),
            states: vec![(ParseState::new(language.syntax), ScopeStack::new())],
            lines: Vec::new(),
        }
    }

    /// The language.
    pub fn language(&self) -> Language {
        self.language
    }

    /// The spans of line `i` (bytes from the line's start), coloring the
    /// lines before it first if they are not yet.
    pub fn line(&mut self, i: usize) -> &[Span] {
        self.color_to(i.saturating_add(1));
        self.lines.get(i).map_or(&[], Vec::as_slice)
    }

    /// Colors the lines before line `end` not colored yet.
    fn color_to(&mut self, end: usize) {
        let end = end.min(self.starts.len());
        while self.lines.len() < end {
            self.color_next();
        }
    }

    /// Colors the line after the last colored.
    fn color_next(&mut self) {
        let i = self.lines.len();
        let s = self.starts[i];
        let e = self.starts.get(i + 1).copied().unwrap_or(self.text.len());
        let mut state = self.states[i].clone();
        let spans = highlight_line(
            self.language.set,
            &mut state.0,
            &mut state.1,
            &self.text[s..e],
        );
        self.lines.push(spans);
        self.states.push(state);
    }

    /// Brings the highlighting up to date with `text` (the whole new
    /// text).
    pub fn update(&mut self, text: &str) {
        if text == self.text {
            return;
        }
        let (a, b) = (self.text.as_bytes(), text.as_bytes());
        let pre = common_prefix(a, b);
        let suf = common_suffix(a, b, a.len().min(b.len()) - pre);
        let added = line_feeds(&b[pre..b.len() - suf]);
        let delta = added as isize - line_feeds(&a[pre..a.len() - suf]) as isize;
        // The first line changed, and the last in the new text.
        let first = line_feeds(&a[..pre]);
        let last = first + added;
        let colored = self.lines.len();
        self.text.clear();
        self.text.push_str(text);
        self.starts = line_starts(text);
        if first >= colored {
            // Only lines not colored yet changed.
            return;
        }
        // From `first` on, colored again until the state before a line the
        // edit left is what it was: the old lines after it are moved.
        let old_states = self.states.split_off(first + 1);
        let old_lines = self.lines.split_off(first);
        // How far lines were colored, in the new text's lines.
        let end = (colored as isize + delta).max(0) as usize;
        while self.lines.len() < self.starts.len() {
            let i = self.lines.len();
            if i > last {
                // Line `i` was line `j`; the states kept are those before
                // lines `first + 1` to `colored`.
                let j = (i as isize - delta) as usize;
                let k = j.wrapping_sub(first + 1);
                if old_states.get(k) == Some(&self.states[i]) {
                    self.lines.extend(old_lines.into_iter().skip(k + 1));
                    self.states.extend(old_states.into_iter().skip(k + 1));
                    return;
                }
                if i >= end {
                    break;
                }
            }
            if i - first >= Self::REDO {
                break;
            }
            self.color_next();
        }
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
    let mut state = ParseState::new(language.syntax);
    let mut stack = ScopeStack::new();
    let mut out: Vec<Vec<Span>> = text
        .split_inclusive('\n')
        .map(|line| highlight_line(language.set, &mut state, &mut stack, line))
        .collect();
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
        // The syntaxes loaded first: their loading is not what is timed
        // (over a second on CI's debug build).
        let _ = Highlighter::new(lang, "var z = 1;\n").line(0);
        let started = std::time::Instant::now();
        let mut h = Highlighter::new(lang, &text);
        h.color_to(usize::MAX);
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
        // Nothing is colored before it is asked for.
        assert!(h.lines.is_empty());
        h.color_to(usize::MAX);
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
            h.color_to(usize::MAX);
            assert_eq!(h.lines, highlight(rust, new), "{new:?}");
        }
    }

    #[test]
    fn colored_as_far_as_asked() {
        let rust = Language::find("rs").unwrap();
        let line = "let s = \"q\"; // é\n";
        let text = line.repeat(3000);
        let whole = highlight(rust, &text);
        let mut h = Highlighter::new(rust, &text);
        assert_eq!(h.line(10), whole[10].as_slice());
        assert_eq!(h.lines.len(), 11);
        // An edit below the lines colored colors nothing.
        let below = format!("{}x{}", &text[..text.len() - 5], &text[text.len() - 5..]);
        h.update(&below);
        assert_eq!(h.lines.len(), 11);
        // An edit above them colors the line again, and the old lines
        // after it are kept.
        let above = format!("{}\n{}", &below[..40], &below[40..]);
        h.update(&above);
        assert_eq!(h.lines.len(), 12);
        h.color_to(usize::MAX);
        assert_eq!(h.lines, highlight(rust, &above));
        // A comment opened at the top colors at most `REDO` lines again;
        // the others when they are shown, as they are now.
        let opened = format!("/*{above}");
        h.update(&opened);
        assert!(h.lines.len() <= Highlighter::REDO + 1);
        let now = highlight(rust, &opened);
        assert_eq!(h.line(2500), now[2500].as_slice());
        h.color_to(usize::MAX);
        assert_eq!(h.lines, now);
        // Several bytes of a letter changed: line counts in bytes.
        let changed = opened.replacen('é', "ü", 1);
        h.update(&changed);
        h.color_to(usize::MAX);
        assert_eq!(h.lines, highlight(rust, &changed));
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
    fn diff() {
        let code = "diff --git a/a.txt b/a.txt\nindex 1234567..89abcde 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@ fn main\n context\n-line 2\n+line TWO\n";
        let lines = highlight(Language::find("diff").unwrap(), code);
        let at = |l: usize, s: &str| {
            let line = code.split('\n').nth(l).unwrap();
            let i = line.find(s).unwrap();
            lines[l]
                .iter()
                .find(|sp| sp.range.contains(&i))
                .map(|sp| sp.kind)
        };
        assert_eq!(at(2, "a/a.txt"), Some(Kind::Comment), "{:?}", lines[2]);
        assert_eq!(at(3, "b/a.txt"), Some(Kind::Comment), "{:?}", lines[3]);
        assert_eq!(at(4, "@@"), Some(Kind::Function), "{:?}", lines[4]);
        assert_eq!(at(6, "line 2"), Some(Kind::Deleted), "{:?}", lines[6]);
        assert_eq!(at(7, "line TWO"), Some(Kind::Inserted), "{:?}", lines[7]);
        assert!(
            lines[5]
                .iter()
                .all(|sp| !matches!(sp.kind, Kind::Inserted | Kind::Deleted)),
            "{:?}",
            lines[5]
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
