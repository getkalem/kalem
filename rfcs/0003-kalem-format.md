# RFC 0003: The Kalem Format (`.klm`), specification draft 0.2

- Status: **Accepted as the working specification** (owner, 2026-09-30), draft 0.2. **Frozen as the design record** (T2.10.9): sections 3 to 19 moved to Part III of the Book (`book/part-3/`), which is the specification from now on; a syntax change goes through Part III, its changelog and the conformance suite `tests/klm-spec/`, never through this document or code alone. What stays here is the reasoning: the survey (2), the alternatives (20), the open questions (21) and the experiments (appendices A and B). Written from the owner's decisions of 2026-09-30 (design_doc2.md, and the discussion that followed) and from a survey of the formats used for writing and typesetting (section 2); revised after the first experiment (appendix A).
- Design document sections affected: 2.6, 3.7, 4.2, 9, 10, 11.0, 11.2, 18.2, 20, 21 (D21, D24, D29)
- Decision IDs affected: D24 (superseded), D31 to D46 (closed here as drafts), D47 to D52 (new)

## Contents

1. Summary
2. What other formats teach, and what Word still has
3. Principles the syntax follows
4. The one syntax
5. Document structure
6. Text
7. Structure: headings, tasks, lists
8. Mathematics
9. Figures, tables, code, blocks
10. References, citations, notes
11. Layout and pages
12. Review: comments and tracked changes
13. Stylesheets (`.klms`)
14. Canonical serialization
15. Well-formedness, error recovery, the editor's guarantee
16. Grammar
17. Rendering and export
18. Conversion from and to other formats
19. Conformance, versioning, security
20. Alternatives considered
21. Unresolved questions

---

## 1. Summary

The Kalem format is a plain-text document format written by an editor and read by people, tools and version control. It has **one syntax**: a command, `\name[attributes]{content}`, used for every construct at every level, with paragraphs separated by blank lines and one shortcut, `$…$` for inline mathematics in LaTeX notation. It carries Org's structure (outline, tasks, properties, timestamps, tables with formulas, footnotes, typed links, executable blocks), LaTeX's mathematics unchanged, Word's styles, fields, sections and review marks, and typesetting-grade layout through a separate stylesheet. Its serialization is canonical, so a change of one word is a diff of one line. It is versioned, has a formal grammar, an executable specification and a conformance suite, and converts losslessly from Org.

---

## 2. What other formats teach, and what Word still has

A last survey before the syntax was fixed. Each row names what the Kalem format takes.

| Source | Lesson taken |
|---|---|
| **Typst** | Set and show rules: appearance is configured per element type in one place, never inline. Page setup as declarative properties. Math entered with `$`. A fast embeddable engine to export to. The Kalem format's stylesheet is a declarative subset of set rules; Typst is the PDF engine, not the format. |
| **CSS Paged Media (Prince, Paged.js)** | The page as sixteen margin boxes; running headers from named strings (`string-set`); page counters; footnotes as elements moved to a margin area; named pages; page groups. The stylesheet's `[page]` tables follow this model, so HTML export to a paged renderer is direct. |
| **Quarto, MyST, Pandoc** | Cross references by kind (`fig:`, `tbl:`, `eq:`, `sec:`, `thm:`) with automatic prefixes; callouts as first-class blocks; subfigures as a figure of figures; layout of several figures in rows and columns; citations from BibTeX with pre- and post-notes; a single source to PDF, HTML, DOCX and slides. |
| **Djot** | Linear-time parsing with no backtracking; inline parsing that never depends on definitions later in the file; one attribute grammar for every element; generic containers. The Kalem format keeps the attribute grammar and the locality rule. |
| **Texinfo, Scribe, Lout** | A single command syntax (`@command{…}`, `@Begin(Env)…@End(Env)`, `@Section @Title{…}`) can carry whole books and manuals; its cost is verbosity, paid gladly when a program writes the file. Their lesson on failure: commands that take several positional arguments become unreadable; the Kalem format has one attribute list and one content. |
| **AsciiDoc, reStructuredText** | Admonitions, callouts, include directives with ranges, conditional text, substitution definitions, field lists, roles on spans; a stable reference in the manual space. Kept: admonitions as block kinds, includes, substitutions from metadata; left: conditionals and a preprocessor. |
| **DocBook, DITA, TEI** | Semantic tagging separated from presentation; index terms in the text; glossary terms; editorial marks (insertions, deletions, responsibility, date); topic reuse. Kept: index and glossary entries, review marks with author and date. |
| **InDesign, Quark** | Paragraph and character styles as the unit of design; master pages; baseline grids; keep options; optical margin alignment; OpenType features per style; tracking and kerning. Kept in the stylesheet as properties, never in the text. |
| **EPUB 3** | Semantic landmarks (`epub:type`), reflowable output with accessibility metadata, media overlays. Kept: every block carries a semantic kind, so EPUB export and screen readers get structure for free. |
| **Markua (Leanpub), Fountain** | Books need parts, asides, sample sections and index marks; a genre format works when its constructs are few and named. Kept: `\part`, asides as a block kind, index marks. |
| **Kramdown** | The origin of `{: .class}` inline attribute lists; attributes belong to the element they follow. Kept in spirit: attributes belong to the command they are written in. |

**What Word has that the earlier discussion did not name.** All of these are in this draft:

- **Fields:** page number, page count, date, title, author, cross reference text, table of contents, list of figures and tables, index; values computed at render time, never typed. Section 11 and the stylesheet's `{field}` syntax.
- **Sections with their own page setup:** orientation, columns, margins, headers and footers changing mid-document. `\pagesetup` (11).
- **Text boxes and sidebars, columns, drop caps, watermarks, page borders:** `\box`, `\columns`, and stylesheet properties.
- **Footnotes and endnotes** as two kinds (10). **Bookmarks** as anchors (10). **Index entries** and a generated index (10, 11). **Captions with numbering** and lists of figures and tables (9, 11). **Line numbering** for legal and academic text (stylesheet). **Tab stops with leaders** for tables of contents (stylesheet).
- **Hyphenation, widow and orphan control, keep-with-next, non-breaking space, soft hyphen, thin space:** stylesheet and the special-character commands (6).
- **Language per span** for spelling and hyphenation (6). **Smart quotes and dashes** by language (6, 13).
- **Comments, insertions and deletions with author and date**, accept and reject: first-class marks (12), even before a collaboration UI exists.
- **Compare documents:** falls out of plain text and canonical serialization; `kalem diff` renders the difference of two `.klm` files (17).
- **Templates:** a stylesheet plus metadata defaults (13). **Document properties:** `\meta` (5). **Protected regions and forms:** not in 1.0 (21).

