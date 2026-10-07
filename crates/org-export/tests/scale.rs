//! Export time grows with the document: a footnote, a link to a custom
//! id, an id or a captioned table, and a block whose line numbers go on
//! from the one before cost the same wherever they are. Each used to
//! walk the whole document, so that time grew with the square of their
//! number: a thousand footnotes took ten seconds, optimized.

use std::time::{Duration, Instant};

const N: usize = 1000;

fn document() -> String {
    let mut s = String::from("#+TITLE: Scale\n\n");
    for i in 0..N {
        let j = (i * 7 + 3) % N;
        s.push_str(&format!(
            "* Heading {i}\n:PROPERTIES:\n:CUSTOM_ID: h{i}\n:ID: id-{i}\n:END:\n\
             A note.[fn:n{i}] See [[#h{j}]], [[id:id-{j}]] and [[tbl{j}]]; the note again.[fn:n{i}]\n\n\
             #+CAPTION: Table {i}\n#+NAME: tbl{i}\n| a | {i} |\n\n\
             #+BEGIN_SRC python +n\nx = {i}\n#+END_SRC\n\n"
        ));
    }
    s.push_str("* Footnotes\n");
    for i in 0..N {
        s.push_str(&format!("[fn:n{i}] Note {i}.\n\n"));
    }
    s
}

#[test]
fn a_thousand_footnotes_and_links() {
    let text = document();
    let settings = org_export::Settings {
        body_only: true,
        now: Some("2026-09-28T10:00:00[Europe/Istanbul]".parse().unwrap()),
        ..org_export::Settings::default()
    };
    let backends: [(&str, &dyn org_export::Backend, &str); 4] = [
        ("html", &org_export::Html, ">1000</a></sup>"),
        ("md", &org_export::Markdown, ">1000</a></sup>"),
        (
            "latex",
            &org_export::Latex::default(),
            "\\footnote{Note 999.",
        ),
        ("text", &org_export::Text::default(), "[1000] Note 999."),
    ];
    for (name, backend, last_note) in backends {
        let t = Instant::now();
        let out =
            org_export::export(&text, backend, &settings).unwrap_or_else(|e| panic!("{name}: {e}"));
        let took = t.elapsed();
        assert!(out.contains(last_note), "{name}: no {last_note:?}");
        assert!(took < Duration::from_secs(20), "{name}: {took:?}");
    }
}
