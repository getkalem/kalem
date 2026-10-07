//! Vim keys (§7.3.1): the modal input layer of the Vim keymap profile.
//!
//! The layer works on the document state, so both frontends share it, and
//! it leaves insert mode to them: typing and the Word-like keys work as
//! usual there, and only Escape comes back here. In the other modes every
//! key comes here first; keys with Control or Alt that Vim does not use go
//! on to the keymap (so Ctrl+S still saves and Ctrl+C copies).
//!
//! Covered: normal, insert, visual (characters, lines and blocks) and
//! replace modes; counts; Vim's motions, operators (with `v`, `V` and
//! CTRL-V to force a motion's kind), text objects (sentences and
//! paragraphs ported from Vim in `objects`), registers, macros, marks
//! with the jump and change lists, undo with `U`, `g-` and `g+`, search
//! with offsets, `.`; and the command line with ranges, `:s`, `:g`,
//! `:normal`, `:sort`, `:retab` and the file and window commands (`ex`).
//! In Org documents `>>` and `<<` demote and promote headlines and indent
//! list items. `tests/vim/cases.json` holds key sequences whose text and
//! cursor must come out as in Vim itself (`tools/vim-expected.py`).

mod ex;
mod insert;
mod normal;
mod objects;
mod org;
mod pattern;

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
    /// The first and last document lines on screen (from 0), for `H`,
    /// `M` and `L`; `None` where the frontend cannot say.
    fn visible_lines(&self) -> Option<(usize, usize)> {
        None
    }
    /// Scrolls the view `by` lines down (up when negative), for CTRL-E
    /// and CTRL-Y, the first line shown at most `last` (the document's
    /// last); the lines on screen after.
    fn scroll(&mut self, by: isize, last: usize) -> Option<(usize, usize)> {
        let _ = (by, last);
        None
    }
    /// Shows `line` at the top (0), middle (1) or bottom (2) of the
    /// screen, for `zt`, `zz` and `zb`.
    fn scroll_to(&mut self, line: usize, at: u8) {
        let _ = (line, at);
    }
    /// The screen's width in columns, for `gm`.
    fn columns(&self) -> usize {
        80
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
    Rot13,
    Reindent,
}

impl Op {
    fn of(c: char) -> Option<Op> {
        Some(match c {
            'd' => Op::Delete,
            'c' => Op::Change,
            'y' => Op::Yank,
            '>' => Op::Indent,
            '<' => Op::Outdent,
            '=' => Op::Reindent,
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
    /// `m`: the mark's name.
    Mark,
    /// `'` (linewise) or `` ` ``: the mark to go to.
    GotoMark(bool),
    /// `q`: the register to record into.
    Macro,
    /// `@`: the register to run.
    Execute,
    /// `z`: what to scroll.
    Z,
    /// `[` (back) or `]`: the bracket, section or put that follows.
    Bracket(bool),
}

/// A register's contents.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Register {
    text: String,
    linewise: bool,
    /// Yanked from a block: put as a block.
    block: bool,
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

/// Where a search puts the cursor from its match (`:help
/// search-offset`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchOffset {
    /// `[+-]n`: that many lines down or up, at the start of the line
    /// (the motion is then of lines).
    Line(isize),
    /// `e[+-n]`: from the match's last character (inclusive).
    End(isize),
    /// `s[+-n]` or `b[+-n]`: from its first.
    Start(isize),
}

/// A search typed after `/` or `?` (`delim`): the pattern and, after an
/// unescaped `delim` (not in a `[]` collection), the offset.
fn split_search(typed: &str, delim: char) -> (&str, Option<&str>) {
    let mut i = 0;
    let mut class = false;
    while let Some(c) = typed[i..].chars().next() {
        if c == '\\' {
            i += 1;
            i += typed[i..].chars().next().map_or(0, char::len_utf8);
            continue;
        }
        if class {
            class = c != ']';
        } else if c == '[' && typed[i + 1..].chars().skip(1).any(|x| x == ']') {
            class = true;
        } else if c == delim {
            return (&typed[..i], Some(&typed[i + 1..]));
        }
        i += c.len_utf8();
    }
    (typed, None)
}

/// The offset `t` after a search's pattern; none when empty (or not one).
fn parse_offset(t: &str) -> Option<SearchOffset> {
    // `;` and a search after it are not taken.
    let t = t.split(';').next().unwrap_or("");
    let (make, rest): (fn(isize) -> SearchOffset, &str) = match t.as_bytes().first()? {
        b'e' => (SearchOffset::End, &t[1..]),
        b's' | b'b' => (SearchOffset::Start, &t[1..]),
        _ => (SearchOffset::Line, t),
    };
    let n = match rest {
        "" => 0,
        "+" => 1,
        "-" => -1,
        r => r.parse().ok()?,
    };
    Some(make(n))
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

/// The options `:set` changes (`:help options`), with Kalem's defaults
/// where Vim's differ: 'autoindent' on, 'shiftwidth' 2, 'expandtab',
/// 'softtabstop' following 'shiftwidth', 'backspace' as
/// `indent,eol,start` (always), 'nrformats' `bin,hex`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// 'shiftwidth': columns for `>>`, `<<`, CTRL-T and CTRL-D (0: 'tabstop').
    pub shiftwidth: usize,
    /// 'tabstop': the columns a tab takes.
    pub tabstop: usize,
    /// 'softtabstop': the columns Tab and Backspace go (negative:
    /// 'shiftwidth').
    pub softtabstop: isize,
    /// 'expandtab': spaces instead of tabs.
    pub expandtab: bool,
    /// 'autoindent': a new line gets the indent of the one before.
    pub autoindent: bool,
    /// 'ignorecase' in patterns.
    pub ignorecase: bool,
    /// 'smartcase': an upper case letter in a pattern matches case.
    pub smartcase: bool,
    /// 'wrapscan': searches go around the end.
    pub wrapscan: bool,
    /// 'nrformats': `bin`, `octal`, `hex`, `alpha` for CTRL-A and CTRL-X.
    pub nrformats: Vec<String>,
    /// 'joinspaces': two spaces after a period when joining.
    pub joinspaces: bool,
    /// 'startofline': line motions go to the first non-blank.
    pub startofline: bool,
    /// 'textwidth': the width `gq` formats to (0: 79).
    pub textwidth: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            shiftwidth: 2,
            tabstop: 8,
            softtabstop: -1,
            expandtab: true,
            autoindent: true,
            ignorecase: false,
            smartcase: false,
            wrapscan: true,
            nrformats: vec!["bin".into(), "hex".into()],
            joinspaces: false,
            startofline: false,
            textwidth: 0,
        }
    }
}

/// CTRL-O in insert mode: one command, then insert mode again.
#[derive(Debug, Clone, Copy)]
struct CtrlO {
    /// The cursor was after the end of the line.
    eol: bool,
    /// Its line.
    line: usize,
}

/// A host with no clipboard, for keys replayed inside the layer.
struct NoHost;

impl Host for NoHost {
    fn clipboard(&mut self) -> Option<String> {
        None
    }

    fn set_clipboard(&mut self, _: &str) {}
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
    /// The last search's offset (`/pat/e+1`), which `n` and `N` keep.
    search_offset: Option<SearchOffset>,
    /// Where Vim's cursor is as the next operator changes the text (its
    /// `uh_cursor`): undo comes back there.
    op_start: Option<usize>,
    /// Where an operator began changing the text, for `'.` and the
    /// change list.
    change_at: Option<usize>,
    /// `v`, `V` or CTRL-V after an operator: the motion made of
    /// characters, lines or a block (`:help o_v`).
    force: Option<char>,
    /// A command failed (Vim beeps): a macro running stops.
    failed: bool,
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
    /// The options.
    pub options: Options,
    insert_pending: insert::InsertPending,
    completion: Option<insert::Completion>,
    /// What the last insert typed (the `.` register, CTRL-A).
    last_inserted: Option<String>,
    ctrl_o: Option<CtrlO>,
    /// The start of a line indented by 'autoindent' with nothing typed on
    /// it yet: Escape and Enter take the indent away again.
    ai_line: Option<usize>,
    /// `3i`: the insert is typed this many times.
    insert_count: usize,
    insert_repeat: insert::Repeat,
    /// What replace mode typed over, for Backspace.
    replaced: Vec<String>,
    /// The macro being recorded: its register and the keys so far.
    macro_rec: Option<(char, Vec<Key>)>,
    last_macro: Option<char>,
    macro_depth: usize,
    /// The last command line (`@:`, the `:` register).
    last_ex: Option<String>,
    /// `U`: the last changed line and its text before the changes.
    line_undo: Option<(usize, String, usize)>,
    /// The line an insert began on, its text then and the number of lines,
    /// for `U` once something is typed.
    insert_line: Option<(usize, String, usize, usize)>,
    /// The last visual selection, for `gv`: its mode, anchor and cursor,
    /// and its size for `1v` (see [`Vim::visual_size`]).
    last_visual: Option<(Mode, usize, usize, (usize, usize))>,
    /// The motion being made is a jump (`G`, `%`, a search).
    jumping: bool,
    /// The last `:s`, for `:&`, `&`, `g&` and `~` in a replacement.
    last_sub: Option<ex::Substitute>,
    /// The last put was of lines.
    put_linewise: bool,
    /// A block selection goes to the end of every line (`$`).
    block_end: bool,
    /// `3/x`: the match to go to.
    search_count: usize,
    /// The text being stored is a block's.
    storing_block: bool,
    /// Where the cursor goes when a block insert ends: the block's top
    /// left corner.
    block_corner: Option<usize>,
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

/// `pos` moved `k` characters on (back when negative), over line ends as
/// Vim's `incl()` and `decl()` do, stopping at the text's ends.
fn step_chars(doc: &DocumentState, pos: usize, k: isize) -> usize {
    let mut p = pos;
    for _ in 0..k.unsigned_abs() {
        let r = if k > 0 { objects::incl(doc, &mut p) } else { objects::decl(doc, &mut p) };
        if r == -1 {
            break;
        }
    }
    p
}

/// What `*`, `#`, `g*` and `g#` look for at `pos` (Vim's
/// `find_ident_under_cursor()`): the keyword under the cursor or the
/// first after it on the line; with none, the non-blanks there.
fn ident_at(doc: &DocumentState, pos: usize) -> Option<Range<usize>> {
    let line = line_of(doc, pos);
    let (s, e) = (line_start(doc, line), line_end(doc, line));
    let len = |p: usize| char_at(doc, p).map_or(1, char::len_utf8);
    let at = |p: usize| char_at(doc, p).filter(|c| *c != '\n');
    // A keyword.
    let mut p = pos.min(e);
    while p < e && !at(p).is_some_and(is_word) {
        p += len(p);
    }
    if p < e {
        while p > s && char_before(doc, p).is_some_and(is_word) {
            p -= char_before(doc, p).map_or(1, char::len_utf8);
        }
        let mut end = p;
        while end < e && at(end).is_some_and(is_word) {
            end += len(end);
        }
        return Some(p..end);
    }
    // Else non-blanks, from the start of their kind.
    let mut p = pos.min(e);
    while p < e && at(p).is_some_and(char::is_whitespace) {
        p += len(p);
    }
    let k = class(at(p)?, false);
    while p > s && char_before(doc, p).is_some_and(|c| c != '\n' && class(c, false) == k) {
        p -= char_before(doc, p).map_or(1, char::len_utf8);
    }
    let mut end = p;
    while end < e && at(end).is_some_and(|c| !c.is_whitespace()) {
        end += len(end);
    }
    Some(p..end)
}

fn find_kind(c: char) -> Find {
    match c {
        'f' => Find::To,
        't' => Find::Till,
        'F' => Find::Back,
        _ => Find::BackTill,
    }
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

/// `pos` in the text: at most its end, at the start of the character it
/// falls in (a place kept from before an edit).
fn snap(doc: &DocumentState, pos: usize) -> usize {
    let text = doc.text().as_str();
    let mut p = pos.min(text.len());
    while !text.is_char_boundary(p) {
        p -= 1;
    }
    p
}

/// `c` in lower case as Vim lowers it: one character for one (Unicode's
/// simple mapping, so `İ` is `i`), else `c` itself.
fn lower_char(c: char) -> char {
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(one), None) => one,
        _ if c == 'İ' => 'i',
        _ => c,
    }
}

/// `c` in upper case as Vim raises it: one character for one, else `c`
/// itself (`ß` has no single capital).
fn upper_char(c: char) -> char {
    let mut u = c.to_uppercase();
    match (u.next(), u.next()) {
        (Some(one), None) => one,
        _ => c,
    }
}

/// The column `pos` shows in on its line: tabs to the next stop of `ts`,
/// wide characters two columns.
fn display_col(doc: &DocumentState, pos: usize, ts: usize) -> usize {
    use unicode_width::UnicodeWidthChar;
    let s = line_start(doc, line_of(doc, pos));
    doc.text().as_str()[s..pos].chars().fold(0, |col, c| {
        if c == '\t' {
            (col / ts + 1) * ts
        } else {
            col + c.width().unwrap_or(0)
        }
    })
}

/// The columns `pos` shows in: its first and last (a tab to the next
/// stop of `ts`); a line's end shows in one.
fn display_span(doc: &DocumentState, pos: usize, ts: usize) -> (usize, usize) {
    use unicode_width::UnicodeWidthChar;
    let first = display_col(doc, pos, ts);
    let width = match char_at(doc, pos) {
        Some('\t') => (first / ts + 1) * ts - first,
        Some(c) if c != '\n' => c.width().unwrap_or(0).max(1),
        _ => 1,
    };
    (first, first + width - 1)
}

/// What a block of the columns `left..right` covers on `line` (Vim's
/// `block_prep()`): the characters at least partly in it, and how many
/// columns of a tab or wide character at its left and right edges lie
/// outside it (`pre`, `post`), which become spaces when it goes.
#[derive(Debug, Clone)]
struct BlockPart {
    range: Range<usize>,
    pre: usize,
    post: usize,
}

