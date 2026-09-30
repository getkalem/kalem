//! The spike against RFC 0003: every example of the RFC and the three
//! samples parse, format idempotently and keep their model; each recovery
//! rule of §15 and each known ambiguity has a case.

use klm_parser_spike::{Body, Inline, Node, examples, fmt, model, parse};
use serde_json::json;

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn inputs() -> Vec<(String, String)> {
    let rfc = std::fs::read_to_string(root().join("rfcs/0003-kalem-format.md")).unwrap();
    let mut out: Vec<(String, String)> = examples(&rfc)
        .into_iter()
        .map(|(line, src)| (format!("RFC line {line}"), src))
        .collect();
    for name in ["mektup", "makale", "gorevler"] {
        let p = root().join(format!("tests/klm-spec/samples/{name}.klm"));
        out.push((name.to_string(), std::fs::read_to_string(p).unwrap()));
    }
    out
}

#[test]
fn examples_and_samples_round_trip() {
    let inputs = inputs();
    assert!(inputs.len() >= 10, "{}", inputs.len());
    for (name, src) in inputs {
        let doc = parse(&src);
        let once = fmt(&doc);
        assert_eq!(fmt(&parse(&once)), once, "{name}: fmt is not idempotent");
        assert_eq!(
            model(&parse(&once)),
            model(&doc),
            "{name}: fmt changed the model"
        );
    }
}

#[test]
fn samples_are_well_formed_and_canonical() {
    for name in ["mektup", "makale", "gorevler"] {
        let src =
            std::fs::read_to_string(root().join(format!("tests/klm-spec/samples/{name}.klm")))
                .unwrap();
        let doc = parse(&src);
        assert!(doc.diagnostics.is_empty(), "{name}: {:?}", doc.diagnostics);
        assert_eq!(fmt(&doc), src, "{name} is not canonical");
    }
}

#[test]
fn examples_are_canonical() {
    let rfc = std::fs::read_to_string(root().join("rfcs/0003-kalem-format.md")).unwrap();
    for (line, src) in examples(&rfc) {
        // The first example is the syntax's shape, not a document.
        if src.starts_with("\\name") {
            continue;
        }
        let doc = parse(&src);
        assert!(
            doc.diagnostics.is_empty(),
            "RFC line {line}: {:?}",
            doc.diagnostics
        );
        assert_eq!(fmt(&doc), src, "RFC line {line} is not canonical");
    }
}

fn codes(src: &str) -> Vec<&'static str> {
    parse(src).diagnostics.iter().map(|d| d.code).collect()
}

fn blocks(src: &str) -> serde_json::Value {
    model(&parse(src))["blocks"].clone()
}

// §15, one malformed input a rule.

#[test]
fn an_unclosed_inline_command_ends_with_its_paragraph() {
    let src = "A \\b{bold text\n\nNext.\n";
    assert_eq!(codes(src), ["unclosed-inline"]);
    assert_eq!(
        blocks(src),
        json!([
            { "p": ["A ", { "cmd": "b", "inline": ["bold text"] }] },
            { "p": ["Next."] }
        ])
    );
}

#[test]
fn an_unclosed_block_ends_before_the_next_block_at_its_indentation() {
    let src = "\\ul{\n  \\li{a}\n  \\li{b}\n\n\\h1{Next}\n\nText.\n";
    assert_eq!(codes(src), ["unclosed-block"]);
    let b = blocks(src);
    assert_eq!(b[0]["cmd"], "ul");
    assert_eq!(b[0]["blocks"].as_array().unwrap().len(), 2);
    assert_eq!(b[1]["cmd"], "h1");
    assert_eq!(b[2], json!({ "p": ["Text."] }));
}

#[test]
fn braces_decide_before_indentation() {
    // Content not indented is still inside a block whose braces match.
    let src = "\\block[.proof]{\nText.\n\n\\eq{\nx\n}\n}\n\nAfter.\n";
    assert!(codes(src).is_empty(), "{:?}", codes(src));
    let b = blocks(src);
    assert_eq!(b[0]["blocks"].as_array().unwrap().len(), 2);
    assert_eq!(b[1], json!({ "p": ["After."] }));
}

#[test]
fn a_stray_closing_brace_is_text() {
    assert_eq!(codes("a } b\n"), ["stray-brace"]);
    assert_eq!(blocks("a } b\n"), json!([{ "p": ["a } b"] }]));
}

#[test]
fn an_unclosed_dollar_ends_with_its_paragraph() {
    let src = "x $a+b\n\ny\n";
    assert_eq!(codes(src), ["unclosed-math"]);
    assert_eq!(
        blocks(src),
        json!([{ "p": ["x ", { "math": "a+b" }] }, { "p": ["y"] }])
    );
}

#[test]
fn an_unclosed_verbatim_block_ends_at_the_end() {
    let src = "\\code[lang=rust]{\nfn f() {\n\nMore.\n";
    assert_eq!(codes(src), ["unclosed-verbatim"]);
    assert_eq!(blocks(src).as_array().unwrap().len(), 1);
}

