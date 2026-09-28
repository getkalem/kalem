//! Property tests on generated documents.
//!
//! Documents are built from Org-like lines (headlines, list items, blocks
//! whose begin and end lines may not match, drawers, tables, keywords) and
//! inline markup, so the generator reaches the parser's edge cases far more
//! often than random bytes would.

use org_syntax::{TextEdit, TextRange, TextSize};
use proptest::prelude::*;

fn inline() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-zA-Zçğışöü]{1,8}",
        Just(" ".to_string()),
        Just("*bold*".to_string()),
        Just("/it/".to_string()),
        Just("=v=".to_string()),
        Just("~c~".to_string()),
        Just("_u_".to_string()),
        Just("+s+".to_string()),
        Just("[[https://x.org][link]]".to_string()),
        Just("https://y.org".to_string()),
        Just("<2026-01-01 Thu>".to_string()),
        Just("[fn:1]".to_string()),
        Just("[fn:: inline]".to_string()),
        Just("$x^2$".to_string()),
        Just("\\alpha".to_string()),
        Just("x_1".to_string()),
        Just("{{{m(a)}}}".to_string()),
        Just("@@html:x@@".to_string()),
        Just("src_sh{echo}".to_string()),
        Just("[cite:@k]".to_string()),
        Just("<<t>>".to_string()),
        Just("[1/2]".to_string()),
        Just("*".to_string()),
        Just("[".to_string()),
        Just("]".to_string()),
        Just("(".to_string()),
    ]
}

fn text_line() -> impl Strategy<Value = String> {
    prop::collection::vec(inline(), 0..8).prop_map(|v| v.concat())
}

fn line() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => text_line(),
        1 => (1usize..4, text_line()).prop_map(|(n, t)| format!("{} {}", "*".repeat(n), t)),
        1 => (1usize..4, text_line()).prop_map(|(n, t)| format!("{} TODO {} :tag:", "*".repeat(n), t)),
        2 => (0usize..6, text_line()).prop_map(|(n, t)| format!("{}- {}", " ".repeat(n), t)),
        1 => (0usize..6, text_line()).prop_map(|(n, t)| format!("{}1. [X] {}", " ".repeat(n), t)),
        1 => Just("#+begin_src sh".to_string()),
        1 => Just("#+end_src".to_string()),
        1 => Just("#+begin_quote".to_string()),
        1 => Just("#+end_quote".to_string()),
        1 => Just(":PROPERTIES:".to_string()),
        1 => Just(":ID: x".to_string()),
        1 => Just(":END:".to_string()),
        1 => Just(":LOGBOOK:".to_string()),
        1 => Just("| a | b |".to_string()),
        1 => Just("|---+---|".to_string()),
        1 => Just("#+TBLFM: $1=2".to_string()),
        1 => Just("#+NAME: n".to_string()),
        1 => Just("#+TITLE: t".to_string()),
        1 => Just("SCHEDULED: <2026-01-01 Thu>".to_string()),
        1 => Just("CLOCK: [2026-01-01 Thu 10:00]".to_string()),
        1 => Just("[fn:1] def".to_string()),
        1 => Just("# comment".to_string()),
        1 => Just(": fixed".to_string()),
        1 => Just("-----".to_string()),
        1 => Just("\\begin{x}".to_string()),
        1 => Just("\\end{x}".to_string()),
        1 => Just("*************** task".to_string()),
        2 => Just(String::new()),
    ]
}

fn document() -> impl Strategy<Value = String> {
    (
        prop::collection::vec(line(), 0..40),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(lines, final_nl, crlf)| {
            let nl = if crlf { "\r\n" } else { "\n" };
            let mut s = lines.join(nl);
            if final_nl {
                s.push_str(nl);
            }
            s
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(512)))]

    #[test]
    fn roundtrip(doc in document()) {
        let p = org_syntax::parse(&doc);
        prop_assert_eq!(p.syntax().to_string(), doc);
        let _ = p.diagnostics();
    }

    #[test]
    fn incremental_equals_full(doc in document(), at in any::<prop::sample::Index>(), len in 0usize..20, insert in line()) {
        let old = org_syntax::parse(&doc);
        let mut a = at.index(doc.len() + 1);
        while !doc.is_char_boundary(a) { a -= 1; }
        let mut b = (a + len).min(doc.len());
        while !doc.is_char_boundary(b) { b -= 1; }
        let edit = TextEdit { range: TextRange::new(TextSize::from(a as u32), TextSize::from(b as u32)), insert };
        let new = edit.apply(&doc);
        let inc = old.reparse(&new, &edit);
        let fresh = org_syntax::parse(&new);
        prop_assert!(inc.green() == fresh.green(), "incremental differs for edit {:?}", edit);
    }
}
