//! Incremental reparsing: the paragraph around an edit, in the innermost
//! closed environment containing it, parsed again and spliced into the
//! tree, when nothing outside it can change. Otherwise `None`, and the
//! caller parses the whole text.
//!
//! What makes a paragraph independent: it lies between paragraph breaks
//! (no group, math or argument of the tree crosses one at the level of its
//! container unless it is a single element of a paragraph), and it is
//! balanced before and after the edit (no diagnostic in it: a `}` or an
//! `\end` pairing with something outside shows up as one). The first and
//! last characters of the region are outside the edit, so the tokens
//! around it lex as before. Sectioning commands matter only when an
//! environment is left open somewhere, and `\makeatletter` changes the
//! lexing of what follows; edits near either take the full parse.

use rowan::NodeOrToken;

use crate::SyntaxKind::*;
use crate::parser::{Mode, Parser};
use crate::{Parse, SyntaxElement, SyntaxNode, TextEdit, signatures};

fn span(el: &SyntaxElement) -> (usize, usize) {
    let r = el.text_range();
    (r.start().into(), r.end().into())
}

/// The body of a closed, not verbatim environment among the children of
/// `paragraph` that holds `s..e` strictly inside, with its mode.
fn inner_body(paragraph: &SyntaxNode, s: usize, e: usize) -> Option<(SyntaxNode, Mode)> {
    paragraph
        .children()
        .filter(|n| n.kind() == ENVIRONMENT)
        .find_map(|env| {
            let name = crate::name(&env)?;
            if signatures::is_verbatim(&name) || !env.children().any(|c| c.kind() == END) {
                return None;
            }
            let body = env.children().find(|c| c.kind() == BODY)?;
            let (bs, be) = span(&NodeOrToken::Node(body.clone()));
            // After the first solid token of the body (so that the
            // arguments of `\begin` cannot change), and before `\end`.
            let first = body
                .descendants_with_tokens()
                .filter_map(|t| t.into_token())
                .find(|t| !matches!(t.kind(), WHITESPACE | NEWLINE | PAR_BREAK))?;
            let fs: usize = first.text_range().start().into();
            if first.text().starts_with('[') || s <= fs || s <= bs || e >= be {
                return None;
            }
            let mode = if signatures::is_math(&name) {
                Mode::Math
            } else {
                Mode::Text
            };
            Some((body, mode))
        })
}

