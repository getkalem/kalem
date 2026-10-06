//! Markdown to Org without pandoc (T2.7c.7): comrak's tree written as
//! Org, element by element, as Org's own Markdown exporter writes the
//! other way. Headings become headlines, emphasis `/…/`, strong `*…*`,
//! code `~…~`, strike-through `+…+`, links `[[url][text]]`, task items
//! `[ ]`, quotes, code blocks and raw HTML blocks, tables with a rule
//! under the header, footnotes `[fn:name]`, math `\(…\)` and `\[…\]`,
//! and the front matter's `title`, `author`, `date` and the rest as
//! `#+KEY:` lines. Text that Org would read as markup (`*x*` written
//! `\*x\*`, `/x/`, `=x=`) gets a zero-width space after its opening
//! character, and a line Org would read as structure (`\# x`, a `|` that
//! makes no table) one before it, as Org's manual advises (Emacs reads
//! them so); wiki links become links to the page's file.

use comrak::nodes::{AstNode, ListDelimType, ListType, NodeValue as V};

/// The Markdown `text` as an Org document.
pub fn to_org(text: &str) -> String {
    let arena = comrak::Arena::new();
    let mut o = comrak::Options::default();
    o.extension.table = true;
    o.extension.strikethrough = true;
    o.extension.autolink = true;
    o.extension.tasklist = true;
    o.extension.footnotes = true;
    o.extension.math_dollars = true;
    o.extension.front_matter_delimiter = Some("---".to_string());
    o.extension.wikilinks_title_after_pipe = true;
    let root = comrak::parse_document(&arena, text, &o);
    let mut w = Writer::default();
    w.blocks(root, "");
    let mut out = w.out.trim_end().to_string();
    out.push('\n');
    out
}

#[derive(Default)]
struct Writer {
    out: String,
}

impl Writer {
    fn line(&mut self, indent: &str, s: &str) {
        self.out.push_str(indent);
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn blank(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    /// The blocks under `node`, each line after `indent`.
    fn blocks<'a>(&mut self, node: &'a AstNode<'a>, indent: &str) {
        for c in node.children() {
            self.block(c, indent);
        }
    }

    fn block<'a>(&mut self, node: &'a AstNode<'a>, indent: &str) {
        let value = node.data().value.clone();
        match value {
            V::FrontMatter(fm) => {
                for l in fm.lines() {
                    let l = l.trim();
                    if l == "---" || l.is_empty() {
                        continue;
                    }
                    if let Some((k, v)) = l.split_once(':') {
                        let v = v.trim().trim_matches(['"', '\'']);
                        self.line("", &format!("#+{}: {v}", k.trim().to_lowercase()));
                    }
                }
                self.blank();
            }
            V::Heading(h) => {
                let title = self.inlines(node);
                let top = node
                    .parent()
                    .is_some_and(|p| matches!(p.data().value, V::Document));
                if top {
                    self.line(
                        "",
                        &format!("{} {}", "*".repeat(h.level as usize), title.trim()),
                    );
                } else {
                    // In a quote or a list a headline would end it: bold.
                    self.line(indent, &escape_line_start(&format!("*{}*", title.trim())));
                }
                self.blank();
            }
            V::Paragraph => {
                let p = self.inlines(node);
                for l in p.lines() {
                    self.line(indent, &escape_line_start(l));
                }
                self.blank();
            }
            V::BlockQuote | V::MultilineBlockQuote(_) | V::Alert(_) => {
                self.line(indent, "#+begin_quote");
                self.blocks(node, indent);
                if self.out.ends_with("\n\n") {
                    self.out.pop();
                }
                self.line(indent, "#+end_quote");
                self.blank();
            }
            V::List(l) => {
                for (n, item) in (l.start.max(1)..).zip(node.children()) {
                    let bullet = match l.list_type {
                        ListType::Bullet => "- ".to_string(),
                        ListType::Ordered => {
                            let d = if l.delimiter == ListDelimType::Paren {
                                ")"
                            } else {
                                "."
                            };
                            format!("{n}{d} ")
                        }
                    };
                    let boxed = match &item.data().value {
                        V::TaskItem(t) => Some(t.symbol.is_some()),
                        _ => None,
                    };
                    let mut head = bullet.clone();
                    if let Some(c) = boxed {
                        head.push_str(if c { "[X] " } else { "[ ] " });
                    }
                    let inner = format!("{indent}{}", " ".repeat(bullet.len()));
                    let mut first = true;
                    for c in item.children() {
                        let start = self.out.len();
                        self.block(c, &inner);
                        if l.tight {
                            while self.out.ends_with("\n\n") {
                                self.out.pop();
                            }
                        }
                        if first {
                            // The first line takes the bullet.
                            let written = self.out.split_off(start);
                            let rest = written.strip_prefix(inner.as_str()).unwrap_or(&written);
                            self.out.push_str(indent);
                            self.out.push_str(&head);
                            self.out.push_str(rest);
                            first = false;
                        }
                    }
                    if first {
                        self.line(indent, head.trim_end());
                    }
                    // A tight list: no blank lines between items.
                    if l.tight {
                        while self.out.ends_with("\n\n") {
                            self.out.pop();
                        }
                    }
                }
                self.blank();
            }
            V::CodeBlock(c) => {
                let lang = c.info.split_whitespace().next().unwrap_or("");
                if lang.is_empty() {
                    self.line(indent, "#+begin_example");
                } else {
                    self.line(indent, &format!("#+begin_src {lang}"));
                }
                for l in c.literal.lines() {
                    // A line that would end the block or read as a keyword
                    // gets Org's comma.
                    let quoted = if l.starts_with('*') || l.trim_start().starts_with("#+") {
                        format!(",{l}")
                    } else {
                        l.to_string()
                    };
                    self.line(indent, &quoted);
                }
                self.line(
                    indent,
                    if lang.is_empty() {
                        "#+end_example"
                    } else {
                        "#+end_src"
                    },
                );
                self.blank();
            }
            V::HtmlBlock(h) => {
                self.line(indent, "#+begin_export html");
                for l in h.literal.lines() {
                    self.line(indent, l);
                }
                self.line(indent, "#+end_export");
                self.blank();
            }
            V::ThematicBreak => {
                self.line(indent, "-----");
                self.blank();
            }
            V::Table(_) => {
                for (i, row) in node.children().enumerate() {
                    let cells: Vec<String> = row
                        .children()
                        .map(|c| self.inlines(c).trim().replace('|', "\\vert{}"))
                        .collect();
                    self.line(indent, &format!("| {} |", cells.join(" | ")));
                    if i == 0 {
                        let rule = vec!["---"; cells.len().max(1)].join("+");
                        self.line(indent, &format!("|{rule}|"));
                    }
                }
                self.blank();
            }
            V::FootnoteDefinition(f) => {
                let start = self.out.len();
                self.blocks(node, "");
                let body = self.out.split_off(start);
                self.line("", &format!("[fn:{}] {}", f.name, body.trim()));
                self.blank();
            }
            V::Math(m) if m.display_math => {
                self.line(indent, &format!("\\[{}\\]", m.literal.trim()));
                self.blank();
            }
            _ => {
                let s = self.inlines(node);
                if !s.trim().is_empty() {
                    self.line(indent, s.trim());
                }
            }
        }
    }

