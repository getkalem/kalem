//! Evaluation of orgize for Kalem (task T0.2, decision D2).
//!
//! Usage: orgize-eval <emacs-dump-dir> <file.org>...
//!
//! Reports, per file and in total:
//! - round-trip equality (`to_org() == input`)
//! - panics on the input and on mutated variants of it
//! - parse throughput
//! - structural agreement with Emacs org-element (Jaccard index over
//!   (type, begin) pairs, elements compared at their post-affiliated start)

use std::collections::{BTreeMap, HashSet};
use std::panic;
use std::path::Path;
use std::time::Instant;

use orgize::{Org, SyntaxKind, SyntaxNode};

fn emacs_type(kind: SyntaxKind) -> Option<&'static str> {
    use SyntaxKind::*;
    Some(match kind {
        HEADLINE => "headline",
        SECTION => "section",
        PARAGRAPH => "paragraph",
        PROPERTY_DRAWER => "property-drawer",
        NODE_PROPERTY => "node-property",
        PLANNING => "planning",
        ORG_TABLE | TABLE_EL => "table",
        ORG_TABLE_RULE_ROW | ORG_TABLE_STANDARD_ROW => "table-row",
        ORG_TABLE_CELL => "table-cell",
        LIST => "plain-list",
        LIST_ITEM => "item",
        DRAWER => "drawer",
        KEYWORD => "keyword",
        BABEL_CALL => "babel-call",
        CLOCK => "clock",
        FN_DEF => "footnote-definition",
        COMMENT => "comment",
        RULE => "horizontal-rule",
        FIXED_WIDTH => "fixed-width",
        DYN_BLOCK => "dynamic-block",
        SPECIAL_BLOCK => "special-block",
        QUOTE_BLOCK => "quote-block",
        CENTER_BLOCK => "center-block",
        VERSE_BLOCK => "verse-block",
        COMMENT_BLOCK => "comment-block",
        EXAMPLE_BLOCK => "example-block",
        EXPORT_BLOCK => "export-block",
        SOURCE_BLOCK => "src-block",
        LATEX_ENVIRONMENT => "latex-environment",
        INLINE_CALL => "inline-babel-call",
        INLINE_SRC => "inline-src-block",
        LINK => "link",
        LINE_BREAK => "line-break",
        COOKIE => "statistics-cookie",
        RADIO_TARGET => "radio-target",
        FN_REF => "footnote-reference",
        LATEX_FRAGMENT => "latex-fragment",
        MACROS => "macro",
        SNIPPET => "export-snippet",
        TARGET => "target",
        BOLD => "bold",
        STRIKE => "strike-through",
        ITALIC => "italic",
        UNDERLINE => "underline",
        VERBATIM => "verbatim",
        CODE => "code",
        ENTITY => "entity",
        SUPERSCRIPT => "superscript",
        SUBSCRIPT => "subscript",
        TIMESTAMP_ACTIVE | TIMESTAMP_INACTIVE | TIMESTAMP_DIARY => "timestamp",
        _ => return None,
    })
}

fn orgize_nodes(node: &SyntaxNode, out: &mut HashSet<(String, u32)>) {
    if let Some(t) = emacs_type(node.kind()) {
        // Skip leading affiliated keywords, like Emacs's :post-affiliated.
        let mut start = u32::from(node.text_range().start());
        for child in node.children_with_tokens() {
            if child.kind() == SyntaxKind::AFFILIATED_KEYWORD {
                start = u32::from(child.text_range().end());
            } else {
                break;
            }
        }
        out.insert((t.to_string(), start));
    }
    for child in node.children() {
        orgize_nodes(&child, out);
    }
}

fn emacs_nodes(v: &serde_json::Value, out: &mut HashSet<(String, u32)>) {
    if let Some(t) = v.get("type").and_then(|t| t.as_str()) {
        let begin = v
            .get("pa")
            .and_then(|p| p.as_u64())
            .or_else(|| v.get("begin").and_then(|b| b.as_u64()))
            .unwrap_or(0);
        out.insert((t.to_string(), begin as u32));
    }
    for key in ["children"] {
        if let Some(arr) = v.get(key).and_then(|c| c.as_array()) {
            for c in arr {
                emacs_nodes(c, out);
            }
        }
    }
    if let Some(sec) = v.get("secondary").and_then(|s| s.as_object()) {
        for arr in sec.values() {
            for c in arr.as_array().into_iter().flatten() {
                emacs_nodes(c, out);
            }
        }
    }
}

