//! Source blocks and calls before export (`ob-exp.el`), without running
//! code: Kalem does not evaluate Babel code, so the buffer changes are
//! those Emacs makes when evaluation is refused (`:eval never-export`).
//!
//! - `:exports code` (the default): the results after the block are
//!   removed and the block is written again (`org-babel-exp-code`): its
//!   body loses its common indentation, Noweb references are expanded
//!   with `:noweb yes`, and every line is indented like the block, with
//!   spaces (blank lines lose their blanks).
//! - `both`: the same, keeping the results.
//! - `results`: the block goes and its results stay.
//! - `none`: the block and its results go.
//! - An inline source block goes unless `:exports code` or `both`.
//! - `#+CALL:` lines and inline calls go.

use org_syntax::SyntaxKind::*;
use org_syntax::ast::AstNode;
use org_syntax::{SyntaxNode, ast};

use crate::html::remove_indentation;

/// Header arguments after `org-babel-merge-params`: later values win,
/// and `:exports` keeps one of code, results, both and none.
#[derive(Debug, Default, Clone)]
struct Params(Vec<(String, String)>);

const EXPORTS: [&str; 4] = ["code", "results", "both", "none"];

impl Params {
    fn defaults() -> Self {
        Params(vec![
            (":exports".into(), "code".into()),
            (":noweb".into(), "no".into()),
        ])
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn set(&mut self, key: &str, value: String) {
        self.0.retain(|(k, _)| k != key);
        self.0.push((key.to_string(), value));
    }

    fn merge(&mut self, header: &str) {
        for (k, v) in parse_header(header) {
            if k == ":exports" {
                let mut words: Vec<String> = self
                    .get(":exports")
                    .map(|s| s.split_whitespace().map(String::from).collect())
                    .unwrap_or_default();
                for w in v.split_whitespace() {
                    if EXPORTS.contains(&w) {
                        words.retain(|o| !EXPORTS.contains(&o.as_str()));
                    }
                    words.retain(|o| o != w);
                    words.push(w.to_string());
                }
                self.set(":exports", words.join(" "));
            } else {
                self.set(&k, v);
            }
        }
    }

    /// The `:exports` choice.
    fn exports(&self) -> &str {
        self.get(":exports")
            .and_then(|s| s.split_whitespace().rev().find(|w| EXPORTS.contains(w)))
            .unwrap_or("code")
    }

    /// `org-babel-noweb-p`.
    fn noweb(&self, allowed: &[&str]) -> bool {
        self.get(":noweb")
            .is_some_and(|v| v.split_whitespace().any(|w| allowed.contains(&w)))
    }
}

const NOWEB_EXPORT: [&str; 2] = ["yes", "strip-tangle"];
const NOWEB_EVAL: [&str; 5] = ["yes", "no-export", "strip-export", "eval", "strip-tangle"];

/// `org-babel-read` of a value without evaluating Lisp: a string in
/// double quotes is read, anything else stays as written.
fn read_value(v: &str) -> String {
    let t = v.trim();
    if let Some(inner) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
        && !inner
            .char_indices()
            .any(|(i, c)| c == '"' && i > 0 && !inner[..i].ends_with('\\'))
    {
        let mut out = String::new();
        let mut it = inner.chars();
        while let Some(c) = it.next() {
            if c == '\\' {
                match it.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(o) => out.push(o),
                    None => {}
                }
            } else {
                out.push(c);
            }
        }
        return out;
    }
    v.to_string()
}

