//! Insert mode's keys (`:help ins-special-keys`): Control with W, U, H,
//! R, O, T, D, E, Y, A, @, V, K, N, P, J, M and Escape; Enter, Tab and
//! Backspace in plain text, with 'autoindent', 'expandtab',
//! 'softtabstop' and 'backspace' as Vim has them. Other documents keep
//! their own Enter, Tab and Backspace (Org's lists and tables), as Doom's
//! Evil does.

use std::time::Instant;

use org_edit::{ChangeKind, Selection, Transaction};

use super::{
    DocumentState, Key, Mode, Outcome, Vim, char_before, edit, first_non_blank, is_word, line_end,
    line_of, line_start,
};
use crate::mode::DocumentMode;

/// A key insert mode waits for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum InsertPending {
    #[default]
    None,
    /// `CTRL-R`: a register's name.
    Register,
    /// `CTRL-V`: a character, or the digits of its code.
    Literal(String),
    /// `CTRL-K`: the two characters of a digraph.
    Digraph(Option<char>),
    /// `CTRL-G`: `u` begins a new undo step.
    CtrlG,
}

/// Keyword completion (`CTRL-N`, `CTRL-P`) in progress.
#[derive(Debug, Clone)]
pub(super) struct Completion {
    /// Where the word being completed starts.
    start: usize,
    /// What was typed.
    prefix: String,
    /// The candidates, nearest first in the direction asked.
    words: Vec<String>,
    /// The candidate shown; `words.len()` is the typed text again.
    index: usize,
}

/// How a count repeats what was typed (`3i`, `3o`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Repeat {
    /// At the cursor (`i`, `a`, `I`, `A`).
    #[default]
    Here,
    /// On new lines (`o`, `O`).
    Lines,
}

/// The digraphs (`:digraphs`), as Vim defines them.
const DIGRAPHS: &str = include_str!("digraphs.txt");

fn digraph(a: char, b: char) -> Option<char> {
    let find = |x: char, y: char| {
        DIGRAPHS.lines().find_map(|l| {
            let mut c = l.chars();
            (c.next() == Some(x) && c.next() == Some(y))
                .then(|| u32::from_str_radix(l[l.char_indices().nth(3)?.0..].trim(), 16).ok())
                .flatten()
                .and_then(char::from_u32)
        })
    };
    // Either order, as Vim tries both.
    find(a, b).or_else(|| find(b, a))
}

/// The display column of `pos` with tabs `ts` wide.
pub(super) fn vcol(doc: &DocumentState, pos: usize, ts: usize) -> usize {
    let s = line_start(doc, line_of(doc, pos));
    let mut col = 0;
    for c in doc.text().as_str()[s..pos].chars() {
        col = if c == '\t' {
            (col / ts + 1) * ts
        } else {
            col + 1
        };
    }
    col
}

/// Indentation `width` columns wide, with tabs unless `expandtab`.
pub(super) fn indent_string(width: usize, ts: usize, expandtab: bool) -> String {
    if expandtab || ts == 0 {
        " ".repeat(width)
    } else {
        format!("{}{}", "\t".repeat(width / ts), " ".repeat(width % ts))
    }
}

