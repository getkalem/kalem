//! Everyday line commands (T2.7a.6): duplicate, move, join and sort the
//! lines of the selection, trim trailing whitespace, select the word at
//! the cursor, and grow or shrink the selection by syntax.

use org_edit::{Selection, Transaction};

/// The byte range of the lines a selection covers, line feed of the last
/// excluded: a selection ending at the start of a line leaves it out.
pub fn covered(text: &str, sel: Selection) -> std::ops::Range<usize> {
    let (a, b) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
    let start = text[..a].rfind('\n').map_or(0, |i| i + 1);
    let b = if b > a && b > start && text[..b].ends_with('\n') {
        b - 1
    } else {
        b
    };
    let end = text[b..].find('\n').map_or(text.len(), |i| b + i);
    start..end
}

/// Duplicates the lines of the selection below them; the selection moves
/// to the copy.
pub fn duplicate(text: &str, sel: Selection) -> Transaction {
    let r = covered(text, sel);
    let block = &text[r.clone()];
    let mut tx = Transaction::new("Duplicate Lines");
    tx.edit(r.end..r.end, format!("\n{block}"));
    let shift = block.len() + 1;
    tx.select(Selection {
        anchor: sel.anchor + shift,
        head: sel.head + shift,
    })
}

/// Moves the lines of the selection one line up (or down), the selection
/// with them; `None` at the start (or end) of the text.
pub fn move_lines(text: &str, sel: Selection, up: bool) -> Option<Transaction> {
    let r = covered(text, sel);
    let block = &text[r.clone()];
    let mut tx = Transaction::new(if up {
        "Move Lines Up"
    } else {
        "Move Lines Down"
    });
    if up {
        if r.start == 0 {
            return None;
        }
        let prev = text[..r.start - 1].rfind('\n').map_or(0, |i| i + 1);
        let above = &text[prev..r.start - 1];
        tx.edit(prev..r.end, format!("{block}\n{above}"));
        let shift = r.start - prev;
        Some(tx.select(Selection {
            anchor: sel.anchor - shift,
            head: sel.head - shift,
        }))
    } else {
        if r.end >= text.len() {
            return None;
        }
        let next_end = text[r.end + 1..]
            .find('\n')
            .map_or(text.len(), |i| r.end + 1 + i);
        let below = &text[r.end + 1..next_end];
        // The last line of a text without a final line feed.
        if below.is_empty() && next_end == text.len() {
            return None;
        }
        tx.edit(r.start..next_end, format!("{below}\n{block}"));
        let shift = below.len() + 1;
        Some(tx.select(Selection {
            anchor: sel.anchor + shift,
            head: sel.head + shift,
        }))
    }
}

/// Joins the lines of the selection into one (the line at the cursor with
/// the next when the selection is on one line), each line's indentation
/// and the blanks at the joins becoming one space.
pub fn join(text: &str, sel: Selection) -> Option<Transaction> {
    let mut r = covered(text, sel);
    if !text[r.clone()].contains('\n') {
        if r.end >= text.len() {
            return None;
        }
        r.end = text[r.end + 1..]
            .find('\n')
            .map_or(text.len(), |i| r.end + 1 + i);
    }
    let mut tx = Transaction::new("Join Lines");
    let mut cursor = sel.head;
    let mut at = r.start;
    while let Some(i) = text[at..r.end].find('\n') {
        let nl = at + i;
        let before = text[..nl].trim_end_matches([' ', '\t']).len().max(r.start);
        let after = nl
            + 1
            + text[nl + 1..r.end]
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .count();
        let sep = if after >= r.end || before == r.start {
            ""
        } else {
            " "
        };
        tx.edit(before..after, sep);
        cursor = before;
        at = after.max(nl + 1);
    }
    let cursor = tx.map(cursor, org_edit::Assoc::After);
    Some(tx.select(Selection::caret(cursor)))
}

