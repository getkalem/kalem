//! Normal mode's commands beyond motions and operators: marks and the
//! jump and change lists, macros, CTRL-A and CTRL-X, `U`.

use org_edit::Selection;

use super::{
    DocumentState, Host, Key, Mode, Motion, Outcome, Register, Vim, at_column, edit,
    first_non_blank, last_line, line_end, line_of, line_start,
};

/// Keys in a register as Vim keeps them: Escape, Enter, Backspace and
/// Control as their control characters.
pub(super) fn keys_to_text(keys: &[Key]) -> String {
    keys.iter()
        .filter_map(|k| match *k {
            Key::Char(c) => Some(c),
            Key::Ctrl(c) => {
                let up = c.to_ascii_uppercase() as u32;
                char::from_u32(if (0x40..0x60).contains(&up) {
                    up - 0x40
                } else {
                    up
                })
            }
            Key::Esc => Some('\u{1b}'),
            Key::Enter => Some('\r'),
            Key::Backspace => Some('\u{8}'),
            Key::Tab => Some('\t'),
            _ => None,
        })
        .collect()
}

/// A register's text as keys, for `@`.
pub(super) fn text_to_keys(text: &str) -> Vec<Key> {
    text.chars()
        .map(|c| match c {
            '\u{1b}' => Key::Esc,
            '\r' | '\n' => Key::Enter,
            '\u{8}' => Key::Backspace,
            '\t' => Key::Tab,
            c if (c as u32) < 0x20 => Key::Ctrl(char::from_u32(c as u32 + 0x60).unwrap_or('@')),
            c => Key::Char(c),
        })
        .collect()
}

impl Vim {
    /// Records a jump from the cursor (the `''` mark and the jump list).
    pub(super) fn jump(&mut self, doc: &mut DocumentState) {
        let at = self.cursor.min(doc.text().len());
        let text = doc.text().clone();
        doc.marks.jump(at, |p| text.line_of(p.min(text.len())));
        doc.marks.named.insert('\'', at);
    }

    /// The position of mark `c`, if set.
    pub(super) fn mark(&self, doc: &DocumentState, c: char) -> Option<usize> {
        let c = if c == '`' { '\'' } else { c };
        let len = doc.text().len();
        match c {
            '\'' => Some(*doc.marks.named.get(&'\'').unwrap_or(&0)),
            _ => doc.marks.named.get(&c).copied(),
        }
        .map(|p| p.min(len))
    }

    /// `'a` (linewise, to the first non-blank) or `` `a `` (to the mark).
    pub(super) fn goto_mark(
        &mut self,
        doc: &mut DocumentState,
        c: char,
        linewise: bool,
    ) -> Option<Motion> {
        let to = self.mark(doc, c)?;
        if self.op.is_none() {
            self.jump(doc);
        }
        Some(if linewise {
            Motion {
                to: first_non_blank(doc, line_of(doc, to)),
                linewise: true,
                inclusive: false,
            }
        } else {
            Motion {
                to,
                linewise: false,
                inclusive: false,
            }
        })
    }

    /// CTRL-O (`back`) and CTRL-I: through the jump list.
    pub(super) fn jump_list(&mut self, doc: &mut DocumentState, back: bool, count: usize) {
        let here = self.cursor;
        let text = doc.text().clone();
        let line = |p: usize| text.line_of(p.min(text.len()));
        let m = &mut doc.marks;
        if back {
            if m.jump_index >= m.jumps.len() {
                m.jump(here, line);
                m.jump_index = m.jumps.len().saturating_sub(1);
            }
            for _ in 0..count {
                if m.jump_index == 0 {
                    return;
                }
                m.jump_index -= 1;
            }
        } else {
            for _ in 0..count {
                if m.jump_index + 1 >= m.jumps.len() {
                    return;
                }
                m.jump_index += 1;
            }
        }
        if let Some(&to) = m.jumps.get(m.jump_index) {
            let to = to.min(text.len());
            doc.selection = Selection::caret(to);
            self.cursor = to;
        }
    }

    /// `g;` (`back`) and `g,`: through the change list.
    pub(super) fn change_list(&mut self, doc: &mut DocumentState, back: bool, count: usize) {
        let m = &mut doc.marks;
        if m.changes.is_empty() {
            return;
        }
        let i = if back {
            m.change_index.saturating_sub(count)
        } else {
            (m.change_index + count).min(m.changes.len() - 1)
        };
        m.change_index = i;
        let to = m.changes[i].min(doc.text().len());
        doc.selection = Selection::caret(to);
        self.cursor = to;
    }

