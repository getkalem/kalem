//! A block's results written as `org-babel-insert-result` writes them,
//! where `org-babel-where-is-src-block-result` puts them: over the
//! results already there, else under a new `#+RESULTS:` line below the
//! block.

use std::ops::Range;

use org_syntax::SyntaxKind::*;
use org_syntax::ast::AstNode;
use org_syntax::{SyntaxNode, ast};

use super::table;
use super::value::Value;

/// How a result is written: `:results`' words, `:wrap`'s value (an empty
/// one when `:wrap` has none), the block's language (for `code`), and
/// `:results_switches`.
#[derive(Debug, Clone, Default)]
pub struct Format {
    pub params: Vec<String>,
    pub wrap: Option<String>,
    pub lang: String,
    pub switches: Option<String>,
}

impl Format {
    fn has(&self, word: &str) -> bool {
        self.params.iter().any(|p| p == word)
    }
}

/// `org-escape-code-in-region`: a comma before a line's `*` or `#+` (or
/// the commas before them).
fn escape(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|l| {
            let lead = l.len() - l.trim_start_matches([' ', '\t']).len();
            let rest = l[lead..].trim_start_matches(',');
            if rest.starts_with('*') || rest.starts_with("#+") {
                format!("{},{}", &l[..lead], &l[lead..])
            } else {
                l.to_string()
            }
        })
        .collect()
}

/// `org-babel-examplify-region`: fewer than ten lines each after `: `,
/// more in an example block.
fn examplify(body: &str, switches: Option<&str>) -> String {
    let size = body.lines().count();
    if size == 0 {
        String::new()
    } else if size < 10 {
        body.lines().map(|l| format!(": {l}\n")).collect()
    } else {
        format!(
            "#+begin_example{}\n{}#+end_example\n",
            switches.map(|s| format!(" {s}")).unwrap_or_default(),
            escape(body)
        )
    }
}

