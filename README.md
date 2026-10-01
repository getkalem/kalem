# Kalem

Kalem is a text editor that shows a file the way it reads and never touches what you did not edit. Org, Markdown, LaTeX, CSV and BibTeX files open as documents and grids; every other text file opens as code. One program, in a window or in a terminal.

<!-- A GIF of the editor goes here (docs/todo.md, T1.8.7): ![Kalem](assets/kalem.gif) -->

**Status: alpha.** There are no binaries yet; build from source, below.

## Why

- **The file stays yours.** Kalem edits the text in place and saves it byte for byte. No database, no import, no "save as". A co-author who uses Emacs, a TeX editor or Excel sees only your edits in the diff.
- **Every format as itself.** Kalem writes nothing into a file that its format does not define, and shows what it does not understand as source.
- **Checked against the reference.** The Org parser, commands and exporters are compared with Emacs on thousands of files; Markdown with the CommonMark and GFM test suites; LaTeX with pandoc and a TeX engine; CSV with the files spreadsheets write.
- **Fast.** One static binary, no runtime. It starts in about 150 ms, and a keystroke reaches the screen in under 10 ms.

## What opens as what

| File | What you see |
|---|---|
| `.org` | A document: folding, TODO states, dates, tags, tables with formulas, footnotes, citations, typeset formulas. Export to HTML, Markdown, LaTeX, PDF and text as Emacs does; Word, OpenDocument and EPUB through pandoc. |
| `.md` | A document, GitHub flavor: task lists, tables with formulas, front matter, wiki links, pictures. |
| `.tex` | A document: numbered sections, typeset formulas, references, citations, figures; PDF builds with the errors at their lines. |
| `.csv`, `.tsv` | A grid: sorting, filters, a record view, column statistics. |
| `.bib` | A grid of entries. |
| Anything else | Code with highlighting. |

Around them: a file manager, projects, find in files, a command palette, Vim keys with Doom Emacs's leader, themes, English and Turkish. Not yet: the agenda, plugins, signed installers.

## Try it

Needs Rust 1.88 or later. On Linux the graphical editor also needs gpui's libraries, listed in the Book's [Installing](book/part-1/installing.org).

```bash
cargo install --path crates/kalem
```

```bash
kalem notes.org                     # the editor; `kalem tui notes.org` for the terminal
kalem export notes.org --to html    # the command-line tools: check, fmt, query, export…
```

## More

- [The Kalem Book](https://getkalem.github.io/kalem): the manual, and what Kalem does with each format.
- [Contributing](CONTRIBUTING.md), the [design documents and task list](docs/README.md), the [changelog](CHANGELOG.md).
- License: MIT or Apache-2.0, at your option.
