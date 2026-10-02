//! The parser: tokens to a rowan tree, never failing. What it does not
//! understand stays in the tree as text; unbalanced input is closed as
//! TeX users expect it (a group left open ends at the paragraph, an
//! environment left open at the next sectioning command) and reported.

use rowan::{GreenNode, GreenNodeBuilder};

use crate::lexer::{self, Tok};
use crate::signatures;
use crate::tables::{self, Tables};
use crate::{Diagnostic, SyntaxKind, SyntaxKind::*};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Text,
    Math,
}

/// What ends a sequence besides its limit.
#[derive(Debug, Clone, Copy, Default)]
struct Stop {
    par: bool,
    bracket: bool,
    dollar: bool,
    double_dollar: bool,
    paren: bool,
    brack: bool,
}

const PAR: Stop = Stop {
    par: true,
    bracket: false,
    dollar: false,
    double_dollar: false,
    paren: false,
    brack: false,
};

/// How deep groups, environments and math nest in the tree; deeper ones
/// stay tokens (rowan's trees are recursive, and so is dropping them).
const MAX_DEPTH: usize = 1000;

pub(crate) struct Parser<'a> {
    depth: usize,
    src: &'a str,
    b: &'a [u8],
    pos: usize,
    at_letter: bool,
    tables: Tables,
    builder: GreenNodeBuilder<'static>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) unclosed_env: bool,
    /// The last search for a `]` that failed: from, where it stopped,
    /// and its limit. A later search from inside that stretch, with the
    /// same limit, fails too (it is at the same group level), so
    /// unmatched `[` are not scanned again and again.
    opt_miss: std::cell::Cell<Option<(usize, usize, usize)>>,
}

fn kind(t: Tok) -> SyntaxKind {
    match t {
        Tok::ControlWord => CONTROL_WORD,
        Tok::ControlSymbol => CONTROL_SYMBOL,
        Tok::Comment => COMMENT,
        Tok::LBrace => L_BRACE,
        Tok::RBrace => R_BRACE,
        Tok::LBracket => L_BRACKET,
        Tok::RBracket => R_BRACKET,
        Tok::Dollar => DOLLAR,
        Tok::Ampersand => AMPERSAND,
        Tok::Hash => HASH,
        Tok::Caret => CARET,
        Tok::Underscore => UNDERSCORE,
        Tok::Tilde => TILDE,
        Tok::Whitespace => WHITESPACE,
        Tok::Newline => NEWLINE,
        Tok::ParBreak => PAR_BREAK,
        Tok::Text => TEXT,
    }
}

