//! The parser on documents written for it and on random input: the tree
//! is the text byte for byte, nothing panics, and reparsing after an edit
//! gives the tree of a full parse.

use latex_syntax::{Parse, SyntaxKind, SyntaxNode, TextEdit, parse};
use proptest::prelude::*;

/// The tree in one line per node, tokens as `KIND "text"`.
fn dump(node: &SyntaxNode) -> String {
    fn go(el: latex_syntax::SyntaxElement, depth: usize, out: &mut String) {
        let pad = "  ".repeat(depth);
        match el {
            rowan::NodeOrToken::Node(n) => {
                out.push_str(&format!("{pad}{:?}\n", n.kind()));
                for c in n.children_with_tokens() {
                    go(c, depth + 1, out);
                }
            }
            rowan::NodeOrToken::Token(t) => {
                out.push_str(&format!("{pad}{:?} {:?}\n", t.kind(), t.text()));
            }
        }
    }
    let mut s = String::new();
    go(rowan::NodeOrToken::Node(node.clone()), 0, &mut s);
    s
}

/// The nodes of `kind`, as their text.
fn texts(p: &Parse, kind: SyntaxKind) -> Vec<String> {
    p.syntax()
        .descendants()
        .filter(|n| n.kind() == kind)
        .map(|n| n.text().to_string())
        .collect()
}

#[test]
fn a_small_paper() {
    let text = "\\documentclass[11pt]{article}\n\\usepackage{amsmath}\n\n\\begin{document}\n\\section*{Intro}\\label{s}\nSee \\cite[p.~2]{knuth} and $a^2+b_{i}$, \\emph{very} % note\nwell.\n\n\\begin{itemize}\n\\item[a)] One\n\\item Two\n\\end{itemize}\n\\begin{align}\nx &= \\frac12 \\\\[2pt]\n\\end{align}\n\\begin{verbatim}\n\\end{itemize} {\n\\end{verbatim}\n\\verb|}| \\url{a%b}\n\\end{document}\n";
    let p = parse(text);
    let root = p.syntax();
    assert_eq!(root.text().to_string(), text);
    assert!(p.diagnostics().is_empty(), "{:?}", p.diagnostics());
    let envs: Vec<String> = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::ENVIRONMENT)
        .filter_map(|n| latex_syntax::name(&n))
        .collect();
    assert_eq!(envs, ["document", "itemize", "align", "verbatim"]);
    let cmds: Vec<String> = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::COMMAND)
        .map(|n| n.text().to_string())
        .collect();
    assert!(cmds.contains(&"\\documentclass[11pt]{article}".to_string()));
    assert!(cmds.contains(&"\\section*{Intro}".to_string()));
    assert!(cmds.contains(&"\\cite[p.~2]{knuth}".to_string()));
    assert!(cmds.contains(&"\\item[a)]".to_string()));
    assert!(cmds.contains(&"\\frac12".to_string()));
    assert!(cmds.contains(&"\\\\[2pt]".to_string()));
    assert!(cmds.contains(&"\\url{a%b}".to_string()));
    assert_eq!(texts(&p, SyntaxKind::INLINE_MATH), ["$a^2+b_{i}$"]);
    assert_eq!(texts(&p, SyntaxKind::VERB), ["\\verb|}|"]);
    // The body of `verbatim` is one token.
    let verbatim = root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() == SyntaxKind::VERBATIM)
        .map(|t| t.text().to_string())
        .collect::<Vec<_>>();
    assert!(verbatim.contains(&"\n\\end{itemize} {\n".to_string()));
    // The preamble and the document are paragraphs of the root.
    let top: Vec<SyntaxKind> = root.children_with_tokens().map(|c| c.kind()).collect();
    assert_eq!(
        top,
        [
            SyntaxKind::PARAGRAPH,
            SyntaxKind::PAR_BREAK,
            SyntaxKind::PARAGRAPH
        ]
    );
}

