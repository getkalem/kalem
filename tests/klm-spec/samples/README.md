# Samples of the Kalem format

Hand-written documents in the draft syntax of `rfcs/0003-kalem-format.md`, in canonical form (the paper made canonical by the parser spike, RFC appendix B). They are the material of the first experiment (T2.13.1) and the seed of the conformance suite.

| File | Use case | Exercises |
|---|---|---|
| `mektup.klm` | A letter (the Word side) | Styles on paragraphs, `\br`, `\date`, a list, spans, a footnote |
| `makale.klm` | A paper with mathematics (the LaTeX side) | Abstract and macros in `\meta`, inline and display math, theorem and proof blocks, citations, references, a table with formulas and Turkish numbers, a figure |
| `gorevler.klm` | A task notebook (the Org side) | Headings with `todo`, `priority`, `tags`, attached `\props` and `\log`, checklists, typed links, a table with a footer and column formulas |

## The experiment

Commit `278cc55` holds the documents before a session of edits in draft 0.1, `4328c3d` after it; `git diff 278cc55 4328c3d -- tests/klm-spec/samples` shows what a user's changes look like in version control. The next commit revised the samples to draft 0.2 (attached `\props`, column formulas, no empty braces). The findings and the changes they caused are in appendix A of the RFC.

Two branches from `278cc55`, one changing the first paragraph of the letter and one adding a list item and changing the total, merged without conflict.

## The suite

Beside each sample are its model (`.json`) and a plain HTML (`.html`),
written by the parser (`crates/klm-syntax`, grown from the spike of
T2.13.2); the samples
are in canonical form, so they are their own canonical files. The
examples of Part III of the Book are in `../spec/`, one file per example
named by its chapter, each with its model, its canonical form and its
HTML (`cargo run -p klm-syntax -- examples tests/klm-spec/spec
book/part-3/*.org` writes them; `kalem book check` fails on an example
that has no file here). `klm-syntax` is tested against these files; the
HTML files will be replaced by `klm-export`'s.
