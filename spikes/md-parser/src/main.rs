//! Spike for D19 (task T2.7c.1): pulldown-cmark and comrak measured on the
//! GFM specification (which holds CommonMark's examples and GitHub's
//! extensions): HTML agreement with the specification, a byte range for
//! every block and inline node, and speed on 10 MB. `cargo run --release`.

#![allow(clippy::print_stdout, deprecated)]

use std::time::Instant;

/// An example of the specification.
struct Example {
    section: String,
    extension: Option<String>,
    markdown: String,
    html: String,
}

fn examples(spec: &str) -> Vec<Example> {
    let fence = "````````````````````````````````";
    let mut out = Vec::new();
    let mut section = String::new();
    let mut lines = spec.lines();
    while let Some(l) = lines.next() {
        if let Some(h) = l.strip_prefix("## ").or_else(|| l.strip_prefix("# ")) {
            section = h.trim().to_string();
        }
        let Some(rest) = l.strip_prefix(fence) else { continue };
        let rest = rest.trim();
        let Some(ext) = rest.strip_prefix("example") else { continue };
        let ext = ext.trim();
        let mut md = String::new();
        let mut html = String::new();
        let mut in_html = false;
        for l in lines.by_ref() {
            if l.starts_with(fence) {
                break;
            }
            if l == "." && !in_html {
                in_html = true;
                continue;
            }
            let l = l.replace('→', "\t");
            if in_html {
                html.push_str(&l);
                html.push('\n');
            } else {
                md.push_str(&l);
                md.push('\n');
            }
        }
        out.push(Example {
            section: section.clone(),
            extension: (!ext.is_empty()).then(|| ext.to_string()),
            markdown: md,
            html,
        });
    }
    out
}

/// HTML compared as cmark's test runner does, roughly: whitespace between
/// tags and at the ends dropped, self-closing tags and attribute order
/// made alike.
fn normalize(html: &str) -> String {
    // `"` and `&quot;` are the same character in text (cmark's runner
    // compares them so).
    let mut s = html.replace("\r\n", "\n").replace("&quot;", "\"");
    // Void elements with or without the slash.
    s = s.replace(" />", ">").replace("/>", ">");
    // Table alignment as an attribute or a style; an empty body.
    for a in ["left", "center", "right"] {
        s = s.replace(&format!("style=\"text-align: {a}\""), &format!("align=\"{a}\""));
    }
    s = s.replace("<tbody></tbody>", "").replace("<tbody>\n</tbody>", "");
    // Attributes of input elements in a fixed order.
    s = s.replace("<input disabled=\"\" type=\"checkbox\"", "<input type=\"checkbox\" disabled=\"\"");
    s = s.replace("<input checked=\"\" disabled=\"\" type=\"checkbox\"", "<input type=\"checkbox\" checked=\"\" disabled=\"\"");
    s = s.replace("<input type=\"checkbox\" disabled=\"\" checked=\"\"", "<input type=\"checkbox\" checked=\"\" disabled=\"\"");
    let mut out = String::new();
    let mut pending = String::new();
    for c in s.chars() {
        if c.is_whitespace() {
            pending.push(c);
            continue;
        }
        if !pending.is_empty() {
            let after_tag = out.ends_with('>');
            if after_tag && c != '<' {
                // A line break or a space after a tag reads the same.
                out.push(' ');
            } else if !after_tag {
                out.push_str(&pending);
            }
            pending.clear();
        }
        out.push(c);
    }
    out.trim().to_string()
}

/// Whether an example is about GitHub's extensions: labelled with one,
/// or in a section marked "(extension)" (the task lists are not
/// labelled).
fn uses_extensions(e: &Example) -> bool {
    e.extension.as_deref().is_some_and(|x| x != "disabled") || e.section.contains("(extension)")
}

fn pulldown_options(ext: bool) -> pulldown_cmark::Options {
    use pulldown_cmark::Options as O;
    if ext {
        O::ENABLE_TABLES | O::ENABLE_STRIKETHROUGH | O::ENABLE_TASKLISTS | O::ENABLE_GFM
    } else {
        O::empty()
    }
}

fn pulldown_html(md: &str, ext: bool) -> String {
    let mut out = String::new();
    pulldown_cmark::html::push_html(&mut out, pulldown_cmark::Parser::new_ext(md, pulldown_options(ext)));
    out
}

fn comrak_options(ext: bool) -> comrak::Options<'static> {
    let mut o = comrak::Options::default();
    o.render.r#unsafe = true;
    if ext {
        o.extension.table = true;
        o.extension.strikethrough = true;
        o.extension.autolink = true;
        o.extension.tagfilter = true;
        o.extension.tasklist = true;
    }
    o
}

fn comrak_html(md: &str, ext: bool) -> String {
    let arena = comrak::Arena::new();
    let root = comrak::parse_document(&arena, md, &comrak_options(ext));
    let mut out = String::new();
    comrak::format_html(root, &comrak_options(ext), &mut out).unwrap();
    out
}

/// Every pulldown-cmark event's range: inside the text, on character
/// boundaries, and nested in its enclosing element's. The number of
/// events, and the problems.
fn pulldown_ranges(md: &str, ext: bool) -> (usize, Vec<String>) {
    use pulldown_cmark::Event;
    let mut problems = Vec::new();
    let mut stack: Vec<std::ops::Range<usize>> = Vec::new();
    let mut n = 0;
    for (ev, r) in pulldown_cmark::Parser::new_ext(md, pulldown_options(ext)).into_offset_iter() {
        n += 1;
        if r.start > r.end || r.end > md.len() || !md.is_char_boundary(r.start) || !md.is_char_boundary(r.end) {
            problems.push(format!("{ev:?} at {r:?}: outside the text"));
            continue;
        }
        if let Some(p) = stack.last()
            && (r.start < p.start || r.end > p.end)
        {
            problems.push(format!("{ev:?} at {r:?} outside its parent {p:?}"));
        }
        match ev {
            Event::Start(_) => stack.push(r),
            Event::End(_) => {
                stack.pop();
            }
            _ => {}
        }
    }
    (n, problems)
}

