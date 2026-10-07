//! Editing code (T2.7a.5): the bracket matching the one at the cursor,
//! a new line indented after an opening bracket, a closing bracket that
//! goes back a level, and comments toggled with the language's marker.

use std::cell::RefCell;

use org_edit::{Selection, Transaction};

/// One indentation level of `doc` as text: a tab or spaces.
pub fn indent_text(doc: &crate::DocumentState) -> String {
    match doc.indent_unit() {
        crate::text::Indent::Tabs => "\t".into(),
        crate::text::Indent::Spaces(n) => " ".repeat(n),
    }
}

/// The language of the source or export block whose contents hold `pos`
/// (between its first and last lines), from the cursor's token up: asked
/// at every key press (Toggle Comment's context), it walks no more than
/// the cursor's ancestors. The parse may be of the text before the last
/// edits (a reparse running): its ranges are sliced with care.
fn block_language(root: &org_syntax::SyntaxNode, text: &str, pos: usize) -> Option<String> {
    use org_syntax::SyntaxKind as K;
    let len = usize::from(root.text_range().end());
    if len == 0 {
        return None;
    }
    let offset = org_syntax::TextSize::try_from(pos.min(len - 1)).ok()?;
    let token = root.token_at_offset(offset).right_biased()?;
    let block = token
        .parent_ancestors()
        .find(|a| matches!(a.kind(), K::SRC_BLOCK | K::EXPORT_BLOCK))?;
    let r = block.text_range();
    let (s, e) = (usize::from(r.start()), usize::from(r.end()));
    let body = text.get(s..e)?;
    let first_end = body.find('\n').map_or(e, |i| s + i);
    let last_start = body.trim_end().rfind('\n').map_or(e, |i| s + i + 1);
    if pos <= first_end || pos >= last_start {
        return None;
    }
    crate::view::kind_of(&block)
        .highlight_language()
        .map(str::to_string)
}

/// The language of the code at the cursor of `doc`: a plain text file's
/// (`rs`), a source block's in an Org document, or `org` elsewhere in
/// one.
pub fn language_at(doc: &crate::DocumentState) -> Option<String> {
    match &doc.meta.mode {
        crate::DocumentMode::Text { language } => language.clone(),
        crate::DocumentMode::Markdown => Some("md".into()),
        crate::DocumentMode::Latex => Some("latex".into()),
        crate::DocumentMode::Org => {
            let (p, _) = doc.parse()?;
            let lang = block_language(&p.syntax(), doc.text().as_str(), doc.selection.head);
            Some(lang.unwrap_or_else(|| "org".into()))
        }
        _ => None,
    }
}

/// How far the matching bracket is looked for, each way.
const REACH: usize = 256 * 1024;

fn partner(c: u8) -> Option<(u8, bool)> {
    Some(match c {
        b'(' => (b')', true),
        b'[' => (b']', true),
        b'{' => (b'}', true),
        b')' => (b'(', false),
        b']' => (b'[', false),
        b'}' => (b'{', false),
        _ => return None,
    })
}

/// The bracket at `at` and the one matching it, if any.
fn match_at(text: &str, at: usize) -> Option<(usize, usize)> {
    let b = text.as_bytes();
    let (want, forward) = partner(*b.get(at)?)?;
    let me = b[at];
    let mut depth = 0usize;
    if forward {
        let end = (at + REACH).min(b.len());
        for (i, &c) in b[at + 1..end].iter().enumerate() {
            if c == me {
                depth += 1;
            } else if c == want {
                if depth == 0 {
                    return Some((at, at + 1 + i));
                }
                depth -= 1;
            }
        }
    } else {
        let start = at.saturating_sub(REACH);
        for i in (start..at).rev() {
            let c = b[i];
            if c == me {
                depth += 1;
            } else if c == want {
                if depth == 0 {
                    return Some((i, at));
                }
                depth -= 1;
            }
        }
    }
    None
}

/// The pair of brackets next to `pos`: the bracket just before it, else
/// the one at it, and its match (opening first).
pub fn matching(text: &str, pos: usize) -> Option<(usize, usize)> {
    let before = pos
        .checked_sub(1)
        .filter(|p| partner(text.as_bytes()[*p]).is_some());
    before
        .and_then(|p| match_at(text, p))
        .or_else(|| match_at(text, pos))
}

