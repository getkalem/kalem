//! `#+INCLUDE:` (`org-export-expand-include-keyword`): other files, or
//! parts of them, placed in the document before export: as Org text
//! (headings shifted to fit, footnotes renamed, links fixed), or in a
//! block (`src`, `example`, `export`, or any other name).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use org_syntax::SyntaxKind::*;
use org_syntax::ast::{self, AstNode};

/// What an `#+INCLUDE:` line asks for (`org-export-parse-include-value`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Include {
    file: Option<String>,
    location: Option<String>,
    only_contents: bool,
    lines: Option<String>,
    literal: bool,
    minlevel: Option<usize>,
    args: Option<String>,
    block: Option<String>,
}

fn is_url(s: &str) -> bool {
    let scheme = s.split_once("://").map(|(a, _)| a);
    scheme.is_some_and(|a| {
        !a.is_empty()
            && a.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && a.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

/// A word of `value` at a word boundary: `\<word\>`.
fn find_word(value: &str, word: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = value[from..].find(word) {
        let at = from + i;
        let before = value[..at].chars().last();
        let after = value[at + word.len()..].chars().next();
        let boundary = |c: Option<char>| !c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        if boundary(before) && boundary(after) {
            return Some(at);
        }
        from = at + word.len();
    }
    None
}

fn parse_value(value: &str, dir: &Path, induced: usize) -> Include {
    let mut value = value.to_string();
    let mut inc = Include::default();
    // `:coding X>` is left out.
    // The file: quoted, or up to a blank.
    let trimmed = value.trim_start().to_string();
    let lead = value.len() - trimmed.len();
    let (matched, rest_at) = if let Some(stripped) = trimmed.strip_prefix('"') {
        match stripped.find('"') {
            Some(i) => (trimmed[..i + 2].to_string(), i + 2),
            None => (String::new(), 0),
        }
    } else {
        let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
        (trimmed[..end].to_string(), end)
    };
    if !matched.is_empty() && lead == 0 {
        value = value[rest_at..].trim_start().to_string();
        let mut m = matched.clone();
        let quoted_end = m.ends_with('"') && m.len() > 1;
        let body = if quoted_end {
            &m[..m.len() - 1]
        } else {
            &m[..]
        };
        if let Some(i) = body.find("::") {
            inc.location = Some(body[i + 2..].to_string());
            m = format!("{}{}", &body[..i], if quoted_end { "\"" } else { "" });
        }
        let stripped = m.trim_matches('"').to_string();
        inc.file = Some(if is_url(&stripped) {
            stripped
        } else {
            crate::export::expand_file_name(&stripped, dir)
        });
    }
    // `:only-contents t`.
    if let Some(i) = value.find(":only-contents") {
        let after = value[i + 14..].trim_start();
        let word: String = after.chars().take_while(|c| !c.is_whitespace()).collect();
        inc.only_contents = !word.is_empty() && !word.starts_with(':') && word != "nil";
        let end = if word.is_empty() || word.starts_with(':') {
            i + 14
        } else {
            i + 14 + (value[i + 14..].len() - after.len()) + word.len()
        };
        value.replace_range(i..end, "");
    }
    // `:lines "5-10"`.
    if let Some(i) = value.find(":lines") {
        let after = &value[i + 6..];
        let t = after.trim_start();
        if let Some(inner) = t.strip_prefix('"')
            && let Some(j) = inner.find('"')
            && inner[..j].chars().all(|c| c.is_ascii_digit() || c == '-')
            && inner[..j].contains('-')
        {
            inc.lines = Some(inner[..j].to_string());
            let end = i + 6 + (after.len() - t.len()) + j + 2;
            value.replace_range(i..end, "");
        }
    }
    // A block: `example`, `export BACKEND`, `src LANG ...`.
    let env = find_word(&value, "example")
        .map(|i| (i, None))
        .or_else(|| {
            find_word(&value, "export").map(|i| {
                let rest = value[i + 6..]
                    .strip_prefix(' ')
                    .map(|r| r.trim_start_matches(' '));
                (
                    i,
                    rest.filter(|r| !r.is_empty())
                        .map(|r| (value.len() - r.len(), r.to_string())),
                )
            })
        })
        .or_else(|| {
            find_word(&value, "src").map(|i| {
                let rest = value[i + 3..]
                    .strip_prefix(' ')
                    .map(|r| r.trim_start_matches(' '));
                (
                    i,
                    rest.filter(|r| !r.is_empty())
                        .map(|r| (value.len() - r.len(), r.to_string())),
                )
            })
        });
    inc.literal = env.is_some();
    if !inc.literal {
        inc.minlevel = Some(induced);
        if let Some(i) = value.find(":minlevel") {
            let after = &value[i + 9..];
            let t = after.trim_start();
            let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
            if !digits.is_empty() && t.len() < after.len() {
                inc.minlevel = digits.parse().ok();
                let end = i + 9 + (after.len() - t.len()) + digits.len();
                value.replace_range(i..end, "");
            }
        }
    } else if let Some((_, Some((at, args)))) = env {
        inc.args = Some(args);
        value.truncate(at);
    }
    // The block's name.
    let t = value.trim();
    let name = if let Some(inner) = t.strip_prefix('"') {
        inner.split('"').next().map(str::to_string)
    } else {
        t.split_whitespace().find(|w| !w.starts_with(':')).map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
                .to_string()
        })
    };
    inc.block = name.filter(|n| !n.is_empty());
    inc
}

/// `org-element-normalize-string`.
fn normalize_string(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let mut t = s.to_string();
    loop {
        let trimmed = t.trim_end_matches([' ', '\t']);
        if let Some(stripped) = trimmed.strip_suffix('\n') {
            t = stripped.to_string();
        } else {
            break;
        }
    }
    t.push('\n');
    t
}

/// Lines `a-b` of `text` (from 1, `b` left out, 0 for the start or the
/// end).
fn line_range(text: &str, lines: &str) -> String {
    let (a, b) = lines.split_once('-').unwrap_or((lines, ""));
    let a: usize = a.parse().unwrap_or(0);
    let b: usize = b.parse().unwrap_or(0);
    let starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let at = |n: usize| {
        starts
            .get(n.saturating_sub(1))
            .copied()
            .unwrap_or(text.len())
    };
    let beg = if a == 0 { 0 } else { at(a) };
    let end = if b == 0 { text.len() } else { at(b) };
    text[beg..end.max(beg)].to_string()
}

/// `path` absolute, with `.` and `..` resolved.
fn normalized(path: &Path) -> PathBuf {
    PathBuf::from(crate::export::expand_file_name(
        &path.display().to_string(),
        Path::new("."),
    ))
}

/// `file-relative-name`.
fn relative(path: &Path, base: &Path) -> String {
    let p: Vec<_> = path.components().collect();
    let b: Vec<_> = base.components().collect();
    let common = p.iter().zip(&b).take_while(|(x, y)| x == y).count();
    // Written with `/`, as Org links are on every system.
    let mut parts: Vec<String> = Vec::new();
    for _ in common..b.len() {
        parts.push("..".into());
    }
    for c in &p[common..] {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    let s = parts.join("/");
    if s.is_empty() { ".".into() } else { s }
}

/// Relative file links of `text`, from folder `from`, made relative to
/// folder `to` (`org-export--update-included-link`).
fn update_links(text: &str, from: &Path, to: &Path) -> String {
    let parse = org_syntax::parse(text);
    let ctx = parse.context().clone();
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for n in parse.syntax().descendants().filter(|n| n.kind() == LINK) {
        let Some(link) = ast::Link::cast(n.clone()) else {
            continue;
        };
        let info = link.info(&ctx);
        if info.link_type != "file" || info.path.starts_with('/') || info.path.starts_with('~') {
            continue;
        }
        let full = crate::export::expand_file_name(&info.path, from);
        let new_path = relative(Path::new(&full), to);
        let explicit = info.raw_link.starts_with("file:") || info.raw_link.starts_with("file+");
        let mut target = String::new();
        if explicit {
            target.push_str("file");
            if let Some(app) = &info.application {
                target.push('+');
                target.push_str(app);
            }
            target.push(':');
        }
        target.push_str(&new_path);
        if let Some(o) = &info.search_option {
            target.push_str("::");
            target.push_str(o);
        }
        let desc = link
            .description()
            .map(|r| text[usize::from(r.start())..usize::from(r.end())].to_string());
        let new = match (desc, info.format) {
            (Some(d), _) => format!("[[{target}][{d}]]"),
            (None, ast::LinkFormat::Angle) => format!("<{target}>"),
            (None, ast::LinkFormat::Plain) => target,
            (None, _) => format!("[[{target}]]"),
        };
        let start = usize::from(n.text_range().start());
        let end = start + n.text().to_string().trim_end_matches([' ', '\t']).len();
        edits.push((start, end, new));
    }
    let mut out = text.to_string();
    for (s, e, r) in edits.into_iter().rev() {
        out.replace_range(s..e, &r);
    }
    out
}

/// A file's text prepared for inclusion (`org-export--prepare-file-contents`).
#[allow(clippy::too_many_arguments)]
fn prepare(
    text: &str,
    file: &Path,
    lines: Option<&str>,
    ind: usize,
    minlevel: Option<usize>,
    id: Option<usize>,
    footnotes: &mut Vec<(String, String)>,
    includer: Option<&Path>,
) -> String {
    let whole = text.to_string();
    let mut text = match lines {
        Some(l) => line_range(&whole, l),
        None => whole.clone(),
    };
    if let (Some(inc), Some(dir)) = (includer.and_then(Path::parent), file.parent()) {
        let (inc, dir) = (normalized(inc), normalized(dir));
        if inc != dir {
            text = update_links(&text, &dir, &inc);
        }
    }
    // Blank lines around the contents go.
    let first = text.len() - text.trim_start_matches([' ', '\t', '\r', '\n']).len();
    let start = text[..first].rfind('\n').map_or(0, |i| i + 1);
    text = text[start..].to_string();
    let last = text.trim_end_matches([' ', '\t', '\r', '\n']).len();
    let end = text[last..].find('\n').map_or(text.len(), |i| last + i + 1);
    text.truncate(end);
    // The keyword's indentation, until the first heading.
    if ind > 0 {
        let pad = " ".repeat(ind);
        let mut out = String::new();
        let mut in_body = true;
        for l in text.split_inclusive('\n') {
            if in_body && is_heading(l) {
                in_body = false;
            }
            if in_body && !l.starts_with("[fn:") {
                out.push_str(&pad);
            }
            out.push_str(l);
        }
        text = out;
    }
    // Headings shifted so the highest is at `minlevel`.
    if let Some(min) = minlevel {
        let levels: Vec<usize> = text
            .split('\n')
            .filter(|l| is_heading(l))
            .map(|l| l.len() - l.trim_start_matches('*').len())
            .collect();
        if let Some(low) = levels.iter().min() {
            let offset = min as isize - *low as isize;
            if offset != 0 {
                text = text
                    .split_inclusive('\n')
                    .map(|l| {
                        if !is_heading(l) {
                            l.to_string()
                        } else if offset > 0 {
                            format!("{}{l}", "*".repeat(offset as usize))
                        } else {
                            l[(-offset) as usize..].to_string()
                        }
                    })
                    .collect();
            }
        }
    }
    // Footnote labels made file specific; definitions outside the lines
    // kept for the end of the document.
    if let Some(id) = id {
        text = rename_footnotes(&text, &whole, lines.is_some(), id, footnotes);
    }
    normalize_string(&text)
}

fn is_heading(l: &str) -> bool {
    let stars = l.len() - l.trim_start_matches('*').len();
    stars > 0 && l[stars..].starts_with([' ', '\t', '\n'])
}

/// `[fn:LABEL` in `text` becomes `[fn:-ID-LABEL`.
fn rename_footnotes(
    text: &str,
    whole: &str,
    partial: bool,
    id: usize,
    footnotes: &mut Vec<(String, String)>,
) -> String {
    let parse = org_syntax::parse(text);
    let mut labels: Vec<(usize, String)> = Vec::new();
    for n in parse.syntax().descendants() {
        let label = match n.kind() {
            FOOTNOTE_REFERENCE => ast::FootnoteReference::cast(n.clone()).and_then(|f| f.label()),
            FOOTNOTE_DEFINITION => ast::FootnoteDefinition::cast(n.clone()).map(|f| f.label()),
            _ => None,
        };
        if let Some(l) = label {
            labels.push((usize::from(n.text_range().start()), l));
        }
    }
    let mut out = text.to_string();
    let mut seen: Vec<String> = Vec::new();
    for (at, label) in labels.iter().rev() {
        let old = format!("[fn:{label}");
        if out[*at..].starts_with(&old) {
            out.replace_range(*at..*at + old.len(), &format!("[fn:-{id}-{label}"));
        }
        if !seen.contains(label) {
            seen.push(label.clone());
        }
    }
    // Definitions of labels used here but outside the included lines.
    if partial {
        let wparse = org_syntax::parse(whole);
        for label in seen.iter().rev() {
            let inside = parse.syntax().descendants().any(|n| {
                n.kind() == FOOTNOTE_DEFINITION
                    && ast::FootnoteDefinition::cast(n.clone()).is_some_and(|f| &f.label() == label)
            });
            if inside {
                continue;
            }
            if let Some(def) = wparse.syntax().descendants().find(|n| {
                n.kind() == FOOTNOTE_DEFINITION
                    && ast::FootnoteDefinition::cast(n.clone()).is_some_and(|f| &f.label() == label)
            }) {
                let body = def.text().to_string();
                let body = body
                    .split_once(']')
                    .map_or(body.as_str(), |(_, b)| b)
                    .trim();
                footnotes.push((format!("-{id}-{label}"), normalize_string(body)));
            }
        }
    }
    out
}

/// `text` with its `#+INCLUDE:` lines expanded; `file` is where `text`
/// comes from. `included` guards against loops.
/// An included file's text as Emacs inserts it: a UTF-8 byte order mark
/// dropped, CR LF line endings read as LF, and bytes that are not UTF-8
/// read as Latin-1 (it read the mark as text, kept the CRs, and refused
/// such a file).
fn read_included(path: &str) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let text = match String::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => e.into_bytes().into_iter().map(char::from).collect(),
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    Some(text.replace("\r\n", "\n"))
}

pub fn expand(text: &str, file: Option<&Path>) -> Result<String, String> {
    let mut footnotes = Vec::new();
    let mut out = expand_in(text, file, &[], &mut footnotes)?;
    for (k, v) in footnotes {
        out.push_str(&format!("\n[fn:{k}] {v}\n"));
    }
    Ok(out)
}

fn expand_in(
    text: &str,
    file: Option<&Path>,
    included: &[(String, Option<String>)],
    footnotes: &mut Vec<(String, String)>,
) -> Result<String, String> {
    if !text.to_ascii_lowercase().contains("#+include:") {
        return Ok(text.to_string());
    }
    let dir = file
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let parse = org_syntax::parse(text);
    let root = parse.syntax();
    let mut prefixes: HashMap<String, usize> = HashMap::new();
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for n in root.descendants().filter(|n| n.kind() == KEYWORD) {
        let Some(k) = ast::Keyword::cast(n.clone()) else {
            continue;
        };
        if k.key() != "INCLUDE" {
            continue;
        }
        let commented = n.ancestors().any(|a| {
            a.kind() == HEADLINE && ast::Headline::cast(a.clone()).is_some_and(|h| h.is_commented())
        });
        if commented {
            continue;
        }
        let start = usize::from(n.text_range().start());
        let line_end = text[start..]
            .find('\n')
            .map_or(text.len(), |i| start + i + 1);
        let induced = 1 + n
            .ancestors()
            .find(|a| a.kind() == HEADLINE)
            .and_then(ast::Headline::cast)
            .map_or(0, |h| h.level(parse.context()));
        let ind = text[start..].len() - text[start..].trim_start_matches([' ', '\t']).len();
        let inc = parse_value(&k.value(), &dir, induced);
        let Some(target) = inc.file.clone() else {
            edits.push((start, line_end, String::new()));
            continue;
        };
        if is_url(&target) {
            return Err(format!("Cannot include file {target}"));
        }
        let Some(contents) = read_included(&target) else {
            return Err(format!("Cannot include file {target}"));
        };
        let key = (target.clone(), inc.lines.clone());
        if included.contains(&key) {
            return Err(format!("Recursive file inclusion: {target}"));
        }
        let target_path = PathBuf::from(&target);
        let pad = " ".repeat(ind);
        let insert = if inc.literal {
            let block = inc.block.clone().unwrap_or_default();
            let args = inc
                .args
                .as_ref()
                .map(|a| format!(" {a}"))
                .unwrap_or_default();
            let body = prepare(
                &contents,
                &target_path,
                inc.lines.as_deref(),
                0,
                None,
                None,
                footnotes,
                None,
            );
            format!(
                "{pad}#+BEGIN_{block}{args}\n{}{pad}#+END_{block}\n",
                escape_code(&body)
            )
        } else if let Some(block) = &inc.block {
            let body = prepare(
                &contents,
                &target_path,
                inc.lines.as_deref(),
                0,
                None,
                None,
                footnotes,
                None,
            );
            format!("{pad}#+BEGIN_{block}\n{body}{pad}#+END_{block}\n")
        } else {
            let lines = match &inc.location {
                Some(loc) => Some(
                    absolute_lines(&contents, loc, inc.only_contents, inc.lines.as_deref())
                        .map_err(|e| format!("{e} for {target}::{loc}"))?,
                ),
                None => inc.lines.clone(),
            };
            let n = prefixes.len();
            let id = *prefixes.entry(target.clone()).or_insert(n);
            let body = prepare(
                &contents,
                &target_path,
                lines.as_deref(),
                ind,
                inc.minlevel,
                Some(id),
                footnotes,
                file,
            );
            let mut deeper = included.to_vec();
            deeper.push(key);
            expand_in(&body, Some(&target_path), &deeper, footnotes)?
        };
        edits.push((start, line_end, insert));
    }
    let mut out = text.to_string();
    for (s, e, r) in edits.into_iter().rev() {
        out.replace_range(s..e, &r);
    }
    Ok(out)
}

/// `org-escape-code-in-string`.
fn escape_code(s: &str) -> String {
    s.split_inclusive('\n')
        .map(|line| {
            let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
            let rest = line[indent..].trim_start_matches(',');
            if rest.starts_with('*') || rest.starts_with("#+") {
                format!("{},{}", &line[..indent], &line[indent..])
            } else {
                line.to_string()
            }
        })
        .collect()
}

/// `org-export--inclusion-absolute-lines`: the lines of the element that
/// `location` finds (`org-link-search`), as `a-b`.
fn absolute_lines(
    text: &str,
    location: &str,
    only_contents: bool,
    lines: Option<&str>,
) -> Result<String, String> {
    let parse = org_syntax::parse(text);
    let root = parse.syntax();
    let words: Vec<String> = location
        .trim_start_matches('*')
        .split_whitespace()
        .map(str::to_uppercase)
        .collect();
    let same = |s: &str| {
        s.split_whitespace()
            .map(str::to_uppercase)
            .collect::<Vec<_>>()
            == words
    };
    let element = if let Some(id) = location.strip_prefix('#') {
        root.descendants().find(|n| {
            n.kind() == HEADLINE
                && ast::Headline::cast(n.clone()).is_some_and(|h| {
                    h.properties()
                        .iter()
                        .any(|(k, v)| k.eq_ignore_ascii_case("CUSTOM_ID") && v.trim() == id)
                })
        })
    } else {
        let starred = location.starts_with('*');
        let named = (!starred)
            .then(|| {
                root.descendants().find(|n| {
                    (n.kind() == TARGET
                        && ast::Target::cast(n.clone()).is_some_and(|t| same(&t.value())))
                        || ast::affiliated_keywords(n)
                            .any(|k| k.key() == "NAME" && same(&k.value()))
                })
            })
            .flatten();
        named.or_else(|| {
            root.descendants().find(|n| {
                n.kind() == HEADLINE
                    && ast::Headline::cast(n.clone()).is_some_and(|h| same(&h.raw_value()))
            })
        })
    };
    let Some(mut el) = element else {
        return Err(format!("No match for fuzzy expression: {location}"));
    };
    // A target: its element.
    while !el.kind().is_element()
        && let Some(p) = el.parent()
    {
        el = p;
    }
    let (mut beg, end) = match (only_contents, ast::contents_range(&el)) {
        (true, Some(r)) => (usize::from(r.start()), usize::from(r.end())),
        _ => (
            usize::from(el.text_range().start()),
            usize::from(el.text_range().end()),
        ),
    };
    if only_contents && el.kind() == HEADLINE {
        // Past the planning line and the property drawer.
        if let Some(h) = ast::Headline::cast(el.clone()) {
            if let Some(p) = h.planning() {
                beg = beg.max(usize::from(p.syntax().text_range().end()));
            }
            if let Some(d) = h.property_drawer() {
                beg = beg.max(usize::from(d.syntax().text_range().end()));
            }
        }
    }
    let mut region = &text[beg..end];
    let mut offset = beg;
    if let Some(l) = lines {
        let skip = region.len() - region.trim_start_matches([' ', '\t', '\r', '\n']).len();
        let ls = region[..skip].rfind('\n').map_or(0, |i| i + 1);
        offset += ls;
        region = &region[ls..];
        let sub = line_range(region, l);
        region = &text[offset..offset + sub.len()];
    }
    let start_line = text[..offset].matches('\n').count() + 1;
    let count =
        region.matches('\n').count() + usize::from(!region.is_empty() && !region.ends_with('\n'));
    Ok(format!("{start_line}-{}", start_line + count))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(files: &[(&str, &str)]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kalem-include-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for (f, t) in files {
            let p = d.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, t).unwrap();
        }
        d
    }

    /// A file written on Windows, with a byte order mark and CR LF, and
    /// one in Latin-1: included as Emacs reads them.
    #[test]
    fn includes_files_as_emacs_reads_them() {
        // A folder of its own: `dir` is shared with the other test.
        let d = std::env::temp_dir().join(format!("kalem-include-enc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("win.org"), "\u{feff}* Included\r\nSome *bold*\r\n").unwrap();
        std::fs::write(d.join("latin1.org"), b"* Caf\xe9\n").unwrap();
        let main = d.join("main.org");
        let out = expand("#+INCLUDE: \"win.org\"\n", Some(&main)).unwrap();
        assert_eq!(out, "* Included\nSome *bold*\n");
        let out = expand("#+INCLUDE: \"latin1.org\"\n", Some(&main)).unwrap();
        assert_eq!(out, "* Café\n");
    }

    #[test]
    fn includes() {
        let d = dir(&[
            ("a.org", "\n* One\ntext[fn:1]\n** Two\n\n[fn:1] note\n\n"),
            ("code.el", "(message \"hi\")\n* not a heading\n"),
            (
                "sub/b.org",
                "See [[file:pic.png]] and [[./c.org::*X][c]].\n",
            ),
            ("lines.txt", "1\n2\n3\n4\n"),
        ]);
        let main = d.join("main.org");
        let t = "* Top\n#+INCLUDE: \"a.org\"\n";
        let out = expand(t, Some(&main)).unwrap();
        assert_eq!(
            out,
            "* Top\n** One\ntext[fn:-0-1]\n*** Two\n\n[fn:-0-1] note\n"
        );
        let out = expand("#+INCLUDE: code.el src emacs-lisp\n", Some(&main)).unwrap();
        assert_eq!(
            out,
            "#+BEGIN_src emacs-lisp\n(message \"hi\")\n,* not a heading\n#+END_src\n"
        );
        let out = expand("#+INCLUDE: \"sub/b.org\"\n", Some(&main)).unwrap();
        assert_eq!(out, "See [[file:sub/pic.png]] and [[sub/c.org::*X][c]].\n");
        let out = expand(
            "#+INCLUDE: \"lines.txt\" example :lines \"2-4\"\n",
            Some(&main),
        )
        .unwrap();
        assert_eq!(out, "#+BEGIN_example\n2\n3\n#+END_example\n");
        let out = expand("#+INCLUDE: \"a.org::*Two\" :only-contents t\n", Some(&main)).unwrap();
        assert_eq!(out, "[fn:-0-1] note\n");
        assert!(
            expand("#+INCLUDE: \"missing.org\"\n", Some(&main))
                .unwrap_err()
                .contains("Cannot include")
        );
        std::fs::write(d.join("loop.org"), "#+INCLUDE: \"loop.org\"\n").unwrap();
        assert!(
            expand("#+INCLUDE: \"loop.org\"\n", Some(&main))
                .unwrap_err()
                .contains("Recursive")
        );
    }
}
