# Kalem

**Kalem** ("pen" in Turkish) is a fast, open source editor that shows plain-text documents the way they read and keeps them plain text. It opens Org, LaTeX, CSV and BibTeX files rendered, as a word processor or a spreadsheet would show them, edits them in place, and writes back only what you changed: each format exactly as its standard defines it, nothing added and nothing dropped. Markdown is next. Kalem also has a document format of its own, `.klm`, designed from what these formats do best, and it runs the same editor in a window and in a terminal.

> **Status: alpha, not released yet.** There are no binaries: build from source to try it. Progress is in [`todo.md`](todo.md) and [`CHANGELOG.md`](CHANGELOG.md).

<!-- A screenshot or GIF of both editors goes here (todo.md, T1.8.7). -->

## What Kalem is

**One editor for every text file.** A file opens in the view its format calls for, and every other file as plain text with syntax highlighting.

| Format | Opens as | The standard Kalem follows | Checked against | Today |
|---|---|---|---|---|
| Org (`.org`) | A document: headings, emphasis, lists, checkboxes, tables with formulas, footnotes, links, images, citations, formulas | The Org Syntax as `org-element.el` implements it (Org 9.7) | Emacs 30.1, construct by construct, on the Org manual, Org's own tests, all of Worg and mutated files; the editing commands and the exporters against Emacs's | Parser, model, editing, tables and export at 100% agreement |
| LaTeX (`.tex`) | A document: sections with their numbers, styled text, formulas drawn inline, references, citations, figures, tables, code | LaTeX as the TeX engines accept it | Byte-exact round trip; structure against pandoc's LaTeX reader; the PDF built by a TeX engine; planned, the PDF the authors published, on thousands of arXiv documents, for numbering, references, citations and formulas | Rendered subset, multi-file projects, diagnostics, completion, building; what Kalem does not render is shown as source |
| CSV, TSV | A grid with a header row, sorting and filters | RFC 4180 and the dialects spreadsheets write | RFC 4180's cases; files written by Excel, LibreOffice Calc and Google Sheets in several locales | Grid, dialect detection, editing |
| BibTeX (`.bib`) | A grid of entries | BibTeX and BibLaTeX data files | The same file read by BibTeX or biber, and by hayagriva | Grid, sorting, field editing |
| Markdown (`.md`) | Plain text today; a document like Org's is planned | CommonMark with the GitHub extensions | The CommonMark and GFM specification suites | Planned |
| Kalem (`.klm`) | Kalem's own format: Org's structure, LaTeX mathematics, styles and page layout | Its own specification, Part III of the Book | A conformance suite, one file per example of the specification | Specification at draft 0.2 with a prototype parser; the editor opens a `.klm` file as Org for now |
| Everything else | Plain text, colored by its language | The file's own bytes | The file unchanged but for the edits | Highlighting, indentation, line tools |

**Faithful to the format.** Three rules hold for every standard format:

1. **Ranges, never text.** A file is parsed into ranges of its text and edited in place. Kalem never regenerates a file from a tree, so the parts you did not touch stay byte for byte as they were: spacing, comments, line endings, byte order marks.
2. **No extension.** Kalem writes nothing into a standard file that its standard does not define. Formatting a format cannot express is not offered in it; it belongs to the Kalem format.
3. **Unknown constructs stay visible.** What Kalem does not understand is shown as the source it is, never hidden or guessed.

