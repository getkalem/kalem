//! Org mode export as Emacs's `ox.el` does it: the export options, the
//! tree with what is not exported taken out, and back-ends that write
//! HTML, Markdown, plain text and LaTeX the way `ox-html`, `ox-md`,
//! `ox-ascii` and `ox-latex` write them (design §10, T2.3).

pub mod babel;
mod dictionary;
pub mod export;
pub mod gfm;
pub mod html;
pub mod include;
pub mod macros;
pub mod md;
pub mod options;
pub mod quotes;
pub mod timestamps;
pub mod tree;

pub use export::{Backend, Exporter};
pub use gfm::Gfm;
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
    /// Export only the subtree containing this byte offset of the text
    /// (`C-c C-e C-s`): its headline's contents, with its `EXPORT_`
    /// properties over the document's options.
    pub subtree: Option<usize>,
    /// Draws formulas for `tex:svg` (and the image processing types
    /// `dvisvgm`, `dvipng` and `imagemagick`, which need LaTeX in Emacs).
    pub math: Option<MathRenderer>,
}

/// Draws LaTeX formulas as SVG images.
pub trait MathSvg: Send + Sync {
    /// `formula`, a LaTeX fragment with its delimiters or an environment,
    /// with the definitions of the `#+LATEX_HEADER` lines `headers`; `None`
    /// if it cannot be drawn.
    fn render(&self, formula: &str, headers: &[String]) -> Option<SvgFormula>;
}

/// A formula drawn by a [`MathSvg`].
#[derive(Debug, Clone, PartialEq)]
pub struct SvgFormula {
    /// The SVG document.
    pub svg: String,
    /// Width, in ems.
    pub width: f64,
    /// Height, in ems.
    pub height: f64,
    /// How far it reaches below the baseline, in ems.
    pub depth: f64,
    /// Whether it is displayed rather than inline.
    pub display: bool,
}

/// A shared [`MathSvg`].
#[derive(Clone)]
pub struct MathRenderer(pub std::sync::Arc<dyn MathSvg>);

impl std::fmt::Debug for MathRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MathRenderer")
    }
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
    let file = settings.input_file.as_deref();
    let region = match settings.subtree {
        Some(at) => Some(subtree_region(text, at)?),
        None => None,
    };
    // Includes are expanded in the part exported; macros everywhere, as
    // `org-macro-replace-all` widens.
    let (text, mut marks) = match &region {
        Some(r) => {
            let inner = include::expand(&text[r.start..r.end], file)?;
            let end = r.start + inner.len();
            (
                format!("{}{inner}{}", &text[..r.start], &text[r.end..]),
                [r.start, end],
            )
        }
        None => {
            let t = include::expand(text, file)?;
            let n = t.len();
            (t, [0, n])
        }
    };
    let text = macros::expand_tracking(&text, &parsed, file, &now, &mut marks)?;
    let whole = parse_document(&text, file);
    let (parse, keywords) = match &region {
        Some(_) => {
            let body = babel::process(&text[marks[0]..marks[1]]);
            (
                org_syntax::parse_with(&body, whole.context()),
                whole.keywords(),
            )
        }
        None => {
            let text = babel::process(&text);
            let parse = parse_document(&text, file);
            let keywords = parse.keywords();
            (parse, keywords)
        }
    };
    let root = parse.syntax();
    let mut ex = Exporter::new(&root, parse.context().clone(), backend);
    ex.info.body_only = settings.body_only;
    ex.info.now = Some(now.clone());
    ex.info.math = settings.math.clone();
    ex.info.input_file = settings.input_file.clone();
    ex.read_environment(&keywords);
    if region.is_some() {
        let (properties, title) = headline_at(&whole, marks[0]);
        ex.read_subtree_options(&properties, &title);
    }
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

/// The part of `text` exporting the subtree at byte `at` exports, as
/// `org-export-as` narrows to it: from the end of the headline's meta
/// data (its line, planning and property drawer), keeping the line feed
/// before, to the end of the subtree, without the line feed before the
/// next headline.
fn subtree_region(text: &str, at: usize) -> Result<std::ops::Range<usize>, String> {
    use org_syntax::ast::{AstNode, Headline};
    let parse = org_syntax::parse(text);
    let root = parse.syntax();
    let at = at.min(text.len());
    let headline = root
        .descendants()
        .filter(|n| n.kind() == org_syntax::SyntaxKind::HEADLINE)
        .filter(|n| {
            let r = n.text_range();
            usize::from(r.start()) <= at && at < usize::from(r.end()).max(1)
                || usize::from(r.end()) == text.len() && at == text.len()
        })
        .last()
        .and_then(Headline::cast)
        .ok_or_else(|| "Before first headline at position".to_string())?;
    let range = headline.syntax().text_range();
    let (start, mut end) = (usize::from(range.start()), usize::from(range.end()));
    if end < text.len() && text[..end].ends_with('\n') {
        end -= 1;
    }
    let line_end = text[start..]
        .find('\n')
        .map_or(text.len(), |i| start + i + 1);
    let mut meta = line_end;
    if let Some(p) = headline.planning() {
        meta = meta.max(usize::from(p.syntax().text_range().end()));
    }
    if let Some(d) = headline.property_drawer() {
        meta = meta.max(usize::from(d.syntax().text_range().end()));
    }
    let meta = meta.min(end.max(line_end.min(text.len())));
    let begin = if text[..meta].ends_with('\n') {
        meta - 1
    } else {
        meta
    };
    Ok(begin..end.max(begin))
}