#[test]
fn unknown_commands_are_generic() {
    let src = "\\callout{\n  Text.\n}\n\nA \\kbd{Ctrl} key.\n";
    assert_eq!(codes(src), ["unknown-command", "unknown-command"]);
    let b = blocks(src);
    assert_eq!(b[0]["cmd"], "callout");
    assert_eq!(b[0]["blocks"], json!([{ "p": ["Text."] }]));
    assert_eq!(b[1]["p"][1], json!({ "cmd": "kbd", "inline": ["Ctrl"] }));
}

#[test]
fn a_duplicate_id_keeps_the_first() {
    let src = "\\h1[#a]{One}\n\n\\h1[#a]{Two}\n";
    assert_eq!(codes(src), ["duplicate-id"]);
    let b = blocks(src);
    assert_eq!(b[0]["attrs"], json!([{ "id": "a" }]));
    assert!(b[1].get("attrs").is_none());
}

// The known ambiguities.

#[test]
fn a_bracket_after_a_command_name() {
    // `[` right after a name opens attributes; text that starts with `[`
    // after a command without content gets empty braces first.
    let src = "See\\br{}[1] here.\n";
    let doc = parse(src);
    assert!(doc.diagnostics.is_empty());
    assert_eq!(
        model(&doc)["blocks"],
        json!([{ "p": ["See", { "cmd": "br" }, "[1] here."] }])
    );
    assert_eq!(fmt(&doc), src);
    assert_eq!(
        blocks("See\\br[1] here.\n"),
        json!([{ "p": ["See", { "cmd": "br", "attrs": [{ "value": "1" }] }, " here."] }])
    );
}

#[test]
fn letters_after_a_command_name() {
    // `hyphen\shyation` would be the command `shyation`.
    let src = "hyphen\\shy{}ation\n";
    assert!(codes(src).is_empty());
    assert_eq!(fmt(&parse(src)), src);
    assert_eq!(codes("hyphen\\shyation\n"), ["unknown-command"]);
}

#[test]
fn a_bracket_inside_a_quoted_value() {
    let src = "\\link[https://x.org title=\"a]b\"]{x}\n";
    let b = blocks(src);
    assert_eq!(
        b[0]["p"][0]["attrs"],
        json!([{ "value": "https://x.org" }, { "key": "title", "value": "a]b" }])
    );
    assert_eq!(fmt(&parse(src)), src);
}

#[test]
fn timestamps_are_one_value() {
    let src = "\\meta[date=<2026-10-03 Sat 10:00 +1w>]\n\n\\date[[2026-10-03 Sat]]\n";
    let b = blocks(src);
    assert_eq!(
        b[0]["attrs"],
        json!([{ "key": "date", "value": "<2026-10-03 Sat 10:00 +1w>" }])
    );
    assert_eq!(
        b[1]["p"][0]["attrs"],
        json!([{ "value": "[2026-10-03 Sat]" }])
    );
    assert_eq!(fmt(&parse(src)), src);
}

#[test]
fn unbalanced_braces_in_verbatim() {
    let src = "Code \\code{a\\{b} and \\code{f(x) { y }}.\n";
    let doc = parse(src);
    assert!(doc.diagnostics.is_empty(), "{:?}", doc.diagnostics);
    assert_eq!(
        model(&doc)["blocks"][0]["p"],
        json!([
            "Code ",
            { "cmd": "code", "verbatim": "a\\{b" },
            " and ",
            { "cmd": "code", "verbatim": "f(x) { y }" },
            "."
        ])
    );
    assert_eq!(fmt(&doc), src);
}

#[test]
fn dollars_in_text() {
    let src = "It costs \\$5, and $x \\$ y$.\n";
    assert_eq!(
        blocks(src),
        json!([{ "p": ["It costs $5, and ", { "math": "x \\$ y" }, "."] }])
    );
    assert_eq!(fmt(&parse(src)), src);
}

#[test]
fn nested_inline_commands() {
    let src = "\\b{a \\i{b \\code{c}} d}\n";
    assert_eq!(
        blocks(src),
        json!([{ "p": [{ "cmd": "b", "inline": ["a ", { "cmd": "i", "inline": ["b ", { "cmd": "code", "verbatim": "c" }] }, " d"] }] }])
    );
    assert_eq!(fmt(&parse(src)), src);
}

#[test]
fn a_paragraph_on_several_lines_is_one_line() {
    let doc = parse("One\ntwo\n  three.\n");
    assert_eq!(fmt(&doc), "One two three.\n");
    let Node::Paragraph(inl, _) = &doc.blocks[0] else {
        panic!()
    };
    assert!(matches!(&inl[0], Inline::Text(_)));
}

#[test]
fn records_are_lines() {
    let doc = parse("\\h1{A}\n\\props{\n    effort=2h\n  x=1\n}\n");
    let Node::Block(c) = &doc.blocks[1] else {
        panic!()
    };
    assert_eq!(c.body, Body::Lines(vec!["effort=2h".into(), "x=1".into()]));
    assert_eq!(fmt(&doc), "\\h1{A}\n\\props{\n  effort=2h\n  x=1\n}\n");
}