#[test]
fn unbalanced_input_is_closed_and_reported() {
    // A group left open ends at the paragraph.
    let p = parse("\\textbf{bold\n\nnext");
    assert_eq!(texts(&p, SyntaxKind::GROUP), ["{bold"]);
    let msgs: Vec<&str> = p.diagnostics().iter().map(|d| d.message.as_str()).collect();
    assert_eq!(msgs, ["{ is not closed"]);
    // A closed one may span paragraphs, as footnotes do.
    let p = parse("\\footnote{one\n\ntwo}");
    assert_eq!(texts(&p, SyntaxKind::GROUP), ["{one\n\ntwo}"]);
    // An environment left open ends at the next sectioning command.
    let p = parse("\\begin{itemize}\n\\item a\n\\section{B}\ntext\n");
    assert_eq!(
        texts(&p, SyntaxKind::ENVIRONMENT),
        ["\\begin{itemize}\n\\item a\n"]
    );
    assert_eq!(p.diagnostics()[0].message, "\\begin{itemize} is not closed");
    // An `\end` without its `\begin`; the inner environment closes with
    // the outer one.
    let p = parse("\\begin{a}\\begin{b}x\\end{a}\\end{c}");
    assert_eq!(
        p.diagnostics()
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>(),
        ["\\begin{b} is not closed", "\\end{c} without \\begin{c}"]
    );
    // Math left open ends at the paragraph.
    let p = parse("$a+b\n\nc$");
    assert_eq!(texts(&p, SyntaxKind::INLINE_MATH)[0], "$a+b");
    // A single `$` closes inline math; `$$` is display math.
    let p = parse("$a$$b$ and $$c$$");
    assert_eq!(texts(&p, SyntaxKind::INLINE_MATH), ["$a$", "$b$"]);
    assert_eq!(texts(&p, SyntaxKind::DISPLAY_MATH), ["$$c$$"]);
    assert!(p.diagnostics().is_empty());
}

#[test]
fn arguments() {
    let p = parse(
        "\\section\n[short]\n{Long} \\frac\\alpha b \\textbf x \\item [not an argument\n\n\\newcommand{\\R}[1]{\\mathbb{R}^#1}\\def\\x#1#2{#1}\\left( \\right.",
    );
    let cmds: Vec<String> = p
        .syntax()
        .descendants()
        .filter(|n| {
            n.kind() == SyntaxKind::COMMAND && n.parent().unwrap().kind() == SyntaxKind::PARAGRAPH
        })
        .map(|n| n.text().to_string())
        .collect();
    assert_eq!(
        cmds,
        [
            "\\section\n[short]\n{Long}",
            "\\frac\\alpha b",
            "\\textbf x",
            "\\item",
            "\\newcommand{\\R}[1]{\\mathbb{R}^#1}",
            "\\def\\x#1#2{#1}",
            "\\left(",
            "\\right.",
        ]
    );
}

#[test]
fn makeatletter() {
    let p = parse("\\a@b \\makeatletter\\a@b\\makeatother\\a@b");
    let words: Vec<String> = p
        .syntax()
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() == SyntaxKind::CONTROL_WORD)
        .map(|t| t.text().to_string())
        .collect();
    assert_eq!(
        words,
        ["\\a", "\\makeatletter", "\\a@b", "\\makeatother", "\\a"]
    );
}

#[test]
fn deep_nesting() {
    let text = format!("{}x{}", "{".repeat(100_000), "}".repeat(100_000));
    let p = parse(&text);
    assert_eq!(p.syntax().text().to_string().len(), text.len());
    assert!(p.diagnostics()[0].message.contains("nested too deeply"));
    let text = "\\begin{a}".repeat(20_000);
    assert_eq!(parse(&text).syntax().text().to_string(), text);
}

#[test]
fn reparse_is_incremental_in_a_paragraph() {
    let text = "\\begin{document}\nOne paragraph.\n\nTwo \\emph{x}.\n\nThree.\n\\end{document}\n";
    let p = parse(text);
    let at = text.find("Two").unwrap() + 4;
    let edit = TextEdit {
        range: at..at,
        insert: "\\textbf{y} ".into(),
    };
    let new = edit.apply(text);
    let r = p.reparse_incremental(&new, &edit).expect("incremental");
    assert_eq!(r, parse(&new));
    // An edit that closes an environment elsewhere is not.
    let edit = TextEdit {
        range: at..at,
        insert: "\\end{document}".into(),
    };
    let new = edit.apply(text);
    assert!(p.reparse_incremental(&new, &edit).is_none());
    assert_eq!(p.reparse(&new, &edit), parse(&new));
}

/// A paper-like document of about `n` bytes.
fn paper(n: usize) -> String {
    let mut s = String::from("\\documentclass{book}\n\\usepackage{amsmath}\n\n\\begin{document}\n");
    let mut i = 0;
    while s.len() < n {
        if i % 20 == 0 {
            s.push_str(&format!("\\section{{Part {i}}}\\label{{s{i}}}\n\n"));
        }
        s.push_str(&format!(
            "Paragraph {i} cites \\cite[p.~{i}]{{key{i}}} and has $x_{{{i}}}^2 + \\frac{{a}}{{b}}$ in it, \\emph{{some}} \\textbf{{words}} % a comment\nand a second line with more text to read.\n\n"
        ));
        if i % 7 == 0 {
            s.push_str("\\begin{itemize}\n\\item One\n\\item Two \\ref{s0}\n\\end{itemize}\n\n\\begin{equation}\n  E = mc^2 \\label{e}\n\\end{equation}\n\n");
        }
        i += 1;
    }
    s.push_str("\\end{document}\n");
    s
}

