# Kalem

**Kalem** ("pen" in Turkish) is a fast, open source, text-first editor. It opens every plain-text format as itself, shows it the way it reads, and leaves the file exactly as it found it. Org, Markdown, LaTeX, CSV and BibTeX are built in; every other text file opens as code with highlighting; other formats come as plugins, written in Rust. One editor runs in a window and in a terminal, and every document operation is also a command-line tool.

> **Status: alpha, not released yet.** There are no binaries: build from source to try it. Progress is in [`todo.md`](todo.md) and [`CHANGELOG.md`](CHANGELOG.md).

<!-- A screenshot or GIF of both editors goes here (todo.md, T1.8.7). -->

## What Kalem is

**Text first.** The file on disk is the truth. Kalem has no database, no container format and no sidecar files: a document is the text file you see in `git diff`, `grep` and every other editor. Kalem renders that text, bold as bold and tables as grids, and edits it in place.

**Every format as itself.** Kalem never converts a file in order to open it, never asks to, and never saves it as something else. A `.md` file stays Markdown, a `.tex` file stays LaTeX, a `.csv` file stays the CSV its spreadsheet wrote. Three rules hold for every format:

1. **Ranges, never text.** A file is parsed into ranges of its text and edited in place. Kalem never regenerates a file from a tree, so the parts you did not touch stay byte for byte as they were: spacing, comments, line endings, byte order marks.
2. **No extension.** Kalem writes nothing into a file that its standard does not define. Formatting a format cannot express is not offered in it.
3. **Unknown constructs stay visible.** What Kalem does not understand is shown as the source it is, never hidden or guessed.

Conversion exists only as a command you run on purpose: *Export*, *Convert to Org*, `kalem export`.

**One editor for every text file.** A file opens in the view its format calls for. Each built-in format follows its standard and is checked against a reference implementation:

| Format | Opens as | The standard Kalem follows | Checked against | Today |
|---|---|---|---|---|
| Org (`.org`) | A document: headings, emphasis, lists, checkboxes, tables with formulas, footnotes, links, images, citations, formulas | The Org Syntax as `org-element.el` implements it (Org 9.7) | Emacs 30.1, construct by construct, on the Org manual, Org's own tests, all of Worg and mutated files; the editing commands and the exporters against Emacs's | Parser, model, editing, tables and export at 100% agreement |
| Markdown (`.md`) | A document: headings, emphasis, code, links, images, task lists, tables with formulas, formulas, footnotes, front matter as a form, wiki links | CommonMark with the GitHub extensions | The CommonMark and GFM specification suites: 632 of 648 and 23 of 24 examples, the differences listed in the Book | Rendered view, editing, tables, conversion to Org |
| LaTeX (`.tex`) | A document: sections with their numbers, styled text, formulas drawn inline, references, citations, figures, tables, code | LaTeX as the TeX engines accept it | Byte-exact round trip; structure against pandoc's LaTeX reader; the PDF built by a TeX engine; planned, the PDF the authors published, on thousands of arXiv documents | Rendered subset, multi-file projects, diagnostics, completion, building; what Kalem does not render is shown as source |
| CSV, TSV | A spreadsheet-like grid: column letters, row numbers, the header pinned, sorting, filters, a record view and column statistics | RFC 4180 and the dialects spreadsheets write | RFC 4180's cases; files written by Excel, LibreOffice Calc and Google Sheets in several locales | Grid, dialect detection, editing |
| BibTeX (`.bib`) | A grid of entries | BibTeX and BibLaTeX data files | The same file read by BibTeX or biber, and by hayagriva | Grid, sorting, field editing |
| Everything else | Plain text, colored by its language | The file's own bytes | The file unchanged but for the edits | Highlighting, indentation, line tools, files of 100 MB |

