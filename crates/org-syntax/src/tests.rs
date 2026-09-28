use crate::parse;

fn roundtrip(text: &str) {
    let p = parse(text);
    assert_eq!(p.syntax().to_string(), text);
}

#[test]
fn roundtrip_basics() {
    roundtrip("");
    roundtrip("\n\n");
    roundtrip("* Headline\nText\n");
    roundtrip(
        "#+TITLE: T\n\n* TODO [#A] Hello *bold* :a:b:\nSCHEDULED: <2026-09-27 Sun>\n:PROPERTIES:\n:ID: x\n:END:\n\n- [X] one\n- two\n\n| a | b |\n|---+---|\n| 1 | 2 |\n#+TBLFM: $2=$1\n",
    );
    roundtrip("#+begin_src rust\nfn main() {}\n#+end_src\n");
    roundtrip("Text with [[https://orgmode.org][a link]] and $x^2$ and \\alpha.\n");
    roundtrip("[fn:1] A footnote.\n\n* Footnotes\n");
    roundtrip("no final newline");
    roundtrip("\r\n* crlf\r\ntext\r\n");
}

#[test]
fn setupfile_keywords_apply() {
    struct Loader;
    impl crate::SetupFileLoader for Loader {
        fn load(&self, name: &str, _: Option<&str>) -> Option<(String, String)> {
            (name == "setup.org")
                .then(|| ("setup.org".to_string(), "#+TODO: WAIT | GONE\n".to_string()))
        }
    }
    let text = "#+SETUPFILE: \"setup.org\"\n* WAIT task\n";
    let ctx =
        crate::ParseContext::for_document_with(text, &crate::ParseContext::default(), &Loader);
    assert_eq!(ctx.todo_keywords, vec!["WAIT".to_string()]);
    assert_eq!(ctx.done_keywords, vec!["GONE".to_string()]);
    let p = crate::parse_with(text, &ctx);
    let has_todo = p
        .syntax()
        .descendants_with_tokens()
        .any(|e| e.kind() == crate::SyntaxKind::TODO_KEYWORD);
    assert!(has_todo);
}

/// The tree and the typed layer reproduce every boundary the parser
/// computed: contents, post-affiliated and post-blank.
#[test]
fn derived_structure_matches_parser() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus");
    let mut stack = vec![root];
    let mut files = Vec::new();
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p)
            } else if p.extension().is_some_and(|x| x == "org") {
                files.push(p)
            }
        }
    }
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let ctx = crate::ParseContext::for_file(&text, &f, &crate::ParseContext::default());
        let a = crate::debug::raw_structure(&text, &ctx);
        let b = crate::debug::tree_structure(&text, &ctx);
        assert_eq!(a.len(), b.len(), "{}", f.display());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x, y, "{}", f.display());
        }
    }
}

#[test]
fn nesting_beyond_the_cap_stays_lossless() {
    let n = crate::MAX_DEPTH + 500;
    let list: String = (0..n)
        .map(|i| format!("{}- item\n", " ".repeat(i % 2000 + i / 2000)))
        .collect();
    let bold = format!("{}x{}\n", "*".repeat(n), "*".repeat(n));
    let fns = format!("{}x{}\n", "[fn::".repeat(n), "]".repeat(n));
    for text in [list, bold, fns] {
        let p = crate::parse(&text);
        assert_eq!(p.syntax().to_string(), text);
    }
}

#[test]
fn link_newlines_collapse_like_emacs() {
    use crate::ast::{AstNode, Link};
    let text = "[[#a b  \n  c ][d]] [[x\ny ]]\n";
    let p = crate::parse(text);
    let paths: Vec<String> = p
        .syntax()
        .descendants()
        .filter_map(Link::cast)
        .map(|l| l.info(p.context()).path)
        .collect();
    assert_eq!(paths, vec!["a b c ".to_string(), "x y ".to_string()]);
}

#[test]
fn todo_sequences_follow_emacs() {
    use crate::context::TodoSequenceKind;
    let text = "#+TODO: TODO(t) NEXT(n) WAIT(w@/!) | DONE(d!) CANCELLED(c@)\n#+TYP_TODO: Alice Bob | Finished\n#+SEQ_TODO: IDEA | USED\n#+TODO: A B\n";
    let p = crate::parse(text);
    let ctx = p.context();
    let sets: Vec<Vec<&str>> = ctx
        .todo_sequences
        .iter()
        .map(|s| s.keywords.iter().map(|k| k.name.as_str()).collect())
        .collect();
    // `#+TYP_TODO` first, then every `#+TODO`, then `#+SEQ_TODO`.
    assert_eq!(
        sets,
        vec![
            vec!["Alice", "Bob", "Finished"],
            vec!["TODO", "NEXT", "WAIT", "DONE", "CANCELLED"],
            vec!["A", "B"],
            vec!["IDEA", "USED"]
        ]
    );
    assert_eq!(ctx.todo_sequences[0].kind, TodoSequenceKind::Type);
    let wait = &ctx.todo_sequences[1].keywords[2];
    assert_eq!(
        (wait.key, wait.spec.as_deref(), wait.done),
        (Some('w'), Some("w@/!"), false)
    );
    assert_eq!(
        ctx.done_keywords,
        vec!["Finished", "DONE", "CANCELLED", "B", "USED"]
    );
    assert_eq!(
        ctx.todo_keywords,
        vec!["Alice", "Bob", "TODO", "NEXT", "WAIT", "A", "IDEA"]
    );
}

#[test]
fn keywords_include_setup_files() {
    struct Loader;
    impl crate::SetupFileLoader for Loader {
        fn load(&self, name: &str, _: Option<&str>) -> Option<(String, String)> {
            (name == "s.org").then(|| ("s.org".to_string(), "#+FILETAGS: :x:\n".to_string()))
        }
    }
    let text = "#+CATEGORY: c\n#+SETUPFILE: s.org\n#+SETUPFILE: s.org\n* H\n#+PROPERTY: A 1\n";
    let p = crate::parse_with_base(
        text,
        &crate::ParseContext::default(),
        Some(std::sync::Arc::new(Loader)),
    );
    let k: Vec<(String, String)> = p.keywords();
    let k: Vec<(&str, &str)> = k.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    assert_eq!(
        k,
        vec![
            ("CATEGORY", "c"),
            ("FILETAGS", ":x:"),
            ("SETUPFILE", "s.org"),
            ("FILETAGS", ":x:"),
            ("SETUPFILE", "s.org"),
            ("PROPERTY", "A 1"),
        ]
    );
}

#[test]
fn blank_line_before_property_drawer_reparses_it() {
    // A property drawer must follow the planning line directly; inserting
    // a blank line turns it into an ordinary drawer.
    for eol in ["\n", "\r\n"] {
        let text = "* H\nSCHEDULED: <2026-01-01 Thu>\n:PROPERTIES:\n:ID: x\n:END:\n- item\n"
            .replace('\n', eol);
        let at = text.find(":PROPERTIES:").unwrap();
        let edit = crate::TextEdit {
            range: crate::TextRange::new((at as u32).into(), (at as u32).into()),
            insert: eol.to_string(),
        };
        let new_text = edit.apply(&text);
        let inc = crate::parse(&text).reparse(&new_text, &edit);
        assert_eq!(inc.green(), crate::parse(&new_text).green(), "{eol:?}");
    }
}
