//! The command line's editing commands (`:help ex-cmd-index`), with
//! ranges (`%`, `.`, `$`, numbers, marks, `/pat/`, `?pat?`, offsets):
//! `:s`, `:&`, `:~`, `:g`, `:v`, `:d`, `:y`, `:pu`, `:m`, `:t`, `:co`,
//! `:j`, `:>`, `:<`, `:norm`, `:sor`, `:k`, `:ma`, `:u`, `:red`, `:le`,
//! `:ret`, `:=`, `:p`, `:reg`, `:marks`, `:set`. What is about files,
//! windows and buffers goes to the editor's commands.

use org_edit::Selection;

use super::{
    DocumentState, Host, Key, Mode, Outcome, Vim, edit, first_non_blank, last_line, line_end,
    line_of, line_span, line_start, pattern::Pattern,
};

/// A command line's range (first and last line, if any) and the rest.
type Ranged<'a> = (Option<(usize, usize)>, &'a str);

/// The last `:s`: its pattern, replacement and flags.
#[derive(Debug, Clone, Default)]
pub(super) struct Substitute {
    pattern: String,
    replacement: String,
    flags: String,
}

/// Splits `s` at the first unescaped `delim`: what comes before (the
/// escape before `delim` dropped) and after.
fn split_unescaped(s: &str, delim: char) -> (String, Option<&str>) {
    let mut out = String::new();
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some((_, d)) if d == delim => out.push(d),
                Some((_, d)) => {
                    out.push('\\');
                    out.push(d);
                }
                None => out.push('\\'),
            }
        } else if c == delim {
            return (out, Some(&s[i + c.len_utf8()..]));
        } else {
            out.push(c);
        }
    }
    (out, None)
}

/// Expands a `:s` replacement for one match: `&` and `\0` the match, `\1`
/// to `\9` its groups, `\r` and `\n` a line break, `\t` a tab, `\u`,
/// `\l`, `\U`, `\L`, `\e` and `\E` the case of what follows.
/// `s` with its case folded a character for a character, as Vim's
/// `:sort i` compares (a letter whose lower case is two, `İ`, stays).
fn fold_case(s: &str) -> String {
    s.chars()
        .map(|c| {
            let mut l = c.to_lowercase();
            match (l.next(), l.next()) {
                (Some(one), None) => one,
                _ => c,
            }
        })
        .collect()
}

/// Where a `|` not after a backslash ends the command in `s`, if one does.
fn bar_at(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    (0..b.len()).find(|&i| b[i] == b'|' && (i == 0 || b[i - 1] != b'\\'))
}

fn expand(rep: &str, caps: &regex::Captures<'_>) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum Case {
        None,
        Upper,
        Lower,
    }
    let mut out = String::new();
    let mut one = Case::None;
    let mut all = Case::None;
    let push = |out: &mut String, s: &str, one: &mut Case, all: Case| {
        for c in s.chars() {
            // A character for a character, as Vim changes case.
            let c = match (*one, all) {
                (Case::Upper, _) | (_, Case::Upper) => super::upper_char(c),
                (Case::Lower, _) | (_, Case::Lower) => super::lower_char(c),
                _ => c,
            };
            *one = Case::None;
            out.push(c);
        }
    };
    let whole = caps
        .name("m")
        .or_else(|| caps.get(0))
        .map_or("", |m| m.as_str());
    let mut chars = rep.chars();
    while let Some(c) = chars.next() {
        match c {
            '&' => push(&mut out, whole, &mut one, all),
            '\\' => match chars.next() {
                Some('0') => push(&mut out, whole, &mut one, all),
                Some(d @ '1'..='9') => {
                    let g = caps
                        .get(d as usize - '0' as usize)
                        .map_or("", |m| m.as_str());
                    push(&mut out, g, &mut one, all);
                }
                Some('r' | 'n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('u') => one = Case::Upper,
                Some('l') => one = Case::Lower,
                Some('U') => all = Case::Upper,
                Some('L') => all = Case::Lower,
                Some('e' | 'E') => all = Case::None,
                Some(d) => push(&mut out, &d.to_string(), &mut one, all),
                None => out.push('\\'),
            },
            c => push(&mut out, &c.to_string(), &mut one, all),
        }
    }
    out
}

impl Vim {
    /// Runs command line `line` (without the `:`).
    pub(super) fn ex(
        &mut self,
        doc: &mut DocumentState,
        line: &str,
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        if !line.trim().is_empty() && self.macro_depth == 0 {
            self.last_ex = Some(line.to_string());
        }
        self.run_ex(doc, line, host, out);
        if self.visual() {
            self.mode = Mode::Normal;
        }
        self.reset();
        self.keys.clear();
    }

