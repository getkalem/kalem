//! Structural editing of LaTeX documents (T2.7h.15): Enter in lists,
//! `\begin{…}` completed with its `\end{…}`, the two ends of an
//! environment renamed together, formatting commands toggled, sectioning
//! commands set, promoted, demoted and moved with their subtrees, list
//! items nested and unnested. Each function gives the transaction, or
//! `None` when it does not apply; generated text follows the document's
//! own indentation.

use std::ops::Range;

use latex_syntax::{SyntaxKind as K, SyntaxNode};
use org_edit::{Selection, Transaction};

fn span(n: &SyntaxNode) -> Range<usize> {
    usize::from(n.text_range().start())..usize::from(n.text_range().end())
}

fn line_range(text: &str, pos: usize) -> Range<usize> {
    let start = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
    start..end
}

fn indent_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

fn is_list(name: &str) -> bool {
    matches!(name, "itemize" | "enumerate" | "description")
}

/// The command each entry of a list environment begins with: `\item`,
/// `\bibitem` in a bibliography.
fn item_command(name: &str) -> Option<&'static str> {
    if is_list(name) {
        Some("\\item")
    } else if name == "thebibliography" {
        Some("\\bibitem")
    } else {
        None
    }
}

/// The innermost list environment around `pos`, and its name.
fn list_at(root: &SyntaxNode, pos: usize) -> Option<(SyntaxNode, String)> {
    let t = latex_syntax::token_before(root, pos)?;
    t.parent_ancestors().find_map(|a| {
        let name = (a.kind() == K::ENVIRONMENT)
            .then(|| latex_syntax::name(&a))
            .flatten()?;
        is_list(&name).then_some((a, name))
    })
}

/// Enter on a list item: a new `\item` with the item's indentation (the
/// rest of the line going to it); on an empty item, the item goes and the
/// cursor moves past the end of the list.
pub fn enter(text: &str, sel: Selection, root: &SyntaxNode) -> Option<Transaction> {
    if sel.anchor != sel.head {
        return None;
    }
    let pos = sel.head;
    let lr = line_range(text, pos);
    let line = &text[lr.clone()];
    let trimmed = line.trim_start();
    if !trimmed.starts_with("\\item") {
        return first_item(text, pos, root).or_else(|| new_row(text, pos, root));
    }
    let (env, _) = list_at(root, pos)?;
    // Only an item of this list, not a word starting with `\item`.
    if trimmed["\\item".len()..].starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let indent = indent_of(line).to_string();
    let after_item = trimmed["\\item".len()..].trim_start();
    let label_end = if after_item.starts_with('[') {
        after_item.find(']').map_or(0, |i| i + 1)
    } else {
        0
    };
    let empty = after_item[label_end..].trim().is_empty();
    if empty && pos >= lr.start + indent.len() + "\\item".len() {
        // Out of the list: the empty item goes, the cursor after `\end`.
        let end = span(&env).end;
        let end_line = line_range(text, end.saturating_sub(1).max(lr.end));
        let outer = indent_of(&text[end_line.clone()]).to_string();
        let remove = lr.start..(lr.end + 1).min(text.len());
        let mut tx = Transaction::new("New Paragraph");
        tx.replace(remove.clone(), "").ok()?;
        tx.replace(end_line.end..end_line.end, format!("\n{outer}"))
            .ok()?;
        let caret = end_line.end - remove.len() + 1 + outer.len();
        return Some(tx.select(Selection::caret(caret)));
    }
    let insert = format!("\n{indent}\\item ");
    let mut tx = Transaction::new("New Item");
    // The blanks before the cursor stay on this line.
    tx.replace(pos..pos, insert.clone()).ok()?;
    Some(tx.select(Selection::caret(pos + insert.len())))
}

/// Enter at the end of a list's `\begin` line: its first item begun (no
/// text can go before it).
fn first_item(text: &str, pos: usize, root: &SyntaxNode) -> Option<Transaction> {
    let lr = line_range(text, pos);
    let line = &text[lr.clone()];
    if text[pos..lr.end].trim() != "" {
        return None;
    }
    let t = latex_syntax::token_before(root, pos)?;
    let begin = t.parent_ancestors().find(|a| a.kind() == K::BEGIN)?;
    let env = begin.parent()?;
    let item = item_command(&latex_syntax::name(&env)?)?;
    if span(&begin).end > lr.end || span(&begin).end > pos {
        return None;
    }
    let style = Style::infer(text);
    let inner = format!("{}{}", indent_of(line), style.step);
    let (insert, back) = if item == "\\bibitem" {
        (format!("\n{inner}\\bibitem{{}} "), 2)
    } else {
        (format!("\n{inner}\\item "), 0)
    };
    let mut tx = Transaction::new("New Item");
    tx.replace(pos..pos, insert.clone()).ok()?;
    Some(tx.select(Selection::caret(pos + insert.len() - back)))
}

/// Enter at the end of a row of an environment of rows (`align`,
/// `gather`, a matrix, `tabular`): the row ended with `\\` and a new one
/// begun, as Enter on an item begins an item.
fn new_row(text: &str, pos: usize, root: &SyntaxNode) -> Option<Transaction> {
    let lr = line_range(text, pos);
    let line = &text[lr.clone()];
    let row = line.trim();
    if text[pos..lr.end].trim() != ""
        || row.is_empty()
        || row.starts_with("\\begin")
        || row.starts_with("\\end")
        || row.starts_with('%')
    {
        return None;
    }
    let t = latex_syntax::token_before(root, pos)?;
    let env = t.parent_ancestors().find(|a| a.kind() == K::ENVIRONMENT)?;
    let name = latex_syntax::name(&env)?;
    let n = name.trim_end_matches('*');
    let rows = matches!(
        n,
        "align"
            | "flalign"
            | "alignat"
            | "gather"
            | "multline"
            | "eqnarray"
            | "split"
            | "aligned"
            | "gathered"
            | "alignedat"
            | "array"
            | "tabular"
            | "tabularx"
            | "tabulary"
            | "longtable"
    ) || is_grid(n);
    // On the body's line, not the `\begin`'s or `\end`'s.
    let body = env.children().find(|c| c.kind() == K::BODY)?;
    if !rows || !span(&body).contains(&lr.start.max(span(&body).start)) || pos > span(&body).end {
        return None;
    }
    // A row already ended, or a rule between rows: a line break only.
    let ended = [
        "\\\\",
        "\\hline",
        "\\toprule",
        "\\midrule",
        "\\bottomrule",
        "\\cr",
    ]
    .iter()
    .any(|e| row.ends_with(e))
        || row.starts_with("\\cline")
        || row.starts_with("\\cmidrule");
    let indent = indent_of(line);
    let gap = if line[..pos - lr.start].ends_with([' ', '\t']) {
        ""
    } else {
        " "
    };
    let insert = if ended {
        format!("\n{indent}")
    } else {
        format!("{gap}\\\\\n{indent}")
    };
    let mut tx = Transaction::new("New Row");
    tx.replace(pos..pos, insert.clone()).ok()?;
    Some(tx.select(Selection::caret(pos + insert.len())))
}

