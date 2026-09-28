//! Citations (`oc.el`): the references a document cites, the
//! bibliography files it names and the processor it exports them with.

use std::path::{Path, PathBuf};

use org_syntax::ast::{AstNode, Citation};

use crate::Document;

/// A citation in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitationInfo {
    /// Where it is.
    pub range: std::ops::Range<usize>,
    /// Its style (`t` in `[cite/t:…]`).
    pub style: Option<String>,
    /// Its variant (`b` in `[cite/t/b:…]`).
    pub variant: Option<String>,
    /// The keys it cites, in order.
    pub keys: Vec<String>,
    /// The global prefix and suffix, as written.
    pub prefix: Option<String>,
    /// See [`CitationInfo::prefix`].
    pub suffix: Option<String>,
    /// Each reference's key with its own prefix and suffix, as written.
    pub references: Vec<(String, Option<String>, Option<String>)>,
}

/// `#+CITE_EXPORT: PROCESSOR [BIBLIOGRAPHY-STYLE [CITATION-STYLE]]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiteExport {
    /// `csl`, `biblatex`, `natbib` or `basic`.
    pub processor: String,
    /// The bibliography style (a CSL file, a biblatex style).
    pub bibliography_style: Option<String>,
    /// The citation style.
    pub citation_style: Option<String>,
}

/// `org-strip-quotes`.
fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(s)
}

/// Words of a keyword value, double-quoted ones kept whole.
fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        if let Some(q) = rest.strip_prefix('"') {
            let end = q.find('"').unwrap_or(q.len());
            out.push(q[..end].to_string());
            rest = q.get(end + 1..).unwrap_or("").trim_start();
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            out.push(rest[..end].to_string());
            rest = rest[end..].trim_start();
        }
    }
    out
}

impl Document {
    /// The citations, in order.
    pub fn citations(&self) -> Vec<CitationInfo> {
        let text = |n: &org_syntax::SyntaxNode| n.text().to_string();
        self.parse
            .syntax()
            .descendants()
            .filter_map(Citation::cast)
            .map(|c| {
                let (style, variant) = c.style_and_variant();
                let r = c.syntax().text_range();
                CitationInfo {
                    range: usize::from(r.start())..usize::from(r.end()),
                    style,
                    variant,
                    keys: c.keys(),
                    prefix: c.prefix().map(|n| text(&n)),
                    suffix: c.suffix().map(|n| text(&n)),
                    references: c
                        .references()
                        .map(|r| {
                            (
                                r.key(),
                                r.prefix().map(|n| text(&n)),
                                r.suffix().map(|n| text(&n)),
                            )
                        })
                        .collect(),
                }
            })
            .collect()
    }

    /// The keys cited, each once, in the order of their first citation.
    pub fn cited_keys(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in self.citations() {
            for k in c.keys {
                if !out.contains(&k) {
                    out.push(k);
                }
            }
        }
        out
    }

    /// `org-cite-list-bibliography-files`: the files of the
    /// `#+BIBLIOGRAPHY` keywords, each once; relative ones from `dir`.
    pub fn bibliography(&self, dir: Option<&Path>) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for (k, v) in &self.info().keywords {
            if !k.eq_ignore_ascii_case("BIBLIOGRAPHY") {
                continue;
            }
            let f = strip_quotes(v);
            if f.is_empty() {
                continue;
            }
            let p = PathBuf::from(f);
            let p = match dir {
                Some(d) if p.is_relative() => d.join(p),
                _ => p,
            };
            if !out.contains(&p) {
                out.push(p);
            }
        }
        out
    }

    /// The last `#+CITE_EXPORT` keyword, read.
    pub fn cite_export(&self) -> Option<CiteExport> {
        let v = self
            .info()
            .keywords
            .iter()
            .rev()
            .find(|(k, _)| k.eq_ignore_ascii_case("CITE_EXPORT"))?
            .1
            .clone();
        let mut w = words(&v).into_iter();
        Some(CiteExport {
            processor: w.next()?,
            bibliography_style: w.next(),
            citation_style: w.next(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn citations() {
        let text = "#+bibliography: refs.bib\n#+BIBLIOGRAPHY: \"/abs/more refs.bib\"\n#+bibliography: refs.bib\n#+cite_export: csl \"chicago author.csl\" note\n\nSee [cite/t/b:before; see @a p. 3; @b; after] and [cite:@c;@a].\n";
        let d = Document::new(org_syntax::parse(text));
        let c = d.citations();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].style.as_deref(), Some("t"));
        assert_eq!(c[0].variant.as_deref(), Some("b"));
        assert_eq!(c[0].keys, ["a", "b"]);
        assert_eq!(c[0].prefix.as_deref(), Some("before"));
        assert_eq!(c[0].suffix.as_deref(), Some(" after"));
        assert_eq!(
            c[0].references[0],
            ("a".into(), Some(" see ".into()), Some(" p. 3".into()))
        );
        assert_eq!(c[1].style, None);
        assert_eq!(d.cited_keys(), ["a", "b", "c"]);
        assert_eq!(
            d.bibliography(Some(Path::new("/doc"))),
            [
                PathBuf::from("/doc/refs.bib"),
                PathBuf::from("/abs/more refs.bib")
            ]
        );
        assert_eq!(
            d.cite_export(),
            Some(CiteExport {
                processor: "csl".into(),
                bibliography_style: Some("chicago author.csl".into()),
                citation_style: Some("note".into()),
            })
        );
    }
}
