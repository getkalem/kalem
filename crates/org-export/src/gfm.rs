//! GitHub Flavored Markdown: a port of the `ox-gfm` package (larstvei's,
//! the one MELPA ships), which derives from `ox-md` (design §10.1): pipe
//! tables, `~~strike-through~~`, fenced source and example blocks,
//! paragraphs on one line, its own table of contents and footnote
//! section. Plain `md` stays `ox-md`'s. `tools/fetch-ox-gfm.sh` fetches
//! the package the tests compare with.

use org_syntax::SyntaxKind::{self, *};
use org_syntax::ast;
use unicode_width::UnicodeWidthStr;

use crate::export::{Backend, BackendOption, Exporter};
use crate::md::{self, Markdown};
use crate::tree::Id;

/// The GitHub Flavored Markdown back-end.
#[derive(Debug, Clone, Copy, Default)]
pub struct Gfm;

impl Gfm {
    /// `org-gfm-table`, with its rows and cells: each cell padded to its
    /// column's width in characters, a rule only as the table's second
    /// row (`|--- |--- |`, at least three dashes), and an empty header
    /// when the table has one row.
    fn table(&self, ex: &mut Exporter<'_>, id: Id) -> String {
        let rows: Vec<Id> = ex
            .table_rows(id)
            .into_iter()
            .filter(|r| !ex.info.ignore.contains(r))
            .collect();
        let cells_of = |ex: &Exporter<'_>, r: Id| -> Vec<Id> {
            ex.row_cells(r)
                .into_iter()
                .filter(|c| !ex.info.ignore.contains(c))
                .collect()
        };
        // `org-export-table-dimensions`: the first standard row's cells.
        let cols = rows
            .iter()
            .find(|&&r| !ex.rule_row_p(r))
            .map_or(0, |&r| cells_of(ex, r).len());
        let mut data: Vec<Vec<String>> = Vec::new();
        for &r in &rows {
            let cells = cells_of(ex, r);
            let row = cells
                .iter()
                .map(|&c| {
                    let children = ex.tree.children(c).to_vec();
                    ex.data_list(&children)
                })
                .collect();
            data.push(row);
        }
        // `org-gfm-table-col-width`: the longest cell, in characters.
        let width = |col: usize| {
            data.iter()
                .filter_map(|r| r.get(col))
                .map(|c| c.chars().count())
                .max()
                .unwrap_or(0)
        };
        let widths: Vec<usize> = (0..cols).map(width).collect();
        let hline = |ch: char| -> String {
            let parts: Vec<String> = widths
                .iter()
                .map(|w| ch.to_string().repeat((*w).max(3)))
                .collect();
            format!("|{} |", parts.join(" |"))
        };
        let mut out = String::new();
        if rows.len() <= 1 {
            out.push_str(&hline(' '));
            out.push('\n');
            out.push_str(&hline('-'));
            out.push('\n');
        }
        for (i, &r) in rows.iter().enumerate() {
            if ex.rule_row_p(r) {
                // In GFM, a rule is valid only as the second row.
                if i == 1 {
                    out.push_str(&hline('-'));
                    out.push('\n');
                }
                continue;
            }
            let cells = cells_of(ex, r);
            for (j, &c) in cells.iter().enumerate() {
                let d = &data[i][j];
                let w = widths.get(j).copied().unwrap_or_else(|| width(j));
                out.push_str(if crate::html::colgroup_starts(ex, c) {
                    "| "
                } else {
                    " "
                });
                out.push_str(d);
                out.push_str(&" ".repeat(w.saturating_sub(d.width())));
                out.push_str(" |");
            }
            out.push('\n');
        }
        out
    }

    /// `org-gfm-footnote-section`: "## Footnotes", then each definition
    /// after its number.
    fn footnote_section(&self, ex: &mut Exporter<'_>) -> String {
        let defs = ex.collect_footnote_definitions();
        if defs.is_empty() {
            return String::new();
        }
        let items: Vec<String> = defs
            .into_iter()
            .map(|(n, _, raw)| {
                let text = ex.data_list(&raw);
                format!(
                    "<sup><a id=\"fn.{n}\" class=\"footnum\" href=\"#fnr.{n}\">{n}</a></sup> {}\n",
                    crate::export::trim(&text)
                )
            })
            .collect();
        format!("## Footnotes\n\n{}\n", items.join("\n"))
    }

    /// `org-gfm-format-toc` for each headline the table of contents lists.
    fn toc(&self, ex: &mut Exporter<'_>) -> String {
        let depth = match ex.opt("with-toc") {
            crate::options::Value::Nil => return String::new(),
            crate::options::Value::Int(n) => Some(n),
            _ => None,
        };
        let heads = crate::html::collect_headlines(ex, depth);
        let mut lines = Vec::new();
        for h in heads {
            let ids = ex.alt_title(h);
            let title = ex.data_list(&ids);
            let indent = " ".repeat(2 * ex.true_level(h).saturating_sub(1));
            let anchor = ex
                .node_property(h, "CUSTOM_ID", false)
                .unwrap_or_else(|| ex.reference(h));
            lines.push(format!("{indent}- [{title}](#{anchor})"));
        }
        if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n\n", lines.join("\n"))
        }
    }
}

