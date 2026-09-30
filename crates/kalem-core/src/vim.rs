//! Vim keys (§7.3.1): the modal input layer of the Vim keymap profile.
//!
//! The layer works on the document state, so both frontends share it, and
//! it leaves insert mode to them: typing and the Word-like keys work as
//! usual there, and only Escape comes back here. In the other modes every
//! key comes here first; keys with Control or Alt that Vim does not use go
//! on to the keymap (so Ctrl+S still saves and Ctrl+C copies).
//!
//! Covered: normal, insert, visual (characters and lines) and replace
//! modes; counts; the motions `h j k l w b e W B E 0 ^ $ gg G f t F T ; ,
//! % { } + - _ Enter H M L n N` and Ctrl+D/U/F/B; the operators `d c y > <
//! gu gU g~` with motions, doubled for lines, and the text objects `iw aw
//! iW aW`, quotes, brackets and `ip ap`; `x X D C Y s S p P J r ~ u Ctrl+R
//! . i a I A o O R v V`; registers `"a`-`"z` (`"A` appends) and the system
//! clipboard `"+`; search with `/ ? n N * #` (regular expressions as in the
//! find bar); and the command line `:w :q :q! :wq :x :N :$ :noh`. In Org
//! documents `>>` and `<<` demote and promote headlines and indent list
//! items. Block selection, macros, marks and `:s` come later (§7.3.1).

use std::collections::HashMap;
use std::ops::Range;
use std::time::Instant;

use org_edit::{ChangeKind, Selection, Transaction};
use serde_json::Value;

use crate::DocumentState;
use crate::mode::DocumentMode;

/// A key as the Vim layer sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A typed character, Shift applied.
    Char(char),
    /// Control with a letter or symbol.
    Ctrl(char),
    /// Escape.
    Esc,
    /// Enter.
    Enter,
    /// Backspace.
    Backspace,
    /// Tab.
    Tab,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Any other key (with Alt, function keys).
    Other,
}

/// A Vim mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Keys are commands.
    Normal,
    /// Typing inserts (the frontend handles it).
    Insert,
    /// Characters are selected.
    Visual,
    /// Lines are selected.
    VisualLine,
    /// A block of columns is selected (Ctrl+V).
    VisualBlock,
    /// Typing overwrites.
    Replace,
}

impl Mode {
    /// The mode's name for the status bar, a message ID of `l10n`.
    pub fn message_id(self) -> &'static str {
        match self {
            Mode::Normal => "vim-normal",
            Mode::Insert => "vim-insert",
            Mode::Visual => "vim-visual",
            Mode::VisualLine => "vim-visual-line",
            Mode::VisualBlock => "vim-visual-block",
            Mode::Replace => "vim-replace",
        }
    }
}

/// What the frontend offers the layer.
pub trait Host {
    /// The system clipboard's text, for `"+p`.
    fn clipboard(&mut self) -> Option<String>;
    /// Puts `text` on the system clipboard, for `"+y`.
    fn set_clipboard(&mut self, text: &str);
    /// Lines on screen, for Ctrl+D and the like.
    fn page_lines(&self) -> usize {
        20
    }
    /// The document shows as rich text (not its source): `h` and `l` move
    /// over what is shown, skipping hidden markers.
    fn rich_view(&self) -> bool {
        false
    }
}

/// One character left or right of `pos`: over the shown text in the rich
/// view, else over the source.
fn step(doc: &DocumentState, pos: usize, right: bool, rich: bool) -> usize {
    if rich && let Some(p) = crate::view::visible_step(doc, pos, right) {
        return p;
    }
    if right {
        doc.grapheme_after(pos)
    } else {
        doc.grapheme_before(pos)
    }
}

/// What a key did, for the frontend to finish.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Outcome {
    /// The key was used (the frontend does nothing more with it).
    pub handled: bool,
    /// Registry commands to run, in order (`:w` saves).
    pub commands: Vec<(String, Value)>,
    /// A message, and whether it is an error.
    pub message: Option<(String, bool)>,
    /// New search marks (empty to clear them).
    pub highlights: Option<Vec<Range<usize>>>,
    /// Close without saving (`:q!`).
    pub force_quit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Delete,
    Change,
    Yank,
    Indent,
    Outdent,
    Lower,
    Upper,
    Toggle,
}