**A specification for each format.** Part II of [the Kalem Book](https://getkalem.github.io/kalem) states, format by format, what Kalem reads, draws, edits, writes and exports, and against which reference implementation that is tested, precisely enough for another implementation to follow it. It is being written for every format, LaTeX included, and a chapter changes with the code in the same pull request. Part III is the specification of the Kalem format, with its grammar and conformance suite.

**Two editors, one behavior.** A graphical editor (gpui) and a terminal editor (ratatui) share the same core: the same commands, keys, settings, menus and views. The terminal editor draws formulas, images and tables too. Every document operation is also a command-line tool, for scripts and CI.

**Emacs's ways, without Emacs.** Word-like keys by default and Vim keys as an option, with Doom Emacs's leader keys in the Vim profile. A file manager like Dired, projects like Projectile, find in files, a command palette, an outline, focus mode and narrowing. No Elisp: everything is a command with a configurable key.

**The Kalem format.** `.klm` is a plain-text document format designed from scratch: one command syntax, `\name[attributes]{content}`, Org's outline, tasks, tags, properties and timestamps, LaTeX mathematics in `$…$`, tables with spreadsheet formulas, styles and page layout in a separate stylesheet, canonical serialization and lossless conversion to and from Org. It is meant for the documents Org and Markdown cannot carry, letters, papers, theses and books with page-quality output, and it converts to Org, HTML, PDF and the other export formats. It is a design track beside the standard formats, which come first.

**Small core, plugins for the rest.** Org, LaTeX, CSV, BibTeX, Markdown, plain text, the file manager and projects are built in. Other formats, viewers for files that are not text (PDF, Word, Excel, images), language servers for programming languages and other features are planned as plugins: WebAssembly components written in Rust against a typed API, sandboxed, from a separate `getkalem/plugins` repository.

Kalem is not a Microsoft Office clone, a page layout tool, a full spreadsheet, a full IDE or a replacement for Emacs. See [non-goals](design_document.md#14-non-goals).

## Who it is for

- **Writers and note takers.** Notes, outlines, tasks and documents in Org, in one light application, with export to HTML, Markdown, LaTeX, PDF and, through pandoc, Word, OpenDocument and EPUB. Markdown comes next. The Book: [Writing in Org](book/part-1/writing-in-org.org), [Exporting](book/part-1/exporting.org).
- **Scientists, students and authors.** LaTeX documents edited as they read, with formulas drawn inline, citations from BibTeX, multi-file projects and PDF builds, and Org with citations, formulas and LaTeX export for papers and books. The Book: [LaTeX files](book/part-1/latex-files.org), [Citations](book/part-1/citations.org).
- **People who want their files opened as themselves.** CSV as a grid, BibTeX as a grid, and later, through plugins, Word, Excel and PDF files, never converted in order to be opened. The Book: [CSV files](book/part-1/csv-files.org).
- **People who use Org with people who use Emacs.** A co-author's `.org` file edited without learning Emacs, and returned to Emacs without a diff outside the edits. The Book: [Org](book/part-2/org.org).
- **Terminal users.** The same editor over SSH and in tmux, with rendered documents, images where the terminal draws them, and a scriptable command line. The Book: [Starting](book/part-1/starting.org), [The command line](book/part-1/the-command-line.org).
- **Programmers**, later: plain text with highlighting today, language servers through plugins in phase 3. The Book: [Plain text and code](book/part-1/plain-text-and-code.org).

## Works today

- **Org as a document**, checked against Emacs: headings, emphasis, lists and checkboxes, links, footnotes, citations with a picker, pictures, LaTeX formulas drawn inline, source blocks with highlighting, folding, the outline, and a source view to switch to at any time. TODO states, priorities, tags, properties, scheduling with a date picker, state logging, repeaters, TODO dependencies and match strings (`kalem query`), each command identical to Emacs on thousands of cases. Tables with `#+TBLFM` formulas and Calc's functions, a formula bar, recalculation and CSV import and export. Export to HTML, Markdown, GitHub Markdown, LaTeX, PDF and plain text matching Emacs's exporter, with citations in Org's `basic` styles or any CSL style, or through biblatex and natbib; Word, OpenDocument, EPUB and RTF through pandoc.
- **LaTeX as a document**: sections, formulas, references, citations, figures, tables and code rendered; multi-file projects; PDF builds with diagnostics at their files and lines; completion of commands, labels and citations; `.tex` files stay LaTeX byte for byte.
- **CSV as a grid**, with sorting and filters, dialect kept; **BibTeX as a grid** of entries.
- **Around the files.** Projects, a folder tree, find in files, a file manager like Dired with the keys every file manager has, plain text with highlighting, themes, settings in TOML, keymaps in JSON, English and Turkish.
- **Two editors.** Graphical and terminal, with the same commands, keys, settings and menus; menus in the macOS menu bar and in the window's own menu bar on Linux and Windows; F10 lists every menu item in both.
- **The Kalem format**, on paper: the specification at draft 0.2 in Part III of the Book, the parser `klm-syntax` that parses, formats and round-trips every example, and the first files of the conformance suite (`tests/klm-spec`).

## Not yet

- Markdown as a document (today it opens as plain text)
- The Kalem format in the editor: `klm-syntax`, `klm-model`, `klm-edit`, rendering, stylesheets and the exporters
- The agenda, capture, clocking reports, Babel (running source blocks)
- Plugins, viewers for files that are not text, language servers, spell checking
- Signed binaries and installers for macOS, Windows and Linux

## Documentation

[The Kalem Book](https://getkalem.github.io/kalem) is the one reference: the manual (Part I), the specification of every format as Kalem implements it (Part II), the specification of the Kalem format (Part III), extending Kalem (Part IV) and the design (Part V). Its source is [`book/`](book/index.org), written in Org; `kalem book build` turns it into the site and `kalem book check` verifies it against the code.

## Command line

`kalem --help`:

```
Usage: kalem [FILE | FOLDER]      the editor: graphical where there is a display, else in the terminal
       kalem gui [FILE]           the graphical editor
       kalem tui [FILE]           the terminal editor (also kalem -t [FILE])
       kalem tui --detect         what the terminal can do
       kalem <COMMAND>            a command-line tool

Commands:
  parse        Print the syntax tree of a file
  check        Check files: syntax diagnostics and round-trip verification (Org and LaTeX files)
  commands     List the commands, one a line: ID, title, scope and keys
  complete     Print the completions at a place in a file
  fmt          Align tables and tags, and blank lines as each file has them
  export       Export Org files as Emacs's Org exporter does: `kalem export notes.org --to html`
  import       Convert Word, OpenDocument, Markdown, HTML, EPUB or RTF files to Org through pandoc
  diff-pandoc  Compare the structure Kalem reads in LaTeX files with pandoc's LaTeX reader (development)
  latex        LaTeX documents: `kalem latex build FILE`
  book         The Book: `kalem book build`, `kalem book check`
  table        Table formulas: `kalem table recalc FILE...`
  query        Print the headlines matching an Org match string: `kalem query notes.org 'TODO="NEXT"+work'`
  dump         Dump the parse tree in a machine-readable format
  diff-emacs   Compare the parse with Emacs's org-element (development)
```

Planned: `kalem agenda`, and `kalem run` for a plugin's command in batch, like `emacs --batch`.

## Repository layout

| Path | Contents |
|---|---|
| `crates/org-syntax` | Lossless, incremental Org parser, usable on its own |
| `crates/org-model` | Org document model: outline, TODO states, tags, properties, match strings, links, statistics, clocks |
| `crates/org-edit` | Org editing commands with undo, identical to Emacs's |
| `crates/org-table` | Org tables and `#+TBLFM` formulas |
| `crates/org-export` | Org exporters: HTML, Markdown, GitHub Markdown, LaTeX, plain text and Org, with citations (a port of `ox.el`) |
| `crates/org-cite` | Citations: BibTeX and CSL-JSON bibliographies, Org's `basic` processor and CSL styles |
| `crates/latex-syntax` | Lossless, incremental LaTeX parser |
| `crates/latex-model` | The document model of LaTeX files: structure, numbering, labels, citations and definitions |
| `crates/org-math` | LaTeX formulas drawn natively |
| `crates/kalem-core` | The editor's model, shared by both frontends: documents and their modes (Org, LaTeX, CSV, BibTeX, plain text), commands, keymaps, settings, the file manager, projects |
| `crates/kalem-ui`, `crates/gpui-rich-text` | The graphical editor |
| `crates/kalem-tui`, `crates/tui-rich-text` | The terminal editor |
| `crates/kalem-fs`, `crates/kalem-project`, `crates/kalem-highlight` | Files, projects and syntax highlighting |
| `crates/kalem-cli` | Command-line subcommands |
| `crates/kalem` | The `kalem` binary (published as `kalem-editor`) |
| `klm-syntax` | The parser and canonical formatter of the Kalem format, with byte ranges and incremental reparsing; `klm-model` and `klm-edit` follow |
| `tests/corpus`, `tests/latex`, `tests/csv`, `tests/klm-spec` | Real-world files and conformance suites used for testing |
| `tests/emacs` | Scripts that compare Kalem's Org parser, commands and exporters with Emacs |
| `book/` | The Kalem Book, in Org: the manual, the formats, the Kalem format, extending Kalem, the design |
| `design_document.md` | The design document (RFC 0001) |
| `design_doc2.md` | The second design document (RFC 0002): the standard modes, the Kalem format, the Book |
| `rfcs/` | RFC 0003, the Kalem format, and the RFC process |
| `todo.md` | The work breakdown: every task with its reason, test and done-criterion, done and open |
| `todo2.md` | The open tasks, in the order they are to be done, under general headings, with the points that need a decision |

## Building

Kalem needs a recent stable Rust toolchain.

```bash
cargo build --release                                              # both editors
cargo build --release -p kalem-editor --no-default-features --features tui  # terminal only
```

The graphical editor needs gpui's system libraries on Linux (for example `libxkbcommon-dev`, `libvulkan-dev` and `libwayland-dev`).

The differential tests against Emacs need Emacs 29 or newer with Org 9.7 on your `PATH`; the LaTeX comparisons need pandoc, and building PDFs needs a TeX distribution.

## History

Kalem started on 2026-09-27 as "Typora for Org", an editor for Org files for people who do not use Emacs. The engine that made that possible, a lossless, range-based editor verified against a reference implementation, turned out to serve every plain-text format the same way, and on 2026-09-30 the project became what this page describes ([RFC 0002](design_doc2.md)).

## Contributing

Contributions are welcome. Start with [`CONTRIBUTING.md`](CONTRIBUTING.md) and the Book. Please follow the [code of conduct](CODE_OF_CONDUCT.md).

## Contact

Maintainer contact details will be published here before the first public release.

## License

Licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE))
- MIT license ([`LICENSE-MIT`](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

Test corpus files under `tests/corpus` keep their original licenses, listed in [`tests/corpus/LICENSES.md`](tests/corpus/LICENSES.md).