impl Backend for Gfm {
    fn name(&self) -> &'static str {
        "gfm"
    }

    fn parents(&self) -> &'static [&'static str] {
        &["md", "html"]
    }

    fn has_transcoder(&self, kind: SyntaxKind) -> bool {
        Markdown.has_transcoder(kind)
    }

    fn options(&self) -> Vec<BackendOption> {
        Markdown.options()
    }

    fn filter_parse_tree(&self, ex: &mut Exporter<'_>) {
        Markdown.filter_parse_tree(ex);
    }

    fn plain_text(&self, ex: &mut Exporter<'_>, text: &str) -> String {
        Markdown.plain_text(ex, text)
    }

    /// `org-gfm-inner-template`: the table of contents, the body and the
    /// footnote section, trimmed.
    fn inner_template(&self, ex: &mut Exporter<'_>, body: String) -> String {
        let toc = self.toc(ex);
        let foot = self.footnote_section(ex);
        crate::export::trim(&format!("{toc}{body}\n{foot}")).to_string()
    }

    fn template(&self, ex: &mut Exporter<'_>, body: String) -> String {
        Markdown.template(ex, body)
    }

    fn filter_final_output(&self, ex: &mut Exporter<'_>, out: String) -> String {
        Markdown.filter_final_output(ex, out)
    }

    fn transcode(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> Option<String> {
        let kind = ex.tree.kind(id)?;
        Some(match kind {
            TABLE
                if ex
                    .syntax(id)
                    .and_then(|s| <ast::Table as ast::AstNode>::cast(s.clone()))
                    .is_some_and(|t| t.table_type() == ast::TableType::Org) =>
            {
                self.table(ex, id)
            }
            TABLE_CELL => contents.unwrap_or_default(),
            STRIKE_THROUGH => format!("~~{}~~", contents.unwrap_or_default()),
            // `org-gfm-src-block`, and `org-gfm-example-block` its alias.
            SRC_BLOCK | EXAMPLE_BLOCK => {
                let lang = ex
                    .syntax(id)
                    .and_then(|s| <ast::SrcBlock as ast::AstNode>::cast(s.clone()))
                    .and_then(|b| b.language())
                    .unwrap_or_default();
                let code = md::format_code_default(ex, id);
                format!("```{lang}\n{code}```")
            }
            // `org-gfm-paragraph`: its words on one line, unless `\n:t`.
            PARAGRAPH => {
                let c = contents.unwrap_or_default();
                let c = if ex.flag("preserve-breaks") {
                    c
                } else {
                    let words: Vec<&str> = c
                        .split([' ', '\x0c', '\t', '\n', '\r', '\x0b'])
                        .filter(|w| !w.is_empty())
                        .collect();
                    format!("{}\n", words.join(" "))
                };
                let first = ex.tree.children(id).first().copied();
                if first
                    .is_some_and(|f| ex.tree.is_text(f) && ex.tree.nodes[f].text.starts_with('#'))
                    && c.starts_with('#')
                {
                    format!("\\{c}")
                } else {
                    c
                }
            }
            EXPORT_SNIPPET => {
                let s: ast::ExportSnippet =
                    ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()))?;
                if s.backend() == "gfm" {
                    s.value()
                } else {
                    return Markdown.transcode(ex, id, contents);
                }
            }
            EXPORT_BLOCK => {
                let b: ast::ExportBlock =
                    ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()))?;
                if b.backend().as_deref() == Some("GFM") {
                    crate::html::remove_indentation(&b.value())
                } else {
                    return Markdown.transcode(ex, id, contents);
                }
            }
            _ => return Markdown.transcode(ex, id, contents),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gfm(text: &str) -> String {
        crate::export(
            &format!("#+OPTIONS: toc:nil\n{text}"),
            &Gfm,
            &crate::Settings {
                body_only: true,
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn tables() {
        // As ox-gfm writes them: cells padded to the column's width, the
        // rule's dashes at least three and without alignment.
        let out = gfm("| Name | n |\n|------+---|\n| *a* | 1 |\n| b\\vert{}c | 22 |\n");
        assert_eq!(
            out.trim_end(),
            "| Name     | n  |\n|-------- |--- |\n| **a**    | 1  |\n| b&vert;c | 22 |"
        );
        // One row: an empty header first. Without a rule, none.
        let out = gfm("| x | y |\n");
        assert_eq!(out.trim_end(), "|    |    |\n|--- |--- |\n| x | y |");
        let out = gfm("| x | y |\n| z | w |\n");
        assert_eq!(out.trim_end(), "| x | y |\n| z | w |");
    }

    #[test]
    fn paragraphs_toc_and_footnotes() {
        let out = crate::export(
            "* One\nA line\nand another.[fn:1]\n** Two\n\n[fn:1] Note.\n",
            &Gfm,
            &crate::Settings {
                body_only: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(out.starts_with("- [One](#"), "{out}");
        assert!(out.contains("\n  - [Two](#"), "{out}");
        assert!(out.contains("A line and another.<sup>"), "{out}");
        assert!(out.ends_with("## Footnotes\n\n<sup><a id=\"fn.1\" class=\"footnum\" href=\"#fnr.1\">1</a></sup> Note."), "{out}");
    }

    #[test]
    fn code_and_strike() {
        let out = gfm("+gone+\n\n#+begin_src rust\nfn main() {}\n#+end_src\n");
        assert!(out.contains("~~gone~~"), "{out}");
        assert!(out.contains("```rust\nfn main() {}\n```"), "{out}");
        let out = gfm("#+begin_example\nplain\n#+end_example\n");
        assert!(out.contains("```\nplain\n```"), "{out}");
    }
}
