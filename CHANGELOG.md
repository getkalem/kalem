# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

The first release. Kalem is an editor for plain-text documents that
shows each format as it reads and keeps the file byte for byte; the
changes made on the way here are recorded in
[`docs/history/changelog-before-0.1.md`](docs/history/changelog-before-0.1.md).

### Added
- Org, compared with Emacs on thousands of files: a lossless parser that agrees with `org-element`, the document drawn as it reads, folding, TODO states and logging, dates and the date picker, tags, properties, priorities, links, footnotes, lists, tables with Emacs's formulas, citations (`org-cite`, CSL styles), typeset formulas, inline pictures, and the in-buffer settings (`#+TODO`, `#+STARTUP`, `#+SETUPFILE`…). Export to HTML, Markdown, LaTeX, PDF and text as Emacs exports, and to Word, OpenDocument, EPUB and RTF through pandoc; import from those formats through pandoc.
- Markdown, GitHub's flavor (all of CommonMark 0.31.2's and GFM's examples agree): markers hidden away from the cursor, tables as grids, task lists, front matter and Edit Properties, wiki links, heading links, pictures, table formulas, list and table editing.
- LaTeX: a rendered view (sections, numbered equations and floats with LaTeX's numbers, theorems, references, citations from BibTeX, pictures, tables as grids) where what is not rendered stays as source; Build PDF with latexmk, the engine or Tectonic, the log's problems at their lines, SyncTeX both ways; diagnostics and quick fixes; completion; `kalem fmt` for LaTeX.
- CSV and TSV as a grid: the dialect detected (`;` and decimal commas included), sorting, filters, the record view, column statistics, Excel's `sep=` line; only the fields edited are written.
- BibTeX files as a grid of entries, checked for unclosed entries, duplicate keys and missing fields.
- Viewers, built in as WebAssembly components from getkalem/plugins: PDF files (outline, links, search, text selection, passwords, Ctrl-click back to a LaTeX source line), pictures (PNG, JPEG, GIF and animations, WebP, TIFF, EXR and more, EXIF orientation), and Excel workbooks, edited and saved as themselves (formulas through IronCalc, styles, charts, pivot tables, macros run on request); `.ods`, `.xls` and `.xlsb` converted to edit.
- Every other text file as code, coloured by its language; language servers through language plugins (diagnostics, completion, hover, rename, code actions, formatting).
- The editor around them: one window of documents by project, a file manager, find and replace in a file and in files, a command palette for every command, an outline, split views, themes, English and Turkish, Word-like keys by default and Vim's with Doom Emacs's leader, every key configurable.
- The terminal editor: the same documents, commands and keys in a terminal, pictures and formulas drawn where the terminal has graphics.
- The command line: `check`, `fmt`, `export`, `import`, `view`, `query`, `table recalc`, `latex build`, `complete`, `commands`, `plugin` and `lsp`, with JSON output for scripts.
- Plugins: viewers and extension plugins (commands, keys, events, the document, settings, panels) as WebAssembly components in a sandbox with their own memory, time and permissions, on a versioned API (0.2.2); declarative language plugins; `kalem plugin` to browse the index, install from it or from GitHub, list, check, remove, start, build and develop plugins.
- A log and crash reports in the state folder; settings in `settings.toml`, workspace settings per project, and `keymap.json`.

### Fixed
- `kalem fmt`, `kalem export` and `kalem query` take folders as `kalem check` does; `export` and `query` report a file they cannot read and go on; `kalem query` says what part of a match string it left out, as Emacs leaves it out.
- Org: an export stops, as Emacs's does, at a footnote with no definition; `kalem check` reports a footnote with no definition or with two, and an `#+INCLUDE`, a `#+SETUPFILE` or a `file:` link whose file is not there.
- Save As a workbook of another kind (`.xlsm` as `.xlsx`, a template as a workbook) writes the content type of that kind and, where macros are not allowed, leaves the VBA project out: Excel refused the file.
- Workbooks: a formula typed with one of Excel's newer functions (`XLOOKUP`, `TEXTJOIN`, `IFS`, `FILTER`…) is saved as Excel reads it; it showed `#NAME?` in Excel. A workbook protected by a password says so instead of calling its file malformed. (The workbook plugin 0.0.6, built in.)
- Org's entities (`\alpha`, `\nbsp`) render as the HTML Standard, LaTeX and Unicode define them: their table is no longer taken from Emacs's `org-entities.el`, which is under the GPL. The names are Org's; a few renderings differ from Emacs's (`\phi` is LaTeX's `ϕ`), and the ASCII and Latin-1 forms are Unicode's.
- The plugin API is 0.2.3: a viewer may export `formats`, writing a document as another of its formats and making a new file from rows of entries, so that the formats' code lives in their plugins, not in Kalem.
- The workbook's formats are written by its plugin (0.0.7), not by Kalem: New Workbook, Open as Workbook, New from Template and Save As another workbook kind or `.ods` go through the plugin API's `formats` interface. A date that is none, typed in a text file opened as a workbook (`2026-02-30`), stays text; its cell was dropped.
- `kalem plugin new NAME` starts a working viewer of a small format (a picture drawn in text), each part of the contract in its place, instead of a manifest alone; the Book's Part III opens with *Plugins in practice*: adding a plugin from the editor, changing one and using your own copy, and writing one for a file type of your own.
- Projects added in Kalem no longer disappear: two Kalems at once (the window and the terminal) kept only the projects of the one that saved last, and a project list that could not be read was saved over as empty; now a save keeps what the other added, and a damaged list is kept aside as `projects.toml.broken` (`plugins.toml` likewise). The graphical editor's tests no longer write the user's project list.