impl Vim {
    /// The insert-mode key `key`; `false` leaves it to the frontend (a
    /// character to type, or a key the document's mode has its own way
    /// with).
    pub(super) fn insert_key(
        &mut self,
        doc: &mut DocumentState,
        key: Key,
        host: &mut dyn super::Host,
        out: &mut Outcome,
    ) -> bool {
        let head = doc.selection.head.min(doc.text().len());
        if self.insert_at.is_none() {
            self.insert_at = Some(head);
        }
        // A key that waits for the next.
        match std::mem::take(&mut self.insert_pending) {
            InsertPending::None => {}
            InsertPending::Register => {
                if let Key::Char(c) = key {
                    self.insert_register(doc, c, host);
                }
                return true;
            }
            InsertPending::Literal(digits) => return self.literal(doc, key, digits),
            // `CTRL-G u`: what is typed from here is undone apart.
            InsertPending::CtrlG => {
                if key == Key::Char('u') {
                    doc.break_undo_group();
                    doc.begin_undo_join();
                }
                return true;
            }
            InsertPending::Digraph(first) => {
                let Key::Char(c) = key else {
                    return true;
                };
                match first {
                    None => self.insert_pending = InsertPending::Digraph(Some(c)),
                    Some(a) => {
                        let ch = digraph(a, c).unwrap_or(c);
                        self.type_text(doc, &ch.to_string());
                    }
                }
                return true;
            }
        }
        // Completion goes on with CTRL-N and CTRL-P; any other key keeps
        // the word shown.
        if !matches!(key, Key::Ctrl('n' | 'p')) {
            self.completion = None;
        }
        let plain = matches!(doc.meta.mode, DocumentMode::Text { .. });
        match key {
            Key::Esc | Key::Ctrl('[' | 'c') => self.leave_insert(doc),
            Key::Ctrl('@') => {
                let t = self.last_inserted.clone().unwrap_or_default();
                self.type_text(doc, &t);
                self.leave_insert(doc);
            }
            Key::Ctrl('a') => {
                let t = self.last_inserted.clone().unwrap_or_default();
                self.type_text(doc, &t);
            }
            Key::Ctrl('w') => self.delete_back(doc, Erase::Word),
            Key::Ctrl('u') => self.delete_back(doc, Erase::Line),
            Key::Ctrl('h') => self.delete_back(doc, Erase::Char),
            Key::Backspace if plain => self.delete_back(doc, Erase::Char),
            Key::Enter | Key::Ctrl('j' | 'm') if plain || !matches!(key, Key::Enter) => {
                self.newline(doc)
            }
            Key::Tab if plain => self.tab(doc),
            Key::Ctrl('t') => self.shift_in_insert(doc, true),
            Key::Ctrl('d') => self.shift_in_insert(doc, false),
            Key::Ctrl('e' | 'y') => {
                let line = line_of(doc, head);
                let other = if key == Key::Ctrl('e') {
                    (line < super::last_line(doc)).then(|| line + 1)
                } else {
                    line.checked_sub(1)
                };
                let ts = self.options.tabstop;
                let col = vcol(doc, head, ts);
                if let Some(l) = other {
                    let (s, e) = (line_start(doc, l), line_end(doc, l));
                    let text = doc.text().as_str()[s..e].to_string();
                    let mut c0 = 0;
                    let ch = text.chars().find(|c| {
                        let here = c0;
                        c0 = if *c == '\t' {
                            (c0 / ts + 1) * ts
                        } else {
                            c0 + 1
                        };
                        here <= col && col < c0
                    });
                    if let Some(ch) = ch {
                        self.type_text(doc, &ch.to_string());
                    }
                }
            }
            Key::Ctrl('r') => self.insert_pending = InsertPending::Register,
            Key::Ctrl('g') => self.insert_pending = InsertPending::CtrlG,
            Key::Ctrl('v' | 'q') => self.insert_pending = InsertPending::Literal(String::new()),
            Key::Ctrl('k') => self.insert_pending = InsertPending::Digraph(None),
            Key::Ctrl('o') => {
                // One command in normal mode, then back here. At the end
                // of the line the cursor stays after it.
                let eol = head == line_end(doc, line_of(doc, head))
                    && head > line_start(doc, line_of(doc, head));
                self.leave_insert_for_command(doc);
                self.ctrl_o = Some(super::CtrlO {
                    eol,
                    line: line_of(doc, doc.selection.head),
                });
            }
            Key::Ctrl('n' | 'p') => self.complete(doc, key == Key::Ctrl('n')),
            Key::Left | Key::Right | Key::Up | Key::Down => {
                // Moving starts the insert again where the cursor lands
                // (for `.` and undo); the frontend moves.
                doc.break_undo_group();
                self.insert_at = None;
                self.ai_line = None;
                return false;
            }
            Key::Char(_) => {
                // Typing on an autoindented line keeps its indent.
                self.ai_line = None;
                return false;
            }
            _ => {
                let _ = out;
                return false;
            }
        }
        true
    }

    /// Types `text` at the cursor.
    pub(super) fn type_text(&mut self, doc: &mut DocumentState, text: &str) {
        if text.is_empty() {
            return;
        }
        self.ai_line = None;
        let head = doc.selection.head.min(doc.text().len());
        let mut tx = Transaction::new("Typing");
        tx.edit(head..head, text);
        let tx = tx.select(Selection::caret(head + text.len()));
        doc.apply(&tx, ChangeKind::Typing, Instant::now());
    }