Part II of [the Kalem Book](https://getkalem.github.io/kalem) states, format by format, what Kalem reads, draws, edits, writes and exports, and against which reference that is tested, precisely enough for another implementation to follow it.

**More formats as plugins, written in Rust.** Formats beyond the built-in ones, viewers for files that are not text (PDF, Word, Excel, images), language servers for programming languages and other features are plugins. A plugin is compiled Rust, run by Kalem as a sandboxed WebAssembly component against a typed API: its own memory, a memory limit, a time budget, and only the permissions it declares. Kalem ships no JavaScript, Lua or Lisp interpreter and reads no script at startup.

That is the difference from the extensible editors people know. An Emacs package, a Vim script or a VS Code extension runs in an interpreter inside the editor, so a slow extension makes a slow editor, and every keystroke pays for the interpreter even before an extension runs. Kalem's plugins are compiled code with a budget: a plugin cannot freeze the editor, a plugin that fails is disabled rather than taking Kalem down, and a plugin author reads the same Rust types as a core contributor. The plugin runtime is phase 3 of the roadmap and is not built yet; the contracts it will expose exist in the core today and the built-in modes and completers are written against them. [Plugins](book/part-4/plugins.org) in the Book has the design.

**Fast.** One static binary, no runtime to start and nothing to interpret. Every target in the design has a measurement; a few, from an Apple M1 Max on 2026-09-28:

| | Measured | Target |
|---|---:|---|
| Cold start to the first frame, graphical | 148 ms | under 300 ms |
| Opening a 1 MB document, graphical | 184 ms | under 200 ms |
| Keystroke to frame, graphical, usually · 1 in 100 | 8.7 ms · 14.9 ms | under 16 ms · 33 ms |
| Reparse after a keystroke in the 840 KB Org manual | 0.05 ms | under 2 ms |
| A 100 MB plain text file, until interactive, graphical | 430 ms | under 1 s |
| Memory with an empty document, graphical | 47 MB | under 80 MB |
| The binary, both editors · terminal only | 13 MB · 7 MB | under 40 MB · 15 MB |

[Performance](book/part-5/performance.org) in the Book has every target, the method and the scripts that repeat the measurement.

**Two editors, one behavior.** A graphical editor (gpui) and a terminal editor (ratatui) share the same core: the same commands, keys, settings, menus and views. The terminal editor draws formulas, images and tables too, so the same document reads the same over SSH. Every document operation is also a command-line tool, for scripts and CI.

**A power editor's tools, no scripting language.** Word-like keys by default and Vim keys as an option, with Doom Emacs's leader keys in the Vim profile. A file manager with every file manager's keys, projects, find in files, a command palette, an outline, panes, workspaces, sessions, bookmarks, focus mode and narrowing. Everything is a command with a configurable key; configuration is data in TOML and JSON, checked when it is read.

**A document format of its own, optional.** For documents the standard formats cannot carry, letters, papers, theses and books with page-quality output, Kalem is designing `.klm`: one command syntax, Org's outline and tasks, LaTeX mathematics, tables with formulas, styles and page layout in a separate stylesheet, with a specification and a conformance suite (Part III of the Book). It is a design track beside the standard formats, which come first. Kalem never converts your files into it: a `.org` or `.md` file is yours, and stays what it is.

Kalem is not a Microsoft Office clone, a page layout tool, a full spreadsheet, a full IDE or an Emacs. See [non-goals](design_document.md#14-non-goals).

## Who it is for

- **Writers and note takers.** Notes, outlines, tasks and documents in Markdown or Org, in one light application, with wiki links and front matter in Markdown, TODO states, scheduling and tags in Org, and export to HTML, LaTeX, PDF and, through pandoc, Word, OpenDocument and EPUB. The Book: [Markdown](book/part-2/markdown.org), [Writing in Org](book/part-1/writing-in-org.org), [Exporting](book/part-1/exporting.org).
- **Scientists, students and authors.** LaTeX documents edited as they read, with formulas drawn inline, citations from BibTeX, multi-file projects and PDF builds. The Book: [LaTeX files](book/part-1/latex-files.org), [Citations](book/part-1/citations.org).
- **People with data in text files.** CSV and TSV as a spreadsheet-like grid with sorting, filters, statistics and the keys of a spreadsheet, the dialect kept; BibTeX as a grid. The Book: [CSV files](book/part-1/csv-files.org).
- **People who want their files opened as themselves.** No import, no conversion prompt, no "save as". Later, through plugins, Word, Excel and PDF files the same way. The Book: [What Kalem is](book/part-1/what-kalem-is.org).
- **People who share files with Emacs users.** A co-author's `.org` file edited without Emacs and returned without a diff outside the edits; a `.tex` file the same way, byte for byte. The Book: [Org](book/part-2/org.org).
- **Terminal users.** The same editor over SSH and in tmux, with rendered documents, images where the terminal draws them, and a scriptable command line. The Book: [Starting](book/part-1/starting.org), [The command line](book/part-1/the-command-line.org).
- **Programmers**, later: plain text with highlighting today, language servers through plugins in phase 3, and a plugin API in the language the editor is written in. The Book: [Plain text and code](book/part-1/plain-text-and-code.org), [Plugins](book/part-4/plugins.org).

## Works today

- **Org as a document**, checked against Emacs: headings, emphasis, lists and checkboxes, links, footnotes, citations with a picker, pictures, LaTeX formulas drawn inline, source blocks with highlighting, folding, the outline, and a source view to switch to at any time. TODO states, priorities, tags, properties, scheduling with a date picker, state logging, repeaters, TODO dependencies and match strings (`kalem query`), each command identical to Emacs on thousands of cases. Tables with `#+TBLFM` formulas and Calc's functions, a formula bar, recalculation and CSV import and export. Export to HTML, Markdown, GitHub Markdown, LaTeX, PDF and plain text matching Emacs's exporter, with citations in Org's `basic` styles or any CSL style, or through biblatex and natbib; Word, OpenDocument, EPUB and RTF through pandoc.
- **Markdown as a document**: headings, emphasis, code, links, images and formulas drawn with the markers hidden away from the cursor; task lists toggled by a click; tables with Org's table keys and `<!-- TBLFM -->` formulas; lists that continue on Enter, move and renumber; front matter folded and edited as a form; wiki links resolved and completed in the project; pictures dropped or pasted into `images/`; Copy as HTML and as Rich Text; Convert to Org without pandoc; reparsed incrementally, files up to 2 MiB drawn as they read.
- **LaTeX as a document**: sections, formulas, references, citations, figures, tables and code rendered; multi-file projects; PDF builds with diagnostics at their files and lines; completion of commands, labels and citations; `.tex` files stay LaTeX byte for byte.
- **CSV as a spreadsheet-like grid**: column letters and row numbers, sorting, filters, a record view, column statistics and a frequency table, fill down and fill series, split and join columns, duplicates removed, the dialect kept; **BibTeX as a grid** of entries.
- **Around the files.** Projects, a folder tree, find in files, a file manager with the keys every file manager has, plain text with highlighting, themes, settings in TOML, keymaps in JSON, panes, workspaces and sessions, English and Turkish.
- **Two editors.** Graphical and terminal, with the same commands, keys, settings and menus; menus in the macOS menu bar and in the window's own menu bar on Linux and Windows; F10 lists every menu item in both.
- **The Kalem format**, on paper: the specification at draft 0.2 in Part III of the Book, the parser `klm-syntax` that parses, formats and round-trips every example, and the first files of the conformance suite (`tests/klm-spec`).

## Not yet

- The plugin runtime: Kalem cannot load a plugin today, so there are no viewers for files that are not text, no language servers and no spell checking
- The Kalem format in the editor: rendering, stylesheets and the exporters
- The agenda, capture, clocking reports, Babel (running source blocks)
- Signed binaries and installers for macOS, Windows and Linux

## Documentation

[The Kalem Book](https://getkalem.github.io/kalem) is the one reference: the manual (Part I), the specification of every format as Kalem implements it (Part II), the specification of the Kalem format (Part III), extending Kalem (Part IV) and the design (Part V). Its source is [`book/`](book/index.org), written in Org; `kalem book build` turns it into the site and `kalem book check` verifies it against the code.

## Command line

`kalem --help`:

```
Kalem: a fast, text-first editor that opens every plain-text format as itself and keeps it byte for byte: Org, Markdown, LaTeX, CSV, BibTeX and code

Usage: kalem [FILE | FOLDER]      the editor: graphical where there is a display, else in the terminal
       kalem gui [FILE]           the graphical editor
       kalem tui [FILE]           the terminal editor (also kalem -t [FILE])
       kalem tui --detect         what the terminal can do
       kalem <COMMAND>            a command-line tool

Commands:
  parse        Print the syntax tree of a file
  check        Check files: syntax diagnostics and round-trip verification (Org and LaTeX files)
  commands     List the commands, one a line: ID, title, scope and keys; with `--type`, those that serve that text type (`org`, `python`, `csv`)
  complete     Print the completions at a place in a file, one a line: label, kind and the completer (`kalem complete notes.org:12:5`)
  fmt          Align tables and tags, and blank lines as each file has them; the Kalem format's canonical form
  export       Export Org files as Emacs's Org exporter does: `kalem export notes.org --to html` writes `notes.html` beside it (or the file `#+EXPORT_FILE_NAME` names)
  import       Convert Word, OpenDocument, Markdown, HTML, EPUB or RTF files to Org through pandoc, cleaned up: `kalem import report.docx` writes `report.org` beside it, its pictures in `report_assets`
  diff-pandoc  Compares the structure Kalem reads in LaTeX files with pandoc's LaTeX reader: headings, formulas, citations, footnotes, figures, tables, code blocks and list items (development)
  latex        LaTeX documents: `kalem latex build FILE`
  book         The Book: `kalem book build`, `kalem book check`
  table        Table formulas: `kalem table recalc FILE...`
  query        Print the headlines matching an Org match string, such as `kalem query notes.org 'TODO="NEXT"+work'`
  dump         Dump the parse tree in a machine-readable format
  diff-emacs   Compare the parse with Emacs's org-element (development tool)
  help         Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

Planned: `kalem agenda`, `kalem plugin` to build and install plugins, and `kalem run` for a plugin's command in batch.

## Repository layout

| Path | Contents |
|---|---|
| `crates/org-syntax` | Lossless, incremental Org parser, usable on its own |
| `crates/org-model` | Org document model: outline, TODO states, tags, properties, match strings, links, statistics, clocks |
| `crates/org-edit` | Org editing commands with undo, identical to Emacs's |
| `crates/org-table` | Org tables and `#+TBLFM` formulas, shared with Markdown tables |
| `crates/org-export` | Org exporters: HTML, Markdown, GitHub Markdown, LaTeX, plain text and Org, with citations (a port of `ox.el`) |
| `crates/org-cite` | Citations: BibTeX and CSL-JSON bibliographies, Org's `basic` processor and CSL styles |
| `crates/latex-syntax` | Lossless, incremental LaTeX parser |
| `crates/latex-model` | The document model of LaTeX files: structure, numbering, labels, citations and definitions |
| `crates/org-math` | LaTeX formulas drawn natively |
| `crates/klm-syntax` | The parser and canonical formatter of the Kalem format, with byte ranges and incremental reparsing |
| `crates/kalem-core` | The editor's model, shared by both frontends: documents and their modes (Org, Markdown, LaTeX, CSV, BibTeX, plain text), the mode and completer contracts, commands, keymaps, settings, the file manager, projects |
| `crates/kalem-ui`, `crates/gpui-rich-text` | The graphical editor |
| `crates/kalem-tui`, `crates/tui-rich-text` | The terminal editor |
| `crates/kalem-fs`, `crates/kalem-project`, `crates/kalem-highlight` | Files, projects and syntax highlighting |
| `crates/kalem-cli` | Command-line subcommands |
| `crates/kalem` | The `kalem` binary (published as `kalem-editor`) |
| `tests/corpus`, `tests/latex`, `tests/csv`, `tests/klm-spec` | Real-world files and conformance suites used for testing |
| `tests/emacs` | Scripts that compare Kalem's Org parser, commands and exporters with Emacs |
| `book/` | The Kalem Book, in Org: the manual, the formats, the Kalem format, extending Kalem, the design |
| `design_document.md` | The design document (RFC 0001) |
| `design_doc2.md` | The second design document (RFC 0002): the standard modes, the Kalem format, the Book |
| `rfcs/` | RFC 0003, the Kalem format, and the RFC process |
| `todo.md` | The open tasks, in the order they are to be done, under general headings, each with its reason, test and done-criterion and the points that need a decision |
| `todo_old.md` | The former work breakdown, frozen on 2026-10-01: the done and cancelled tasks, the decision table, the history |

Markdown is parsed by [comrak](https://github.com/kivikakk/comrak), the Rust port of GitHub's own parser, in Kalem's fork [`getkalem/comrak`](https://github.com/getkalem/comrak), where the source positions an editor needs are fixed and offered upstream.

## Building

Kalem needs a recent stable Rust toolchain.

```bash
cargo build --release                                              # both editors
cargo build --release -p kalem-editor --no-default-features --features tui  # terminal only
```

The graphical editor needs gpui's system libraries on Linux (for example `libxkbcommon-dev`, `libvulkan-dev` and `libwayland-dev`).

The differential tests against Emacs need Emacs 29 or newer with Org 9.7 on your `PATH`; the LaTeX comparisons need pandoc, and building PDFs needs a TeX distribution.

## History

Kalem started on 2026-09-27 as "Typora for Org", an editor for Org files for people who do not use Emacs. The engine that made that possible, a lossless, range-based editor verified against a reference implementation, turned out to serve every plain-text format the same way, and on 2026-09-30 the project became what this page describes ([RFC 0002](design_doc2.md)): one fast editor for every text file, each opened as itself.

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