#[test]
fn typing_in_a_paper_is_incremental() {
    let pristine = paper(20_000);
    let mut seed = 7u64;
    let mut rand = move |n: usize| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as usize % n.max(1)
    };
    // Typing anywhere in a clean paper: nearly always incremental.
    let p = parse(&pristine);
    let typing = ["a", " ", "\\emph{x}", "$y$", "\n", "b"];
    let mut incremental = 0;
    let rounds = 300;
    for _ in 0..rounds {
        let at = rand(pristine.len());
        let edit = TextEdit {
            range: at..at,
            insert: typing[rand(typing.len())].into(),
        };
        let new = edit.apply(&pristine);
        if let Some(r) = p.reparse_incremental(&new, &edit) {
            incremental += 1;
            assert_eq!(r, parse(&new), "{edit:?}");
        }
    }
    assert!(incremental * 100 > rounds * 85, "{incremental} of {rounds}");
    // Any edits, one after another: always right.
    let inserts = [
        "a",
        " ",
        "\\emph{x}",
        "$y$",
        "\n",
        "\n\n",
        "{",
        "}",
        "\\",
        "%",
        "\\end{itemize}",
        "\\section{N}",
        "\\verb|",
        "\\makeatletter",
    ];
    let mut text = pristine.clone();
    let mut p = parse(&text);
    for _ in 0..400 {
        let start = rand(text.len());
        let end = (start + rand(3)).min(text.len());
        let edit = TextEdit {
            range: start..end,
            insert: inserts[rand(inserts.len())].into(),
        };
        let new = edit.apply(&text);
        let full = parse(&new);
        if let Some(r) = p.reparse_incremental(&new, &edit) {
            assert_eq!(r, full, "{edit:?}");
        }
        text = new;
        p = full;
    }
}

#[test]
fn speed() {
    let text = paper(1_000_000);
    let t = std::time::Instant::now();
    let p = parse(&text);
    let full = t.elapsed();
    let at = text.len() / 2;
    let at = at + text[at..].find("Paragraph").unwrap() + 3;
    let edit = TextEdit {
        range: at..at,
        insert: "x".into(),
    };
    let new = edit.apply(&text);
    let t = std::time::Instant::now();
    let r = p.reparse_incremental(&new, &edit).expect("incremental");
    let key = t.elapsed();
    assert_eq!(r.syntax().text().len(), (new.len() as u32).into());
    // The targets (100 ms and 2 ms) are for release builds; debug builds
    // are several times slower.
    let debug = cfg!(debug_assertions);
    assert!(
        full.as_millis() < if debug { 3000 } else { 100 },
        "{full:?}"
    );
    assert!(
        key.as_micros() < if debug { 50_000 } else { 2000 },
        "{key:?}"
    );
}

/// LaTeX-like pieces for random documents.
fn piece() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("\\begin{itemize}".to_string()),
        Just("\\end{itemize}".to_string()),
        Just("\\begin{verbatim}".to_string()),
        Just("\\end{verbatim}".to_string()),
        Just("\\begin{align}".to_string()),
        Just("\\end{align}".to_string()),
        Just("\\begin{tabular}".to_string()),
        Just("\\end{tabular}".to_string()),
        Just("\\be".to_string()),
        Just("\\ee".to_string()),
        Just("\\def\\bq{\\begin{equation}}".to_string()),
        Just("\\bq".to_string()),
        Just("\\newenvironment{eqn}{\\begin{equation}}{\\end{equation}}".to_string()),
        Just("\\begin{eqn}".to_string()),
        Just("\\end{eqn}".to_string()),
        Just("\\begin{document}".to_string()),
        Just("\\end{document}".to_string()),
        Just("\\section".to_string()),
        Just("\\item".to_string()),
        Just("\\emph".to_string()),
        Just("\\frac".to_string()),
        Just("\\verb|".to_string()),
        Just("\\url{".to_string()),
        Just("\\makeatletter".to_string()),
        Just("\\makeatother".to_string()),
        Just("\\def\\x#1".to_string()),
        Just("\\let\\a\\overline".to_string()),
        Just("\\let\\b=".to_string()),
        Just("\\left".to_string()),
        Just("\\\\".to_string()),
        Just("\\(".to_string()),
        Just("\\)".to_string()),
        Just("\\[".to_string()),
        Just("\\]".to_string()),
        Just("\\a@b".to_string()),
        Just("{".to_string()),
        Just("}".to_string()),
        Just("[".to_string()),
        Just("]".to_string()),
        Just("$".to_string()),
        Just("$$".to_string()),
        Just("%".to_string()),
        Just("&".to_string()),
        Just("#".to_string()),
        Just("*".to_string()),
        Just(" ".to_string()),
        Just("\n".to_string()),
        Just("\n\n".to_string()),
        Just("\r\n".to_string()),
        Just("\\".to_string()),
        Just("é".to_string()),
        "[a-z]{1,4}",
    ]
}

