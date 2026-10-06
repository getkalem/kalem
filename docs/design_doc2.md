# Kalem Design Document 2: Standard Modes, the Kalem Format, and the Book

> The Kalem format (`.klm`, RFC 0003, Part III of the Book) was removed on 2026-10-04 by the owner's decision; the sections about it are kept as the design record.

RFC 0002. Status: **accepted by the owner, 2026-09-30.** Sections 11, 12 and 14 are carried into RFC 0001 (`design_document.md`), RFC 0003 (`rfcs/0003-kalem-format.md`) and `todo.md`. Written as a report: it records the roadmap the owner stated on 2026-09-30, examines it, and proposes how to carry it out.

## Contents

1. The roadmap in one page
2. Pillar A: the standard modes, fully faithful
3. Pillar B: the Kalem format, redesigned
4. The case for and against a format of our own
5. Design principles of the Kalem format
6. What the format takes from each ancestor
7. A sketch of the format
8. Styles, layout and typesetting
9. Conversion and coexistence
10. Pillar C: the Book
11. Roadmap and phases
12. Changes to RFC 0001
13. Risks and mitigations
14. Open decisions
15. Recommendation

---

## 1. The roadmap in one page

The owner's direction, 2026-09-30:

- **Standard modes are standard.** Org, Markdown, CSV and LaTeX conform completely to their standards. Kalem renders and edits them, never extends them. A standard file saved by Kalem differs from the original only where the user edited it.
- **One native mode: the Kalem format, `.klm`.** Not a continuation of Org. A text-based, rendered format with its own complete specification and implementation, uniform and self-consistent, taking the strongest parts of Org, Markdown, LaTeX and the other text formats: usable for every kind of writing as Word is, structured and flexible as Org is, at least as good as LaTeX for mathematics, and fit for typesetting, from a note to a printed book.
- **The Book.** One source of truth for the application, published as a book on GitHub Pages: the manual, the specification of every supported format as Kalem implements it, and the complete Kalem format specification.

The rest of this document turns those three sentences into a plan.

---

## 2. Pillar A: the standard modes, fully faithful

"Fully faithful" is measurable only against an oracle. Each standard mode has one, and the mode is done when the oracle agrees.

| Mode | Standard | Oracle | State |
|---|---|---|---|
| Org | The Org Syntax document as `org-element.el` implements it, Org 9.7 | Differential tests against Emacs on the corpus and on mutated inputs; editing commands against Emacs; exporters byte for byte against `ox.el` | Parser, model, editing, tables, HTML and Markdown export at 100% |
| Markdown | CommonMark plus the GitHub extensions | The CommonMark and GFM specification test suites (every example in the specs is a test), and `cmark-gfm` as the reference implementation | Planned, 2.7c |
| CSV | RFC 4180 and the dialects in the wild | Round trip on a corpus of files written by Excel, LibreOffice Calc and Google Sheets in several locales; RFC 4180 edge cases | Done, 2.7d (the Book, Part II, "CSV") |
| LaTeX | Standard LaTeX as the engines accept it | Byte-exact round trip; structural agreement with pandoc's LaTeX reader on an arXiv corpus; compile-and-compare with tectonic; KaTeX for math; the PDF the authors published as the ground truth for numbering, references, citations and the typeset formulas, on thousands of documents (T2.7h.34a, owner, 2026-09-30) | In progress, 2.7h |

Three rules bind every standard mode, and they already hold for Org:

1. **Ranges, never text.** The mode returns ranges into the file; it never regenerates the file from a tree.
2. **No extension.** Kalem writes nothing into a standard file that the standard does not define. The word-processor formatting of RFC 0001 section 3.7 leaves Org entirely (section 12 below) and lives in the Kalem format.
3. **Unknown constructs stay visible.** What a mode does not understand is shown as source, highlighted, never hidden or guessed.

The standard modes are the traction engine of the product: they are what a new user opens first, and their fidelity is the credibility the Kalem format borrows. They are finished first, and their pace does not slow down for the format.

---

## 3. Pillar B: the Kalem format, redesigned