/// The last pair found: for the text version, cursor and length.
type PairMemo = Option<((u64, u64, usize, usize), Option<(usize, usize)>)>;

thread_local! {
    static PAIR: RefCell<PairMemo> = const { RefCell::new(None) };
}

/// [`matching`] for the cursor of `doc`, kept while the text and the
/// cursor stay as they are (the frontends ask for every line they draw).
pub fn pair_at_cursor(doc: &crate::DocumentState) -> Option<(usize, usize)> {
    if doc.dired.is_some() {
        return None;
    }
    let key = (
        doc.serial(),
        doc.version(),
        doc.selection.head,
        doc.text().len(),
    );
    PAIR.with(|p| {
        if let Some((k, v)) = *p.borrow()
            && k == key
        {
            return v;
        }
        let v = matching(doc.text().as_str(), doc.selection.head);
        *p.borrow_mut() = Some((key, v));
        v
    })
}

/// How a language writes a comment: a line marker (`//`), or the ends of
/// a block comment for languages without one (`<!--`, `-->`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentStyle {
    /// A marker before the text of each line.
    Line(&'static str),
    /// Markers around the text of each line.
    Block(&'static str, &'static str),
}

/// The comment markers of `language` (a name or extension as the modes
/// and source blocks give it).
pub fn comment_style(language: &str) -> Option<CommentStyle> {
    use CommentStyle::*;
    // A language plugin's markers first (HEEx's `<%!-- --%>`).
    if let Some(style) = crate::languages::comment_style(language) {
        return Some(style);
    }
    let l = language.to_ascii_lowercase();
    Some(match l.as_str() {
        "rs" | "rust" | "c" | "h" | "cc" | "cpp" | "c++" | "hpp" | "cxx" | "java" | "js"
        | "javascript" | "jsx" | "ts" | "typescript" | "tsx" | "go" | "swift" | "kt" | "kotlin"
        | "scala" | "cs" | "csharp" | "dart" | "groovy" | "zig" | "d" | "php" | "json5"
        | "jsonc" | "proto" | "glsl" | "hlsl" | "less" | "scss" | "sass" | "v" => Line("//"),
        "py" | "python" | "sh" | "bash" | "zsh" | "fish" | "shell" | "rb" | "ruby" | "pl"
        | "perl" | "r" | "yaml" | "yml" | "toml" | "make" | "makefile" | "mk" | "cmake"
        | "conf" | "cfg" | "dockerfile" | "nix" | "jl" | "julia" | "tcl" | "ps1" | "powershell"
        | "elixir" | "ex" | "exs" | "nim" | "coffee" | "gitignore" | "awk" | "sed"
        | "properties" | "env" | "crystal" | "cr" | "graphql" => Line("#"),
        "lua" | "sql" | "hs" | "haskell" | "elm" | "ada" | "adb" | "ads" | "vhdl" | "purs" => {
            Line("--")
        }
        "lisp" | "el" | "elisp" | "emacs-lisp" | "clj" | "clojure" | "cljs" | "scm" | "scheme"
        | "rkt" | "racket" | "fnl" | "fennel" | "ini" | "asm" | "s" => Line(";"),
        "tex" | "latex" | "sty" | "cls" | "bib" | "erl" | "erlang" | "m" | "matlab" | "octave"
        | "pro" | "prolog" | "ps" | "postscript" => Line("%"),
        "vim" | "vimscript" => Line("\""),
        "f90" | "f95" | "fortran" => Line("!"),
        "bat" | "cmd" => Line("REM"),
        "org" => Line("#"),
        "html" | "htm" | "xml" | "xhtml" | "svg" | "md" | "markdown" | "vue" | "plist" => {
            Block("<!--", "-->")
        }
        "css" => Block("/*", "*/"),
        "ml" | "ocaml" | "fs" | "fsharp" => Block("(*", "*)"),
        _ => return None,
    })
}

/// The lines a selection from `a` to `b` covers: a selection that ends at
/// the start of a line leaves that line out.
fn covered_lines(text: &str, a: usize, b: usize) -> (usize, usize) {
    let start = text[..a].rfind('\n').map_or(0, |i| i + 1);
    let b = if b > a && text[..b].ends_with('\n') {
        b - 1
    } else {
        b
    };
    let end = text[b..].find('\n').map_or(text.len(), |i| b + i);
    (start, end)
}

