//! The in-buffer settings pre-pass: what `org-set-regexps-and-options`
//! and `org-update-radio-target-regexp` compute when Org mode starts.

use crate::SyntaxKind::*;
use crate::context::ParseContext;
use crate::elements::Parser;
use crate::raw::Raw;

/// Loads the contents of `#+SETUPFILE` files.
pub trait SetupFileLoader {
    /// Returns the contents of the setup file named `name` (the keyword's
    /// value, without quotes or angle brackets), or `None` if it cannot be
    /// read. `from` is the name of the file containing the keyword, or
    /// `None` for the document itself.
    fn load(&self, name: &str, from: Option<&str>) -> Option<(String, String)>;
}

/// A loader that never loads anything.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSetupFiles;

impl SetupFileLoader for NoSetupFiles {
    fn load(&self, _: &str, _: Option<&str>) -> Option<(String, String)> {
        None
    }
}

/// Loads setup files from the file system, relative to the directory of the
/// file that references them. URLs are not fetched.
#[derive(Debug, Clone)]
pub struct FsSetupFiles {
    /// Directory of the document.
    pub base: std::path::PathBuf,
}

impl SetupFileLoader for FsSetupFiles {
    fn load(&self, name: &str, from: Option<&str>) -> Option<(String, String)> {
        if name.contains("://") {
            return None;
        }
        let dir = match from {
            Some(f) => std::path::Path::new(f)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| self.base.clone()),
            None => self.base.clone(),
        };
        let expanded = if let Some(rest) = name.strip_prefix("~/") {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(rest))?
        } else {
            dir.join(name)
        };
        let text = std::fs::read_to_string(&expanded).ok()?;
        Some((expanded.to_string_lossy().into_owned(), text))
    }
}

/// Quick test for keywords that change the context, so documents without
/// them skip the keyword pre-pass.
fn has_context_keywords(text: &str) -> bool {
    let b = text.as_bytes();
    memchr::memmem::find_iter(b, b"#+").any(|i| {
        let rest = &b[i + 2..];
        [
            "TODO",
            "SEQ_TODO",
            "TYP_TODO",
            "LINK",
            "STARTUP",
            "SETUPFILE",
        ]
        .iter()
        .any(|k| rest.len() >= k.len() && rest[..k.len()].eq_ignore_ascii_case(k.as_bytes()))
    })
}

/// Returns the context for `text` from its keywords only (no radio targets).
pub(crate) fn keyword_context(
    text: &str,
    base: &ParseContext,
    loader: &dyn SetupFileLoader,
) -> ParseContext {
    let normalized = crate::crlf::normalize(text);
    let text: &str = normalized.as_ref().map_or(text, |(n, _)| n.as_str());
    let mut ctx = base.clone();
    ctx.compiled = Default::default();
    if !has_context_keywords(text) {
        return ctx;
    }
    apply_keywords(text, base, loader, &mut ctx);
    ctx
}

/// Parses a document whose context comes from its own keywords: one
/// keyword pre-pass (when needed) and one full parse, plus a second parse
/// only when the document has radio targets.
pub(crate) fn parse_document(
    text: &str,
    base: &ParseContext,
    loader: &dyn SetupFileLoader,
) -> (crate::raw::Raw, ParseContext, Option<crate::crlf::Norm>) {
    let ctx = keyword_context(text, base, loader);
    let (raw, norm) = crate::parse_raw(text, &ctx);
    if memchr::memmem::find(text.as_bytes(), b"<<<").is_some() {
        let mut targets: Vec<String> = Vec::new();
        collect_radio_targets(&raw, text, &mut targets);
        if !targets.is_empty() {
            let mut ctx2 = ctx;
            ctx2.radio_targets = targets;
            ctx2.compiled = Default::default();
            let (raw2, norm2) = crate::parse_raw(text, &ctx2);
            return (raw2, ctx2, norm2);
        }
    }
    (raw, ctx, norm)
}

/// Returns the context for `text`, starting from `base`.
pub(crate) fn context_for(
    text: &str,
    base: &ParseContext,
    loader: &dyn SetupFileLoader,
) -> ParseContext {
    let normalized = crate::crlf::normalize(text);
    let text: &str = normalized.as_ref().map_or(text, |(n, _)| n.as_str());
    let mut ctx = base.clone();
    ctx.compiled = Default::default();
    if has_context_keywords(text) {
        apply_keywords(text, base, loader, &mut ctx);
    }
    if memchr::memmem::find(text.as_bytes(), b"<<<").is_some() {
        let parser = Parser::new(text, &ctx);
        let doc = parser.parse_document();
        let mut targets: Vec<String> = Vec::new();
        collect_radio_targets(&doc, text, &mut targets);
        if !targets.is_empty() {
            targets.dedup();
            ctx.radio_targets = targets;
            ctx.compiled = Default::default();
        }
    }
    ctx
}