    fn insert_register(&mut self, doc: &mut DocumentState, c: char, host: &mut dyn super::Host) {
        let text = match c {
            '.' => self.last_inserted.clone(),
            _ => {
                self.register = Some(c);
                self.fetch(host).map(|r| r.text)
            }
        };
        // As if typed: a line break opens a line (with 'autoindent'), a
        // tab is a Tab (with 'expandtab' and 'softtabstop'); the small
        // delete register `-` is put as it is (Vim's `do_put()`).
        if c == '-'
            && let Some(t) = &text
        {
            self.type_text(doc, t);
            return;
        }
        if let Some(t) = text {
            for (i, part) in t.split('\n').enumerate() {
                if i > 0 {
                    self.newline(doc);
                }
                for (j, run) in part.split('\t').enumerate() {
                    if j > 0 {
                        self.tab(doc);
                    }
                    if !run.is_empty() {
                        self.type_text(doc, run);
                    }
                }
            }
        }
    }

    /// `CTRL-V`: the next key as it is, or a character by its code: up to
    /// three decimal digits, `x` and two hex digits, `u` and four, `U`
    /// and eight, `o` and three octal digits.
    fn literal(&mut self, doc: &mut DocumentState, key: Key, mut digits: String) -> bool {
        let (radix, max) = match digits.chars().next() {
            Some('x' | 'X') => (16, 3),
            Some('u') => (16, 5),
            Some('U') => (16, 9),
            Some('o' | 'O') => (8, 4),
            _ => (10, 3),
        };
        let code = |d: &str| {
            let body = if radix == 10 { d } else { &d[1..] };
            u32::from_str_radix(body, radix)
                .ok()
                .and_then(char::from_u32)
        };
        match key {
            Key::Char(c)
                if (digits.is_empty() && "xXuUoO".contains(c))
                    || (c.is_digit(radix) && (radix != 10 || c.is_ascii_digit())) =>
            {
                digits.push(c);
                if digits.len() >= max {
                    if let Some(ch) = code(&digits) {
                        self.type_text(doc, &ch.to_string());
                    }
                } else {
                    self.insert_pending = InsertPending::Literal(digits);
                }
                return true;
            }
            _ if !digits.is_empty() => {
                // The code ends at the first other key, which then counts.
                if let Some(ch) = code(&digits).filter(|_| digits.len() > (radix != 10) as usize) {
                    self.type_text(doc, &ch.to_string());
                }
                let mut sink = Outcome::default();
                let mut none = super::NoHost;
                return self.insert_key(doc, key, &mut none, &mut sink);
            }
            Key::Char(c) => self.type_text(doc, &c.to_string()),
            Key::Tab => self.type_text(doc, "\t"),
            Key::Enter => self.type_text(doc, "\r"),
            Key::Esc => self.type_text(doc, "\u{1b}"),
            Key::Backspace => self.type_text(doc, "\u{8}"),
            Key::Ctrl(c) if c.is_ascii_alphabetic() || "@[\\]^_".contains(c) => {
                let ch = char::from_u32((c.to_ascii_uppercase() as u32) & 0x1f).unwrap_or(c);
                self.type_text(doc, &ch.to_string());
            }
            _ => {}
        }
        true
    }

    /// `CTRL-W`, `CTRL-U`, `CTRL-H` and Backspace, with 'backspace' as
    /// `indent,eol,start`: at the start of a line they join it to the one
    /// before; they stop once where the insert started.
    fn delete_back(&mut self, doc: &mut DocumentState, what: Erase) {
        let head = doc.selection.head.min(doc.text().len());
        let line = line_of(doc, head);
        let s = line_start(doc, line);
        self.completion = None;
        if head == s {
            if line == 0 {
                return;
            }
            // Joined to the line before.
            let prev_end = line_end(doc, line - 1);
            edit(doc, prev_end..head, "", prev_end);
            self.ai_line = None;
            if let Some(at) = self.insert_at
                && at > prev_end
            {
                self.insert_at = Some(prev_end);
            }
            return;
        }
        let text = doc.text().as_str();
        let start = self.insert_at.filter(|a| *a >= s && *a < head);
        let mut to = match what {
            Erase::Char => {
                // Blanks back to the soft tab stop before; a tab the stop
                // is inside of becomes spaces up to it ('softtabstop').
                let sts = self.soft_tab();
                if sts > 0 && matches!(char_before(doc, head), Some(' ' | '\t')) {
                    let ts = self.options.tabstop.max(1);
                    let want = (vcol(doc, head, ts) - 1) / sts * sts;
                    let mut p = head;
                    while p > s && matches!(text.as_bytes()[p - 1], b' ' | b'\t') && vcol(doc, p, ts) > want {
                        p -= 1;
                    }
                    let fill = want.saturating_sub(vcol(doc, p, ts));
                    if fill > 0 {
                        edit(doc, p..head, &" ".repeat(fill), p + fill);
                        if let Some(a) = self.insert_at
                            && a > p
                        {
                            self.insert_at = Some(p);
                        }
                        return;
                    }
                    p.min(doc.grapheme_before(head))
                } else {
                    doc.grapheme_before(head).max(s)
                }
            }
            Erase::Word => {
                let mut p = head;
                while let Some(c) = char_before(doc, p).filter(|c| *c == ' ' || *c == '\t') {
                    if p <= s {
                        break;
                    }
                    p -= c.len_utf8();
                }
                if let Some(c0) = char_before(doc, p).filter(|_| p > s) {
                    let word = is_word(c0);
                    while let Some(c) = char_before(doc, p) {
                        if p <= s || c == ' ' || c == '\t' || is_word(c) != word {
                            break;
                        }
                        p -= c.len_utf8();
                    }
                }
                p
            }
            Erase::Line => {
                // Back to the indent with 'autoindent', when past it.
                let fnb = first_non_blank(doc, line);
                if self.options.autoindent && head > fnb {
                    fnb
                } else {
                    s
                }
            }
        };
        // Once at the start of the insert.
        if let Some(a) = start
            && to < a
            && what != Erase::Char
        {
            to = a;
        }
        if to < head {
            edit(doc, to..head, "", to);
            if let Some(a) = self.insert_at
                && a > to
            {
                self.insert_at = Some(to);
            }
        }
    }