/// Sorts the lines of the selection (by their text; `reverse` for
/// descending), keeping them selected.
pub fn sort(text: &str, sel: Selection, reverse: bool) -> Option<Transaction> {
    let r = covered(text, sel);
    let mut lines: Vec<&str> = text[r.clone()].split('\n').collect();
    if lines.len() < 2 {
        return None;
    }
    lines.sort();
    if reverse {
        lines.reverse();
    }
    let sorted = lines.join("\n");
    let mut tx = Transaction::new("Sort Lines");
    tx.edit(r.clone(), sorted);
    Some(tx.select(Selection {
        anchor: r.start,
        head: r.end,
    }))
}

/// Removes the blanks at the ends of lines; `None` when there are none.
/// The blank lines at the end of `text` removed, one line break kept
/// (Doom's `SPC c W`).
pub fn trim_trailing_blank_lines(text: &str) -> Option<Transaction> {
    let kept = text.trim_end_matches(['\n', '\r', ' ', '\t']).len();
    let end = if kept == 0 { 0 } else { kept + 1 };
    if end >= text.len() {
        return None;
    }
    let mut tx = Transaction::new("Delete Trailing Blank Lines");
    let at = kept.min(text.len());
    tx.edit(at..text.len(), if kept == 0 { "" } else { "\n" });
    Some(tx)
}

/// The difference of `old` and `new` as a unified diff (`---`, `+++`,
/// hunks with three lines of context), by the longest common subsequence
/// of their lines; empty when they are the same. `name` heads it.
pub fn unified_diff(old: &str, new: &str, name: &str) -> String {
    if old == new {
        return String::new();
    }
    let a: Vec<&str> = old.split_inclusive('\n').collect();
    let b: Vec<&str> = new.split_inclusive('\n').collect();
    // The common head and tail, then the table on what is left.
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (ma, mb) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);
    // Ops over the middle: ' ' kept, '-' removed, '+' added.
    let mut ops: Vec<(char, &str)> = Vec::new();
    if ma.len().saturating_mul(mb.len()) <= 4_000_000 {
        let (n, m) = (ma.len(), mb.len());
        let mut t = vec![0u32; (n + 1) * (m + 1)];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                t[i * (m + 1) + j] = if ma[i] == mb[j] {
                    t[(i + 1) * (m + 1) + j + 1] + 1
                } else {
                    t[(i + 1) * (m + 1) + j].max(t[i * (m + 1) + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && ma[i] == mb[j] {
                ops.push((' ', ma[i]));
                i += 1;
                j += 1;
            } else if i < n && (j == m || t[(i + 1) * (m + 1) + j] >= t[i * (m + 1) + j + 1]) {
                ops.push(('-', ma[i]));
                i += 1;
            } else {
                ops.push(('+', mb[j]));
                j += 1;
            }
        }
    } else {
        ops.extend(ma.iter().map(|l| ('-', *l)));
        ops.extend(mb.iter().map(|l| ('+', *l)));
    }
    let all: Vec<(char, &str)> = a[..head]
        .iter()
        .map(|l| (' ', *l))
        .chain(ops)
        .chain(a[a.len() - tail..].iter().map(|l| (' ', *l)))
        .collect();
    let mut out = format!("--- {name}\n+++ {name}\n");
    let changed: Vec<usize> = (0..all.len()).filter(|&k| all[k].0 != ' ').collect();
    let mut k = 0;
    while k < changed.len() {
        let start = changed[k].saturating_sub(3);
        let mut end = changed[k];
        while k < changed.len() && changed[k] <= end + 6 {
            end = changed[k];
            k += 1;
        }
        let end = (end + 4).min(all.len());
        // Line numbers of the hunk in each file.
        let before = |upto: usize, side: char| {
            all[..upto]
                .iter()
                .filter(|(c, _)| *c == ' ' || *c == side)
                .count()
        };
        let (oa, ob) = (before(start, '-'), before(start, '+'));
        let la = all[start..end].iter().filter(|(c, _)| *c != '+').count();
        let lb = all[start..end].iter().filter(|(c, _)| *c != '-').count();
        out.push_str(&format!("@@ -{},{la} +{},{lb} @@\n", oa + 1, ob + 1));
        for (c, l) in &all[start..end] {
            out.push(*c);
            out.push_str(l);
            if !l.ends_with('\n') {
                out.push_str("\n\\ No newline at end of file\n");
            }
        }
    }
    out
}

/// `text` changed into `new` by one replacement of the part that differs,
/// so the cursor and folds outside it stay (a formatter's result).
pub fn replace_differing(text: &str, new: &str, label: &str) -> Option<Transaction> {
    if text == new {
        return None;
    }
    let mut start = text
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !text.is_char_boundary(start) || !new.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = text[start..]
        .bytes()
        .rev()
        .zip(new[start..].bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    while !text.is_char_boundary(text.len() - end) || !new.is_char_boundary(new.len() - end) {
        end -= 1;
    }
    let mut tx = Transaction::new(label);
    tx.edit(start..text.len() - end, &new[start..new.len() - end]);
    Some(tx)
}

pub fn trim_trailing(text: &str) -> Option<Transaction> {
    let mut tx = Transaction::new("Trim Trailing Whitespace");
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let kept = body.trim_end_matches([' ', '\t']).len();
        if kept < body.len() {
            tx.edit(at + kept..at + body.len(), "");
        }
        at += line.len();
    }
    (!tx.is_empty()).then_some(tx)
}

/// The word at `pos` (letters, digits and `_`).
pub fn word_at(text: &str, pos: usize) -> Option<std::ops::Range<usize>> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let start = text[..pos]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map_or(pos, |(i, _)| i);
    let end = text[pos..]
        .char_indices()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map_or(pos, |(i, c)| pos + i + c.len_utf8());
    (start < end).then_some(start..end)
}