    /// After a change: the `.` mark and the change list.
    pub(super) fn note_change(&mut self, doc: &mut DocumentState, at: usize) {
        let text = doc.text().clone();
        let at = at.min(text.len());
        doc.marks.change(at, |p| text.line_of(p.min(text.len())));
        doc.marks.named.insert('.', at);
    }

    /// `q{reg}` starts recording, `q` stops.
    pub(super) fn record_macro(&mut self, c: char) {
        if c.is_ascii_alphanumeric() || c == '"' {
            self.macro_rec = Some((c, Vec::new()));
        }
    }

    /// The end of a recording: the keys go into the register (appended
    /// to with an upper case name).
    pub(super) fn stop_macro(&mut self) {
        if let Some((c, keys)) = self.macro_rec.take() {
            let text = keys_to_text(&keys);
            let name = c.to_ascii_lowercase();
            if c.is_ascii_uppercase() {
                let e = self.registers.entry(name).or_insert(Register {
                    text: String::new(),
                    linewise: false,
                });
                e.text.push_str(&text);
            } else {
                self.registers.insert(
                    name,
                    Register {
                        text,
                        linewise: false,
                    },
                );
            }
        }
    }

    /// `@{reg}`: the register's text as keys, `count` times (`@@` the
    /// last one again, `@:` the last command line).
    pub(super) fn execute_macro(
        &mut self,
        doc: &mut DocumentState,
        c: char,
        count: usize,
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        let c = if c == '@' {
            match self.last_macro {
                Some(c) => c,
                None => return,
            }
        } else {
            c
        };
        self.last_macro = Some(c);
        let keys = if c == ':' {
            match &self.last_ex {
                Some(l) => {
                    let mut k = vec![Key::Char(':')];
                    k.extend(l.chars().map(Key::Char));
                    k.push(Key::Enter);
                    k
                }
                None => return,
            }
        } else {
            match self.registers.get(&c.to_ascii_lowercase()) {
                Some(r) => {
                    let mut k = text_to_keys(&r.text);
                    // A linewise register ends with its line feed: Enter.
                    if r.linewise && k.last() == Some(&Key::Enter) {
                        k.pop();
                        k.push(Key::Enter);
                    }
                    k
                }
                None => return,
            }
        };
        if self.macro_depth > 20 {
            return;
        }
        self.macro_depth += 1;
        for _ in 0..count.max(1) {
            self.feed(doc, &keys, host, out);
        }
        self.macro_depth -= 1;
    }

    /// Runs `keys` as if typed, typing what the layer leaves to the
    /// frontend itself.
    pub(super) fn feed(
        &mut self,
        doc: &mut DocumentState,
        keys: &[Key],
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        for &k in keys {
            let o = self.key(doc, k, host);
            out.commands.extend(o.commands);
            if o.message.is_some() {
                out.message = o.message;
            }
            if o.highlights.is_some() {
                out.highlights = o.highlights;
            }
            out.force_quit |= o.force_quit;
            if o.handled {
                continue;
            }
            match k {
                Key::Char(c) if self.mode == Mode::Insert => self.type_text(doc, &c.to_string()),
                Key::Enter if self.mode == Mode::Insert => self.newline(doc),
                Key::Tab if self.mode == Mode::Insert => self.type_text(doc, "\t"),
                Key::Backspace if self.mode == Mode::Insert => {
                    let _ = doc.delete_backward(std::time::Instant::now());
                }
                _ => {}
            }
        }
    }

    /// CTRL-A (`by` > 0) and CTRL-X: the number at or after the cursor on
    /// its line, in the formats 'nrformats' allows; the cursor ends on its
    /// last character.
    pub(super) fn increment(
        &mut self,
        doc: &mut DocumentState,
        line: usize,
        from: usize,
        by: i64,
    ) -> bool {
        let (s, e) = (line_start(doc, line), line_end(doc, line));
        let text = doc.text().as_str()[s..e].to_string();
        let from = from.clamp(s, e) - s;
        let Some((range, new)) = number_at(&text, from, by, &self.options.nrformats) else {
            return false;
        };
        let at = s + range.start;
        let end = at + new.len();
        edit(doc, s + range.start..s + range.end, &new, end - 1);
        doc.selection = Selection::caret(end - 1);
        self.cursor = end - 1;
        true
    }

