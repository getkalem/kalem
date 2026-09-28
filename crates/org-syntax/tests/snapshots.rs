//! Snapshot tests: the full tree, tokens included, for each element and
//! object type. Review changes with `cargo insta review`.

fn tree(text: &str) -> String {
    let p = org_syntax::parse(text);
    assert_eq!(p.syntax().to_string(), text);
    format!("{:#?}", p.syntax())
}

fn group(examples: &[&str]) -> String {
    examples
        .iter()
        .map(|e| format!("=== {e:?}\n{}", tree(e)))
        .collect::<Vec<_>>()
        .join("\n")
}

macro_rules! snap {
    ($name:ident, [$($e:expr),+ $(,)?]) => {
        #[test]
        fn $name() {
            insta::assert_snapshot!(stringify!($name), group(&[$($e),+]));
        }
    };
}

// Greater elements.
snap!(
    headline,
    [
        "* Title\n",
        "** TODO [#A] COMMENT Title :a:b:\nBody\n",
        "*** DONE Only stars\n\n\n* Next\n"
    ]
);
snap!(
    section,
    [
        "Before headline\n* H\nInside\n",
        "\n\nLeading blank lines\n",
        "* H\n\nAfter blank\n"
    ]
);
snap!(
    planning,
    [
        "* H\nSCHEDULED: <2026-01-01 Thu>\n",
        "* H\nDEADLINE: <2026-01-02 Fri -2d> CLOSED: [2026-01-01 Thu 10:00]\n",
        "* H\n  SCHEDULED: <2026-01-01 Thu +1w>\nText\n"
    ]
);
snap!(
    property_drawer,
    [
        "* H\n:PROPERTIES:\n:ID: x\n:END:\n",
        "* H\nSCHEDULED: <2026-01-01 Thu>\n:PROPERTIES:\n:A+: 1\n:EMPTY:\n:END:\n",
        ":PROPERTIES:\n:TITLE: top\n:END:\n"
    ]
);
snap!(
    drawer,
    [
        ":LOGBOOK:\nCLOCK: [2026-01-01 Thu 10:00]--[2026-01-01 Thu 11:00] =>  1:00\n:END:\n",
        ":NOTE:\ntext\n:END:\n",
        ":EMPTY:\n:END:\n\nafter\n"
    ]
);
snap!(
    plain_list,
    [
        "- a\n- b\n",
        "1. one\n2) two\n   - nested\n",
        "- term :: description\n- [X] done\n- [@3] counter\n"
    ]
);
snap!(
    table,
    [
        "| a | b |\n|---+---|\n| 1 | 2 |\n",
        "| x |\n#+TBLFM: $1=2\n",
        "+---+\n| x |\n+---+\n"
    ]
);
snap!(
    blocks,
    [
        "#+begin_quote\nQuoted *text*.\n#+end_quote\n",
        "#+begin_center\ncentered\n#+end_center\n",
        "#+begin_note params\ninner\n#+end_note\n"
    ]
);
snap!(
    dynamic_block,
    [
        "#+BEGIN: clocktable :scope file\n| x |\n#+END:\n",
        "#+begin: empty\n#+end:\n",
        "#+BEGIN: name\ntext\n#+END\n"
    ]
);
snap!(
    footnote_definition,
    [
        "[fn:1] A note.\n",
        "[fn:label]\n\nOn next lines.\n",
        "[fn:a] one\n[fn:b] two\n"
    ]
);
snap!(
    inlinetask,
    [
        "*************** Task\nBody\n*************** END\n",
        "*************** TODO One line task\n",
        "*************** Empty\n*************** END\n"
    ]
);