/// The ranges around `a..b` that a bracket pair makes: the inside, then
/// with the brackets, from the innermost out.
fn bracket_ranges(text: &str, a: usize, b: usize) -> Vec<std::ops::Range<usize>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let (mut lo, mut hi) = (a, b);
    for _ in 0..16 {
        // The nearest unmatched opening bracket before `lo`.
        let mut depth = 0i32;
        let mut open = None;
        let floor = lo.saturating_sub(64 * 1024);
        for i in (floor..lo).rev() {
            match bytes[i] {
                b')' | b']' | b'}' => depth += 1,
                b'(' | b'[' | b'{' if depth == 0 => {
                    open = Some(i);
                    break;
                }
                b'(' | b'[' | b'{' => depth -= 1,
                _ => {}
            }
        }
        let Some(o) = open else { break };
        let Some((_, c)) = crate::code::matching(text, o + 1).filter(|(x, _)| *x == o) else {
            break;
        };
        if c < hi {
            break;
        }
        out.push(o + 1..c);
        out.push(o..c + 1);
        lo = o;
        hi = c + 1;
    }
    out
}

/// The next larger selection around `sel`: the word, the inside of the
/// brackets around it and the brackets, the element or object of an Org
/// document (`root`) or the line and the paragraph of plain text, and the
/// whole text.
pub fn expand(
    text: &str,
    root: Option<&org_syntax::SyntaxNode>,
    sel: Selection,
) -> Option<Selection> {
    expand_with(text, root, Vec::new(), sel)
}