fn block_part(doc: &DocumentState, line: usize, left: usize, right: usize, ts: usize) -> BlockPart {
    use unicode_width::UnicodeWidthChar;
    let (s, e) = (line_start(doc, line), line_end(doc, line));
    let mut col = 0;
    let mut start = None;
    let (mut pre, mut post, mut end) = (0, 0, e);
    for (i, c) in doc.text().as_str()[s..e].char_indices() {
        let next = if c == '\t' { (col / ts + 1) * ts } else { col + c.width().unwrap_or(0) };
        if start.is_none() && col >= right {
            break;
        }
        if start.is_none() && next > left {
            start = Some(s + i);
            pre = left.saturating_sub(col);
        }
        if start.is_some() {
            if col >= right && next > col {
                end = s + i;
                break;
            }
            if next > right {
                post = next - right;
                end = s + i + c.len_utf8();
                break;
            }
        }
        col = next;
    }
    match start {
        Some(a) => BlockPart {
            range: a..end.max(a),
            pre,
            post,
        },
        None => BlockPart {
            range: e..e,
            pre: 0,
            post: 0,
        },
    }
}

/// The character of `line` that shows in column `want` (a tab or wide
/// character covering it), or the line's end when it is shorter (Vim's
/// `coladvance()`).
fn at_display_col(doc: &DocumentState, line: usize, want: usize, ts: usize) -> usize {
    use unicode_width::UnicodeWidthChar;
    let (s, e) = (line_start(doc, line), line_end(doc, line));
    let mut col = 0;
    for (i, c) in doc.text().as_str()[s..e].char_indices() {
        let next = if c == '\t' {
            (col / ts + 1) * ts
        } else {
            col + c.width().unwrap_or(0)
        };
        if next > want && c.width() != Some(0) {
            return s + i;
        }
        col = next;
    }
    e
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
    tx.edit(range, insert);
    let tx = tx.select(Selection::caret(caret));
    doc.apply(&tx, ChangeKind::Command, Instant::now());
}

// Motions.

