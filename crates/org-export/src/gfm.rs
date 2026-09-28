//! GitHub Flavored Markdown: the Markdown back-end with pipe tables,
//! `~~strike-through~~` and fenced code blocks, as the `ox-gfm` package
//! adds them to `ox-md` (design §10.1). Plain `md` stays `ox-md`'s.

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
    /// A table as a pipe table: the first row group is the header when
    /// the table has one, else the header is empty; other rules go, since
    /// GFM has one rule only. Columns are padded to their widths.
    fn table(&self, ex: &mut Exporter<'_>, id: Id) -> String {
        let rows: Vec<Id> = ex
            .table_rows(id)
            .into_iter()
            .filter(|r| !ex.rule_row_p(*r))
            .collect();
        let mut cells: Vec<Vec<String>> = Vec::new();
        let mut aligns: Vec<&'static str> = Vec::new();
        for &r in &rows {
            let ids: Vec<Id> = ex
                .row_cells(r)
                .into_iter()
                .filter(|c| !ex.info.ignore.contains(c))
                .collect();
            if aligns.is_empty() {
                aligns = ids.iter().map(|&c| ex.cell_alignment(c)).collect();
            }
            let row = ids
                .iter()
                .map(|&c| ex.data(c).trim().replace('\n', " ").replace('|', "\\|"))
                .collect();
            cells.push(row);
        }
        let columns = cells.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return String::new();
        }
        let header = match rows.first() {
            Some(&first) if ex.row_in_header(first) => Some(cells.remove(0)),
            _ => None,
        };
        let header = header.unwrap_or_else(|| vec![String::new(); columns]);
        let mut widths = vec![3usize; columns];
        for row in std::iter::once(&header).chain(cells.iter()) {
            for (i, c) in row.iter().enumerate() {
                widths[i] = widths[i].max(c.width());
            }
        }
        let line = |row: &[String]| -> String {
            let mut out = String::from("|");
            for (i, w) in widths.iter().enumerate() {
                let c = row.get(i).map(String::as_str).unwrap_or("");
                let pad = w - c.width();
                let (l, r) = match aligns.get(i).copied().unwrap_or("left") {
                    "right" => (pad, 0),
                    "center" => (pad / 2, pad - pad / 2),
                    _ => (0, pad),
                };
                out.push_str(&format!(" {}{c}{} |", " ".repeat(l), " ".repeat(r)));
            }
            out
        };
        let mut out = line(&header);
        out.push_str("\n|");
        for (i, w) in widths.iter().enumerate() {
            let rule = match aligns.get(i).copied().unwrap_or("left") {
                "right" => format!("{}:", "-".repeat(w - 1)),
                "center" => format!(":{}:", "-".repeat(w - 2)),
                _ => "-".repeat(*w),
            };
            out.push_str(&format!(" {rule} |"));
        }
        for row in &cells {
            out.push('\n');
            out.push_str(&line(row));
        }
        out
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

    fn inner_template(&self, ex: &mut Exporter<'_>, body: String) -> String {
        Markdown.inner_template(ex, body)
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
            SRC_BLOCK => {
                let b: ast::SrcBlock = ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()))?;
                let lang = b.language().unwrap_or_default();
                let code = md::format_code_default(ex, id);
                format!("```{lang}\n{code}```")
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
        let out = gfm("| Name | n |\n|------+---|\n| *a* | 1 |\n| b\\vert{}c | 22 |\n");
        assert_eq!(
            out.trim_end(),
            "| Name     |   n |\n| -------- | --: |\n| **a**    |   1 |\n| b&vert;c |  22 |"
        );
        let out = gfm("| x | y |\n| z | w |\n");
        assert_eq!(
            out.trim_end(),
            "|     |     |\n| --- | --- |\n| x   | y   |\n| z   | w   |"
        );
    }

    #[test]
    fn code_and_strike() {
        let out = gfm("+gone+\n\n#+begin_src rust\nfn main() {}\n#+end_src\n");
        assert!(out.contains("~~gone~~"), "{out}");
        assert!(out.contains("```rust\nfn main() {}\n```"), "{out}");
    }
}