/// Comments out the lines the selection covers with `style`, or, when
/// every line that is not blank is commented already, uncomments them.
/// The marker goes at the smallest indentation of the lines, with a space
/// after it; uncommenting removes that space too.
pub fn toggle_comment(text: &str, sel: Selection, style: CommentStyle) -> Transaction {
    let (a, b) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
    let (start, end) = covered_lines(text, a, b);
    let lines: Vec<(usize, &str)> = {
        let mut out = Vec::new();
        let mut at = start;
        for l in text[start..end].split('\n') {
            out.push((at, l));
            at += l.len() + 1;
        }
        out
    };
    let indent = |l: &str| l.len() - l.trim_start_matches([' ', '\t']).len();
    let open = match style {
        CommentStyle::Line(m) | CommentStyle::Block(m, _) => m,
    };
    let close = match style {
        CommentStyle::Block(_, c) => Some(c),
        CommentStyle::Line(_) => None,
    };
    let content: Vec<&(usize, &str)> = lines.iter().filter(|(_, l)| !l.trim().is_empty()).collect();
    let commented = !content.is_empty()
        && content.iter().all(|(_, l)| {
            let t = l.trim_start_matches([' ', '\t']);
            t.starts_with(open) && close.is_none_or(|c| t.trim_end().ends_with(c))
        });
    let mut tx = Transaction::new(if commented { "Uncomment" } else { "Comment" });
    if commented {
        for (at, l) in &content {
            let i = indent(l);
            let t = &l[i..];
            let mut n = open.len();
            if t[n..].starts_with(' ') {
                n += 1;
            }
            let _ = tx.replace(at + i..at + i + n, "");
            if let Some(c) = close {
                let body = t.trim_end();
                let mut e = body.len() - c.len();
                if body[..e].ends_with(' ') && e > n {
                    e -= 1;
                }
                let _ = tx.replace(at + i + e..at + i + body.len(), "");
            }
        }
    } else {
        let col = content.iter().map(|(_, l)| indent(l)).min().unwrap_or(0);
        for (at, l) in &content {
            let _ = tx.replace(at + col..at + col, format!("{open} "));
            if let Some(c) = close {
                let e = l.trim_end().len();
                let _ = tx.replace(at + e..at + e, format!(" {c}"));
            }
        }
    }
    let map = |p: usize| tx.map(p, org_edit::Assoc::After);
    let after = Selection {
        anchor: map(sel.anchor),
        head: map(sel.head),
    };
    tx.select(after)
}

/// Enter in code: a new line with the indentation of the current one, a
/// level deeper after an opening bracket (and after `:` in Python and
/// YAML), and between a pair of brackets (`{|}`) the closing one on a line
/// of its own. `unit` is one indentation level.
pub fn newline(text: &str, sel: Selection, unit: &str, language: Option<&str>) -> Transaction {
    let (a, b) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
    let bol = text[..a].rfind('\n').map_or(0, |i| i + 1);
    let line = &text[bol..a];
    let indent: String = line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let before = line.trim_end_matches([' ', '\t']);
    let colon = matches!(
        language.map(str::to_ascii_lowercase).as_deref(),
        Some("py" | "python" | "yaml" | "yml" | "nim")
    );
    let opens = before.ends_with(['{', '(', '[']) || (colon && before.ends_with(':'));
    let closer = text[b..].chars().next();
    let pair = opens
        && matches!(
            (before.chars().last(), closer),
            (Some('{'), Some('}')) | (Some('('), Some(')')) | (Some('['), Some(']'))
        );
    let mut tx = Transaction::new("New line");
    let (insert, cursor) = if pair {
        let inner = format!("\n{indent}{unit}");
        (format!("{inner}\n{indent}"), inner.len())
    } else if opens {
        let s = format!("\n{indent}{unit}");
        let n = s.len();
        (s, n)
    } else {
        let s = format!("\n{indent}");
        let n = s.len();
        (s, n)
    };
    tx.edit(a..b, insert);
    tx.select(Selection::caret(a + cursor))
}