---

## 3. Principles the syntax follows

From docs/design_doc2.md section 5, sharpened by the owner's decisions:

1. **One explicit syntax for everything.** No lightweight markers for headings, lists or emphasis: the editor writes the file, so typing cost does not count, and one mechanism keeps the grammar and the tooling small. The single exception is inline mathematics (8), because its notation is LaTeX's own and `$` belongs to that notation.
2. **The editor guarantees well-formedness.** Every command is inserted with its closing brace; the cursor never enters a delimiter; every edit is a tree operation; the document is well-formed after every transaction, checked in debug builds and by property tests (15).
3. **Canonical serialization.** One content, one byte sequence (14). A word changed is one line changed.
4. **Ranges, never text.** The parser returns ranges into the file; the model is derived; unknown commands stay visible (15).
5. **Structure by lines.** Block commands start at the start of a line and close on their own line, so blocks reparse alone and an unclosed block cannot swallow the file (15).
6. **Semantics in the text, appearance in the stylesheet** (13). Direct formatting is possible and discouraged.
7. **Mathematics is LaTeX's**, unchanged, in the file and on the clipboard (8).
8. **Everything Org means is expressible** (7, 9, 10, 18).
9. **Small, versioned core; extensions by declared commands and namespaced attributes** (19).

---

## 4. The one syntax

### 4.1 Commands

```
\name[attributes]{content}
```

- `name`: one or more ASCII lowercase letters or digits, starting with a letter: `h1`, `b`, `figure`, `pagesetup`.
- `[attributes]`: optional. Missing means no attributes. An empty list `[]` is not written in canonical form.
- `{content}`: optional for commands that take none (`\pagebreak`, `\toc`, `\ref[…]`). Content is Kalem text: paragraphs, other commands, mathematics.

Commands are **inline** or **block** by their definition (sections 6 to 12 say which). An inline command's content holds inline content only; a block command's content holds blocks (paragraphs, block commands) and, for a few, inline content directly (headings, captions, list items). A command outside the core must be **declared** by the stylesheet (`[block.KIND]`, `[span.KIND]`) or by a plugin; an undeclared command parses as a generic block or span, renders its content, and is reported by `kalem check`.

### 4.2 Attributes

```
[#eq:euler .warning width=60% title="Pythagoras' theorem" numbered]
```

Items separated by whitespace:

| Form | Meaning |
|---|---|
| `#id` | The element's identifier, unique in the document, the target of `\ref` and of links. Letters, digits, `-`, `_`, `:`, `.`. |
| `.style` | A named style (13) applied to the element; several allowed. |
| `key=value` | A named attribute. `value` is a bare token (no whitespace, `]`, `"` or `=`) or a quoted string `"…"` with `\"` and `\\` escapes. |
| `key` | A boolean attribute set to true. |
| `value` | A positional value, at most one, meaning defined by the command (`\link[URL]`, `\ref[TARGET]`, `\cite[KEYS]`, `\img[SRC]`). |

A positional value contains no whitespace: several citation keys are separated by commas, `\cite[euler1740,apostol1976]`. A reference names an identifier bare, without `#`: `\ref[eq:euler]` refers to the element declared with `#eq:euler`; the `#` in an attribute list always declares, never refers.

Attribute keys are ASCII names. A key the specification does not define for a command is a **user property**: kept, shown in the properties panel, exported to the formats that have a place for it. A key with a namespace prefix (`plugin.key`) belongs to that plugin.

Values with meaning across the format: lengths (`12pt`, `2cm`, `60%`, `1.5em`), colors (`#c00000`, `red`), timestamps in Org's grammar (`<2026-10-03 Sat 10:00 +1w -2d>`, `[2026-10-03 Sat]`), durations (`2h`, `1d`), lists (`tags=writing,urgent`). A value that starts with `<` or `[` is one value up to its matching `>` or `]`, spaces included, so a timestamp is written without quotes: `date=<2026-10-03 Sat>`, `\date[[2026-10-03 Sat]]` (appendix B).

### 4.3 Text, escapes, verbatim

Text is UTF-8. Four escapes and no more: `\\`, `\{`, `\}`, `\$`. `[` and `]` need no escape: they are special only immediately after a command name. A command name ends at the first character that is not a lowercase letter or a digit; where the text after a command without content starts with a letter, a digit or `[`, the command is written with empty braces, `hyphen\shy{}ation`, `see\br{}[1]` (14.4). A backslash that starts neither an escape nor a command is an error, recovered as a literal backslash (15).

**Verbatim commands** (`code`, `raw`, `comment`, `eq`, and `$…$`) take their content as written: braces inside them nest and end the content only when unbalanced; the escapes `\{`, `\}` and `\\` are the only escapes recognized, and only for an unbalanced brace or a trailing backslash. Nothing else is interpreted.

### 4.4 Paragraphs

A paragraph is a run of inline content ended by a blank line, by the start of a block command at line start, or by the closing brace of the enclosing block. Paragraphs are implicit: `\p` exists only to carry attributes (`\p[.center]{…}`). This is the one structural rule that is not a command, kept because every text format shares it and because blank lines are the natural unit of a diff.

---

## 5. Document structure

```
\klm[1.0]
\meta[title="On the Shape of Notes" author="Mehmet Şekercioğlu" lang=tr style=article date=<2026-10-03 Sat>]

\h1[#intro]{Introduction}

Text.
```

