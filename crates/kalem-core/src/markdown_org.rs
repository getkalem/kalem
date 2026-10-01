//! Markdown to Org without pandoc (T2.7c.7): comrak's tree written as
//! Org, element by element, as Org's own Markdown exporter writes the
//! other way. Headings become headlines, emphasis `/…/`, strong `*…*`,
//! code `~…~`, strike-through `+…+`, links `[[url][text]]`, task items
//! `[ ]`, quotes, code blocks and raw HTML blocks, tables with a rule
//! under the header, footnotes `[fn:name]`, math `\(…\)` and `\[…\]`,
//! and the front matter's `title`, `author`, `date` and the rest as
//! `#+KEY:` lines.

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
                self.line(
                    "",
                    &format!("{} {}", "*".repeat(h.level as usize), title.trim()),
                );
                self.blank();
            }
            V::Paragraph => {
                let p = self.inlines(node);
                for l in p.lines() {
                    self.line(indent, l);
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
            V::Text(t) => t.to_string(),
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
            V::FootnoteReference(f) => format!("[fn:{}]", f.name),
            V::Math(m) if m.display_math => format!("\\[{}\\]", m.literal),
            V::Math(m) => format!("\\({}\\)", m.literal),
            V::HtmlInline(h) => format!("@@html:{h}@@"),
            _ => self.inlines(node),
        }
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
    fn code_that_looks_like_org_is_quoted() {
        let org = to_org("```\n* not a heading\n#+not: a keyword\n```\n");
        assert_eq!(
            org,
            "#+begin_example\n,* not a heading\n,#+not: a keyword\n#+end_example\n"
        );
    }
}