    /// `U`: the last changed line as it was before its changes (itself
    /// a change, which `U` takes back).
    pub(super) fn line_undo(&mut self, doc: &mut DocumentState) {
        let Some((line, old)) = self.line_undo.clone() else {
            return;
        };
        if line > last_line(doc) {
            return;
        }
        let (s, e) = (line_start(doc, line), line_end(doc, line));
        let now = doc.text().as_str()[s..e].to_string();
        edit(doc, s..e, &old, s);
        self.line_undo = Some((line, now));
        doc.selection = Selection::caret(s);
        self.cursor = s;
    }

    /// `z` commands that scroll: `zt`, `zz`, `zb` and `z<CR>`, `z.`, `z-`
    /// (which also go to the first non-blank).
    pub(super) fn z_command(
        &mut self,
        doc: &mut DocumentState,
        key: Key,
        count: Option<usize>,
        host: &mut dyn Host,
    ) {
        let line = count.map_or(line_of(doc, self.cursor), |n| {
            n.saturating_sub(1).min(last_line(doc))
        });
        let (at, first) = match key {
            Key::Char('t') => (0, false),
            Key::Enter => (0, true),
            Key::Char('z') => (1, false),
            Key::Char('.') => (1, true),
            Key::Char('b') => (2, false),
            Key::Char('-') => (2, true),
            _ => return,
        };
        host.scroll_to(line, at);
        let to = if first {
            first_non_blank(doc, line)
        } else if count.is_some() {
            at_column(doc, line, super::column(doc, self.cursor))
        } else {
            self.cursor
        };
        doc.selection = Selection::caret(to);
        self.cursor = to;
    }

    /// CTRL-E (`down`) and CTRL-Y: the view scrolls; the cursor stays on
    /// screen.
    pub(super) fn scroll_lines(
        &mut self,
        doc: &mut DocumentState,
        down: bool,
        count: usize,
        host: &mut dyn Host,
    ) {
        let by = count as isize * if down { 1 } else { -1 };
        let Some((top, bottom)) = host.scroll(by) else {
            return;
        };
        let line = line_of(doc, self.cursor);
        let to_line = line.clamp(top, bottom.max(top)).min(last_line(doc));
        if to_line != line {
            let col = super::column(doc, self.cursor);
            let to = at_column(doc, to_line, col);
            let e = line_end(doc, to_line);
            let to = if to >= e && e > line_start(doc, to_line) {
                doc.grapheme_before(e)
            } else {
                to
            };
            doc.selection = Selection::caret(to);
            self.cursor = to;
        }
    }
}

/// The number in `line` at or after byte `from`, changed by `by`: its
/// range in the line and its new text. `nrformats` names the formats
/// besides decimal (`bin`, `hex`, `octal`, `alpha`).
pub(super) fn number_at(
    line: &str,
    from: usize,
    by: i64,
    nrformats: &[String],
) -> Option<(std::ops::Range<usize>, String)> {
    let has = |f: &str| nrformats.iter().any(|x| x == f);
    let b = line.as_bytes();
    // A hexadecimal or binary number the cursor is in or before.
    let radix_at = |i: usize| -> Option<(usize, u32, usize)> {
        // (start of the digits, radix, start of the prefix)
        if i + 2 <= b.len() && b[i] == b'0' && i + 2 < b.len() + 1 {
            let x = b.get(i + 1).copied().unwrap_or(0);
            let digit = |c: u8, r: u32| (c as char).is_digit(r);
            if has("hex") && matches!(x, b'x' | b'X') && b.get(i + 2).is_some_and(|c| digit(*c, 16))
            {
                return Some((i + 2, 16, i));
            }
            if has("bin") && matches!(x, b'b' | b'B') && b.get(i + 2).is_some_and(|c| digit(*c, 2))
            {
                return Some((i + 2, 2, i));
            }
        }
        None
    };
    // Back from the cursor to the start of the number it is in.
    let mut i = from.min(b.len());
    let is_digit_or_hex = |c: u8| c.is_ascii_hexdigit() || matches!(c, b'x' | b'X');
    if i < b.len() && is_digit_or_hex(b[i]) {
        while i > 0 && is_digit_or_hex(b[i - 1]) {
            i -= 1;
        }
    }
    // Forward to the first number.
    let mut start = None;
    let mut j = i;
    while j < b.len() {
        if let Some(r) = radix_at(j) {
            start = Some(r);
            break;
        }
        if b[j].is_ascii_digit() {
            start = Some((j, 10, j));
            break;
        }
        if has("alpha") && b[j].is_ascii_alphabetic() {
            let c = b[j];
            let new = if by > 0 {
                if c == b'z' || c == b'Z' { c } else { c + 1 }
            } else if c == b'a' || c == b'A' {
                c
            } else {
                c - 1
            };
            return Some((j..j + 1, (new as char).to_string()));
        }
        j += 1;
    }
    let (digits, radix, prefix) = start?;
    let mut end = digits;
    while end < b.len() && (b[end] as char).is_digit(radix) {
        end += 1;
    }
    let body = &line[digits..end];
    if radix == 10 {
        // A minus sign right before makes it negative.
        let neg = digits > 0 && b[digits - 1] == b'-';
        let n: i128 = body.parse().ok()?;
        let n = if neg { -n } else { n } + by as i128;
        let s = if neg { digits - 1 } else { digits };
        let text = n.to_string();
        // Leading zeros keep the width (`007` gives `008`).
        let text = if body.len() > 1 && body.starts_with('0') && n >= 0 {
            format!("{:0width$}", n, width = body.len())
        } else {
            text
        };
        return Some((s..end, text));
    }
    // Unsigned 64 bits, around at the ends, as Vim has them.
    let n = u64::from_str_radix(body, radix)
        .ok()?
        .wrapping_add(by as u64);
    let mut digits_text = match radix {
        16 => format!("{n:x}"),
        _ => format!("{n:b}"),
    };
    // The width and the case of the letters stay.
    if digits_text.len() < body.len() {
        digits_text = format!(
            "{}{digits_text}",
            "0".repeat(body.len() - digits_text.len())
        );
    }
    if body
        .chars()
        .rev()
        .find(|c| c.is_ascii_alphabetic())
        .is_some_and(|c| c.is_ascii_uppercase())
    {
        digits_text = digits_text.to_uppercase();
    }
    Some((
        prefix..end,
        format!("{}{digits_text}", &line[prefix..digits]),
    ))
}