- **`\klm[VERSION]`** is the first line of every document. The version is the specification's (19).
- **`\meta[…]`** holds the document's properties: `title`, `subtitle`, `author` (repeatable as `author="A" author="B"`), `date`, `lang`, `style` (the stylesheet, by name in the styles directory or by path), `keywords`, `abstract` (a command form `\meta{\abstract{…}}` for long values), `bibliography` (a `.bib` path, repeatable), `cite-style` (a CSL name), `numbering` (`sections`, `figures`, `equations` on or off), `toc` depth, `class` (a hint for LaTeX export: `article`, `book`, `beamer`), and user properties. Every key is a field (11).
- The body follows: blocks at nesting level zero. A **`\part{…}`** groups `\h1` sections in books; `\h1` to `\h6` form the outline (7).
- **`\include[path]`** at block level inserts another `.klm` file's body at that point at render time; with `section=#id` only that section; the included file keeps its own `\klm` line, which is ignored on inclusion. Paths are relative to the including file and confined to the project (19).
- **`\bibliography[…]`**, **`\toc`**, **`\lof`**, **`\lot`**, **`\printindex`** are block commands placed where their generated content goes (11).

---

## 6. Text

Inline commands. Content is inline content unless said otherwise.

| Command | Meaning | Notes |
|---|---|---|
| `\b{…}` `\i{…}` `\u{…}` | Strong, emphasis, underline | Semantic names: `b` renders bold by default, the stylesheet may change it |
| `\del{…}` `\ins{…}` | Deleted, inserted text | Also the review marks of section 12 when they carry `by=` |
| `\hl{…}` | Highlight | |
| `\sup{…}` `\sub{…}` | Superscript, subscript | |
| `\sc{…}` | Small capitals | |
| `\code{…}` | Inline code, verbatim | `lang=` optional |
| `\span[.style …]{…}` | Generic inline container | The home of direct formatting: `font=`, `size=`, `color=`, `bg=`, `weight=`, `tracking=` |
| `\lang[tr]{…}` | Language of a span | Spelling, hyphenation, quotes and dashes follow it; the document language is `\meta[lang=…]` |
| `\q{…}` | Quotation marks by language | Turkish `“…”`, German `„…“`, French `« … »`; nested `\q` alternates |
| `\br` | Line break inside a paragraph | |
| `\nbsp` `\shy` `\thinsp` `\zwsp` | Non-breaking space, soft hyphen, thin space, zero-width space | Written as commands so that they are visible in diffs |
| `\date[<…>]` | A timestamp shown in the document's format | Org's grammar for the value; active and inactive; ranges |
| `\var[key]` | The value of a `\meta` key or a stylesheet variable | A field (11) |
| `\sym[name]` | A named symbol when the character is hard to type: `\sym[ellipsis]`, `\sym[emdash]` | Unicode written directly is always allowed and canonical |
| `\index[term]` `\index[term sub="…"]` | An index entry at this point; renders nothing | See `\printindex` |
| `\gloss[term]{…}` | A glossary term with its definition at first use | Renders the term; the glossary block lists them |

Typographic replacements (straight to curly quotes, `--` to dashes) are **not** performed on the text: the editor inserts the right characters as the user types, by the span's language, so the file holds what the reader sees.

---

## 7. Structure: headings, tasks, lists

### 7.1 Headings and the outline

```
\h2[#write-section todo=TODO priority=A tags=writing,urgent]{Write the second section}
\props{
  scheduled=<2026-10-03 Sat>
  effort=2h
  client=getkalem
}
```