    /// The width of a soft tab: 'softtabstop', or 'shiftwidth' when it is
    /// negative; 0 for none.
    pub(super) fn soft_tab(&self) -> usize {
        match self.options.softtabstop {
            n if n < 0 => self.shiftwidth(),
            n => n as usize,
        }
    }

    pub(super) fn shiftwidth(&self) -> usize {
        if self.options.shiftwidth == 0 {
            self.options.tabstop
        } else {
            self.options.shiftwidth
        }
    }

    /// An indent as wide as the text before `upto` on its line, made
    /// with tabs or spaces as 'expandtab' says (as Vim builds the indent
    /// 'autoindent' copies).
    pub(super) fn indent_like(&self, doc: &DocumentState, upto: usize) -> String {
        let ts = self.options.tabstop.max(1);
        indent_string(vcol(doc, upto, ts), ts, self.options.expandtab)
    }

    /// Enter: a new line, indented as this one with 'autoindent' (the
    /// blanks after the cursor go); an indent nothing was typed after
    /// goes away.
    pub(super) fn newline(&mut self, doc: &mut DocumentState) {
        let head = doc.selection.head.min(doc.text().len());
        let line = line_of(doc, head);
        let (s, e) = (line_start(doc, line), line_end(doc, line));
        let text = doc.text().as_str();
        let indent_end = first_non_blank(doc, line);
        let mut indent = if self.options.autoindent {
            self.indent_like(doc, indent_end.min(head))
        } else {
            String::new()
        };
        let mut from = head;
        let mut to = head;
        if self.options.autoindent {
            // The blanks after the cursor go to the new line's indent.
            to += text[head..e].len() - text[head..e].trim_start_matches([' ', '\t']).len();
        }
        // A line that has only the indent it was given loses it.
        if self.ai_line == Some(s) && text[s..e].trim().is_empty() {
            from = s;
            indent = text[s..e].to_string();
        }
        let insert = format!("\n{indent}");
        let caret = from + insert.len();
        edit(doc, from..to, &insert, caret);
        let new_line = line_of(doc, caret);
        self.ai_line = (!indent.is_empty()).then(|| line_start(doc, new_line));
        if let Some(a) = self.insert_at
            && a > from
        {
            self.insert_at = Some(from);
        }
    }

    /// Tab: a tab, or spaces to the next stop with 'expandtab' or
    /// 'softtabstop'.
    fn tab(&mut self, doc: &mut DocumentState) {
        let head = doc.selection.head.min(doc.text().len());
        let ts = self.options.tabstop.max(1);
        let sts = self.soft_tab();
        if !self.options.expandtab && sts == 0 {
            return self.type_text(doc, "\t");
        }
        let step = if sts > 0 { sts } else { ts };
        let col = vcol(doc, head, ts);
        let want = (col / step + 1) * step;
        if self.options.expandtab {
            return self.type_text(doc, &" ".repeat(want - col));
        }
        // Tabs where a run of blanks reaches a tab stop.
        let s = line_start(doc, line_of(doc, head));
        let text = doc.text().as_str();
        let blank = text[s..head].len() - text[s..head].trim_end_matches([' ', '\t']).len();
        let from = head - blank;
        let start_col = vcol(doc, from, ts);
        let mut fill = String::new();
        let mut c = start_col;
        while (c / ts + 1) * ts <= want {
            fill.push('\t');
            c = (c / ts + 1) * ts;
        }
        fill.push_str(&" ".repeat(want - c));
        let caret = from + fill.len();
        edit(doc, from..head, &fill, caret);
        self.ai_line = None;
    }