/// After `}` typed at `pos` (the text already has it): a `\begin{name}`
/// that is left open gets its `\end{name}`, the cursor on a line between
/// them.
pub fn complete_begin(text: &str, pos: usize, parse: &latex_syntax::Parse) -> Option<Transaction> {
    let before = &text[..pos];
    let open = before.rfind("\\begin{")?;
    let name = &before[open + "\\begin{".len()..pos.checked_sub(1)?];
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '*') {
        return None;
    }
    let unclosed = format!("\\begin{{{name}}} is not closed");
    if !parse
        .diagnostics()
        .iter()
        .any(|d| d.range.start == open && d.message == unclosed)
    {
        return None;
    }
    let lr = line_range(text, open);
    let indent = indent_of(&text[lr.clone()]).to_string();
    let inner = format!("{indent}{}", if is_list(name) { "  \\item " } else { "  " });
    let insert = format!("\n{inner}\n{indent}\\end{{{name}}}");
    let mut tx = Transaction::new("Environment");
    tx.replace(pos..pos, insert).ok()?;
    Some(tx.select(Selection::caret(pos + 1 + inner.len())))
}

/// For an edit of `range` inside the name of a closed environment's
/// `\begin` or `\end`: the same place in the name at the other end.
pub fn mirror(root: &SyntaxNode, range: Range<usize>) -> Option<Range<usize>> {
    // At a boundary, the name on either side.
    let t = [
        latex_syntax::token_at(root, range.start),
        latex_syntax::token_before(root, range.start),
    ]
    .into_iter()
    .flatten()
    .find(|t| t.kind() == K::ENV_NAME)?;
    let name = usize::from(t.text_range().start())..usize::from(t.text_range().end());
    if range.start < name.start || range.end > name.end {
        return None;
    }
    let edge = t
        .parent_ancestors()
        .find(|a| matches!(a.kind(), K::BEGIN | K::END))?;
    let env = edge.parent().filter(|e| e.kind() == K::ENVIRONMENT)?;
    let other_kind = if edge.kind() == K::BEGIN {
        K::END
    } else {
        K::BEGIN
    };
    let other = env.children().find(|c| c.kind() == other_kind)?;
    let other_name = other
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == K::ENV_NAME)?;
    if other_name.text() != t.text() {
        return None;
    }
    let o = usize::from(other_name.text_range().start());
    Some(o + (range.start - name.start)..o + (range.end - name.start))
}

/// Toggles the formatting command `command` (`textbf`, `emph`, …): off
/// when the selection or the cursor is in its argument, else around the
/// selection or the word at the cursor.
pub fn toggle(text: &str, sel: Selection, root: &SyntaxNode, command: &str) -> Option<Transaction> {
    let (a, b) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
    // Commands that toggle as one: italics is `\emph` or `\textit`.
    let family: &[&str] = match command {
        "emph" | "textit" => &["emph", "textit"],
        "textbf" => &["textbf"],
        "texttt" => &["texttt"],
        "underline" => &["underline"],
        _ => &[],
    };
    let same = |n: &str| n == command || family.contains(&n);
    // Inside `\command{…}` (or one of its family): unwrapped.
    if let Some(t) = latex_syntax::token_at(root, a) {
        let cmd = t.parent_ancestors().find(|n| {
            n.kind() == K::COMMAND
                && latex_syntax::name(n).as_deref().is_some_and(same)
                && span(n).end >= b
        });
        if let Some(cmd) = cmd
            && let Some(g) = cmd.children().find(|c| c.kind() == K::GROUP)
        {
            let cs = span(&cmd);
            let gs = span(&g);
            let closed = text[..gs.end].ends_with('}') && gs.len() >= 2;
            let inner = gs.start + 1..if closed { gs.end - 1 } else { gs.end };
            let mut tx = Transaction::new("Formatting");
            tx.replace(cs.start..inner.start, "").ok()?;
            if closed {
                tx.replace(inner.end..cs.end, "").ok()?;
            }
            let shift = inner.start - cs.start;
            return Some(tx.select(Selection {
                anchor: sel.anchor.saturating_sub(shift).max(cs.start),
                head: sel.head.saturating_sub(shift).max(cs.start),
            }));
        }
    }
    let (a, b) = if a == b {
        let w = crate::lines::word_at(text, a)?;
        (w.start, w.end)
    } else {
        (a, b)
    };
    // A selection that starts or ends inside a command takes all of it,
    // so that the braces stay balanced: `a \emph{b| c} d|` wraps the
    // whole `\emph{b c}`.
    let (a, b) = balanced(root, a, b);
    // Only text: not a word of `\end{itemize}` or of a label.
    if !in_text(root, a) || !in_text(root, b) {
        return None;
    }
    // Within a paragraph, and no environment's edge, item or heading in
    // it: `\textbf{…}` around those does not compile.
    let crosses = root
        .descendants_with_tokens()
        .filter(|e| {
            let r = usize::from(e.text_range().start())..usize::from(e.text_range().end());
            a < r.end && r.start < b
        })
        .any(|e| match e.kind() {
            // (Nor a table's cell or row's end.)
            K::PAR_BREAK | K::BEGIN | K::END | K::AMPERSAND => true,
            K::CONTROL_SYMBOL => e.as_token().is_some_and(|t| t.text() == "\\\\"),
            K::COMMAND => e
                .as_node()
                .and_then(latex_syntax::name)
                .is_some_and(|x| block_command(&x)),
            _ => false,
        });
    if crosses {
        return None;
    }
    let open = format!("\\{command}{{");
    let mut tx = Transaction::new("Formatting");
    tx.replace(a..a, open.clone()).ok()?;
    tx.replace(b..b, "}").ok()?;
    Some(tx.select(Selection {
        anchor: a + open.len(),
        head: b + open.len(),
    }))
}

/// `a..b` widened until no command or group is cut by it.
fn balanced(root: &SyntaxNode, mut a: usize, mut b: usize) -> (usize, usize) {
    loop {
        let (a0, b0) = (a, b);
        for pos in [a, b.saturating_sub(1).max(a)] {
            let Some(t) = latex_syntax::token_at(root, pos) else {
                continue;
            };
            for n in t.parent_ancestors() {
                if !matches!(n.kind(), K::COMMAND | K::GROUP | K::INLINE_MATH) {
                    continue;
                }
                let r = span(&n);
                // Cut by the selection: inside it at one end only.
                let cut = (r.start < a && a < r.end && r.end < b)
                    || (a < r.start && r.start < b && b < r.end);
                if cut {
                    a = a.min(r.start);
                    b = b.max(r.end);
                }
            }
        }
        if (a, b) == (a0, b0) {
            return (a, b);
        }
    }
}

/// Where a float's `\\label` goes, as the document puts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelPlace {
    /// On a line of its own after the caption (the default).
    AfterCaption,
    /// Inside the caption's braces, at their end.
    InCaption,
    /// On a line of its own before the caption.
    BeforeCaption,
}

/// The document's own style for what Kalem writes into it (T2.7h.15), as
/// Org's style inference reads a document's: the indentation of an
/// environment's body, where a float's and an equation's `\\label` go,
/// and the prefix of each kind of label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style {
    /// The indentation of an environment's body, one step.
    pub step: String,
    /// A float's label.
    pub label: LabelPlace,
    /// An equation's label on the `\\begin{equation}` line.
    pub equation_label_inline: bool,
    /// The label prefixes: figures, tables, equations (with their
    /// separator, `fig:`).
    pub prefixes: [String; 3],
}