RFC 0001 (3.7, D24) defined `.klm` as Org plus Kalem's additions written through Org's extension points: export snippets for spans, attribute lines for alignment, a keyword for document defaults. That was the cheapest way to add Word-like formatting without inventing syntax.

The owner now asks for something else: **a format designed from scratch, with its own specification**, that is not bound by Org's syntax and takes the best of every text format instead. This document accepts that direction and asks how to do it well. The redefinition is possible without pain because the project has no users and no `.klm` files in the wild yet: the interim `.klm` of D24 is retired before any release (section 12).

What the Kalem format must be, in the owner's words, restated as requirements:

| Requirement | Meaning |
|---|---|
| Text-based, rendered | Plain text in a file, edited as it looks in Kalem, readable raw in any editor |
| Its own specification | A written, versioned, testable grammar and semantics; the Book carries it |
| Uniform | One way to say each thing; one attribute syntax; one block syntax; no synonyms and no legacy |
| Every kind of writing | Notes, letters, reports, theses, books, slides, a shopping list: the Word range |
| Org's flexibility | Outline, tasks, tags, properties, timestamps, agenda, tables with formulas, footnotes, typed links, source blocks, drawers' role |
| LaTeX-grade mathematics | Math is first class: inline and display, numbered, labeled, referenced, theorem-like blocks; the notation inside math is LaTeX's, because that is the notation scientists know |
| Typesetting | Page-quality output: PDF for print, with styles, page layout, headers and footers, figures and tables that float or do not |
| All of Org's good features | Nothing a Kalem user has in an Org file is lost by moving to `.klm` |

---

## 4. The case for and against a format of our own

The report owes the owner an honest account of both sides.

**For.**

- Org's syntax is a twenty-year accretion. Emphasis is defined by a regexp with word-boundary quirks; there are three list bullets, two plain-list numbering styles, `#+` keywords, drawers and properties as three different ways to attach data; links are `[[target][text]]`, tables `|`-drawn, blocks `#+BEGIN_SRC … #+END_SRC`, footnotes `[fn:1]`, math delegated to raw LaTeX fragments. Uniformity is not achievable inside it.
- Word-like formatting on Org's extension points produces ugly markup: `@@kalem:font="Georgia" size=14@@ text @@kalem:end@@`. It works, but nobody would choose to write it, and it marks the file as second class in Emacs.
- The Word use case needs semantics Org never had: named styles, spans with attributes, page directives, a stylesheet. Adding them to Org means a dialect; adding them to a new format means a design.
- A format with a specification and a conformance suite can be implemented by others. Org has one implementation that matters; the Kalem format can have many, starting with Kalem's own and a WASM plugin's.
- Kalem already owns every piece a new format needs: the lossless range-based engine, the rendering pipeline, the math renderer, the table formula engine, the export engine and the terminal renderer. A format is the one thing missing to make them coherent.

**Against.**

- **A new format is an island on day one.** No other tool reads it, no site renders it, no colleague's editor opens it rendered. Kalem's promise "your files are plain text everyone can read" weakens to "your files are plain text you can read raw". Mitigation: readability raw as a design principle, lossless conversion to Org and export to every format Kalem exports (section 9), and an open specification with a public test suite.
- **Designing a format is slow and easy to get wrong.** CommonMark took years to pin down Markdown; Org's ambiguities still surface; Typst's team designed for years before 0.1. A spec written in a week will be rewritten three times. Mitigation: a small core, a formal grammar, an RFC process with prototypes, and the Book as the forcing function (the spec is written by writing the spec in it).
- **Typst already exists.** Typst is a modern markup and typesetting language with excellent math and page layout. The honest question is why not adopt it as the Kalem format. Answer: Typst is a programming language compiled to PDF, source first, editor agnostic, without Org's outline, tasks, agenda, properties or table formulas, and its scripting makes lossless rendered editing hard (a document can compute its own structure). The Kalem format is an editing-first document format with Org's structure and LaTeX math, and it uses Typst as an export engine. The two are complementary, and the report recommends saying so loudly, because the comparison will be the first question asked.
- **Focus.** The format competes for the same months as Markdown mode, LaTeX mode and the plugin runtime. Mitigation: the standard modes stay first (section 11), and the format's first milestone is a specification and a prototype parser, not a product.
- **Reception.** "Yet another markup language" is a reflex on every forum. Mitigation: never lead with the format. Lead with the standard modes; introduce the format as "the document format of Kalem, for those who want more than Markdown and less pain than LaTeX", and show it through the Book.