pub(crate) fn reparse(old: &Parse, new_text: &str, edit: &TextEdit) -> Option<Parse> {
    let (s, e) = (edit.range.start, edit.range.end);
    let old_len: usize = old.green.text_len().into();
    let delta = edit.insert.len() as isize - (e - s) as isize;
    if e > old_len || new_text.len() as isize != old_len as isize + delta {
        return None;
    }
    let root = old.syntax();
    let mut container = root.clone();
    let mut mode = Mode::Text;
    let kids = loop {
        let kids: Vec<SyntaxElement> = container.children_with_tokens().collect();
        if kids.is_empty() {
            return None;
        }
        let a = kids.partition_point(|k| span(k).1 <= s).min(kids.len() - 1);
        let b = kids.partition_point(|k| span(k).1 <= e).min(kids.len() - 1);
        if a == b
            && let Some(p) = kids[a].as_node()
            && p.kind() == PARAGRAPH
            && let Some((body, m)) = inner_body(p, s, e)
        {
            container = body;
            mode = m;
            continue;
        }
        break (kids, a, b);
    };
    let (kids, a, b) = kids;
    let is_break = |el: &SyntaxElement| el.kind() == PAR_BREAK;
    let mut i = a;
    while !(is_break(&kids[i]) && span(&kids[i]).0 < s) && i > 0 {
        i -= 1;
    }
    let mut j = b;
    while !(is_break(&kids[j]) && span(&kids[j]).1 > e) && j + 1 < kids.len() {
        j += 1;
    }
    let starts_with_break = is_break(&kids[i]) && span(&kids[i]).0 < s;
    let ends_with_break = is_break(&kids[j]) && span(&kids[j]).1 > e;
    let rs = span(&kids[i]).0;
    let re = span(&kids[j]).1;
    let is_root = container.parent().is_none();
    // At the ends of an environment's body, the edit keeps off the
    // `\begin` and `\end` around it (`inner_body`); at the ends of the
    // document there is nothing to keep off.
    if !starts_with_break && !is_root && s <= rs {
        return None;
    }
    if !ends_with_break && !is_root && e >= re {
        return None;
    }
    // Balanced before the edit.
    if old
        .diagnostics
        .iter()
        .any(|d| d.range.start <= re && d.range.end >= rs)
        || old.toggles.iter().any(|&(p, _)| p >= rs && p < re)
        // An environment left open (a verbatim one's body runs to where
        // the parser decides): the pre-scan's `\makeatletter` toggles may
        // lie inside it, where the parser does not read them.
        || (old.unclosed_env && !old.toggles.is_empty())
    {
        return None;
    }
    let sections = |from: usize, to: usize| {
        kids[from..=to].iter().any(|k| {
            k.as_node().is_some_and(|n| {
                n.descendants_with_tokens()
                    .filter_map(|t| t.into_token())
                    .any(|t| t.kind() == CONTROL_WORD && signatures::is_sectioning(&t.text()[1..]))
            })
        })
    };
    if old.unclosed_env && sections(i, j) {
        return None;
    }
    let new_re = (re as isize + delta) as usize;
    // A verbatim environment's `\\begin` or `\\end` in the region, before or
    // after the edit, can end or start one elsewhere (inside it `%` is no
    // comment): only a full parse knows.
    let old_region: String = kids[i..=j].iter().map(|k| k.to_string()).collect();
    if verbatim_edge(&old_region) || verbatim_edge(&new_text[rs..new_re]) {
        return None;
    }
    // A comment or a verbatim argument on the last line could reach past
    // the end of an environment's body in a full parse.
    if !ends_with_break && !is_root {
        let tail = &new_text[rs..new_re];
        let line = &tail[tail.rfind(['\n', '\r']).map_or(0, |n| n + 1)..];
        if line.contains('%')
            || ["\\verb", "\\url", "\\href", "\\lstinline"]
                .iter()
                .any(|c| line.contains(c))
        {
            return None;
        }
    }
    let at_letter = old
        .toggles
        .iter()
        .rev()
        .find(|&&(p, _)| p < rs)
        .is_some_and(|&(_, on)| on);
    // A definition edited may change how the whole text parses; macros
    // for an equation (`\be … \ee`) pair across paragraphs: only a full
    // parse knows.
    let region = &new_text[rs..new_re];
    if crate::tables::defines(region) || crate::tables::defines(&old_region) {
        return None;
    }
    let defs = &old.defs;
    let uses = |text: &str, n: &String| {
        let pat = format!("\\{n}");
        text.match_indices(&pat)
            .any(|(k, _)| !text[k + pat.len()..].starts_with(|c: char| c.is_ascii_alphabetic()))
    };
    let old_region_has = |n: &String| uses(region, n) || uses(&old_region, n);
    if defs
        .openers
        .iter()
        .chain(defs.closers.iter())
        .any(old_region_has)
    {
        return None;
    }
    // A usual name (`\ee`) the document took out of the aliases by
    // defining it, its definition in another paragraph (`\def` with blank
    // lines before the name): the edit may change which it is.
    let usual = |text: &str| {
        text.match_indices('\\').any(|(k, _)| {
            let w: String = text[k + 1..]
                .chars()
                .take_while(char::is_ascii_alphabetic)
                .collect();
            crate::tables::usual_alias(&w)
        })
    };
    if usual(region) || usual(&old_region) {
        return None;
    }
    let mut p = Parser::new(new_text, rs, new_re, at_letter, defs);
    // In a table's cells, `$$` is an empty formula.
    p.cells = container
        .ancestors()
        .filter(|a| a.kind() == ENVIRONMENT)
        .filter_map(|a| crate::name(&a))
        .filter(|n| {
            matches!(
                n.as_str(),
                "tabular" | "tabular*" | "tabularx" | "tabulary" | "longtable" | "longtable*"
            )
        })
        .count();
    if !p.toggles().is_empty() || (old.unclosed_env && p.has_sections()) {
        return None;
    }
    let (body, p) = p.finish(BODY, new_re, mode);
    if !p.diagnostics.is_empty() || p.unclosed_env {
        return None;
    }
    let children: Vec<_> = body.children().map(|c| c.to_owned()).collect();
    let first_break = children
        .first()
        .is_some_and(|c| c.kind() == PAR_BREAK.into());
    let last_break = children
        .last()
        .is_some_and(|c| c.kind() == PAR_BREAK.into());
    if (starts_with_break && !first_break) || (ends_with_break && !last_break) {
        return None;
    }
    let spliced = container.green().splice_children(i..j + 1, children);
    let green = if is_root {
        spliced
    } else {
        container.replace_with(spliced)
    };
    let shift = |p: usize| {
        if p >= re {
            (p as isize + delta) as usize
        } else {
            p
        }
    };
    let diagnostics = old
        .diagnostics
        .iter()
        .map(|d| crate::Diagnostic {
            range: shift(d.range.start)..shift(d.range.end),
            message: d.message.clone(),
        })
        .collect();
    let toggles = old.toggles.iter().map(|&(p, on)| (shift(p), on)).collect();
    Some(Parse {
        green,
        diagnostics,
        toggles,
        unclosed_env: old.unclosed_env,
        defs: old.defs.clone(),
    })
}

/// Whether `s` holds the `\\begin` or `\\end` of a verbatim environment.
fn verbatim_edge(s: &str) -> bool {
    ["\\begin{", "\\end{"].iter().any(|pat| {
        s.match_indices(pat).any(|(i, _)| {
            let rest = &s[i + pat.len()..];
            rest.find('}')
                .is_some_and(|close| signatures::is_verbatim(rest[..close].trim()))
        })
    })
}