    /// The inline content of `node` as Org.
    fn inlines<'a>(&mut self, node: &'a AstNode<'a>) -> String {
        let mut s = String::new();
        for c in node.children() {
            s.push_str(&self.inline(c));
        }
        s
    }

    fn inline<'a>(&mut self, node: &'a AstNode<'a>) -> String {
        let value = node.data().value.clone();
        match value {
            V::Text(t) => escape_markup(&t),
            V::SoftBreak => "\n".into(),
            V::LineBreak => "\\\\\n".into(),
            V::Code(c) => {
                let mark = if c.literal.contains('~') { '=' } else { '~' };
                format!("{mark}{}{mark}", c.literal)
            }
            V::Emph => format!("/{}/", self.inlines(node)),
            V::Strong => format!("*{}*", self.inlines(node)),
            V::Strikethrough => format!("+{}+", self.inlines(node)),
            V::Link(l) => {
                let t = self.inlines(node);
                if t.is_empty() || t == l.url {
                    format!("[[{}]]", l.url)
                } else {
                    format!("[[{}][{t}]]", l.url)
                }
            }
            V::Image(l) => format!("[[{}]]", l.url),
            // `[[Page]]` and `[[Page|title]]`: the page's Markdown file.
            V::WikiLink(l) => {
                let t = self.inlines(node);
                let file = if std::path::Path::new(&l.url).extension().is_some() {
                    l.url.clone()
                } else {
                    format!("{}.md", l.url)
                };
                if t.is_empty() || t == l.url {
                    format!("[[file:{file}][{}]]", l.url)
                } else {
                    format!("[[file:{file}][{t}]]")
                }
            }
            V::FootnoteReference(f) => format!("[fn:{}]", f.name),
            V::Math(m) if m.display_math => format!("\\[{}\\]", m.literal),
            V::Math(m) => format!("\\({}\\)", m.literal),
            V::HtmlInline(h) => format!("@@html:{h}@@"),
            _ => self.inlines(node),
        }
    }
}

/// The zero-width space Org's manual puts beside a character that would
/// otherwise be read as markup.
const ZWSP: char = '\u{200b}';