/// Typing a closing bracket where only indentation is before it on the
/// line: one level less of it first (`unit`), when the line is indented
/// deeper than the line of the opening bracket.
pub fn dedent_for(
    text: &str,
    pos: usize,
    typed: &str,
    unit: &str,
) -> Option<std::ops::Range<usize>> {
    if !matches!(typed, "}" | ")" | "]") || unit.is_empty() {
        return None;
    }
    let bol = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let before = &text[bol..pos];
    if !before.chars().all(|c| c == ' ' || c == '\t') || !before.ends_with(unit) {
        return None;
    }
    Some(pos - unit.len()..pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, tx: Transaction) -> (String, usize) {
        let p = tx.selection_after.map_or(0, |s| s.head);
        (tx.apply(text), p)
    }

    #[test]
    fn brackets() {
        let t = "f(a[1], {b: (c)})";
        assert_eq!(matching(t, 2), Some((1, 16)));
        assert_eq!(matching(t, 1), Some((1, 16)));
        assert_eq!(matching(t, 17), Some((1, 16)));
        assert_eq!(matching(t, 4), Some((3, 5)));
        assert_eq!(matching(t, 0), None);
        assert_eq!(matching("(()", 1), Some((1, 2)));
        assert_eq!(matching("((", 1), None);
    }

    #[test]
    fn comments() {
        let t = "fn a() {\n    let x = 1;\n\n    x\n}\n";
        let sel = Selection {
            anchor: 9,
            head: 30,
        };
        let (c, _) = apply(t, toggle_comment(t, sel, CommentStyle::Line("//")));
        assert_eq!(c, "fn a() {\n    // let x = 1;\n\n    // x\n}\n");
        let sel = Selection {
            anchor: 9,
            head: 36,
        };
        let (u, _) = apply(&c, toggle_comment(&c, sel, CommentStyle::Line("//")));
        assert_eq!(u, t);
        // A mix of commented and plain lines: all commented.
        let m = "# a\nb\n";
        let (x, _) = apply(
            m,
            toggle_comment(m, Selection { anchor: 0, head: 5 }, CommentStyle::Line("#")),
        );
        assert_eq!(x, "# # a\n# b\n");
        // Block markers on each line.
        let h = "<p>x</p>\n";
        let (x, _) = apply(
            h,
            toggle_comment(h, Selection::caret(2), CommentStyle::Block("<!--", "-->")),
        );
        assert_eq!(x, "<!-- <p>x</p> -->\n");
        let (y, _) = apply(
            &x,
            toggle_comment(&x, Selection::caret(2), CommentStyle::Block("<!--", "-->")),
        );
        assert_eq!(y, h);
        assert_eq!(comment_style("rs"), Some(CommentStyle::Line("//")));
        assert_eq!(comment_style("Python"), Some(CommentStyle::Line("#")));
        assert_eq!(comment_style("nope"), None);
    }

    #[test]
    fn the_language_of_a_block_at_the_cursor() {
        let t = "* H\n#+begin_src python\nx = 1\n#+end_src\n- item\n  #+begin_src rust\n  let y;\n  #+end_src\n#+begin_export html\n<p>\n#+end_export\n";
        let root = org_syntax::parse(t).syntax();
        let at = |s: &str| t.find(s).unwrap();
        let lang = |p: usize| block_language(&root, t, p);
        assert_eq!(lang(at("x = 1")).as_deref(), Some("python"));
        // Not on the first or the last line.
        assert_eq!(lang(at("#+begin_src python") + 3), None);
        assert_eq!(lang(at("#+end_src") + 2), None);
        // In a list item too.
        assert_eq!(lang(at("let y")).as_deref(), Some("rust"));
        assert_eq!(lang(at("<p>")).as_deref(), Some("html"));
        assert_eq!(lang(at("item")), None);
        // A parse of a longer text than the one at hand: no slicing past
        // its end.
        assert_eq!(block_language(&root, &t[..30], at("let y")), None);
    }

    #[test]
    fn indenting() {
        let t = "fn a() {}";
        let (n, p) = apply(t, newline(t, Selection::caret(8), "    ", Some("rs")));
        assert_eq!((n.as_str(), p), ("fn a() {\n    \n}", 13));
        let t = "  if x:";
        let (n, _) = apply(t, newline(t, Selection::caret(7), "    ", Some("py")));
        assert_eq!(n, "  if x:\n      ");
        let t = "  a";
        let (n, _) = apply(t, newline(t, Selection::caret(3), "    ", Some("rs")));
        assert_eq!(n, "  a\n  ");
        assert_eq!(dedent_for("{\n        ", 10, "}", "    "), Some(6..10));
        assert_eq!(dedent_for("{\n  x ", 6, "}", "    "), None);
    }
}