impl Style {
    /// The style of `text`; Kalem's defaults where it shows none.
    pub fn infer(text: &str) -> Style {
        let lines: Vec<&str> = text.lines().collect();
        let indent = |l: &str| l.len() - l.trim_start_matches([' ', '\t']).len();
        // The step: a body line's indentation beyond its `\\begin`'s.
        let mut steps: std::collections::HashMap<String, usize> = Default::default();
        for (i, l) in lines.iter().enumerate() {
            let t = l.trim_start();
            if !t.starts_with("\\begin{") || t.starts_with("\\begin{document}") {
                continue;
            }
            if let Some(next) = lines[i + 1..].iter().find(|n| !n.trim().is_empty()) {
                let (a, b) = (indent(l), indent(next));
                if b > a && !next.trim_start().starts_with("\\end{") {
                    *steps.entry(next[a..b].to_string()).or_default() += 1;
                } else if b == a && !next.trim_start().starts_with("\\end{") {
                    *steps.entry(String::new()).or_default() += 1;
                }
            }
        }
        let step = steps
            .into_iter()
            .max_by_key(|(s, n)| (*n, std::cmp::Reverse(s.len())))
            .map_or_else(|| "  ".to_string(), |(s, _)| s);
        // Labels of floats and equations.
        let (mut inside, mut before, mut after) = (0, 0, 0);
        let (mut eq_inline, mut eq_own) = (0, 0);
        let mut prefixes: [std::collections::HashMap<String, usize>; 3] = Default::default();
        let mut env: Vec<&str> = Vec::new();
        let mut caption_seen = false;
        let mut label_before = false;
        for l in &lines {
            let t = l.trim_start();
            if let Some(name) = t.strip_prefix("\\begin{").and_then(|r| r.split('}').next()) {
                env.push(name);
                caption_seen = false;
                label_before = false;
                if matches!(name, "equation" | "align" | "gather" | "multline") {
                    if l.contains("\\label{") {
                        eq_inline += 1;
                    } else {
                        eq_own += 1;
                    }
                }
            }
            let kind = match env.last().map(|e| e.trim_end_matches('*')) {
                Some("figure") => Some(0),
                Some("table") => Some(1),
                Some("equation" | "align" | "gather" | "multline") => Some(2),
                _ => None,
            };
            if let Some(k) = kind
                && let Some(i) = l.find("\\label{")
            {
                let label = &l[i + 7..];
                if let Some(c) = label
                    .find([':', '-', '_'])
                    .filter(|&c| c < label.find('}').unwrap_or(0))
                {
                    *prefixes[k].entry(label[..=c].to_string()).or_default() += 1;
                }
                if k < 2 {
                    if let Some(c) = l.find("\\caption") {
                        if c < i {
                            inside += 1;
                        }
                    } else if caption_seen {
                        after += 1;
                    } else {
                        label_before = true;
                    }
                }
            }
            if kind.is_some_and(|k| k < 2) && l.contains("\\caption") {
                caption_seen = true;
                if label_before {
                    before += 1;
                    label_before = false;
                }
            }
            if t.starts_with("\\end{") {
                env.pop();
            }
        }
        let label = if inside > after.max(before) {
            LabelPlace::InCaption
        } else if before > after {
            LabelPlace::BeforeCaption
        } else {
            LabelPlace::AfterCaption
        };
        let pick = |m: &std::collections::HashMap<String, usize>, default: &str| {
            m.iter()
                .max_by_key(|(p, n)| (**n, std::cmp::Reverse((*p).clone())))
                .map_or_else(|| default.to_string(), |(p, _)| p.clone())
        };
        Style {
            step,
            label,
            equation_label_inline: eq_inline > eq_own,
            prefixes: [
                pick(&prefixes[0], "fig:"),
                pick(&prefixes[1], "tab:"),
                pick(&prefixes[2], "eq:"),
            ],
        }
    }
}

/// The sectioning commands of a class, from level 1.
fn levels(class: Option<&str>) -> &'static [&'static str] {
    match class {
        Some(c) if latex_model::has_chapters(c) => &[
            "chapter",
            "section",
            "subsection",
            "subsubsection",
            "paragraph",
            "subparagraph",
        ],
        _ => &[
            "section",
            "subsection",
            "subsubsection",
            "paragraph",
            "subparagraph",
        ],
    }
}

/// The sectioning command on the line holding `pos`.
fn section_on_line(root: &SyntaxNode, text: &str, pos: usize) -> Option<SyntaxNode> {
    let lr = line_range(text, pos);
    root.descendants().find(|n| {
        n.kind() == K::COMMAND
            && lr.contains(&span(n).start)
            && latex_syntax::name(n).is_some_and(|x| latex_syntax::signatures::is_sectioning(&x))
    })
}

/// Heading level `level` (Ctrl+1 to Ctrl+6) for the line holding `pos`:
/// the class's command of that level; 0 turns a heading back into text.
pub fn set_level(
    text: &str,
    pos: usize,
    root: &SyntaxNode,
    class: Option<&str>,
    level: usize,
) -> Option<Transaction> {
    let names = levels(class);
    let lr = line_range(text, pos);
    let mut tx = Transaction::new("Heading Level");
    if let Some(cmd) = section_on_line(root, text, pos) {
        let name_tok = cmd.first_token()?;
        let nr =
            usize::from(name_tok.text_range().start())..usize::from(name_tok.text_range().end());
        if level == 0 {
            // The title stays as text.
            let g = cmd.children().find(|c| c.kind() == K::GROUP)?;
            let title = g.text().to_string();
            let title = title
                .trim_start_matches('{')
                .trim_end_matches('}')
                .to_string();
            let cs = span(&cmd);
            tx.replace(cs.clone(), title.clone()).ok()?;
            return Some(tx.select(Selection::caret(cs.start + title.len())));
        }
        let name = names.get(level - 1).or(names.last())?;
        tx.replace(nr, format!("\\{name}")).ok()?;
        return Some(tx);
    }
    if level == 0 {
        return None;
    }
    let name = names.get(level - 1).or(names.last())?;
    let line = &text[lr.clone()];
    let indent = indent_of(line);
    let body = line.trim();
    let start = lr.start + indent.len();
    // A line of text in the document's body: not in an environment (a
    // list's item, a float, math), not holding one's edge.
    let in_env = latex_syntax::token_at(root, start).is_some_and(|t| {
        t.parent_ancestors().any(|a| {
            a.kind() == K::ENVIRONMENT && latex_syntax::name(&a).is_some_and(|n| n != "document")
        })
    });
    let before_body = text
        .find("\\begin{document}")
        .is_some_and(|b| start < b + "\\begin{document}".len());
    let unbalanced = body.matches('{').count() != body.matches('}').count();
    if body.is_empty()
        || in_env
        || before_body
        || unbalanced
        || ["\\begin{", "\\end{", "\\item", "\\\\"]
            .iter()
            .any(|m| body.contains(m))
        || !in_text(root, start)
    {
        return None;
    }
    tx.replace(start..lr.end, format!("\\{name}{{{body}}}"))
        .ok()?;
    Some(tx.select(Selection::caret(start + name.len() + 2 + body.len())))
}

/// A sectioning command: its start, level and the range of its name.
type Section = (usize, i8, Range<usize>);

/// The sectioning commands in order.
fn sections(root: &SyntaxNode) -> Vec<Section> {
    root.descendants()
        .filter(|n| n.kind() == K::COMMAND)
        .filter_map(|n| {
            let name = latex_syntax::name(&n)?;
            let level = match name.as_str() {
                "part" => -1,
                "chapter" => 0,
                "section" => 1,
                "subsection" => 2,
                "subsubsection" => 3,
                "paragraph" => 4,
                "subparagraph" => 5,
                _ => return None,
            };
            let t = n.first_token()?;
            Some((
                span(&n).start,
                level,
                usize::from(t.text_range().start())..usize::from(t.text_range().end()),
            ))
        })
        .collect()
}