// Lesser elements.
snap!(
    paragraph,
    [
        "One line.\n",
        "Two\nlines.\n\nNext paragraph.\n",
        "no final newline"
    ]
);
snap!(
    src_block,
    [
        "#+begin_src rust -n :results output\nfn main() {}\n#+end_src\n",
        "#+BEGIN_SRC\n,* escaped\n#+END_SRC\n",
        "#+begin_src sh\n#+end_src\n"
    ]
);
snap!(
    example_export_comment_verse,
    [
        "#+begin_example -n\nexample\n#+end_example\n",
        "#+begin_export html\n<b>x</b>\n#+end_export\n",
        "#+begin_comment\nhidden\n#+end_comment\n#+begin_verse\n  Verse *line*\n#+end_verse\n"
    ]
);
snap!(
    keyword,
    [
        "#+TITLE: Document\n",
        "#+author:   Name  \n",
        "#+NAME: tbl\n| x |\n#+CAPTION[short]: A *long* caption\n#+ATTR_HTML: :width 50%\n[[file:x.png]]\n"
    ]
);
snap!(
    babel_call,
    [
        "#+CALL: f(x=1)\n",
        "#+call: f[:results raw](y=2)[:exports both]\n",
        "#+CALL: nothing\n"
    ]
);
snap!(
    clock,
    [
        "CLOCK: [2026-01-01 Thu 10:00]--[2026-01-01 Thu 11:30] =>  1:30\n",
        "CLOCK: [2026-01-01 Thu 10:00]\n",
        "  CLOCK: => 2:00\n"
    ]
);
snap!(
    comment_fixed_rule,
    [
        "# a comment\n# second line\n",
        ": fixed\n: width\n",
        "-----\n"
    ]
);
snap!(
    latex_environment,
    [
        "\\begin{equation}\nx^2\n\\end{equation}\n",
        "\\begin{align*}\na &= b\n\\end{align*}\n",
        "\\begin{x} one line \\end{x}\n"
    ]
);
snap!(
    diary_sexp,
    [
        "%%(diary-anniversary 1 1 2000) Birthday\n",
        "%%(org-calendar-holiday)\n",
        "text\n%%(diary)\n"
    ]
);

// Objects.
snap!(
    emphasis,
    [
        "*bold* /italic/ _under_ +strike+\n",
        "=verbatim= and ~code~\n",
        "*nested /italic/ inside*\n"
    ]
);
snap!(
    links,
    [
        "[[https://orgmode.org][Org]]\n",
        "https://example.com/path and <mailto:me@example.com>\n",
        "[[file:a.org::*Head]] [[#custom]] [[(ref)]] [[fuzzy]]\n"
    ]
);
snap!(
    timestamps,
    [
        "<2026-01-01 Thu>\n",
        "[2026-01-01 Thu 10:00-11:00]\n",
        "<2026-01-01 Thu>--<2026-01-03 Sat> <%%(diary-float t 4 2)>\n"
    ]
);
snap!(
    footnote_reference,
    [
        "Text[fn:1].\n",
        "Inline[fn:: definition].\n",
        "Named[fn:n: definition].\n"
    ]
);
snap!(
    latex_fragments_entities,
    [
        "$x^2$ and $$y$$\n",
        "\\(a\\) and \\[b\\] and \\frac{1}{2}\n",
        "\\alpha \\beta{} \\_  spaces\n"
    ]
);
snap!(
    sub_superscript,
    ["x_1 and x^2\n", "x_{12} and x^{ab}\n", "x_(a) and y^*\n"]
);
snap!(
    macros_snippets,
    [
        "{{{title}}}\n",
        "{{{m(a,b\\,c)}}}\n",
        "@@html:<br>@@ and @@latex:\\\\@@\n"
    ]
);
snap!(
    inline_code,
    [
        "src_python{1+1}\n",
        "src_sh[:results raw]{echo x}\n",
        "call_f(x=1) and call_g[:h](y)[:e]\n"
    ]
);
snap!(
    targets_cookies,
    [
        "<<target>> [1/3] [50%]\n",
        "<<<radio>>> and radio again\n",
        "[/] [%]\n"
    ]
);
snap!(
    citations,
    [
        "[cite:@key]\n",
        "[cite/t:see @a p. 3; @b]\n",
        "[cite:prefix; @a; @b; suffix]\n"
    ]
);
snap!(
    line_break,
    [
        "line one \\\\\nline two\n",
        "trailing \\\\  \nnext\n",
        "not \\\\ a break\n"
    ]
);
snap!(
    table_cells,
    ["| *a* | [[x]] | <2026-01-01 Thu> |\n", "|a|b|\n", "|  |\n"]
);