impl Vim {
    /// CTRL-A and CTRL-X on a visual selection: the first number of each
    /// line in it; with `g`, by `by` more on each line (a sequence).
    pub(super) fn visual_increment(
        &mut self,
        doc: &mut DocumentState,
        target: super::Target,
        by: i64,
        progressive: bool,
    ) {
        let (first, last, from_col) = match &target {
            super::Target::Chars(r) => (
                line_of(doc, r.start),
                line_of(doc, r.end.saturating_sub(1).max(r.start)),
                Some(r.start),
            ),
            super::Target::Lines(a, b) => (*a, *b, None),
            super::Target::Block { first, last, .. } => (*first, *last, None),
        };
        self.begin_change();
        let mut k = 0;
        let mut top = None;
        for l in first..=last.min(last_line(doc)) {
            let from = match (&target, from_col) {
                (_, Some(p)) if l == first => p,
                (super::Target::Block { left, .. }, _) => at_column(doc, l, *left),
                _ => line_start(doc, l),
            };
            let step = if progressive { by * (k + 1) } else { by };
            let (s, e) = (line_start(doc, l), line_end(doc, l));
            let text = doc.text().as_str()[s..e].to_string();
            if let Some((range, new)) = number_at(&text, from - s, step, &self.options.nrformats) {
                edit(doc, s + range.start..s + range.end, &new, s + range.start);
                top.get_or_insert(s + range.start);
                k += 1;
            }
        }
        let to = top.unwrap_or_else(|| line_start(doc, first));
        doc.selection = Selection::caret(to);
        self.cursor = to;
        self.changed();
    }

    /// `gn` and `gN`: the next (or previous) match of the last search,
    /// selected; with an operator, operated on (`cgn`, which `.` repeats
    /// on the next match).
    pub(super) fn select_match(
        &mut self,
        doc: &mut DocumentState,
        back: bool,
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        let Some((pat, _)) = self.last_search.clone() else {
            return self.reset();
        };
        let Ok(matches) = self.matches(doc, &pat) else {
            return self.reset();
        };
        let at = self.cursor;
        let m = if back {
            matches
                .iter()
                .rev()
                .find(|m| m.start <= at)
                .or(matches.last())
        } else {
            matches.iter().find(|m| m.end > at).or(matches.first())
        };
        let Some(m) = m.cloned().filter(|m| m.end > m.start) else {
            return self.reset();
        };
        if let Some((op, _)) = self.op.take() {
            self.count = None;
            self.begin_change_if(op);
            self.apply_op(doc, op, super::Target::Chars(m), host, out);
            self.changed_if(op);
            return;
        }
        if !self.visual() {
            self.mode = Mode::Visual;
        }
        self.anchor = m.start;
        self.cursor = doc.grapheme_before(m.end).max(m.start);
    }
}