/// `text` with a zero-width space after each `*`, `/`, `_`, `=`, `~` or
/// `+` that would open Org markup: after a blank, the start or one of
/// `-({'"`, before a character that is not blank, with a closing one
/// after it in `text`. (Before it, Emacs reads the space as a blank and
/// the markup stays.)
fn escape_markup(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let marker = |c: char| matches!(c, '*' | '/' | '_' | '=' | '~' | '+');
    let pre = |c: Option<&char>| c.is_none_or(|c| c.is_whitespace() || "-({'\"".contains(*c));
    let post =
        |c: Option<&char>| c.is_none_or(|c| c.is_whitespace() || "-.,;:!?')}[\"\\".contains(*c));
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if marker(c)
            && pre(i.checked_sub(1).and_then(|j| chars.get(j)))
            && chars.get(i + 1).is_some_and(|n| !n.is_whitespace())
            && (i + 2..chars.len())
                .any(|j| chars[j] == c && !chars[j - 1].is_whitespace() && post(chars.get(j + 1)))
        {
            out.push(c);
            out.push(ZWSP);
            continue;
        }
        out.push(c);
    }
    out
}

/// Line `l` of a paragraph, with a zero-width space before it when Org
/// would read its start as something else: a comment or keyword (`#`),
/// a headline or a list item (`*`, `-`, `+`, `1.`), a table (`|`), a
/// fixed-width line (`:`), a rule, a drawer or a footnote definition.
fn escape_line_start(l: &str) -> String {
    let t = l.trim_start();
    let word = t.split(char::is_whitespace).next().unwrap_or("");
    let ordered = word.len() > 1
        && (word.ends_with('.') || word.ends_with(')'))
        && word[..word.len() - 1].chars().all(|c| c.is_ascii_digit())
        && t.len() > word.len();
    let structural = matches!(word, "#" | "*" | "-" | "+" | ":")
        || t.starts_with("#+")
        || t.starts_with('|')
        || t.starts_with("-----")
        || t.starts_with("[fn:")
        || (t.starts_with(':') && t.len() > 1 && t[1..].find(':').is_some())
        || ordered;
    if structural {
        let indent = &l[..l.len() - t.len()];
        format!("{indent}{ZWSP}{t}")
    } else {
        l.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_reads_as_org() {
        let md = "---\ntitle: Notes\nauthor: Ada\n---\n\n# One\n\nSome *em*, **bold**, `code`, ~~gone~~ and [a link](http://x.org).\n\n## Two\n\n- a\n- [x] done\n  - nested\n\n1. first\n2. second\n\n> quoted\n\n```rust\nlet x = 1;\n```\n\n| a | b |\n|---|--:|\n| 1 | 2 |\n\n---\n\nRef[^n] and $x^2$.\n\n[^n]: The note.\n";
        let org = to_org(md);
        let expect = "#+title: Notes\n#+author: Ada\n\n* One\n\nSome /em/, *bold*, ~code~, +gone+ and [[http://x.org][a link]].\n\n** Two\n\n- a\n- [X] done\n  - nested\n\n1. first\n2. second\n\n#+begin_quote\nquoted\n#+end_quote\n\n#+begin_src rust\nlet x = 1;\n#+end_src\n\n| a | b |\n|---+---|\n| 1 | 2 |\n\n-----\n\nRef[fn:n] and \\(x^2\\).\n\n[fn:n] The note.\n";
        assert_eq!(org, expect, "\n{org}");
        // The Org reads back as a document with those headlines.
        let doc = org_model::Document::new(org_syntax::parse(&org));
        assert_eq!(doc.outline().entries.len(), 2);
    }

    #[test]
    fn text_that_org_would_read_as_markup_stays_text() {
        let org = to_org(
            "\\*not bold\\* and /not italic/ and =not verbatim= but a/b and 5 * 3.\n\n\\# not a comment\n\n\\- not an item\n\n| not a table\n\n[[Page|The page]] and [[Other]].\n\n> # In a quote\n> text\n",
        );
        let z = ZWSP;
        assert_eq!(
            org,
            format!(
                "*{z}not bold* and /{z}not italic/ and ={z}not verbatim= but a/b and 5 * 3.\n\n{z}# not a comment\n\n{z}- not an item\n\n{z}| not a table\n\n[[file:Page.md][The page]] and [[file:Other.md][Other]].\n\n#+begin_quote\n*In a quote*\n\ntext\n#+end_quote\n"
            ),
            "\n{org}"
        );
        // Read back: one paragraph of plain text each, no headline, no
        // comment, no list, no table; the quote whole, its heading bold.
        let parse = org_syntax::parse(&org);
        let kinds: Vec<_> = parse
            .syntax()
            .descendants()
            .map(|n| n.kind())
            .filter(|k| {
                use org_syntax::SyntaxKind::*;
                matches!(
                    k,
                    HEADLINE
                        | COMMENT
                        | PLAIN_LIST
                        | TABLE
                        | BOLD
                        | ITALIC
                        | VERBATIM
                        | QUOTE_BLOCK
                )
            })
            .collect();
        assert_eq!(
            kinds,
            [
                org_syntax::SyntaxKind::QUOTE_BLOCK,
                org_syntax::SyntaxKind::BOLD
            ]
        );
    }

    #[test]
    fn code_that_looks_like_org_is_quoted() {
        let org = to_org("```\n* not a heading\n#+not: a keyword\n```\n");
        assert_eq!(
            org,
            "#+begin_example\n,* not a heading\n,#+not: a keyword\n#+end_example\n"
        );
    }
}
