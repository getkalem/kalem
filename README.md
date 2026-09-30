# Kalem

**Kalem** ("pen" in Turkish) is a lightweight, fast, open source editor for [Org mode](https://orgmode.org) files, for people who do not use Emacs.

It opens `.org` files the way Word opens documents: headings, emphasis, lists, checkboxes, tables with formulas, footnotes, images, citations and LaTeX math are shown formatted and edited directly. Underneath, the file stays plain Org text, byte for byte compatible with Emacs, so you can share documents with Emacs users without either side noticing.

In short: **Typora for Org.**

> **Status: alpha, not released yet.** Phase 1 of the [roadmap](design_document.md#20-roadmap) (the editor) is done and phase 2 (the everyday features) is about half done. There are no binaries yet: build from source to try it. Follow the progress in [`todo.md`](todo.md) and [`CHANGELOG.md`](CHANGELOG.md).

<!-- A screenshot or GIF of both editors goes here (todo.md, T1.8.7). -->

### Works today

- **Two editors, one behavior.** A graphical editor (gpui) and a terminal editor (ratatui) with the same commands, keys, settings and menus; Word-like keys by default, Vim keys as an option.
- **Org as a document.** Headings, emphasis, lists and checkboxes, links, footnotes, images (terminal), LaTeX formulas drawn inline, source blocks with highlighting, folding, the outline, and a source view to switch to at any time.
- **Byte-for-byte Org.** The parser agrees with Emacs's `org-element` on the Org manual, Org's own tests and all of Worg; a file saved by Kalem changes only where you edited it.
- **Tasks.** TODO states, priorities, tags, properties, scheduling with a date picker, state logging, repeaters, TODO dependencies, and match strings (`kalem query`), each command identical to Emacs on thousands of cases.
- **Tables and formulas.** Automatic alignment, a grid editor, `#+TBLFM` formulas with Calc's functions, durations and dates, a formula bar, recalculation, import and export (CSV, TSV).
- **Export.** HTML (with Kalem's style sheet, MathJax or SVG formulas), Markdown or GitHub Markdown, LaTeX and PDF, and plain text, matching Emacs's exporter, with citations and bibliographies in Org's `basic` styles or any CSL style (APA, IEEE, Chicago…), or left to biblatex and natbib in LaTeX; Word, OpenDocument, EPUB and RTF through pandoc; `.klm` Kalem documents with fonts, colors and alignment that stay valid Org.
- **LaTeX, CSV and BibTeX.** `.tex` files rendered as the document reads (sections, formulas, references, citations, figures, tables) and built to PDF, with diagnostics and completion, staying LaTeX byte for byte; CSV files as a grid with sorting and filters; `.bib` files as a grid of entries.
- **Around the files.** Projects, a folder tree, find in files, a Dired-style file manager, plain text with highlighting, themes, English and Turkish.

### Not yet

- A citation picker
- Markdown as a document (today it opens as text)
- The agenda, capture, clocking reports, Babel (running source blocks)
- Plugins (planned as WebAssembly components), spell checking
- Signed binaries and installers for macOS, Windows and Linux

## Goals

- **Lossless.** A file saved by Kalem differs from the original only where you edited it.
- **No Emacs required.** Write documents, tasks, tables and formulas without seeing Org syntax, or toggle the source view when you want it.
- **Light and fast.** One small native binary written in Rust. No Electron.
- **Org built in.** Headlines, TODOs, tags, scheduling, tables and formulas (`#+TBLFM`), footnotes, links, source blocks, citations, export.
- **Books and papers.** LaTeX math preview, export to LaTeX, PDF, HTML, Markdown, and through pandoc to DOCX, ODT and EPUB.
- **Desktop and terminal.** A graphical editor, a terminal editor with the same behavior, and a scriptable command line.
- **Extensible.** Plugins written in Rust and run as sandboxed WebAssembly components add modes, completers, commands, link types, block renderers, views, exporters and document checks, while documents stay valid Org.

What Kalem is not: a Microsoft Office clone, a page layout tool, or a replacement for Emacs. See [non-goals](design_document.md#14-non-goals).

## Documentation

[The Kalem Book](https://getkalem.github.io/kalem) is the reference: the manual, the specification of every format as Kalem implements it, extending Kalem, and the design. Its source is [`book/`](book/index.org); `kalem book build` turns it into the site.

## Command line

```bash
kalem notes.org                       # open in the graphical editor
kalem tui notes.org                   # open in the terminal editor
kalem check notes.org                 # syntax diagnostics, round-trip check
kalem fmt notes.org                   # align tables and tags, normalize spacing
kalem export notes.org --to html      # export: html, md, gfm, org
kalem table recalc budget.org         # recompute #+TBLFM formulas
kalem query notes.org 'TODO="NEXT"'   # headlines matching an Org match string
```

Planned: `kalem export --to pdf`, `kalem agenda`, and `kalem run` for a plugin's command in batch, like `emacs --batch`.

## Repository layout

| Path | Contents |
|---|---|
| `crates/org-syntax` | Lossless, incremental Org parser, usable on its own |
| `crates/org-model` | Document model: outline, TODO states, tags, properties, match strings, links, statistics, clocks |
| `crates/org-edit` | Org editing commands with undo, identical to Emacs's |
| `crates/org-table` | Tables and `#+TBLFM` formulas |
| `crates/org-export` | Exporters: HTML, Markdown, GitHub Markdown (a port of `ox.el`) |
| `crates/org-math` | LaTeX formulas drawn natively |
| `crates/kalem-core` | The editor's model, shared by both frontends: documents, commands, keymaps, settings |
| `crates/kalem-ui`, `crates/gpui-rich-text` | The graphical editor |
| `crates/kalem-tui`, `crates/tui-rich-text` | The terminal editor |
| `crates/kalem-fs`, `crates/kalem-project`, `crates/kalem-highlight` | Files, projects and syntax highlighting |
| `crates/kalem-cli` | Command-line subcommands |
| `crates/kalem` | The `kalem` binary (published as `kalem-editor`) |
| `tests/corpus` | Real-world Org files used for testing |
| `tests/emacs` | Scripts that compare Kalem's parser, commands and exporters with Emacs |
| `design_document.md` | The design document (RFC 0001) |
| `todo.md` | The work breakdown |

## Building

Kalem needs a recent stable Rust toolchain.

```bash
cargo build --release                                              # both editors
cargo build --release -p kalem-editor --no-default-features --features tui  # terminal only
```

The graphical editor needs gpui's system libraries on Linux (for example `libxkbcommon-dev`, `libvulkan-dev` and `libwayland-dev`).

The differential tests against Emacs need Emacs 29 or newer with Org 9.7 on your `PATH`.

## Contributing

Contributions are welcome. Start with [`CONTRIBUTING.md`](CONTRIBUTING.md) and the [design document](design_document.md). Please follow the [code of conduct](CODE_OF_CONDUCT.md).

## Contact

Maintainer contact details will be published here before the first public release.

## License

Licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE))
- MIT license ([`LICENSE-MIT`](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

Test corpus files under `tests/corpus` keep their original licenses, listed in [`tests/corpus/LICENSES.md`](tests/corpus/LICENSES.md).