/// [`expand`] with more ranges to grow to: a mode's own structure (a
/// LaTeX group, command, environment and section).
pub fn expand_with(
    text: &str,
    root: Option<&org_syntax::SyntaxNode>,
    extra: Vec<std::ops::Range<usize>>,
    sel: Selection,
) -> Option<Selection> {
    let (a, b) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
    let mut candidates: Vec<std::ops::Range<usize>> = extra;
    if let Some(w) = word_at(text, a) {
        candidates.push(w);
    }
    candidates.extend(bracket_ranges(text, a, b));
    let trim = |r: std::ops::Range<usize>| {
        let end = r.start
            + text[r.clone()]
                .trim_end_matches([' ', '\t', '\n', '\r'])
                .len();
        r.start..end.max(r.start)
    };
    match root {
        Some(root) if !text.is_empty() => {
            let at = org_syntax::TextSize::try_from(a.min(text.len() - 1)).ok()?;
            if let Some(tok) = root.token_at_offset(at).right_biased() {
                for n in tok.parent_ancestors() {
                    let r = n.text_range();
                    candidates.push(trim(usize::from(r.start())..usize::from(r.end())));
                }
            }
        }
        _ => {
            // The line, then the paragraph of lines without blank ones.
            let ls = text[..a].rfind('\n').map_or(0, |i| i + 1);
            let le = text[b..].find('\n').map_or(text.len(), |i| b + i);
            let first = text[ls..le].len() - text[ls..le].trim_start().len();
            candidates.push(ls + first..le);
            candidates.push(ls..le);
            let mut ps = ls;
            while ps > 0 {
                let prev = text[..ps - 1].rfind('\n').map_or(0, |i| i + 1);
                if text[prev..ps - 1].trim().is_empty() {
                    break;
                }
                ps = prev;
            }
            let mut pe = le;
            while pe < text.len() {
                let next = text[pe + 1..].find('\n').map_or(text.len(), |i| pe + 1 + i);
                if text[pe + 1..next].trim().is_empty() {
                    break;
                }
                pe = next;
            }
            candidates.push(ps..pe);
        }
    }
    candidates.push(0..text.len());
    candidates
        .into_iter()
        .filter(|r| r.start <= a && b <= r.end && r.len() > b - a)
        .min_by_key(|r| r.len())
        .map(|r| Selection {
            anchor: r.start,
            head: r.end,
        })
}

/// The LaTeX structure around byte `at` of a LaTeX document, to grow a
/// selection to: the groups, commands and environments holding it, then
/// its sections (to the next one of the same level or above).
fn latex_ranges(doc: &crate::DocumentState, at: usize) -> Vec<std::ops::Range<usize>> {
    let Some(state) = doc.latex() else {
        return Vec::new();
    };
    let text = doc.text().as_str();
    if text.is_empty() {
        return Vec::new();
    }
    let trim = |r: std::ops::Range<usize>| {
        let end = r.start
            + text[r.clone()]
                .trim_end_matches([' ', '\t', '\n', '\r'])
                .len();
        r.start..end.max(r.start)
    };
    let mut out = Vec::new();
    let root = state.parse().syntax();
    if let Ok(off) = latex_syntax::TextSize::try_from(at.min(text.len() - 1))
        && let Some(tok) = root.token_at_offset(off).right_biased()
    {
        for n in tok.parent_ancestors() {
            let r = n.text_range();
            out.push(trim(usize::from(r.start())..usize::from(r.end())));
        }
    }
    let model = state.model();
    let own: Vec<_> = model.sections.iter().filter(|s| s.file == 0).collect();
    for (i, s) in own.iter().enumerate() {
        if s.range.start > at {
            break;
        }
        let end = own[i + 1..]
            .iter()
            .find(|n| n.level <= s.level)
            .map_or_else(
                || text.find("\\end{document}").unwrap_or(text.len()),
                |n| n.range.start,
            );
        if at < end {
            out.push(trim(s.range.start..end));
        }
    }
    out
}

impl crate::DocumentState {
    /// Expand Selection: the next larger selection by syntax.
    pub fn expand_selection(&mut self) -> bool {
        let root = match (&self.meta.mode, self.parse()) {
            (crate::DocumentMode::Org, Some((p, _))) => Some(p.syntax()),
            _ => None,
        };
        let before = self.selection;
        let extra = latex_ranges(self, before.anchor.min(before.head));
        let Some(after) = expand_with(self.text().as_str(), root.as_ref(), extra, before) else {
            return false;
        };
        if self.expansions.last().is_some_and(|(_, a)| *a != before) {
            self.expansions.clear();
        }
        self.expansions.push((before, after));
        self.selection = after;
        true
    }