/// `org-list-to-org` of a result's items as `- ` items.
fn list_text(result: &Value) -> String {
    let items: Vec<String> = match result {
        Value::List(v) => v
            .iter()
            .map(|e| match e {
                Value::Str(s) => s.clone(),
                Value::List(parts) => parts
                    .iter()
                    .map(|x| match x {
                        Value::Str(s) => s.clone(),
                        o => o.prin1(),
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
                o => o.prin1(),
            })
            .collect(),
        Value::Str(s) => s
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect(),
        o => vec![o.prin1()],
    };
    let mut out: String = items.iter().map(|i| format!("- {i}\n")).collect();
    // `org-trim`, then a line feed.
    let trimmed = out.trim().to_string();
    out = format!("{trimmed}\n");
    out
}

/// The text `org-babel-insert-result` writes under the `#+RESULTS:` line
/// for `result` (unindented), every line ending with a line feed.
pub fn text(result: &Value, f: &Format) -> String {
    // A value neither a string nor a list is written as `%S` writes it.
    let result = match result {
        Value::Str(_) | Value::List(_) => result.clone(),
        other => Value::Str(other.prin1()),
    };
    let tabulable = table::tabulable(&result);
    let mut body = match &result {
        v if v.is_nil() => String::new(),
        v if f.has("list") => list_text(v),
        v if tabulable => table::org_table(v),
        Value::Str(s) => {
            let mut s = s.clone();
            // Non-inline results end with a line feed.
            if !s.trim().is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            s
        }
        other => format!("{}\n", other.princ()),
    };
    let wrap = |body: &str, start: &str, finish: &str, no_escape: bool| {
        let inner = if no_escape {
            body.to_string()
        } else {
            escape(body)
        };
        format!("{start}\n{inner}{finish}\n")
    };
    let switches = f.switches.as_deref();
    body = if let Some(full) = &f.wrap {
        let full = if full.trim().is_empty() {
            "results".to_string()
        } else {
            full.trim().to_string()
        };
        let kind = full
            .split_whitespace()
            .next()
            .unwrap_or("results")
            .to_string();
        let verbatim = ["export", "example", "src"]
            .iter()
            .any(|v| kind.eq_ignore_ascii_case(v));
        wrap(
            &body,
            &format!("#+begin_{full}"),
            &format!("#+end_{kind}"),
            !verbatim,
        )
    } else if f.has("html") {
        wrap(&body, "#+begin_export html", "#+end_export", false)
    } else if f.has("latex") {
        wrap(&body, "#+begin_export latex", "#+end_export", false)
    } else if f.has("org") {
        wrap(&body, "#+begin_src org", "#+end_src", false)
    } else if f.has("code") {
        let lang = if f.lang.is_empty() { "none" } else { &f.lang };
        let sw = switches.map(|s| format!(" {s}")).unwrap_or_default();
        wrap(
            &body,
            &format!("#+begin_src {lang}{sw}"),
            "#+end_src",
            false,
        )
    } else if f.has("raw") {
        body
    } else if f.has("drawer") || f.has("wrap") {
        wrap(&body, ":results:", ":end:", true)
    } else if !tabulable && !f.has("file") {
        examplify(&body, switches)
    } else {
        body
    };
    body
}

/// `indent-rigidly`: every line but a blank one `indent` columns further.
fn indented(text: &str, indent: usize) -> String {
    if indent == 0 {
        return text.to_string();
    }
    let pad = " ".repeat(indent);
    text.split_inclusive('\n')
        .map(|l| {
            if l.trim().is_empty() {
                l.to_string()
            } else {
                format!("{pad}{l}")
            }
        })
        .collect()
}

fn start(n: &SyntaxNode) -> usize {
    usize::from(n.text_range().start())
}

fn end(n: &SyntaxNode) -> usize {
    usize::from(n.text_range().end())
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

fn indentation_at(text: &str, line: usize) -> usize {
    text[line..].len() - text[line..].trim_start_matches([' ', '\t']).len()
}

/// Whether `#+RESULTS` keyword `k` names no block and has a plain hash
/// part or none: what `org-babel-result-regexp` with nothing after the
/// colon takes.
fn anonymous(k: &ast::AffiliatedKeyword) -> bool {
    k.key() == "RESULTS" && k.value().trim().is_empty()
}

/// The `#+RESULTS` line of the results of block `block` (named `name`),
/// and the node carrying it: by name anywhere (`org-babel-find-named-result`),
/// else an anonymous one on the element right below the block.
fn existing(
    text: &str,
    root: &SyntaxNode,
    block: &SyntaxNode,
    name: Option<&str>,
) -> Option<(usize, usize, SyntaxNode)> {
    if let Some(name) = name {
        return root.descendants().find_map(|n| {
            let k = ast::affiliated_keywords(&n).find(|k| {
                k.key() == "RESULTS" && k.value().trim().eq_ignore_ascii_case(name.trim())
            })?;
            Some((start(k.syntax()), end(k.syntax()), n))
        });
    }
    let next = block.next_sibling()?;
    let k = ast::affiliated_keywords(&next).find(anonymous)?;
    let _ = text;
    Some((start(k.syntax()), end(k.syntax()), next))
}

/// The edit writing `body` (from [`text`]) as the results of block
/// `block` of `text` (whose syntax tree is `root`), named `name`: the
/// range replaced and its new text. `params` are `:results`' words:
/// `replace` takes the old results' place, `append` and `prepend` keep
/// them.
pub fn edit(
    text: &str,
    root: &SyntaxNode,
    block: &SyntaxNode,
    name: Option<&str>,
    body: &str,
    params: &[String],
) -> (Range<usize>, String) {
    let has = |w: &str| params.iter().any(|p| p == w);
    if let Some((kw_start, kw_end, node)) = existing(text, root, block, name) {
        let line = line_start(text, kw_start);
        let indent = indentation_at(text, line);
        // The keyword's node holds its line feed: the results start right
        // after it; a keyword on the last line, without one, gets one.
        let (beg, feed) = if text[..kw_end].ends_with('\n') {
            (kw_end, "")
        } else {
            (kw_end, "\n")
        };
        let old_end = org_export::babel::results_end(text, &node, kw_end).max(beg);
        let body = format!("{feed}{}", indented(body, indent));
        return if has("append") {
            (old_end..old_end, body)
        } else if has("prepend") {
            (beg..beg, body)
        } else {
            (beg..old_end, body)
        };
    }
    // A new `#+RESULTS:` line: below the block's last line, after a blank
    // line, indented as the block in a list item; a blank line after the
    // results unless one follows already.
    let last = text[..end(block)].trim_end_matches([' ', '\t', '\n']).len();
    let (at, mut ins) = match text[last..].find('\n') {
        Some(i) => (last + i + 1, String::new()),
        None => (text.len(), String::from("\n")),
    };
    let indent = if block.ancestors().any(|a| a.kind() == ITEM) {
        indentation_at(
            text,
            line_start(
                text,
                start(block).max(usize::from(ast::post_affiliated(block))),
            ),
        )
    } else {
        0
    };
    ins.push('\n');
    ins.push_str(&" ".repeat(indent));
    ins.push_str("#+RESULTS:");
    if let Some(n) = name {
        ins.push(' ');
        ins.push_str(n);
    }
    ins.push('\n');
    ins.push_str(&indented(body, indent));
    let next_line = &text[at..text[at..].find('\n').map_or(text.len(), |i| at + i)];
    if at < text.len() && !next_line.trim().is_empty() {
        ins.push('\n');
    }
    (at..at, ins)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(params: &[&str]) -> Format {
        Format {
            params: params.iter().map(|p| (*p).to_string()).collect(),
            lang: "sh".into(),
            ..Format::default()
        }
    }

    #[test]
    fn texts_as_emacs_writes_them() {
        let s = |t: &str| Value::Str(t.into());
        assert_eq!(text(&s("hello"), &f(&["replace"])), ": hello\n");
        let twelve: String = (1..=12).map(|i| format!("{i}\n")).collect();
        assert_eq!(
            text(&s(&twelve), &f(&["replace", "output"])),
            format!("#+begin_example\n{twelve}#+end_example\n")
        );
        assert_eq!(text(&s("*bold* text\n"), &f(&["raw"])), "*bold* text\n");
        assert_eq!(
            text(&s("in a drawer\n"), &f(&["output", "drawer"])),
            ":results:\nin a drawer\n:end:\n"
        );
        assert_eq!(
            text(&s("<b>b</b>\n"), &f(&["output", "html"])),
            "#+begin_export html\n<b>b</b>\n#+end_export\n"
        );
        assert_eq!(
            text(&s("x=1\n"), &f(&["output", "code"])),
            "#+begin_src sh\nx=1\n#+end_src\n"
        );
        let mut w = f(&["output"]);
        w.wrap = Some("example".into());
        assert_eq!(
            text(&s("* a\n"), &w),
            "#+begin_example\n,* a\n#+end_example\n"
        );
        assert_eq!(text(&Value::Int(2), &f(&["replace"])), ": 2\n");
        assert_eq!(
            text(
                &Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
                &f(&["value", "list"])
            ),
            "- 1\n- 2\n- 3\n"
        );
        assert_eq!(text(&Value::nil(), &f(&["replace"])), "");
    }

    fn placed(doc: &str, body: &str, params: &[&str]) -> String {
        let parse = org_syntax::parse(doc);
        let root = parse.syntax();
        let block = root.descendants().find(|n| n.kind() == SRC_BLOCK).unwrap();
        let name = ast::affiliated_keywords(&block)
            .find(|k| k.key() == "NAME")
            .map(|k| k.value());
        let params: Vec<String> = params.iter().map(|p| (*p).to_string()).collect();
        let (r, t) = edit(doc, &root, &block, name.as_deref(), body, &params);
        format!("{}{}{}", &doc[..r.start], t, &doc[r.end..])
    }

    #[test]
    fn placed_where_emacs_puts_them() {
        let doc = "#+begin_src sh\necho hello\n#+end_src\n\n* Next\n";
        assert_eq!(
            placed(doc, ": hello\n", &["replace"]),
            "#+begin_src sh\necho hello\n#+end_src\n\n#+RESULTS:\n: hello\n\n* Next\n"
        );
        // At the end of the file.
        assert_eq!(
            placed("#+begin_src sh\nx\n#+end_src", ": 1\n", &["replace"]),
            "#+begin_src sh\nx\n#+end_src\n\n#+RESULTS:\n: 1\n"
        );
        // Replacing the results there.
        let old = "#+begin_src sh\nx\n#+end_src\n\n#+RESULTS:\n: old\n\nText.\n";
        assert_eq!(
            placed(old, ": new\n", &["replace"]),
            "#+begin_src sh\nx\n#+end_src\n\n#+RESULTS:\n: new\n\nText.\n"
        );
        // Named.
        assert_eq!(
            placed(
                "#+name: n\n#+begin_src sh\nx\n#+end_src\n",
                ": 1\n",
                &["replace"]
            ),
            "#+name: n\n#+begin_src sh\nx\n#+end_src\n\n#+RESULTS: n\n: 1\n"
        );
        // In a list item: indented as the block.
        assert_eq!(
            placed(
                "- item\n  #+begin_src sh\n  x\n  #+end_src\n",
                ": 1\n",
                &["replace"]
            ),
            "- item\n  #+begin_src sh\n  x\n  #+end_src\n\n  #+RESULTS:\n  : 1\n"
        );
    }
}