**The report's judgement.** The direction is sound if four conditions hold: the standard modes stay complete and first; the specification comes before the implementation and stays small; conversion to and from Org is lossless for everything Org can express; and the Book is written in the format, so that the format is used seriously before anyone else is asked to use it. Under those conditions the Kalem format is the piece that turns Kalem's engines into a product of its own rather than a viewer of other people's formats.

---

## 5. Design principles of the Kalem format

The constitution of the specification. Every later syntax decision is tested against these.

1. **Readable raw.** A `.klm` file makes sense in `cat`, in an email, in a diff. Markup is light where text dominates and explicit where structure dominates.
2. **One way to say a thing.** One emphasis marker per meaning, one list bullet, one block syntax, one inline syntax, one attribute grammar used everywhere. No synonyms, no "also accepts".
3. **A formal grammar and an executable specification.** The grammar is written down (PEG or EBNF); every example in the specification is a test in the conformance suite, as CommonMark does; no construct is defined by a regexp in prose. Every input parses: error tolerance is specified, not accidental.
4. **Lossless and incremental by construction.** Block structure is decided by line prefixes and fences, so a block can be reparsed alone; no construct's meaning depends on text arbitrarily far away; the parser returns ranges, and the editor never regenerates text (RFC 0001, 4.1 principle 1).
5. **Structure first.** Headings with outline semantics; tasks, tags, priorities, properties and timestamps as first-class, attached to headings and list items with the one attribute grammar; the agenda and the query language work over `.klm` as over Org.
6. **Mathematics first class, notation borrowed.** Inline and display math are native syntax; the notation inside is LaTeX mathematics (the subset KaTeX renders, plus document macros). Kalem does not invent a math notation: scientists have one.
7. **Styling and layout are semantics, not appearance.** A document is content plus a stylesheet. Text carries named styles and spans with attributes; the stylesheet says what a style looks like and how pages are laid out. Fonts, colors and sizes are allowed inline, but the specification steers toward styles, as Word's styles and CSS do.
8. **Typesetting-grade output.** Every construct has a defined rendering in HTML, in PDF through Typst, in LaTeX and in DOCX; page layout, headers and footers, footnotes, floats, cross references and bibliographies are specified, not left to the exporter's taste.
9. **Extensible without syntax growth.** Generic blocks, generic spans and namespaced attributes are the only extension points; plugins add meaning to them, never syntax. The same principle as RFC 0001 section 11.0, designed in from the start instead of borrowed from Org.
10. **Small core, versioned.** A document declares its format version. The core is the smallest set that covers the requirements; everything else is an extension with a namespace. Breaking changes need a major version and a migration tool.
11. **Convertible.** Import from Org is complete; export to Org drops only what Org cannot express, and says what it dropped. Import from Markdown and LaTeX covers what those formats mean. Nothing enters the core that cannot be exported to at least HTML and PDF.
12. **Everyone's languages.** Unicode throughout, bidirectional text, CJK, Turkish casing, hyphenation and quotes by language; semantic structure that screen readers can follow.

---

## 6. What the format takes from each ancestor