    /// Shrink Selection: back to the selection before the last Expand
    /// Selection.
    pub fn shrink_selection(&mut self) -> bool {
        match self.expansions.pop() {
            Some((before, after)) if after == self.selection => {
                self.selection = before;
                true
            }
            _ => {
                self.expansions.clear();
                false
            }
        }
    }

    /// What saving does first, as the settings say: the blanks at the ends
    /// of lines removed (`editor.trim_trailing_whitespace`), except in CSV
    /// (trailing tabs are empty fields, blanks part of values) and
    /// Markdown (two trailing spaces are a hard line break).
    pub fn before_save(&mut self, config: &crate::settings::Config, now: std::time::Instant) {
        if config.bool("editor.trim_trailing_whitespace")
            && self.dired.is_none()
            && !matches!(
                self.meta.mode,
                crate::DocumentMode::Csv | crate::DocumentMode::Markdown
            )
            && let Some(tx) = trim_trailing(self.text().as_str())
        {
            self.apply(&tx, org_edit::ChangeKind::Command, now);
        }
        // The Kalem format is saved in its canonical form, when it is
        // well-formed (RFC 0003 §15); an ill-formed one is saved as it is.
        if crate::klm::is_klm_file(self) && self.dired.is_none() {
            let doc = klm_syntax::parse(self.text().as_str());
            if klm_syntax::well_formed(&doc) {
                let new = klm_syntax::fmt(&doc);
                if let Some(tx) = replace_differing(self.text().as_str(), &new, "Format Document") {
                    self.apply(&tx, org_edit::ChangeKind::Command, now);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, tx: Transaction) -> (String, Selection) {
        let s = tx.selection_after.unwrap_or(Selection::caret(0));
        (tx.apply(text), s)
    }

    #[test]
    fn line_commands() {
        let t = "a\nb\nc\n";
        let (d, s) = apply(t, duplicate(t, Selection::caret(2)));
        assert_eq!((d.as_str(), s.head), ("a\nb\nb\nc\n", 4));
        let (m, s) = apply(t, move_lines(t, Selection::caret(2), true).unwrap());
        assert_eq!((m.as_str(), s.head), ("b\na\nc\n", 0));
        let (m, s) = apply(t, move_lines(t, Selection::caret(2), false).unwrap());
        assert_eq!((m.as_str(), s.head), ("a\nc\nb\n", 4));
        assert!(move_lines(t, Selection::caret(0), true).is_none());
        assert!(move_lines("a\nb", Selection::caret(2), false).is_none());
        // Two selected lines move together.
        let (m, _) = apply(
            t,
            move_lines(t, Selection { anchor: 0, head: 3 }, false).unwrap(),
        );
        assert_eq!(m, "c\na\nb\n");
        let t = "one  \n   two\nthree\n";
        let (j, s) = apply(t, join(t, Selection::caret(1)).unwrap());
        assert_eq!((j.as_str(), s.head), ("one two\nthree\n", 4));
        let (j, _) = apply(
            t,
            join(
                t,
                Selection {
                    anchor: 0,
                    head: 17,
                },
            )
            .unwrap(),
        );
        assert_eq!(j, "one two three\n");
        let t = "pear\napple\nfig\n";
        let (o, _) = apply(
            t,
            sort(
                t,
                Selection {
                    anchor: 0,
                    head: 14,
                },
                false,
            )
            .unwrap(),
        );
        assert_eq!(o, "apple\nfig\npear\n");
        let (o, _) = apply(
            t,
            sort(
                t,
                Selection {
                    anchor: 0,
                    head: 14,
                },
                true,
            )
            .unwrap(),
        );
        assert_eq!(o, "pear\nfig\napple\n");
        let (w, _) = apply("a  \nb\t\r\nc", trim_trailing("a  \nb\t\r\nc").unwrap());
        assert_eq!(w, "a\nb\r\nc");
        assert!(trim_trailing("a\nb\n").is_none());
        assert_eq!(word_at("foo_bar baz", 3), Some(0..7));
        assert_eq!(word_at("a  b", 2), None);
    }

    #[test]
    fn expanding_latex() {
        // By LaTeX's structure: the group, the command, the environment,
        // then the section to the next one.
        let text = "\\documentclass{article}\n\\begin{document}\n\\section{One}\n\\begin{itemize}\n\\item an \\emph{odd word} here\n\\end{itemize}\n\\subsection{Sub}\nmore\n\\section{Two}\nend\n\\end{document}\n";
        let dir = std::env::temp_dir().join(format!("kalem-expand-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("e.tex");
        std::fs::write(&file, text).unwrap();
        let mut d = crate::DocumentState::open(
            &file,
            std::sync::Arc::new(org_model::Settings::default()),
            &Default::default(),
        )
        .unwrap();
        d.selection = Selection::caret(text.find("word").unwrap());
        let mut seen = Vec::new();
        while d.expand_selection() {
            let s = d.selection;
            seen.push(text[s.anchor..s.head].to_string());
        }
        let has = |x: &str| seen.iter().any(|s| s == x);
        assert!(has("\\emph{odd word}"), "{seen:#?}");
        assert!(
            has("\\begin{itemize}\n\\item an \\emph{odd word} here\n\\end{itemize}"),
            "{seen:#?}"
        );
        let section =
            &text[text.find("\\section{One}").unwrap()..text.find("\\section{Two}").unwrap()];
        assert!(has(section.trim_end()), "{seen:#?}");
        // The section comes after the environment, before the whole text.
        let at = |x: &str| seen.iter().position(|s| s == x).unwrap();
        assert!(at("\\emph{odd word}") < at(section.trim_end()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn expanding() {
        let t = "fn a() {\n    call(x, y);\n}\n\nnext\n";
        let x = t.find("x,").unwrap();
        let step = |s: Selection| expand(t, None, s).unwrap();
        let s = step(Selection::caret(x));
        assert_eq!(&t[s.anchor..s.head], "x");
        let s = step(s);
        assert_eq!(&t[s.anchor..s.head], "x, y");
        let s = step(s);
        assert_eq!(&t[s.anchor..s.head], "(x, y)");
        let s = step(s);
        assert_eq!(&t[s.anchor..s.head], "call(x, y);");
        let s = step(s);
        assert_eq!(&t[s.anchor..s.head], "    call(x, y);");
        let s = step(s);
        assert_eq!(&t[s.anchor..s.head], "\n    call(x, y);\n");
        let s = step(s);
        assert_eq!(&t[s.anchor..s.head], "{\n    call(x, y);\n}");
        // Org: by the syntax tree.
        let o = "* Head\nSome *bold word* here.\n";
        let root = org_syntax::parse(o).syntax();
        let w = o.find("word").unwrap();
        let s = expand(o, Some(&root), Selection::caret(w)).unwrap();
        assert_eq!(&o[s.anchor..s.head], "word");
        let s = expand(o, Some(&root), s).unwrap();
        assert_eq!(&o[s.anchor..s.head], "*bold word*");
        let s = expand(o, Some(&root), s).unwrap();
        assert_eq!(&o[s.anchor..s.head], "Some *bold word* here.");
    }

    #[test]
    fn trailing_blank_lines_and_differences() {
        let tx = trim_trailing_blank_lines("a\n\n\n  \n").unwrap();
        assert_eq!(tx.apply("a\n\n\n  \n"), "a\n");
        assert!(trim_trailing_blank_lines("a\n").is_none());
        let tx = replace_differing("| a |b|\nx\n", "| a | b |\nx\n", "Format").unwrap();
        assert_eq!(tx.apply("| a |b|\nx\n"), "| a | b |\nx\n");
        assert!(replace_differing("same", "same", "Format").is_none());
    }

    #[test]
    fn unified_diffs() {
        assert_eq!(unified_diff("a\n", "a\n", "f"), "");
        let d = unified_diff("a\nb\nc\n", "a\nB\nc\nd\n", "f.klm");
        assert_eq!(
            d,
            "--- f.klm\n+++ f.klm\n@@ -1,3 +1,4 @@\n a\n-b\n+B\n c\n+d\n"
        );
    }
}