/// Reads the in-buffer settings of `text` into `ctx`.
fn apply_keywords(
    text: &str,
    base: &ParseContext,
    loader: &dyn SetupFileLoader,
    ctx: &mut ParseContext,
) {
    let mut typ_lines: Vec<String> = Vec::new();
    let mut todo_lines: Vec<String> = Vec::new();
    let mut seq_lines: Vec<String> = Vec::new();
    let mut links: Vec<(String, String)> = Vec::new();
    let mut startup: Vec<String> = Vec::new();
    let mut chain: Vec<String> = Vec::new();
    keywords_with_setupfiles(
        text,
        None,
        base,
        loader,
        &mut chain,
        0,
        &mut |key, value| match key.as_str() {
            "TYP_TODO" => typ_lines.push(value.to_string()),
            "TODO" => todo_lines.push(value.to_string()),
            "SEQ_TODO" => seq_lines.push(value.to_string()),
            "LINK" => {
                let mut it = value.splitn(2, [' ', '\t']);
                if let (Some(k), Some(v)) = (it.next(), it.next()) {
                    links.push((k.to_string(), v.trim().to_string()));
                }
            }
            "STARTUP" => startup.extend(value.split_whitespace().map(str::to_string)),
            _ => {}
        },
    );
    // `#+TYP_TODO` sets come first, then `#+TODO`, then `#+SEQ_TODO`.
    if !(typ_lines.is_empty() && todo_lines.is_empty() && seq_lines.is_empty()) {
        use crate::context::{TodoSequence, TodoSequenceKind};
        let sequences = typ_lines
            .iter()
            .map(|l| TodoSequence::parse(TodoSequenceKind::Type, l))
            .chain(
                todo_lines
                    .iter()
                    .chain(&seq_lines)
                    .map(|l| TodoSequence::parse(TodoSequenceKind::Sequence, l)),
            )
            .collect();
        ctx.set_todo_sequences(sequences);
    }
    if !links.is_empty() {
        ctx.link_abbrevs.extend(links);
    }
    for s in &startup {
        match s.as_str() {
            "odd" | "oddeven" => ctx.odd_levels_only = s == "odd",
            _ => {}
        }
    }
}

/// Inserts the keywords of each `#+SETUPFILE` before that entry.
pub(crate) fn expand_setupfiles(
    found: Vec<(String, String)>,
    base: &ParseContext,
    loader: &dyn SetupFileLoader,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chain: Vec<String> = Vec::new();
    for (k, v) in found {
        if k == "SETUPFILE" {
            let name = v
                .trim()
                .trim_matches('"')
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string();
            if let Some((path, contents)) = loader.load(&name, None) {
                chain.push(path.clone());
                let normalized = crate::crlf::normalize(&contents);
                let contents: &str = normalized
                    .as_ref()
                    .map_or(contents.as_str(), |(n, _)| n.as_str());
                keywords_with_setupfiles(
                    contents,
                    Some(&path),
                    base,
                    loader,
                    &mut chain,
                    1,
                    &mut |k, v| out.push((k, v.to_string())),
                );
                chain.pop();
            }
        }
        out.push((k, v));
    }
    out
}

/// Collects keywords of `text` in order, expanding `#+SETUPFILE` in place.
fn keywords_with_setupfiles(
    text: &str,
    from: Option<&str>,
    base: &ParseContext,
    loader: &dyn SetupFileLoader,
    visited: &mut Vec<String>,
    depth: usize,
    f: &mut dyn FnMut(String, &str),
) {
    let parser = Parser::elements_only(text, base);
    let doc = parser.parse_document();
    let mut found: Vec<(String, String)> = Vec::new();
    collect_keywords(&doc, text, &mut |k, v| found.push((k, v.to_string())));
    for (k, v) in found {
        if k == "SETUPFILE" && depth < 8 {
            let name = v
                .trim()
                .trim_matches('"')
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string();
            // Like `org--collect-keywords-1`, only the chain of files being
            // read is excluded, so a file included twice is read twice.
            if let Some((path, contents)) = loader.load(&name, from)
                && !visited.contains(&path)
            {
                visited.push(path.clone());
                let normalized = crate::crlf::normalize(&contents);
                let contents: &str = normalized
                    .as_ref()
                    .map_or(contents.as_str(), |(n, _)| n.as_str());
                keywords_with_setupfiles(
                    contents,
                    Some(&path),
                    base,
                    loader,
                    visited,
                    depth + 1,
                    f,
                );
                visited.pop();
            }
        }
        f(k, &v);
    }
}

fn collect_keywords(raw: &Raw, text: &str, f: &mut dyn FnMut(String, &str)) {
    crate::deep(|| collect_keywords_inner(raw, text, f))
}

fn collect_keywords_inner(raw: &Raw, text: &str, f: &mut dyn FnMut(String, &str)) {
    if raw.kind == KEYWORD {
        let key = raw.tokens.iter().find(|t| t.kind == KEY);
        if let Some(k) = key {
            let name = crate::tables::upcase(&text[k.start..k.end]);
            let line_end = text[k.end..raw.end]
                .find('\n')
                .map_or(raw.end, |i| k.end + i);
            let value = text[(k.end + 1).min(line_end)..line_end].trim();
            f(name, value);
        }
    }
    for c in &raw.children {
        collect_keywords(c, text, f);
    }
}

fn collect_radio_targets(raw: &Raw, text: &str, out: &mut Vec<String>) {
    crate::deep(|| collect_radio_targets_inner(raw, text, out))
}

fn collect_radio_targets_inner(raw: &Raw, text: &str, out: &mut Vec<String>) {
    if raw.kind == RADIO_TARGET
        && let (Some(cb), Some(ce)) = (raw.cb, raw.ce)
    {
        let v = text[cb..ce].to_string();
        if !out.contains(&v) {
            out.push(v);
        }
    }
    for c in &raw.children {
        collect_radio_targets(c, text, out);
    }
}