/// Byte offsets of the line starts, for comrak's line and column.
fn line_starts(md: &str) -> Vec<usize> {
    std::iter::once(0).chain(md.match_indices('\n').map(|(i, _)| i + 1)).collect()
}

/// Every comrak node's source position, as bytes: inside the text and in
/// its parent's. The number of nodes, those without a position, and the
/// problems.
fn comrak_ranges(md: &str, ext: bool) -> (usize, usize, Vec<String>) {
    let arena = comrak::Arena::new();
    let o = comrak_options(ext);
    let root = comrak::parse_document(&arena, md, &o);
    let starts = line_starts(md);
    let to_byte = |lc: comrak::nodes::LineColumn| -> Option<usize> {
        if lc.line == 0 {
            return None;
        }
        starts.get(lc.line - 1).map(|s| s + lc.column.saturating_sub(1))
    };
    let mut n = 0;
    let mut missing = 0;
    let mut problems = Vec::new();
    for node in root.descendants().skip(1) {
        n += 1;
        let pos = node.data.borrow().sourcepos;
        let (Some(a), Some(b)) = (to_byte(pos.start), to_byte(pos.end)) else {
            missing += 1;
            continue;
        };
        let end = b + 1;
        if a > end || end > md.len() + 1 {
            problems.push(format!("{:?} at {a}..{end}: outside the text", node.data.borrow().value));
            continue;
        }
        if let Some(parent) = node.parent()
            && parent.parent().is_some()
        {
            let pp = parent.data.borrow().sourcepos;
            if let (Some(pa), Some(pb)) = (to_byte(pp.start), to_byte(pp.end))
                && (a < pa || end > pb + 1)
            {
                problems.push(format!(
                    "{:?} at {a}..{end} outside its parent at {pa}..{}",
                    node.data.borrow().value,
                    pb + 1
                ));
            }
        }
    }
    (n, missing, problems)
}

fn main() {
    let spec = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/data/gfm-spec.txt")).unwrap();
    let ex = examples(&spec);
    let core = ex.iter().filter(|e| !uses_extensions(e)).count();
    println!("{} examples: {core} CommonMark, {} GFM extensions\n", ex.len(), ex.len() - core);
    for (name, render) in [
        ("pulldown-cmark 0.13", pulldown_html as fn(&str, bool) -> String),
        ("comrak 0.55", comrak_html),
    ] {
        let mut pass = 0;
        let mut pass_ext = 0;
        let mut fails: std::collections::BTreeMap<String, usize> = Default::default();
        for e in &ex {
            let ext = uses_extensions(e);
            let got = render(&e.markdown, ext);
            if normalize(&got) == normalize(&e.html) {
                if uses_extensions(e) {
                    pass_ext += 1;
                } else {
                    pass += 1;
                }
            } else {
                *fails.entry(e.section.clone()).or_default() += 1;
                if std::env::var("SHOW").is_ok_and(|v| name.starts_with(&v)) {
                    println!("--- {}\n{:?}\nwant {:?}\ngot  {:?}", e.section, e.markdown, normalize(&e.html), normalize(&got));
                }
            }
        }
        println!("{name}: HTML as the specification: {pass}/{core} CommonMark, {pass_ext}/{} extensions", ex.len() - core);
        let worst: Vec<String> = {
            let mut v: Vec<_> = fails.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            v.into_iter().take(6).map(|(s, n)| format!("{s} {n}")).collect()
        };
        println!("  differing sections: {}", worst.join(", "));
    }
    // Ranges.
    let mut events = 0;
    let mut pproblems = Vec::new();
    let mut nodes = 0;
    let mut missing = 0;
    let mut cproblems = Vec::new();
    for e in &ex {
        let ext = uses_extensions(e);
        let (n, p) = pulldown_ranges(&e.markdown, ext);
        events += n;
        pproblems.extend(p);
        let (n, m, p) = comrak_ranges(&e.markdown, ext);
        nodes += n;
        missing += m;
        if std::env::var("SHOWPOS").is_ok() && !p.is_empty() {
            println!("POS {:?}", e.markdown);
        }
        cproblems.extend(p);
    }
    println!("\npulldown-cmark ranges: {events} events, {} problems", pproblems.len());
    for p in pproblems.iter().take(5) {
        println!("  {p}");
    }
    println!("comrak positions: {nodes} nodes, {missing} without a position, {} problems", cproblems.len());
    for p in cproblems.iter().take(5) {
        println!("  {p}");
    }
    // Speed on 10 MB.
    let mut big = String::new();
    while big.len() < 10_000_000 {
        for e in &ex {
            big.push_str(&e.markdown);
            big.push('\n');
        }
    }
    let t = Instant::now();
    let n = pulldown_cmark::Parser::new_ext(&big, pulldown_options(true)).into_offset_iter().count();
    let pd = t.elapsed();
    let t = Instant::now();
    let arena = comrak::Arena::new();
    let root = comrak::parse_document(&arena, &big, &comrak_options(true));
    let cn = root.descendants().count();
    let cm = t.elapsed();
    println!(
        "\n10 MB: pulldown-cmark {pd:?} ({n} events), comrak {cm:?} ({cn} nodes)"
    );
}