/// The properties and title (without keyword, priority or tags) of the
/// headline whose meta data ends at byte `at` of the parsed text.
fn headline_at(parse: &org_syntax::Parse, at: usize) -> (Vec<(String, String)>, String) {
    use org_syntax::ast::{AstNode, Headline};
    let headline = parse
        .syntax()
        .descendants()
        .filter(|n| n.kind() == org_syntax::SyntaxKind::HEADLINE)
        .filter(|n| usize::from(n.text_range().start()) <= at)
        .filter(|n| at <= usize::from(n.text_range().end()))
        .last()
        .and_then(Headline::cast);
    match headline {
        Some(h) => (h.properties(), h.raw_value()),
        None => (Vec::new(), String::new()),
    }
}

/// Parses `text`, the contents of `file`, reading its `#+SETUPFILE`
/// files relative to it (without a file, setup files are not read).
pub(crate) fn parse_document(text: &str, file: Option<&std::path::Path>) -> org_syntax::Parse {
    match file {
        Some(f) => org_syntax::parse_file(text, f),
        None => org_syntax::parse(text),
    }
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
    output_file_name_for(text, input, extension, None)
}

/// [`output_file_name`] for exporting the subtree at byte `subtree`, whose
/// `EXPORT_FILE_NAME` property comes first.
pub fn output_file_name_for(
    text: &str,
    input: &std::path::Path,
    extension: &str,
    subtree: Option<usize>,
) -> std::path::PathBuf {
    let dir = input.parent().unwrap_or(std::path::Path::new("."));
    let parse = org_syntax::parse(text);
    let property = subtree
        .and_then(|at| subtree_region(text, at).ok())
        .and_then(|r| {
            headline_at(&parse, r.start)
                .0
                .into_iter()
                .find(|(k, v)| k.eq_ignore_ascii_case("EXPORT_FILE_NAME") && !v.trim().is_empty())
                .map(|(_, v)| v.trim().to_string())
        });
    let keyword = property.or_else(|| {
        parse
            .syntax()
            .descendants()
            .filter_map(<org_syntax::ast::Keyword as org_syntax::ast::AstNode>::cast)
            .find(|k| k.key() == "EXPORT_FILE_NAME" && !k.value().trim().is_empty())
            .map(|k| k.value().trim().to_string())
    });
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
    fn subtree() {
        let text = "#+TITLE: Doc\n#+MACRO: m M\nBefore.\n* One\nNo.\n* Two :t:\n:PROPERTIES:\n:EXPORT_TITLE: Sub {{{m}}}\n:EXPORT_OPTIONS: num:nil\n:END:\nIn two {{{m}}}.\n** Child\nText.\n* Three\nNo.\n";
        let settings = Settings {
            subtree: Some(text.find("* Two").unwrap() + 3),
            ..Settings::default()
        };
        let out = export(text, &Markdown, &settings).unwrap();
        assert!(out.contains("In two M."), "{out}");
        assert!(out.contains("# Child"), "{out}");
        assert!(!out.contains("No."), "{out}");
        assert!(!out.contains("Before"), "{out}");
        let page = export(text, &Html, &settings).unwrap();
        assert!(page.contains("<title>Sub M</title>"), "{page}");
        assert!(
            export(
                text,
                &Markdown,
                &Settings {
                    subtree: Some(3),
                    ..Settings::default()
                }
            )
            .is_err()
        );
    }

    struct Fake;

    impl MathSvg for Fake {
        fn render(&self, formula: &str, headers: &[String]) -> Option<SvgFormula> {
            (!formula.contains("bad")).then(|| SvgFormula {
                svg: format!("<svg>{}</svg>", headers.len()),
                width: 2.0,
                height: 1.0,
                depth: 0.25,
                display: formula.starts_with("\\begin"),
            })
        }
    }

    #[test]
    fn svg_math() {
        let settings = Settings {
            body_only: true,
            math: Some(MathRenderer(std::sync::Arc::new(Fake))),
            ..Settings::default()
        };
        let text = "#+OPTIONS: tex:svg\n#+LATEX_HEADER: \\def\\R{x}\nA $x$ and $bad$.\n\n\\begin{equation}\ny\n\\end{equation}\n";
        let out = export(text, &Html, &settings).unwrap();
        assert!(
            out.contains("<img src=\"data:image/svg+xml,%3Csvg%3E1%3C/svg%3E\" alt=\"$x$\" class=\"org-latex org-latex-inline\" style=\"width: 2.000em; height: 1.000em; vertical-align: -0.250em\" />"),
            "{out}"
        );
        assert!(out.contains("\\(bad\\)"), "{out}");
        assert!(out.contains("class=\"equation-container\""), "{out}");
        // MathJax otherwise, and no image without a renderer.
        let plain = export(&text.replace("tex:svg", "tex:t"), &Html, &settings).unwrap();
        assert!(plain.contains("\\(x\\)"), "{plain}");
        let none = export(
            text,
            &Html,
            &Settings {
                body_only: true,
                ..Settings::default()
            },
        )
        .unwrap();
        assert!(none.contains("\\(x\\)"), "{none}");
    }

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
        let t = "#+EXPORT_FILE_NAME: all\n* A\n:PROPERTIES:\n:EXPORT_FILE_NAME: part\n:END:\n* B\n";
        assert_eq!(
            output_file_name_for(t, p, ".html", Some(t.find("* A").unwrap())),
            Path::new("/d/part.html")
        );
        assert_eq!(
            output_file_name_for(t, p, ".html", Some(t.find("* B").unwrap())),
            Path::new("/d/all.html")
        );
        assert_eq!(
            output_file_name("", Path::new("/d/page.html"), ".html"),
            Path::new("/d/page.html.html")
        );
    }
}