| From | Taken | Left behind |
|---|---|---|
| Org | The outline model; TODO states, priorities, tags, properties, timestamps, repeaters, scheduling; tables with formulas; footnotes; typed links (`id:`, `file:`, custom types); source blocks with execution semantics; the agenda; the export options model; style inference for generated text | `#+` keywords, drawers, three bullets, emphasis regexps, `[[..][..]]` links, `#+BEGIN` verbosity, TBLFM's Calc notation |
| Markdown (CommonMark, GFM) and Djot | Light inline markup; `#` headings; fenced code; pipe tables; `[text](target)` links; task list items; a specification with executable examples; Djot's uniform attribute syntax `{.class key=value}` on blocks and spans | Markdown's ambiguity, synonyms and HTML fallback |
| LaTeX | Math notation and environments' meaning (numbered equations, labels, `\ref`); theorem-like blocks; floats with captions; citations and bibliographies; cross references; the notion of a document class | The macro language, the preamble, the compile step as the only way to see the result |
| Typst | Set rules as the shape of a stylesheet; page layout as declarative directives; a fast embeddable PDF engine as the export target; the lesson that a small, uniform syntax can carry typesetting | Scripting; computing document structure at compile time |
| Word | Named styles applied to paragraphs and spans; direct formatting when a style is overkill; page setup, headers and footers, page breaks, sections; comments and, later, suggestions | Binary files, layout baked into content, no structure |
| CSV and spreadsheets | A1 cell references and the common function names in table formulas, so a formula reads as a spreadsheet user expects | Cell-level formatting as data |

---

## 7. A sketch of the format

**Illustrative, not normative.** The specification (section 10, Part III of the Book) decides every marker after the RFC process of section 14. The sketch exists so that the principles can be judged on something concrete.

```
klm 1.0
title: On the Shape of Notes
author: Ayşe Yılmaz
lang: tr
style: article.klms

# Introduction {#intro}

Plain text is readable raw, with *strong*, _emphasis_, `code`, a [link](https://kalem.dev),
a footnote[^1], inline math $e^{i\pi} + 1 = 0$, a citation [@knuth1984] and a
span with a style [like this]{.warning}. A cross reference to @fig:shape and to
equation @eq:euler.

[^1]: Footnotes are defined where Markdown defines them.

## TODO Write the second section :writing:urgent:
  scheduled: <2026-10-03 Sat>
  priority: A
  effort: 2h

A heading can carry a task state, tags and properties; the agenda reads them as it
reads Org.

- A list item
- [ ] A task item
  - nested, with the one bullet
1. An ordered item

| Item     | Qty | Price | Total    |
|----------|----:|------:|---------:|
| Paper    |   3 |  4.50 |          |
| Ink      |   1 | 12.00 |          |
= D2:D3 = B * C
= D4 = SUM(D2:D3)

$$
\int_0^1 x^2 \, dx = \frac{1}{3}
$$ {#eq:euler}

::: figure {#fig:shape width=60%}
![The shape of a note](shape.png)
:::

::: theorem {title="Pythagoras"}
In a right triangle, $a^2 + b^2 = c^2$.
:::

```rust {exec=false}
fn main() { println!("code blocks carry a language and attributes"); }
```

::: page-break
```

What the sketch shows: one heading marker; one bullet; task states, tags and properties on headings and items with one indented `key: value` grammar; Djot-style attributes `{…}` on blocks, spans, equations and figures; LaTeX math inside `$…$` and `$$…$$`; generic fenced blocks `:::` for figures, theorems, page breaks and anything a plugin adds; a table with spreadsheet-style formulas below it; front matter as plain `key: value` lines under a version line; typographic output decided by a stylesheet, not by the text.

What the sketch leaves open, deliberately: the exact markers (section 14).

---

## 8. Styles, layout and typesetting

The Word requirement is met by separating content from presentation and giving presentation a first-class, versioned language of its own.

- **Styles.** A style is a named set of properties (font, size, spacing, color, alignment, numbering, keep-with-next…). Paragraph styles apply to blocks, character styles to spans, through the attribute grammar (`{.quote}`, `{.warning}`). Built-in styles cover the semantic elements (headings by level, body, code, caption, footnote); a document may define its own.
- **Stylesheets.** `.klms` files, or a `styles` block in the document, in a small declarative language: rules that set properties for elements and styles, in the spirit of Typst's set rules and CSS's cascade but without selectors' full generality. A document names one stylesheet; templates are stylesheets plus front matter.
- **Direct formatting.** Allowed on spans and blocks (`{color=#c00 size=14pt}`) for the letter that needs one red word; the editor's toolbar writes it; the specification documents why a style is usually better.
- **Layout.** Page size, margins, orientation, columns, headers and footers with fields (page number, title, date), page and section breaks, widow and orphan control, footnote placement, float placement rules: declared in the stylesheet or as directives, never inferred from the content.
- **Typesetting engine.** PDF through Typst embedded as a crate (fast, no external installation, good typography), with LaTeX as the alternative for those who want it; HTML with the stylesheet compiled to CSS; DOCX through a mapping of styles to Word styles, so a `.klm` written with styles becomes a well-formed Word document rather than a soup of direct formatting.
- **Preview.** The editor shows the rendered document; a page view (the typeset pages, as the Typst engine lays them out) is a panel, not the editing surface, so editing stays fast and the paged result stays honest.

