//! Org mode export as Emacs's `ox.el` does it: the export options, the
//! tree with what is not exported taken out, and back-ends that write
//! HTML, Markdown, plain text and LaTeX the way `ox-html`, `ox-md`,
//! `ox-ascii` and `ox-latex` write them (design §10, T2.3).

pub mod babel;
mod dictionary;
pub mod export;
pub mod html;
pub mod include;
pub mod macros;
pub mod md;
pub mod options;
pub mod quotes;
pub mod timestamps;
pub mod tree;

pub use export::{Backend, Exporter};
pub use html::Html;
pub use md::Markdown;

/// How to export.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    /// Only the body, without the document template.
    pub body_only: bool,
    /// The file the text comes from, for relative links and includes.
    pub input_file: Option<std::path::PathBuf>,
    /// The time `{{{time}}}` and dates use; now if not given.
    pub now: Option<jiff::Zoned>,
}

/// Exports Org `text` with `backend`.
pub fn export(text: &str, backend: &dyn Backend, settings: &Settings) -> Result<String, String> {
    let now = settings.now.clone().unwrap_or_else(jiff::Zoned::now);
    // Keywords parsed as Org text, whose macros expand too.
    let mut parsed: Vec<&str> = vec!["TITLE", "DATE", "AUTHOR"];
    for (_, k, _, b, _) in backend.options() {
        if b == options::Behavior::Parse
            && let Some(k) = k
        {
            parsed.push(k);
        }
    }
    // A file with DOS line endings throughout reads as Emacs decodes it.
    let decoded;
    let text =
        if text.contains("\r\n") && text.matches('\n').count() == text.matches("\r\n").count() {
            decoded = text.replace("\r\n", "\n");
            decoded.as_str()
        } else {
            text
        };
    let text = include::expand(text, settings.input_file.as_deref())?;
    let text = macros::expand(&text, &parsed, settings.input_file.as_deref(), &now)?;
    let text = babel::process(&text);
    let parse = org_syntax::parse(&text);
    let root = parse.syntax();
    let mut ex = Exporter::new(&root, parse.context().clone(), backend);
    ex.info.body_only = settings.body_only;
    ex.info.input_file = settings.input_file.clone();
    ex.read_environment(&parse.keywords());
    ex.prune();
    backend.filter_parse_tree(&mut ex);
    ex.collect_tree_properties();
    let root_id = ex.tree.root;
    let body = export::normalize_string(&ex.data(root_id));
    let full = backend.inner_template(&mut ex, body);
    let out = if settings.body_only {
        full
    } else {
        backend.template(&mut ex, full)
    };
    Ok(backend.filter_final_output(&mut ex, out))
}

/// `org-export-output-file-name`: where exporting `input` (with `text`)
/// writes, for a back-end whose files end with `extension` (`.html`):
/// `#+EXPORT_FILE_NAME`, else the input's name, beside the input, with
/// the extension.
pub fn output_file_name(
    text: &str,
    input: &std::path::Path,
    extension: &str,
) -> std::path::PathBuf {
    let dir = input.parent().unwrap_or(std::path::Path::new("."));
    let parse = org_syntax::parse(text);
    let keyword = parse
        .syntax()
        .descendants()
        .filter_map(<org_syntax::ast::Keyword as org_syntax::ast::AstNode>::cast)
        .find(|k| k.key() == "EXPORT_FILE_NAME" && !k.value().trim().is_empty())
        .map(|k| k.value().trim().to_string());
    let name = keyword.unwrap_or_else(|| {
        let n = input
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        n.strip_suffix(".gpg").map(str::to_string).unwrap_or(n)
    });
    let stem = std::path::Path::new(&name).with_extension("");
    let base = format!("{}{extension}", stem.display());
    let out = if std::path::Path::new(&base).is_absolute() {
        std::path::PathBuf::from(base)
    } else {
        dir.join(base)
    };
    if out == input {
        let mut s = out.into_os_string();
        s.push(extension);
        return s.into();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn output_names() {
        let p = Path::new("/d/notes.org");
        assert_eq!(
            output_file_name("* A\n", p, ".html"),
            Path::new("/d/notes.html")
        );
        assert_eq!(
            output_file_name("#+EXPORT_FILE_NAME: out/x\n", p, ".md"),
            Path::new("/d/out/x.md")
        );
        assert_eq!(
            output_file_name("", Path::new("/d/page.html"), ".html"),
            Path::new("/d/page.html.html")
        );
    }
}
