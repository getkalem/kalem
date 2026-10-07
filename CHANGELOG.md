# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- A settings panel in the terminal editor (Settings: Alt+, or Ctrl+,, `SPC h v`, the Kalem menu): every setting grouped by its table, changed in place with lazygit-like keys (`j`/`k` choose, `h`/`l` or Space change, Enter types a text, `/` filters, `d` back to the default, `e` opens `settings.toml`).
- A Projects row in the settings panel of the graphical editor: projects added by hand only, or folders under version control too.
- More of Doom Emacs's leader keys with Vim keys, checked against Doom's own map: `SPC b -` (narrow or widen), `SPC b C`, `SPC b I`, `SPC c S` (the outline), `SPC p X`, `SPC p &`, `SPC s O`, `SPC n F` (Browse Notes, new), `SPC o o` and `SPC o O` (Show in System File Manager, new: Finder on macOS), `SPC o i`, `SPC o I`, `SPC TAB D`, `SPC TAB R`, `SPC TAB 0`, the window map's `SPC w S`, `V`, `W`, `C-h`/`C-j`/`C-k`/`C-l`, `C-w`, `C-o`, `C-u` and `C-r`, and `SPC h c`, `o`, `V`, `O`, `p` (the installed plugins), `b t`, `b m`, `r t`, `r f`; in Org `SPC m @` (cite), `SPC m ,`, `SPC m +`, `SPC m g G`, `SPC m c E`, `SPC m b i H`; in Markdown `SPC m i e`, `SPC m i s`, `SPC m t x`; in LaTeX `SPC m ;` (the outline).

### Changed
- Projects are added by hand by default: opening a file in a folder under version control no longer adds that folder to the project list. Toggle Adding Projects Automatically (Project menu) or the settings panel turns it back on (`projects.auto_add`).
- Leader keys that did something else than Doom's now do what Doom's do: `SPC q F` closes every document (it quit), `SPC TAB x` is Doom's "kill session" and not there yet (Delete Saved Workspace moved to `SPC TAB D`), `SPC b X` is the scratch document (the project's is `SPC p X`), `SPC o P` reveals the file in the folder tree (the projects view moved to `SPC p P`), `SPC o o` shows the file in Finder (the outline is `SPC c S`); in Org `SPC m n` stores a link (narrowing stays on `SPC m s n`, and `SPC m N` is gone for `SPC m s N`), `SPC m l S` inserts the stored link and `SPC m l i` stores one, `SPC m g r` no longer refiles; in LaTeX `SPC m c` builds (Complete stays on Ctrl+Space), `SPC m p` toggles the formulas' preview and Next Problem moved to `SPC m n`.

### Fixed
- The terminal editor applies a changed setting at once (line numbers, wrapping, line width, centering, where the open files show, the keys and the Vim layer, the theme), from the settings panel, a command such as Toggle Line Numbers, or Reload Settings and Keys; before, only a restart showed it.

## [0.1.0] - 2026-10-06

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
- Excel workbooks with a password to open (Excel 2007's and 2010's encryption) open with the password Kalem asks for and are saved encrypted again with it (xlsx 0.0.8).