---

## 9. Conversion and coexistence

- **Org to Kalem format: complete.** Every Org construct maps; the converter is tested on the Org corpus by converting, rendering and comparing the models. A user who wants to move a notebook moves it in one command and loses nothing.
- **Kalem format to Org: lossy where Org has no word, and explicit.** Styles, spans, layout and the formula dialect either map to Org's extension points (as D24 did) or are dropped with a report. This is how a `.klm` reaches an Emacs co-author when it must.
- **Markdown and LaTeX to Kalem format:** import through the standard modes' trees; the reverse through the export engine (Markdown output, LaTeX output).
- **Coexistence in one workspace.** A project mixes `.org`, `.md`, `.tex` and `.klm` files. The agenda, search, links and the outline work across them; a `.klm` may link to an Org heading by ID and the reverse.
- **The standard modes stay strict.** Nothing of the Kalem format leaks into them. The formatting toolbar in a `.org` or `.md` file offers only what that format has, and offers conversion to `.klm` for the rest.

---

## 10. Pillar C: the Book

The Book is the one source of truth for the application, published as a website with the shape of a book at `getkalem.github.io/kalem` (a custom domain later). It replaces the scattered documents of today (`docs/manual.org`, the design documents, the decision records, the known-differences files) as the place a reader is sent.

**Structure.**

| Part | Contents | Nature |
|---|---|---|
| I. Kalem | Installing; the graphical and terminal editors; modes; commands, keys and settings; projects and the file manager; export; the command line; troubleshooting | Manual, informative |
| II. The standard formats as Kalem implements them | Org: the Org Syntax as implemented, with the known differences; Markdown: CommonMark and GFM as implemented; CSV: dialects; LaTeX: the rendered subset, what stays source, the known differences; BibTeX | Reference, informative, with the oracle named for each |
| III. The Kalem format | The specification: principles, grammar, semantics of every construct, the stylesheet language, rendering and export mappings, conformance, versioning and migration | **Normative** |
| IV. Extending Kalem | The plugin model, the WIT API, writing a mode, a highlighter, a completer, an exporter; the plugin repository | Reference |
| V. Design | The design documents (RFC 0001, this document), the decision records, performance, terminal parity, the roadmap | Informative |
| Appendices | Keymaps, settings, CLI reference, glossary, licenses | Reference |

**Rules.**

- **Written in the Kalem format.** The Book is the first large document in `.klm`, and its specification is written in the format it specifies. Until the format exists, chapters are written in Org (as the manual is today) and converted with the Org importer when it lands: the conversion of the Book is itself a test of the importer.
- **Built by Kalem.** `kalem export --to html` over the chapters, a small static site template for navigation, search and the two themes, no third-party book generator in the long run. mdBook may bridge the first months; it is not the destination.
- **Examples are tests.** Every example in Part III is a case in the conformance suite (`tests/klm-spec/`), and the Book is generated from the suite's files, so the specification cannot drift from the tests. Part II does the same with the corpus tests where a format has a suite.
- **Normative and informative are marked.** Part III uses the RFC 2119 words; everything else describes.
- **Versioned with the software.** The Book at `main` describes `main`; a release tags the Book; the specification carries its own version independent of the application's.
- **Published by CI** to GitHub Pages on every merge to `main`, from the `book/` directory of `getkalem/kalem`, so that code and documentation change in one pull request.
- **English canonical.** A Turkish translation of Part I follows when the manual settles; Part III stays English only, as specifications do.

---

## 11. Roadmap and phases

The format does not delay the standard modes. It runs as its own track, gated by specification milestones rather than by dates.

