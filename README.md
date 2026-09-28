# Kalem

**Kalem** ("pen" in Turkish) is a lightweight, fast, open source editor for [Org mode](https://orgmode.org) files, for people who do not use Emacs.

It opens `.org` files the way Word opens documents: headings, emphasis, lists, checkboxes, tables with formulas, footnotes, images, citations and LaTeX math are shown formatted and edited directly. Underneath, the file stays plain Org text, byte for byte compatible with Emacs, so you can share documents with Emacs users without either side noticing.

In short: **Typora for Org.**

> **Status: pre-alpha.** Kalem is in phase 0 of its [roadmap](design_document.md#20-roadmap): the parser and the foundations are being built. There is nothing to use yet. Follow the progress in [`todo.md`](todo.md).

## Goals

- **Lossless.** A file saved by Kalem differs from the original only where you edited it.
- **No Emacs required.** Write documents, tasks, tables and formulas without seeing Org syntax, or toggle the source view when you want it.
- **Light and fast.** One small native binary written in Rust. No Electron.
- **Org built in.** Headlines, TODOs, tags, scheduling, tables and formulas (`#+TBLFM`), footnotes, links, source blocks, citations, export.
- **Books and papers.** LaTeX math preview, export to LaTeX, PDF, HTML, Markdown, and through pandoc to DOCX, ODT and EPUB.
- **Desktop and terminal.** A graphical editor, a terminal editor with the same behavior, and a scriptable command line.
- **Extensible.** JavaScript and TypeScript plugins can add commands, link types, block renderers, views, exporters and document checks, while documents stay valid Org.

What Kalem is not: a Microsoft Office clone, a page layout tool, or a replacement for Emacs. See [non-goals](design_document.md#14-non-goals).

## Planned command line

```bash
kalem notes.org                       # open in the graphical editor
kalem tui notes.org                   # open in the terminal editor
kalem check notes.org                 # syntax diagnostics
kalem fmt notes.org                   # align tables, normalize spacing
kalem export book.org --to pdf        # export
kalem agenda --week ~/org             # print the agenda
kalem run script.js notes.org         # batch scripting, like emacs --batch
```

## Repository layout

| Path | Contents |
|---|---|
| `crates/org-syntax` | Lossless, incremental Org parser, usable on its own |
| `crates/org-model` | Document model: outline, TODO states, tags, properties, match strings, links, statistics, clocks |
| `crates/kalem-cli` | Command-line subcommands |
| `crates/kalem` | The `kalem` binary (published as `kalem-editor`) |
| `tests/corpus` | Real-world Org files used for testing |
| `tests/emacs` | Scripts that compare Kalem's parser with Emacs's `org-element` |
| `design_document.md` | The design document (RFC 0001) |
| `todo.md` | The work breakdown |

## Building

Kalem needs a recent stable Rust toolchain.

```bash
cargo build --release
```

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