    /// One address: its line (from 0) and the rest of the text.
    fn address<'a>(
        &mut self,
        doc: &DocumentState,
        s: &'a str,
        cur: usize,
    ) -> Result<Option<(usize, &'a str)>, String> {
        let last = last_line(doc);
        let cur = cur.min(last);
        let mut rest = s.trim_start();
        let mut line = match rest.chars().next() {
            Some('.') => {
                rest = &rest[1..];
                Some(cur)
            }
            Some('$') => {
                rest = &rest[1..];
                Some(last)
            }
            Some(c) if c.is_ascii_digit() => {
                let n: String = rest.chars().take_while(char::is_ascii_digit).collect();
                rest = &rest[n.len()..];
                // Line 0 is before the first (`:m0`); one past the last is
                // left for the command to refuse.
                Some(n.parse::<usize>().unwrap_or(1).saturating_sub(1)).map(|l| {
                    if n.trim_start_matches('0').is_empty() {
                        usize::MAX
                    } else {
                        l
                    }
                })
            }
            Some('\'') => {
                let m = rest[1..].chars().next().ok_or("E20: Mark not set")?;
                rest = &rest[1 + m.len_utf8()..];
                let p = self.mark(doc, m).ok_or("E20: Mark not set")?;
                Some(line_of(doc, p))
            }
            Some(d @ ('/' | '?')) => {
                let (pat, after) = split_unescaped(&rest[1..], d);
                rest = after.unwrap_or("");
                let pat = if pat.is_empty() {
                    self.last_search.clone().map(|p| p.0).unwrap_or_default()
                } else {
                    pat
                };
                self.last_search = Some((pat.clone(), d == '?'));
                let p = Pattern::new(&pat, self.options.ignorecase, self.options.smartcase)?;
                let lines: Vec<usize> = (0..=last).collect();
                let hit = |l: &usize| {
                    let r = line_start(doc, *l)..line_end(doc, *l);
                    p.is_match(&doc.text().as_str()[r])
                };
                let found = if d == '/' {
                    lines[cur + 1..]
                        .iter()
                        .chain(&lines[..=cur])
                        .find(|l| hit(l))
                } else {
                    lines[..cur]
                        .iter()
                        .rev()
                        .chain(lines[cur..].iter().rev())
                        .find(|l| hit(l))
                };
                Some(*found.ok_or_else(|| format!("E486: Pattern not found: {pat}"))?)
            }
            Some('+' | '-') => Some(cur),
            _ => None,
        };
        // Offsets: `+3`, `-`, `++`.
        loop {
            let t = rest.trim_start();
            let Some(sign) = t.chars().next().filter(|c| *c == '+' || *c == '-') else {
                break;
            };
            let digits: String = t[1..].chars().take_while(char::is_ascii_digit).collect();
            let n: isize = if digits.is_empty() {
                1
            } else {
                digits.parse().unwrap_or(1)
            };
            let base = line.unwrap_or(cur);
            let base = if base == usize::MAX {
                0
            } else {
                base as isize + 1
            } - 1;
            let l = if sign == '+' { base + n } else { base - n };
            // Line 0 (one before the first) a command takes as the first;
            // before it there is nothing.
            line = Some(match l {
                -1 => usize::MAX,
                l if l < -1 => return Err("E16: Invalid range".into()),
                l => l as usize,
            });
            rest = &t[1 + digits.len()..];
        }
        Ok(line.map(|l| (l, rest)))
    }

    /// The range at the start of `cmd`: the first and last lines (`None`
    /// when there is none), and the rest.
    fn range<'a>(&mut self, doc: &DocumentState, cmd: &'a str) -> Result<Ranged<'a>, String> {
        let cur = line_of(doc, self.cursor);
        let t = cmd.trim_start();
        if let Some(rest) = t.strip_prefix('%') {
            return Ok((Some((0, last_line(doc))), rest));
        }
        let Some((a, mut rest)) = self.address(doc, t, cur)? else {
            return Ok((None, t));
        };
        let mut b = a;
        let mut first = a;
        while let Some(sep) = rest
            .trim_start()
            .chars()
            .next()
            .filter(|c| *c == ',' || *c == ';')
        {
            let after = &rest.trim_start()[1..];
            let base = if sep == ';' && b != usize::MAX {
                b
            } else {
                cur
            };
            match self.address(doc, after, base)? {
                Some((l, r)) => {
                    first = b;
                    b = l;
                    rest = r;
                }
                None => {
                    first = b;
                    b = cur;
                    rest = after;
                }
            }
        }
        let a = if first == usize::MAX { 0 } else { first };
        Ok((Some((a.min(b), a.max(b))), rest))
    }

    fn run_ex(
        &mut self,
        doc: &mut DocumentState,
        line: &str,
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        let (range, rest) = match self.range(doc, line) {
            Ok(r) => r,
            Err(e) => {
                out.message = Some((e, true));
                return;
            }
        };
        let rest = rest.trim_start();
        // The command's name: letters, or one of the signs.
        let name_len = if rest.starts_with(['&', '~', '<', '>', '=', '!', '#']) {
            1
        } else {
            rest.chars().take_while(char::is_ascii_alphabetic).count()
        };
        let name = &rest[..name_len];
        // `|` ends a command and begins the next, but for those that take
        // it in their argument (`:g`, `:normal`, `:!`).
        let takes_bar = name == "!"
            || (!name.is_empty() && ("global".starts_with(name) || "vglobal".starts_with(name)))
            || (name.len() >= 4 && "normal".starts_with(name));
        if !takes_bar && let Some(i) = bar_at(&rest[name_len..]) {
            let cut = line.len() - rest.len() + name_len + i;
            let (first, next) = (line[..cut].to_string(), line[cut + 1..].to_string());
            self.run_ex(doc, &first, host, out);
            if !out.message.as_ref().is_some_and(|m| m.1) {
                self.run_ex(doc, &next, host, out);
            }
            return;
        }
        let mut args = &rest[name_len..];
        let bang = args.starts_with('!') && !matches!(name, "!" | "s" | "g");
        if bang {
            args = &args[1..];
        }
        let cur = line_of(doc, self.cursor);
        let last = last_line(doc);
        // Lines past the last: a command refuses them (E16); a line number
        // alone goes to the last line.
        let past = |l: usize| l != usize::MAX && l > last;
        if let Some((ra, rb)) = range
            && !name.is_empty()
            && (past(ra) || past(rb))
        {
            out.message = Some(("E16: Invalid range".into(), true));
            return;
        }
        let (a, b) = range.unwrap_or((cur, cur));
        // Line 0 is the first for a command that works on lines.
        let fix = |l: usize| if l == usize::MAX { 0 } else { l.min(last) };
        let (a, b) = (fix(a), fix(b).max(fix(a)));
        let is = |short: &str, long: &str| name.len() >= short.len() && long.starts_with(name);
        match name {
            "" => {
                // A range alone goes to its last line.
                if range.is_some() {
                    self.jump(doc);
                    self.goto_line(doc, b.min(last));
                }
            }
            "s" | "&" | "~" => self.substitute(doc, name, args, a, b, range.is_some(), out),
            _ if name.len() >= 2 && "substitute".starts_with(name) => {
                self.substitute(doc, "s", args, a, b, range.is_some(), out)
            }
            "g" | "v" => {
                let invert = name == "v" || args.starts_with('!');
                let args = args.strip_prefix('!').unwrap_or(args);
                let (a, b) = range.unwrap_or((0, last));
                self.global(doc, args, a, b, invert, host, out);
            }
            _ if is("g", "global") || is("v", "vglobal") => {
                let invert = name.starts_with('v') || bang;
                let (a, b) = range.unwrap_or((0, last));
                self.global(doc, args, a, b, invert, host, out);
            }
            _ if is("d", "delete") => {
                let (reg, count) = reg_count(args);
                let (a, b) = count.map_or((a, b), |n| (b, (b + n - 1).min(last)));
                self.register = reg;
                let text = lines_text(doc, a, b);
                self.store(text, true, false, host);
                // Vim goes to the first non-blank first: undo comes back
                // there.
                doc.selection = Selection::caret(first_non_blank(doc, a));
                let span = line_span(doc, a, b);
                edit(doc, span.clone(), "", span.start);
                self.goto_line(doc, a.min(last_line(doc)));
            }
            _ if is("y", "yank") => {
                let (reg, count) = reg_count(args);
                let (a, b) = count.map_or((a, b), |n| (b, (b + n - 1).min(last)));
                self.register = reg;
                let text = lines_text(doc, a, b);
                self.store(text, true, true, host);
            }
            _ if is("pu", "put") => {
                let reg = args.trim().chars().next();
                self.register = reg;
                let Some(r) = self.fetch(host) else {
                    return;
                };
                let mut t = r.text.clone();
                if !t.ends_with('\n') {
                    t.push('\n');
                }
                // `:0put` and `:put!` above the line.
                let above = bang || range.is_some_and(|r| r.1 == usize::MAX);
                let at_line = if range.is_some_and(|r| r.1 == usize::MAX) {
                    0
                } else {
                    b
                };
                self.put_lines(doc, at_line, above, &t);
            }
            _ if is("m", "move") => {
                let Ok(Some((to, _))) = self.address(doc, args, cur) else {
                    out.message = Some(("E14: Invalid address".into(), true));
                    return;
                };
                self.move_lines(doc, a, b, to, false);
            }
            "t" => self.copy_to(doc, args, a, b, cur, out),
            _ if is("co", "copy") => self.copy_to(doc, args, a, b, cur, out),
            _ if is("j", "join") => {
                let (_, count) = reg_count(args);
                let (a, b) = match (range, count) {
                    (_, Some(n)) => (b, (b + n - 1).min(last)),
                    (Some((a, b)), None) if a != b => (a, b),
                    _ => (a, (a + 1).min(last)),
                };
                if b > a {
                    if bang {
                        self.join_raw(doc, a, b - a + 1);
                    } else {
                        self.join(doc, a, b - a + 1);
                    }
                    self.goto_line(doc, a);
                }
            }
            ">" | "<" => {
                let depth = 1 + args.chars().take_while(|c| c.to_string() == name).count();
                let rest = args.trim_start_matches(name.chars().next().unwrap_or('>'));
                let (_, count) = reg_count(rest);
                let (a, b) = count.map_or((a, b), |n| (b, (b + n - 1).min(last)));
                doc.selection = Selection::caret(first_non_blank(doc, a));
                for _ in 0..depth {
                    self.shift_lines(doc, a, b, name == ">", out);
                }
                self.goto_line(doc, b);
            }
            _ if is("norm", "normal") => {
                let keys: Vec<Key> = args.trim_start().chars().map(Key::Char).collect();
                let lines: Vec<Option<usize>> = match range {
                    Some((a, b)) => (a..=b).map(Some).collect(),
                    // Without a range: from the cursor where it is.
                    None => vec![None],
                };
                // Line by line number, as Vim goes: a line the keys
                // delete moves the next up past it, and past the end the
                // last line takes the rest.
                for &l in &lines {
                    let p = l.map_or(self.cursor, |l| line_start(doc, l.min(last_line(doc))));
                    doc.selection = Selection::caret(p);
                    self.cursor = p;
                    self.mode = Mode::Normal;
                    self.reset();
                    // A command that fails stops this line's keys only.
                    self.macro_depth += 1;
                    self.failed = false;
                    self.feed(doc, &keys, host, out);
                    // An unfinished command or insert ends there.
                    self.failed = false;
                    self.feed(doc, &[Key::Esc], host, out);
                    self.failed = false;
                    self.macro_depth -= 1;
                }
            }
            _ if is("sor", "sort") => self.sort(doc, args, range.unwrap_or((0, last)), bang),
            "k" => self.set_mark(doc, args, b),
            // `:ka`: `k` takes its mark right after it (not `:ke…`).
            _ if name.len() == 2 && name.starts_with('k') && !name.starts_with("ke") => {
                self.set_mark(doc, &name[1..], b)
            }
            _ if is("ma", "mark") => self.set_mark(doc, args, b),
            _ if is("u", "undo") => {
                let _ = doc.undo();
            }
            _ if is("red", "redo") => {
                let _ = doc.redo_from_start();
            }
            _ if is("le", "left") => {
                let indent: usize = args.trim().parse().unwrap_or(0);
                let ts = self.options.tabstop.max(1);
                for l in (a..=b).rev() {
                    let (s, fnb) = (line_start(doc, l), first_non_blank(doc, l));
                    let ind = super::insert::indent_string(indent, ts, self.options.expandtab);
                    // Blank lines too, as in Vim.
                    if doc.text().as_str()[s..fnb] != ind {
                        edit(doc, s..fnb, &ind, s);
                    }
                }
                self.goto_line(doc, a);
            }
            // `:right` and `:center` in `width` columns (80 when not
            // given): the indent set so the text (its blanks around left
            // out) ends there or sits in the middle; blank lines stay.
            _ if is("ri", "right") || is("ce", "center") => {
                use unicode_width::UnicodeWidthChar;
                let width = args.trim().parse::<usize>().ok().filter(|w| *w > 0).unwrap_or(80);
                let ts = self.options.tabstop.max(1);
                let right = name.starts_with('r');
                for l in a..=b {
                    let (fnb, e) = (first_non_blank(doc, l), line_end(doc, l));
                    let body = doc.text().as_str()[fnb..e].trim_end_matches([' ', '\t']);
                    let start = super::insert::vcol(doc, fnb, ts);
                    let len = body.chars().fold(start, |col, c| {
                        if c == '\t' { (col / ts + 1) * ts } else { col + c.width().unwrap_or(0) }
                    }) - start;
                    if len == 0 {
                        continue;
                    }
                    let want = if right { width.saturating_sub(len) } else { width.saturating_sub(len) / 2 };
                    let ind = super::insert::indent_string(want, ts, self.options.expandtab);
                    let s = line_start(doc, l);
                    if doc.text().as_str()[s..fnb] != ind {
                        edit(doc, s..fnb, &ind, s);
                    }
                }
                let line = cur.min(last_line(doc));
                let at = first_non_blank(doc, line);
                doc.selection = Selection::caret(at);
                self.cursor = at;
            }
            // Every run of blanks with a tab in it (with `!` any run)
            // made again for the new 'tabstop' (`:retab 4`), all spaces
            // with 'expandtab'; where the text shows stays the same.
            _ if is("ret", "retab") => {
                use unicode_width::UnicodeWidthChar;
                let old_ts = self.options.tabstop.max(1);
                let new_ts = args.trim().parse::<usize>().ok().filter(|n| *n > 0).unwrap_or(old_ts);
                let et = self.options.expandtab;
                let blanks = |from: usize, to: usize| {
                    if et {
                        return " ".repeat(to - from);
                    }
                    let mut v = from;
                    let mut w = String::new();
                    while (v / new_ts + 1) * new_ts <= to {
                        w.push('\t');
                        v = (v / new_ts + 1) * new_ts;
                    }
                    w.push_str(&" ".repeat(to - v));
                    w
                };
                // The whole text without a range.
                let (a, b) = if range.is_some() { (a, b) } else { (0, last) };
                for l in (a..=b).rev() {
                    let (s, e) = (line_start(doc, l), line_end(doc, l));
                    let text = doc.text().as_str()[s..e].to_string();
                    let chars: Vec<char> = text.chars().collect();
                    let mut new = String::with_capacity(text.len());
                    let (mut i, mut vcol) = (0, 0);
                    while i < chars.len() {
                        if !matches!(chars[i], ' ' | '\t') {
                            vcol += chars[i].width().unwrap_or(0);
                            new.push(chars[i]);
                            i += 1;
                            continue;
                        }
                        let (from, mut j, mut tab) = (vcol, i, false);
                        while j < chars.len() && matches!(chars[j], ' ' | '\t') {
                            if chars[j] == '\t' {
                                tab = true;
                                vcol = (vcol / old_ts + 1) * old_ts;
                            } else {
                                vcol += 1;
                            }
                            j += 1;
                        }
                        // With `!` a run of spaces too, where it gets shorter.
                        let made = blanks(from, vcol);
                        if (tab || (bang && j - i > 1)) && (tab || et || made.len() < j - i) {
                            new.push_str(&made);
                        } else {
                            new.extend(&chars[i..j]);
                        }
                        i = j;
                    }
                    if new != text {
                        edit(doc, s..e, &new, s);
                    }
                }
                self.options.tabstop = new_ts;
                self.goto_line(doc, cur);
            }
            "=" => {
                let n = if range.is_some() { b + 1 } else { last + 1 };
                out.message = Some((n.to_string(), false));
            }
            "#" => self.print_lines(doc, a, b, true, out),
            _ if is("p", "print") || is("nu", "number") || is("l", "list") => {
                self.print_lines(doc, a, b, name.starts_with('n'), out)
            }
            _ if is("reg", "registers") || is("di", "display") => {
                let list: Vec<String> = self
                    .register_texts()
                    .into_iter()
                    .map(|(c, t)| format!("\"{c}   {}", t.replace('\n', "^J")))
                    .collect();
                out.message = Some((list.join("  "), false));
            }
            "marks" => {
                let mut m: Vec<(char, usize)> =
                    doc.marks.named.iter().map(|(c, p)| (*c, *p)).collect();
                m.sort_unstable();
                let list: Vec<String> = m
                    .into_iter()
                    .map(|(c, p)| format!("{c} {}:{}", line_of(doc, p) + 1, super::column(doc, p)))
                    .collect();
                out.message = Some((list.join("  "), false));
            }
            _ if is("se", "set") || is("setl", "setlocal") => self.set(args, out),
            _ => {
                let cmd = if range.is_some() || bang {
                    rest.to_string()
                } else {
                    line.trim().to_string()
                };
                self.ex_app(doc, &cmd, out);
            }
        }
    }

    /// The cursor to `line`'s first non-blank.
    fn goto_line(&mut self, doc: &mut DocumentState, line: usize) {
        let p = first_non_blank(doc, line.min(last_line(doc)));
        doc.selection = Selection::caret(p);
        self.cursor = p;
    }

    fn print_lines(
        &self,
        doc: &DocumentState,
        a: usize,
        b: usize,
        numbers: bool,
        out: &mut Outcome,
    ) {
        let list: Vec<String> = (a..=b.min(last_line(doc)))
            .map(|l| {
                let t = &doc.text().as_str()[line_start(doc, l)..line_end(doc, l)];
                if numbers {
                    format!("{:>3} {t}", l + 1)
                } else {
                    t.to_string()
                }
            })
            .collect();
        out.message = Some((list.join("  "), false));
    }

    fn set_mark(&mut self, doc: &mut DocumentState, args: &str, line: usize) {
        if let Some(c) = args.trim().chars().next() {
            let p = line_start(doc, line.min(last_line(doc)));
            doc.marks.named.insert(c, p);
        }
    }

    /// `:t` and `:co`: the lines again after line `to` (`0`: at the top).
    fn copy_to(
        &mut self,
        doc: &mut DocumentState,
        args: &str,
        a: usize,
        b: usize,
        cur: usize,
        out: &mut Outcome,
    ) {
        let Ok(Some((to, _))) = self.address(doc, args, cur) else {
            out.message = Some(("E14: Invalid address".into(), true));
            return;
        };
        let text = lines_text(doc, a, b);
        let above = to == usize::MAX;
        self.put_lines(doc, if above { 0 } else { to }, above, &text);
    }

    /// Lines `text` (each with its line feed) below `line`, or above it;
    /// the cursor on the last.
    fn put_lines(&mut self, doc: &mut DocumentState, line: usize, above: bool, text: &str) {
        let n = text.matches('\n').count();
        let len = doc.text().len();
        let (at, insert, first) = if above {
            (line_start(doc, line), text.to_string(), line)
        } else if line + 1 < doc.text().line_count() {
            (line_start(doc, line + 1), text.to_string(), line + 1)
        } else {
            (len, format!("\n{}", text.trim_end_matches('\n')), line + 1)
        };
        edit(doc, at..at, &insert, at);
        self.goto_line(doc, first + n.saturating_sub(1));
    }

    /// `:m`: lines `a..=b` after line `to` (`usize::MAX`: to the top).
    pub(super) fn move_lines(
        &mut self,
        doc: &mut DocumentState,
        a: usize,
        b: usize,
        to: usize,
        _keep: bool,
    ) {
        // Into itself, or where the lines are already: nothing moves.
        if (to != usize::MAX && to + 1 >= a && to <= b) || (to == usize::MAX && a == 0) {
            self.goto_line(doc, b);
            return;
        }
        let text = lines_text(doc, a, b);
        let n = b - a + 1;
        let span = line_span(doc, a, b);
        edit(doc, span.clone(), "", span.start);
        let dest = if to == usize::MAX {
            usize::MAX
        } else if to > b {
            to - n
        } else {
            to
        };
        let (line, above) = if dest == usize::MAX {
            (0, true)
        } else {
            (dest, false)
        };
        self.put_lines(doc, line, above, &text);
    }

    /// `:sort` with `n` (by the first number), `u` (unique), `i` (case
    /// ignored), `r` with a pattern (by what it matches), `!` (reversed).
    fn sort(&mut self, doc: &mut DocumentState, args: &str, (a, b): (usize, usize), reverse: bool) {
        let mut flags = String::new();
        let mut pat = None;
        let mut rest = args.trim();
        while let Some(c) = rest.chars().next() {
            if c == '/' {
                let (p, after) = split_unescaped(&rest[1..], '/');
                pat = Some(p);
                rest = after.unwrap_or("");
            } else {
                flags.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
        let numeric = flags.contains('n');
        let fold = flags.contains('i');
        let unique = flags.contains('u');
        let by_match = flags.contains('r');
        let pat = pat
            .filter(|p| !p.is_empty())
            .and_then(|p| Pattern::new(&p, self.options.ignorecase, self.options.smartcase).ok());
        let lines: Vec<String> = (a..=b.min(last_line(doc)))
            .map(|l| doc.text().as_str()[line_start(doc, l)..line_end(doc, l)].to_string())
            .collect();
        let key = |l: &str| -> String {
            let k = match &pat {
                Some(p) => {
                    let m = p.find_all(l).into_iter().next();
                    match (m, by_match) {
                        (Some(r), true) => l[r].to_string(),
                        (Some(r), false) => l[r.end..].to_string(),
                        (None, true) => String::new(),
                        (None, false) => l.to_string(),
                    }
                }
                None => l.to_string(),
            };
            if fold { fold_case(&k) } else { k }
        };
        let number = |k: &str| -> Option<i64> {
            let start = k.find(|c: char| c.is_ascii_digit())?;
            let neg = start > 0 && k.as_bytes()[start - 1] == b'-';
            let digits: String = k[start..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            digits.parse::<i64>().ok().map(|n| if neg { -n } else { n })
        };
        let mut sorted = lines.clone();
        if numeric {
            // Lines without a number first, in their order.
            sorted.sort_by_key(|l| number(&key(l)).map_or((0, 0), |n| (1, n)));
        } else {
            sorted.sort_by_key(|l| key(l));
        }
        if reverse {
            sorted.reverse();
        }
        if unique {
            sorted.dedup_by(|x, y| {
                if fold {
                    fold_case(x) == fold_case(y)
                } else {
                    x == y
                }
            });
        }
        if sorted != lines {
            let r = line_start(doc, a)..line_end(doc, b.min(last_line(doc)));
            edit(doc, r.clone(), &sorted.join("\n"), r.start);
        }
        self.goto_line(doc, a);
    }

    /// `:g/pat/cmd` and `:v/pat/cmd`: `cmd` (`:p` when none) on every
    /// line that matches (or does not), in order, as the lines move.
    #[allow(clippy::too_many_arguments)]
    fn global(
        &mut self,
        doc: &mut DocumentState,
        args: &str,
        a: usize,
        b: usize,
        invert: bool,
        host: &mut dyn Host,
        out: &mut Outcome,
    ) {
        let Some(delim) = args.chars().next() else {
            out.message = Some(("E35: No previous regular expression".into(), true));
            return;
        };
        let (pat, cmd) = split_unescaped(&args[delim.len_utf8()..], delim);
        let pat = if pat.is_empty() {
            self.last_search.clone().map(|p| p.0).unwrap_or_default()
        } else {
            pat
        };
        self.last_search = Some((pat.clone(), false));
        let p = match Pattern::new(&pat, self.options.ignorecase, self.options.smartcase) {
            Ok(p) => p,
            Err(e) => {
                out.message = Some((e, true));
                return;
            }
        };
        let cmd = cmd.unwrap_or("").trim().to_string();
        let cmd = if cmd.is_empty() { "p".to_string() } else { cmd };
        // Each line found held by its start and the start of the next:
        // they meet when the command deletes it.
        let base = doc.marks.held.len();
        let len = doc.text().len();
        for l in a..=b.min(last_line(doc)) {
            let t = &doc.text().as_str()[line_start(doc, l)..line_end(doc, l)];
            if p.is_match(t) != invert {
                doc.marks.held.push(line_start(doc, l));
                doc.marks.held.push((line_end(doc, l) + 1).min(len));
            }
        }
        let count = (doc.marks.held.len() - base) / 2;
        for i in 0..count {
            let pos = doc.marks.held[base + 2 * i].min(doc.text().len());
            let next = doc.marks.held[base + 2 * i + 1].min(doc.text().len());
            // A line deleted (or joined to another) already is not run on
            // again.
            let start = line_start(doc, line_of(doc, pos));
            if start != pos || next <= pos {
                continue;
            }
            doc.selection = Selection::caret(pos);
            self.cursor = pos;
            self.run_ex(doc, &cmd, host, out);
        }
        doc.marks.held.truncate(base);
    }

    /// `:s/pat/rep/flags count`, `:&` (again, `&&` with the flags) and
    /// `:~` (again with the last search pattern).
    #[allow(clippy::too_many_arguments)]
    fn substitute(
        &mut self,
        doc: &mut DocumentState,
        name: &str,
        args: &str,
        a: usize,
        b: usize,
        _ranged: bool,
        out: &mut Outcome,
    ) {
        let last = self.last_sub.clone().unwrap_or_default();
        let (pattern, replacement, flags) = match args.chars().next() {
            Some(d)
                if name == "s"
                    && !d.is_alphanumeric()
                    && !d.is_whitespace()
                    && d != '"'
                    && d != '&' =>
            {
                let (pat, rest) = split_unescaped(&args[d.len_utf8()..], d);
                let (rep, flags) = match rest {
                    Some(r) => {
                        let (rep, f) = split_unescaped(r, d);
                        (rep, f.unwrap_or("").to_string())
                    }
                    None => (String::new(), String::new()),
                };
                let pat = if pat.is_empty() {
                    self.last_search
                        .clone()
                        .map(|p| p.0)
                        .unwrap_or(last.pattern.clone())
                } else {
                    pat
                };
                // `~` is the replacement before.
                let rep = if rep.contains('~') {
                    let mut out = String::new();
                    let mut chars = rep.chars().peekable();
                    while let Some(c) = chars.next() {
                        match c {
                            '\\' => {
                                out.push('\\');
                                if let Some(n) = chars.next() {
                                    out.push(n);
                                }
                            }
                            '~' => out.push_str(&last.replacement),
                            c => out.push(c),
                        }
                    }
                    out
                } else {
                    rep
                };
                (pat, rep, flags)
            }
            _ => {
                // Again: `:s`, `:&`, `:&&`, `:~`, with new flags after.
                let mut flags = args.trim().to_string();
                if let Some(f) = flags.strip_prefix('&') {
                    flags = format!("{}{f}", last.flags);
                }
                let pat = if name == "~" {
                    self.last_search
                        .clone()
                        .map(|p| p.0)
                        .unwrap_or(last.pattern.clone())
                } else {
                    last.pattern.clone()
                };
                (pat, last.replacement.clone(), flags)
            }
        };
        let (flags, count) = {
            let digits: String = flags.chars().filter(char::is_ascii_digit).collect();
            let f: String = flags
                .chars()
                .filter(|c| !c.is_ascii_digit() && !c.is_whitespace())
                .collect();
            (f, digits.parse::<usize>().ok())
        };
        let flags = if let Some(f) = flags.strip_prefix('&') {
            format!("{}{f}", last.flags)
        } else {
            flags
        };
        self.last_sub = Some(Substitute {
            pattern: pattern.clone(),
            replacement: replacement.clone(),
            flags: flags.clone(),
        });
        self.last_search = Some((pattern.clone(), false));
        let (a, b) = match count {
            Some(n) => (b, (b + n - 1).min(last_line(doc))),
            None => (a, b),
        };
        let ic = if flags.contains('I') {
            false
        } else {
            flags.contains('i') || self.options.ignorecase
        };
        let p = match Pattern::new(&pattern, ic, self.options.smartcase && !flags.contains('i')) {
            Ok(p) => p,
            Err(e) => {
                out.message = Some((e, true));
                return;
            }
        };
        let global = flags.matches('g').count() % 2 == 1;
        let count_only = flags.contains('n');
        let b = b.min(last_line(doc));
        // A pattern over line breaks (`\n`, `\_s`): matched in the text
        // from the first line on, each starting in the range.
        if pattern.contains("\\n") || pattern.contains("\\_") {
            let m = (&p, pattern.as_str(), replacement.as_str());
            return self.substitute_lines(doc, m, (a, b), (global, count_only), out);
        }
        // Undo comes back to the first line changed, as in Vim.
        if !count_only
            && let Some(l) = (a..=b).find(|&l| {
                p.is_match(&doc.text().as_str()[line_start(doc, l)..line_end(doc, l)])
            })
        {
            doc.selection = Selection::caret(line_start(doc, l));
        }
        let mut n = 0;
        let mut last_line_done = None;
        // Line breaks the replacements made, all and in the last line.
        let mut added = 0;
        let mut last_breaks = 0;
        // Last line first, so the earlier lines stay where they are.
        for l in (a..=b.min(last_line(doc))).rev() {
            let (s, e) = (line_start(doc, l), line_end(doc, l));
            let text = doc.text().as_str()[s..e].to_string();
            let mut new = String::new();
            let mut at = 0;
            let mut changed = false;
            for caps in p.regex().captures_iter(&text) {
                let Some(m) = caps.name("m").or_else(|| caps.get(0)) else {
                    continue;
                };
                // An empty match right after the last is skipped.
                if m.start() < at {
                    continue;
                }
                new.push_str(&text[at..m.start()]);
                new.push_str(&expand(&replacement, &caps));
                at = m.end();
                changed = true;
                n += 1;
                if !global {
                    break;
                }
            }
            if !changed {
                continue;
            }
            new.push_str(&text[at..]);
            last_line_done.get_or_insert(l);
            if !count_only {
                added += new.matches('\n').count();
                if last_line_done == Some(l) {
                    last_breaks = new.matches('\n').count();
                }
                edit(doc, s..e, &new, s);
            }
        }
        match last_line_done {
            None => {
                if !flags.contains('e') {
                    out.message = Some((format!("E486: Pattern not found: {pattern}"), true));
                }
            }
            Some(l) if count_only => {
                let _ = l;
                out.message = Some((format!("{n} matches"), false));
            }
            Some(l) => {
                // The last line of the last replacement.
                let line = l + (added - last_breaks) + last_breaks;
                let p = first_non_blank(doc, line.min(last_line(doc)));
                doc.selection = Selection::caret(p);
                self.cursor = p;
                self.note_change(doc, p);
            }
        }
    }

    /// `:s` with a pattern that matches line breaks: over the text from
    /// line `a`, each match starting in lines `a..=b` (the first of each
    /// line without `g`); the cursor at the last replacement.
    fn substitute_lines(
        &mut self,
        doc: &mut DocumentState,
        (p, pattern, replacement): (&Pattern, &str, &str),
        (a, b): (usize, usize),
        (global, count_only): (bool, bool),
        out: &mut Outcome,
    ) {
        let start = line_start(doc, a);
        let limit = line_end(doc, b);
        // The text's last line break is not one between lines.
        let all = doc.text().as_str();
        let end = if all.ends_with('\n') { all.len() - 1 } else { all.len() };
        let text = all[start..end.max(start)].to_string();
        let mut new = String::with_capacity(text.len());
        let (mut at, mut n) = (0, 0);
        let mut last_start = 0;
        let mut first_line = None;
        let mut line_done = None;
        for caps in p.regex().captures_iter(&text) {
            let Some(m) = caps.name("m").or_else(|| caps.get(0)) else {
                continue;
            };
            if start + m.start() > limit {
                break;
            }
            if m.start() < at {
                continue;
            }
            let l = line_of(doc, start + m.start());
            if !global && line_done == Some(l) {
                continue;
            }
            line_done = Some(l);
            first_line.get_or_insert(l);
            new.push_str(&text[at..m.start()]);
            last_start = new.len();
            new.push_str(&expand(replacement, &caps));
            at = m.end();
            n += 1;
        }
        let Some(first) = first_line else {
            out.message = Some((format!("E486: Pattern not found: {pattern}"), true));
            return;
        };
        if count_only {
            out.message = Some((format!("{n} matches"), false));
            return;
        }
        new.push_str(&text[at..]);
        doc.selection = Selection::caret(line_start(doc, first));
        edit(doc, start..end.max(start), &new, start + last_start);
        let p = (start + last_start).min(doc.text().len());
        doc.selection = Selection::caret(p);
        self.cursor = p;
        self.note_change(doc, p);
    }

    /// `:set`: options by name, `no` and `inv` before a flag, `!` after,
    /// `?` to show, `=`, `+=`, `-=` for numbers and lists.
    pub(super) fn set(&mut self, args: &str, out: &mut Outcome) {
        for item in args.split_whitespace() {
            if let Err(e) = self.set_one(item, out) {
                out.message = Some((e, true));
                return;
            }
        }
    }

    fn set_one(&mut self, item: &str, out: &mut Outcome) -> Result<(), String> {
        let o = &mut self.options;
        let (name, op, value) = match item.find(['=', ':']) {
            Some(i) => {
                let (n, v) = item.split_at(i);
                let (n, op) = match n.chars().last() {
                    Some(c @ ('+' | '-' | '^')) => (&n[..n.len() - 1], Some(c)),
                    _ => (n, None),
                };
                (n, op.or(Some('=')), Some(&v[1..]))
            }
            None => (item, None, None),
        };
        let show = name.ends_with('?');
        let name = name.trim_end_matches(['?', '!', '&']);
        let toggle = item.ends_with('!') || name.starts_with("inv");
        let (flag_name, on) = if let Some(n) = name.strip_prefix("no") {
            (n, false)
        } else if let Some(n) = name.strip_prefix("inv") {
            (n, true)
        } else {
            (name, true)
        };
        fn flag<'o>(o: &'o mut super::Options, f: &str) -> Option<&'o mut bool> {
            Some(match f {
                "et" | "expandtab" => &mut o.expandtab,
                "ai" | "autoindent" => &mut o.autoindent,
                "ic" | "ignorecase" => &mut o.ignorecase,
                "scs" | "smartcase" => &mut o.smartcase,
                "ws" | "wrapscan" => &mut o.wrapscan,
                "js" | "joinspaces" => &mut o.joinspaces,
                "sol" | "startofline" => &mut o.startofline,
                _ => return None,
            })
        }
        if let Some(b) = flag(o, flag_name) {
            if show {
                let v = *b;
                out.message = Some((format!("{}{flag_name}", if v { "  " } else { "no" }), false));
            } else if toggle {
                *b = !*b;
            } else {
                *b = on;
            }
            return Ok(());
        }
        fn number<'o>(o: &'o mut super::Options, n: &str) -> Option<NumOpt<'o>> {
            Some(match n {
                "sw" | "shiftwidth" => NumOpt::U(&mut o.shiftwidth),
                "ts" | "tabstop" => NumOpt::U(&mut o.tabstop),
                "tw" | "textwidth" => NumOpt::U(&mut o.textwidth),
                "sts" | "softtabstop" => NumOpt::I(&mut o.softtabstop),
                _ => return None,
            })
        }
        if let Some(opt) = number(o, name) {
            match (opt, value) {
                (NumOpt::U(v), Some(val)) => {
                    let x: usize = val
                        .parse()
                        .map_err(|_| format!("E521: Number required after =: {item}"))?;
                    *v = match op {
                        Some('+') => *v + x,
                        Some('-') => v.saturating_sub(x),
                        Some('^') => *v * x,
                        _ => x,
                    };
                }
                (NumOpt::I(v), Some(val)) => {
                    let x: isize = val
                        .parse()
                        .map_err(|_| format!("E521: Number required after =: {item}"))?;
                    *v = match op {
                        Some('+') => *v + x,
                        Some('-') => *v - x,
                        Some('^') => *v * x,
                        _ => x,
                    };
                }
                (NumOpt::U(v), None) => out.message = Some((format!("  {name}={v}"), false)),
                (NumOpt::I(v), None) => out.message = Some((format!("  {name}={v}"), false)),
            }
            return Ok(());
        }
        if matches!(name, "nf" | "nrformats") {
            match value {
                Some(val) => {
                    let items: Vec<String> = val
                        .split(',')
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect();
                    match op {
                        Some('+') => {
                            for i in items {
                                if !o.nrformats.contains(&i) {
                                    o.nrformats.push(i);
                                }
                            }
                        }
                        Some('-') => o.nrformats.retain(|x| !items.contains(x)),
                        _ => o.nrformats = items,
                    }
                }
                None => {
                    out.message = Some((format!("  nrformats={}", o.nrformats.join(",")), false))
                }
            }
            return Ok(());
        }
        // Options of the editor (numbers, wrapping) are its settings;
        // others Vim has that Kalem does not keep are accepted quietly.
        if matches!(
            name,
            "nu" | "number"
                | "rnu"
                | "relativenumber"
                | "wrap"
                | "list"
                | "hls"
                | "hlsearch"
                | "is"
                | "incsearch"
                | "bs"
                | "backspace"
                | "cindent"
                | "cin"
                | "si"
                | "smartindent"
                | "ru"
                | "ruler"
                | "sc"
                | "showcmd"
                | "smd"
                | "showmode"
                | "so"
                | "scrolloff"
                | "siso"
                | "sidescrolloff"
                | "magic"
                | "ff"
                | "fileformat"
                | "enc"
                | "encoding"
                | "fenc"
                | "fileencoding"
                | "spell"
                | "cul"
                | "cursorline"
        ) || flag_name.is_empty()
        {
            return Ok(());
        }
        Err(format!("E518: Unknown option: {item}"))
    }
}

enum NumOpt<'a> {
    U(&'a mut usize),
    I(&'a mut isize),
}

/// A register name and a count after a command (`:d a 3`).
fn reg_count(args: &str) -> (Option<char>, Option<usize>) {
    let t = args.trim();
    let mut chars = t.chars();
    match chars.next() {
        Some(c) if !c.is_ascii_digit() => {
            let n = chars.as_str().trim().parse().ok();
            (Some(c), n)
        }
        Some(_) => (None, t.parse().ok()),
        None => (None, None),
    }
}

/// Lines `a..=b`, each with a line feed.
fn lines_text(doc: &DocumentState, a: usize, b: usize) -> String {
    (a..=b.min(last_line(doc)))
        .map(|l| {
            format!(
                "{}\n",
                &doc.text().as_str()[line_start(doc, l)..line_end(doc, l)]
            )
        })
        .collect()
}