impl<'a> Parser<'a> {
    /// A parser of `src[start..end]`.
    pub(crate) fn new(src: &'a str, start: usize, end: usize, at_letter: bool) -> Parser<'a> {
        Parser {
            depth: 0,
            src,
            b: src.as_bytes(),
            pos: start,
            at_letter,
            tables: tables::build(src, start, end, at_letter),
            builder: GreenNodeBuilder::new(),
            diagnostics: Vec::new(),
            unclosed_env: false,
            opt_miss: std::cell::Cell::new(None),
        }
    }

    /// The `\makeatletter` and `\makeatother` commands found.
    pub(crate) fn toggles(&self) -> &[(usize, bool)] {
        &self.tables.toggles
    }

    /// Whether the text has sectioning commands.
    pub(crate) fn has_sections(&self) -> bool {
        !self.tables.sections.is_empty()
    }

    /// Parses to `end` as the contents of a node of kind `node`: the
    /// document, or an environment's body in `mode`.
    pub(crate) fn finish(mut self, node: SyntaxKind, end: usize, mode: Mode) -> (GreenNode, Self) {
        self.builder.start_node(node.into());
        self.container(end, mode);
        self.builder.finish_node();
        let builder = std::mem::take(&mut self.builder);
        self.diagnostics
            .sort_by_key(|d| (d.range.start, d.range.end));
        (builder.finish(), self)
    }

    fn lex(&self, limit: usize) -> (Tok, usize) {
        lexer::next(self.src, self.pos, limit, self.at_letter)
    }

    fn token(&mut self, kind: SyntaxKind, end: usize) {
        debug_assert!(end > self.pos);
        self.builder.token(kind.into(), &self.src[self.pos..end]);
        self.pos = end;
    }

    fn diag(&mut self, range: std::ops::Range<usize>, message: String) {
        self.diagnostics.push(Diagnostic { range, message });
    }

    /// Paragraphs and paragraph breaks.
    fn container(&mut self, limit: usize, mode: Mode) {
        while self.pos < limit {
            let (tok, end) = self.lex(limit);
            if tok == Tok::ParBreak {
                self.token(PAR_BREAK, end);
                continue;
            }
            self.builder.start_node(PARAGRAPH.into());
            self.seq(limit, mode, PAR);
            self.builder.finish_node();
        }
    }

    fn seq(&mut self, limit: usize, mode: Mode, stop: Stop) {
        while self.pos < limit {
            let (tok, end) = self.lex(limit);
            let here = match tok {
                Tok::ParBreak => stop.par,
                Tok::RBracket => stop.bracket,
                Tok::Dollar => {
                    stop.dollar
                        || (stop.double_dollar
                            && self.pos + 1 < limit
                            && self.b[self.pos + 1] == b'$')
                }
                Tok::ControlSymbol => {
                    let s = &self.src[self.pos..end];
                    (stop.paren && s == "\\)") || (stop.brack && s == "\\]")
                }
                _ => false,
            };
            if here {
                return;
            }
            stacker::maybe_grow(64 * 1024, 1024 * 1024, || {
                self.element(tok, end, limit, mode);
            });
        }
    }

    fn element(&mut self, tok: Tok, end: usize, limit: usize, mode: Mode) {
        if self.depth >= MAX_DEPTH {
            if matches!(
                tok,
                Tok::LBrace | Tok::ControlWord | Tok::Dollar | Tok::ControlSymbol
            ) {
                let at = self.pos;
                self.diag(at..end, "nested too deeply to be structured".into());
            }
            return self.token(kind(tok), end);
        }
        self.depth += 1;
        self.element_structured(tok, end, limit, mode);
        self.depth -= 1;
    }

    fn element_structured(&mut self, tok: Tok, end: usize, limit: usize, mode: Mode) {
        match tok {
            Tok::ControlWord => self.command(end, limit, mode),
            Tok::ControlSymbol => {
                let src = self.src;
                let s = &src[self.pos..end];
                match s {
                    "\\(" if mode == Mode::Text => {
                        let stop = Stop { paren: true, ..PAR };
                        self.math(INLINE_MATH, CONTROL_SYMBOL, 2, "\\)", limit, stop);
                    }
                    "\\[" if mode == Mode::Text => {
                        let stop = Stop { brack: true, ..PAR };
                        self.math(DISPLAY_MATH, CONTROL_SYMBOL, 2, "\\]", limit, stop);
                    }
                    "\\\\" => {
                        self.builder.start_node(COMMAND.into());
                        self.token(CONTROL_SYMBOL, end);
                        self.args("*o", limit, mode, true);
                        self.builder.finish_node();
                    }
                    "\\)" | "\\]" => {
                        let at = self.pos;
                        self.diag(at..end, format!("{s} without its opening"));
                        self.token(CONTROL_SYMBOL, end);
                    }
                    _ => self.token(CONTROL_SYMBOL, end),
                }
            }
            Tok::LBrace => self.group(limit, mode),
            Tok::RBrace => {
                let at = self.pos;
                self.diag(at..end, "} without {".into());
                self.token(R_BRACE, end);
            }
            Tok::Dollar if mode == Mode::Text => {
                if self.pos + 1 < limit && self.b[self.pos + 1] == b'$' {
                    let stop = Stop {
                        double_dollar: true,
                        ..PAR
                    };
                    self.math(DISPLAY_MATH, DOUBLE_DOLLAR, 2, "$$", limit, stop);
                } else {
                    let stop = Stop {
                        dollar: true,
                        ..PAR
                    };
                    self.math(INLINE_MATH, DOLLAR, 1, "$", limit, stop);
                }
            }
            t => self.token(kind(t), end),
        }
    }

    fn math(
        &mut self,
        node: SyntaxKind,
        delim: SyntaxKind,
        len: usize,
        close: &str,
        limit: usize,
        stop: Stop,
    ) {
        let open = self.pos;
        self.builder.start_node(node.into());
        self.token(delim, open + len);
        self.seq(limit, Mode::Math, stop);
        if self.pos + close.len() <= limit && self.src[self.pos..].starts_with(close) {
            let end = self.pos + close.len();
            self.token(delim, end);
        } else {
            self.diag(open..open + len, "math is not closed".into());
        }
        self.builder.finish_node();
    }

    fn group(&mut self, limit: usize, mode: Mode) {
        let open = self.pos;
        self.builder.start_node(GROUP.into());
        self.token(L_BRACE, open + 1);
        match self.tables.braces.get(&open) {
            Some(&close) if close < limit => {
                self.seq(close, mode, Stop::default());
                self.token(R_BRACE, close + 1);
            }
            _ => {
                self.diag(open..open + 1, "{ is not closed".into());
                self.seq(limit, mode, PAR);
            }
        }
        self.builder.finish_node();
    }

    /// Spaces and at most one line ending from `pos`: where an argument
    /// may start.
    fn skip_blank(&self, from: usize, limit: usize) -> usize {
        let mut i = from;
        let mut newline = false;
        while i < limit {
            match lexer::next(self.src, i, limit, self.at_letter) {
                (Tok::Whitespace, e) => i = e,
                (Tok::Newline, e) if !newline => {
                    newline = true;
                    i = e;
                }
                // TeX skips a comment and the line ending it:
                // `\section%⏎{Title}`.
                (Tok::Comment, e) => {
                    i = e;
                    if i < limit
                        && let (Tok::Newline, e) = lexer::next(self.src, i, limit, self.at_letter)
                    {
                        i = e;
                    }
                }
                _ => break,
            }
        }
        i
    }

    /// The blanks up to `to` as tokens.
    fn blanks(&mut self, to: usize) {
        while self.pos < to {
            let (tok, e) = self.lex(to);
            self.token(kind(tok), e);
        }
    }

    /// Where the optional argument starting after `[` at `from - 1` ends:
    /// the first `]` outside groups, before the paragraph ends.
    fn opt_close(&self, from: usize, limit: usize) -> Option<usize> {
        if let Some((a, b, l)) = self.opt_miss.get()
            && l == limit
            && a <= from
            && from <= b
        {
            return None;
        }
        let miss = |at: usize| {
            self.opt_miss.set(Some((from, at, limit)));
            None
        };
        let mut i = from;
        while i < limit {
            let (tok, e) = lexer::next(self.src, i, limit, self.at_letter);
            match tok {
                Tok::RBracket => return Some(i),
                Tok::ParBreak | Tok::RBrace => return miss(i),
                Tok::LBrace => match self.tables.braces.get(&i) {
                    Some(&c) if c < limit => {
                        i = c + 1;
                        continue;
                    }
                    _ => return miss(i),
                },
                Tok::ControlWord => {
                    if let Some(skip) = self.raw_skip(i, e, limit) {
                        i = skip;
                        continue;
                    }
                }
                _ => {}
            }
            i = e;
        }
        miss(limit)
    }

    /// Where the tables pass skipped text after the control word at
    /// `start..name_end` (verbatim arguments, environment names).
    fn raw_skip(&self, start: usize, name_end: usize, limit: usize) -> Option<usize> {
        match &self.src[start + 1..name_end] {
            "verb" => lexer::verb_end(self.b, name_end, limit, false),
            "lstinline" => lexer::verb_end(self.b, name_end, limit, true),
            "url" | "href" => lexer::raw_braces(self.b, name_end, limit).map(|(_, c)| c + 1),
            "begin" | "end" => lexer::env_name(self.b, name_end, limit).map(|(_, _, k)| k + 1),
            _ => None,
        }
    }

    fn args(&mut self, sig: &str, limit: usize, mode: Mode, immediate: bool) {
        for c in sig.bytes() {
            match c {
                b'*' => {
                    if self.pos < limit && self.b[self.pos] == b'*' {
                        let e = self.pos + 1;
                        self.token(STAR, e);
                    }
                }
                b'o' => {
                    let p = if immediate {
                        self.pos
                    } else {
                        self.skip_blank(self.pos, limit)
                    };
                    if p < limit
                        && self.b[p] == b'['
                        && let Some(close) = self.opt_close(p + 1, limit)
                    {
                        self.blanks(p);
                        self.builder.start_node(OPT_ARG.into());
                        self.token(L_BRACKET, p + 1);
                        self.seq(close, mode, Stop::default());
                        self.token(R_BRACKET, close + 1);
                        self.builder.finish_node();
                    }
                }
                _ => {
                    let p = self.skip_blank(self.pos, limit);
                    if p >= limit {
                        return;
                    }
                    let (tok, e) = lexer::next(self.src, p, limit, self.at_letter);
                    match tok {
                        Tok::LBrace => {
                            self.blanks(p);
                            self.group(limit, mode);
                        }
                        Tok::ControlWord
                            if !matches!(
                                &self.src[p + 1..e],
                                "begin"
                                    | "end"
                                    | "verb"
                                    | "lstinline"
                                    | "url"
                                    | "href"
                                    | "makeatletter"
                                    | "makeatother"
                            ) =>
                        {
                            self.blanks(p);
                            self.token(CONTROL_WORD, e);
                        }
                        Tok::ControlSymbol => {
                            self.blanks(p);
                            self.token(CONTROL_SYMBOL, e);
                        }
                        Tok::Text => {
                            self.blanks(p);
                            let e = p + lexer::char_len(self.b[p]);
                            self.token(TEXT, e);
                        }
                        _ => return,
                    }
                }
            }
        }
    }

    fn command(&mut self, name_end: usize, limit: usize, mode: Mode) {
        let start = self.pos;
        let src = self.src;
        let name = &src[start + 1..name_end];
        // `\be … \ee`, macros for an equation's environment: a displayed
        // formula.
        if mode == Mode::Text
            && let Some(&close) = self.tables.aliases.get(&start)
            && close < limit
        {
            let (_, close_end) = lexer::next(src, close, limit, self.at_letter);
            self.builder.start_node(DISPLAY_MATH.into());
            self.token(CONTROL_WORD, name_end);
            self.seq(close, Mode::Math, Stop::default());
            self.token(CONTROL_WORD, close_end);
            self.builder.finish_node();
            return;
        }
        match name {
            "begin" => {
                if let Some(n) = lexer::env_name(self.b, name_end, limit) {
                    return self.environment(name_end, n, limit);
                }
                self.diag(
                    start..name_end,
                    "\\begin without an environment name".into(),
                );
            }
            "end" => {
                if let Some((open, s, k)) = lexer::env_name(self.b, name_end, limit) {
                    let env = self.src[s..k].to_string();
                    self.diag(
                        start..k + 1,
                        format!("\\end{{{env}}} without \\begin{{{env}}}"),
                    );
                    self.end(name_end, open, s, k);
                    return;
                }
            }
            "verb" | "lstinline" => {
                if let Some(v) = lexer::verb_end(self.b, name_end, limit, name == "lstinline") {
                    self.builder.start_node(VERB.into());
                    self.token(CONTROL_WORD, name_end);
                    self.token(VERBATIM, v);
                    self.builder.finish_node();
                    return;
                }
                self.diag(
                    start..name_end,
                    format!("\\{name} without its closing delimiter"),
                );
            }
            "url" | "href" => {
                if let Some((open, close)) = lexer::raw_braces(self.b, name_end, limit) {
                    let href = name == "href";
                    self.builder.start_node(COMMAND.into());
                    self.token(CONTROL_WORD, name_end);
                    self.blanks(open);
                    self.builder.start_node(GROUP.into());
                    self.token(L_BRACE, open + 1);
                    if close > open + 1 {
                        self.token(VERBATIM, close);
                    }
                    self.token(R_BRACE, close + 1);
                    self.builder.finish_node();
                    if href {
                        self.args("m", limit, mode, false);
                    }
                    self.builder.finish_node();
                    return;
                }
                self.diag(start..name_end, format!("\\{name} without its address"));
            }
            "makeatletter" | "makeatother" => {
                self.at_letter = name == "makeatletter";
            }
            "def" | "gdef" | "edef" | "xdef" => return self.def(name_end, limit, mode),
            "left" | "right" | "middle" => {
                self.builder.start_node(COMMAND.into());
                self.token(CONTROL_WORD, name_end);
                let p = self.skip_blank(self.pos, limit);
                if p < limit {
                    let (tok, e) = lexer::next(self.src, p, limit, self.at_letter);
                    let e = match tok {
                        Tok::Text => Some(p + lexer::char_len(self.b[p])),
                        Tok::ControlSymbol | Tok::ControlWord | Tok::LBracket | Tok::RBracket => {
                            Some(e)
                        }
                        _ => None,
                    };
                    if let Some(e) = e {
                        // The tables toggle `@` here too; stay in step
                        // with them.
                        if tok == Tok::ControlWord {
                            match &self.src[p + 1..e] {
                                "makeatletter" => self.at_letter = true,
                                "makeatother" => self.at_letter = false,
                                _ => {}
                            }
                        }
                        self.blanks(p);
                        self.token(kind(tok), e);
                    }
                }
                self.builder.finish_node();
                return;
            }
            _ => {}
        }
        let sig = match name {
            "begin" | "end" | "verb" | "lstinline" => "",
            "url" => "m",
            n => signatures::command(n),
        };
        let mode = if signatures::text_arguments(name) {
            Mode::Text
        } else {
            mode
        };
        self.builder.start_node(COMMAND.into());
        self.token(CONTROL_WORD, name_end);
        self.args(sig, limit, mode, false);
        self.builder.finish_node();
    }

    /// `\def\name#1#2{…}`.
    fn def(&mut self, name_end: usize, limit: usize, mode: Mode) {
        self.builder.start_node(COMMAND.into());
        self.token(CONTROL_WORD, name_end);
        while self.pos < limit {
            let (tok, e) = self.lex(limit);
            match tok {
                Tok::LBrace => {
                    self.group(limit, mode);
                    break;
                }
                Tok::ParBreak | Tok::RBrace => break,
                t => {
                    // The tables toggle `@` here too; stay in step with them.
                    if t == Tok::ControlWord {
                        match &self.src[self.pos + 1..e] {
                            "makeatletter" => self.at_letter = true,
                            "makeatother" => self.at_letter = false,
                            _ => {}
                        }
                    }
                    self.token(kind(t), e)
                }
            }
        }
        self.builder.finish_node();
    }

    /// `\end{name}`, the braces at `open`, the name at `s..k`.
    fn end(&mut self, name_end: usize, open: usize, s: usize, k: usize) {
        self.builder.start_node(END.into());
        self.token(CONTROL_WORD, name_end);
        self.env_group(open, s, k);
        self.builder.finish_node();
    }

    fn env_group(&mut self, open: usize, s: usize, k: usize) {
        self.blanks(open);
        self.builder.start_node(GROUP.into());
        self.token(L_BRACE, s);
        self.token(ENV_NAME, k);
        self.token(R_BRACE, k + 1);
        self.builder.finish_node();
    }

    fn environment(&mut self, name_end: usize, (open, s, k): (usize, usize, usize), limit: usize) {
        let begin = self.pos;
        let src = self.src;
        let name = &src[s..k];
        let verbatim = signatures::is_verbatim(name);
        let math = signatures::is_math(name);
        let sig = signatures::environment(name);
        let closed = self.tables.envs.get(&begin).copied().filter(|&e| e < limit);
        self.builder.start_node(ENVIRONMENT.into());
        self.builder.start_node(BEGIN.into());
        self.token(CONTROL_WORD, name_end);
        self.env_group(open, s, k);
        let arg_limit = closed.unwrap_or(limit);
        if verbatim {
            self.raw_args(sig, arg_limit);
        } else {
            self.args(sig, arg_limit, Mode::Text, false);
        }
        self.builder.finish_node();
        let body_end = match closed {
            Some(e) => e,
            None => {
                self.diag(begin..k + 1, format!("\\begin{{{name}}} is not closed"));
                self.unclosed_env = true;
                self.tables
                    .section_after(self.pos)
                    .unwrap_or(limit)
                    .min(limit)
            }
        }
        .max(self.pos);
        self.builder.start_node(BODY.into());
        if verbatim {
            if self.pos < body_end {
                self.token(VERBATIM, body_end);
            }
        } else {
            self.container(body_end, if math { Mode::Math } else { Mode::Text });
        }
        self.builder.finish_node();
        if let Some(e) = closed {
            let (open, s, k) = lexer::env_name(self.b, e + 4, limit).expect("paired by the tables");
            self.end(e + 4, open, s, k);
        }
        self.builder.finish_node();
    }

    /// The arguments of a verbatim environment: right after `\begin{…}`,
    /// on its line, taken as they are.
    fn raw_args(&mut self, sig: &str, limit: usize) {
        for c in sig.bytes() {
            let eol = lexer::line_end(self.b, self.pos, limit);
            let at = self.pos;
            match c {
                b'o' if at < eol && self.b[at] == b'[' => {
                    let Some(close) = self.b[at + 1..eol].iter().position(|&c| c == b']') else {
                        return;
                    };
                    let close = at + 1 + close;
                    self.builder.start_node(OPT_ARG.into());
                    self.token(L_BRACKET, at + 1);
                    if close > at + 1 {
                        self.token(VERBATIM, close);
                    }
                    self.token(R_BRACKET, close + 1);
                    self.builder.finish_node();
                }
                b'm' if at < eol && self.b[at] == b'{' => {
                    let mut depth = 0usize;
                    let mut close = None;
                    for (i, &c) in self.b[at..eol].iter().enumerate() {
                        match c {
                            b'{' => depth += 1,
                            b'}' => {
                                depth -= 1;
                                if depth == 0 {
                                    close = Some(at + i);
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    let Some(close) = close else { return };
                    self.builder.start_node(GROUP.into());
                    self.token(L_BRACE, at + 1);
                    if close > at + 1 {
                        self.token(VERBATIM, close);
                    }
                    self.token(R_BRACE, close + 1);
                    self.builder.finish_node();
                }
                b'o' => {}
                _ => return,
            }
        }
    }
}