### Fixed
- Vim's `:q` closes the document when the window is not split, as a tab closes, instead of quitting Kalem with other documents open; Kalem quits from it only when nothing is left to close (in the terminal, from the last document). `:q!` loses the document's changes and no other's (it quit Kalem, losing every document's); `:qa!` quits without saving after asking.
- Add Project (`SPC p a`) asks for the folder, the document's own offered, instead of adding the document's folder unasked, which showed nothing to see when it was a project already. A path typed for it or for Open File (`SPC .`) lists the folder's entries, and Tab completes it.
- A line longer than 20,000 bytes (a minified script) shows without syntax colors: a 1 MB one took 3.6 s at each keystroke.
- A UTF-8 file with a stray byte is read as UTF-8 (the byte shown as �), not as Windows-1252 throughout; one with a byte order mark and a stray byte opens. Save As takes the new name's mode and does what Save does (the events, trailing blanks, a build on save); overwriting a file changed on disk does too.
- Line endings and the byte order mark can be changed (Line Endings: LF or CRLF, Byte Order Mark: Add or Remove), and the status bar names CRLF and a BOM; a regular expression's `$` matches before a CR LF.
- Vim keys: in visual mode Page Down and Page Up (fn with the arrows on a Mac), Home, End and the arrows with Option grow the selection, as Vim's `<PageDown>` does; they ended it.
- CSV: a new `.tsv` is read with tabs; a header alone is a header; Enter, Tab and Shift with the arrows step through the rows a filter or a sort shows; cells pasted from a spreadsheet keep their line breaks and quotes, and one line without a tab is one value; `kalem check` checks the round trip.
- LaTeX: a document's macro for `\tag` (`\numberthis`) or for `\nonumber` (`\nn`) numbers its line as LaTeX does, without false label warnings; a bibliography in `BIBINPUTS` or found by `kpsewhich` is read; a shared library cited in part gives one note, not one per entry.
- Citation previews and the bibliography styles show an entry as it prints: `B\"uy\"uk` is Büyük and `{\TeX}book` TeXbook.
- LaTeX: a path through `..` on the command line finds its root document; Show in PDF says when the file is not in the PDF rather than asking to build again.
- LaTeX: `\nocite{*}` is no unknown key, and the citation messages name a key as LaTeX writes it; `% !TEX program` and `% !TEX root` are read after a byte order mark.
- A bibliography in Latin-1 or UTF-16 is read, as the editors read the file, instead of being reported unreadable with its keys unknown.
- Markdown: Sort Rows puts Turkish letters in the alphabet's order, as CSV's sort does; a front matter list item with a comma (`"Doe, Jane"`) stays one item when the field is edited.
- Markdown: a footnote mark without a definition, or a definition inside a code block, no longer makes every keystroke parse the whole document; footnote definitions are found on their lines.
- Convert to Org keeps text as text: what Org would read as markup or structure (`\*x\*`, `/x/`, `\# x`) gets the zero-width space Org's manual advises; wiki links become links to the page's file; a heading in a quote no longer ends the quote.
- A picture that is not drawn (a README's remote badge) shows its alt text, or its file's name, not its whole address.
- Markdown: a displayed formula over several lines (`$$` on lines of their own) is drawn, as one formula on its first line away from the cursor, in both editors.
- Markdown: TOML front matter (`+++`), as Hugo writes it, is front matter: folded away from the cursor, its `# comments` no headings.
- A table wider than the window (Org, Markdown, LaTeX) is drawn as a grid whose widest columns are narrowed and whose cells wrap inside them, in both editors; its rows were broken across lines, the bars and rules with them. Markdown tables are drawn in the proportional grid of Org's.
- Exporting a paragraph of thousands of links to HTML takes as long as to Markdown (8,000 links: 19 s to 0.6 s in a debug build), and a list nested thousands of levels deep exports instead of overflowing the stack.
- `kalem fmt`, `kalem export` and `kalem query` take folders as `kalem check` does; `export` and `query` report a file they cannot read and go on; `kalem query` says what part of a match string it left out, as Emacs leaves it out.
- Org: an export stops, as Emacs's does, at a footnote with no definition; `kalem check` reports a footnote with no definition or with two, and an `#+INCLUDE`, a `#+SETUPFILE` or a `file:` link whose file is not there.
- Save As a workbook of another kind (`.xlsm` as `.xlsx`, a template as a workbook) writes the content type of that kind and, where macros are not allowed, leaves the VBA project out: Excel refused the file.
- Workbooks: a formula typed with one of Excel's newer functions (`XLOOKUP`, `TEXTJOIN`, `IFS`, `FILTER`…) is saved as Excel reads it; it showed `#NAME?` in Excel. A workbook protected by a password says so instead of calling its file malformed. (The workbook plugin 0.0.6, built in.)
- Org's entities (`\alpha`, `\nbsp`) render as the HTML Standard, LaTeX and Unicode define them: their table is no longer taken from Emacs's `org-entities.el`, which is under the GPL. The names are Org's; a few renderings differ from Emacs's (`\phi` is LaTeX's `ϕ`), and the ASCII and Latin-1 forms are Unicode's.
- The plugin API is 0.2.3: a viewer may export `formats`, writing a document as another of its formats and making a new file from rows of entries, so that the formats' code lives in their plugins, not in Kalem.
- The workbook's formats are written by its plugin (0.0.7), not by Kalem: New Workbook, Open as Workbook, New from Template and Save As another workbook kind or `.ods` go through the plugin API's `formats` interface. A date that is none, typed in a text file opened as a workbook (`2026-02-30`), stays text; its cell was dropped.
- `kalem plugin new NAME` starts a working viewer of a small format (a picture drawn in text), each part of the contract in its place, instead of a manifest alone; the Book's Part III opens with *Plugins in practice*: adding a plugin from the editor, changing one and using your own copy, and writing one for a file type of your own.
- Projects added in Kalem no longer disappear: two Kalems at once (the window and the terminal) kept only the projects of the one that saved last, and a project list that could not be read was saved over as empty; now a save keeps what the other added, and a damaged list is kept aside as `projects.toml.broken` (`plugins.toml` likewise). The graphical editor's tests no longer write the user's project list.