impl Op {
    fn of(c: char) -> Option<Op> {
        Some(match c {
            'd' => Op::Delete,
            'c' => Op::Change,
            'y' => Op::Yank,
            '>' => Op::Indent,
            '<' => Op::Outdent,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Find {
    /// `f`: to the character.
    To,
    /// `t`: before it.
    Till,
    /// `F`: back to it.
    Back,
    /// `T`: back, after it.
    BackTill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    None,
    G,
    Find(Find),
    Replace,
    Register,
    Object(bool),
}

/// A register's contents.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Register {
    text: String,
    linewise: bool,
}

/// Where a motion goes.
#[derive(Debug, Clone, Copy)]
struct Motion {
    to: usize,
    linewise: bool,
    inclusive: bool,
}

/// The last change, for `.`.
#[derive(Debug, Clone, Default)]
struct Change {
    keys: Vec<Key>,
    inserted: Option<String>,
}

/// What an operator works on.
#[derive(Debug, Clone)]
enum Target {
    /// Characters.
    Chars(Range<usize>),
    /// Whole lines, first and last.
    Lines(usize, usize),
    /// A block: lines `first..=last`, columns `left..right` (in
    /// characters).
    Block {
        first: usize,
        last: usize,
        left: usize,
        right: usize,
    },
}

/// The state of the Vim layer for one document.
#[derive(Debug, Clone)]
pub struct Vim {
    /// The mode.
    pub mode: Mode,
    /// The command line being typed: `:w`, `/word`, `?word`.
    pub command_line: Option<String>,
    count: Option<usize>,
    op: Option<(Op, usize)>,
    register: Option<char>,
    pending: Pending,
    registers: HashMap<char, Register>,
    last_find: Option<(Find, char)>,
    last_search: Option<(String, bool)>,
    goal: Option<usize>,
    anchor: usize,
    cursor: usize,
    motion_count: usize,
    keys: Vec<Key>,
    last_change: Option<Change>,
    recording: Option<Change>,
    insert_at: Option<usize>,
    /// Text typed at the start of a block (`I`, `c`) or after it (`A`) is
    /// repeated on these lines at this column when insert mode ends; `A`
    /// pads short lines with spaces.
    block_insert: Option<(Range<usize>, usize, bool)>,
    replaying: bool,
    /// The leader key: in normal and visual mode it is left to the keymap,
    /// which binds sequences starting with it (Doom Emacs's `SPC p p`).
    pub leader: Option<Key>,
}

impl Default for Vim {
    fn default() -> Self {
        Vim::new()
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// A character's class for word motions: blanks 0, word characters 1,
/// other characters 2 (for `W` and the like, everything not blank is 1).
fn class(c: char, big: bool) -> u8 {
    if c.is_whitespace() {
        0
    } else if big || is_word(c) {
        1
    } else {
        2
    }
}

fn find_kind(c: char) -> Find {
    match c {
        'f' => Find::To,
        't' => Find::Till,
        'F' => Find::Back,
        _ => Find::BackTill,
    }
}

fn regex_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

// Positions in the document.

fn line_of(doc: &DocumentState, pos: usize) -> usize {
    doc.text().line_of(pos.min(doc.text().len()))
}

/// The last line as Vim counts them: not the empty one after a final line
/// feed.
fn last_line(doc: &DocumentState) -> usize {
    let n = doc.text().line_count();
    if n > 1 && doc.text().as_str().ends_with('\n') {
        n - 2
    } else {
        n - 1
    }
}

fn line_start(doc: &DocumentState, line: usize) -> usize {
    doc.text().line_start(line)
}

/// The end of `line`, before its line feed (and a CR before that).
fn line_end(doc: &DocumentState, line: usize) -> usize {
    let text = doc.text();
    let r = text.line_range(line);
    if r.end > r.start && text.as_str().as_bytes()[r.end - 1] == b'\r' {
        r.end - 1
    } else {
        r.end
    }
}

fn first_non_blank(doc: &DocumentState, line: usize) -> usize {
    let s = line_start(doc, line);
    let e = line_end(doc, line);
    s + doc.text().as_str()[s..e]
        .bytes()
        .take_while(|b| *b == b' ' || *b == b'\t')
        .count()
}

fn column(doc: &DocumentState, pos: usize) -> usize {
    let s = line_start(doc, line_of(doc, pos));
    doc.text().as_str()[s..pos].chars().count()
}

fn at_column(doc: &DocumentState, line: usize, col: usize) -> usize {
    let s = line_start(doc, line);
    let e = line_end(doc, line);
    doc.text().as_str()[s..e]
        .char_indices()
        .nth(col)
        .map_or(e, |(i, _)| s + i)
}

fn char_at(doc: &DocumentState, pos: usize) -> Option<char> {
    let text = doc.text().as_str();
    text.get(pos.min(text.len())..)?.chars().next()
}

fn char_before(doc: &DocumentState, pos: usize) -> Option<char> {
    doc.text().as_str().get(..pos)?.chars().next_back()
}

fn blank_line(doc: &DocumentState, line: usize) -> bool {
    let r = doc.text().line_range(line);
    doc.text().as_str()[r].trim().is_empty()
}

/// Lines `a..=b` with the line feed that goes with them (the one before
/// them for a last line without one).
fn line_span(doc: &DocumentState, a: usize, b: usize) -> Range<usize> {
    let text = doc.text();
    let s = text.line_start(a);
    if b + 1 < text.line_count() {
        s..text.line_start(b + 1)
    } else if a > 0 {
        let before = if text.as_str()[..s].ends_with("\r\n") {
            2
        } else {
            1
        };
        (s - before)..text.len()
    } else {
        s..text.len()
    }
}

fn edit(doc: &mut DocumentState, range: Range<usize>, insert: &str, caret: usize) {
    let mut tx = Transaction::new("Vim");
    tx.replace(range, insert).expect("one edit");
    let tx = tx.select(Selection::caret(caret));
    doc.apply(&tx, ChangeKind::Command, Instant::now());
}

// Motions.

fn word_forward(doc: &DocumentState, mut pos: usize, big: bool, stop_at_eol: bool) -> usize {
    let len = doc.text().len();
    if pos >= len {
        return len;
    }
    let k = class(char_at(doc, pos).unwrap_or(' '), big);
    if k != 0 {
        while let Some(c) = char_at(doc, pos) {
            if class(c, big) != k || c == '\n' {
                break;
            }
            pos += c.len_utf8();
        }
    }
    while let Some(c) = char_at(doc, pos) {
        if c == '\n' {
            if stop_at_eol {
                return pos;
            }
            pos += 1;
            // An empty line is a word.
            if char_at(doc, pos) == Some('\n') {
                return pos;
            }
            continue;
        }
        if !c.is_whitespace() {
            break;
        }
        pos += c.len_utf8();
    }
    pos
}

fn word_end(doc: &DocumentState, mut pos: usize, big: bool) -> usize {
    let len = doc.text().len();
    if let Some(c) = char_at(doc, pos) {
        pos += c.len_utf8();
    }
    while let Some(c) = char_at(doc, pos) {
        if !c.is_whitespace() {
            break;
        }
        pos += c.len_utf8();
    }
    if char_at(doc, pos).is_none() {
        return len.saturating_sub(1);
    }
    word_end_from(doc, pos, big)
}

/// The last character of the word at `pos`.
fn word_end_from(doc: &DocumentState, pos: usize, big: bool) -> usize {
    let k = char_at(doc, pos).map_or(0, |c| class(c, big));
    let mut p = pos;
    loop {
        let next = p + char_at(doc, p).map_or(1, char::len_utf8);
        match char_at(doc, next) {
            Some(c) if class(c, big) == k && c != '\n' => p = next,
            _ => return p,
        }
    }
}

fn word_back(doc: &DocumentState, mut pos: usize, big: bool) -> usize {
    while let Some(c) = char_before(doc, pos) {
        // An empty line is a word.
        if c == '\n' && pos >= 2 && char_before(doc, pos - 1) == Some('\n') {
            return pos - 1;
        }
        if !c.is_whitespace() {
            break;
        }
        pos -= c.len_utf8();
    }
    let Some(c0) = char_before(doc, pos) else {
        return 0;
    };
    let k = class(c0, big);
    while let Some(c) = char_before(doc, pos) {
        if class(c, big) != k || c == '\n' {
            break;
        }
        pos -= c.len_utf8();
    }
    pos
}

fn find_char(
    doc: &DocumentState,
    pos: usize,
    kind: Find,
    target: char,
    count: usize,
) -> Option<usize> {
    let line = line_of(doc, pos);
    let (s, e) = (line_start(doc, line), line_end(doc, line));
    let text = doc.text().as_str();
    let mut p = pos;
    for i in 0..count {
        match kind {
            Find::To | Find::Till => {
                // Repeating `t` next to its target goes on to the next one.
                let from = if kind == Find::Till && i == 0 {
                    doc.grapheme_after(p)
                } else {
                    p
                };
                let start = doc.grapheme_after(from).min(e);
                p = start + text[start..e].find(target)?;
            }
            Find::Back | Find::BackTill => {
                let end = if kind == Find::BackTill && i == 0 {
                    doc.grapheme_before(p).max(s)
                } else {
                    p
                };
                p = s + text[s..end].rfind(target)?;
            }
        }
    }
    Some(match kind {
        Find::Till => doc.grapheme_before(p).max(pos),
        Find::BackTill => doc.grapheme_after(p).min(pos),
        _ => p,
    })
}

fn matching_bracket(doc: &DocumentState, pos: usize) -> Option<usize> {
    let text = doc.text().as_str();
    let end = line_end(doc, line_of(doc, pos));
    let (at, c) = text[pos..end]
        .char_indices()
        .find(|(_, c)| "()[]{}".contains(*c))
        .map(|(i, c)| (pos + i, c))?;
    let (open, close, forward) = match c {
        '(' => ('(', ')', true),
        '[' => ('[', ']', true),
        '{' => ('{', '}', true),
        ')' => ('(', ')', false),
        ']' => ('[', ']', false),
        _ => ('{', '}', false),
    };
    let mut depth = 0i32;
    if forward {
        for (i, ch) in text[at..].char_indices() {
            if ch == open {
                depth += 1;
            } else if ch == close {
                depth -= 1;
                if depth == 0 {
                    return Some(at + i);
                }
            }
        }
    } else {
        for (i, ch) in text[..=at].char_indices().rev() {
            if ch == close {
                depth += 1;
            } else if ch == open {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }
    }
    None
}

fn paragraph(doc: &DocumentState, pos: usize, forward: bool, count: usize) -> usize {
    let last = last_line(doc);
    let mut line = line_of(doc, pos);
    for _ in 0..count {
        if forward {
            while line < last && blank_line(doc, line) {
                line += 1;
            }
            while line < last && !blank_line(doc, line) {
                line += 1;
            }
            if line >= last && !blank_line(doc, line) {
                return line_end(doc, line);
            }
        } else {
            while line > 0 && blank_line(doc, line) {
                line -= 1;
            }
            while line > 0 && !blank_line(doc, line) {
                line -= 1;
            }
        }
    }
    line_start(doc, line)
}

fn search_from(
    doc: &DocumentState,
    pattern: &str,
    back: bool,
    from: usize,
    count: usize,
) -> Option<usize> {
    let opts = crate::find::FindOptions { regex: true };
    let matches = crate::find::find_with(doc.text().as_str(), pattern, opts).ok()?;
    let mut p = from;
    for _ in 0..count {
        let next = if back {
            crate::find::next(&matches, p, true)?
        } else {
            crate::find::next(&matches, p + 1, false)?
        };
        p = next.start;
    }
    Some(p)
}

// Text objects.

fn object(doc: &DocumentState, pos: usize, c: char, inner: bool) -> Option<Target> {
    let text = doc.text().as_str();
    let pos = pos.min(text.len());
    match c {
        'w' | 'W' => {
            let big = c == 'W';
            let line = line_of(doc, pos);
            let (s, e) = (line_start(doc, line), line_end(doc, line));
            let k = class(char_at(doc, pos).filter(|c| *c != '\n').unwrap_or(' '), big);
            let mut a = pos;
            while a > s && char_before(doc, a).is_some_and(|ch| class(ch, big) == k) {
                a -= char_before(doc, a).map_or(1, char::len_utf8);
            }
            let mut b = pos;
            while b < e && char_at(doc, b).is_some_and(|ch| class(ch, big) == k) {
                b += char_at(doc, b).map_or(1, char::len_utf8);
            }
            if !inner {
                // With the blanks after it, or before it at the end.
                let mut b2 = b;
                while b2 < e && char_at(doc, b2).is_some_and(|ch| ch == ' ' || ch == '\t') {
                    b2 += 1;
                }
                if b2 > b {
                    b = b2;
                } else {
                    while a > s && char_before(doc, a).is_some_and(|ch| ch == ' ' || ch == '\t') {
                        a -= 1;
                    }
                }
            }
            Some(Target::Chars(a..b))
        }
        '"' | '\'' | '`' => {
            let line = line_of(doc, pos);
            let (s, e) = (line_start(doc, line), line_end(doc, line));
            let quotes: Vec<usize> = text[s..e].match_indices(c).map(|(i, _)| s + i).collect();
            let pairs: Vec<&[usize]> = quotes.chunks(2).filter(|p| p.len() == 2).collect();
            let pair = pairs
                .iter()
                .find(|p| p[0] <= pos && pos <= p[1])
                .or_else(|| pairs.iter().find(|p| p[0] > pos))?;
            let (a, b) = (pair[0], pair[1]);
            Some(Target::Chars(if inner { a + 1..b } else { a..b + 1 }))
        }
        '(' | ')' | 'b' | '[' | ']' | '{' | '}' | 'B' | '<' | '>' => {
            let (open, close) = match c {
                '(' | ')' | 'b' => ('(', ')'),
                '[' | ']' => ('[', ']'),
                '<' | '>' => ('<', '>'),
                _ => ('{', '}'),
            };
            // The innermost pair around the cursor.
            let mut depth = 0i32;
            let mut a = None;
            let upto = (pos + char_at(doc, pos).map_or(0, char::len_utf8)).min(text.len());
            for (i, ch) in text[..upto].char_indices().rev() {
                if ch == close && i != pos {
                    depth += 1;
                } else if ch == open {
                    if depth == 0 {
                        a = Some(i);
                        break;
                    }
                    depth -= 1;
                }
            }
            let a = a?;
            depth = 0;
            let mut b = None;
            for (i, ch) in text[a..].char_indices() {
                if ch == open {
                    depth += 1;
                } else if ch == close {
                    depth -= 1;
                    if depth == 0 {
                        b = Some(a + i);
                        break;
                    }
                }
            }
            let b = b?;
            Some(Target::Chars(if inner { a + 1..b } else { a..b + 1 }))
        }
        'p' => {
            let last = last_line(doc);
            let line = line_of(doc, pos);
            let blank = blank_line(doc, line);
            let mut first = line;
            while first > 0 && blank_line(doc, first - 1) == blank {
                first -= 1;
            }
            let mut end = line;
            while end < last && blank_line(doc, end + 1) == blank {
                end += 1;
            }
            if !inner {
                while end < last && blank_line(doc, end + 1) {
                    end += 1;
                }
            }
            Some(Target::Lines(first, end))
        }
        'h' | 'R' | 'i' | 'c' | 'e' => org_object(doc, pos, c, inner),
        _ => None,
    }
}

/// Org's text objects, from the current parse: `h` the headline (inner:
/// its title), `R` the subtree (inner: below its headline), `i` the list
/// item (inner: its text after the bullet and checkbox), `c` the table
/// cell (inner: its text; outer: with its blanks and the `|` after it),
/// `e` the emphasis or code around the cursor (inner: inside the
/// markers; outer: with the markers and the blanks after them).
fn org_object(doc: &DocumentState, pos: usize, c: char, inner: bool) -> Option<Target> {
    use org_syntax::SyntaxKind::*;
    let (parse, fresh) = doc.parse()?;
    if !fresh {
        return None;
    }
    let root = parse.syntax();
    let len = doc.text().len();
    let at = org_syntax::TextSize::from(pos.min(len) as u32);
    let token = root
        .token_at_offset(at)
        .right_biased()
        .or_else(|| root.token_at_offset(at).left_biased())?;
    let find = |kinds: &[org_syntax::SyntaxKind]| {
        token.parent_ancestors().find(|n| kinds.contains(&n.kind()))
    };
    let range = |n: &org_syntax::SyntaxNode| {
        let r = n.text_range();
        usize::from(r.start())..usize::from(r.end())
    };
    // The lines of `r`, blank lines at its end left out.
    let lines = |r: std::ops::Range<usize>| -> Option<Target> {
        let first = line_of(doc, r.start);
        let mut last = line_of(doc, r.end.saturating_sub(1).max(r.start));
        while last > first && blank_line(doc, last) {
            last -= 1;
        }
        Some(Target::Lines(first, last))
    };
    match c {
        'h' | 'R' => {
            let h = find(&[HEADLINE])?;
            let r = range(&h);
            let head_line = line_of(doc, r.start);
            match (c, inner) {
                ('h', false) => Some(Target::Lines(head_line, head_line)),
                ('h', true) => {
                    let t = h.children().find(|n| n.kind() == HEADLINE_TITLE)?;
                    let tr = range(&t);
                    let text = &doc.text().as_str()[tr.clone()];
                    let end = tr.start + text.trim_end().len();
                    Some(Target::Chars(tr.start..end))
                }
                ('R', false) => lines(r),
                _ => {
                    let body = line_end(doc, head_line) + 1;
                    if body >= r.end {
                        return None;
                    }
                    lines(body..r.end)
                }
            }
        }
        'i' => {
            let item = find(&[ITEM])?;
            let r = range(&item);
            if !inner {
                return lines(r);
            }
            // After the bullet, the counter set and the checkbox.
            let mut start = r.start;
            for el in item.children_with_tokens() {
                match el.kind() {
                    BULLET | CHECKBOX | COUNTER | WHITESPACE => {
                        start = usize::from(el.text_range().end())
                    }
                    _ => break,
                }
            }
            let text = &doc.text().as_str()[start..r.end];
            Some(Target::Chars(start..start + text.trim_end().len()))
        }
        'c' => {
            let cell = find(&[TABLE_CELL])?;
            let r = range(&cell);
            let text = &doc.text().as_str()[r.clone()];
            if inner {
                let body = text.trim_end_matches('|');
                let lead = body.len() - body.trim_start().len();
                let a = r.start + lead;
                return Some(Target::Chars(a..(a + body.trim().len()).max(a)));
            }
            Some(Target::Chars(r))
        }
        'e' => {
            let e = find(&[BOLD, ITALIC, UNDERLINE, STRIKE_THROUGH, CODE, VERBATIM])?;
            let r = range(&e);
            let end = r.end - org_syntax::ast::post_blank(&e);
            // Outer: with the blanks after it, as `aw`.
            Some(Target::Chars(if inner { r.start + 1..end - 1 } else { r }))
        }
        _ => None,
    }
}

impl Vim {
    /// A layer in normal mode.
    pub fn new() -> Vim {
        Vim {
            mode: Mode::Normal,
            command_line: None,
            count: None,
            op: None,
            register: None,
            pending: Pending::None,
            registers: HashMap::new(),
            last_find: None,
            last_search: None,
            goal: None,
            anchor: 0,
            cursor: 0,
            motion_count: 1,
            keys: Vec::new(),
            last_change: None,
            recording: None,
            insert_at: None,
            block_insert: None,
            replaying: false,
            leader: Some(Key::Char(' ')),
        }
    }

    /// The Vim key of leader `keys` (`space`, `,`, `\\`), if it is one key
    /// Vim would use; other leaders reach the keymap anyway.
    pub fn leader_key(keys: &str) -> Option<Key> {
        let chord = crate::keys::KeySequence::parse(keys)?
            .0
            .into_iter()
            .next()?;
        let m = chord.mods;
        if m.ctrl || m.alt || m.cmd {
            return None;
        }
        match chord.key.as_str() {
            "space" => Some(Key::Char(' ')),
            k if k.chars().count() == 1 => {
                let c = k.chars().next()?;
                Some(Key::Char(if m.shift { c.to_ascii_uppercase() } else { c }))
            }
            _ => None,
        }
    }

    /// The mode's name for when-clauses (`vimMode`).
    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            Mode::Normal => "normal",
            Mode::Insert => "insert",
            Mode::Visual => "visual",
            Mode::VisualLine => "visualLine",
            Mode::VisualBlock => "visualBlock",
            Mode::Replace => "replace",
        }
    }

    /// Keys are commands, with no command half typed: normal or visual
    /// mode, where the leader and keymap sequences apply.
    pub fn idle_command(&self) -> bool {
        matches!(
            self.mode,
            Mode::Normal | Mode::Visual | Mode::VisualLine | Mode::VisualBlock
        ) && self.idle()
    }

    /// Whether typed text goes into the document (insert mode).
    pub fn takes_text(&self) -> bool {
        self.mode == Mode::Insert && self.command_line.is_none()
    }

    /// Whether the cursor is a block on a character (not a bar).
    pub fn block_cursor(&self) -> bool {
        !self.takes_text()
    }

    /// Where the block cursor is: the character under it (in visual mode
    /// the moving end, not the selection's end).
    pub fn caret(&self, doc: &DocumentState) -> usize {
        let at = if self.visual() {
            self.cursor
        } else {
            doc.selection.head
        };
        at.min(doc.text().len())
    }

    /// Whether the Vim layer applies to documents of `mode` with the
    /// `editor.vim.modes` setting `modes` (empty for all).
    pub fn applies(modes: &[&str], mode: &DocumentMode) -> bool {
        let name = match mode {
            DocumentMode::Binary => "text",
            m => m.name(),
        };
        modes.is_empty() || modes.contains(&name) || (name == "text" && modes.contains(&"plain"))
    }

    /// The status bar's text: the mode, or the command line being typed.
    pub fn status(&self) -> String {
        match &self.command_line {
            Some(l) => format!("{l}▏"),
            None => crate::l10n::tr(self.mode.message_id()),
        }
    }

    /// Handles `key`.
    pub fn key(&mut self, doc: &mut DocumentState, key: Key, host: &mut dyn Host) -> Outcome {
        // Horizontal motions in the rich view read the current parse.
        if host.rich_view()
            && matches!(
                key,
                Key::Char('h' | 'l' | ' ') | Key::Left | Key::Right | Key::Backspace
            )
        {
            doc.wait_for_parse();
        }
        let out = self.key_inner(doc, key, host);
        // The cursor stays in the text, even where an edit was refused (a
        // folder listing is read-only), and such a document takes no text.
        let len = doc.text().len();
        if doc.selection.head > len || doc.selection.anchor > len {
            doc.selection = Selection {
                anchor: doc.selection.anchor.min(len),
                head: doc.selection.head.min(len),
            };
        }
        self.cursor = self.cursor.min(len);
        if doc.dired.as_ref().is_some_and(|d| d.wdired.is_none())
            && matches!(self.mode, Mode::Insert | Mode::Replace)
        {
            self.mode = Mode::Normal;
            self.clamp(doc);
        }
        out
    }

    fn key_inner(&mut self, doc: &mut DocumentState, key: Key, host: &mut dyn Host) -> Outcome {
        let mut out = Outcome {
            handled: true,
            ..Outcome::default()
        };
        if self.command_line.is_some() {
            self.command_line_key(doc, key, host, &mut out);
            if self.mode == Mode::Normal {
                self.clamp(doc);
            }
            return out;
        }
        match self.mode {
            Mode::Insert => {
                if matches!(key, Key::Esc | Key::Ctrl('[')) {
                    self.leave_insert(doc);
                } else {
                    out.handled = false;
                }
            }
            Mode::Replace => self.replace_key(doc, key),
            Mode::Normal | Mode::Visual | Mode::VisualLine | Mode::VisualBlock
                if self.idle() && self.leader == Some(key) =>
            {
                // The leader starts a keymap sequence.
                out.handled = false;
            }
            Mode::Normal | Mode::Visual | Mode::VisualLine | Mode::VisualBlock => {
                // Visual mode ends when the selection changed elsewhere (a
                // click).
                if self.visual() && doc.selection != self.visual_selection(doc) {
                    self.mode = Mode::Normal;
                }
                if !self.visual() {
                    self.cursor = doc.selection.head.min(doc.text().len());
                }
                self.keys.push(key);
                self.command(doc, key, host, &mut out);
                if self.idle() {
                    self.keys.clear();
                }
                if self.visual() {
                    doc.selection = self.visual_selection(doc);
                } else if self.mode == Mode::Normal {
                    self.clamp(doc);
                }
            }
        }
        out
    }

    fn idle(&self) -> bool {
        self.pending == Pending::None
            && self.op.is_none()
            && self.count.is_none()
            && self.register.is_none()
            && self.command_line.is_none()
    }

    fn visual(&self) -> bool {
        matches!(
            self.mode,
            Mode::Visual | Mode::VisualLine | Mode::VisualBlock
        )
    }

    /// The block's lines and columns (left inclusive, right exclusive).
    fn block(&self, doc: &DocumentState) -> (usize, usize, usize, usize) {
        let (a, c) = (self.anchor, self.cursor);
        let (la, lc) = (line_of(doc, a), line_of(doc, c));
        let (ca, cc) = (column(doc, a), column(doc, c));
        (la.min(lc), la.max(lc), ca.min(cc), ca.max(cc) + 1)
    }

    /// The parts of each line the block selection covers, for the
    /// frontends to paint as selected; `None` outside block selection.
    pub fn block_ranges(&self, doc: &DocumentState) -> Option<Vec<Range<usize>>> {
        if self.mode != Mode::VisualBlock {
            return None;
        }
        let (first, last, left, right) = self.block(doc);
        Some(
            (first..=last.min(last_line(doc)))
                .map(|l| at_column(doc, l, left)..at_column(doc, l, right))
                .collect(),
        )
    }

    /// In normal mode the cursor is on a character, not after the last.
    fn clamp(&mut self, doc: &mut DocumentState) {
        let pos = doc.selection.head.min(doc.text().len());
        let line = line_of(doc, pos);
        let (s, e) = (line_start(doc, line), line_end(doc, line));
        let p = if pos >= e && e > s {
            doc.grapheme_before(e)
        } else {
            pos.clamp(s, e.max(s))
        };
        doc.selection = Selection::caret(p);
        self.cursor = p;
    }

    fn visual_selection(&self, doc: &DocumentState) -> Selection {
        let (a, c) = (self.anchor, self.cursor);
        // The block is painted from `block_ranges`; the cursor is a caret.
        if self.mode == Mode::VisualBlock {
            return Selection::caret(c);
        }
        if self.mode == Mode::VisualLine {
            let (l1, l2) = (line_of(doc, a.min(c)), line_of(doc, a.max(c)));
            let (s, e) = (line_start(doc, l1), line_end(doc, l2));
            return if c >= a {
                Selection { anchor: s, head: e }
            } else {
                Selection { anchor: e, head: s }
            };
        }
        if c >= a {
            Selection {
                anchor: a,
                head: doc.grapheme_after(c).max(c),
            }
        } else {
            Selection {
                anchor: doc.grapheme_after(a).max(a),
                head: c,
            }
        }
    }

    /// The target of the visual selection.
    fn visual_target(&self, doc: &DocumentState) -> Target {
        let sel = self.visual_selection(doc);
        let (s, e) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
        if self.mode == Mode::VisualBlock {
            let (first, last, left, right) = self.block(doc);
            Target::Block {
                first,
                last,
                left,
                right,
            }
        } else if self.mode == Mode::VisualLine {
            Target::Lines(line_of(doc, s), line_of(doc, e))
        } else {
            Target::Chars(s..e)
        }
    }

    /// The motion of `key`, `Some(None)` for a motion that fails, `None`
    /// if `key` is not a motion.
    fn motion(
        &mut self,
        doc: &DocumentState,
        key: Key,
        count: Option<usize>,
        host: &dyn Host,
    ) -> Option<Option<Motion>> {
        let n = count.unwrap_or(1).max(1);
        let pos = self.cursor;
        let line = line_of(doc, pos);
        let last = last_line(doc);
        let charwise = |to: usize| {
            Some(Motion {
                to,
                linewise: false,
                inclusive: false,
            })
        };
        let inclusive = |to: usize| {
            Some(Motion {
                to,
                linewise: false,
                inclusive: true,
            })
        };
        let to_line = |l: usize, goal: Option<usize>| {
            let l = l.min(last);
            let to = match goal {
                Some(c) => at_column(doc, l, c),
                None => first_non_blank(doc, l),
            };
            Some(Motion {
                to,
                linewise: true,
                inclusive: false,
            })
        };
        let vertical = matches!(
            key,
            Key::Char('j' | 'k') | Key::Down | Key::Up | Key::Ctrl('n' | 'p')
        );
        let m = match key {
            Key::Char('h') | Key::Left | Key::Backspace => {
                let s = line_start(doc, line);
                let rich = host.rich_view() && self.op.is_none();
                let mut p = pos;
                for _ in 0..n {
                    if p > s {
                        p = step(doc, p, false, rich).max(s);
                    }
                }
                charwise(p)
            }
            Key::Char('l' | ' ') | Key::Right => {
                let e = line_end(doc, line);
                // An operator acts on the characters themselves (`dl` is
                // the one under the cursor, not the hidden text after it).
                let rich = host.rich_view() && self.op.is_none();
                let mut p = pos;
                for _ in 0..n {
                    if p < e {
                        p = step(doc, p, true, rich).min(e);
                    }
                }
                // The cursor stays on the last character; an operator gets
                // to the end of the line.
                if self.op.is_none() && p >= e && e > line_start(doc, line) {
                    p = step(doc, e, false, rich);
                }
                charwise(p)
            }
            Key::Char('j' | 'k') | Key::Down | Key::Up | Key::Ctrl('n' | 'p') => {
                let down = matches!(key, Key::Char('j') | Key::Down | Key::Ctrl('n'));
                let goal = *self.goal.get_or_insert_with(|| column(doc, pos));
                let l = if down {
                    (line + n).min(last)
                } else {
                    line.saturating_sub(n)
                };
                to_line(l, Some(goal))
            }
            Key::Ctrl('d' | 'u' | 'f' | 'b') => {
                let page = host.page_lines().max(2);
                let step = if matches!(key, Key::Ctrl('d' | 'u')) {
                    page / 2
                } else {
                    page - 2
                } * n;
                let down = matches!(key, Key::Ctrl('d' | 'f'));
                let l = if down {
                    line + step
                } else {
                    line.saturating_sub(step)
                };
                to_line(l, None)
            }
            Key::Char('+') | Key::Enter => to_line(line + n, None),
            Key::Char('-') => to_line(line.saturating_sub(n), None),
            Key::Char('_') => to_line(line + n - 1, None),
            Key::Char('H') => to_line(line.saturating_sub(host.page_lines() / 2), None),
            Key::Char('L') => to_line(line + host.page_lines() / 2, None),
            Key::Char('M') => to_line(line, None),
            Key::Char('0') => charwise(line_start(doc, line)),
            Key::Char('^') => charwise(first_non_blank(doc, line)),
            Key::Char('$') => {
                let l = (line + n - 1).min(last);
                let (s, e) = (line_start(doc, l), line_end(doc, l));
                if self.op.is_some() {
                    charwise(e)
                } else {
                    inclusive(if e > s { doc.grapheme_before(e) } else { e })
                }
            }
            Key::Char('G') => to_line(count.map_or(last, |c| c.saturating_sub(1)), None),
            Key::Char('w' | 'W') => {
                let big = key == Key::Char('W');
                let mut p = pos;
                for i in 0..n {
                    p = word_forward(doc, p, big, i + 1 == n && self.op.is_some());
                }
                charwise(p)
            }
            Key::Char('b' | 'B') => {
                let mut p = pos;
                for _ in 0..n {
                    p = word_back(doc, p, key == Key::Char('B'));
                }
                charwise(p)
            }
            Key::Char('e' | 'E') => {
                let mut p = pos;
                for _ in 0..n {
                    p = word_end(doc, p, key == Key::Char('E'));
                }
                inclusive(p)
            }
            Key::Char(';' | ',') => {
                let Some((kind, c)) = self.last_find else {
                    return Some(None);
                };
                let kind = if key == Key::Char(',') {
                    match kind {
                        Find::To => Find::Back,
                        Find::Till => Find::BackTill,
                        Find::Back => Find::To,
                        Find::BackTill => Find::Till,
                    }
                } else {
                    kind
                };
                find_char(doc, pos, kind, c, n).map(|to| Motion {
                    to,
                    linewise: false,
                    inclusive: matches!(kind, Find::To | Find::Till),
                })
            }
            Key::Char('%') => matching_bracket(doc, pos).map(|to| Motion {
                to,
                linewise: false,
                inclusive: true,
            }),
            Key::Char('}') => charwise(paragraph(doc, pos, true, n)),
            Key::Char('{') => charwise(paragraph(doc, pos, false, n)),
            Key::Char('n' | 'N') => {
                let Some((pat, back)) = self.last_search.clone() else {
                    return Some(None);
                };
                let back = back != (key == Key::Char('N'));
                search_from(doc, &pat, back, pos, n).map(|to| Motion {
                    to,
                    linewise: false,
                    inclusive: false,
                })
            }
            _ => return None,
        };
        if !vertical {
            self.goal = None;
        }
        self.motion_count = n;
        Some(m)
    }

    // Registers.

    fn store(&mut self, text: String, linewise: bool, host: &mut dyn Host) {
        let r = Register { text, linewise };
        match self.register.take() {
            Some('+' | '*') => host.set_clipboard(&r.text),
            Some(c) if c.is_ascii_uppercase() => {
                let e = self
                    .registers
                    .entry(c.to_ascii_lowercase())
                    .or_insert(Register {
                        text: String::new(),
                        linewise,
                    });
                e.text.push_str(&r.text);
            }
            Some(c) if c.is_ascii_lowercase() => {
                self.registers.insert(c, r.clone());
            }
            _ => {}
        }
        self.registers.insert('"', r);
    }

    fn fetch(&mut self, host: &mut dyn Host) -> Option<Register> {
        match self.register.take() {
            Some('+' | '*') => host.clipboard().map(|text| {
                let linewise = text.ends_with('\n');
                Register { text, linewise }
            }),
            Some(c) => self.registers.get(&c.to_ascii_lowercase()).cloned(),
            None => self.registers.get(&'"').cloned(),
        }
    }

    // Insert mode.

    fn enter_insert(&mut self, doc: &mut DocumentState, at: usize) {
        doc.selection = Selection::caret(at);
        doc.break_undo_group();
        self.mode = Mode::Insert;
        self.insert_at = Some(at);
        self.cursor = at;
    }

    fn leave_insert(&mut self, doc: &mut DocumentState) {
        self.mode = Mode::Normal;
        let head = doc.selection.head.min(doc.text().len());
        if let (Some((lines, col, pad)), Some(at)) = (self.block_insert.take(), self.insert_at)
            && head > at
            && !doc.text().as_str()[at..head].contains('\n')
        {
            // The typed text again on the block's other lines.
            let typed = doc.text().as_str()[at..head].to_string();
            let mut tx = Transaction::new("Vim");
            for l in lines.filter(|l| *l <= last_line(doc)) {
                let e = line_end(doc, l);
                let width = doc.text().as_str()[line_start(doc, l)..e].chars().count();
                if width < col {
                    if pad {
                        let fill = " ".repeat(col - width);
                        let _ = tx.insert(e, format!("{fill}{typed}"));
                    }
                    continue;
                }
                let _ = tx.insert(at_column(doc, l, col), typed.clone());
            }
            let tx = tx.select(Selection::caret(head));
            doc.apply(&tx, ChangeKind::Command, Instant::now());
        }
        if let (Some(mut c), Some(at)) = (self.recording.take(), self.insert_at) {
            if head >= at {
                c.inserted = Some(doc.text().as_str()[at..head].to_string());
            }
            if !self.replaying {
                self.last_change = Some(c);
            }
        }
        self.insert_at = None;
        doc.break_undo_group();
        if head > line_start(doc, line_of(doc, head)) {
            doc.selection = Selection::caret(doc.grapheme_before(head));
        }
        self.clamp(doc);
    }

    // Operators.

    /// Applies `op` to `target`.
    fn apply_op(
        &mut self,
        doc: &mut DocumentState,
        op: Op,
        target: Target,
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        let len = doc.text().len();
        match target {
            Target::Lines(l1, l2) => {
                let lines: String = (l1..=l2)
                    .map(|l| {
                        let r = doc.text().line_range(l);
                        let s = &doc.text().as_str()[r];
                        format!("{}\n", s.strip_suffix('\r').unwrap_or(s))
                    })
                    .collect();
                match op {
                    Op::Yank => {
                        self.store(lines, true, host);
                        let caret = if line_of(doc, self.cursor) == l1 {
                            self.cursor
                        } else {
                            first_non_blank(doc, l1)
                        };
                        doc.selection = Selection::caret(caret);
                    }
                    Op::Delete => {
                        self.store(lines, true, host);
                        let span = line_span(doc, l1, l2);
                        edit(doc, span.clone(), "", span.start);
                        let l = l1.min(last_line(doc));
                        doc.selection = Selection::caret(first_non_blank(doc, l));
                    }
                    Op::Change => {
                        self.store(lines, true, host);
                        // The first line's indentation stays.
                        let s = first_non_blank(doc, l1);
                        let e = line_end(doc, l2);
                        edit(doc, s..e, "", s);
                        self.enter_insert(doc, s);
                    }
                    Op::Indent | Op::Outdent => {
                        self.shift_lines(doc, l1, l2, op == Op::Indent, out)
                    }
                    Op::Lower | Op::Upper | Op::Toggle => {
                        let r = line_start(doc, l1)..line_end(doc, l2);
                        self.change_case(doc, op, r);
                        doc.selection = Selection::caret(first_non_blank(doc, l1));
                    }
                }
            }
            Target::Block {
                first,
                last,
                left,
                right,
            } => {
                let last = last.min(last_line(doc));
                let ranges: Vec<Range<usize>> = (first..=last)
                    .map(|l| at_column(doc, l, left)..at_column(doc, l, right))
                    .collect();
                let top = ranges[0].start;
                let text: Vec<&str> = ranges
                    .iter()
                    .map(|r| &doc.text().as_str()[r.clone()])
                    .collect();
                let text = text.join("\n");
                match op {
                    Op::Yank => {
                        self.store(text, false, host);
                        doc.selection = Selection::caret(top);
                    }
                    Op::Delete | Op::Change => {
                        self.store(text, false, host);
                        let mut tx = Transaction::new("Vim");
                        for r in &ranges {
                            let _ = tx.delete(r.clone());
                        }
                        let tx = tx.select(Selection::caret(top));
                        doc.apply(&tx, ChangeKind::Command, Instant::now());
                        if op == Op::Change {
                            self.enter_insert(doc, top);
                            self.block_insert = Some((first + 1..last + 1, left, false));
                        }
                    }
                    Op::Indent | Op::Outdent => {
                        self.shift_lines(doc, first, last, op == Op::Indent, out)
                    }
                    Op::Lower | Op::Upper | Op::Toggle => {
                        for r in ranges.into_iter().rev() {
                            self.change_case(doc, op, r);
                        }
                        doc.selection = Selection::caret(top);
                    }
                }
            }
            Target::Chars(r) => {
                let r = r.start.min(len)..r.end.min(len);
                let text = doc.text().as_str()[r.clone()].to_string();
                match op {
                    Op::Yank => {
                        self.store(text, false, host);
                        doc.selection = Selection::caret(r.start);
                    }
                    Op::Delete | Op::Change => {
                        self.store(text, false, host);
                        edit(doc, r.clone(), "", r.start);
                        if op == Op::Change {
                            self.enter_insert(doc, r.start);
                        }
                    }
                    Op::Indent | Op::Outdent => {
                        let (l1, l2) = (
                            line_of(doc, r.start),
                            line_of(doc, r.end.saturating_sub(1).max(r.start)),
                        );
                        self.shift_lines(doc, l1, l2, op == Op::Indent, out);
                    }
                    Op::Lower | Op::Upper | Op::Toggle => {
                        self.change_case(doc, op, r.clone());
                        doc.selection = Selection::caret(r.start);
                    }
                }
            }
        }
    }

    fn change_case(&mut self, doc: &mut DocumentState, op: Op, r: Range<usize>) {
        let s = doc.text().as_str()[r.clone()].to_string();
        let new: String = match op {
            Op::Lower => s.to_lowercase(),
            Op::Upper => s.to_uppercase(),
            _ => s
                .chars()
                .flat_map(|c| {
                    if c.is_uppercase() {
                        c.to_lowercase().collect::<Vec<_>>()
                    } else {
                        c.to_uppercase().collect::<Vec<_>>()
                    }
                })
                .collect(),
        };
        if new != s {
            edit(doc, r.clone(), &new, r.start);
        }
    }

    /// `>>` and `<<`: in Org documents headlines and list items as Org
    /// does it, other lines by two spaces.
    fn shift_lines(
        &mut self,
        doc: &mut DocumentState,
        l1: usize,
        l2: usize,
        right: bool,
        out: &mut Outcome,
    ) {
        let org = doc.meta.mode == DocumentMode::Org;
        let now = Instant::now();
        for l in (l1..=l2).rev() {
            let (s, e) = (line_start(doc, l), line_end(doc, l));
            let line = doc.text().as_str()[s..e].to_string();
            let stars = line.len() - line.trim_start_matches('*').len();
            let heading = org && stars > 0 && line[stars..].starts_with(' ');
            let t = line.trim_start();
            let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            let item = org
                && !heading
                && (t.starts_with("- ")
                    || t.starts_with("+ ")
                    || (t.starts_with("* ") && t.len() < line.len())
                    || (digits > 0
                        && (t[digits..].starts_with(". ") || t[digits..].starts_with(") "))));
            doc.selection = Selection::caret(s + (line.len() - t.len()));
            let result = if heading {
                doc.run(now, |d, p, _| {
                    let text = d.parse().syntax().to_string();
                    if right {
                        org_edit::headline::demote(&text, p, d.parse().context())
                    } else {
                        org_edit::headline::promote(&text, p, d.parse().context())
                    }
                })
            } else if item {
                doc.run(now, |d, p, _| {
                    org_edit::list::indent_item(d, p, None, right, true)
                })
            } else if right {
                if !line.is_empty() {
                    edit(doc, s..s, "  ", s);
                }
                Ok(())
            } else {
                let n = line.bytes().take(2).take_while(|b| *b == b' ').count();
                if n > 0 {
                    edit(doc, s..s + n, "", s);
                }
                Ok(())
            };
            if let Err(e) = result {
                out.message = Some((e.message, true));
            }
        }
        doc.selection = Selection::caret(first_non_blank(doc, l1.min(last_line(doc))));
    }

    fn put(&mut self, doc: &mut DocumentState, before: bool, count: usize, host: &mut dyn Host) {
        let Some(r) = self.fetch(host) else { return };
        let pos = self.cursor;
        let line = line_of(doc, pos);
        if r.linewise {
            let mut t = r.text.clone();
            if !t.ends_with('\n') {
                t.push('\n');
            }
            let t = t.repeat(count);
            let at = if before {
                line_start(doc, line)
            } else if line + 1 < doc.text().line_count() {
                line_start(doc, line + 1)
            } else {
                // After a last line without a line feed.
                let len = doc.text().len();
                let body = t.strip_suffix('\n').unwrap_or(&t).to_string();
                edit(doc, len..len, &format!("\n{body}"), len + 1);
                let l = line_of(doc, len + 1);
                doc.selection = Selection::caret(first_non_blank(doc, l));
                return;
            };
            edit(doc, at..at, &t, at);
            doc.selection = Selection::caret(first_non_blank(doc, line_of(doc, at)));
        } else {
            let t = r.text.repeat(count);
            let e = line_end(doc, line);
            let at = if before || pos >= e {
                pos
            } else {
                doc.grapheme_after(pos)
            };
            let end = at + t.len();
            edit(doc, at..at, &t, end);
            doc.selection = Selection::caret(doc.grapheme_before(end).max(at));
        }
    }

    fn join(&mut self, doc: &mut DocumentState, line: usize, count: usize) {
        for _ in 0..count.max(2) - 1 {
            if line >= last_line(doc) {
                break;
            }
            let e = line_end(doc, line);
            let next = first_non_blank(doc, line + 1);
            let next_empty = next >= line_end(doc, line + 1);
            let this_empty = e == line_start(doc, line);
            let sep = if next_empty || this_empty || doc.text().as_str()[..e].ends_with(' ') {
                ""
            } else {
                " "
            };
            edit(doc, e..next, sep, e);
        }
    }

    // The command parser.

    fn command(
        &mut self,
        doc: &mut DocumentState,
        key: Key,
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        match self.pending {
            Pending::Register => {
                self.pending = Pending::None;
                if let Key::Char(c) = key
                    && (c.is_ascii_alphabetic() || matches!(c, '"' | '+' | '*'))
                {
                    self.register = Some(c);
                }
                return;
            }
            Pending::Find(kind) => {
                self.pending = Pending::None;
                let Key::Char(c) = key else {
                    return self.reset();
                };
                self.last_find = Some((kind, c));
                let n = self.count.take().unwrap_or(1) * self.op.map_or(1, |o| o.1.max(1));
                self.motion_count = n;
                let m = find_char(doc, self.cursor, kind, c, n).map(|to| Motion {
                    to,
                    linewise: false,
                    inclusive: matches!(kind, Find::To | Find::Till),
                });
                return self.finish_motion(doc, m, host, out);
            }
            Pending::Replace => {
                self.pending = Pending::None;
                let Key::Char(c) = key else {
                    return self.reset();
                };
                let n = self.count.take().unwrap_or(1);
                let pos = self.cursor;
                let e = line_end(doc, line_of(doc, pos));
                let mut end = pos;
                for _ in 0..n {
                    if end >= e {
                        return self.reset();
                    }
                    end = doc.grapheme_after(end);
                }
                let s: String = std::iter::repeat_n(c, n).collect();
                edit(doc, pos..end, &s, pos + s.len() - c.len_utf8());
                self.changed();
                return;
            }
            Pending::G => {
                self.pending = Pending::None;
                match key {
                    Key::Char('g') => {
                        let n = self.count.take();
                        let l = n.map_or(0, |c| c.saturating_sub(1)).min(last_line(doc));
                        let m = Some(Motion {
                            to: first_non_blank(doc, l),
                            linewise: true,
                            inclusive: false,
                        });
                        self.finish_motion(doc, m, host, out);
                    }
                    // `gt` and `gT`: the next and previous document, as
                    // Vim's tabs.
                    Key::Char('t') if !self.visual() && self.op.is_none() => {
                        self.count = None;
                        out.commands.push(("file.next".into(), Value::Null));
                    }
                    Key::Char('T') if !self.visual() && self.op.is_none() => {
                        self.count = None;
                        out.commands.push(("file.previous".into(), Value::Null));
                    }
                    Key::Char(c @ ('u' | 'U' | '~')) => {
                        let op = match c {
                            'u' => Op::Lower,
                            'U' => Op::Upper,
                            _ => Op::Toggle,
                        };
                        if self.visual() {
                            let t = self.visual_target(doc);
                            self.mode = Mode::Normal;
                            self.apply_op(doc, op, t, host, out);
                        } else {
                            self.operator(doc, op, host, out);
                        }
                    }
                    _ => self.reset(),
                }
                return;
            }
            Pending::Object(inner) => {
                self.pending = Pending::None;
                let Key::Char(c) = key else {
                    return self.reset();
                };
                if "hRice".contains(c) {
                    doc.wait_for_parse();
                }
                let Some(t) = object(doc, self.cursor, c, inner) else {
                    return self.reset();
                };
                if self.visual() {
                    match t {
                        Target::Chars(r) => {
                            self.anchor = r.start;
                            self.cursor = doc.grapheme_before(r.end).max(r.start);
                        }
                        Target::Lines(a, b) => {
                            self.mode = Mode::VisualLine;
                            self.anchor = line_start(doc, a);
                            self.cursor = line_start(doc, b);
                        }
                        // Text objects give characters or lines.
                        Target::Block { .. } => {}
                    }
                    return;
                }
                if let Some((op, _)) = self.op.take() {
                    self.count = None;
                    self.begin_change_if(op);
                    self.apply_op(doc, op, t, host, out);
                    self.changed_if(op);
                }
                return;
            }
            Pending::None => {}
        }
        // A count.
        if let Key::Char(c @ '0'..='9') = key
            && (c != '0' || self.count.is_some())
        {
            let d = c as usize - '0' as usize;
            self.count = Some(self.count.unwrap_or(0).saturating_mul(10).saturating_add(d));
            return;
        }
        if matches!(key, Key::Esc | Key::Ctrl('[')) {
            if self.visual() {
                self.mode = Mode::Normal;
                doc.selection = Selection::caret(self.cursor);
            }
            return self.reset();
        }
        // Motions.
        let count = match (self.count, self.op) {
            (Some(c), Some((_, oc))) => Some(c * oc.max(1)),
            (None, Some((_, oc))) if oc > 0 => Some(oc),
            (c, _) => c,
        };
        if let Some(m) = self.motion(doc, key, count, host) {
            self.count = None;
            return self.finish_motion(doc, m, host, out);
        }
        // Other commands forget the column vertical motion keeps.
        self.goal = None;
        let Key::Char(c) = key else {
            match key {
                Key::Ctrl('r') if !self.visual() => {
                    for _ in 0..self.count.take().unwrap_or(1) {
                        if doc.redo().is_none() {
                            break;
                        }
                    }
                }
                Key::Ctrl('v') if self.mode == Mode::VisualBlock => {
                    self.mode = Mode::Normal;
                    doc.selection = Selection::caret(self.cursor);
                }
                Key::Ctrl('v') if self.visual() => self.mode = Mode::VisualBlock,
                Key::Ctrl('v') => self.start_visual(Mode::VisualBlock),
                // Not a Vim key: the keymap may have it.
                _ => out.handled = false,
            }
            return self.reset();
        };
        // Operators.
        if let Some(op) = Op::of(c) {
            if self.visual() {
                let t = self.visual_target(doc);
                self.mode = Mode::Normal;
                self.apply_op(doc, op, t, host, out);
                return self.reset();
            }
            return self.operator(doc, op, host, out);
        }
        if self.visual() {
            return self.visual_key(doc, c, host, out);
        }
        if let Some((op, _)) = self.op {
            match c {
                'i' | 'a' => self.pending = Pending::Object(c == 'i'),
                'f' | 't' | 'F' | 'T' => self.pending = Pending::Find(find_kind(c)),
                'g' => self.pending = Pending::G,
                '/' | '?' => self.command_line = Some(c.to_string()),
                // `guu`, `gUU`, `g~~`.
                'u' if op == Op::Lower => self.operator(doc, op, host, out),
                'U' if op == Op::Upper => self.operator(doc, op, host, out),
                '~' if op == Op::Toggle => self.operator(doc, op, host, out),
                _ => self.reset(),
            }
            return;
        }
        let count = self.count.take();
        let n = count.unwrap_or(1);
        let pos = self.cursor;
        let line = line_of(doc, pos);
        match c {
            '"' => {
                self.count = count;
                self.pending = Pending::Register;
            }
            'g' => {
                self.count = count;
                self.pending = Pending::G;
            }
            'f' | 't' | 'F' | 'T' => {
                self.count = count;
                self.pending = Pending::Find(find_kind(c));
            }
            'r' => {
                self.count = count;
                self.pending = Pending::Replace;
            }
            ':' | '/' | '?' => self.command_line = Some(c.to_string()),
            'x' | 'X' | 's' | 'D' | 'C' | 'S' | 'Y' => {
                // Shorthands for an operator and a motion.
                let seq = match c {
                    'x' => "dl",
                    'X' => "dh",
                    's' => "cl",
                    'D' => "d$",
                    'C' => "c$",
                    'S' => "cc",
                    _ => "yy",
                };
                let keys = std::mem::take(&mut self.keys);
                self.count = count;
                for k in seq.chars() {
                    self.command(doc, Key::Char(k), host, out);
                }
                if !self.replaying && c != 'Y' {
                    if let Some(ch) = &mut self.recording {
                        ch.keys = keys;
                    } else if let Some(ch) = &mut self.last_change {
                        ch.keys = keys;
                    }
                }
            }
            'p' | 'P' => {
                self.put(doc, c == 'P', n, host);
                self.changed();
            }
            'J' => {
                self.join(doc, line, n);
                self.changed();
            }
            '~' => {
                let e = line_end(doc, line);
                let mut end = pos;
                for _ in 0..n {
                    if end < e {
                        end = doc.grapheme_after(end);
                    }
                }
                if end > pos {
                    self.change_case(doc, Op::Toggle, pos..end);
                    doc.selection = Selection::caret(end.min(e));
                    self.changed();
                }
            }
            'u' => {
                for _ in 0..n {
                    if doc.undo().is_none() {
                        out.message = Some((crate::l10n::tr("msg-nothing-to-undo"), false));
                        break;
                    }
                }
            }
            '.' => self.repeat(doc, count, host, out),
            'i' => {
                self.begin_change();
                self.enter_insert(doc, pos);
            }
            'a' => {
                self.begin_change();
                let e = line_end(doc, line);
                self.enter_insert(
                    doc,
                    if pos < e {
                        doc.grapheme_after(pos)
                    } else {
                        pos
                    },
                );
            }
            'I' => {
                self.begin_change();
                self.enter_insert(doc, first_non_blank(doc, line));
            }
            'A' => {
                self.begin_change();
                self.enter_insert(doc, line_end(doc, line));
            }
            'o' | 'O' => {
                self.begin_change();
                let s = line_start(doc, line);
                let indent = doc.text().as_str()[s..first_non_blank(doc, line)].to_string();
                if c == 'o' {
                    let e = line_end(doc, line);
                    let at = e + 1 + indent.len();
                    edit(doc, e..e, &format!("\n{indent}"), at);
                    self.enter_insert(doc, at);
                } else {
                    edit(doc, s..s, &format!("{indent}\n"), s + indent.len());
                    self.enter_insert(doc, s + indent.len());
                }
            }
            'R' => {
                self.begin_change();
                if let Some(r) = &mut self.recording {
                    r.inserted = Some(String::new());
                }
                self.mode = Mode::Replace;
                doc.break_undo_group();
            }
            'v' => self.start_visual(Mode::Visual),
            'V' => self.start_visual(Mode::VisualLine),
            '*' | '#' => {
                let Some(Target::Chars(w)) = object(doc, pos, 'w', true) else {
                    return;
                };
                let word = doc.text().as_str()[w].to_string();
                if !word.chars().any(is_word) {
                    return;
                }
                let pat = format!(r"\b{}\b", regex_escape(&word));
                let back = c == '#';
                self.last_search = Some((pat.clone(), back));
                if let Some(to) = search_from(doc, &pat, back, pos, n) {
                    doc.selection = Selection::caret(to);
                }
                let opts = crate::find::FindOptions { regex: true };
                out.highlights = crate::find::find_with(doc.text().as_str(), &pat, opts).ok();
            }
            // An unused character does nothing (and is not typed).
            _ => self.reset(),
        }
    }

    fn start_visual(&mut self, mode: Mode) {
        self.mode = mode;
        self.anchor = self.cursor;
        self.reset();
    }

    fn visual_key(
        &mut self,
        doc: &mut DocumentState,
        c: char,
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        let target = self.visual_target(doc);
        let lines = match &target {
            Target::Lines(a, b) => Target::Lines(*a, *b),
            Target::Block { first, last, .. } => Target::Lines(*first, *last),
            Target::Chars(r) => Target::Lines(
                line_of(doc, r.start),
                line_of(doc, r.end.saturating_sub(1).max(r.start)),
            ),
        };
        let start = match &target {
            Target::Chars(r) => r.start,
            Target::Lines(a, _) => line_start(doc, *a),
            Target::Block { first, left, .. } => at_column(doc, *first, *left),
        };
        // In a block, `I` and `A` type on every line; `c` and `s` change it.
        if let Target::Block {
            first,
            last,
            left,
            right,
        } = target
        {
            match c {
                'I' | 'A' => {
                    self.mode = Mode::Normal;
                    let (col, pad) = if c == 'I' {
                        (left, false)
                    } else {
                        (right, true)
                    };
                    let width = |l: usize| {
                        doc.text().as_str()[line_start(doc, l)..line_end(doc, l)]
                            .chars()
                            .count()
                    };
                    self.begin_change();
                    let mut at = at_column(doc, first, col);
                    if pad && width(first) < col {
                        let e = line_end(doc, first);
                        let fill = " ".repeat(col - width(first));
                        edit(doc, e..e, &fill, e + fill.len());
                        at = e + fill.len();
                    }
                    self.enter_insert(doc, at);
                    self.block_insert = Some((first + 1..last + 1, col, pad));
                    return;
                }
                'c' | 's' => {
                    self.mode = Mode::Normal;
                    self.begin_change();
                    return self.apply_op(doc, Op::Change, target, host, out);
                }
                'o' => {
                    std::mem::swap(&mut self.anchor, &mut self.cursor);
                    return;
                }
                _ => {}
            }
        }
        let was_lines = self.mode == Mode::VisualLine;
        let finish = |v: &mut Vim| {
            v.mode = Mode::Normal;
            v.cursor = start;
        };
        match c {
            'v' | 'V' => {
                let m = if c == 'v' {
                    Mode::Visual
                } else {
                    Mode::VisualLine
                };
                if self.mode == m {
                    self.mode = Mode::Normal;
                    doc.selection = Selection::caret(self.cursor);
                } else {
                    self.mode = m;
                }
            }
            'o' => std::mem::swap(&mut self.anchor, &mut self.cursor),
            'i' | 'a' => self.pending = Pending::Object(c == 'i'),
            'g' => self.pending = Pending::G,
            'f' | 't' | 'F' | 'T' => self.pending = Pending::Find(find_kind(c)),
            '"' => self.pending = Pending::Register,
            'x' => {
                finish(self);
                self.apply_op(doc, Op::Delete, target, host, out);
            }
            's' => {
                finish(self);
                self.apply_op(doc, Op::Change, target, host, out);
            }
            'X' | 'D' => {
                finish(self);
                self.apply_op(doc, Op::Delete, lines, host, out);
            }
            'S' | 'C' => {
                finish(self);
                self.apply_op(doc, Op::Change, lines, host, out);
            }
            'Y' => {
                finish(self);
                self.apply_op(doc, Op::Yank, lines, host, out);
            }
            'u' | 'U' | '~' => {
                finish(self);
                let op = match c {
                    'u' => Op::Lower,
                    'U' => Op::Upper,
                    _ => Op::Toggle,
                };
                self.apply_op(doc, op, target, host, out);
            }
            'J' => {
                finish(self);
                if let Target::Lines(a, b) = lines {
                    self.join(doc, a, b - a + 1);
                }
            }
            'p' | 'P' => {
                finish(self);
                let reg = self.fetch(host);
                let range = match target {
                    Target::Chars(r) => r,
                    Target::Lines(a, b) => line_start(doc, a)..line_end(doc, b),
                    Target::Block { first, last, .. } => {
                        line_start(doc, first)..line_end(doc, last)
                    }
                };
                if let Some(r) = reg {
                    let t = if was_lines || !r.linewise {
                        r.text.trim_end_matches('\n').to_string()
                    } else {
                        r.text
                    };
                    let at = range.start;
                    edit(doc, range, &t, at);
                }
            }
            ':' => self.command_line = Some(":'<,'>".into()),
            _ => {}
        }
    }

    fn operator(
        &mut self,
        doc: &mut DocumentState,
        op: Op,
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        match self.op {
            // Doubled: whole lines (`dd`, `>>`, `gUU`).
            Some((o, oc)) if o == op => {
                self.op = None;
                let n = self.count.take().unwrap_or(1) * oc.max(1);
                let l1 = line_of(doc, self.cursor);
                let l2 = (l1 + n - 1).min(last_line(doc));
                self.begin_change_if(op);
                self.apply_op(doc, op, Target::Lines(l1, l2), host, out);
                self.changed_if(op);
            }
            Some(_) => self.reset(),
            None => {
                let c = self.count.take().unwrap_or(0);
                self.op = Some((op, c));
            }
        }
    }

    fn finish_motion(
        &mut self,
        doc: &mut DocumentState,
        m: Option<Motion>,
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        let Some(m) = m else {
            return self.reset();
        };
        if self.visual() {
            self.cursor = m.to;
            return;
        }
        let Some((op, _)) = self.op.take() else {
            doc.selection = Selection::caret(m.to);
            self.cursor = m.to;
            return;
        };
        self.goal = None;
        let from = self.cursor;
        let (s, mut e) = (from.min(m.to), from.max(m.to));
        if m.linewise {
            let target = Target::Lines(line_of(doc, s), line_of(doc, e));
            self.begin_change_if(op);
            self.apply_op(doc, op, target, host, out);
            self.changed_if(op);
            return;
        }
        if m.inclusive {
            e = doc.grapheme_after(e).max(e);
        } else if e > s
            && line_of(doc, e) > line_of(doc, s)
            && e == line_start(doc, line_of(doc, e))
        {
            // An exclusive motion to the start of a line stops at the end of
            // the line before.
            e = line_end(doc, line_of(doc, e) - 1).max(s);
        }
        // `cw` changes to the end of the word, as `ce`.
        let word_motion = self
            .keys
            .last()
            .is_some_and(|k| matches!(k, Key::Char('w' | 'W')));
        if op == Op::Change && word_motion && char_at(doc, s).is_some_and(|c| !c.is_whitespace()) {
            let big = self.keys.last() == Some(&Key::Char('W'));
            let mut p = word_end_from(doc, s, big);
            for _ in 1..self.motion_count {
                p = word_end(doc, p, big);
            }
            e = doc.grapheme_after(p).max(p);
        }
        self.begin_change_if(op);
        self.apply_op(doc, op, Target::Chars(s..e), host, out);
        self.changed_if(op);
    }

    // Repeat.

    fn begin_change(&mut self) {
        if self.replaying {
            return;
        }
        self.recording = Some(Change {
            keys: self.keys.clone(),
            inserted: None,
        });
    }

    fn begin_change_if(&mut self, op: Op) {
        if op != Op::Yank {
            self.begin_change();
        }
    }

    fn changed_if(&mut self, op: Op) {
        if op != Op::Yank {
            self.changed();
        }
    }

    /// A change finished in normal mode (or went on to insert mode, which
    /// finishes it at Escape).
    fn changed(&mut self) {
        if self.replaying || matches!(self.mode, Mode::Insert | Mode::Replace) {
            return;
        }
        self.last_change = Some(Change {
            keys: self.keys.clone(),
            inserted: None,
        });
        self.recording = None;
    }

    fn repeat(
        &mut self,
        doc: &mut DocumentState,
        count: Option<usize>,
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        let Some(change) = self.last_change.clone() else {
            return;
        };
        let mut keys = change.keys.clone();
        if let Some(n) = count {
            // A new count replaces the recorded one (a leading `0` is a
            // motion, not a count).
            let digits = if matches!(keys.first(), Some(Key::Char('0'))) {
                0
            } else {
                keys.iter()
                    .take_while(|k| matches!(k, Key::Char('0'..='9')))
                    .count()
            };
            keys.drain(..digits);
            keys.splice(0..0, n.to_string().chars().map(Key::Char));
        }
        self.replaying = true;
        self.keys.clear();
        for k in keys {
            if matches!(self.mode, Mode::Insert | Mode::Replace) {
                break;
            }
            self.keys.push(k);
            self.command(doc, k, host, out);
            if self.idle() {
                self.keys.clear();
            }
        }
        if self.mode == Mode::Insert {
            if let Some(t) = &change.inserted {
                doc.insert_text(t, Instant::now());
            }
            self.leave_insert(doc);
        } else if self.mode == Mode::Replace {
            if let Some(t) = change.inserted.clone() {
                for ch in t.chars() {
                    self.replace_key(doc, Key::Char(ch));
                }
            }
            self.replace_key(doc, Key::Esc);
        }
        self.replaying = false;
        self.keys.clear();
    }

    fn reset(&mut self) {
        self.count = None;
        self.op = None;
        self.register = None;
        self.pending = Pending::None;
    }

    // Replace mode.

    fn replace_key(&mut self, doc: &mut DocumentState, key: Key) {
        match key {
            Key::Esc | Key::Ctrl('[') => {
                self.mode = Mode::Normal;
                if let Some(c) = self.recording.take()
                    && !self.replaying
                {
                    self.last_change = Some(c);
                }
                doc.break_undo_group();
                let head = doc.selection.head;
                if head > line_start(doc, line_of(doc, head)) {
                    doc.selection = Selection::caret(doc.grapheme_before(head));
                }
                self.clamp(doc);
            }
            Key::Char(c) => {
                let pos = doc.selection.head;
                let e = line_end(doc, line_of(doc, pos));
                let end = if pos < e {
                    doc.grapheme_after(pos)
                } else {
                    pos
                };
                let mut tx = Transaction::new("Typing");
                tx.replace(pos..end, c.to_string()).expect("one edit");
                let tx = tx.select(Selection::caret(pos + c.len_utf8()));
                doc.apply(&tx, ChangeKind::Typing, Instant::now());
                if let Some(r) = &mut self.recording {
                    r.inserted.get_or_insert_default().push(c);
                }
            }
            Key::Backspace | Key::Left => {
                let pos = doc.selection.head;
                doc.selection = Selection::caret(doc.grapheme_before(pos));
            }
            Key::Right => {
                let pos = doc.selection.head;
                doc.selection = Selection::caret(doc.grapheme_after(pos));
            }
            _ => {}
        }
    }

    // The command line.

    fn command_line_key(
        &mut self,
        doc: &mut DocumentState,
        key: Key,
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        let Some(line) = &mut self.command_line else {
            return;
        };
        match key {
            Key::Esc | Key::Ctrl('[') => {
                self.command_line = None;
                self.reset();
            }
            Key::Backspace => {
                line.pop();
                if line.is_empty() {
                    self.command_line = None;
                    self.reset();
                }
            }
            Key::Char(c) => line.push(c),
            Key::Enter => {
                let line = self.command_line.take().unwrap_or_default();
                let (kind, rest) = line.split_at(1);
                if kind == ":" {
                    self.ex(doc, rest.trim_start_matches("'<,'>"), out);
                } else {
                    self.search(doc, rest, kind == "?", host, out);
                }
            }
            _ => {}
        }
    }

    fn search(
        &mut self,
        doc: &mut DocumentState,
        pattern: &str,
        back: bool,
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        let pattern = if pattern.is_empty() {
            match &self.last_search {
                Some((p, _)) => p.clone(),
                None => return self.reset(),
            }
        } else {
            pattern.to_string()
        };
        self.last_search = Some((pattern.clone(), back));
        let opts = crate::find::FindOptions { regex: true };
        match crate::find::find_with(doc.text().as_str(), &pattern, opts) {
            Ok(all) => {
                out.highlights = Some(all);
                match search_from(doc, &pattern, back, self.cursor, 1) {
                    Some(to) => {
                        let m = Some(Motion {
                            to,
                            linewise: false,
                            inclusive: false,
                        });
                        self.finish_motion(doc, m, host, out);
                    }
                    None => {
                        out.message = Some((crate::tr!("vim-not-found", pattern = &pattern), true));
                        self.reset();
                    }
                }
            }
            Err(e) => {
                out.message = Some((e, true));
                self.reset();
            }
        }
        self.keys.clear();
    }

    fn ex(&mut self, doc: &mut DocumentState, cmd: &str, out: &mut Outcome) {
        let cmd = cmd.trim();
        let save = || ("app.save".to_string(), Value::Null);
        let quit = || ("app.quit".to_string(), Value::Null);
        match cmd {
            "" => {}
            "w" | "write" => out.commands.push(save()),
            "q" | "quit" => out.commands.push(quit()),
            "q!" | "quit!" => out.force_quit = true,
            "wq" | "x" | "xit" | "exit" => {
                out.commands.push(save());
                out.commands.push(quit());
            }
            "noh" | "nohlsearch" => out.highlights = Some(Vec::new()),
            // Buffers: the open documents.
            "bn" | "bnext" => out.commands.push(("file.next".into(), Value::Null)),
            "bp" | "bprevious" | "bN" | "bNext" => {
                out.commands.push(("file.previous".into(), Value::Null));
            }
            "bd" | "bdelete" | "bw" | "bwipeout" => {
                out.commands.push(("file.close".into(), Value::Null));
            }
            "ls" | "buffers" | "files" | "b" | "buffer" => {
                out.commands.push(("file.switch".into(), Value::Null));
            }
            "enew" => out.commands.push(("file.new".into(), Value::Null)),
            // The file manager, as netrw's `:Explore`, and the projects.
            "E" | "Ex" | "Explore" | "Dired" => {
                out.commands.push(("dired.jump".into(), Value::Null));
            }
            "Projects" => out.commands.push(("dired.projects".into(), Value::Null)),
            _ if cmd.starts_with("E ") || cmd.starts_with("Ex ") || cmd.starts_with("Explore ") => {
                let path = cmd.split_once(' ').map_or("", |x| x.1).trim();
                out.commands
                    .push(("file.open".into(), serde_json::json!({ "path": path })));
            }
            "e" | "edit" | "e!" | "edit!" => {
                if cmd.ends_with('!') {
                    out.commands.push(("app.revert".into(), Value::Null));
                } else {
                    out.commands.push(("file.open".into(), Value::Null));
                }
            }
            _ if cmd.starts_with("e ") || cmd.starts_with("edit ") => {
                let path = cmd.split_once(' ').map_or("", |x| x.1).trim();
                out.commands
                    .push(("file.open".into(), serde_json::json!({ "path": path })));
            }
            "$" => {
                let l = last_line(doc);
                doc.selection = Selection::caret(first_non_blank(doc, l));
            }
            _ if cmd.bytes().all(|b| b.is_ascii_digit()) => {
                let n: usize = cmd.parse().unwrap_or(1);
                let l = n.saturating_sub(1).min(last_line(doc));
                doc.selection = Selection::caret(first_non_blank(doc, l));
            }
            _ => {
                out.message = Some((crate::tr!("vim-not-a-command", command = cmd), true));
            }
        }
        if self.visual() {
            self.mode = Mode::Normal;
        }
        self.reset();
        self.keys.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LineEnding, Metadata};

    #[test]
    fn modes_it_applies_to() {
        let text = DocumentMode::Text { language: None };
        let bib = DocumentMode::Text {
            language: Some("bib".into()),
        };
        assert!(Vim::applies(&[], &DocumentMode::Latex));
        assert!(Vim::applies(&["latex"], &DocumentMode::Latex));
        assert!(!Vim::applies(&["plain"], &DocumentMode::Latex));
        assert!(Vim::applies(&["plain"], &text));
        assert!(Vim::applies(&["text"], &bib));
        assert!(!Vim::applies(&["text"], &DocumentMode::Directory));
        assert!(Vim::applies(&["directory"], &DocumentMode::Directory));
    }

    #[derive(Default)]
    struct TestHost {
        clip: Option<String>,
        rich: bool,
    }

    impl Host for TestHost {
        fn clipboard(&mut self) -> Option<String> {
            self.clip.clone()
        }

        fn rich_view(&self) -> bool {
            self.rich
        }

        fn set_clipboard(&mut self, text: &str) {
            self.clip = Some(text.to_string());
        }
    }

    fn doc(text: &str, mode: DocumentMode) -> DocumentState {
        let meta = Metadata {
            path: None,
            mode,
            line_ending: LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        DocumentState::new(text, meta, std::sync::Arc::default())
    }

    /// Runs `keys` (with `<Esc>`, `<CR>`, `<BS>`, `<C-r>`) on `text` with
    /// the cursor at `at`; returns the text with `|` at the cursor. Keys
    /// the layer leaves alone are typed, as a frontend does.
    fn run_in(text: &str, at: usize, keys: &str, mode: DocumentMode) -> (String, Vim, TestHost) {
        let mut d = doc(text, mode);
        d.selection = Selection::caret(at);
        let mut v = Vim::new();
        let mut host = TestHost::default();
        let mut chars = keys.chars().peekable();
        while let Some(c) = chars.next() {
            let key = if c == '<' && chars.peek().is_some_and(char::is_ascii_alphabetic) {
                let name: String = chars.by_ref().take_while(|c| *c != '>').collect();
                match name.as_str() {
                    "Esc" => Key::Esc,
                    "CR" => Key::Enter,
                    "BS" => Key::Backspace,
                    n if n.starts_with("C-") => Key::Ctrl(n.chars().nth(2).expect("a letter")),
                    _ => panic!("{name}"),
                }
            } else {
                Key::Char(c)
            };
            let out = v.key(&mut d, key, &mut host);
            if !out.handled {
                match key {
                    Key::Char(c) => d.insert_text(&c.to_string(), Instant::now()),
                    Key::Enter => d.insert_text("\n", Instant::now()),
                    Key::Backspace => {
                        let _ = d.delete_backward(Instant::now());
                    }
                    _ => {}
                }
            }
        }
        let mut s = d.text().as_str().to_string();
        s.insert(d.selection.head, '|');
        (s, v, host)
    }

    fn run(text: &str, at: usize, keys: &str) -> String {
        run_in(text, at, keys, DocumentMode::Text { language: None }).0
    }

    #[test]
    fn editing_keys_in_a_listing_keep_the_cursor_in_it() {
        let dir = std::env::temp_dir().join(format!("kalem-vim-listing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.org"), "x").unwrap();
        let mut d =
            DocumentState::open(&dir, std::sync::Arc::default(), &Default::default()).unwrap();
        let text = d.text().as_str().to_string();
        let mut v = Vim::new();
        let mut host = TestHost {
            clip: Some("pasted\nlines\n".into()),
            ..TestHost::default()
        };
        for keys in [
            "Go<Esc>",
            "GA tail<Esc>",
            "Gp",
            "GP",
            "Gdd",
            "Gx",
            "GJ",
            "G~",
            "u",
            "<C-r>",
            ".",
            "Gyyp",
            "GO<Esc>",
            "Gcc<Esc>",
            "GD",
            "G$C<Esc>",
            "ggi<CR><Esc>",
            "G$a<CR><CR><Esc>",
        ] {
            let mut chars = keys.chars().peekable();
            while let Some(c) = chars.next() {
                let key = if c == '<' {
                    let name: String = chars.by_ref().take_while(|c| *c != '>').collect();
                    match name.as_str() {
                        "Esc" => Key::Esc,
                        "CR" => Key::Enter,
                        n => Key::Ctrl(n.chars().nth(2).expect("a letter")),
                    }
                } else {
                    Key::Char(c)
                };
                let out = v.key(&mut d, key, &mut host);
                if !out.handled
                    && let Key::Char(c) = key
                {
                    d.insert_text(&c.to_string(), Instant::now());
                }
                let len = d.text().len();
                assert!(
                    d.selection.head <= len && d.selection.anchor <= len,
                    "{keys}: {:?} {len}",
                    d.selection
                );
                let c = v.caret(&d);
                let _ = d.grapheme_after(c);
                let _ = d.grapheme_before(c);
            }
            assert_eq!(d.text().as_str(), text, "{keys}");
        }
        assert_eq!(d.grapheme_after(10_000), d.text().len());
    }

    #[test]
    fn leader_and_documents() {
        let mut d = doc("one two\n", DocumentMode::Text { language: None });
        let mut v = Vim::new();
        let mut host = TestHost::default();
        // The leader is left to the keymap in normal mode, not after a
        // count or an operator.
        assert!(!v.key(&mut d, Key::Char(' '), &mut host).handled);
        assert!(v.key(&mut d, Key::Char('2'), &mut host).handled);
        assert!(v.key(&mut d, Key::Char(' '), &mut host).handled);
        assert_eq!(d.selection.head, 2);
        assert!(v.idle_command());
        assert_eq!(v.mode_name(), "normal");
        // Without a leader Space moves.
        v.leader = None;
        assert!(v.key(&mut d, Key::Char(' '), &mut host).handled);
        assert_eq!(Vim::leader_key("space"), Some(Key::Char(' ')));
        assert_eq!(Vim::leader_key(","), Some(Key::Char(',')));
        assert_eq!(Vim::leader_key("ctrl+space"), None);
        // Buffers: `:bn`, `:bp`, `:bd`, `:ls`, `:e FILE`, `gt`, `gT`.
        let ex = |v: &mut Vim, d: &mut DocumentState, keys: &str| {
            let mut host = TestHost::default();
            let mut commands = Vec::new();
            for c in keys.chars() {
                let key = if c == '\n' { Key::Enter } else { Key::Char(c) };
                commands.extend(v.key(d, key, &mut host).commands);
            }
            commands
        };
        let names = |c: Vec<(String, Value)>| c.into_iter().map(|c| c.0).collect::<Vec<_>>();
        assert_eq!(names(ex(&mut v, &mut d, ":bn\n")), ["file.next"]);
        assert_eq!(names(ex(&mut v, &mut d, ":bp\n")), ["file.previous"]);
        assert_eq!(names(ex(&mut v, &mut d, ":bd\n")), ["file.close"]);
        assert_eq!(names(ex(&mut v, &mut d, ":ls\n")), ["file.switch"]);
        assert_eq!(
            names(ex(&mut v, &mut d, "gtgT")),
            ["file.next", "file.previous"]
        );
        let open = ex(&mut v, &mut d, ":e notes/a.org\n");
        assert_eq!(open[0].0, "file.open");
        assert_eq!(open[0].1["path"], "notes/a.org");
        assert_eq!(names(ex(&mut v, &mut d, ":e!\n")), ["app.revert"]);
    }

    #[test]
    fn motions() {
        let t = "one two  three\nfour five\n\nsix\n";
        assert_eq!(run(t, 0, "w"), "one |two  three\nfour five\n\nsix\n");
        assert_eq!(run(t, 0, "3w"), "one two  three\n|four five\n\nsix\n");
        assert_eq!(run(t, 0, "e"), "on|e two  three\nfour five\n\nsix\n");
        assert_eq!(run(t, 9, "b"), "one |two  three\nfour five\n\nsix\n");
        assert_eq!(run(t, 5, "$"), "one two  thre|e\nfour five\n\nsix\n");
        assert_eq!(run(t, 5, "0"), "|one two  three\nfour five\n\nsix\n");
        assert_eq!(run(t, 5, "j"), "one two  three\nfour |five\n\nsix\n");
        assert_eq!(run(t, 12, "jj"), "one two  three\nfour five\n|\nsix\n");
        assert_eq!(run(t, 12, "jjj"), "one two  three\nfour five\n\nsi|x\n");
        // The empty line after the final line feed is not a line.
        assert_eq!(run(t, 0, "G"), "one two  three\nfour five\n\n|six\n");
        assert_eq!(run(t, 0, "9j"), "one two  three\nfour five\n\n|six\n");
        assert_eq!(run(t, 20, "gg"), "|one two  three\nfour five\n\nsix\n");
        assert_eq!(run(t, 0, "2G"), "one two  three\n|four five\n\nsix\n");
        assert_eq!(run(t, 0, "fe"), "on|e two  three\nfour five\n\nsix\n");
        assert_eq!(run(t, 0, "fe;"), "one two  thr|ee\nfour five\n\nsix\n");
        assert_eq!(run(t, 0, "tt"), "one| two  three\nfour five\n\nsix\n");
        assert_eq!(run(t, 0, "}"), "one two  three\nfour five\n|\nsix\n");
        assert_eq!(run("a (b [c] d) e\n", 2, "%"), "a (b [c] d|) e\n");
        assert_eq!(run(t, 0, "/fi<CR>"), "one two  three\nfour |five\n\nsix\n");
        assert_eq!(run(t, 0, "/e<CR>n"), "one two  thr|ee\nfour five\n\nsix\n");
        assert_eq!(run(t, 20, "?t<CR>"), "one two  |three\nfour five\n\nsix\n");
        assert_eq!(run("foo bar foo\n", 0, "*"), "foo bar |foo\n");
        assert_eq!(run("ab\n", 0, "llll"), "a|b\n");
    }

    #[test]
    fn operators() {
        let t = "one two three\nfour\nfive\n";
        assert_eq!(run(t, 0, "dw"), "|two three\nfour\nfive\n");
        assert_eq!(run(t, 0, "d2w"), "|three\nfour\nfive\n");
        assert_eq!(run(t, 0, "2dw"), "|three\nfour\nfive\n");
        assert_eq!(run(t, 8, "dw"), "one two| \nfour\nfive\n");
        assert_eq!(run(t, 4, "de"), "one | three\nfour\nfive\n");
        assert_eq!(run(t, 4, "d$"), "one| \nfour\nfive\n");
        assert_eq!(run(t, 4, "D"), "one| \nfour\nfive\n");
        assert_eq!(run(t, 0, "dd"), "|four\nfive\n");
        assert_eq!(run(t, 0, "2dd"), "|five\n");
        assert_eq!(run(t, 14, "dj"), "|one two three\n");
        assert_eq!(run("a\nb", 2, "dd"), "|a");
        assert_eq!(run(t, 4, "x"), "one |wo three\nfour\nfive\n");
        assert_eq!(run(t, 4, "3x"), "one | three\nfour\nfive\n");
        assert_eq!(run(t, 4, "cwTWO<Esc>"), "one TW|O three\nfour\nfive\n");
        assert_eq!(run(t, 0, "c2wx<Esc>"), "|x three\nfour\nfive\n");
        assert_eq!(run(t, 4, "ciwx<Esc>"), "one |x three\nfour\nfive\n");
        assert_eq!(run(t, 0, "ccnew<Esc>"), "ne|w\nfour\nfive\n");
        assert_eq!(
            run(t, 0, "yyp"),
            "one two three\n|one two three\nfour\nfive\n"
        );
        assert_eq!(
            run(t, 0, "yyjP"),
            "one two three\n|one two three\nfour\nfive\n"
        );
        assert_eq!(run(t, 0, "ywP"), "one| one two three\nfour\nfive\n");
        assert_eq!(run(t, 0, "ddp"), "four\n|one two three\nfive\n");
        assert_eq!(run(t, 0, "J"), "one two three| four\nfive\n");
        assert_eq!(run(t, 0, "rx"), "|xne two three\nfour\nfive\n");
        assert_eq!(run(t, 0, "~~"), "ON|e two three\nfour\nfive\n");
        assert_eq!(run(t, 0, "gUiw"), "|ONE two three\nfour\nfive\n");
        assert_eq!(run(t, 0, "gUU"), "|ONE TWO THREE\nfour\nfive\n");
        assert_eq!(run(t, 0, ">>"), "  |one two three\nfour\nfive\n");
        assert_eq!(run("  a\n", 2, "<<"), "|a\n");
        assert_eq!(run("f(a, (b))\n", 5, "di("), "f(a, (|))\n");
        assert_eq!(run("f(a, (b))\n", 3, "da("), "|f\n");
        assert_eq!(
            run("say \"hi there\" x\n", 6, "ci\"yo<Esc>"),
            "say \"y|o\" x\n"
        );
        assert_eq!(run("a\nb\n\nc\n", 0, "dap"), "|c\n");
    }

    #[test]
    fn insert_visual_and_repeat() {
        let t = "one\ntwo\n";
        assert_eq!(run(t, 0, "ix<Esc>"), "|xone\ntwo\n");
        assert_eq!(run(t, 0, "ax<Esc>"), "o|xne\ntwo\n");
        assert_eq!(run(t, 0, "A!<Esc>"), "one|!\ntwo\n");
        assert_eq!(run(t, 0, "onew<Esc>"), "one\nne|w\ntwo\n");
        assert_eq!(run(t, 4, "Onew<Esc>"), "one\nne|w\ntwo\n");
        assert_eq!(run(t, 0, "vld"), "|e\ntwo\n");
        assert_eq!(run(t, 0, "Vjd"), "|");
        assert_eq!(run(t, 0, "vey$p"), "oneon|e\ntwo\n");
        assert_eq!(run(t, 0, "viwU"), "|ONE\ntwo\n");
        assert_eq!(run(t, 0, "x.."), "|\ntwo\n");
        assert_eq!(run(t, 0, "A!<Esc>j."), "one!\ntwo|!\n");
        assert_eq!(run("a b c\n", 0, "cwx<Esc>w."), "x |x c\n");
        assert_eq!(run("a b c d\n", 0, "dw2."), "|d\n");
        assert_eq!(run(t, 0, "ddu"), "|one\ntwo\n");
        assert_eq!(run(t, 0, "ddu<C-r>"), "|two\n");
        assert_eq!(run("abcde\nx\nabc\n", 4, "jddki!<Esc>"), "|!abcde\nabc\n");
        assert_eq!(run(t, 0, "Rxy<Esc>"), "x|ye\ntwo\n");
    }

    #[test]
    fn registers_and_command_line() {
        let text = DocumentMode::Text { language: None };
        let (s, _, host) = run_in("one two\n", 0, "\"+yw\"ayy$\"ap", text.clone());
        assert_eq!(host.clip.as_deref(), Some("one "));
        assert_eq!(s, "one two\n|one two\n");
        let mut d = doc("a\nb\nc\n", text);
        let mut v = Vim::new();
        let mut h = TestHost::default();
        for c in ":3".chars() {
            v.key(&mut d, Key::Char(c), &mut h);
        }
        assert_eq!(v.command_line.as_deref(), Some(":3"));
        v.key(&mut d, Key::Enter, &mut h);
        assert_eq!(d.selection.head, 4);
        for c in ":wq".chars() {
            v.key(&mut d, Key::Char(c), &mut h);
        }
        let out = v.key(&mut d, Key::Enter, &mut h);
        let names: Vec<&str> = out.commands.iter().map(|c| c.0.as_str()).collect();
        assert_eq!(names, ["app.save", "app.quit"]);
        // Keys Vim does not use go on; unused characters do nothing.
        assert!(!v.key(&mut d, Key::Ctrl('s'), &mut h).handled);
        let before = d.text().as_str().to_string();
        assert!(v.key(&mut d, Key::Char('Q'), &mut h).handled);
        assert_eq!(d.text().as_str(), before);
        assert!(!v.takes_text());
        v.key(&mut d, Key::Char('i'), &mut h);
        assert!(v.takes_text());
    }

    #[test]
    fn org_shifts() {
        let org = DocumentMode::Org;
        assert_eq!(
            run_in("* A\n- x\n- y\n", 0, ">>", org.clone()).0,
            "|** A\n- x\n- y\n"
        );
        let s = run_in("* A\n- x\n- y\n", 10, ">>", org).0;
        assert!(s.contains("- x\n  |- y"), "{s}");
    }

    #[test]
    fn visual_block() {
        let t = "abcd\nefgh\nijkl\n";
        // Columns 1 and 2 of three lines.
        assert_eq!(run(t, 1, "<C-v>jjld"), "a|d\neh\nil\n");
        assert_eq!(run(t, 1, "<C-v>jlx"), "a|d\neh\nijkl\n");
        assert_eq!(run(t, 1, "<C-v>jjlU"), "a|BCd\neFGh\niJKl\n");
        // Yanked as lines of the block.
        let (_, v, _) = run_in(t, 1, "<C-v>jly", DocumentMode::Text { language: None });
        assert_eq!(
            v.registers.get(&'"').map(|r| r.text.as_str()),
            Some("bc\nfg")
        );
        // I and A type on every line; A pads short lines.
        assert_eq!(run(t, 1, "<C-v>jjI-<Esc>"), "a|-bcd\ne-fgh\ni-jkl\n");
        assert_eq!(run("ab\nc\nde\n", 0, "<C-v>jjlA!<Esc>"), "ab|!\nc !\nde!\n");
        // c changes the block and types on every line.
        assert_eq!(run(t, 1, "<C-v>jlcX<Esc>"), "a|Xd\neXh\nijkl\n");
        // o swaps the corners; Ctrl+V again ends it.
        let (s, v, _) = run_in(t, 1, "<C-v>jlo", DocumentMode::Text { language: None });
        assert_eq!(v.mode, Mode::VisualBlock);
        assert_eq!(s, "a|bcd\nefgh\nijkl\n");
        let (_, v, _) = run_in(t, 1, "<C-v><C-v>", DocumentMode::Text { language: None });
        assert_eq!(v.mode, Mode::Normal);
    }

    #[test]
    fn block_ranges_for_painting() {
        let mut d = doc("abcd\nefgh\n", DocumentMode::Text { language: None });
        d.selection = Selection::caret(1);
        let mut v = Vim::new();
        let mut host = TestHost::default();
        for k in [Key::Ctrl('v'), Key::Char('j'), Key::Char('l')] {
            v.key(&mut d, k, &mut host);
        }
        assert_eq!(v.block_ranges(&d), Some(vec![1..3, 6..8]));
        assert_eq!(d.selection, Selection::caret(7));
        v.key(&mut d, Key::Esc, &mut host);
        assert_eq!(v.block_ranges(&d), None);
    }

    #[test]
    fn org_text_objects() {
        let org = || DocumentMode::Org;
        let r = |t: &str, at: usize, k: &str| run_in(t, at, k, org()).0;
        let t = "* TODO Title here :tag:\nbody\n** Child\nmore\n* Next\n";
        // Headline: its title, or its line.
        assert_eq!(
            r(t, 9, "cihNew<Esc>"),
            "* TODO Ne|w :tag:\nbody\n** Child\nmore\n* Next\n"
        );
        assert_eq!(r(t, 26, "dah"), "|body\n** Child\nmore\n* Next\n");
        // Subtree: below the headline, or all of it.
        assert_eq!(r(t, 2, "diR"), "* TODO Title here :tag:\n|* Next\n");
        assert_eq!(r(t, 2, "daR"), "|* Next\n");
        // List items.
        let l = "- [ ] first item\n- second\n  more\n";
        assert_eq!(r(l, 8, "ciigo<Esc>"), "- [ ] g|o\n- second\n  more\n");
        assert_eq!(r(l, 20, "dai"), "|- [ ] first item\n");
        // Table cells.
        let c = "| a  | bee |\n|----+-----|\n";
        assert_eq!(r(c, 8, "cicx<Esc>"), "| a  | |x |\n|----+-----|\n");
        // Emphasis.
        let e = "Some *bold words* here.\n";
        assert_eq!(r(e, 8, "die"), "Some *|* here.\n");
        assert_eq!(r(e, 8, "dae"), "Some |here.\n");
        // Nothing to act on: nothing happens.
        assert_eq!(r("plain\n", 1, "die"), "p|lain\n");
    }
}