fn document() -> impl Strategy<Value = String> {
    prop::collection::vec(piece(), 0..60).prop_map(|v| v.concat())
}

/// A char boundary of `s` near `n`.
fn boundary(s: &str, n: usize) -> usize {
    let mut n = n.min(s.len());
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    n
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    #[test]
    fn lossless(text in document()) {
        let p = parse(&text);
        prop_assert_eq!(p.syntax().text().to_string(), text);
    }

    #[test]
    fn lossless_on_any_text(text in "\\PC{0,80}") {
        let p = parse(&text);
        prop_assert_eq!(p.syntax().text().to_string(), text);
    }

    #[test]
    fn reparse_matches_a_full_parse(
        text in document(),
        a in 0usize..400,
        len in 0usize..12,
        insert in prop::collection::vec(piece(), 0..3),
    ) {
        let start = boundary(&text, a);
        let end = boundary(&text, start + len).max(start);
        let edit = TextEdit { range: start..end, insert: insert.concat() };
        let new = edit.apply(&text);
        let old = parse(&text);
        let full = parse(&new);
        if let Some(r) = old.reparse_incremental(&new, &edit) {
            prop_assert_eq!(&r, &full, "{}\n---\n{}", dump(&r.syntax()), dump(&full.syntax()));
        }
    }

    #[test]
    fn reparse_in_documents_with_paragraphs(
        paras in prop::collection::vec(document(), 1..6),
        which in 0usize..6,
        a in 0usize..60,
        len in 0usize..6,
        insert in prop::collection::vec(piece(), 0..3),
    ) {
        let text = format!("\\begin{{document}}\n{}\n\\end{{document}}\n", paras.join("\n\n"));
        let para = paras[which % paras.len()].clone();
        let at = text.find(&para).unwrap_or(0);
        let start = boundary(&text, at + a.min(para.len()));
        let end = boundary(&text, start + len).max(start);
        let edit = TextEdit { range: start..end, insert: insert.concat() };
        let new = edit.apply(&text);
        let old = parse(&text);
        let full = parse(&new);
        if let Some(r) = old.reparse_incremental(&new, &edit) {
            prop_assert_eq!(&r, &full, "{}\n---\n{}", dump(&r.syntax()), dump(&full.syntax()));
        }
    }
}

#[test]
fn tokens_of_an_empty_text() {
    let p = parse("");
    let root = p.syntax();
    for pos in [0, 1, 5] {
        assert!(latex_syntax::token_at(&root, pos).is_none());
        assert!(latex_syntax::token_before(&root, pos).is_none());
    }
    let p = parse("a");
    let root = p.syntax();
    assert!(latex_syntax::token_at(&root, 0).is_some());
    assert!(latex_syntax::token_before(&root, 0).is_some());
    assert!(latex_syntax::token_at(&root, 9).is_some());
}

#[test]
fn comment_before_an_argument() {
    // TeX skips a comment and its line ending between a command and its
    // argument.
    let p = parse("\\section% the title\n  {Title} after\n");
    let cmd = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::COMMAND)
        .unwrap();
    assert_eq!(cmd.text().to_string(), "\\section% the title\n  {Title}");
    assert_eq!(
        p.syntax().text().to_string(),
        "\\section% the title\n  {Title} after\n"
    );
}

#[test]
fn formulas_of_a_document_s_own() {
    // `\be … \ee` and an environment defined as an equation are displayed
    // formulas; in a table's cell `$$` is an empty formula.
    let p = parse(
        "\\newenvironment{eqn}{\\begin{equation}}{\\end{equation}}\n\\be x \\ee\n\\begin{eqn}y\\end{eqn}\n\\begin{tabular}{cc}$$ & $a$ \\\\ $$ & b\\end{tabular}\n",
    );
    assert_eq!(
        texts(&p, SyntaxKind::DISPLAY_MATH),
        ["\\be x \\ee", "\\begin{eqn}y\\end{eqn}"]
    );
    assert_eq!(texts(&p, SyntaxKind::INLINE_MATH), ["$$", "$a$", "$$"]);
    // `\\begin{equation} … \\ee`: one environment.
    let p = parse("\\begin{equation}\na\n\\ee\nb\n");
    assert_eq!(
        texts(&p, SyntaxKind::ENVIRONMENT),
        ["\\begin{equation}\na\n\\ee"]
    );
}