- `\part`, `\h1` … `\h6`: block commands with inline content; they form the outline. `\part` sits above `\h1` and is used in books.
- `numbered=false` on a heading excludes it from numbering (LaTeX's starred form). `toc=false` excludes it from the table of contents.
- **The heading line carries the outline attributes only,** what Org shows on a headline: `#id`, `.style`, `todo=KEYWORD` (the keyword set from `\meta[todo="TODO NEXT | DONE CANCELLED"]`, default `TODO | DONE`), `priority=A`, `tags=a,b` (inherited by subsections, `\meta[tags-inherit=false]` turns it off), `numbered`, `toc`, `noexport` (Org's `:noexport:`), `comment` (Org's `COMMENT` heading, kept and never rendered). Everything else about a heading lives in an attached **`\props{…}`** block directly under it, one `key=value` per line with the attribute value grammar: `scheduled=`, `deadline=`, `closed=`, `effort=`, `category=`, `archive=`, and the user properties. This is Org's headline against its planning line and property drawer, with one syntax; a heading stays one readable line, and a rescheduling is a one-line diff (appendix A).
- **Properties:** any other key in `\props` is a user property (4.2), inherited by subsections when the stylesheet or `\meta[props-inherit=…]` says so, exactly as Org's property inheritance. The agenda, the match language (`+writing-urgent/TODO`) and the queries of `kalem query` read them as they read Org's.
- **Logs and clocks:** a heading's history lives in a `\log{…}` block attached under it (after `\props` when both exist), with `\entry[state=DONE from=TODO at=<…>]{note}` and `\clock[from=<…> to=<…>]` lines; the editor writes them, the stylesheet hides them. This is Org's `LOGBOOK` with one syntax.
- **Statistics:** `\h2[stats=checkbox]{…}` shows the `[2/5]` cookie computed from the items and subtasks below; `stats=percent` shows `40%`.
- **Identifiers:** the editor assigns `#id` to every heading on creation (a short random string), so links survive renames and a moved section appears as a move in a diff. Ids never change; a duplicate id is an error the editor cannot produce and `kalem check` reports.

### 7.2 Lists

```
\ul{
  \li{Paper}
  \li[state=done]{Ink}
  \li[state=todo]{Envelopes
    \ul{
      \li{A4}
    }
  }
}

\ol[start=3 type=a]{
  \li{…}
}

\dl{
  \dt{Term}
  \dd{Definition}
}
```

- `\ul`, `\ol`, `\dl` are block commands holding `\li` (or `\dt`/`\dd`); `\li` holds inline content and, after a blank line, blocks.
- `\ol[start= type=1|a|A|i|I]`; nested numbering styles come from the stylesheet.
- **Task items:** `state=todo|done|partial`; the parent's `[2/5]` cookie follows; `\li[todo=NEXT]` uses the heading keyword set when a list is used for tasks.
- Every `\li` may carry `#id`, `tags=` and `state=`; timestamps and user properties go in an attached `\props{…}` inside the item, as for headings.

---

## 8. Mathematics

- **Inline:** `$…$`. The only shortcut in the format. Content is LaTeX mathematics as amsmath, amssymb and mathtools define it, plus the document's macros. A literal dollar in text is `\$`.
- **Display:** `\eq[#eq:euler]{ … }`, a block command, numbered by default (`numbered=false` to turn off), `env=align|gather|multline|cases|…` selects the environment the LaTeX export writes and the layout the renderer uses; `tag="*"` sets a custom tag.
- **Macros:** `\meta{\macros{ \newcommand{\R}{\mathbb{R}} … }}` holds `\newcommand`, `\renewcommand`, `\DeclareMathOperator` definitions in LaTeX form; they apply to every formula in the document and are emitted to the LaTeX and Typst exports.
- **Theorem-like blocks:** `\block[.theorem #thm:pyth title="Pythagoras"]{…}`; the stylesheet defines `theorem`, `lemma`, `proof`, `definition`, `example`, `remark` with their numbering and shared counters.
- **Rendering:** in the editor through org-math (KaTeX's subset); what KaTeX cannot lay out is rendered by a TeX engine to an image when one is installed, otherwise shown as source in a frame. **Export:** LaTeX output copies the formula verbatim; Typst output translates it (the mitex approach) and falls back to embedding a TeX-rendered image when translation fails; HTML uses KaTeX or MathML; DOCX uses OMML through pandoc's mapping.
- **Guarantee:** the file never stores a translated formula. What the user typed in LaTeX is what the file holds.

---

## 9. Figures, tables, code, blocks

### 9.1 Figures and images

```
\figure[#fig:shape width=60% placement=top]{
  \img[shape.png alt="The shape of a note"]
  \caption{The shape of a note}
}

\figure[#fig:pair layout=2]{
  \figure[#fig:a]{
    \img[a.png]
    \caption{Left}
  }
  \figure[#fig:b]{
    \img[b.png]
    \caption{Right}
  }
  \caption{Two shapes}
}
```

- `\img[SRC alt= width= height= scale=]`: inline when inside a paragraph, a block when alone. Formats: PNG, JPEG, SVG, PDF (first page), WebP.
- `\figure` is a block: content, an optional `\caption`, nested figures for subfigures (`layout=N` columns).
- `placement=here|top|bottom|page|inline` steers floats in paged output; HTML ignores it.
- `\caption[short="…"]{…}`: the short form goes to the list of figures.

### 9.2 Tables

```
\table[#tbl:costs cols="l r r r" header=1 caption-position=top]{
  \tr{\th{Item} \th{Qty} \th{Price} \th{Total}}
  \tr{\td{Paper} \td{3} \td{4.50} \td{}}
  \tr{\td{Ink} \td{1} \td{12.00} \td{}}
  \tfoot{
    \tr{\td{Total} \td{} \td{} \td{}}
  }
  \formulas{
    D = B * C
    D@foot = SUM(D)
  }
  \caption{Costs}
}
```

- `cols`: one letter per column, `l`, `c`, `r`, `p` (paragraph), with optional widths in the stylesheet or `cols="l r{3cm}"`; `numbers=` for the number format (see formulas).
- `header=N` rows are header rows; `\th` marks header cells; `\td[colspan= rowspan= align=]`.
- **Formulas** (`\formulas`, one formula per line, indented canonically): the spreadsheet dialect, `A1` references, ranges, `$` for absolute references, the common functions (`SUM`, `AVERAGE`, `MIN`, `MAX`, `COUNT`, `IF`, `ROUND`, `ABS`, `SQRT`, `MOD`, `CONCAT`, dates and durations), remote tables (`@tbl:other!B2`), Org's evaluation order and iteration limit. **Column formulas** are the rule: `D = B * C` applies to every data row, and a bare column in a function (`SUM(D)`) covers the data rows, so inserting a row changes no formula (appendix A). Data rows are the rows that are neither header rows (`header=N`) nor footer rows (`\tfoot`); footer cells are addressed with `@foot` (`D@foot`, `D@foot2` for a second footer row). Explicit ranges (`D2:D4`) remain for partial ranges. **Numbers** follow the document language for decimal and group separators (`1,4636` in Turkish) unless `numbers=en` or another tag is set on the table; the engine reads and writes cells in that format. The engine is `org-table`'s with a second front-end grammar; results are written into the cells, as Org does, so the file reads without a calculator.
- `\caption` and `caption-position`; `\tfoot` for footer rows; `longtable` behavior in paged output through the stylesheet.

### 9.3 Code and results

```
\code[lang=rust name=hello exec results=output]{
fn main() { println!("hello"); }
}
\results[for=hello]{
hello
}
```

- `\code` is verbatim; `lang=`, `linenos`, `highlight=3-5`, `name=`.
- **Execution** (Org Babel's semantics): `exec` marks the block runnable; `results=output|value|replace|append|silent`, `session=`, `dir=`, `tangle=PATH`, `var=`, `noweb` references `\noweb[name]` inside code; `\results[for=NAME]` holds the output block the editor writes. Never automatic: the trust model of RFC 0001 section 12.
- `\raw[format=html|latex|typst]{…}`: passed through to that export target only, verbatim.

### 9.4 Generic and predefined blocks

- `\block[.kind …]{…}`: the generic block. Predefined kinds with default rendering: `note`, `tip`, `warning`, `important`, `caution` (admonitions), `theorem` family (8), `abstract`, `aside`, `example`, `quote` (with `by=`), `verse` (line breaks kept), `center`, `epigraph`, `dedication`, `glossary`, `sample` (a section marked as a preview sample, Markua's lesson).
- `\box[.sidebar …]{…}`: a text box placed by the stylesheet (margin, float, full width).
- `\hr`: a thematic break.

---

## 10. References, citations, notes

| Command | Meaning |
|---|---|
| `\link[TARGET]{text}` | External link; `TARGET` is a URL, or a typed link `file:`, `id:`, `mailto:`, or a custom type a plugin declares. Text optional: the target is shown. |
| `\target[#id]` | An anchor (a bookmark) at a point in text. |
| `\ref[TARGET]{text}` | A cross reference to the element declared with `#TARGET` (the target is written bare); without text, and then without braces, the generated text by kind: "Figure 3", "Table 2", "equation (4)", "section 2.1", "Theorem 1"; `kind=number|page|title|full` chooses. Across included files. |
| `\cite[KEYS pre="see" post="p. 3" style=text]` | A citation of one or more comma-separated BibTeX keys, rendered by the CSL style of `\meta[cite-style]`; `style=text` gives "Knuth (1984)", default parenthetical. Sources from `\meta[bibliography=…]`. |
| `\fn{…}` | A footnote, content at the reference point; `kind=end` makes it an endnote; `#id` allows a second reference to the same note with `\fnref[#id]`. |
| `\index[term]` | Index entry (6); `\printindex` (11) generates the index. |
| `\bibliography[style=apa title="References"]` | The generated bibliography block. |

Unresolved targets and keys are diagnostics, shown in the editor and by `kalem check`.

---

## 11. Layout and pages

Layout belongs to the stylesheet (13). The text carries only what has a place in the content:

| Command | Meaning |
|---|---|
| `\pagebreak` `\columnbreak` | Forced breaks. |
| `\pagesetup[size= orientation= margins= columns= header= footer= numbering=]` | Starts a new section with its own page setup from this point (Word's section break); attributes not given keep the previous values. |
| `\columns[2 gap=8mm]{…}` | Content set in columns. |
| `\toc[depth=2]` `\lof` `\lot` `\printindex` | Generated lists, placed where they appear. |
| `\vspace[12pt]` | Vertical space; discouraged, the stylesheet's spacing is the rule. |
| `\linenumbers[on]` `\linenumbers[off]` | Line numbering for the following text. |

**Fields.** In text, `\var[key]` (6); in stylesheet strings, `{key}`: `{page}`, `{pages}`, `{title}`, `{author}`, `{date}`, `{chapter}`, `{section}`, `{h1}` (the current first-level heading, as CSS `string-set` does), `{filename}`, `{version}`. Fields are computed at render time, never stored.

---

## 12. Review: comments and tracked changes

Even before a collaboration interface, the format defines the marks, so that files exchanged through git carry reviews:

- `\note[by="Ayşe" at=<2026-10-01 Thu 14:02>]{Is this claim sourced?}`: an inline review comment attached to the point where it stands, or to the preceding `\span[#id]` by `on=#id`. Rendered as a margin note or a marker; excluded from print unless the stylesheet says otherwise.
- `\ins[by= at=]{…}` and `\del[by= at=]{…}` with `by=` are tracked changes; without `by=` they are ordinary semantic insertions and deletions (6). The editor's Accept and Reject commands remove the marks; `kalem review accept --all` does it in batch.
- `\comment{…}` (verbatim, block or inline) is the author's own comment, never rendered, unlike `\note` which is for others to read.

---

## 13. Stylesheets (`.klms`)

A stylesheet is a TOML file. It never contains text or computation: only properties. A document names one in `\meta[style=…]`; the editor ships built-in ones (`article`, `report`, `book`, `letter`, `thesis`, `notes`, `slides`); a template is a stylesheet plus a `\meta` skeleton.

```toml
[document]
font = "Source Serif 4"
size = "11pt"
leading = 1.35
lang = "tr"
hyphenate = true
justify = true
widows = 2
orphans = 2

[typography]
ligatures = true
kerning = true
oldstyle-figures = false
protrusion = true          # optical margin alignment
small-caps = "opentype"    # or "synthetic"

[page]
size = "a4"                # or "148mm x 210mm"
margins = { top = "25mm", bottom = "25mm", inner = "30mm", outer = "20mm" }
columns = 1
numbering = "1"            # "i" for front matter, set per \pagesetup
first-page-header = false

[page.header]
left = "{h1}"
right = "{page}"
rule = true

[page.footer]
center = ""

[style.h1]
size = "20pt"
weight = "bold"
space-before = "24pt"
space-after = "12pt"
keep-with-next = true
numbering = "1"            # "1.1" for h2 comes from [numbering]
page-break-before = false

[style.quote]
indent = "1em"
italic = true

[style.warning]            # a \span or \block kind
color = "#a40000"

[block.theorem]
counter = "theorem"        # shared: lemma, corollary use the same counter
label = "Theorem {n}"
italic-body = true

[numbering]
sections = "1.1.1"
figures = "per-chapter"    # "Figure 2.3"
equations = "(1)"

[lists]
bullets = ["•", "–", "·"]
ordered = ["1.", "a.", "i."]

[footnotes]
placement = "page"         # or "end", "margin"
numbering = "per-page"

[toc]
depth = 2
leaders = "dots"

[lang.tr]
quotes = ["“", "”", "‘", "’"]
hyphenation = "tr"

[fonts]
"Source Serif 4" = { file = "fonts/SourceSerif4-*.otf" }
```

Rules: properties only, no selectors beyond element and style names, no inheritance chains other than style → element → document; unknown keys are warnings; every property has a defined mapping to Typst, LaTeX, CSS and DOCX or is documented as unsupported for that target. Variables for fields: `[vars] company = "Kalem"` gives `{company}` and `\var[company]`.

---

## 14. Canonical serialization

The specification defines the bytes the editor writes. `kalem fmt` produces them; the serializer is the reference.

1. UTF-8, no byte order mark, LF line endings, one trailing newline.
2. `\klm[…]` on line 1, `\meta` on line 2 when present, one blank line, then the body.
3. **Block commands** start at the line's indentation, `{` ends the opening line, content is indented two spaces per nesting level, `}` stands alone on its own line at the parent's indentation. Consecutive blocks are separated by one blank line at level zero and by no blank line inside a container, except paragraphs, which are always separated by one blank line. **Attached blocks** (`\props`, `\log`, `\results`, `\formulas`, `\caption`, `\tfoot`) follow their owner with no blank line.
4. **Inline commands** are written inline with no spaces inside the delimiters. A command whose content is optional and empty is written **without braces**: `\ref[eq:euler]`, `\cite[knuth1984]`, `\img[a.png]`, `\toc`; with empty braces only where the next character would otherwise be read as part of its name or as its attributes (4.3). A command whose content is not optional keeps its braces when the content is empty: `\td{}`, an empty cell.
5. **Paragraph text is one line.** No hard wrapping. A setting `format.lines = sentence` breaks after sentence ends instead, for projects that prefer sentence-per-line diffs; both are canonical for the project that chose them, recorded in `\meta[format=…]`.
6. **Attributes:** always on the command's line: `#id` first, then `.style`s in the order applied, then keys in the order the specification lists them for that command, then user properties alphabetically; one space between items; quotes only when the value needs them; booleans as bare keys; no trailing spaces. Long lists do not wrap: what would make a heading long belongs in `\props` (7.1), where each property has its own line.
7. **Verbatim content** is written as it is, with the closing brace on its own line for blocks.
8. **Text** is written as typed; the editor inserts typographic characters, the serializer never rewrites them. Escapes are written only where needed.
9. Formatting is **idempotent** and **content-preserving**: `fmt(fmt(x)) = fmt(x)` and `parse(fmt(x)) = parse(x)` for every well-formed `x`; the conformance suite checks both.
10. **Version control.** Kalem writes a `.gitattributes` for a project on request: `*.klm diff=klm` with a `klm` diff driver whose word regex treats a command, an attribute and a word as tokens, so `git diff --word-diff` shows the changed word inside a long paragraph; `kalem diff` renders the difference of two files or revisions without git literacy.

---

## 15. Well-formedness, error recovery, the editor's guarantee

**Well-formed:** every `{` has its `}`, every `$` its pair, every command name is known or declared, every `#id` unique, every reference resolvable (a warning, not an error).

**The editor's guarantee.** In the rendered editor a document is well-formed after every transaction:

- Commands are inserted as a unit with their closing brace; the caret cannot be placed inside a delimiter; delimiters are not characters to the cursor.
- Backspace and Delete at a delimiter act on the content beside it, or remove an empty command whole.
- Cut, copy and paste operate on balanced ranges: a selection is extended or split to the nearest balanced form before it is cut; pasted text that is not well-formed is inserted as escaped text.
- Structural commands (heading level, list nesting, wrap in a command, unwrap) are tree operations of `klm-edit`.
- A debug assertion reparses after every transaction and checks the invariant; a property test runs random command sequences and asserts it; both are conformance requirements for an editor that claims the format.

**Files from elsewhere** (the source view, other editors, merges) can be ill-formed. The parser recovers deterministically, and `kalem check` reports every recovery:

- An unclosed **inline** command ends at the end of its paragraph.
- An unclosed **block** command ends before the next block command at the same or a lower indentation that starts at line start, or at the end of the enclosing block or file. Braces decide first: indentation is looked at only for a block whose closing brace never comes, so a well-formed file whose content is not indented parses as its braces say.
- An unclosed `$` ends at the end of the paragraph; an unclosed verbatim block ends at the end of the file.
- A stray `}` is text.
- An unknown command is a generic block or span with its content rendered.
- A duplicate `#id` keeps the first and reports the second.

Formatting on save applies only to well-formed documents; an ill-formed document is saved as it is and flagged. Repair is a command that shows its diff.

---

## 16. Grammar

EBNF, normative once accepted. Whitespace handling and indentation are as in 14; the grammar reads the canonical form and the recovered forms of 15.

```
document     = version-line , [ meta ] , { block } ;
version-line = "\klm[" , version , "]" , newline ;
meta         = "\meta" , [ attributes ] , [ "{" , { block } , "}" ] , newline ;

block        = block-command | paragraph | blank-line ;
block-command= indent , "\" , name , [ attributes ] , [ "{" , newline , { block } , indent , "}" ] , newline
             | indent , "\" , name , [ attributes ] , [ "{" , inline-content , "}" ] , newline   (* headings, captions, li *)
             | indent , "\" , verbatim-name , [ attributes ] , "{" , newline , verbatim-lines , indent , "}" , newline ;
paragraph    = inline-content , newline , { inline-content , newline } ;

inline-content = { text | escape | inline-command | math } ;
inline-command = "\" , name , [ attributes ] , [ "{" , inline-content , "}" ] ;
math         = "$" , verbatim-text , "$" ;
escape       = "\\" | "\{" | "\}" | "\$" ;
text         = character - ( "\" | "{" | "}" | "$" ) , { … } ;

attributes   = "[" , [ attribute , { ws , attribute } ] , "]" ;
attribute    = "#" , id | "." , name | key , "=" , value | key | value ;
value        = bare | quoted | bracketed ;
bare         = ( character - ( ws | "]" | "\"" | "=" | "<" | "[" ) ) , { character - ( ws | "]" | "\"" | "=" ) } ;
bracketed    = "<" , { character - ">" } , ">"          (* a timestamp, spaces included *)
             | "[" , { character - "]" } , "]" ;
quoted       = "\"" , { character - "\"" | "\\\"" | "\\\\" } , "\"" ;
name         = letter , { letter | digit } ;
key          = name , { "." , name } ;
id           = ( letter | digit ) , { letter | digit | "-" | "_" | ":" | "." } ;
```

Verbatim content: braces balanced, `\{`, `\}` and `\\` as the only escapes. Parsing is linear and local: no construct depends on text after its end, except references, which resolve in the model; an unclosed block is the one place the parser looks back, to cut it where 15 says (appendix B).

Which commands are blocks and how their content is read is fixed by their definition (sections 5 to 12), in five kinds: **containers** holding blocks (`\meta`, `\ul`, `\ol`, `\dl`, `\table`, `\tfoot`, `\figure`, `\block`, `\box`, `\columns`, `\log`); **line blocks** holding inline content on their line (`\part`, `\h1`…`\h6`, `\p`, `\li`, `\dt`, `\dd`, `\caption`, `\tr`, `\entry`, `\abstract`; `\li` also blocks after a blank line or a nested list); **bare blocks** without content (`\toc`, `\hr`, `\include`, `\bibliography`, `\pagebreak`, `\pagesetup`, `\clock`, …); **verbatim** commands (`\code`, `\raw`, `\comment`, `\eq`, `\macros`, `\results`); and **record blocks** holding one `key=value` or formula a line, re-indented by the serializer (`\props`, `\formulas`). `\img` alone on its line is a block. An unknown command at line start is a block when `{` ends its line or nothing follows it, a span otherwise.

---

## 17. Rendering and export

Every construct has a defined rendering in each target; the table names the mapping's shape, the specification's Part III of the Book carries the full tables.

| Target | Engine | Shape |
|---|---|---|
| Editor (graphical and terminal) | Kalem's view model | The same model as Org's: markers hidden, widgets for math, images, checkboxes, tables in the grid; the terminal shows Unicode math and images through the graphics protocols |
| HTML | `klm-export` | Semantic HTML5 with `data-kind`; the stylesheet compiled to CSS, `[page]` to CSS Paged Media for paged renderers; MathML or KaTeX for math |
| PDF | Typst embedded (default), LaTeX through tectonic or the installed distribution (for TeX-exact math and journal classes) | The stylesheet compiled to Typst set rules or a LaTeX preamble; fonts embedded; PDF/A on request; deterministic builds |
| LaTeX | `klm-export` | `\meta[class]`'s document class; math verbatim; styles as macros; review marks through `changes` or dropped by option |
| DOCX | pandoc bridge (first), own writer (later) | Styles mapped to Word paragraph and character styles by name, so Word users get real styles; comments and tracked changes to Word's; fields to Word fields |
| Markdown, Org | `klm-export` | Lossy where the target has no word, with a report (18) |
| EPUB | through HTML | Landmarks from the outline, `epub:type` from block kinds |
| `kalem diff` | model diff | A rendered difference of two versions, by paragraph and by word, for review without git literacy |

---

## 18. Conversion from and to other formats

**Org to Kalem: complete.** Headlines to `\hN` with `todo`, `priority`, `tags`, planning lines to timestamps, property drawers to attributes, `LOGBOOK` to `\log`, lists and checkboxes to `\ul`/`\ol` with `state`, tables and `#+TBLFM` to `\table` and `\formulas` (Calc formulas translated to the spreadsheet dialect where a mapping exists, kept as a `calc="…"` attribute otherwise), footnotes to `\fn`, links to `\link`/`\ref`, blocks to `\code`/`\block`, `#+OPTIONS` and keywords to `\meta`, macros to `\var`, `#+INCLUDE` to `\include`, entities to Unicode, LaTeX fragments to `$…$` and `\eq`. Checked on the Org corpus: convert, render, compare models.

**Kalem to Org: explicit loss.** Styles, spans with direct formatting, layout, review marks, `\pagesetup` and the spreadsheet dialect have no Org form: the exporter drops them and lists what it dropped, or writes them through Org's extension points when asked (`--org-extensions`), which is the mechanism of the retired D24.

**Markdown and LaTeX to Kalem:** through the standard modes' trees: headings, emphasis, lists, code, tables, math, citations, figures; LaTeX's unknown macros become `\raw[format=latex]`.

---

## 19. Conformance, versioning, security

- **Executable specification.** Every example in this document and in Part III of the Book is a file under `tests/klm-spec/` with its expected model (JSON), its canonical form and its HTML rendering. An implementation conforms when it passes the suite; Kalem's own `klm-syntax` is the reference.
- **Versioning.** `\klm[MAJOR.MINOR]`. A minor version adds commands, attributes or stylesheet properties; documents of an older minor version parse unchanged. A major version may change syntax and ships with `kalem migrate`. The version of the format is independent of Kalem's.
- **Extensions.** A plugin declares commands, block kinds and attributes under a namespace (`mermaid.diagram`, `plugin.key`); undeclared use is a warning; a document that uses an extension records it in `\meta[requires=…]`.
- **Security.** `\include` and `\img` paths are confined to the project folder unless the setting allows more; `exec` never runs without the document trust of RFC 0001 section 12; `\raw` output is passed only to its named target; fields never execute code; stylesheets carry no code.

---

## 20. Alternatives considered

- **Lightweight markers for headings, lists and emphasis** (Markdown, Org, Djot, Typst all keep some). Declined by the owner: the editor writes the file, and one syntax keeps the grammar, the tooling and the mental model small. Kept open as pure sugar if the diffs of real documents prove hard to read (21).
- **Org plus extension points as the format** (RFC 0001, D24). Retired: uniformity is not reachable inside Org's syntax and Word's needs have no Org home.
- **Typst as the format.** Declined: a programming language compiled to PDF cannot be edited losslessly in rendered form and lacks Org's structure; adopted as the PDF engine.
- **XML or HTML.** Declined: verbosity beyond what even an editor-written file should carry in diffs; no outline or task semantics.
- **Fenced generic blocks (`:::`) and bracketed spans** (Djot). Declined for the one-syntax rule; their attribute grammar is kept.
- **Sigil `@` or `#`.** `@` collides with email addresses and citations in prose; `#` with numbers and tags. `\` collides only with itself and is what LaTeX authors expect.

---

## 21. Unresolved questions

1. The exact spreadsheet function list and its agreement with Excel's semantics for dates and rounding.
2. Whether `\log` and `\results` belong in the document or in a sidecar file for projects that dislike machine-written blocks in their diffs.
3. Table cell content beyond inline text (lists and paragraphs in cells) and its canonical form.
4. Slides: a `slides` stylesheet with `\h2` as a slide, or a `\slide` command.
5. Forms and protected regions (Word's content controls): not in 1.0.
6. The stylesheet's limit: which InDesign-grade controls (baseline grid, optical alignment per glyph) are in 1.0 and which wait for engine support.
7. Verbatim content inside an indented block (appendix B): written as it is (14.7), a formula in a theorem block has its lines at the left edge and its braces indented. Whether the serializer should indent verbatim lines with their block, and strip that indentation when reading, is open.
8. The order of keys "the specification lists for that command" (14.6) needs a table per command; until it exists, keys keep the order written.

Closed by the first experiment (appendix A): no inline shortcut beyond `$…$` is needed; headings keep their outline attributes on one line and everything else in `\props`.

---

## Appendix A. The first experiment (T2.13.1, 2026-09-30)

Three documents were written by hand in draft 0.1 and its canonical form (`tests/klm-spec/samples/`: a letter, a paper with mathematics, a task notebook), then edited as a user would in one session, and the diffs and one merge were examined (commits `278cc55` and `4328c3d` of `getkalem/kalem`).

| Edit | Lines changed |
|---|---|
| A word changed inside a bold span in a paragraph (letter) | 1 |
| A date changed inside `\date` (letter) | 1 |
| A list item added (letter, notebook) | 1 added |
| A span wrapped around two words (letter) | 1 |
| A total changed (letter) | 1 |
| A theorem title, a citation key, a sentence appended (paper) | 1 each |
| A figure width, a caption (paper) | 1 each |
| A deadline moved on a heading written with one attribute per line (notebook) | 1 |
| A schedule moved on a heading written on one line (notebook) | 1, the whole heading line |
| A checkbox ticked (notebook) | 1 |
| A `\log` with a clock added under a heading (notebook) | 3 added |
| A table row added (paper, notebook) | 1 added, plus the formula range lines that named the old range: 1 (paper), 2 (notebook), plus the totals row |
| Two branches editing different paragraphs of the letter, merged | Clean |

What it showed, and what draft 0.2 changed:

1. **No shortcut is needed.** `\b{üç}` inside a paragraph reads as well in a diff as `*üç*` would; the one-syntax rule stands. Question 1 of 0.1 closed.
2. **Headings.** One attribute per line gave one-line diffs but a nine-line heading in raw text; one line gave a readable heading but a whole-line diff. Draft 0.2 keeps the outline attributes on the heading line and moves planning and properties to an attached `\props` block, one per line: both readable and diff-friendly (7.1, 14.6). Question 2 closed.
3. **Formulas.** Inserting a row forced a change to every formula that named a range, exactly as in a spreadsheet. Draft 0.2 makes column formulas the rule (`D = B * C`, `SUM(D)` over data rows) and addresses footer rows with `@foot`, so a row insertion changes no formula (9.2).
4. **Empty braces** after `\ref` and `\cite` were noise; the canonical form omits them (14.4). References name identifiers bare; citation keys are comma-separated (4.2, 10).
5. **Turkish numbers** in table cells (`1,4636`) need the engine to know the decimal separator; the number format follows the document language, with `numbers=` to override (9.2).
6. **Attached blocks** (`\log`, `\props`, `\formulas`, `\caption`) follow their owner without a blank line; the rule is now written (14.3).
7. **Long paragraphs** are one line, so a changed word shows as one changed line; reading the change inside the line needs `--word-diff`, hence the `.gitattributes` and `kalem diff` of 14.10. The sentence-per-line option remains for projects that prefer it.

---

## Appendix B. The parser spike (T2.13.2, 2026-09-30)

A throwaway parser for this grammar, `spikes/klm-parser`, parses every example of this document and the three samples, writes the canonical form back and checks `fmt(fmt(x)) = fmt(x)` and `parse(fmt(x)) = parse(x)` on the model; it runs each recovery rule of 15 and each known ambiguity on a malformed or tricky input (its `tests/spec.rs`). What it found, and what changed:

| Finding | Change |
|---|---|
| `date=<2026-10-03 Sat>` in the samples: the grammar's bare value stops at the space, so the timestamp split into a value and a stray attribute | A value starting with `<` or `[` runs to its matching `>` or `]` (4.2, 16) |
| `\date[[2026-10-03 Sat]]`: an inactive timestamp inside an attribute list | The same rule; the list's own `]` is the one after the timestamp's |
| `hyphen\shyation` reads as the command `shyation`; `\br` followed by the text `[1]` reads as attributes | The serializer writes empty braces there, `hyphen\shy{}ation`, `\br{}[1]` (4.3, 14.4) |
| `\td{}` in the table example against 14.4's "empty content without braces" | Only commands whose content is optional drop empty braces; a cell keeps them (14.4) |
| Recovery by indentation would close a well-formed block whose content is not indented (the paper's theorem block, as first written) | Braces decide first; indentation cuts only a block whose brace never comes (15) |
| The grammar did not say which commands are blocks, or how `\props` and `\formulas` are read | The five kinds, listed in 16 |
| Not canonical by 14: the paper's theorem and proof blocks (content not indented, `.theorem` before `#thm:sinir`, `title="Kaynaklar"` quoted), the list example (no blank lines between top-level blocks), the figure example (subfigures on one line) | The paper sample and the two examples rewritten in canonical form; the letter and the notebook were canonical already |
| Verbatim lines keep their written indentation inside an indented block | Open: question 7 of 21 |
| Key order per command is not tabulated | Open: question 8 of 21; keys keep the order written |

The spike needs no backtracking except for an unclosed block, reads each construct once, and its model is what `klm-syntax` (T2.13.3) will be tested against: the conformance suite's first files, one per example, with the model as JSON, the canonical form and a plain HTML, come from `klm-parser-spike examples`.