    /// `CTRL-T` and `CTRL-D`: the line's indent a 'shiftwidth' more or
    /// less, the cursor staying with the text.
    fn shift_in_insert(&mut self, doc: &mut DocumentState, right: bool) {
        let head = doc.selection.head.min(doc.text().len());
        let line = line_of(doc, head);
        let (s, fnb) = (line_start(doc, line), first_non_blank(doc, line));
        let ts = self.options.tabstop.max(1);
        let sw = self.shiftwidth();
        let width = vcol(doc, fnb, ts);
        let new = if right {
            (width / sw + 1) * sw
        } else if width == 0 {
            return;
        } else {
            (width - 1) / sw * sw
        };
        let indent = indent_string(new, ts, self.options.expandtab);
        let caret = if head >= fnb {
            head - (fnb - s) + indent.len()
        } else {
            s + indent.len()
        };
        edit(doc, s..fnb, &indent, caret);
        if self.ai_line == Some(s) && line_end(doc, line) > s {
            self.ai_line = Some(s);
        }
        // Where the insert began moves with the text after the indent.
        if let Some(a) = self.insert_at
            && a > s
        {
            self.insert_at = Some(if a >= fnb {
                a - (fnb - s) + indent.len()
            } else {
                a.min(s + indent.len())
            });
        }
    }

    /// `CTRL-N` and `CTRL-P`: the word before the cursor completed from the
    /// document's words, the next or the one before each time; past the
    /// last, what was typed again.
    fn complete(&mut self, doc: &mut DocumentState, next: bool) {
        let head = doc.selection.head.min(doc.text().len());
        if self.completion.is_none() {
            let s = line_start(doc, line_of(doc, head));
            let mut start = head;
            while start > s && char_before(doc, start).is_some_and(is_word) {
                start -= char_before(doc, start).map_or(1, char::len_utf8);
            }
            let text = doc.text().as_str();
            let prefix = text[start..head].to_string();
            let mut words: Vec<(usize, String)> = Vec::new();
            let mut i = 0;
            for (at, w) in text
                .split(|c: char| !is_word(c))
                .map(|w| {
                    let at = i;
                    i += w.len() + 1;
                    (at, w)
                })
                .filter(|(at, w)| {
                    !w.is_empty() && *at != start && w.starts_with(&prefix) && **w != *prefix
                })
            {
                words.push((at, w.to_string()));
            }
            // Nearest first: forward from the cursor and around for
            // CTRL-N, backward and around for CTRL-P.
            let (after, before): (Vec<_>, Vec<_>) =
                words.into_iter().partition(|(at, _)| *at > start);
            let words: Vec<(usize, String)> = if next {
                after.into_iter().chain(before).collect()
            } else {
                before
                    .into_iter()
                    .rev()
                    .chain(after.into_iter().rev())
                    .collect()
            };
            let mut seen = std::collections::HashSet::new();
            let words: Vec<String> = words
                .into_iter()
                .map(|(_, w)| w)
                .filter(|w| seen.insert(w.clone()))
                .collect();
            self.completion = Some(Completion {
                start,
                prefix,
                words,
                index: usize::MAX,
            });
        }
        let Some(c) = &mut self.completion else {
            return;
        };
        let n = c.words.len() + 1;
        c.index = if c.index == usize::MAX {
            0
        } else if next {
            (c.index + 1) % n
        } else {
            (c.index + n - 1) % n
        };
        let word = c
            .words
            .get(c.index)
            .cloned()
            .unwrap_or_else(|| c.prefix.clone());
        let start = c.start;
        edit(doc, start..head, &word, start + word.len());
    }

    /// CTRL-O: normal mode for one command.
    fn leave_insert_for_command(&mut self, doc: &mut DocumentState) {
        let head = doc.selection.head.min(doc.text().len());
        if let Some(at) = self.insert_at
            && head >= at
        {
            self.last_inserted = Some(doc.text().as_str()[at..head].to_string());
        }
        // The command is part of the insert's undo step, as in Vim.
        self.mode = Mode::Normal;
        self.insert_at = None;
        self.ai_line = None;
        // The cursor goes onto the last character, as in normal mode; the
        // return puts it back after it.
        self.cursor = head;
    }
}

/// What a deleting key takes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Erase {
    Char,
    Word,
    Line,
}