| Track | Phase | Deliverable | Gate |
|---|---|---|---|
| A. Standard modes | 2 (current) | Markdown, CSV, LaTeX modes complete against their oracles; plugin contracts proven by them | RFC 0001 phase 2 exit criteria |
| B. Kalem format | K0, now, in parallel | RFC 0003: the specification draft (grammar, semantics, stylesheet language), a throwaway parser that runs the draft's examples, three real documents written in it (a letter, a paper with math, a chapter of the Book), decisions of section 14 closed | The owner accepts RFC 0003 |
| B | K1 | `klm-syntax` and `klm-model` crates on the mode contract; the conformance suite; lossless round trip and incremental parsing at Org's speed; the Org importer complete; rendering in both frontends | Every spec example passes; the Org corpus converts and renders |
| B | K2 | Styles and stylesheets; layout directives; Typst export and PDF; HTML with compiled CSS; DOCX with styles; the formatting toolbar writing styles | A letter, a paper and a thesis typeset from `.klm` look right on paper |
| B | K3 | Tasks, agenda, table formulas, source blocks and footnotes at Org's level; export to Org with its report; specification 1.0 frozen | The Book's chapters are `.klm`; the agenda runs over a mixed workspace |
| C. The Book | now | `book/` with Parts I, II, IV and V from today's documents, published by CI (mdBook as the bridge) | The site is live and replaces `docs/` as the reference |
| C | with K3 | Part III from the conformance suite; the Book converted to `.klm` and built by Kalem | No third-party generator; examples are tests |

Effort, honestly: K0 is two to three months of design if the decisions of section 14 are taken quickly; K1 and K2 are each the size of Markdown mode; K3 is smaller because it reuses Org's model and engines. The whole track is a year of one developer's attention at today's pace, alongside the standard modes.

---

## 12. Changes to RFC 0001

To take effect when the owner accepts this document:

| RFC 0001 | Change |
|---|---|
| 3.7 Kalem's own features beyond Org, and D24 | Retired. Org files are strict Org, as decided; `.klm` is no longer Org plus additions but the format of this document. The `@@kalem:…@@`, `#+ATTR_KALEM:` and `#+KALEM:` mechanisms are removed before the first release; the word-processor formatting of 2.2a is reimplemented as styles and spans of the Kalem format |
| 1.4 Non-goals | "Page layout editor" is narrowed: layout is a property of the Kalem format's stylesheet and its typeset output, still not of the editing surface. "Microsoft Office compatibility" stays a non-goal; DOCX export with styles is an export target, not a native format |
| 2.6 Modes | The Kalem format is a fifth core mode with its own parser crates; it is not a subtype of Org for command scope (11.2): `klm` is its own text type |
| 11.0 "plugins extend semantics, not syntax" | Unchanged for the standard modes and for plugins; the Kalem format is the one place where Kalem itself defines syntax, and it does so in a specification |
| 18.2 Documentation with mdBook | Replaced by section 10: the Book, built by Kalem, mdBook only as a bridge |
| D21 Positioning | Closed: Kalem edits the standard plain-text formats faithfully and has a document format of its own for everything they cannot do; scientific writing, notes and tasks, documentation and printed documents are the four use cases |
| 4.2 Crate map | `klm-syntax`, `klm-model`, `klm-style` (the stylesheet language), `klm-export` (or the generalization of `org-export` to both trees) |
| Roadmap section 20 | Track B and Track C of section 11 added |

---

## 13. Risks and mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| The format becomes an island nobody else reads | High | High | Readable raw; complete Org import and explicit Org export; HTML, PDF, DOCX export; an open specification with a public suite; the standard modes as the front door |
| Specification churn and bikeshedding | High | Medium | A small core; the twelve principles as the tie-breaker; decisions taken in RFC 0003 with prototypes, not in chat; a freeze at 1.0 with a migration tool for breaking changes |
| "Why not Typst" and "yet another markup" | High | Medium | A clear answer written once (section 4) and repeated in the Book: editing-first, Org's structure, LaTeX's math, Typst as the engine |
| Focus drawn from the standard modes | Medium | High | Track A first; the format's first milestone is a document, not a product; the same person does not context-switch weekly, the tracks alternate by month |
| The stylesheet language grows into a programming language | Medium | Medium | Declarative only; no variables beyond named styles; no computation; anything more goes to Typst templates at export |
| Lossless editing breaks on constructs that span blocks | Low | High | Principle 4: no long-range constructs; footnote definitions and references are the only cross-block link, resolved in the model as Org does |
| The Book lags the software | Medium | Medium | Examples are tests; the Book is built in CI on every merge; a pull request that changes behavior changes the Book |