/// The section at the line of `pos` and where its subtree ends.
fn subtree(text: &str, root: &SyntaxNode, pos: usize) -> Option<(Vec<Section>, usize, usize)> {
    let all = sections(root);
    let lr = line_range(text, pos);
    let i = all.iter().position(|(s, _, _)| lr.contains(s))?;
    let level = all[i].1;
    let end_i = all[i + 1..]
        .iter()
        .position(|(_, l, _)| *l <= level)
        .map_or(all.len(), |k| i + 1 + k);
    Some((all, i, end_i))
}

fn level_name(level: i8) -> &'static str {
    match level {
        -1 => "part",
        0 => "chapter",
        1 => "section",
        2 => "subsection",
        3 => "subsubsection",
        4 => "paragraph",
        _ => "subparagraph",
    }
}

/// Promotes (`up`) or demotes the section at the line of `pos` with its
/// subtree: each sectioning command in it a level up or down.
pub fn promote(text: &str, pos: usize, root: &SyntaxNode, up: bool) -> Option<Transaction> {
    let (all, i, end) = subtree(text, root, pos)?;
    // Not above the top level the document uses (`\chapter` in a
    // book, `\section` in an article; `\part` above either).
    let top = if all.iter().any(|s| s.1 == 0) { -1 } else { 1 };
    if up && all[i].1 <= top {
        return None;
    }
    if !up && all[i..end].iter().any(|s| s.1 >= 5) {
        return None;
    }
    let mut tx = Transaction::new(if up {
        "Promote Section"
    } else {
        "Demote Section"
    });
    for (_, level, name) in &all[i..end] {
        let l = if up { level - 1 } else { level + 1 };
        tx.replace(name.clone(), format!("\\{}", level_name(l)))
            .ok()?;
    }
    Some(tx)
}

/// Moves the section at the line of `pos`, with its subtree, below the
/// next section of its level (`down`) or above the one before.
pub fn move_section(text: &str, pos: usize, root: &SyntaxNode, down: bool) -> Option<Transaction> {
    let (all, i, end) = subtree(text, root, pos)?;
    let level = all[i].1;
    let line_start = |p: usize| text[..p].rfind('\n').map_or(0, |k| k + 1);
    let start_of = |k: usize| all.get(k).map_or(text.len(), |s| line_start(s.0));
    let this = start_of(i)..start_of(end);
    let other = if down {
        let n = all.get(end).filter(|s| s.1 == level)?;
        let _ = n;
        let n_end = all[end + 1..]
            .iter()
            .position(|s| s.1 <= level)
            .map_or(all.len(), |k| end + 1 + k);
        start_of(end)..start_of(n_end)
    } else {
        let p = all[..i].iter().rposition(|s| s.1 <= level)?;
        if all[p].1 != level {
            return None;
        }
        start_of(p)..start_of(i)
    };
    // The document's end (`\end{document}`) stays where it is.
    let limit = text.rfind("\\end{document}").map_or(text.len(), line_start);
    let clip = |r: Range<usize>| r.start.min(limit)..r.end.min(limit);
    let (a, b) = if down {
        (clip(this), clip(other))
    } else {
        (clip(other), clip(this))
    };
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let (ta, tb) = (&text[a.clone()], &text[b.clone()]);
    let (ta, tb) = (with_newline(ta), with_newline(tb));
    let mut tx = Transaction::new("Move Section");
    tx.replace(a.start..b.end, format!("{tb}{ta}")).ok()?;
    let moved = if down { a.start + tb.len() } else { a.start };
    let offset = sel_offset(pos, if down { a.start } else { b.start });
    Some(tx.select(Selection::caret(moved + offset)))
}

fn with_newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

fn sel_offset(pos: usize, start: usize) -> usize {
    pos.saturating_sub(start)
}

/// Tab (`deeper`) on a list item: it goes into a list of the same kind
/// under the item before it (joining one that is there); Shift+Tab: out
/// of its list, after it, as an item of the list around.
pub fn indent_item(text: &str, pos: usize, root: &SyntaxNode, deeper: bool) -> Option<Transaction> {
    let lr = line_range(text, pos);
    let line = &text[lr.clone()];
    if !line.trim_start().starts_with("\\item") {
        return None;
    }
    let (env, name) = list_at(root, pos)?;
    let indent = indent_of(line).to_string();
    let item_line = format!("{}\n", line.trim_start());
    let full = lr.start..(lr.end + 1).min(text.len());
    let mut tx = Transaction::new(if deeper { "Nest Item" } else { "Unnest Item" });
    if deeper {
        // Not the first item: there must be one before it to go under.
        let body = env.children().find(|c| c.kind() == K::BODY)?;
        let first_item = text[span(&body)]
            .find("\\item")
            .map(|i| span(&body).start + i)?;
        if first_item >= lr.start {
            return None;
        }
        // Right after a nested list of the same kind: into it.
        let prev = text[..lr.start.saturating_sub(1)]
            .rfind('\n')
            .map_or(0, |i| i + 1);
        let prev_line = text[prev..lr.start].trim();
        let inner = format!("{indent}  ");
        if prev_line == format!("\\end{{{name}}}") {
            tx.replace(
                prev..full.end,
                format!("{inner}{item_line}{}", &text[prev..lr.start]),
            )
            .ok()?;
            let caret = prev + inner.len() + (pos - lr.start).saturating_sub(indent.len());
            return Some(tx.select(Selection::caret(caret)));
        }
        let new = format!("{indent}\\begin{{{name}}}\n{inner}{item_line}{indent}\\end{{{name}}}\n");
        tx.replace(full.clone(), new).ok()?;
        let caret = lr.start
            + indent.len()
            + name.len()
            + 8
            + inner.len()
            + (pos - lr.start).saturating_sub(indent.len());
        return Some(tx.select(Selection::caret(caret)));
    }
    // Out: the list must be inside another one.
    let outer = env.parent_ancestors_list();
    let (_, _) = outer?;
    let es = span(&env);
    let end_line = line_range(text, es.end.saturating_sub(1));
    let outer_indent = indent
        .get(2..)
        .map_or(String::new(), |_| indent[..indent.len() - 2].to_string());
    // Alone in its list: the list goes with it.
    let begin_line = line_range(text, es.start);
    let only = text[span(&env.children().find(|c| c.kind() == K::BODY)?)]
        .matches("\\item")
        .count()
        == 1;
    if only {
        let whole = begin_line.start..(end_line.end + 1).min(text.len());
        tx.replace(whole.clone(), format!("{outer_indent}{item_line}"))
            .ok()?;
        return Some(tx.select(Selection::caret(
            whole.start + outer_indent.len() + (pos - lr.start).saturating_sub(indent.len()),
        )));
    }
    tx.replace(full.clone(), "").ok()?;
    tx.replace(
        end_line.end + 1..end_line.end + 1,
        format!("{outer_indent}{item_line}"),
    )
    .ok()?;
    let caret = end_line.end + 1 - full.len()
        + outer_indent.len()
        + (pos - lr.start).saturating_sub(indent.len());
    Some(tx.select(Selection::caret(caret)))
}

/// Whether `pos` is in math (a formula or a math environment's body).
fn in_math(root: &SyntaxNode, pos: usize) -> bool {
    latex_syntax::token_before(root, pos).is_some_and(|t| {
        t.parent_ancestors().any(|a| {
            // Right after a formula's closing `$`: out of it.
            let closed = a.last_token().as_ref() == Some(&t)
                && a.first_token().as_ref() != Some(&t)
                && usize::from(t.text_range().end()) == pos
                && matches!(t.text(), "$" | "$$" | "\\)" | "\\]");
            (matches!(a.kind(), K::INLINE_MATH | K::DISPLAY_MATH) && !closed)
                || (a.kind() == K::BODY
                    && a.parent()
                        .and_then(|e| latex_syntax::name(&e))
                        .is_some_and(|n| latex_syntax::signatures::is_math(&n)))
        })
    })
}

