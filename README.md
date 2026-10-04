# Kalem

Kalem is a text editor that shows a file the way it reads and never touches what you did not edit. Org, Markdown, LaTeX, CSV and BibTeX files open as documents and grids; PDF files, pictures and Excel workbooks open in viewers; every other text file opens as code. One program, in a window or in a terminal.

<!-- A GIF of the editor goes here (docs/roadmap.md, R2.5): ![Kalem](assets/kalem.gif) -->

**Status: alpha.** The first release, 0.1, is being prepared ([roadmap](docs/roadmap.md), M2); until it is out, build from source, below.

## Why

- **The file stays yours.** Kalem edits the text in place and saves it byte for byte. No database, no import, no "save as". A co-author who uses Emacs, a TeX editor or Excel sees only your edits in the diff.
- **Every format as itself.** Kalem writes nothing into a file that its format does not define, and shows what it does not understand as source.
- **Checked against the reference.** The Org parser, commands and exporters are compared with Emacs on thousands of files; Markdown with the CommonMark and GFM test suites; LaTeX with pandoc and a TeX engine; CSV with the files spreadsheets write.
- **Fast.** One binary, no runtime. A keystroke in a paragraph reparses in a twentieth of a millisecond; the [measurements](book/part-5/performance.org) give each number with the command that reproduces it.

## What opens as what

| File | What you see |
|---|---|
| `.org` | A document: folding, TODO states, dates, tags, tables with formulas, footnotes, citations, typeset formulas. Export to HTML, Markdown, LaTeX, PDF and text as Emacs does; Word, OpenDocument and EPUB through pandoc. |
| `.md` | A document, GitHub flavor: task lists, tables with formulas, front matter, wiki links, pictures. |
| `.tex` | A document: numbered sections, typeset formulas, references, citations, figures; PDF builds with the errors at their lines. |
| `.csv`, `.tsv` | A grid: sorting, filters, a record view, column statistics. |
| `.bib` | A grid of entries. |
| `.pdf` | A viewer: pages, zoom, search, and from a LaTeX build, Ctrl-click back to the source line. |
| Pictures | A viewer for PNG, JPEG, GIF, WebP, TIFF, SVG and a dozen more, in the terminal too where it draws pictures. |
| `.xlsx` | A workbook: sheets as grids with their formulas and charts. |
| Anything else | Code with highlighting. |

Around them: a file manager, projects, find in files, a command palette, Vim keys with Doom Emacs's leader, themes, English and Turkish. The viewers are plugins: WebAssembly components with their own memory and only the permissions they declare, bundled into the binary; `kalem plugin install` adds others from the index. Not yet: the agenda, signed installers.

## Install

From a release, once 0.1 is out, on macOS and Linux:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/getkalem/kalem/releases/latest/download/kalem-editor-installer.sh | sh
```

and on Windows, in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/getkalem/kalem/releases/latest/download/kalem-editor-installer.ps1 | iex"
```

From source, with Rust 1.88 or later; on Linux the graphical editor also needs gpui's libraries, listed in the Book's [Installing](book/part-1/installing.org):

```bash
cargo install --path crates/kalem
```

```bash
kalem notes.org                     # the editor; `kalem tui notes.org` for the terminal
kalem export notes.org --to html    # the command-line tools: check, fmt, query, export…
```

## More

- [The Kalem Book](https://getkalem.github.io/kalem): the manual, and what Kalem does with each format. [Kalem and Emacs](book/part-5/kalem-and-emacs.org): what is taken from Emacs, and what is left out on purpose.
- [Contributing](CONTRIBUTING.md), the [design documents and task list](docs/README.md), the [changelog](CHANGELOG.md).
- License: MIT or Apache-2.0, at your option.