fn word_forward(doc: &DocumentState, mut pos: usize, big: bool, stop_at_eol: bool) -> usize {
    let len = doc.text().len();
    if pos >= len {
        return len;
    }
    // From a line's end (an empty line) the motion moves at least onto
    // the next line, even for an operator.
    if stop_at_eol && char_at(doc, pos) == Some('\n') {
        return if pos + 1 < len { pos + 1 } else { pos };
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
    repeat: bool,
) -> Option<usize> {
    let line = line_of(doc, pos);
    let (s, e) = (line_start(doc, line), line_end(doc, line));
    let text = doc.text().as_str();
    let mut p = pos;
    for i in 0..count {
        match kind {
            Find::To | Find::Till => {
                // Repeating `t` next to its target goes on to the next one.
                let from = if kind == Find::Till && i == 0 && repeat {
                    doc.grapheme_after(p)
                } else {
                    p
                };
                let start = doc.grapheme_after(from).min(e);
                p = start + text[start..e].find(target)?;
            }
            Find::Back | Find::BackTill => {
                let end = if kind == Find::BackTill && i == 0 && repeat {
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

/// The end of the word before `pos` (`ge`, `gE`).
fn word_end_back(doc: &DocumentState, pos: usize, big: bool) -> usize {
    let text = doc.text().as_str();
    let k = char_at(doc, pos).map_or(0, |c| class(c, big));
    let mut p = pos;
    // Out of the word the cursor is in.
    if k != 0 {
        while let Some(c) = char_before(doc, p) {
            if c == '\n' || class(c, big) != k {
                break;
            }
            p -= c.len_utf8();
        }
    }
    // Back over blanks; an empty line is a word.
    while let Some(c) = char_before(doc, p) {
        if c == '\n' {
            if p >= 2 && text.as_bytes()[p - 2] == b'\n' {
                return p - 1;
            }
            p -= 1;
            continue;
        }
        if !c.is_whitespace() {
            return p - c.len_utf8();
        }
        p -= c.len_utf8();
    }
    0
}

/// What an operator works on after a motion or object over the
/// characters `s..=e` (`inclusive`) or `s..e`, as Vim adjusts it: an
/// exclusive end at the start of a later line stops at the end of the
/// line before, or takes whole lines when it starts in the indent
/// (`:help exclusive-linewise`); a delete across lines from the indent
/// to blanks takes whole lines (`:help o_v`'s exception for `d`).
fn char_target(doc: &DocumentState, op: Op, s: usize, e: usize, inclusive: bool) -> Target {
    let mut e = e;
    // An inclusive end at a line's end takes no line break; the motion
    // ends in that line.
    let mut end_line = None;
    if inclusive {
        if char_at(doc, e) == Some('\n') {
            end_line = Some(line_of(doc, e));
        } else {
            e = doc.grapheme_after(e).max(e);
        }
    } else if e > s && line_of(doc, e) > line_of(doc, s) && e == line_start(doc, line_of(doc, e)) {
        let (l1, l2) = (line_of(doc, s), line_of(doc, e) - 1);
        if s <= first_non_blank(doc, l1) {
            return Target::Lines(l1, l2);
        }
        e = line_end(doc, l2).max(s);
    }
    if op == Op::Delete && e > s {
        let l1 = line_of(doc, s);
        let l2 = end_line.unwrap_or_else(|| line_of(doc, e.saturating_sub(1).max(s)));
        let rest = &doc.text().as_str()[e.min(line_end(doc, l2))..line_end(doc, l2)];
        if l2 > l1 && rest.trim().is_empty() && s <= first_non_blank(doc, l1) {
            return Target::Lines(l1, l2);
        }
    }
    Target::Chars(s..e)
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
            if !inner && k == 0 {
                // On blanks: the blanks and the word after them.
                if let Some(c) = char_at(doc, b).filter(|c| *c != '\n') {
                    let kb = class(c, big);
                    while b < e && char_at(doc, b).is_some_and(|ch| class(ch, big) == kb) {
                        b += char_at(doc, b).map_or(1, char::len_utf8);
                    }
                }
                return Some(Target::Chars(a..b));
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
        '"' | '\'' | '`' => objects::current_quote(doc, pos, c, !inner, 1).map(Target::Chars),
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
            // Not inside a pair: the next one after the cursor, past the
            // closing brackets of pairs opened before it (Vim 9).
            let a = match a {
                Some(a) => a,
                None => {
                    let mut depth = 0i32;
                    let from = (pos + char_at(doc, pos).map_or(0, char::len_utf8)).min(text.len());
                    let mut found = None;
                    for (i, ch) in text[from..].char_indices() {
                        if ch == close {
                            depth += 1;
                        } else if ch == open {
                            if depth == 0 {
                                found = Some(from + i);
                                break;
                            }
                            depth -= 1;
                        }
                    }
                    found?
                }
            };
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
            if !inner {
                return Some(Target::Chars(a..b + 1));
            }
            // Inside a block written over lines: the lines between the
            // braces' lines, not the line break after `{` nor the indent
            // before `}`.
            let mut start = a + 1;
            let mut end = b;
            let after_open = &text[start..line_end(doc, line_of(doc, start))];
            if after_open.trim().is_empty() && line_of(doc, b) > line_of(doc, a) {
                start = line_end(doc, line_of(doc, start)) + 1;
                let close_line = line_of(doc, b);
                if text[line_start(doc, close_line)..b].trim().is_empty() {
                    end = line_start(doc, close_line);
                    if end > start {
                        return Some(Target::Lines(line_of(doc, start), close_line - 1));
                    }
                }
            }
            Some(Target::Chars(start..end.max(start)))
        }
        's' | 'p' => vim_object(doc, pos, c, inner, 1),
        't' => {
            let (open, close) = tag_around(text, pos)?;
            Some(Target::Chars(if inner {
                open.end..close.start
            } else {
                open.start..close.end
            }))
        }
        'h' | 'R' | 'i' | 'c' | 'e' => org_object(doc, pos, c, inner),
        _ => None,
    }
}

/// Text object `c` with a count: `2aw` two words (and their blanks),
/// `2i(` the second pair of parentheses out.
/// `n` sentences (`is`, `as`) or paragraphs (`ip`, `ap`) at `pos` as Vim
/// takes them (see [`objects`]), the sentences' range as it is, before
/// an operator's rules for its end.
fn vim_object(doc: &DocumentState, pos: usize, c: char, inner: bool, n: usize) -> Option<Target> {
    if c == 'p' {
        return objects::current_par(doc, line_of(doc, pos), None, n, !inner)
            .map(|(a, b)| Target::Lines(a, b));
    }
    match objects::current_sent(doc, pos, None, n, !inner) {
        objects::Taken::Range {
            start,
            end,
            inclusive,
        } => Some(Target::Chars(
            start..if inclusive && char_at(doc, end) != Some('\n') {
                doc.grapheme_after(end).max(end)
            } else {
                end
            },
        )),
        objects::Taken::Visual { .. } => None,
    }
}

fn object_n(doc: &DocumentState, pos: usize, c: char, inner: bool, n: usize) -> Option<Target> {
    if matches!(c, 's' | 'p') {
        return vim_object(doc, pos, c, inner, n);
    }
    if matches!(c, '"' | '\'' | '`') {
        return objects::current_quote(doc, pos, c, !inner, n).map(Target::Chars);
    }
    let first = object(doc, pos, c, inner)?;
    if n <= 1 {
        return Some(first);
    }
    match (c, first) {
        ('w' | 'W', Target::Chars(mut r)) => {
            for _ in 1..n {
                let Some(Target::Chars(next)) = object(doc, r.end, c, inner) else {
                    break;
                };
                if next.end <= r.end {
                    break;
                }
                r.end = next.end;
            }
            Some(Target::Chars(r))
        }
        (_, Target::Chars(mut r)) if "()b[]{}B<>t".contains(c) => {
            // Outward, a pair at a time; fewer pairs than the count around
            // the cursor: the object fails, as in Vim.
            for _ in 1..n {
                let Target::Chars(o) = object(doc, r.start, c, false)? else {
                    return None;
                };
                let start = r.start.checked_sub(1)?;
                let Some(Target::Chars(next)) =
                    object(doc, start.min(o.start.saturating_sub(1)), c, inner)
                else {
                    return None;
                };
                if next.start >= r.start {
                    return None;
                }
                r = next;
            }
            Some(Target::Chars(r))
        }
        (_, t) => Some(t),
    }
}

/// The innermost element `<tag>…</tag>` around `pos`: its opening and
/// closing tags' ranges.
fn tag_around(text: &str, pos: usize) -> Option<(Range<usize>, Range<usize>)> {
    let bytes = text.as_bytes();
    let mut stack: Vec<(String, Range<usize>)> = Vec::new();
    let mut best: Option<(Range<usize>, Range<usize>)> = None;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let Some(close) = text[i..].find('>').map(|j| i + j) else {
            break;
        };
        let inside = &text[i + 1..close];
        let closing = inside.starts_with('/');
        let name: String = inside
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
            .collect();
        let range = i..close + 1;
        if name.is_empty() || inside.ends_with('/') {
            i = close + 1;
            continue;
        }
        if closing {
            if let Some(k) = stack.iter().rposition(|(n, _)| *n == name) {
                let (_, open) = stack.remove(k);
                stack.truncate(k);
                if open.start <= pos && pos < range.end {
                    let smaller = best
                        .as_ref()
                        .is_none_or(|(o, c)| open.start >= o.start && range.end <= c.end);
                    if smaller {
                        best = Some((open, range.clone()));
                    }
                }
            }
        } else {
            stack.push((name, range.clone()));
        }
        i = close + 1;
    }
    best
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
            search_offset: None,
            op_start: None,
            change_at: None,
            force: None,
            failed: false,
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
            options: Options::default(),
            insert_pending: insert::InsertPending::None,
            completion: None,
            last_inserted: None,
            ctrl_o: None,
            ai_line: None,
            insert_count: 1,
            insert_repeat: insert::Repeat::Here,
            replaced: Vec::new(),
            macro_rec: None,
            last_macro: None,
            macro_depth: 0,
            last_ex: None,
            line_undo: None,
            insert_line: None,
            last_visual: None,
            jumping: false,
            last_sub: None,
            put_linewise: false,
            block_end: false,
            search_count: 1,
            storing_block: false,
            block_corner: None,
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

    /// The moving end of the visual selection, in visual mode.
    pub fn visual_cursor(&self) -> Option<usize> {
        self.visual().then_some(self.cursor)
    }

    /// Moves the moving end of the visual selection to `to`: for the
    /// editor's own motion keys (Page Down, Home, End, the arrows with
    /// Option or fn), which grow the selection in visual mode as Vim's
    /// `<PageDown>` does, rather than ending it. `false` outside visual
    /// mode.
    pub fn move_visual(&mut self, doc: &mut DocumentState, to: usize) -> bool {
        if !self.visual() {
            return false;
        }
        self.cursor = to.min(doc.text().len());
        doc.selection = self.visual_selection(doc);
        true
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
        if self.macro_depth == 0 {
            self.failed = false;
        }
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
        // An error said is a failure too (a pattern not found).
        if out.message.as_ref().is_some_and(|m| m.1) {
            self.failed = true;
        }
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
        // Recording a macro: the keys typed (not those it runs).
        if self.macro_depth == 0
            && let Some((_, keys)) = &mut self.macro_rec
        {
            keys.push(key);
        }
        if self.command_line.is_some() {
            self.command_line_key(doc, key, host, &mut out);
            if self.mode == Mode::Normal {
                self.clamp(doc);
            }
            return out;
        }
        match self.mode {
            Mode::Insert => {
                out.handled = self.insert_key(doc, key, host, &mut out);
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
                // CTRL-O's command cancelled: insert mode again.
                if self.ctrl_o.is_some() && self.idle() && matches!(key, Key::Esc | Key::Ctrl('['))
                {
                    self.back_from_ctrl_o(doc, false);
                    return out;
                }
                let version = doc.version();
                let lines_before = doc.text().line_count();
                let line = line_of(doc, self.cursor);
                let line_col = self.cursor - line_start(doc, line);
                let line_text = {
                    let r = doc.text().line_range(line.min(doc.text().line_count() - 1));
                    doc.text().as_str()[r]
                        .trim_end_matches(['\n', '\r'])
                        .to_string()
                };
                // Measured before the key changes the text.
                let visual_before = self
                    .visual()
                    .then(|| (self.mode, self.anchor, self.cursor, self.visual_size(doc)));
                // Each command its own undo step, with the insert it starts
                // (and a CTRL-O command within that insert).
                if self.idle() && self.keys.is_empty() && self.ctrl_o.is_none() {
                    doc.begin_undo_join();
                }
                self.keys.push(key);
                self.command(doc, key, host, &mut out);
                if self.idle() {
                    self.keys.clear();
                    if !matches!(self.mode, Mode::Insert | Mode::Replace) && self.ctrl_o.is_none() {
                        doc.break_undo_group();
                    }
                }
                // The `.` mark, the change list and `U`'s line.
                if doc.version() != version && key != Key::Char('U') {
                    // Where the change began (`dd`: the line's start).
                    let at = self.change_at.take().unwrap_or(doc.selection.head);
                    self.note_change(doc, at);
                    // `U` keeps a line changed within itself; lines added or
                    // deleted above it lose it (it is elsewhere now).
                    if doc.text().line_count() == lines_before {
                        if self.line_undo.as_ref().map(|l| l.0) != Some(line) {
                            self.line_undo = Some((line, line_text, line_col));
                        }
                    } else if self.line_undo.as_ref().is_some_and(|l| l.0 >= line) {
                        self.line_undo = None;
                    }
                }
                // Leaving visual mode: `'<`, `'>` and `gv`.
                if let Some((mode, a, c, size)) = visual_before
                    && !self.visual()
                {
                    // Where the text changed, the places as near as can be.
                    let (a, c) = (snap(doc, a), snap(doc, c));
                    doc.marks.named.insert('<', a.min(c));
                    doc.marks.named.insert('>', a.max(c));
                    self.last_visual = Some((mode, a, c, size));
                }
                if self.ctrl_o.is_some()
                    && self.idle()
                    && self.command_line.is_none()
                    && self.mode == Mode::Normal
                {
                    self.back_from_ctrl_o(doc, key == Key::Char('$'));
                    return out;
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
        // In the columns the text shows in (a tab is as wide as it shows).
        let ts = self.options.tabstop.max(1);
        let (a, c) = (self.anchor, self.cursor);
        let (la, lc) = (line_of(doc, a), line_of(doc, c));
        let ((a0, a1), (c0, c1)) = (display_span(doc, a, ts), display_span(doc, c, ts));
        let right = if self.block_end {
            usize::MAX
        } else {
            a1.max(c1) + 1
        };
        (la.min(lc), la.max(lc), a0.min(c0), right)
    }

    /// The keys that select what the visual selection covers, from the
    /// cursor, for `.` to do the same to as much text.
    fn visual_keys(&self, doc: &DocumentState) -> Vec<Key> {
        let mut keys = Vec::new();
        let count = |keys: &mut Vec<Key>, n: usize, k: char| {
            if n > 0 {
                if n > 1 {
                    keys.extend(n.to_string().chars().map(Key::Char));
                }
                keys.push(Key::Char(k));
            }
        };
        let (a, c) = (self.anchor.min(self.cursor), self.anchor.max(self.cursor));
        let (la, lc) = (line_of(doc, a), line_of(doc, c));
        match self.mode {
            Mode::VisualLine => {
                keys.push(Key::Char('V'));
                count(&mut keys, lc - la, 'j');
            }
            Mode::VisualBlock => {
                let (first, last, left, right) = self.block(doc);
                keys.push(Key::Ctrl('v'));
                count(&mut keys, last - first, 'j');
                if right == usize::MAX {
                    keys.push(Key::Char('$'));
                } else {
                    count(&mut keys, right - left - 1, 'l');
                }
            }
            _ => {
                keys.push(Key::Char('v'));
                if la == lc {
                    count(&mut keys, doc.text().as_str()[a..c].chars().count(), 'l');
                } else {
                    count(&mut keys, lc - la, 'j');
                    keys.push(Key::Char('0'));
                    count(&mut keys, column(doc, c), 'l');
                }
            }
        }
        keys
    }

    /// Before an operator on the visual selection: the keys `.` repeats
    /// start with the selection's.
    fn prefix_visual_keys(&mut self, doc: &DocumentState) {
        if self.replaying {
            return;
        }
        let mut k = self.visual_keys(doc);
        k.append(&mut self.keys);
        self.keys = k;
    }

    /// The parts of each line the block selection covers, for the
    /// frontends to paint as selected; `None` outside block selection.
    pub fn block_ranges(&self, doc: &DocumentState) -> Option<Vec<Range<usize>>> {
        if self.mode != Mode::VisualBlock {
            return None;
        }
        let (first, last, left, right) = self.block(doc);
        let ts = self.options.tabstop.max(1);
        Some(
            (first..=last.min(last_line(doc)))
                .map(|l| block_part(doc, l, left, right, ts).range)
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
        let ts = self.options.tabstop.max(1);
        let to_line = |l: usize, goal: Option<usize>| {
            let l = l.min(last);
            let to = match goal {
                Some(c) => at_display_col(doc, l, c, ts),
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
            // A CSV grid: the same column of the row shown below or above
            // (a record can span lines, and a filter or a sorted view
            // leaves rows out or orders them anew); an operator still
            // takes the lines.
            Key::Char('j' | 'k') | Key::Down | Key::Up | Key::Ctrl('n' | 'p')
                if self.op.is_none()
                    && host.rich_view()
                    && doc.meta.mode == DocumentMode::Csv =>
            {
                let down = matches!(key, Key::Char('j') | Key::Down | Key::Ctrl('n'));
                let n = isize::try_from(n).unwrap_or(isize::MAX);
                match crate::csv::view_vertical(doc, pos, if down { n } else { -n }) {
                    Some(to) => charwise(to),
                    None => return Some(None),
                }
            }
            // 'whichwrap' `b,s`: Backspace and Space go on to the line
            // before and after.
            Key::Backspace if self.op.is_none() && pos == line_start(doc, line) && line > 0 => {
                let (s, e) = (line_start(doc, line - 1), line_end(doc, line - 1));
                charwise(if e > s { doc.grapheme_before(e) } else { s })
            }
            Key::Char(' ')
                if self.op.is_none()
                    && line < last
                    && (pos == line_end(doc, line)
                        || doc.grapheme_after(pos) >= line_end(doc, line)) =>
            {
                charwise(line_start(doc, line + 1))
            }
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
                // Nowhere to go: the motion fails (and its operator).
                if (down && line >= last) || (!down && line == 0) {
                    return Some(None);
                }
                // The column on screen is kept (tabs and wide characters
                // counted as they show), and the end of lines after `$`.
                // On a tab the cursor shows in its last column (Vim's
                // `w_virtcol` in normal mode).
                let goal = *self.goal.get_or_insert_with(|| {
                    let col = display_col(doc, pos, ts);
                    if char_at(doc, pos) == Some('\t') && self.mode != Mode::Insert {
                        (col / ts + 1) * ts - 1
                    } else {
                        col
                    }
                });
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
            Key::Char('H' | 'L' | 'M') => {
                let (top, bottom) = host.visible_lines().unwrap_or_else(|| {
                    let half = host.page_lines() / 2;
                    (line.saturating_sub(half), line + half)
                });
                let bottom = bottom.min(last);
                let l = match key {
                    Key::Char('H') => (top + n - 1).min(bottom),
                    Key::Char('L') => bottom.saturating_sub(n - 1).max(top),
                    _ => top + (bottom - top) / 2,
                };
                to_line(l, None)
            }
            Key::Char('|') => {
                let ts = self.options.tabstop.max(1);
                let (s, e) = (line_start(doc, line), line_end(doc, line));
                let want = n - 1;
                let mut col = 0;
                let mut p = s;
                for (i, c) in doc.text().as_str()[s..e].char_indices() {
                    let next = if c == '\t' {
                        (col / ts + 1) * ts
                    } else {
                        col + 1
                    };
                    p = s + i;
                    if next > want {
                        break;
                    }
                    col = next;
                    p = s + i + c.len_utf8();
                }
                if self.op.is_none() && p >= e && e > s {
                    p = doc.grapheme_before(e);
                }
                charwise(p)
            }
            Key::Char('(' | ')') => {
                let Some(p) = objects::findsent(doc, pos, key == Key::Char(')'), n) else {
                    return Some(None);
                };
                charwise(p)
            }
            Key::Char('0') => charwise(line_start(doc, line)),
            Key::Char('^') => charwise(first_non_blank(doc, line)),
            Key::Char('$') => {
                // A count on the last line has no line to go down to.
                if n > 1 && line >= last {
                    return Some(None);
                }
                let l = (line + n - 1).min(last);
                let (s, e) = (line_start(doc, l), line_end(doc, l));
                // In visual mode the selection takes the line break too.
                if self.op.is_some() && l > line && e == s {
                    // To an empty line: its end, included (`2d$` there takes
                    // the lines).
                    inclusive(e)
                } else if self.op.is_some() || (self.visual() && self.mode != Mode::VisualBlock) {
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
                // An operator past the last word: to the end of the last
                // line, not over its line break.
                if self.op.is_some() && p >= doc.text().len() {
                    p = line_end(doc, last);
                }
                // Past the last word: its last character.
                if self.op.is_none() {
                    let (s, e) = (line_start(doc, last), line_end(doc, last));
                    let end = if e > s { doc.grapheme_before(e) } else { s };
                    p = p.min(end);
                }
                charwise(p)
            }
            Key::Char('b' | 'B') => {
                // At the start of the text the motion fails, its operator
                // too, the cursor having gone as far as it could.
                let mut p = pos;
                for _ in 0..n {
                    if p == 0 {
                        if self.op.is_some() || p == pos {
                            return Some(None);
                        }
                        break;
                    }
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
                find_char(doc, pos, kind, c, n, true).map(|to| Motion {
                    to,
                    linewise: false,
                    inclusive: matches!(kind, Find::To | Find::Till),
                })
            }
            // `50%`: the line that far into the document.
            Key::Char('%') if count.is_some() => {
                let lines = last + 1;
                let l = (n.min(100) * lines).div_ceil(100).saturating_sub(1);
                to_line(l, None)
            }
            Key::Char('%') => matching_bracket(doc, pos).map(|to| Motion {
                to,
                linewise: false,
                inclusive: true,
            }),
            Key::Char('}' | '{') => {
                let Some((to, last)) = objects::findpar(doc, pos, key == Key::Char('}'), n) else {
                    return Some(None);
                };
                if last { inclusive(to) } else { charwise(to) }
            }
            Key::Char('n' | 'N') => {
                let Some((pat, back)) = self.last_search.clone() else {
                    return Some(None);
                };
                let back = back != (key == Key::Char('N'));
                // From where the match was, as the offset moved the cursor.
                let from = match self.search_offset {
                    Some(SearchOffset::Start(k) | SearchOffset::End(k)) => step_chars(doc, pos, -k),
                    _ => pos,
                };
                self.search_from(doc, &pat, back, from, n)
                    .and_then(|to| self.offset_motion(doc, &pat, to))
            }
            _ => return None,
        };
        if !vertical {
            self.goal = (key == Key::Char('$')).then_some(usize::MAX);
        }
        self.jumping = matches!(
            key,
            Key::Char('G' | '%' | '(' | ')' | '{' | '}' | 'n' | 'N' | 'H' | 'L' | 'M')
        );
        self.motion_count = n;
        Some(m)
    }

    // Registers.

    /// The registers with text, by name, the unnamed `"` first.
    pub fn register_texts(&self) -> Vec<(char, String)> {
        let mut v: Vec<(char, String)> = self
            .registers
            .iter()
            .filter(|(_, r)| !r.text.is_empty())
            .map(|(c, r)| (*c, r.text.clone()))
            .collect();
        v.sort_by_key(|(c, _)| (*c != '"', *c));
        v
    }

    /// Puts yanked or deleted text in the registers as Vim does: the
    /// register named (`"a`, appended to with `"A`), else `"0` for a yank,
    /// `"1` (the older ones moving up to `"9`) for a delete of lines or
    /// across lines and `"-` for a smaller one; the unnamed `""` always,
    /// except for the black hole `"_`.
    fn store(&mut self, text: String, linewise: bool, yank: bool, host: &mut dyn Host) {
        let r = Register {
            text,
            linewise,
            block: std::mem::take(&mut self.storing_block),
        };
        match self.register.take() {
            Some('_') => return,
            Some('+' | '*') => host.set_clipboard(&r.text),
            Some(c) if c.is_ascii_uppercase() => {
                let e = self
                    .registers
                    .entry(c.to_ascii_lowercase())
                    .or_insert(Register {
                        text: String::new(),
                        linewise,
                        block: false,
                    });
                if linewise && !e.linewise && !e.text.is_empty() {
                    e.text.push('\n');
                }
                e.text.push_str(&r.text);
                e.linewise |= linewise;
                let all = e.clone();
                crate::command::record_history(&all.text);
                self.registers.insert('"', all);
                return;
            }
            Some(c) if c.is_ascii_lowercase() => {
                self.registers.insert(c, r.clone());
            }
            _ if yank => {
                self.registers.insert('0', r.clone());
            }
            _ if linewise || r.text.contains('\n') => {
                for i in (1..9).rev() {
                    let from = char::from(b'0' + i);
                    if let Some(old) = self.registers.remove(&from) {
                        self.registers.insert(char::from(b'0' + i + 1), old);
                    }
                }
                self.registers.insert('1', r.clone());
            }
            _ => {
                self.registers.insert('-', r.clone());
            }
        }
        crate::command::record_history(&r.text);
        self.registers.insert('"', r);
    }

    fn fetch(&mut self, host: &mut dyn Host) -> Option<Register> {
        match self.register.take() {
            Some('+' | '*') => host.clipboard().map(|text| {
                let linewise = text.ends_with('\n');
                Register {
                    text,
                    linewise,
                    block: false,
                }
            }),
            Some('.') => self.last_inserted.clone().map(|text| Register {
                text,
                linewise: false,
                block: false,
            }),
            Some(':') => self.last_ex.clone().map(|text| Register {
                text,
                linewise: false,
                block: false,
            }),
            Some('/') => self.last_search.clone().map(|(text, _)| Register {
                text,
                linewise: false,
                block: false,
            }),
            Some('_') => None,
            Some(c) => self.registers.get(&c.to_ascii_lowercase()).cloned(),
            None => self.registers.get(&'"').cloned(),
        }
    }

    // Insert mode.

    fn enter_insert(&mut self, doc: &mut DocumentState, at: usize) {
        doc.selection = Selection::caret(at);
        self.mode = Mode::Insert;
        self.insert_at = Some(at);
        self.cursor = at;
        let line = line_of(doc, at);
        let s = line_start(doc, line);
        let text = doc.text().as_str()[s..line_end(doc, line)].to_string();
        self.insert_line = Some((line, text, doc.text().line_count(), at - s));
    }

    fn leave_insert(&mut self, doc: &mut DocumentState) {
        self.mode = Mode::Normal;
        self.insert_pending = insert::InsertPending::None;
        self.completion = None;
        self.ctrl_o = None;
        let mut head = doc.selection.head.min(doc.text().len());
        // An indent 'autoindent' gave that nothing was typed after goes.
        if let Some(s) = self.ai_line.take()
            && s <= head
            && line_start(doc, line_of(doc, s)) == s
        {
            let e = line_end(doc, line_of(doc, s));
            if e > s && head >= s && head <= e && doc.text().as_str()[s..e].trim().is_empty() {
                edit(doc, s..e, "", s);
                head = s;
            }
        }
        doc.marks.named.insert('^', head);
        if let Some(at) = self.insert_at.filter(|at| head > *at) {
            doc.marks.named.insert('[', at);
            doc.marks.named.insert(']', head);
        }
        let typed = self
            .insert_at
            .filter(|at| head >= *at)
            .map(|at| doc.text().as_str()[at..head].to_string());
        if let Some(t) = &typed {
            self.last_inserted = Some(t.clone());
            // `'.` and the change list: where the last character went;
            // `U`: the line as it was, typed in alone.
            if !t.is_empty() {
                let at = doc.grapheme_before(head);
                self.note_change(doc, at);
                if let Some((l, text, lines, col)) = self.insert_line.take()
                    && lines == doc.text().line_count()
                    && self.line_undo.as_ref().map(|u| u.0) != Some(l)
                {
                    self.line_undo = Some((l, text, col));
                }
            }
        }
        // `3i`: the text typed twice more; `3o`: on two more lines.
        let count = std::mem::replace(&mut self.insert_count, 1);
        if count > 1
            && let Some(t) = typed.as_ref().filter(|t| !t.is_empty())
        {
            let more = match self.insert_repeat {
                insert::Repeat::Here => t.repeat(count - 1),
                insert::Repeat::Lines => {
                    let line = line_of(doc, head);
                    let s = line_start(doc, line);
                    let at = self.insert_at.unwrap_or(s).max(s);
                    let indent = doc.text().as_str()[s..at].to_string();
                    format!("\n{indent}{t}").repeat(count - 1)
                }
            };
            let at = if self.insert_repeat == insert::Repeat::Lines {
                line_end(doc, line_of(doc, head))
            } else {
                head
            };
            edit(doc, at..at, &more, at + more.len());
            head = at + more.len();
        }
        if let (Some((lines, col, pad)), Some(at)) = (self.block_insert.take(), self.insert_at)
            && head > at
            && !doc.text().as_str()[at..head].contains('\n')
        {
            // The typed text again on the block's other lines.
            let typed = doc.text().as_str()[at..head].to_string();
            let mut tx = Transaction::new("Vim");
            let ts = self.options.tabstop.max(1);
            for l in lines.filter(|l| *l <= last_line(doc)) {
                let e = line_end(doc, l);
                let width = display_col(doc, e, ts);
                if col == usize::MAX {
                    let _ = tx.insert(e, typed.clone());
                    continue;
                }
                if width < col {
                    if pad {
                        let fill = " ".repeat(col - width);
                        let _ = tx.insert(e, format!("{fill}{typed}"));
                    }
                    continue;
                }
                let _ = tx.insert(block_part(doc, l, col, usize::MAX, ts).range.start, typed.clone());
            }
            let tx = tx.select(Selection::caret(head));
            doc.apply(&tx, ChangeKind::Command, Instant::now());
        }
        if let Some(mut c) = self.recording.take() {
            if typed.is_some() {
                c.inserted = typed.clone();
            }
            if !self.replaying {
                self.last_change = Some(c);
            }
        }
        self.insert_at = None;
        doc.break_undo_group();
        if let Some(corner) = self.block_corner.take() {
            // A block insert ends at the block's top left corner.
            doc.selection = Selection::caret(corner.min(doc.text().len()));
        } else if head > line_start(doc, line_of(doc, head)) {
            doc.selection = Selection::caret(doc.grapheme_before(head));
        }
        self.clamp(doc);
    }

    /// Insert mode again after CTRL-O's command; at the end of the line
    /// when the cursor was there and stayed, or after `$`.
    fn back_from_ctrl_o(&mut self, doc: &mut DocumentState, dollar: bool) {
        let Some(c) = self.ctrl_o.take() else {
            return;
        };
        let mut at = doc.selection.head.min(doc.text().len());
        let e = line_end(doc, line_of(doc, at));
        // After the end of the line before CTRL-O, and on its last
        // character now (the same line): after it again.
        let same_line = line_of(doc, at) == c.line;
        if dollar || (c.eol && same_line && at < e && doc.grapheme_after(at) >= e) {
            at = e;
        }
        self.mode = Mode::Insert;
        doc.selection = Selection::caret(at);
        self.cursor = at;
        self.insert_at = Some(at);
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
        // `'[` and `']`: the text operated on (moving with the edit).
        let (a, b) = match &target {
            Target::Lines(l1, l2) => (line_start(doc, *l1), line_end(doc, *l2)),
            Target::Chars(r) => (
                r.start.min(len),
                doc.grapheme_before(r.end.min(len)).max(r.start.min(len)),
            ),
            Target::Block {
                first,
                last,
                left,
                right,
            } => {
                let ts = self.options.tabstop.max(1);
                (
                    at_display_col(doc, *first, *left, ts),
                    at_display_col(doc, (*last).min(last_line(doc)), right.saturating_sub(1), ts),
                )
            }
        };
        doc.marks.named.insert('[', a);
        doc.marks.named.insert(']', b);
        // The cursor where Vim has it as the change is made: undo puts it
        // back there.
        let at = self.op_start.take().unwrap_or(a).min(len);
        if op != Op::Yank {
            doc.selection = Selection::caret(at);
            self.change_at = Some(a);
        }
        if op == Op::Reindent {
            let (l1, l2) = (line_of(doc, a), line_of(doc, b));
            return self.reindent(doc, l1, l2);
        }
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
                        self.store(lines, true, op == Op::Yank, host);
                        let caret = if line_of(doc, self.cursor) == l1 {
                            self.cursor
                        } else {
                            first_non_blank(doc, l1)
                        };
                        doc.selection = Selection::caret(caret);
                    }
                    Op::Delete => {
                        self.store(lines, true, op == Op::Yank, host);
                        let span = line_span(doc, l1, l2);
                        edit(doc, span.clone(), "", span.start);
                        let l = l1.min(last_line(doc));
                        doc.selection = Selection::caret(first_non_blank(doc, l));
                    }
                    Op::Change => {
                        self.store(lines, true, op == Op::Yank, host);
                        // With 'autoindent' the first line's indent stays,
                        // gone again when nothing is typed after it.
                        let ls = line_start(doc, l1);
                        let s = if self.options.autoindent {
                            first_non_blank(doc, l1)
                        } else {
                            ls
                        };
                        let e = line_end(doc, l2);
                        edit(doc, s..e, "", s);
                        self.enter_insert(doc, s);
                        if s > ls {
                            self.ai_line = Some(ls);
                        }
                    }
                    Op::Indent | Op::Outdent | Op::Reindent => {
                        self.shift_lines(doc, l1, l2, op == Op::Indent, out)
                    }
                    Op::Lower | Op::Upper | Op::Toggle | Op::Rot13 => {
                        let r = line_start(doc, l1)..line_end(doc, l2);
                        self.change_case(doc, op, r);
                        // Where the operator began (`g~j` keeps the column).
                        doc.selection = Selection::caret(at.min(line_end(doc, l1)));
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
                let ts = self.options.tabstop.max(1);
                let parts: Vec<BlockPart> =
                    (first..=last).map(|l| block_part(doc, l, left, right, ts)).collect();
                let ranges: Vec<Range<usize>> = parts.iter().map(|p| p.range.clone()).collect();
                let top = ranges[0].start + parts[0].pre;
                // What is in the block: a tab at an edge as the spaces of it
                // inside.
                let text: Vec<String> = parts
                    .iter()
                    .map(|p| {
                        let t = &doc.text().as_str()[p.range.clone()];
                        if p.pre == 0 && p.post == 0 {
                            return t.to_string();
                        }
                        let width = |c: char, col: usize| {
                            use unicode_width::UnicodeWidthChar;
                            if c == '\t' { (col / ts + 1) * ts - col } else { c.width().unwrap_or(0) }
                        };
                        let mut col = display_col(doc, p.range.start, ts);
                        let n = t.chars().count();
                        let mut out = String::new();
                        for (i, c) in t.chars().enumerate() {
                            let w = width(c, col);
                            let cut = if i == 0 { p.pre } else { 0 } + if i + 1 == n { p.post } else { 0 };
                            if (i == 0 && p.pre > 0) || (i + 1 == n && p.post > 0) {
                                out.push_str(&" ".repeat(w.saturating_sub(cut)));
                            } else {
                                out.push(c);
                            }
                            col += w;
                        }
                        out
                    })
                    .collect();
                let text = text.join("\n");
                self.storing_block = true;
                match op {
                    Op::Yank => {
                        self.store(text, false, op == Op::Yank, host);
                        doc.selection = Selection::caret(top);
                    }
                    Op::Delete | Op::Change => {
                        self.store(text, false, op == Op::Yank, host);
                        // A tab at an edge leaves the spaces of it outside.
                        let mut tx = Transaction::new("Vim");
                        for p in &parts {
                            let _ = tx.edit(p.range.clone(), " ".repeat(p.pre + p.post));
                        }
                        let tx = tx.select(Selection::caret(top));
                        doc.apply(&tx, ChangeKind::Command, Instant::now());
                        if op == Op::Change {
                            self.enter_insert(doc, top);
                            self.block_insert = Some((first + 1..last + 1, left, false));
                            self.block_corner = Some(top);
                        }
                    }
                    Op::Indent | Op::Outdent | Op::Reindent => {
                        self.shift_lines(doc, first, last, op == Op::Indent, out)
                    }
                    Op::Lower | Op::Upper | Op::Toggle | Op::Rot13 => {
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
                        self.store(text, false, op == Op::Yank, host);
                        doc.selection = Selection::caret(r.start);
                    }
                    Op::Delete | Op::Change => {
                        self.store(text, false, op == Op::Yank, host);
                        edit(doc, r.clone(), "", r.start);
                        if op == Op::Change {
                            self.enter_insert(doc, r.start);
                        }
                    }
                    Op::Indent | Op::Outdent | Op::Reindent => {
                        let (l1, l2) = (
                            line_of(doc, r.start),
                            line_of(doc, r.end.saturating_sub(1).max(r.start)),
                        );
                        self.shift_lines(doc, l1, l2, op == Op::Indent, out);
                    }
                    Op::Lower | Op::Upper | Op::Toggle | Op::Rot13 => {
                        self.change_case(doc, op, r.clone());
                        doc.selection = Selection::caret(r.start);
                    }
                }
            }
        }
    }

    /// `r` in another case (or ROT13) as `op` says; the length of the
    /// new text, which can differ (`ß` up is `SS`).
    fn change_case(&mut self, doc: &mut DocumentState, op: Op, r: Range<usize>) -> usize {
        let s = doc.text().as_str()[r.clone()].to_string();
        let mut new = String::with_capacity(s.len());
        for c in s.chars() {
            match op {
                Op::Lower => new.push(lower_char(c)),
                // Vim's one exception to a character for a character.
                Op::Upper if c == 'ß' => new.push_str("SS"),
                Op::Upper => new.push(upper_char(c)),
                Op::Rot13 => new.push(match c {
                    'a'..='z' => (((c as u8 - b'a') + 13) % 26 + b'a') as char,
                    'A'..='Z' => (((c as u8 - b'A') + 13) % 26 + b'A') as char,
                    c => c,
                }),
                _ if lower_char(c) != c => new.push(lower_char(c)),
                _ => new.push(upper_char(c)),
            }
        }
        if new != s {
            edit(doc, r.clone(), &new, r.start);
        }
        new.len()
    }

    /// `=` with no 'equalprg' nor 'indentexpr': Vim's C indenting, here
    /// by braces: a line one 'shiftwidth' in for each `{` open before
    /// it, out again on a line that starts with `}`; empty lines stay
    /// empty.
    fn reindent(&mut self, doc: &mut DocumentState, l1: usize, l2: usize) {
        let ts = self.options.tabstop.max(1);
        let sw = self.shiftwidth();
        // Braces open before the first line (not in strings).
        let depth_of = |t: &str| -> isize {
            let mut d = 0isize;
            let mut quote = None;
            let mut prev = '\0';
            for c in t.chars() {
                match (quote, c) {
                    (Some(q), c) if c == q && prev != '\\' => quote = None,
                    (Some(_), _) => {}
                    (None, '"' | '\'') => quote = Some(c),
                    (None, '{') => d += 1,
                    (None, '}') => d -= 1,
                    _ => {}
                }
                prev = c;
            }
            d
        };
        let mut depth = depth_of(&doc.text().as_str()[..line_start(doc, l1)]).max(0);
        for l in l1..=l2.min(last_line(doc)) {
            let (s, fnb, e) = (
                line_start(doc, l),
                first_non_blank(doc, l),
                line_end(doc, l),
            );
            let body = doc.text().as_str()[fnb..e].to_string();
            let here = if body.starts_with('}') {
                (depth - 1).max(0)
            } else {
                depth
            };
            if fnb < e {
                let want = insert::indent_string(here as usize * sw, ts, self.options.expandtab);
                if doc.text().as_str()[s..fnb] != want {
                    edit(doc, s..fnb, &want, s);
                }
            }
            depth = (depth + depth_of(&body)).max(0);
        }
        let p = first_non_blank(doc, l1);
        doc.selection = Selection::caret(p);
    }

    /// `>>` and `<<`: in Org documents headlines and list items as Org
    /// does it, other lines by 'shiftwidth'.
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
            } else {
                // 'shiftwidth' columns more or less; empty lines stay.
                let ts = self.options.tabstop.max(1);
                let sw = self.shiftwidth();
                let blank = line.len() - line.trim_start_matches([' ', '\t']).len();
                let width = insert::vcol(doc, s + blank, ts);
                let new = if right {
                    width + sw
                } else {
                    width.saturating_sub(sw)
                };
                if !line.is_empty() && new != width {
                    let indent = insert::indent_string(new, ts, self.options.expandtab);
                    edit(doc, s..s + blank, &indent, s);
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
        self.put_linewise = r.linewise;
        let (start, added) = self.put_text(doc, &r, before, count, pos, line);
        // From insert mode (CTRL-O), the cursor after the text put.
        if self.ctrl_o.is_some() {
            let end = (start + added).min(doc.text().len());
            doc.selection = Selection::caret(end);
            self.cursor = end;
        }
        // `'[` and `']`: the first and the last character put.
        doc.marks.named.insert('[', start);
        let end = start + added;
        doc.marks.named.insert(
            ']',
            if r.linewise {
                end.min(doc.text().len())
            } else {
                doc.grapheme_before(end).max(start)
            },
        );
    }

    /// `]p` and `[p`: lines put with the indent of the cursor's line, their
    /// own indents kept relative to the first (empty lines without one);
    /// other text as `p` and `P` put it.
    fn put_indented(&mut self, doc: &mut DocumentState, before: bool, count: usize, host: &mut dyn Host) {
        let Some(mut r) = self.fetch(host) else { return };
        if r.linewise {
            let ts = self.options.tabstop.max(1);
            let width = |l: &str| {
                l.chars().take_while(|c| *c == ' ' || *c == '\t').fold(0, |col, c| {
                    if c == '\t' { (col / ts + 1) * ts } else { col + 1 }
                })
            };
            let line = line_of(doc, self.cursor);
            let want = insert::vcol(doc, first_non_blank(doc, line), ts) as isize;
            let body = r.text.strip_suffix('\n').unwrap_or(&r.text).to_string();
            let first = body.split('\n').find(|l| !l.is_empty()).map_or(0, width) as isize;
            let lines: Vec<String> = body
                .split('\n')
                .map(|l| {
                    if l.is_empty() {
                        return String::new();
                    }
                    let w = (width(l) as isize + want - first).max(0) as usize;
                    let rest = l.trim_start_matches([' ', '\t']);
                    format!("{}{rest}", insert::indent_string(w, ts, self.options.expandtab))
                })
                .collect();
            r.text = format!("{}\n", lines.join("\n"));
        }
        let pos = self.cursor;
        let line = line_of(doc, pos);
        self.put_linewise = r.linewise;
        let (start, added) = self.put_text(doc, &r, before, count, pos, line);
        doc.marks.named.insert('[', start);
        doc.marks.named.insert(']', (start + added).min(doc.text().len()));
    }

    /// Puts register `r`; where the text went and its length.
    fn put_text(
        &mut self,
        doc: &mut DocumentState,
        r: &Register,
        before: bool,
        count: usize,
        pos: usize,
        line: usize,
    ) -> (usize, usize) {
        if r.block {
            // A block: each of its lines on a line of its own, from the
            // cursor's column (after it for `p`), the lines padded.
            let col = column(doc, pos) + usize::from(!before && pos < line_end(doc, line));
            let parts: Vec<String> = r.text.split('\n').map(|p| p.repeat(count)).collect();
            let mut tx = Transaction::new("Vim");
            let lines_now = last_line(doc);
            let mut tail = String::new();
            for (i, part) in parts.iter().enumerate() {
                let l = line + i;
                if l > lines_now {
                    tail.push_str(&format!("\n{}{part}", " ".repeat(col)));
                    continue;
                }
                let e = line_end(doc, l);
                let width = doc.text().as_str()[line_start(doc, l)..e].chars().count();
                if width < col {
                    let _ = tx.insert(e, format!("{}{part}", " ".repeat(col - width)));
                } else {
                    let _ = tx.insert(at_column(doc, l, col), part.clone());
                }
            }
            if !tail.is_empty() {
                let end = line_end(doc, lines_now);
                let _ = tx.insert(end, tail);
            }
            let start = at_column(doc, line, col);
            let tx = tx.select(Selection::caret(start));
            doc.apply(&tx, ChangeKind::Command, Instant::now());
            let start = at_column(doc, line, col);
            doc.selection = Selection::caret(start);
            return (start, parts.first().map_or(0, String::len));
        }
        if r.linewise {
            let mut t = r.text.clone();
            if !t.ends_with('\n') {
                t.push('\n');
            }
            let t = t.repeat(count);
            // Above the one empty line of an empty text: it stays below.
            if before && doc.text().is_empty() {
                edit(doc, 0..0, &format!("{t}\n"), 0);
                doc.selection = Selection::caret(first_non_blank(doc, 0));
                return (0, t.len());
            }
            let at = if before {
                line_start(doc, line)
            } else if line + 1 < doc.text().line_count() {
                line_start(doc, line + 1)
            } else {
                // After a last line without a line feed (an empty text:
                // its one empty line, the lines put after it ending as
                // lines do).
                let len = doc.text().len();
                if len == 0 {
                    edit(doc, 0..0, &format!("\n{t}"), 1);
                    doc.selection = Selection::caret(first_non_blank(doc, 1));
                    return (1, t.len());
                }
                let body = t.strip_suffix('\n').unwrap_or(&t).to_string();
                edit(doc, len..len, &format!("\n{body}"), len + 1);
                let l = line_of(doc, len + 1);
                doc.selection = Selection::caret(first_non_blank(doc, l));
                return (len + 1, body.len());
            };
            edit(doc, at..at, &t, at);
            doc.selection = Selection::caret(first_non_blank(doc, line_of(doc, at)));
            (at, t.len())
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
            // Several lines: the cursor at their start.
            let caret = if t.contains('\n') {
                at
            } else {
                doc.grapheme_before(end).max(at)
            };
            doc.selection = Selection::caret(caret);
            (at, t.len())
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
            let before = &doc.text().as_str()[line_start(doc, line)..e];
            let sep = if next_empty
                || this_empty
                || before.ends_with([' ', '\t'])
                || doc.text().as_str()[next..].starts_with(')')
            {
                ""
            } else if self.options.joinspaces && before.ends_with(['.', '!', '?']) {
                "  "
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
                    && (c.is_ascii_alphanumeric() || "\"+*-_.:/".contains(c))
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
                let m = find_char(doc, self.cursor, kind, c, n, false).map(|to| Motion {
                    to,
                    linewise: false,
                    inclusive: matches!(kind, Find::To | Find::Till),
                });
                return self.finish_motion(doc, m, host, out);
            }
            Pending::Replace => {
                self.pending = Pending::None;
                // `r<CR>`: the characters become one line break.
                if key == Key::Enter && !self.visual() {
                    let n = self.count.take().unwrap_or(1);
                    let pos = self.cursor;
                    let line = line_of(doc, pos);
                    let e = line_end(doc, line);
                    let mut end = pos;
                    for _ in 0..n {
                        if end >= e {
                            return self.reset();
                        }
                        end = doc.grapheme_after(end);
                    }
                    // With 'autoindent' the blanks after go too.
                    if self.options.autoindent {
                        while end < e && matches!(doc.text().as_str().as_bytes()[end], b' ' | b'\t') {
                            end += 1;
                        }
                    }
                    let indent = if self.options.autoindent {
                        self.indent_like(doc, first_non_blank(doc, line))
                    } else {
                        String::new()
                    };
                    // Blanks before go too, as Enter in insert mode does.
                    let mut from = pos;
                    while from > line_start(doc, line)
                        && matches!(doc.text().as_str().as_bytes()[from - 1], b' ' | b'\t')
                    {
                        from -= 1;
                    }
                    let insert = format!("\n{indent}");
                    let caret = from + insert.len();
                    edit(doc, from..end, &insert, caret);
                    self.cursor = caret;
                    self.changed();
                    return self.reset();
                }
                let Key::Char(c) = key else {
                    return self.reset();
                };
                if self.visual() {
                    self.prefix_visual_keys(doc);
                    // Every character selected becomes `c`.
                    let t = self.visual_target(doc);
                    self.mode = Mode::Normal;
                    let ranges: Vec<Range<usize>> = match t {
                        Target::Chars(r) => Vec::from([r]),
                        Target::Lines(a, b) => {
                            let r = line_start(doc, a)..line_end(doc, b);
                            Vec::from([r])
                        }
                        Target::Block {
                            first,
                            last,
                            left,
                            right,
                        } => (first..=last.min(last_line(doc)))
                            .map(|l| {
                                block_part(doc, l, left, right, self.options.tabstop.max(1)).range
                            })
                            .collect(),
                    };
                    let top = ranges.first().map_or(self.cursor, |r| r.start);
                    for r in ranges.into_iter().rev() {
                        let old = doc.text().as_str()[r.clone()].to_string();
                        let new: String =
                            old.chars().map(|x| if x == '\n' { x } else { c }).collect();
                        edit(doc, r, &new, top);
                    }
                    doc.selection = Selection::caret(top);
                    self.cursor = top;
                    self.changed();
                    return self.reset();
                }
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
            Pending::Bracket(forward) => {
                self.pending = Pending::None;
                let Key::Char(c) = key else {
                    return self.reset();
                };
                let n = match (self.count.take(), self.op) {
                    (Some(c), Some((_, oc))) => c * oc.max(1),
                    (None, Some((_, oc))) if oc > 0 => oc,
                    (c, _) => c.unwrap_or(1),
                };
                match (forward, c) {
                    // `[(`, `[{`, `])`, `]}`: a bracket not matched.
                    (false, '(' | '{') | (true, ')' | '}') => {
                        let m = objects::unmatched(doc, self.cursor, c, n).map(|to| Motion {
                            to,
                            linewise: false,
                            inclusive: false,
                        });
                        self.finish_motion(doc, m, host, out);
                    }
                    // `[[` and `]]`: a `{` starting a line; `[]` and `][`:
                    // a `}`. Without an operator, at the first non-blank.
                    (_, '[' | ']') => {
                        let what = if (c == ']') == forward { '{' } else { '}' };
                        let both = self.op.is_some() && forward && what == '{';
                        let alone = self.op.is_none();
                        let m = objects::findpar_of(doc, self.cursor, forward, n, Some(what), both)
                            .map(|(to, inclusive)| Motion {
                                to: if alone {
                                    first_non_blank(doc, line_of(doc, to))
                                } else {
                                    to
                                },
                                linewise: false,
                                inclusive: inclusive && !alone,
                            });
                        self.jumping = m.is_some();
                        self.finish_motion(doc, m, host, out);
                    }
                    // `]h` and `[h` in Org: the next or previous heading
                    // of the same level, as Doom Emacs has them.
                    (_, 'h') if doc.meta.mode == crate::DocumentMode::Org => {
                        let m =
                            org::heading_motion(doc, self.cursor, forward, n).map(|to| Motion {
                                to,
                                linewise: false,
                                inclusive: false,
                            });
                        self.jumping = m.is_some();
                        self.finish_motion(doc, m, host, out);
                    }
                    // `]p`: put after with this line's indent; `[p`, `[P`,
                    // `]P` before.
                    (_, 'p' | 'P') if self.op.is_none() && !self.visual() => {
                        self.begin_change();
                        self.put_indented(doc, !(forward && c == 'p'), n, host);
                        self.changed();
                        self.reset();
                    }
                    _ => {
                        self.failed = true;
                        self.reset();
                    }
                }
                return;
            }
            Pending::G => {
                self.pending = Pending::None;
                match key {
                    Key::Char('g') => {
                        self.jumping = true;
                        let n = self.count.take();
                        let l = n.map_or(0, |c| c.saturating_sub(1)).min(last_line(doc));
                        let m = Some(Motion {
                            to: first_non_blank(doc, l),
                            linewise: true,
                            inclusive: false,
                        });
                        self.finish_motion(doc, m, host, out);
                    }
                    // Doom's `gd` and `gD` (`+lookup/definition`,
                    // `+lookup/references`), where a language server
                    // serves the file.
                    Key::Char(c @ ('d' | 'D'))
                        if !self.visual() && self.op.is_none() && crate::lsp::serves(doc) =>
                    {
                        self.count = None;
                        let id = if c == 'd' {
                            "code.definition"
                        } else {
                            "code.references"
                        };
                        out.commands.push((id.into(), Value::Null));
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
                    Key::Char(c @ ('e' | 'E' | '_' | 'o' | 'm' | 'M')) => {
                        let count = match (self.count.take(), self.op) {
                            (Some(c), Some((_, oc))) => Some(c * oc.max(1)),
                            (None, Some((_, oc))) if oc > 0 => Some(oc),
                            (c, _) => c,
                        };
                        let m = self.g_motion(doc, c, count, host);
                        self.finish_motion(doc, m, host, out);
                    }
                    Key::Char('J') if !self.visual() && self.op.is_none() => {
                        let n = self.count.take().unwrap_or(1);
                        self.begin_change();
                        self.join_raw(doc, line_of(doc, self.cursor), n);
                        self.changed();
                    }
                    // `g-` and `g+`: back and on in time, as `u` and CTRL-R
                    // here, the history having no branches.
                    Key::Char(c @ ('-' | '+')) if !self.visual() && self.op.is_none() => {
                        let n = self.count.take().unwrap_or(1);
                        for _ in 0..n {
                            let done = if c == '-' { doc.undo() } else { doc.redo_from_start() };
                            if done.is_none() {
                                break;
                            }
                        }
                    }
                    // Visual `gJ`: the lines selected joined as they are.
                    Key::Char('J') if self.visual() => {
                        self.prefix_visual_keys(doc);
                        let (a, c) = (self.anchor.min(self.cursor), self.anchor.max(self.cursor));
                        let (l1, l2) = (line_of(doc, a), line_of(doc, c));
                        self.mode = Mode::Normal;
                        self.count = None;
                        self.join_raw(doc, l1, l2 - l1 + 1);
                        self.changed();
                    }
                    Key::Char('v') if self.op.is_none() => {
                        if let Some((mode, a, c, _)) = self.last_visual {
                            self.mode = mode;
                            self.anchor = snap(doc, a);
                            self.cursor = snap(doc, c);
                        }
                        self.count = None;
                    }
                    Key::Char('i') if !self.visual() && self.op.is_none() => {
                        let n = self.count.take().unwrap_or(1);
                        let at = self.mark(doc, '^').unwrap_or(self.cursor);
                        self.insert_count = n;
                        self.insert_repeat = insert::Repeat::Here;
                        self.begin_change();
                        self.enter_insert(doc, at);
                    }
                    Key::Char('I') if !self.visual() && self.op.is_none() => {
                        let n = self.count.take().unwrap_or(1);
                        let s = line_start(doc, line_of(doc, self.cursor));
                        self.insert_count = n;
                        self.insert_repeat = insert::Repeat::Here;
                        self.begin_change();
                        self.enter_insert(doc, s);
                    }
                    Key::Char(c @ ('p' | 'P')) if !self.visual() && self.op.is_none() => {
                        let n = self.count.take().unwrap_or(1);
                        self.begin_change();
                        self.put(doc, c == 'P', n, host);
                        // The cursor just after the new text.
                        if let Some(&end) = doc.marks.named.get(&']') {
                            let end = end.min(doc.text().len());
                            let to = if self.put_linewise {
                                end
                            } else {
                                doc.grapheme_after(end).min(doc.text().len())
                            };
                            doc.selection = Selection::caret(to);
                            self.cursor = to;
                        }
                        self.changed();
                    }
                    Key::Char(c @ (';' | ',')) if self.op.is_none() => {
                        let n = self.count.take().unwrap_or(1);
                        self.change_list(doc, c == ';', n);
                    }
                    Key::Char(c @ ('*' | '#')) if self.op.is_none() => {
                        // As `*` and `#`, the word not as a whole word.
                        let n = self.count.take().unwrap_or(1);
                        if let Some(to) = self.star_search(doc, c == '#', false, n, out) {
                            self.jump(doc);
                            doc.selection = Selection::caret(to);
                            self.cursor = to;
                        }
                    }
                    Key::Char(c @ ('n' | 'N')) => {
                        self.count = None;
                        self.select_match(doc, c == 'N', host, out);
                    }
                    Key::Char('a') if self.op.is_none() => {
                        self.count = None;
                        if let Some(ch) = char_at(doc, self.cursor).filter(|c| *c != '\n') {
                            let n = ch as u32;
                            out.message =
                                Some((format!("<{ch}> {n}, Hex {n:02x}, Oct {n:03o}"), false));
                        }
                    }
                    Key::Char('&') if self.op.is_none() && self.last_sub.is_none() => {
                        self.count = None;
                        self.failed = true;
                        out.message = Some(("E35: No previous regular expression".into(), true));
                    }
                    Key::Char('&') if self.op.is_none() => {
                        self.count = None;
                        self.begin_change();
                        self.ex(doc, "%s//~/&", host, out);
                        self.changed();
                    }
                    Key::Ctrl(c @ ('a' | 'x')) if self.visual() => {
                        let n = self.count.take().unwrap_or(1) as i64;
                        let t = self.visual_target(doc);
                        self.mode = Mode::Normal;
                        self.visual_increment(doc, t, if c == 'a' { n } else { -n }, true);
                    }
                    // evil-org's `gj`, `gk`, `gh` and `gl` in Org: by
                    // element, up to the one around, into it.
                    Key::Char(c @ ('j' | 'k' | 'h' | 'l'))
                        if doc.meta.mode == crate::DocumentMode::Org =>
                    {
                        let n = self.count.take().unwrap_or(1);
                        let m = match org::element_motion(doc, self.cursor, c, n) {
                            Ok(to) => Some(Motion {
                                to,
                                linewise: false,
                                inclusive: false,
                            }),
                            Err(e) => {
                                out.message = Some((e, true));
                                None
                            }
                        };
                        self.finish_motion(doc, m, host, out);
                    }
                    Key::Char(c @ ('u' | 'U' | '~' | '?')) => {
                        let op = match c {
                            'u' => Op::Lower,
                            'U' => Op::Upper,
                            '?' => Op::Rot13,
                            _ => Op::Toggle,
                        };
                        if self.visual() {
                            self.prefix_visual_keys(doc);
                            let t = self.visual_target(doc);
                            self.mode = Mode::Normal;
                            self.begin_change();
                            self.apply_op(doc, op, t, host, out);
                            self.changed();
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
                let n = self.count.take().unwrap_or(1) * self.op.map_or(1, |o| o.1.max(1));
                if matches!(c, 's' | 'p') {
                    return self.sentence_or_paragraph(doc, c, inner, n, host, out);
                }
                let Some(t) = object_n(doc, self.cursor, c, inner, n) else {
                    self.failed = true;
                    return self.reset();
                };
                if self.visual() {
                    // `vi"` again: the quotes too.
                    if matches!(c, '"' | '\'' | '`')
                        && let Target::Chars(r) = &t
                        && self.anchor.min(self.cursor) == r.start
                        && doc.grapheme_after(self.anchor.max(self.cursor)) == r.end
                        && let Some(with) = objects::current_quote(doc, self.cursor, c, false, 2)
                    {
                        self.anchor = with.start;
                        self.cursor = doc.grapheme_before(with.end).max(with.start);
                        return;
                    }
                    // A selection already: the next object adds on.
                    if self.anchor != self.cursor
                        && self.cursor > self.anchor
                        && "wWs".contains(c)
                        && let Some(Target::Chars(next)) =
                            object_n(doc, doc.grapheme_after(self.cursor), c, inner, n)
                    {
                        self.cursor = doc.grapheme_before(next.end).max(next.start);
                        return;
                    }
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
            Pending::Mark => {
                self.pending = Pending::None;
                if let Key::Char(c) = key
                    && (c.is_ascii_alphabetic() || "'`[]<>".contains(c))
                {
                    let c = if c == '`' { '\'' } else { c };
                    doc.marks.named.insert(c, self.cursor);
                }
                return self.reset();
            }
            Pending::GotoMark(linewise) => {
                self.pending = Pending::None;
                let Key::Char(c) = key else {
                    return self.reset();
                };
                let m = self.goto_mark(doc, c, linewise);
                if m.is_none() {
                    out.message = Some(("E20: Mark not set".into(), true));
                }
                return self.finish_motion(doc, m, host, out);
            }
            Pending::Macro => {
                self.pending = Pending::None;
                if let Key::Char(c) = key {
                    self.record_macro(c);
                }
                return self.reset();
            }
            Pending::Execute => {
                self.pending = Pending::None;
                let n = self.count.take().unwrap_or(1);
                self.reset();
                if let Key::Char(c) = key {
                    self.keys.clear();
                    self.execute_macro(doc, c, n, host, out);
                }
                return;
            }
            Pending::Z => {
                self.pending = Pending::None;
                let n = self.count.take();
                // Folds, as Doom Emacs has them in Org: `zo`, `zO`, `zc`,
                // `zC`, `za`, `zA` (the global cycle), `zM` and `zR`.
                let fold = match key {
                    Key::Char('o') => Some("view.foldOpen"),
                    Key::Char('O') => Some("view.foldOpenSubtree"),
                    Key::Char('c' | 'C') => Some("view.foldClose"),
                    Key::Char('a') => Some("view.foldToggle"),
                    Key::Char('A') => Some("view.foldAll"),
                    Key::Char('M') => Some("view.foldCloseAll"),
                    Key::Char('R') => Some("view.foldOpenAll"),
                    _ => None,
                };
                if let Some(id) = fold {
                    out.commands.push((id.into(), Value::Null));
                    return self.reset();
                }
                self.z_command(doc, key, n, host);
                return self.reset();
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
        // Other commands forget the column vertical motion keeps, but for
        // those that leave the cursor be (`q`, `@`, `m`, `"`).
        if !matches!(key, Key::Char('q' | '@' | 'm' | '"')) {
            self.goal = None;
        }
        let Key::Char(c) = key else {
            let n = self.count.unwrap_or(1);
            let plain = matches!(doc.meta.mode, DocumentMode::Text { .. });
            match key {
                Key::Ctrl('o') if !self.visual() && self.op.is_none() => {
                    self.jump_list(doc, true, n)
                }
                Key::Tab if plain && !self.visual() && self.op.is_none() => {
                    self.jump_list(doc, false, n)
                }
                Key::Ctrl('i') if !self.visual() && self.op.is_none() => {
                    self.jump_list(doc, false, n)
                }
                Key::Ctrl('a' | 'x') if self.op.is_none() => {
                    let by = n as i64 * if key == Key::Ctrl('a') { 1 } else { -1 };
                    if self.visual() {
                        let t = self.visual_target(doc);
                        self.mode = Mode::Normal;
                        self.visual_increment(doc, t, by, false);
                    } else {
                        self.begin_change();
                        let line = line_of(doc, self.cursor);
                        if self.increment(doc, line, self.cursor, by) {
                            self.changed();
                        }
                    }
                }
                Key::Ctrl('e' | 'y') if self.op.is_none() => {
                    self.scroll_lines(doc, key == Key::Ctrl('e'), n, host)
                }
                Key::Ctrl('g') if self.op.is_none() => {
                    let lines = last_line(doc) + 1;
                    let l = line_of(doc, self.cursor) + 1;
                    let name = doc
                        .meta
                        .path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map_or_else(
                            || "[No Name]".to_string(),
                            |n| n.to_string_lossy().into_owned(),
                        );
                    let modified = if doc.is_modified() { " [Modified]" } else { "" };
                    out.message = Some((
                        format!(
                            "\"{name}\"{modified} {lines} lines --{}%--",
                            l * 100 / lines.max(1)
                        ),
                        false,
                    ));
                }
                Key::Ctrl('^' | '6') if self.op.is_none() => {
                    out.commands.push(("file.last".into(), Value::Null));
                }
                Key::Ctrl('l') => {}
                Key::Ctrl('r') if !self.visual() => {
                    for _ in 0..self.count.take().unwrap_or(1) {
                        if doc.redo_from_start().is_none() {
                            break;
                        }
                    }
                }
                // After an operator: the motion a block.
                Key::Ctrl('v') if self.op.is_some() => {
                    self.force = Some('\u{16}');
                    return;
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
                self.prefix_visual_keys(doc);
                let t = self.visual_target(doc);
                let lines_from_anchor = self.mode == Mode::VisualLine && self.cursor >= self.anchor;
                self.op_start = self.visual_start(doc);
                self.mode = Mode::Normal;
                self.begin_change_if(op);
                // `3>`: the shift three times.
                let times = match op {
                    Op::Indent | Op::Outdent => self.count.unwrap_or(1),
                    _ => 1,
                };
                for _ in 0..times {
                    self.apply_op(doc, op, t.clone(), host, out);
                }
                // Lines yanked from where visual mode began: the cursor at
                // the start of the first line.
                if op == Op::Yank
                    && lines_from_anchor
                    && let Target::Lines(l1, _) = t
                {
                    let at = line_start(doc, l1);
                    doc.selection = Selection::caret(at);
                    self.cursor = at;
                }
                self.changed_if(op);
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
                '[' | ']' => self.pending = Pending::Bracket(c == ']'),
                'v' | 'V' => self.force = Some(c),
                // `d*`: to the next match of the word.
                '*' | '#' => {
                    let n = self.count.take().unwrap_or(1) * self.op.map_or(1, |o| o.1.max(1));
                    let m = self.star_search(doc, c == '#', true, n, out).map(|to| Motion {
                        to,
                        linewise: false,
                        inclusive: false,
                    });
                    self.finish_motion(doc, m, host, out);
                }
                '\'' | '`' => self.pending = Pending::GotoMark(c == '\''),
                '?' if op == Op::Rot13 => self.operator(doc, op, host, out),
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
            '[' | ']' => {
                self.count = count;
                self.pending = Pending::Bracket(c == ']');
            }
            'f' | 't' | 'F' | 'T' => {
                self.count = count;
                self.pending = Pending::Find(find_kind(c));
            }
            'r' => {
                self.count = count;
                self.pending = Pending::Replace;
            }
            ':' | '/' | '?' => {
                self.search_count = count.unwrap_or(1);
                // `3:` is `:.,.+2`.
                self.command_line = Some(match (c, count) {
                    (':', Some(n)) if n > 1 => format!(":.,.+{}", n - 1),
                    _ => c.to_string(),
                });
            }
            'm' => self.pending = Pending::Mark,
            '\'' | '`' => {
                self.count = count;
                self.pending = Pending::GotoMark(c == '\'');
            }
            'q' if self.macro_rec.is_some() => {
                // The `q` that ends the recording is not part of it.
                if let Some((_, keys)) = &mut self.macro_rec {
                    keys.pop();
                }
                self.stop_macro();
            }
            'q' => self.pending = Pending::Macro,
            '@' => {
                self.count = count;
                self.pending = Pending::Execute;
            }
            'U' => self.line_undo(doc),
            'z' => {
                self.count = count;
                self.pending = Pending::Z;
            }
            '&' => {
                self.begin_change();
                self.ex(doc, "s", host, out);
                self.changed();
            }
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
            // Doom's `K` (`+lookup/documentation`): the language server's
            // documentation, where one serves the file.
            'K' if crate::lsp::serves(doc) => {
                self.count = None;
                out.commands
                    .push(("code.documentation".into(), Value::Null));
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
                    let len = self.change_case(doc, Op::Toggle, pos..end);
                    let e = line_end(doc, line);
                    doc.selection = Selection::caret((pos + len).min(e));
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
            'i' | 'a' | 'I' | 'A' | 'o' | 'O' => {
                self.insert_count = n;
                self.insert_repeat = if matches!(c, 'o' | 'O') {
                    insert::Repeat::Lines
                } else {
                    insert::Repeat::Here
                };
                self.open_insert(doc, c, pos, line);
            }
            'R' => {
                self.begin_change();
                if let Some(r) = &mut self.recording {
                    r.inserted = Some(String::new());
                }
                self.insert_count = n;
                self.mode = Mode::Replace;
            }
            // `1v`: as much as the last visual selection again, from here.
            'v' | 'V' if count.is_some() && self.last_visual.is_some() => {
                #[expect(clippy::expect_used, reason = "the arm's guard checked it")]
                let (mode, _, _, (lines, chars)) = self.last_visual.expect("checked");
                self.start_visual(mode);
                let k = n.max(1);
                match mode {
                    Mode::VisualLine => {
                        let to = (line + lines * k - 1).min(last_line(doc));
                        self.cursor = at_column(doc, to, column(doc, pos));
                    }
                    // Over lines: as many lines, to the same column.
                    _ if lines > 1 => {
                        let to = (line + lines * k - 1).min(last_line(doc));
                        self.cursor = at_column(doc, to, chars.saturating_sub(1));
                    }
                    _ => {
                        let e = line_end(doc, line);
                        let mut p = pos;
                        for _ in 1..chars * k {
                            if p < e {
                                p = doc.grapheme_after(p);
                            }
                        }
                        self.cursor = p.min(e);
                    }
                }
            }
            'v' => self.start_visual(Mode::Visual),
            'V' => self.start_visual(Mode::VisualLine),
            '*' | '#' => {
                if let Some(to) = self.star_search(doc, c == '#', true, n, out) {
                    self.jump(doc);
                    doc.selection = Selection::caret(to);
                }
            }
            // An unused character does nothing (and is not typed); Vim beeps,
            // which stops a macro.
            _ => {
                self.failed = true;
                self.reset();
            }
        }
    }

    /// `*` and `#` (`g*`, `g#`: `whole` false, not as a whole word): the
    /// word at the cursor searched for, now the last search; where its
    /// `n`th match from the word's start is.
    fn star_search(
        &mut self,
        doc: &DocumentState,
        back: bool,
        whole: bool,
        n: usize,
        out: &mut Outcome,
    ) -> Option<usize> {
        let w = ident_at(doc, self.cursor)?;
        let from = w.start;
        let word = doc.text().as_str()[w].to_string();
        let pat = format!(
            "{}\\V{}\\m{}",
            if whole && word.starts_with(is_word) { "\\<" } else { "" },
            word.replace('\\', "\\\\"),
            if whole && word.ends_with(is_word) { "\\>" } else { "" }
        );
        self.last_search = Some((pat.clone(), back));
        self.search_offset = None;
        out.highlights = self.matches(doc, &pat).ok();
        self.search_from(doc, &pat, back, from, n)
    }

    /// The matches of Vim pattern `pattern`, with 'ignorecase' and
    /// 'smartcase'.
    fn matches(&self, doc: &DocumentState, pattern: &str) -> Result<Vec<Range<usize>>, String> {
        let p = pattern::Pattern::new(pattern, self.options.ignorecase, self.options.smartcase)?;
        Ok(p.find_all(doc.text().as_str()))
    }

    /// The start of the `count`th match of `pattern` after `from` (before
    /// it with `back`), around the end with 'wrapscan'.
    fn search_from(
        &self,
        doc: &DocumentState,
        pattern: &str,
        back: bool,
        from: usize,
        count: usize,
    ) -> Option<usize> {
        let matches = self.matches(doc, pattern).ok()?;
        let wrap = self.options.wrapscan;
        let mut p = from;
        for _ in 0..count {
            let next = if back {
                matches
                    .iter()
                    .rev()
                    .find(|m| m.start < p)
                    .or_else(|| matches.last().filter(|_| wrap))
            } else {
                matches
                    .iter()
                    .find(|m| m.start > p)
                    .or_else(|| matches.first().filter(|_| wrap))
            }?;
            p = next.start;
        }
        Some(p)
    }

    /// The motions after `g`: `ge`, `gE`, `g_`, `go`, `gm`, `gM`.
    fn g_motion(
        &mut self,
        doc: &DocumentState,
        c: char,
        count: Option<usize>,
        host: &dyn Host,
    ) -> Option<Motion> {
        let n = count.unwrap_or(1).max(1);
        let pos = self.cursor;
        let line = line_of(doc, pos);
        self.goal = None;
        self.motion_count = n;
        let (s, e) = (line_start(doc, line), line_end(doc, line));
        let last_char = |s: usize, e: usize| if e > s { doc.grapheme_before(e) } else { e };
        let to = match c {
            'e' | 'E' => {
                let mut p = pos;
                for _ in 0..n {
                    // At the start of the text: as `b`.
                    if p == 0 {
                        if self.op.is_some() || p == pos {
                            return None;
                        }
                        break;
                    }
                    p = word_end_back(doc, p, c == 'E');
                }
                return Some(Motion {
                    to: p,
                    linewise: false,
                    inclusive: true,
                });
            }
            '_' => {
                let l = (line + n - 1).min(last_line(doc));
                let (s, e) = (line_start(doc, l), line_end(doc, l));
                let t = doc.text().as_str()[s..e].trim_end_matches([' ', '\t']);
                let p = if t.is_empty() {
                    s
                } else {
                    last_char(s, s + t.len())
                };
                return Some(Motion {
                    to: p,
                    linewise: false,
                    inclusive: true,
                });
            }
            'o' => {
                let mut p = (n - 1).min(doc.text().len());
                while !doc.text().as_str().is_char_boundary(p) {
                    p -= 1;
                }
                // Not on a line feed, as in normal mode.
                if char_at(doc, p) == Some('\n') && p > line_start(doc, line_of(doc, p)) {
                    p = doc.grapheme_before(p);
                }
                return Some(Motion {
                    to: p,
                    linewise: false,
                    inclusive: false,
                });
            }
            'm' => {
                let half = host.columns() / 2;
                at_column(doc, line, half).min(last_char(s, e))
            }
            _ => {
                let width = doc.text().as_str()[s..e].chars().count();
                let pct = count.unwrap_or(50).min(100);
                at_column(doc, line, width * pct / 100).min(last_char(s, e))
            }
        };
        Some(Motion {
            to,
            linewise: false,
            inclusive: false,
        })
    }

    /// `gJ`: lines joined as they are, no blanks added or taken.
    fn join_raw(&mut self, doc: &mut DocumentState, line: usize, count: usize) {
        let mut caret = None;
        for _ in 0..count.max(2) - 1 {
            if line >= last_line(doc) {
                break;
            }
            let e = line_end(doc, line);
            let next = line_start(doc, line + 1);
            edit(doc, e..next, "", e);
            caret = Some(e);
        }
        if let Some(e) = caret {
            doc.selection = Selection::caret(e.saturating_sub(0));
        }
    }

    /// `i`, `a`, `I`, `A`, `o` and `O`.
    fn open_insert(&mut self, doc: &mut DocumentState, c: char, pos: usize, line: usize) {
        self.begin_change();
        match c {
            'i' => self.enter_insert(doc, pos),
            'a' => {
                let e = line_end(doc, line);
                let at = if pos < e {
                    doc.grapheme_after(pos)
                } else {
                    pos
                };
                self.enter_insert(doc, at);
            }
            'I' => self.enter_insert(doc, first_non_blank(doc, line)),
            'A' => self.enter_insert(doc, line_end(doc, line)),
            _ => {
                let s = line_start(doc, line);
                let indent = if self.options.autoindent {
                    self.indent_like(doc, first_non_blank(doc, line))
                } else {
                    String::new()
                };
                let at = if c == 'o' {
                    let e = line_end(doc, line);
                    let at = e + 1 + indent.len();
                    edit(doc, e..e, &format!("\n{indent}"), at);
                    at
                } else {
                    edit(doc, s..s, &format!("{indent}\n"), s + indent.len());
                    s + indent.len()
                };
                self.enter_insert(doc, at);
                if !indent.is_empty() {
                    self.ai_line = Some(line_start(doc, line_of(doc, at)));
                }
            }
        }
    }

    /// Where Vim's cursor is as an operator works on the visual
    /// selection: its start, at the start of the line in line mode unless
    /// the cursor is the start (`oap->start`); none in a block (its
    /// corner then).
    fn visual_start(&self, doc: &DocumentState) -> Option<usize> {
        let first = self.anchor.min(self.cursor);
        match self.mode {
            Mode::Visual => Some(first),
            Mode::VisualLine if self.cursor < self.anchor => Some(self.cursor),
            Mode::VisualLine => Some(line_start(doc, line_of(doc, first))),
            _ => None,
        }
    }

    /// The visual selection's size as `1v` takes it again: its lines,
    /// and its characters on one line or the column it ends in over
    /// lines (Vim's `resel_VIsual_line_count` and `resel_VIsual_vcol`).
    fn visual_size(&self, doc: &DocumentState) -> (usize, usize) {
        let a = snap(doc, self.anchor.min(self.cursor));
        let c = snap(doc, self.anchor.max(self.cursor));
        let lines = line_of(doc, c) - line_of(doc, a) + 1;
        if lines == 1 {
            (1, doc.text().as_str()[a..c].chars().count() + 1)
        } else {
            (lines, column(doc, c) + 1)
        }
    }

    fn start_visual(&mut self, mode: Mode) {
        self.block_end = false;
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
        if !"ovVigfFtT\"r:[]".contains(c) {
            self.prefix_visual_keys(doc);
        }
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
            Target::Block { first, left, .. } => {
                at_display_col(doc, *first, *left, self.options.tabstop.max(1))
            }
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
                        (right, right != usize::MAX)
                    };
                    let ts = self.options.tabstop.max(1);
                    self.block_corner = Some(at_display_col(doc, first, left, ts));
                    let width = |l: usize| display_col(doc, line_end(doc, l), ts);
                    self.begin_change();
                    let mut at = block_part(doc, first, col, usize::MAX, ts).range.start;
                    if col == usize::MAX {
                        at = line_end(doc, first);
                    } else if pad && width(first) < col {
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
                // `D` and `C` in a block: to the end of every line.
                'D' | 'C' => {
                    self.mode = Mode::Normal;
                    let to_end = Target::Block {
                        first,
                        last,
                        left,
                        right: usize::MAX,
                    };
                    self.begin_change();
                    if c == 'C' {
                        return self.apply_op(doc, Op::Change, to_end, host, out);
                    }
                    self.apply_op(doc, Op::Delete, to_end, host, out);
                    self.changed();
                    return;
                }
                'o' => {
                    std::mem::swap(&mut self.anchor, &mut self.cursor);
                    return;
                }
                _ => {}
            }
        }
        // Changes `.` repeats, on as much text from the cursor; those
        // that type begin the change they end.
        if "sSCR".contains(c) {
            self.begin_change();
        }
        let ends_change = "xXDuU~JpP".contains(c);
        if ends_change || "sSCR".contains(c) {
            self.op_start = self.visual_start(doc);
            if let Some(at) = self.op_start {
                doc.selection = Selection::caret(at);
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
            'r' => self.pending = Pending::Replace,
            'i' | 'a' => self.pending = Pending::Object(c == 'i'),
            'g' => self.pending = Pending::G,
            '[' | ']' => self.pending = Pending::Bracket(c == ']'),
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
            'S' | 'C' | 'R' => {
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
                // An empty register: the selection is deleted all the same.
                let reg = self.fetch(host).or(Some(Register {
                    text: String::new(),
                    linewise: false,
                    block: false,
                }));
                let block_target = matches!(target, Target::Block { .. });
                let range = match target {
                    Target::Chars(r) => r,
                    Target::Lines(a, b) => line_start(doc, a)..line_end(doc, b),
                    Target::Block { first, last, .. } => {
                        line_start(doc, first)..line_end(doc, last)
                    }
                };
                if let Some(r) = reg {
                    let into_chars = !was_lines && r.linewise && !block_target;
                    let t = if into_chars {
                        // Lines put in characters: on lines of their own.
                        format!("\n{}", r.text)
                    } else if was_lines || !r.linewise {
                        r.text.trim_end_matches('\n').to_string()
                    } else {
                        r.text
                    };
                    let at = range.start;
                    let old = doc.text().as_str()[range.clone()].to_string();
                    edit(doc, range, &t, at);
                    // The cursor on the last character put (the first line
                    // of lines).
                    let caret = if into_chars {
                        first_non_blank(doc, line_of(doc, at) + 1)
                    } else if r.linewise || was_lines {
                        first_non_blank(doc, line_of(doc, at))
                    } else {
                        doc.grapheme_before(at + t.len()).max(at)
                    };
                    doc.selection = Selection::caret(caret);
                    self.cursor = caret;
                    // `p` puts what it replaced in the unnamed register.
                    if c == 'p' {
                        self.store(old, was_lines, false, host);
                    }
                }
            }
            // The command line on the lines selected, visual mode left
            // (`'<` and `'>` set as it ends).
            ':' => {
                self.mode = Mode::Normal;
                doc.selection = Selection::caret(self.cursor);
                self.command_line = Some(":'<,'>".into());
            }
            _ => {}
        }
        if ends_change {
            self.changed();
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
                // A count on the last line has no line to go down to: the
                // command fails (`3dd` there), as in Vim.
                if n > 1 && l1 >= last_line(doc) {
                    self.failed = true;
                    return self.reset();
                }
                let l2 = (l1 + n - 1).min(last_line(doc));
                // Vim goes to the first non-blank as the motion, and works
                // from whichever of that and the cursor comes first.
                self.op_start = Some(first_non_blank(doc, l1).min(self.cursor));
                self.begin_change_if(op);
                self.apply_op(doc, op, Target::Lines(l1, l2), host, out);
                self.changed_if(op);
            }
            // Another operator after one (`<d`): Vim beeps.
            Some(_) => {
                self.failed = true;
                self.reset();
            }
            None => {
                let c = self.count.take().unwrap_or(0);
                self.op = Some((op, c));
            }
        }
    }

    /// `is`, `as`, `ip` and `ap` with count `n` after an operator, or in
    /// visual mode, where they make the selection bigger.
    fn sentence_or_paragraph(
        &mut self,
        doc: &mut DocumentState,
        c: char,
        inner: bool,
        n: usize,
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        let anchor = self.visual().then_some(self.anchor);
        let target = if c == 'p' {
            let anchor = anchor.map(|a| line_of(doc, a));
            let Some((a, b)) =
                objects::current_par(doc, line_of(doc, self.cursor), anchor, n, !inner)
            else {
                return self.reset();
            };
            if self.visual() {
                self.mode = Mode::VisualLine;
                self.anchor = line_start(doc, a);
                self.cursor = line_start(doc, b);
                return;
            }
            Target::Lines(a, b)
        } else {
            match objects::current_sent(doc, self.cursor, anchor, n, !inner) {
                objects::Taken::Visual { anchor, cursor } => {
                    if anchor != self.anchor {
                        self.mode = Mode::Visual;
                    }
                    self.anchor = anchor;
                    self.cursor = cursor;
                    return;
                }
                objects::Taken::Range {
                    start,
                    end,
                    inclusive,
                } => {
                    let op = self.op.map_or(Op::Yank, |o| o.0);
                    char_target(doc, op, start, end, inclusive)
                }
            }
        };
        if let Some((op, _)) = self.op.take() {
            self.count = None;
            self.begin_change_if(op);
            let first = match target {
                Target::Lines(a, _) => Some(a),
                _ => None,
            };
            self.apply_op(doc, op, target, host, out);
            // `yip`: the cursor at the start of the paragraph.
            if op == Op::Yank
                && let Some(a) = first
            {
                let at = line_start(doc, a);
                doc.selection = Selection::caret(at);
                self.cursor = at;
            }
            self.changed_if(op);
        }
    }

    fn finish_motion(
        &mut self,
        doc: &mut DocumentState,
        m: Option<Motion>,
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        let Some(mut m) = m else {
            self.failed = true;
            return self.reset();
        };
        if self.visual() {
            self.jumping = false;
            self.cursor = m.to;
            // `$` in a block: to the end of every line, until a motion
            // sideways.
            let last = self.keys.last().copied();
            let vertical = matches!(
                last,
                Some(Key::Char('j' | 'k' | 'G') | Key::Down | Key::Up | Key::Ctrl('n' | 'p'))
            );
            self.block_end = self.mode == Mode::VisualBlock
                && (last == Some(Key::Char('$')) || (self.block_end && vertical));
            return;
        }
        let jumping = std::mem::take(&mut self.jumping);
        let Some((op, _)) = self.op.take() else {
            // `G` and `gg` to the line the cursor is on leave the jump list
            // as it was (Vim's `checkpcmark()`); searches always add.
            let to_line = matches!(self.keys.last(), Some(Key::Char('G' | 'g')));
            if jumping && !(to_line && line_of(doc, m.to) == line_of(doc, self.cursor)) {
                self.jump(doc);
            }
            doc.selection = Selection::caret(m.to);
            self.cursor = m.to;
            return;
        };
        self.goal = None;
        let from = self.cursor;
        // `dv`, `dV`, `d<C-v>`: characters (exclusive and inclusive
        // swapped), lines, or a block.
        match self.force.take() {
            Some('v') if m.linewise => {
                m.linewise = false;
                m.inclusive = false;
            }
            Some('v') => {
                // To a line's end (`$`): its last character, included.
                if !m.inclusive
                    && m.to > from
                    && char_at(doc, m.to) == Some('\n')
                    && m.to > line_start(doc, line_of(doc, m.to))
                {
                    m.to = doc.grapheme_before(m.to);
                    m.inclusive = true;
                }
                m.inclusive = !m.inclusive;
            }
            Some('V') => m.linewise = true,
            Some(_) => {
                let ts = self.options.tabstop.max(1);
                let ((a0, a1), (b0, b1)) = (display_span(doc, from, ts), display_span(doc, m.to, ts));
                let (la, lb) = (line_of(doc, from), line_of(doc, m.to));
                let target = Target::Block {
                    first: la.min(lb),
                    last: la.max(lb),
                    left: a0.min(b0),
                    right: a1.max(b1) + 1,
                };
                self.op_start = Some(from.min(m.to));
                self.begin_change_if(op);
                self.apply_op(doc, op, target, host, out);
                self.changed_if(op);
                return;
            }
            None => {}
        }
        let (s, e) = (from.min(m.to), from.max(m.to));
        self.op_start = Some(s);
        let target = if m.linewise {
            Target::Lines(line_of(doc, s), line_of(doc, e))
        } else {
            char_target(doc, op, s, e, m.inclusive)
        };
        let Target::Chars(Range { end: mut e, .. }) = target else {
            self.begin_change_if(op);
            self.apply_op(doc, op, target, host, out);
            self.changed_if(op);
            return;
        };
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
        let Some(mut change) = self.last_change.clone() else {
            return;
        };
        // A numbered register goes up one each time: `"1p..` puts `"2`
        // and `"3` (`:help redo-register`).
        let lead = change
            .keys
            .iter()
            .take_while(|k| matches!(k, Key::Char('0'..='9')))
            .count();
        if change.keys.get(lead) == Some(&Key::Char('"'))
            && let Some(Key::Char(d @ '1'..='8')) = change.keys.get(lead + 1).copied()
        {
            change.keys[lead + 1] = Key::Char((d as u8 + 1) as char);
            self.last_change = Some(change.clone());
        }
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
            // A search the change used (`d/pat<CR>`) is typed again.
            if self.command_line.is_some() {
                self.command_line_key(doc, k, host, out);
                continue;
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
                    self.replace_key(doc, if ch == '\t' { Key::Tab } else { Key::Char(ch) });
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
        self.force = None;
    }

    // Replace mode.

    fn replace_key(&mut self, doc: &mut DocumentState, key: Key) {
        match key {
            Key::Esc | Key::Ctrl('[') => {
                // `3R`: what was typed, twice more.
                let count = std::mem::replace(&mut self.insert_count, 1);
                if count > 1 {
                    let typed: String = self
                        .recording
                        .as_ref()
                        .and_then(|r| r.inserted.clone())
                        .unwrap_or_default();
                    for _ in 1..count {
                        for ch in typed.chars() {
                            self.replace_key(doc, if ch == '\t' { Key::Tab } else { Key::Char(ch) });
                        }
                    }
                    if let Some(r) = &mut self.recording {
                        r.inserted = Some(typed);
                    }
                }
                self.mode = Mode::Normal;
                self.replaced.clear();
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
                self.replaced
                    .push(doc.text().as_str()[pos..end].to_string());
                let mut tx = Transaction::new("Typing");
                tx.edit(pos..end, c.to_string());
                let tx = tx.select(Selection::caret(pos + c.len_utf8()));
                doc.apply(&tx, ChangeKind::Typing, Instant::now());
                if let Some(r) = &mut self.recording {
                    r.inserted.get_or_insert_default().push(c);
                }
            }
            Key::Enter => {
                let pos = doc.selection.head;
                edit(doc, pos..pos, "\n", pos + 1);
                self.replaced.push(String::new());
            }
            // Tab takes one character's place, as a typed one; with
            // 'expandtab' its spaces past the first are put in.
            Key::Tab => {
                let pos = doc.selection.head;
                let ts = self.options.tabstop.max(1);
                let text = if self.options.expandtab {
                    let sts = self.soft_tab();
                    let step = if sts > 0 { sts } else { ts };
                    let col = insert::vcol(doc, pos, ts);
                    " ".repeat((col / step + 1) * step - col)
                } else {
                    "\t".to_string()
                };
                let e = line_end(doc, line_of(doc, pos));
                let end = if pos < e {
                    doc.grapheme_after(pos)
                } else {
                    pos
                };
                self.replaced
                    .push(doc.text().as_str()[pos..end].to_string());
                for _ in 1..text.len() {
                    self.replaced.push(String::new());
                }
                edit(doc, pos..end, &text, pos + text.len());
                if let Some(r) = &mut self.recording {
                    r.inserted.get_or_insert_default().push('\t');
                }
            }
            // CTRL-W and CTRL-U: back over a word, or to the indent (the
            // line's start), as Backspaces.
            Key::Ctrl(c @ ('w' | 'u')) => {
                let pos = doc.selection.head;
                let line = line_of(doc, pos);
                let s = line_start(doc, line);
                let target = if c == 'u' {
                    let fnb = first_non_blank(doc, line);
                    if self.options.autoindent && pos > fnb { fnb } else { s }
                } else {
                    let mut p = pos;
                    while p > s && char_before(doc, p).is_some_and(|c| c == ' ' || c == '\t') {
                        p -= 1;
                    }
                    if let Some(c0) = char_before(doc, p).filter(|_| p > s) {
                        let word = is_word(c0);
                        while p > s
                            && char_before(doc, p)
                                .is_some_and(|c| c != ' ' && c != '\t' && is_word(c) == word)
                        {
                            p -= char_before(doc, p).map_or(1, char::len_utf8);
                        }
                    }
                    p
                };
                while doc.selection.head > target {
                    let before = doc.selection.head;
                    self.replace_key(doc, Key::Backspace);
                    if doc.selection.head >= before {
                        break;
                    }
                }
            }
            // Backspace puts back what was typed over.
            Key::Backspace | Key::Ctrl('h') => {
                let pos = doc.selection.head;
                let before = doc.grapheme_before(pos);
                match self.replaced.pop() {
                    Some(orig) if before < pos => {
                        edit(doc, before..pos, &orig, before);
                        if let Some(r) = &mut self.recording
                            && let Some(t) = &mut r.inserted
                        {
                            t.pop();
                        }
                    }
                    _ => doc.selection = Selection::caret(before),
                }
            }
            Key::Left => {
                let pos = doc.selection.head;
                doc.selection = Selection::caret(doc.grapheme_before(pos));
                self.replaced.clear();
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
                    self.ex(doc, rest, host, out);
                } else {
                    // A search after an operator is part of the change `.`
                    // repeats.
                    if self.op.is_some() && !self.replaying {
                        self.keys.extend(rest.chars().map(Key::Char));
                        self.keys.push(Key::Enter);
                    }
                    self.search(doc, rest, kind == "?", host, out);
                }
            }
            _ => {}
        }
    }

    /// The motion to the match of `pattern` at `to`, moved by the
    /// search's offset.
    fn offset_motion(&self, doc: &DocumentState, pattern: &str, to: usize) -> Option<Motion> {
        let chars = |to: usize, inclusive: bool| Motion {
            to,
            linewise: false,
            inclusive,
        };
        Some(match self.search_offset {
            None => chars(to, false),
            Some(SearchOffset::Start(k)) => chars(step_chars(doc, to, k), false),
            Some(SearchOffset::End(k)) => {
                let end = self
                    .matches(doc, pattern)
                    .ok()
                    .and_then(|all| all.into_iter().find(|m| m.start == to))
                    .map_or(to, |m| m.end);
                let last = if end > to { doc.grapheme_before(end).max(to) } else { to };
                chars(step_chars(doc, last, k), true)
            }
            Some(SearchOffset::Line(k)) => {
                let l = (line_of(doc, to) as isize + k).clamp(0, last_line(doc) as isize) as usize;
                Motion {
                    to: line_start(doc, l),
                    linewise: true,
                    inclusive: false,
                }
            }
        })
    }

    fn search(
        &mut self,
        doc: &mut DocumentState,
        pattern: &str,
        back: bool,
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        let (typed, offset) = split_search(pattern, if back { '?' } else { '/' });
        // `/<CR>` keeps the last offset, `//<CR>` drops it.
        self.search_offset = match offset {
            Some(o) => parse_offset(o),
            None if typed.is_empty() => self.search_offset,
            None => None,
        };
        let pattern = if typed.is_empty() {
            match &self.last_search {
                Some((p, _)) => p.clone(),
                None => return self.reset(),
            }
        } else {
            typed.to_string()
        };
        self.last_search = Some((pattern.clone(), back));
        match self.matches(doc, &pattern) {
            Ok(all) => {
                out.highlights = Some(all);
                let n = std::mem::replace(&mut self.search_count, 1).max(1);
                // From before the cursor by a character offset, so that
                // `/pat/s+2` can find the match the cursor is in (Vim's
                // `do_search()`).
                let from = match self.search_offset {
                    Some(SearchOffset::Start(k) | SearchOffset::End(k)) => {
                        step_chars(doc, self.cursor, -k)
                    }
                    _ => self.cursor,
                };
                match self.search_from(doc, &pattern, back, from, n) {
                    Some(to) => {
                        self.jumping = true;
                        let m = self.offset_motion(doc, &pattern, to);
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

    /// The command line's commands about files, windows and buffers.
    fn ex_app(&mut self, doc: &mut DocumentState, cmd: &str, out: &mut Outcome) {
        let cmd = cmd.trim();
        let save = || ("app.save".to_string(), Value::Null);
        // `:q` closes the pane, else the document, as closing a tab; it
        // quits only when nothing is left to close (the frontends say
        // when). `:q!` loses the document's changes, not the others'.
        let quit = || ("pane.closeOrQuit".to_string(), Value::Null);
        match cmd {
            "" => {}
            "w" | "write" => out.commands.push(save()),
            "q" | "quit" => out.commands.push(quit()),
            "q!" | "quit!" => out.commands.push((
                "pane.closeOrQuit".into(),
                serde_json::json!({ "force": true }),
            )),
            "wq" | "x" | "xit" | "exit" => {
                out.commands.push(save());
                out.commands.push(quit());
            }
            "qa" | "qall" | "quitall" => out.commands.push(("app.quit".into(), Value::Null)),
            "qa!" | "qall!" | "quitall!" => {
                out.commands
                    .push(("app.quitWithoutSaving".into(), Value::Null));
            }
            "wqa" | "wqall" | "xa" | "xall" => {
                out.commands.push(("file.saveAll".into(), Value::Null));
                out.commands.push(("app.quit".into(), Value::Null));
            }
            // Windows: the panes.
            "sp" | "split" => out.commands.push(("pane.splitBelow".into(), Value::Null)),
            "vs" | "vsp" | "vsplit" => out.commands.push(("pane.splitRight".into(), Value::Null)),
            "clo" | "close" => out.commands.push(("pane.close".into(), Value::Null)),
            "on" | "only" => out.commands.push(("pane.only".into(), Value::Null)),
            "new" => out.commands.push(("pane.new".into(), Value::Null)),
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
        assert_eq!(names, ["app.save", "pane.closeOrQuit"]);
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
        // I and A type on every line; A pads short lines; the cursor
        // ends at the block's top left corner, as in Vim.
        assert_eq!(run(t, 1, "<C-v>jjI-<Esc>"), "a|-bcd\ne-fgh\ni-jkl\n");
        assert_eq!(run("ab\nc\nde\n", 0, "<C-v>jjlA!<Esc>"), "|ab!\nc !\nde!\n");
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