---

## 14. Open decisions

To be closed in RFC 0003 with prototypes; listed so that they are not decided by accident.

| ID | Decision | Options | Report's leaning |
|---|---|---|---|
| D31 | Heading marker | `#` (Markdown), `*` (Org), `=` (Typst) | `#`: known by the most people; `*` conflicts with emphasis |
| D32 | Emphasis markers | `*strong* _emphasis_` (Djot), `**strong** *emphasis*` (Markdown), Org's set | Djot's: one character per meaning |
| D33 | Attribute syntax | `{.class key=value}` after the element (Djot, Pandoc); `@key(value)`; Typst-like `#set` | Djot's: one grammar for blocks, spans, equations, figures |
| D34 | Generic block fence | `:::` (Djot, Pandoc); `` ``` `` for everything; indentation | `:::` for semantic blocks, `` ``` `` for code only |
| D35 | Front matter | `key: value` lines under a version line; TOML; YAML | Plain `key: value` with a `klm 1.0` first line; TOML for stylesheets |
| D36 | Tasks and properties on headings | Org's `TODO` keyword and `:tags:` on the heading line plus indented `key: value` properties; attributes in `{…}` | Org's line shape for keyword and tags (readable, agenda-tested), indented properties |
| D37 | Table formulas | Spreadsheet dialect (A1, `SUM`) in `=` lines under the table; Org's TBLFM; both | Spreadsheet dialect, with Org's on import |
| D38 | Math delimiters and blocks | `$…$` and `$$…$$` with `{…}` attributes; a `math` fenced block | `$` family; environments named by attribute (`{env=align}`) |
| D39 | Stylesheet language | Declarative rules (CSS-like properties, no selectors beyond element and style names); Typst set rules; TOML tables | Declarative rules in TOML-shaped files, compiled to Typst and CSS |
| D40 | Layout directives | In the stylesheet only; also inline (`::: page-break`) | Both: setup in the stylesheet, breaks and sections inline |
| D41 | Comments | `%` (LaTeX), `//`, `#` at line start (Org) | A dedicated block `::: comment` and an inline `{% … %}`; no line-start comment that collides with headings |
| D42 | Includes and macros | `@include`, `{{name}}` substitution macros, none | Includes yes; substitution macros yes, no computation |
| D43 | Executable blocks | Org Babel semantics with the same trust model | Yes, by attribute (`{exec}`), never automatic |
| D44 | Extensions | `.klm` for documents, `.klms` for stylesheets | As stated |
| D45 | Specification license and governance | CC BY 4.0 for the text, MIT OR Apache-2.0 for the suite; the owner decides, RFCs propose | As stated |
| D46 | Where the specification lives | `book/part-3/` in `getkalem/kalem`; its own repository `getkalem/klm-spec` | In the Book, with the suite beside it; a separate repository only when a second implementation asks |

---

## 15. Recommendation

1. **Accept the three pillars** and the order: standard modes first, the format as a parallel design track, the Book started now from today's documents.
2. **Open RFC 0003, "The Kalem format",** with the twelve principles of section 5 as its preamble and the decisions of section 14 as its agenda; close each decision with a prototype and a page of the specification, not with a chat.
3. **Retire the interim `.klm` of D24 now**, before any release, so that the name is free and the strict-Org rule needs no exception.
4. **Start the Book this week** from `docs/`, the design documents and the decision records, published by CI with mdBook as the bridge, so that the specification has a home before its first page is written.
5. **Say "Typst is our engine, not our competitor"** in the first paragraph the public reads about the format.

If the owner accepts, the next commit turns sections 11, 12 and 14 into `todo.md` tasks and decision rows, and RFC 0003 begins.