/// Whether `pos` is in running text, where text can be added or
/// formatted: not in math, code or a comment, not inside a command's or
/// an environment's name, and not in an argument that is not text (a
/// label, a citation key, `\\begin`'s name, a package).
pub(crate) fn in_text(root: &SyntaxNode, pos: usize) -> bool {
    if in_math(root, pos) {
        return false;
    }
    let Some(t) =
        latex_syntax::token_at(root, pos).or_else(|| latex_syntax::token_before(root, pos))
    else {
        return true;
    };
    let ts = usize::from(t.text_range().start())..usize::from(t.text_range().end());
    // Inside a name: `\beg|in`, `\begin{tab|ular}`.
    if ts.start < pos
        && pos < ts.end
        && matches!(t.kind(), K::CONTROL_WORD | K::CONTROL_SYMBOL | K::ENV_NAME)
    {
        return false;
    }
    if t.kind() == K::COMMENT && ts.start < pos {
        return false;
    }
    // Before a rule between a table's rows: no row has begun.
    let text = root.text().to_string();
    let rest = text[pos.min(text.len())..].trim_start();
    if [
        "\\hline",
        "\\toprule",
        "\\midrule",
        "\\bottomrule",
        "\\cline",
        "\\cmidrule",
    ]
    .iter()
    .any(|r| rest.starts_with(r))
    {
        return false;
    }
    // In a list before its first item: nothing but items can go there.
    if let Some(env) = t.parent_ancestors().find(|a| a.kind() == K::ENVIRONMENT)
        && let Some(name) = latex_syntax::name(&env)
        && let Some(item) = item_command(&name)
        && let Some(body) = env.children().find(|c| c.kind() == K::BODY)
    {
        let b = span(&body);
        let first = root.text().slice(body.text_range()).to_string().find(item);
        if b.contains(&pos) && first.is_none_or(|i| pos <= b.start + i) {
            return false;
        }
    }
    !t.parent_ancestors().any(|a| match a.kind() {
        K::BEGIN | K::END | K::VERB => true,
        K::ENVIRONMENT => {
            latex_syntax::name(&a).is_some_and(|n| latex_syntax::signatures::is_verbatim(&n))
        }
        // In a command's arguments (not at its name's edge): text only
        // where the command typesets its argument as text.
        K::COMMAND => {
            let name_end = a
                .first_token()
                .map_or(0, |f| usize::from(f.text_range().end()));
            // Between the name and the arguments: `\section|{A}`.
            (pos == name_end && span(&a).end > name_end)
                || (pos > name_end
                    && latex_syntax::name(&a).is_some_and(|n| !crate::latex_view::prose(&n)))
        }
        _ => false,
    })
}

/// Commands that do not go in a formatting command's argument: an item,
/// a heading, a caption, what sets a paragraph or a page, a table's rule
/// or spanning cell.
fn block_command(name: &str) -> bool {
    latex_syntax::signatures::is_sectioning(name)
        || matches!(
            name,
            "item"
                | "bibitem"
                | "caption"
                | "centering"
                | "raggedright"
                | "raggedleft"
                | "par"
                | "maketitle"
                | "tableofcontents"
                | "listoffigures"
                | "listoftables"
                | "bibliography"
                | "bibliographystyle"
                | "printbibliography"
                | "appendix"
                | "newpage"
                | "clearpage"
                | "cleardoublepage"
                | "pagebreak"
                | "hline"
                | "toprule"
                | "midrule"
                | "bottomrule"
                | "cline"
                | "cmidrule"
                | "multicolumn"
        )
}

/// Whether an environment holds its body in a box, where no float can
/// go: floats, tables, minipages, pictures, math and verbatim.
fn boxed(name: &str) -> bool {
    let n = name.trim_end_matches('*');
    matches!(
        n,
        "figure"
            | "table"
            | "wrapfigure"
            | "wraptable"
            | "sidewaysfigure"
            | "sidewaystable"
            | "subfigure"
            | "subtable"
            | "minipage"
            | "tabular"
            | "tabularx"
            | "tabulary"
            | "longtable"
            | "array"
            | "tikzpicture"
            | "picture"
            | "algorithm"
            | "lstlisting"
            | "minted"
    ) || is_grid(n)
        || latex_syntax::signatures::is_math(name)
        || latex_syntax::signatures::is_verbatim(name)
}

/// Where a block (a figure, a table, an equation) asked for at `pos` can
/// go: after the outermost box or command around `pos` (a float in a
/// float, an equation in a caption do not compile), inside the document's
/// body.
pub fn block_position(text: &str, root: &SyntaxNode, pos: usize) -> usize {
    let mut pos = pos;
    if let Some(begin) = text.find("\\begin{document}") {
        pos = pos.max(begin + "\\begin{document}".len());
    }
    if let Some(end) = text.rfind("\\end{document}") {
        pos = pos.min(end.saturating_sub(1));
    }
    let Some(t) =
        latex_syntax::token_at(root, pos).or_else(|| latex_syntax::token_before(root, pos))
    else {
        return pos;
    };
    let mut out = None;
    for a in t.parent_ancestors() {
        let s = span(&a);
        let inside = match a.kind() {
            K::ENVIRONMENT => latex_syntax::name(&a).is_some_and(|n| boxed(&n)),
            K::COMMAND | K::INLINE_MATH | K::DISPLAY_MATH => s.start < pos && pos < s.end,
            _ => false,
        };
        if inside {
            out = Some(s.end);
        }
    }
    out.unwrap_or(pos)
}

/// `base` as a label the document does not have yet: `fig:cat`, then
/// `fig:cat-2`; `eq:`, then `eq:2`.
pub fn unique_label(text: &str, base: &str) -> String {
    let taken = |l: &str| text.contains(&format!("\\label{{{l}}}"));
    if !taken(base) {
        return base.to_string();
    }
    let sep = if base.ends_with([':', '-', '_', '.']) {
        ""
    } else {
        "-"
    };
    (2..)
        .map(|n| format!("{base}{sep}{n}"))
        .find(|l| !taken(l))
        .unwrap_or_default()
}

/// Whether `pos` is in running text where `"` means quotes: not in math,
/// code or a comment, and not in a document whose babel language makes
/// `"` a shorthand (German, Dutch and others).
fn in_prose(text: &str, root: &SyntaxNode, pos: usize) -> bool {
    if in_math(root, pos) {
        return false;
    }
    let code = latex_syntax::token_before(root, pos).is_some_and(|t| {
        t.kind() == K::COMMENT
            || t.parent_ancestors().any(|a| {
                a.kind() == K::VERB
                    || (a.kind() == K::ENVIRONMENT
                        && latex_syntax::name(&a)
                            .is_some_and(|n| latex_syntax::signatures::is_verbatim(&n)))
            })
    });
    !code && !quote_shorthand(text)
}