/// Small deterministic PRNG so that runs are reproducible.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn floor_char(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn mutate(src: &str, rng: &mut Rng) -> String {
    const SNIPPETS: &[&str] = &[
        "*", "* ", "\n", "#+", "#+BEGIN_SRC", "#+END_SRC", "[[", "]]", "|", "- ", "1. ", ":", "::",
        "<", ">", "[", "]", "=", "~", "/", "+", "_", "^", "{", "}", "\\", "$", "@@", "[fn:",
        "<<", ">>", "\r\n", "\t", " ", ":END:", ":PROPERTIES:", "#+TBLFM:", "src_", "call_",
    ];
    let mut s = src.to_string();
    for _ in 0..(1 + rng.below(4)) {
        if s.is_empty() {
            break;
        }
        let a = floor_char(&s, rng.below(s.len()));
        match rng.below(3) {
            0 => s.insert_str(a, SNIPPETS[rng.below(SNIPPETS.len())]),
            1 => {
                let b = floor_char(&s, (a + rng.below(40)).min(s.len()));
                s.replace_range(a..b, "");
            }
            _ => {
                let b = floor_char(&s, (a + rng.below(200)).min(s.len()));
                let piece = s[a..b].to_string();
                let c = floor_char(&s, rng.below(s.len()));
                s.insert_str(c, &piece);
            }
        }
    }
    s
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dump_dir = Path::new(&args[0]);
    let files = &args[1..];
    panic::set_hook(Box::new(|_| {}));

    let (mut bytes, mut secs) = (0usize, 0f64);
    let (mut rt_ok, mut rt_fail, mut panics) = (0, 0, 0);
    let (mut mut_total, mut mut_panics, mut mut_rt_fail) = (0, 0, 0);
    let mut per_type: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new(); // (both, emacs only, orgize only)
    let mut rng = Rng(0x9E3779B97F4A7C15);

    for f in files {
        let text = match std::fs::read_to_string(f) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let t0 = Instant::now();
        let res = panic::catch_unwind(|| Org::parse(&text));
        let dt = t0.elapsed().as_secs_f64();
        let org = match res {
            Ok(o) => o,
            Err(_) => {
                panics += 1;
                println!("PANIC      {f}");
                continue;
            }
        };
        bytes += text.len();
        secs += dt;
        if org.to_org() == text {
            rt_ok += 1;
        } else {
            rt_fail += 1;
            println!("ROUNDTRIP  {f}");
        }

        // Mutation testing: panics and round-trip on broken inputs.
        for _ in 0..200 {
            let m = mutate(&text, &mut rng);
            mut_total += 1;
            match panic::catch_unwind(|| Org::parse(&m).to_org()) {
                Ok(out) if out == m => {}
                Ok(_) => mut_rt_fail += 1,
                Err(_) => mut_panics += 1,
            }
        }

        // Structural comparison with Emacs.
        let name = Path::new(f).file_name().unwrap().to_string_lossy();
        let dump = dump_dir.join(format!("{name}.json"));
        if let Ok(j) = std::fs::read_to_string(&dump) {
            let v: serde_json::Value = serde_json::from_str(&j).unwrap();
            let mut e = HashSet::new();
            for c in v["children"].as_array().unwrap() {
                emacs_nodes(c, &mut e);
            }
            let mut o = HashSet::new();
            orgize_nodes(&SyntaxNode::new_root(org.green().clone()), &mut o);
            if std::env::var("DIFF").is_ok() {
                let mut v: Vec<_> = e.symmetric_difference(&o).collect();
                v.sort_by_key(|x| x.1);
                for x in v {
                    if std::env::var("DIFF").unwrap() != "" && std::env::var("DIFF").unwrap() != x.0 { continue; }
                    let side = if e.contains(x) { "emacs " } else { "orgize" };
                    let a = x.1 as usize;
                    let ctx: String = text[a..].chars().take(60).collect::<String>().replace('\n', "⏎");
                    println!("{side} {:<20} {:>7} {}", x.0, a, ctx);
                }
            }
            for x in e.union(&o) {
                let entry = per_type.entry(x.0.clone()).or_default();
                match (e.contains(x), o.contains(x)) {
                    (true, true) => entry.0 += 1,
                    (true, false) => entry.1 += 1,
                    _ => entry.2 += 1,
                }
            }
        }
    }

    println!();
    println!("files: {}  bytes: {}", rt_ok + rt_fail + panics, bytes);
    println!("round-trip ok: {rt_ok}  failed: {rt_fail}  panics: {panics}");
    println!("throughput: {:.1} MB/s", bytes as f64 / 1e6 / secs);
    println!("mutations: {mut_total}  panics: {mut_panics}  round-trip failures: {mut_rt_fail}");
    println!();
    println!("{:<22} {:>8} {:>11} {:>11} {:>8}", "type", "both", "emacs only", "orgize only", "jaccard");
    let (mut tb, mut te, mut to) = (0, 0, 0);
    for (t, (b, e, o)) in &per_type {
        tb += b;
        te += e;
        to += o;
        println!("{t:<22} {b:>8} {e:>11} {o:>11} {:>7.1}%", 100.0 * *b as f64 / (b + e + o) as f64);
    }
    println!("{:<22} {tb:>8} {te:>11} {to:>11} {:>7.1}%", "TOTAL", 100.0 * tb as f64 / (tb + te + to) as f64);
}