/// `org-babel-parse-header-arguments`: `:key value` pairs, split at a
/// colon after a blank outside brackets and strings.
fn parse_header(s: &str) -> Vec<(String, String)> {
    if s.trim().is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let (mut depth, mut in_string) = (0i32, false);
    for (i, &c) in chars.iter().enumerate() {
        if in_string {
            cur.push(c);
            if c == '"' && (i == 0 || chars[i - 1] != '\\') {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ':' if depth <= 0 && i > 0 && matches!(chars[i - 1], ' ' | '\t') => {
                cur.pop();
                parts.push(std::mem::take(&mut cur));
                cur.push(':');
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    parts.push(cur);
    let mut out = Vec::new();
    for p in parts {
        let p = p.trim_end();
        if p.trim().is_empty() {
            continue;
        }
        let t = p.trim_start();
        match t.split_once(char::is_whitespace) {
            Some((k, v)) if !v.trim().is_empty() => out.push((k.to_string(), read_value(v.trim()))),
            _ => out.push((t.to_string(), String::new())),
        }
    }
    out
}

fn start(n: &SyntaxNode) -> usize {
    usize::from(n.text_range().start())
}

fn end(n: &SyntaxNode) -> usize {
    usize::from(n.text_range().end())
}

fn in_skipped_heading(n: &SyntaxNode) -> bool {
    n.ancestors().any(|a| {
        a.kind() == HEADLINE
            && ast::AstNode::cast(a.clone())
                .is_some_and(|h: ast::Headline| h.is_commented() || h.is_archived())
    })
}

fn in_commented_heading(n: &SyntaxNode) -> bool {
    n.ancestors().any(|a| {
        a.kind() == HEADLINE
            && ast::AstNode::cast(a.clone()).is_some_and(|h: ast::Headline| h.is_commented())
    })
}

/// The value of property `name` of one entry: the plain one first, then
/// those of `name+`.
fn entry_values(props: &[(String, String)], name: &str) -> (Option<String>, Vec<String>) {
    let plus = format!("{name}+");
    let base = props
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.clone());
    let more = props
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case(&plus))
        .map(|(_, v)| v.clone())
        .collect();
    (base, more)
}

/// Property `name` at `n` with inheritance (`org-entry-get … 'inherit`):
/// the nearest entry that sets it, with the `name+` values of the entries
/// in between; the document's `#+PROPERTY` lines last.
fn inherited(n: &SyntaxNode, name: &str, global: &[(String, String)]) -> Option<String> {
    let mut entries: Vec<Vec<(String, String)>> = n
        .ancestors()
        .filter(|a| a.kind() == HEADLINE)
        .filter_map(|a| ast::AstNode::cast(a).map(|h: ast::Headline| h.properties()))
        .collect();
    // The property drawer at the top of the document.
    if let Some(doc) = n.ancestors().find(|a| a.kind() == DOCUMENT)
        && let Some(section) = doc.children().find(|c| c.kind() == SECTION)
        && let Some(first) = section.children().find(|c| c.kind().is_element())
        && let Some(d) = ast::PropertyDrawer::cast(first)
    {
        entries.push(d.properties().map(|p| (p.key(), p.value())).collect());
    }
    let mut plus: Vec<String> = Vec::new();
    for e in &entries {
        let (base, mut more) = entry_values(e, name);
        more.append(&mut plus);
        plus = more;
        if let Some(b) = base {
            let mut all = vec![b];
            all.append(&mut plus);
            return Some(all.join(" "));
        }
    }
    // The `#+PROPERTY` lines, in order: a plain one replaces what came
    // before, a `+` one adds to it (`org-keyword-properties`).
    let mut value: Option<String> = None;
    let with_plus = format!("{name}+");
    for (k, v) in global {
        if k.eq_ignore_ascii_case(name) {
            value = Some(v.clone());
        } else if k.eq_ignore_ascii_case(&with_plus) {
            value = Some(match value {
                Some(old) => format!("{old} {v}"),
                None => v.clone(),
            });
        }
    }
    match value {
        Some(b) => {
            let mut all = vec![b];
            all.append(&mut plus);
            Some(all.join(" "))
        }
        None => (!plus.is_empty()).then(|| plus.join(" ")),
    }
}

/// The `#+PROPERTY` lines as (name, value).
fn global_properties(keywords: &[(String, String)]) -> Vec<(String, String)> {
    keywords
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("PROPERTY"))
        .map(|(_, v)| {
            let v = v.trim();
            let (name, rest) = v.split_once(char::is_whitespace).unwrap_or((v, ""));
            (name.to_string(), rest.trim().to_string())
        })
        .collect()
}

/// What Babel knows about a source block (`org-babel-get-src-block-info`).
struct Block {
    node: SyntaxNode,
    lang: Option<String>,
    name: Option<String>,
    switches: Option<String>,
    parameters: Option<String>,
    params: Params,
    preserve: bool,
    /// The code without its last newline and, unless indentation is
    /// preserved, without its common indentation.
    body: String,
}

impl Block {
    fn new(node: SyntaxNode, global: &[(String, String)]) -> Option<Block> {
        let b: ast::SrcBlock = ast::AstNode::cast(node.clone())?;
        let lang = b.language();
        let switches = b.switches();
        let parameters = b.parameters();
        let mut params = Params::defaults();
        if let Some(v) = inherited(&node, "header-args", global) {
            params.merge(&v);
        }
        if let Some(l) = &lang
            && let Some(v) = inherited(&node, &format!("header-args:{l}"), global)
        {
            params.merge(&v);
        }
        if let Some(p) = &parameters {
            params.merge(p);
        }
        for k in ast::affiliated_keywords(&node).filter(|k| k.key() == "HEADER") {
            params.merge(&k.value());
        }
        let name = ast::affiliated_keywords(&node)
            .find(|k| k.key() == "NAME")
            .map(|k| k.value());
        let preserve = switches
            .as_deref()
            .is_some_and(|s| s.split_whitespace().any(|w| w == "-i"));
        let value = b.value();
        let body = value.strip_suffix('\n').unwrap_or(&value).to_string();
        let body = if preserve {
            body
        } else {
            remove_indentation(&body)
        };
        Some(Block {
            node,
            lang,
            name,
            switches,
            parameters,
            params,
            preserve,
            body,
        })
    }
}

/// `org-babel-noweb-wrap`: the next `<<ref>>` in `s` from byte `from`, as
/// (start, end, reference).
fn next_reference(s: &str, from: usize) -> Option<(usize, usize, &str)> {
    let mut at = from;
    while let Some(i) = s[at..].find("<<") {
        let open = at + i;
        let inner = open + 2;
        let first = s[inner..].chars().next();
        if first.is_some_and(|c| !matches!(c, ' ' | '\t' | '\n')) {
            // The content may be one character: `>>` right after it.
            let mut close_from = inner + first.map_or(0, char::len_utf8);
            while let Some(j) = s[close_from..].find(">>") {
                let close = close_from + j;
                let content = &s[inner..close];
                if content.contains('\n') {
                    break;
                }
                if content
                    .chars()
                    .last()
                    .is_some_and(|c| !matches!(c, ' ' | '\t'))
                {
                    return Some((open, close + 2, content));
                }
                close_from = close + 1;
            }
        }
        at = open + 1;
    }
    None
}

/// The blocks of the unchanged document, for Noweb references.
struct Library {
    blocks: Vec<Block>,
}

impl Library {
    /// `org-babel-expand-noweb-references` of `body` (of a block with
    /// `params`).
    fn expand(&self, body: &str, params: &Params, depth: usize) -> String {
        let prefix_on = params
            .get(":noweb-prefix")
            .is_none_or(|v| v != "no" && v != "nil");
        let mut out = String::new();
        let mut at = 0;
        let mut line_start = 0;
        while let Some((open, close, id)) = next_reference(body, at) {
            // Text up to the reference, and the prefix of its line.
            let seg_start = body[..open]
                .rfind('\n')
                .map_or(0, |i| i + 1)
                .max(line_start.max(at));
            out.push_str(&body[at..open]);
            let prefix = &body[seg_start..open];
            let expansion = self.reference(id, depth);
            if prefix_on {
                let lines: Vec<&str> = expansion.split(['\n', '\r']).collect();
                out.push_str(&lines.join(&format!("\n{prefix}")));
            } else {
                out.push_str(&expansion);
            }
            at = close;
            line_start = close;
        }
        out.push_str(&body[at..]);
        out
    }

    fn body_of(&self, b: &Block, depth: usize) -> String {
        if depth < 32 && b.params.noweb(&NOWEB_EVAL) {
            self.expand(&b.body, &b.params, depth + 1)
        } else {
            b.body.clone()
        }
    }

    fn reference(&self, id: &str, depth: usize) -> String {
        if id.contains('(') && id[id.find('(').unwrap_or(0)..].contains(')') {
            // The results of a call: not evaluated.
            return "nil".into();
        }
        if let Some(b) = self.blocks.iter().find(|b| {
            b.name
                .as_deref()
                .is_some_and(|n| n.to_lowercase() == id.to_lowercase())
        }) && !in_commented_heading(&b.node)
        {
            return self.body_of(b, depth);
        }
        let refs: Vec<&Block> = self
            .blocks
            .iter()
            .filter(|b| !in_commented_heading(&b.node) && b.params.get(":noweb-ref") == Some(id))
            .collect();
        let mut out = String::new();
        for (k, b) in refs.iter().enumerate() {
            if k > 0 {
                let sep = refs[k - 1].params.get(":noweb-sep").unwrap_or("\n");
                out.push_str(sep);
            }
            out.push_str(&self.body_of(b, depth));
        }
        out
    }
}

/// `org-escape-code-in-string`.
fn escape_code(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (k, line) in s.split('\n').enumerate() {
        if k > 0 {
            out.push('\n');
        }
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        let rest = line[indent..].trim_start_matches(',');
        if rest.starts_with('*') || rest.starts_with("#+") {
            out.push_str(&line[..indent]);
            out.push(',');
            out.push_str(&line[indent..]);
        } else {
            out.push_str(line);
        }
    }
    out
}

/// The indentation of `line` in columns (tabs every 8).
fn indentation(line: &str) -> usize {
    let mut w = 0;
    for c in line.chars() {
        match c {
            ' ' => w += 1,
            '\t' => w = (w / 8 + 1) * 8,
            _ => break,
        }
    }
    w
}

/// `indent-rigidly` by `ind` columns with spaces: blank lines lose their
/// blanks.
fn indent_rigidly(s: &str, ind: usize) -> String {
    s.split('\n')
        .map(|l| {
            let rest = l.trim_start_matches([' ', '\t']);
            if rest.is_empty() {
                String::new()
            } else {
                format!("{}{rest}", " ".repeat(indentation(l) + ind))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `indent-line-to`: `line` indented by `ind` spaces.
fn indent_line_to(line: &str, ind: usize) -> String {
    format!(
        "{}{}",
        " ".repeat(ind),
        line.trim_start_matches([' ', '\t'])
    )
}

/// The end of the blank lines after byte `at` (the start of the next
/// non-blank line).
fn skip_blank_lines(text: &str, at: usize) -> usize {
    let mut p = at;
    loop {
        let line_end = text[p..].find('\n').map_or(text.len(), |i| p + i + 1);
        if p >= text.len() || !text[p..line_end].trim().is_empty() {
            return p;
        }
        p = line_end;
    }
}

/// The start of the line after the last non-blank character before `at`.
fn after_last_nonblank(text: &str, at: usize) -> usize {
    let t = text[..at].trim_end_matches([' ', '\r', '\t', '\n']);
    text[t.len()..at].find('\n').map_or(at, |i| t.len() + i + 1)
}

/// `org-babel-result-end` for the result element `n` whose `#+RESULTS`
/// line ends at `after_keyword`.
fn result_end(text: &str, n: &SyntaxNode, after_keyword: usize) -> usize {
    let line_end = text[after_keyword..]
        .find('\n')
        .map_or(text.len(), |i| after_keyword + i);
    let line = text[after_keyword..line_end].trim_matches([' ', '\t']);
    if line.trim().is_empty() {
        return after_keyword;
    }
    // A link alone on its line (a file result): that line.
    if line.starts_with("[[") && line.ends_with("]]") && !line[2..line.len() - 2].contains("]]") {
        return (line_end + 1).min(text.len());
    }
    if matches!(
        n.kind(),
        DRAWER
            | EXAMPLE_BLOCK
            | EXPORT_BLOCK
            | FIXED_WIDTH
            | SPECIAL_BLOCK
            | SRC_BLOCK
            | ITEM
            | PLAIN_LIST
            | TABLE
            | LATEX_ENVIRONMENT
    ) {
        after_last_nonblank(text, end(n)).max(after_keyword)
    } else {
        after_keyword
    }
}

/// Whether a `#+RESULTS` keyword's hash part, if any, is what
/// `org-babel-result-regexp` accepts: `[HASH]` or `[(TIME) HASH]`.
fn plain_hash(k: &ast::AffiliatedKeyword) -> bool {
    let raw = k.syntax().text().to_string();
    let t = raw.trim_start();
    let Some(after) = t
        .get(9..)
        .filter(|_| t[..9].eq_ignore_ascii_case("#+results"))
    else {
        return true;
    };
    let Some(inner) = after.strip_prefix('[') else {
        return true;
    };
    let Some(close) = inner.find("]:") else {
        return false;
    };
    let inner = &inner[..close];
    let hash = match inner.strip_prefix('(') {
        Some(rest) => match rest.split_once(") ") {
            Some((time, h)) if !time.contains(')') => h,
            _ => return false,
        },
        None => inner,
    };
    !hash.is_empty()
        && hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// The region of the results of block `b` (`org-babel-remove-result`).
fn result_region(text: &str, root: &SyntaxNode, b: &Block) -> Option<(usize, usize)> {
    let is_results = |k: &ast::AffiliatedKeyword| k.key() == "RESULTS";
    let (n, kw) = if let Some(name) = &b.name {
        // A result named after the block, anywhere.
        root.descendants().find_map(|n| {
            let kw = ast::affiliated_keywords(&n)
                .find(|k| is_results(k) && k.value().trim().eq_ignore_ascii_case(name.trim()))?;
            Some((n, kw))
        })?
    } else {
        let next = b.node.next_sibling()?;
        let kw = ast::affiliated_keywords(&next)
            .find(|k| is_results(k) && k.value().trim().is_empty() && plain_hash(k))?;
        (next, kw)
    };
    let kw_start = usize::from(kw.syntax().text_range().start());
    let kw_end = usize::from(kw.syntax().text_range().end());
    Some((
        after_last_nonblank(text, kw_start),
        result_end(text, &n, kw_end),
    ))
}

/// The Org source of block `b` as `org-babel-exp-code` writes it again,
/// indented like its first line; `None` when it would not change.
fn code_again(text: &str, b: &Block, lib: &Library) -> Option<(usize, usize, String)> {
    let match_start = usize::from(ast::post_affiliated(&b.node));
    let trimmed = after_last_nonblank(text, end(&b.node));
    let last = text[..trimmed]
        .trim_end_matches([' ', '\r', '\t', '\n'])
        .len();
    let region_end = text[last..].find('\n').map_or(text.len(), |i| last + i);
    let first_line_end = text[match_start..]
        .find('\n')
        .map_or(text.len(), |i| match_start + i);
    let ind = indentation(&text[match_start..first_line_end]);
    let body = match b.params.get(":noweb") {
        Some(v) if v.split_whitespace().any(|w| w == "strip-export") => {
            let mut out = String::new();
            let mut at = 0;
            while let Some((s, e, _)) = next_reference(&b.body, at) {
                out.push_str(&b.body[at..s]);
                at = e;
            }
            out.push_str(&b.body[at..]);
            out
        }
        _ if b.params.noweb(&NOWEB_EXPORT) => lib.expand(&b.body, &b.params, 0),
        _ => b.body.clone(),
    };
    let body = escape_code(&body).replace("%name", b.name.as_deref().unwrap_or(""));
    let mut replacement = format!(
        "#+begin_src {}{}{}\n{body}\n#+end_src",
        b.lang.as_deref().unwrap_or(""),
        b.switches
            .as_deref()
            .map(|s| format!(" {s}"))
            .unwrap_or_default(),
        b.parameters
            .as_deref()
            .map(|s| format!(" {s}"))
            .unwrap_or_default(),
    );
    replacement = if b.preserve {
        let lines: Vec<&str> = replacement.split('\n').collect();
        let n = lines.len();
        lines
            .iter()
            .enumerate()
            .map(|(k, l)| {
                if k == 0 || k + 1 == n {
                    indent_line_to(l, ind)
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        indent_rigidly(&replacement, ind)
    };
    (replacement != text[match_start..region_end]).then_some((match_start, region_end, replacement))
}

/// `text` with Babel's export changes.
pub fn process(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if !lower.contains("src_")
        && !lower.contains("call_")
        && !lower.contains("#+begin_src")
        && !lower.contains("#+call:")
    {
        return text.to_string();
    }
    let parse = org_syntax::parse(text);
    let root = parse.syntax();
    let keywords = parse.keywords();
    let global = global_properties(&keywords);
    let lib = Library {
        blocks: root
            .descendants()
            .filter(|n| n.kind() == SRC_BLOCK)
            .filter_map(|n| Block::new(n, &global))
            .collect(),
    };
    // (start, end, replacement)
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for n in root.descendants() {
        if !matches!(
            n.kind(),
            INLINE_SRC_BLOCK | INLINE_BABEL_CALL | BABEL_CALL | SRC_BLOCK
        ) || in_skipped_heading(&n)
        {
            continue;
        }
        match n.kind() {
            INLINE_SRC_BLOCK => {
                let b: Option<ast::InlineSrcBlock> = ast::AstNode::cast(n.clone());
                let lang = b.as_ref().map(|b| b.language());
                let mut params = Params(vec![(":exports".into(), "results".into())]);
                if let Some(v) = inherited(&n, "header-args", &global) {
                    params.merge(&v);
                }
                if let Some(l) = &lang
                    && let Some(v) = inherited(&n, &format!("header-args:{l}"), &global)
                {
                    params.merge(&v);
                }
                if let Some(p) = b.as_ref().and_then(|b| b.parameters()) {
                    params.merge(&p);
                }
                if !matches!(params.exports(), "code" | "both") {
                    edits.push((start(&n), end(&n), String::new()));
                }
            }
            INLINE_BABEL_CALL => edits.push((start(&n), end(&n), String::new())),
            BABEL_CALL => edits.push((start(&n), skip_blank_lines(text, end(&n)), String::new())),
            SRC_BLOCK => {
                let Some(b) = lib.blocks.iter().find(|b| b.node == n) else {
                    continue;
                };
                let exports = b.params.exports();
                let result = if matches!(exports, "code" | "none") {
                    result_region(text, &root, b)
                } else {
                    None
                };
                if let Some((s, e)) = result {
                    edits.push((s, e, String::new()));
                }
                match exports {
                    "none" | "results" => {
                        // The block and the blank lines after it (after
                        // its results, when they went too).
                        let after = match result {
                            Some((s, e)) if s <= skip_blank_lines(text, end(&n)) => {
                                skip_blank_lines(text, e)
                            }
                            _ => skip_blank_lines(text, end(&n)),
                        };
                        edits.push((start(&n), after, String::new()));
                    }
                    _ => {
                        if let Some(e) = code_again(text, b, &lib) {
                            edits.push(e);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if edits.is_empty() {
        return text.to_string();
    }
    edits.sort_by_key(|e| (e.0, e.1));
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (s, e, r) in edits {
        if s < at {
            // Inside a region already removed or rewritten: Emacs never
            // gets to it.
            continue;
        }
        out.push_str(&text[at..s]);
        out.push_str(&r);
        at = e;
    }
    out.push_str(&text[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports() {
        assert_eq!(process("a src_py{1} b\n"), "a b\n");
        assert_eq!(
            process("a src_py[:exports code]{1} b\n"),
            "a src_py[:exports code]{1} b\n"
        );
        assert_eq!(process("#+CALL: f()\n\nnext\n"), "next\n");
        let t = "#+begin_src sh :exports none\nls\n#+end_src\n\nafter\n";
        assert_eq!(process(t), "after\n");
        let t = "#+begin_src sh\nls\n#+end_src\n\n#+RESULTS:\n: out\n\nafter\n";
        assert_eq!(process(t), "#+begin_src sh\nls\n#+end_src\n\nafter\n");
        let t = "#+begin_src sh :exports results\nls\n#+end_src\n\n#+RESULTS:\n: out\n\nafter\n";
        assert_eq!(process(t), "#+RESULTS:\n: out\n\nafter\n");
    }

    #[test]
    fn indentation_and_noweb() {
        let t = "  #+begin_src elisp\n    (a\n  \t(b))\n   \n  #+end_src\n";
        assert_eq!(
            process(t),
            "  #+begin_src elisp\n  (a\n          (b))\n\n  #+end_src\n"
        );
        let t = "#+name: x\n#+begin_src sh\necho x\n#+end_src\n\n#+begin_src sh :noweb yes\n# <<x>>\n#+end_src\n";
        assert!(process(t).contains("# echo x\n"), "{}", process(t));
        let t = "#+begin_src sh :noweb-ref r\na\nb\n#+end_src\n#+begin_src sh :noweb-ref r\nc\n#+end_src\n#+begin_src sh :noweb yes\n;; <<r>>\n#+end_src\n";
        assert!(process(t).contains(";; a\n;; b\n;; c\n"), "{}", process(t));
        assert_eq!(
            parse_header(":exports both :var x=\"a b\" :noweb-sep \"\\n\\n\""),
            vec![
                (":exports".into(), "both".into()),
                (":var".into(), "x=\"a b\"".into()),
                (":noweb-sep".into(), "\n\n".into())
            ]
        );
    }
}