/// Whether the document's babel language makes `"` a shorthand (German,
/// Dutch and others): the preamble, or the first 8 KiB.
pub(crate) fn quote_shorthand(text: &str) -> bool {
    let end = text.find("\\begin{document}").unwrap_or_else(|| {
        // The first 8 KiB, cut at a character.
        let mut e = text.len().min(8192);
        while !text.is_char_boundary(e) {
            e -= 1;
        }
        e
    });
    let preamble = &text[..end];
    preamble.lines().any(|l| {
        l.contains("babel")
            && [
                "german", "dutch", "danish", "finnish", "swedish", "russian", "czech",
            ]
            .iter()
            .any(|lang| l.contains(lang))
    })
}

/// What typing `typed` at the cursor does (T2.7h.15, T2.7h.16): `$` pairs
/// (and steps over the closing one), `\(` and `\[` get their closing
/// pair, `\left(` its `\right)`; `"` in text makes LaTeX's quotes. `None`
/// types it as it is.
pub fn typed(text: &str, sel: Selection, root: &SyntaxNode, typed: &str) -> Option<Transaction> {
    if sel.anchor != sel.head {
        return None;
    }
    let pos = sel.head;
    let before = &text[..pos];
    let escaped = before.ends_with('\\') && !before.ends_with("\\\\");
    let insert = |s: &str, caret: usize| {
        let mut tx = Transaction::new("Typing");
        tx.replace(pos..pos, s).ok()?;
        Some(tx.select(Selection::caret(pos + caret)))
    };
    match typed {
        "$" if !escaped => {
            // The closing `$` of this formula: stepped over.
            if text[pos..].starts_with('$') && in_math(root, pos) {
                let mut tx = Transaction::new("Typing");
                tx.replace(pos..pos, "").ok()?;
                return Some(tx.select(Selection::caret(pos + 1)));
            }
            if in_math(root, pos) {
                return None;
            }
            insert("$$", 1)
        }
        "(" if escaped => insert("(\\)", 1),
        "[" if escaped => insert("[\\]", 1),
        "(" | "[" | "." | "|" if before.ends_with("\\left") => {
            let close = match typed {
                "(" => ")",
                "[" => "]",
                c => c,
            };
            insert(&format!("{typed} \\right{close}"), 1)
        }
        "{" if before.ends_with("\\left\\") => insert("{ \\right\\}", 1),
        // In text, `"` as LaTeX's quotes: ``` `` ``` opening, `''` closing;
        // typed again right after, a plain `"` (as AUCTeX does).
        "\"" if !escaped && in_prose(text, root, pos) => {
            for q in ["``", "''"] {
                if before.ends_with(q) {
                    let mut tx = Transaction::new("Typing");
                    tx.replace(pos - 2..pos, "\"").ok()?;
                    return Some(tx.select(Selection::caret(pos - 1)));
                }
            }
            let opening = before
                .chars()
                .next_back()
                .is_none_or(|c| c.is_whitespace() || "([{~".contains(c));
            let q = if opening { "``" } else { "''" };
            insert(q, 2)
        }
        _ => None,
    }
}

/// Environments whose body is a grid of cells: matrices and `cases`.
pub fn is_grid(name: &str) -> bool {
    matches!(
        name.trim_end_matches('*'),
        "matrix"
            | "pmatrix"
            | "bmatrix"
            | "Bmatrix"
            | "vmatrix"
            | "Vmatrix"
            | "smallmatrix"
            | "cases"
            | "dcases"
            | "rcases"
    )
}

/// In math, the next empty `{}` after the cursor on its line (Tab's stop
/// in `\frac{}{}`, `\sqrt{}`, `\sum_{}^{}`); else, in a matrix or `cases`,
/// the next cell: after the next `&` or the next row's `\\`.
pub fn next_stop(text: &str, pos: usize, root: &SyntaxNode) -> Option<Transaction> {
    if !in_math(root, pos) {
        return None;
    }
    let lr = line_range(text, pos);
    // Past the brace the cursor is in.
    let from = if text[pos..].starts_with('}') {
        pos + 1
    } else {
        pos
    };
    let to = match text[from..lr.end].find("{}") {
        Some(at) => from + at + 1,
        None => next_cell(text, pos, root)?,
    };
    let mut tx = Transaction::new("Next Field");
    tx.replace(pos..pos, "").ok()?;
    Some(tx.select(Selection::caret(to)))
}

/// The start of the cell after the one at `pos` in the matrix or `cases`
/// around it (one blank after the `&` or `\\` kept).
fn next_cell(text: &str, pos: usize, root: &SyntaxNode) -> Option<usize> {
    let t = latex_syntax::token_before(root, pos)?;
    let body = t.parent_ancestors().find(|a| {
        a.kind() == K::BODY
            && a.parent()
                .and_then(|e| latex_syntax::name(&e))
                .is_some_and(|n| is_grid(&n))
    })?;
    let end = span(&body).end;
    let rest = &text[pos..end];
    let amp = rest.find('&');
    let row = rest.find("\\\\");
    let after = match (amp, row) {
        (Some(a), Some(r)) if r < a => r + 2,
        (Some(a), _) => a + 1,
        (None, Some(r)) => r + 2,
        (None, None) => return None,
    };
    let mut at = pos + after;
    // To the cell's text: past the line break and the blanks before it,
    // one blank kept after `&`.
    let skip = text[at..end]
        .find(|c: char| c != ' ' && c != '\t' && c != '\n' && c != '\r')
        .unwrap_or(end - at);
    let blank = text[at..at + skip].rfind(['\n']).map_or(0, |i| i + 1);
    at += if blank > 0 {
        blank + text[at + blank..at + skip].len()
    } else {
        skip.min(1)
    };
    Some(at)
}

/// Inline math at the cursor displayed (`$x$` to `\[x\]`), or displayed
/// math inline.
pub fn toggle_display(text: &str, pos: usize, root: &SyntaxNode) -> Option<Transaction> {
    let t = latex_syntax::token_before(root, pos)?;
    let m = t
        .parent_ancestors()
        .find(|a| matches!(a.kind(), K::INLINE_MATH | K::DISPLAY_MATH))?;
    let r = span(&m);
    let src = &text[r.clone()];
    let body = latex_syntax_body(src)?;
    let new = if m.kind() == K::INLINE_MATH {
        format!("\\[ {} \\]", body.trim())
    } else {
        format!("${}$", body.trim())
    };
    let mut tx = Transaction::new("Display Math");
    tx.replace(r.clone(), new.clone()).ok()?;
    Some(tx.select(Selection::caret(r.start + new.len().min(pos - r.start + 1))))
}

fn latex_syntax_body(src: &str) -> Option<&str> {
    for (a, b) in [("$$", "$$"), ("\\[", "\\]"), ("\\(", "\\)"), ("$", "$")] {
        if let Some(inner) = src.strip_prefix(a).and_then(|s| s.strip_suffix(b)) {
            return Some(inner);
        }
    }
    None
}

/// The math environment at the cursor numbered or not: its name with a
/// star or without, at both ends.
pub fn toggle_numbering(pos: usize, root: &SyntaxNode) -> Option<Transaction> {
    let t = latex_syntax::token_before(root, pos)?;
    let env = t.parent_ancestors().find(|a| {
        a.kind() == K::ENVIRONMENT
            && latex_syntax::name(a).is_some_and(|n| {
                matches!(
                    n.trim_end_matches('*'),
                    "equation"
                        | "align"
                        | "gather"
                        | "multline"
                        | "flalign"
                        | "alignat"
                        | "eqnarray"
                )
            })
    })?;
    let mut tx = Transaction::new("Numbering");
    for n in env
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() == K::ENV_NAME)
    {
        if n.parent_ancestors()
            .find(|a| a.kind() == K::ENVIRONMENT)
            .as_ref()
            != Some(&env)
        {
            continue;
        }
        let r = usize::from(n.text_range().start())..usize::from(n.text_range().end());
        let name = n.text();
        let new = match name.strip_suffix('*') {
            Some(base) => base.to_string(),
            None => format!("{name}*"),
        };
        tx.replace(r, new).ok()?;
    }
    Some(tx)
}

trait Around {
    /// The list environment around this one, if any.
    fn parent_ancestors_list(&self) -> Option<(SyntaxNode, String)>;
}

impl Around for SyntaxNode {
    fn parent_ancestors_list(&self) -> Option<(SyntaxNode, String)> {
        self.ancestors().skip(1).find_map(|a| {
            let name = (a.kind() == K::ENVIRONMENT)
                .then(|| latex_syntax::name(&a))
                .flatten()?;
            is_list(&name).then_some((a, name))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, tx: &Transaction) -> (String, usize) {
        let mut s = text.to_string();
        for e in tx.edits.iter().rev() {
            s.replace_range(e.range.clone(), &e.insert);
        }
        (s, tx.selection_after.map_or(0, |x| x.head))
    }

    fn root(text: &str) -> SyntaxNode {
        latex_syntax::parse(text).syntax()
    }

    #[test]
    fn typed_quotes() {
        // A long text without `\begin{document}` whose 8 KiB end in a
        // letter of two bytes.
        let long = format!("{}ç ", "a".repeat(8191));
        let root = latex_syntax::parse(&long).syntax();
        assert!(typed(&long, Selection::caret(long.len()), &root, "\"").is_some());
        let ty = |text: &str, pos: usize| {
            let root = latex_syntax::parse(text).syntax();
            let tx = typed(text, Selection::caret(pos), &root, "\"")?;
            let mut t = text.to_string();
            for e in tx.edits.iter().rev() {
                t.replace_range(e.range.clone(), &e.insert);
            }
            Some(t)
        };
        assert_eq!(ty("Say ", 4).as_deref(), Some("Say ``"));
        assert_eq!(ty("Say ``hi", 8).as_deref(), Some("Say ``hi''"));
        // Twice: a plain `"`.
        assert_eq!(ty("Say ``", 6).as_deref(), Some("Say \""));
        // Not in math, code, comments, or German documents.
        assert_eq!(ty("$x$", 2), None);
        assert_eq!(ty("\\verb|a|", 7), None);
        assert_eq!(ty("% a", 3), None);
        assert_eq!(ty("\\usepackage[ngerman]{babel}\nSo ", 31), None);
    }

    #[test]
    fn matrix_cells() {
        // Tab goes from cell to cell, and to the next row's first cell.
        let text = "$\\begin{pmatrix}\n  a1 & b2 \\\\\n  c3 & d4\n\\end{pmatrix}$\n";
        let root = latex_syntax::parse(text).syntax();
        let stop = |pos: usize| {
            let tx = next_stop(text, pos, &root)?;
            Some(tx.selection_after?.head)
        };
        let at = |cell: &str| text.find(cell).unwrap();
        assert_eq!(stop(at("a1") + 2), Some(at("b2")));
        assert_eq!(stop(at("b2") + 2), Some(at("c3")));
        assert_eq!(stop(at("c3") + 2), Some(at("d4")));
        assert_eq!(stop(at("d4") + 2), None);
        // An empty `{}` first.
        let text = "$\\sum_{i}^{}$";
        let root = latex_syntax::parse(text).syntax();
        let tx = next_stop(text, 8, &root).unwrap();
        assert_eq!(tx.selection_after.unwrap().head, 11);
    }

    #[test]
    fn enter_in_lists() {
        let t = "\\begin{itemize}\n  \\item One\n\\end{itemize}\nAfter\n";
        let at = t.find("One").unwrap() + 3;
        let tx = enter(t, Selection::caret(at), &root(t)).unwrap();
        let (s, c) = apply(t, &tx);
        assert_eq!(
            s,
            "\\begin{itemize}\n  \\item One\n  \\item \n\\end{itemize}\nAfter\n"
        );
        // Enter again on the empty item: out of the list.
        let tx = enter(&s, Selection::caret(c), &root(&s)).unwrap();
        let (s2, c2) = apply(&s, &tx);
        assert_eq!(
            s2,
            "\\begin{itemize}\n  \\item One\n\\end{itemize}\n\nAfter\n"
        );
        assert_eq!(c2, s2.find("\n\nAfter").unwrap() + 1);
        assert!(enter(t, Selection::caret(t.len() - 2), &root(t)).is_none());
    }

    #[test]
    fn begin_and_names() {
        let t = "  \\begin{align}";
        let p = latex_syntax::parse(t);
        let tx = complete_begin(t, t.len(), &p).unwrap();
        let (s, c) = apply(t, &tx);
        assert_eq!(s, "  \\begin{align}\n    \n  \\end{align}");
        assert_eq!(c, s.find("\n    ").unwrap() + 5);
        // Closed already: nothing.
        let t = "\\begin{x}\\end{x}";
        assert!(complete_begin(t, "\\begin{x}".len(), &latex_syntax::parse(t)).is_none());
        // Renaming one end renames the other.
        let t = "\\begin{itemize}\nx\n\\end{itemize}\n";
        let at = t.find("itemize").unwrap() + 4;
        let m = mirror(&root(t), at..at + 3).unwrap();
        assert_eq!(&t[m.clone()], "ize");
        assert!(m.start > t.find("\\end").unwrap());
    }

    #[test]
    fn edits_that_compile() {
        // Found by `tools/latex-edit-fuzz.py`: each would have left a
        // document pdflatex rejects.
        // Not a word of markup, nor across an environment's edge, a
        // paragraph, an item, a table's cell or a caption.
        let t = "\\begin{itemize}\n\\item a\n\\end{itemize}\nx\n\ny \\caption{C} z\n";
        let at = |s: &str| t.find(s).unwrap();
        for (a, b) in [
            (at("end{") + 5, at("end{") + 5),
            (at("item a") + 6, at("x")),
            (at("x"), at("y")),
            (at("y"), at("z")),
        ] {
            assert!(toggle(t, Selection { anchor: a, head: b }, &root(t), "textbf").is_none());
        }
        let t = "\\begin{tabular}{ll}\na & b \\\\\n\\end{tabular}\n";
        let sel = Selection {
            anchor: t.find('a').unwrap(),
            head: t.find('b').unwrap() + 1,
        };
        assert!(toggle(t, sel, &root(t), "emph").is_none());
        // Where text can go.
        let t = "\\section{A} x \\label{k} $y$ \\begin{tabular}{l}\n\\hline\n\\end{tabular}\n";
        let r = root(t);
        assert!(in_text(&r, t.find(" x").unwrap() + 1));
        assert!(!in_text(&r, t.find("{A}").unwrap()));
        assert!(!in_text(&r, t.find("{k}").unwrap() + 1));
        assert!(!in_text(&r, t.find('y').unwrap()));
        assert!(in_text(&r, t.find("$ ").unwrap() + 1));
        assert!(!in_text(&r, t.find("tabular}").unwrap() + 3));
        assert!(!in_text(&r, t.find("\\hline").unwrap()));
        let t = "\\begin{itemize}\n  \\item a\n\\end{itemize}\n";
        assert!(!in_text(&root(t), t.find("  \\item").unwrap() + 1));
        // A block goes after a float or a command it is asked for in.
        let t =
            "\\begin{document}\n\\begin{figure}\n\\caption{C}\n\\end{figure}\nx\n\\end{document}\n";
        let r = root(t);
        let end = t.find("\\end{figure}").unwrap() + "\\end{figure}".len();
        assert_eq!(block_position(t, &r, t.find('C').unwrap()), end);
        assert_eq!(block_position(t, &r, 0), t.find('\n').unwrap());
        // Labels not taken twice.
        let t = "\\label{eq:} \\label{fig:a} \\label{fig:a-2}";
        assert_eq!(unique_label(t, "eq:"), "eq:2");
        assert_eq!(unique_label(t, "fig:a"), "fig:a-3");
        assert_eq!(unique_label(t, "tab:"), "tab:");
        // Enter ends a row of rows, and begins a list's first item.
        let t = "\\begin{align}\na &= b\n\\end{align}\n";
        let pos = t.find("= b").unwrap() + 3;
        let tx = enter(t, Selection::caret(pos), &root(t)).unwrap();
        assert_eq!(
            apply(t, &tx).0,
            "\\begin{align}\na &= b \\\\\n\n\\end{align}\n"
        );
        let t = "\\begin{enumerate}\n\\end{enumerate}\n";
        let tx = enter(t, Selection::caret(17), &root(t)).unwrap();
        assert_eq!(
            apply(t, &tx).0,
            "\\begin{enumerate}\n  \\item \n\\end{enumerate}\n"
        );
    }

    #[test]
    fn toggles() {
        let t = "some words here";
        let tx = toggle(t, Selection::caret(6), &root(t), "textbf").unwrap();
        let (s, _) = apply(t, &tx);
        assert_eq!(s, "some \\textbf{words} here");
        let tx = toggle(&s, Selection::caret(15), &root(&s), "textbf").unwrap();
        assert_eq!(apply(&s, &tx).0, t);
        // Italics is one family: `\textit` unwrapped by the emphasis toggle.
        let t = "a \\textit{b} c";
        let tx = toggle(t, Selection::caret(11), &root(t), "emph").unwrap();
        assert_eq!(apply(t, &tx).0, "a b c");
        // A selection cutting a command takes all of it.
        let t = "a \\emph{b c} d";
        let sel = Selection {
            anchor: t.find('c').unwrap(),
            head: t.len(),
        };
        let tx = toggle(t, sel, &root(t), "textbf").unwrap();
        assert_eq!(apply(t, &tx).0, "a \\textbf{\\emph{b c} d}");
        let sel = Selection {
            anchor: 0,
            head: t.find('b').unwrap() + 1,
        };
        let tx = toggle(t, sel, &root(t), "textbf").unwrap();
        assert_eq!(apply(t, &tx).0, "\\textbf{a \\emph{b c}} d");
        // And math.
        let t = "x $a+b$ y";
        let sel = Selection { anchor: 0, head: 4 };
        let tx = toggle(t, sel, &root(t), "textbf").unwrap();
        assert_eq!(apply(t, &tx).0, "\\textbf{x $a+b$} y");
    }

    #[test]
    fn style_inference() {
        assert_eq!(
            Style::infer(""),
            Style {
                step: "  ".into(),
                label: LabelPlace::AfterCaption,
                equation_label_inline: false,
                prefixes: ["fig:".into(), "tab:".into(), "eq:".into()],
            }
        );
        let t = "\\begin{document}\n\\begin{figure}\n\t\\centering\n\t\\caption{A cat.\\label{f-cat}}\n\\end{figure}\n\\begin{equation}\\label{e-one}\n\tx\n\\end{equation}\n\\begin{table}\n\t\\caption{T.\\label{t-x}}\n\\end{table}\n";
        let s = Style::infer(t);
        assert_eq!(s.step, "\t");
        assert_eq!(s.label, LabelPlace::InCaption);
        assert!(s.equation_label_inline);
        assert_eq!(s.prefixes, ["f-".to_string(), "t-".into(), "e-".into()]);
        let t = "\\begin{figure}\n\\label{fig:a}\n\\caption{A}\n\\end{figure}\n";
        let s = Style::infer(t);
        assert_eq!((s.step.as_str(), s.label), ("", LabelPlace::BeforeCaption));
    }

    #[test]
    fn sections() {
        let t =
            "\\documentclass{article}\nIntro\n\\section{A}\na\n\\subsection{A1}\n\\section{B}\nb\n";
        let r = root(t);
        let tx = set_level(t, t.find("Intro").unwrap(), &r, Some("article"), 1).unwrap();
        assert!(apply(t, &tx).0.contains("\\section{Intro}\n"));
        let tx = set_level(t, t.find("{A}").unwrap(), &r, Some("article"), 2).unwrap();
        assert!(apply(t, &tx).0.contains("\\subsection{A}\n"));
        let tx = promote(t, t.find("{A}").unwrap(), &r, false).unwrap();
        let (s, _) = apply(t, &tx);
        assert!(s.contains("\\subsection{A}\na\n\\subsubsection{A1}\n\\section{B}"));
        let tx = move_section(t, t.find("{A}").unwrap(), &r, true).unwrap();
        let (s, _) = apply(t, &tx);
        assert_eq!(
            s,
            "\\documentclass{article}\nIntro\n\\section{B}\nb\n\\section{A}\na\n\\subsection{A1}\n"
        );
    }

    #[test]
    fn math_editing() {
        let t = "Let x";
        let r = root(t);
        let tx = typed(t, Selection::caret(4), &r, "$").unwrap();
        let (s, c) = apply(t, &tx);
        assert_eq!((s.as_str(), c), ("Let $$x", 5));
        // In the formula, `$` steps over the closing one.
        let s = "Let $a$ x";
        let tx = typed(s, Selection::caret(6), &root(s), "$").unwrap();
        assert_eq!(apply(s, &tx), (s.to_string(), 7));
        assert!(typed("a \\", Selection::caret(3), &root("a \\"), "$").is_none());
        let s = "a \\";
        let tx = typed(s, Selection::caret(3), &root(s), "(").unwrap();
        assert_eq!(apply(s, &tx).0, "a \\(\\)");
        let s = "$\\left$";
        let tx = typed(s, Selection::caret(6), &root(s), "(").unwrap();
        assert_eq!(apply(s, &tx).0, "$\\left( \\right)$");
        // Tab stops.
        let s = "$\\frac{a}{}$";
        let tx = next_stop(s, 7, &root(s)).unwrap();
        assert_eq!(apply(s, &tx).1, 10);
        // Inline and displayed.
        let s = "x $a+b$ y";
        let tx = toggle_display(s, 4, &root(s)).unwrap();
        assert_eq!(apply(s, &tx).0, "x \\[ a+b \\] y");
        let s = "\\begin{equation}\na\n\\end{equation}";
        let tx = toggle_numbering(17, &root(s)).unwrap();
        assert_eq!(apply(s, &tx).0, "\\begin{equation*}\na\n\\end{equation*}");
    }

    #[test]
    fn nesting() {
        let t = "\\begin{itemize}\n\\item One\n\\item Two\n\\end{itemize}\n";
        let tx = indent_item(t, t.find("Two").unwrap(), &root(t), true).unwrap();
        let (s, _) = apply(t, &tx);
        assert_eq!(
            s,
            "\\begin{itemize}\n\\item One\n\\begin{itemize}\n  \\item Two\n\\end{itemize}\n\\end{itemize}\n"
        );
        assert!(indent_item(t, t.find("One").unwrap(), &root(t), true).is_none());
        let tx = indent_item(&s, s.find("Two").unwrap(), &root(&s), false).unwrap();
        assert_eq!(apply(&s, &tx).0, t);
    }
}
