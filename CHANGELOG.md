# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Design document and work breakdown.
- Cargo workspace with `org-syntax`, `kalem-cli` and the `kalem` binary.
- Dual MIT OR Apache-2.0 license, contribution guide, code of conduct, CI.
- `org-syntax`: a lossless Org parser that follows `org-element.el`. On the test corpus (the Org manual, Org's test files and all of Worg) and on tens of thousands of randomly mutated files, every element and object and every property matches Emacs 30.1 / Org 9.7.11.
- `org-syntax::ast`: typed accessors for every org-element type and property.
- In-buffer settings: `#+TODO`, `#+SEQ_TODO`, `#+TYP_TODO`, `#+LINK`, `#+STARTUP: odd`, radio targets, and `#+SETUPFILE` through a pluggable loader.
- DOS line endings are parsed like Emacs parses them after decoding.
- Incremental reparsing (`Parse::reparse`): identical to a full parse, about 60 µs per keystroke in the 840 KB Org manual.
- Diagnostics (`Parse::diagnostics`) in the spirit of `org-lint`.
- No arbitrary limits: nesting up to 4096 levels (Emacs stops after a few hundred), no radio target limit, linear-time searches for block ends, brackets and emphasis.
- UTF-8 byte order marks are handled like Emacs (dropped for parsing, kept in the tree).
- `kalem parse`, `kalem check` (diagnostics, `--deny-warnings`, `--format json`), `kalem dump` and `kalem diff-emacs`.
- Radio links match through a trie of targets folded with Emacs's case-canon table, so documents with thousands of radio targets parse quickly (0.2 s for 1 MB with 10,000 targets).
- Documents with CRLF line endings or a byte order mark reparse incrementally.
- `org-model`: the document model as Org computes it: outline, TODO sequences, tags with inheritance, properties (drawers, `#+PROPERTY`, inheritance, special properties, allowed values), categories, match strings, internal link targets, footnote definitions, statistics cookies and clock totals, with a cache that reuses unchanged subtrees across edits. `kalem diff-emacs --model` compares it with Emacs.
- `ParseContext::todo_sequences` and `Parse::keywords`.
- `org-edit`: transactions with undo and redo, and Org headline commands that produce the text Emacs produces: promote and demote (headlines and subtrees), move, cut, copy and paste subtrees, and `org-sort-entries` by every key except a custom function. Checked against Emacs on 2,158 cases. Style inference reads a document's conventions (indentation, blank lines, keyword case, tags column) for new content.
- `org-edit`: TODO states (`org-todo` with keyword sets, CLOSED timestamps, state logging with notes and log drawers, parent statistics cookies and repeating tasks), priorities, and setting drawer properties, identical to Emacs on 3,577 command cases.
- Tags: setting, toggling and region changes of tags, tag selection with mutually exclusive groups, and group tags from `#+TAGS` in match strings.
- Plain lists: indenting, bullet cycling, checkboxes with statistics cookies, moving and inserting items, and repairing numbering, following `org-list.el`.
- `org_syntax::list_structure`: the structure org-element gives a plain list.
- Emphasis (`org-emphasize`, and a toggle for the editor that respects nesting) and insert commands for links, blocks, timestamps and horizontal rules.
- Tables: `org-table-align` and the row, column and field commands, with `#+TBLFM` references renumbered.
- Narrowing (subtree, element, block) and commands run on a narrowed part; snapshot tests for every editing command.
- `kalem-core`: document state with text, incremental and background parsing, model, selection, undo and document mode detection.
- `kalem-core`: the command registry with every editing command built in, key chords (`ctrl+shift+t`, Emacs style `C-c C-t`) and when-clauses (`editorMode == org && inTable`).
- Keymaps: the Word-like and Vim profiles, the user's `keymap.json`, a report of bindings that never apply, and terminal variants for chords that legacy terminals cannot send.
- Event bus with the events of the design, vetoes with a timeout that never blocks the UI thread, and debounced change events.
- Settings: built-in defaults, the user's `settings.toml` and the workspace's `.kalem/settings.toml`, checked value by value, with document keywords on top; values can be written back without losing comments.
- Files: atomic saving that keeps permissions, links and line endings, optional `.bak` backups, and a file watcher; documents changed by other programs reload when they have no unsaved changes.
- The view model both frontends draw: blocks, styled runs mapped to the source, markup hidden away from the cursor, widgets for checkboxes, formulas and images, cursor motion over hidden text, and folding.
- Logging to a rotated log file, panic reports, and a diagnostics report for bug reports.
- Background full parses run one at a time; edits made meanwhile are kept for mapping positions.
- Typing at the first character of a section reparses incrementally (it caused a full reparse).
- Backspace at the start of a line in a CRLF document removes the whole line break.
- `kalem tui FILE`: the terminal editor, with the shared commands, keymaps and settings, headline glyphs, hidden markup with cursor reveal, OSC 8 links, folding, mouse, and saving.
- Typing as Org does it: tables stay aligned and the first key after moving to a field replaces it; tags stay aligned on headlines. Tables are drawn as aligned grids in the terminal.
- `kalem-highlight`: syntax highlighting of source blocks (syntect, D16). In the terminal, blocks are framed, drawers and setting keywords folded, and the title and byline shown without their keywords.
- Enter continues and ends lists and keeps indentation; completion menus after `#+`, `[[` and `[fn:`; formula previews; a source view.
- Terminal editor: command palette, find and replace, and an outline panel.
- `kalem FILE`: the graphical editor on gpui, with menus, a toolbar, a status bar, light and dark themes, the shared commands and keymaps, IME input, folding, checkboxes and syntax highlighting.
- The graphical editor uses gpui from Zed's main branch and tells screen readers its text, caret and selection (AccessKit).
- Terminal images through kitty, iTerm2 and sixel; rendering snapshot tests; `tui-rich-text`, the terminal rich text widget as its own crate.
- Frontend commands (save, quit, clipboard, palette, find, outline, source view, folding) in the registry, and typing, deleting and selecting in the document state.
- Heading levels for the editor's heading styles, and `org-table-create` (in the middle of a line, Kalem avoids an Emacs bug).
- `org_model::complex_heading_todo` and `complex_heading_title`: heading lines split as `org-complex-heading-regexp` splits them.
- Decision records for the UI framework (gpui), the math engine (RaTeX) and the terminal stack (ratatui + crossterm), with their spikes under `spikes/`.
- Graphical editor: completion menus after `#+`, `[[` and `[fn:`, formula previews under the caret, and Enter and Tab by context.
- Paste: spreadsheet cells become an aligned Org table, HTML from the clipboard (macOS) becomes Org, and "Paste as Plain Text" inserts the text as it is. Source blocks and other verbatim text always take plain text.
- The source view highlights Org (headings, TODO keywords, emphasis, links, timestamps, dimmed keywords) and shows every character as it is, in both editors.
- Split view in the graphical editor: the source beside the rich view of the same document.
- Outline sidebar in the graphical editor: a folding tree of headings; click to jump, drag to move a subtree.
- Graphical editor: command palette (with prompts for command arguments) and a find and replace bar that marks matches. Both editors search with regular expressions (Alt+R), with `$1` in replacements.
- Status bars count the words a reader sees, in the document and in the current section.
- Insert Date (Alt+Shift+D; `C-c .` and `C-c !` in the Emacs example keymap): a calendar in the graphical editor, a prompt in the terminal, both reading `+3d`, `fri 10:00` and ISO dates. Tag completion at the end of headlines.
- Settings panel in the graphical editor (Ctrl+,): theme, keys, text size and font, saved to `settings.toml` with its comments and applied to every window. The graphical editor now follows the font settings and the new `editor.theme`.
- The graphical editor reloads files changed by other programs and warns when it has unsaved changes; File > Revert to Saved. A manual release checklist (`docs/release-checklist.md`).
- `gpui-rich-text`: the graphical editor's inline layout as its own crate, the gpui counterpart of `tui-rich-text`.
- The interface in English and Turkish (Fluent), in both editors: commands, menus, messages, panels and dialogs; `ui.language` follows the system by default.
- Themes in TOML (light and dark, changeable from the settings directory), used by both editors; the terminal editor follows the terminal's background.
- Focus mode (F8) shows only the section holding the cursor; narrowing now hides the rest in both editors; the text column is `editor.line_width` characters wide and centered; `editor.code_font_family`.
- Vim keys: the Vim profile (`editor.keymap_profile = "vim"`) gives both editors normal, insert, visual and replace modes, motions, operators with counts, text objects, registers with the system clipboard, `.` repeat, search and `:w :q :wq :q!`, with the Word-like keys in insert mode. It replaces the Emacs Org profile, whose keys are now an example user keymap, `docs/keymaps/emacs.json`.
- `kalem WORD` for a word that is no file and has no extension reports an unknown command instead of opening an editor.
- Set Document Mode: a file can be edited as Org, Markdown, CSV or text whatever its name, remembered in the workspace settings.
- Plain text files: monospace, colored by their language (highlighting updated incrementally), with line numbers, the cursor's line marked and indentation guides; Alt+Z turns wrapping off, and the view then scrolls sideways.
- Tab in plain text follows the file's indentation (tabs or its number of spaces) and indents or outdents selected lines; Shift+Tab outdents.
- `kalem fmt` aligns tables and tags and tidies blank lines as each document has them (`--check` for CI); `kalem query FILE... MATCH` prints the headlines matching an Org match string, as text or JSON.
- `org-table`: `#+TBLFM` formulas computed as Emacs computes them, without Emacs: Org's references, ranges, names, parameters, constants and remote tables, and the part of Calc that formulas use (integers of any size, decimal floats to the digit, vectors, dates, durations, flags). Every field agrees with Emacs on a corpus of 928 tables. Emacs Lisp formulas are kept and reported, not evaluated.
- Recalculate Table (F9; `C-c *` in the Emacs example keymap) and `kalem table recalc FILE...` (`--iterate`, `--check`).
- In a table, the status bar shows the formula of the field at the cursor, why it shows `#ERROR`, and Lisp formulas; the fields it refers to are highlighted. Edit Formula (F2; `C-c =`) changes it: `=` for the column, `:=` for the field.
- Sort Rows (`C-c ^` in a table), Import Table and Export Table (CSV and TSV), and Insert Table with a selection makes the selected lines a table, all as in Emacs.
- `org-math`: LaTeX formulas rendered with RaTeX and the KaTeX fonts behind a `MathEngine` trait, with a cache, the `\newcommand` and `\def` subset of `#+LATEX_HEADER`, and image snapshot tests.
- Formulas show typeset in the graphical editor: fragments in the line, LaTeX environments as one displayed formula while the cursor is outside; formulas RaTeX cannot lay out show their source in a red frame. Inside a formula a popup previews it; a click on a formula edits its source; Toggle Formula Preview shows the sources. The terminal editor approximates fragments in Unicode and, where the terminal draws images, shows displayed formulas and environments as images.
- Several documents in a window: files open in the same window (Open, Finder, links), listed on the left or as tabs at the top (`ui.open_files`), grouped under their project, with the files outside every project listed one by one. Next and previous document, Switch Document, Open Recent File, New and Close, in both frontends; quitting asks about every unsaved document.
- Projects, a simple Projectile: a project list (`projects.toml`), Switch Project, Find File in Project (recent files first), Search in Project (string or regular expression, case and whole-word switches, results as they come), recent files per project, the project in the status bar. The `kalem-project` crate walks and searches with ripgrep's crates and ignore rules, kept current by a file watcher.
- Doom Emacs keys in the Vim profile: `SPC p p`, `SPC p f`, `SPC SPC`, `SPC ,`, `SPC b …`, `SPC f …`, `SPC s p`, `SPC :` and more, with a which-key panel; the leader is `editor.vim.leader`; `:e FILE`, `:bn`, `:bp`, `:bd`, `:ls`, `gt`, `gT`. Every binding can be changed in `keymap.json`, which also understands `leader`.
- Word processor formatting, Kalem's own addition to Org: font, size, text color, highlight and paragraph alignment from the toolbar (font and size menus, A+ and A−, swatches, alignment buttons, Clear), the palette and Word's keys. Files stay Org that Emacs opens and exports: spans are `@@kalem:…@@` export snippets, alignment `#+ATTR_KALEM:`, centering Org's center block. Kalem hides them, keeps them whole when text is deleted, and shows colors and alignment in the terminal too.
- `org-export`: Org's export engine (`ox.el`) ported to Rust, with the HTML and Markdown back-ends: options, tags, macros, `#+INCLUDE`, numbering, footnotes, links (with Org's `doi:`, `info:` and other link types), tables, timestamps, smart quotes, translations by `#+LANGUAGE` and Babel's export changes (`:exports`, results, Noweb references; code is not run). The output is Emacs's, byte for byte, on every test case and on all 287 Worg files Emacs exports.
- Export as HTML and Export as Markdown in both editors, and `kalem export FILE... --to html|md` (`-o`, `--body-only`), writing where `#+EXPORT_FILE_NAME` says or beside the file.
- Two file kinds: `.org` stays strict Org, `.klm` is a Kalem document with Kalem's formatting. Formatting a `.org` file offers Make Kalem Document (links in the project follow) or `#+KALEM: markup=yes`; `org.allow_kalem_markup` allows a whole folder; the status bar names the kind.
- Save as Org writes a `.klm` document as strict Org beside it and lists what it dropped; `kalem export --to org` does the same; `kalem check` warns about Kalem markup in a `.org` file.
- Justified paragraphs in both editors; space before and after paragraphs (`#+ATTR_KALEM: :before :after`); typing in the font menu searches fonts; recently used colors in the color menus and prompts. `docs/terminal-parity.org` records what the terminal cannot show.
- HTML export keeps Kalem's formatting: fonts, sizes, colors and highlights of spans, paragraph alignment and the document's font, size and line spacing.
- A selection in a table shows the count, sum, average, minimum and maximum of its fields in the status bar of both editors.
- Vim: `h` and `l` move over the text the rich view shows, as the arrow keys do, skipping hidden text such as formatting snippets.
- Tables recalculate when a field is left (Tab, Shift+Tab, Enter) with `org.table_auto_recalc` or `#+KALEM: recalc=auto`, off by default.
- Vim: block selection (Ctrl+V) with `d x y c s u U ~ > <` and `I`/`A` typing on every line, painted in both editors.
- Vim: Org text objects: `ih`/`ah` headline, `iR`/`aR` subtree, `ii`/`ai` list item, `ic`/`ac` table cell, `ie`/`ae` emphasis.
- The current project's folder tree in the sidebar of both editors, below the open files: a click opens a folder or a file; Reveal in Folder Tree; `ui.folder_tree` turns it off.
- File manager: folders listed inside the listing (`i`, `K` to take one out), as Dired's inserted subdirectories; Mark Changed Since (`* t`) with ages (`2h`, `3d`, `1w`), `today`, `yesterday` or dates.
- Export… (Ctrl+Alt+E) in both editors: the formats and subtree exports, and the export settings (`export.body_only`, `export.open_after`, `export.math`) shown and changed in the same list.
- GitHub Flavored Markdown export (`kalem export --to gfm`, Export as GitHub Markdown): pipe tables with column alignment, fenced code blocks and `~~strike-through~~`; `--to md` stays Emacs's `ox-md`.
- HTML export writes whole pages as `org-html-template` does (doctypes including HTML5 and `html5-fancy`, meta tags, `#+HTML_HEAD`, `#+HTML_LINK_UP` and `#+HTML_LINK_HOME`, pre- and postamble, MathJax set-up), with Kalem's own style sheet in light and dark; `#+OPTIONS: tex:svg` draws formulas as inline SVG images with the editor's math engine.
- Export reads `#+SETUPFILE` files (options, TODO keywords, macros, link abbreviations) as Emacs does, and exports one subtree: Export Subtree as HTML and as Markdown at the cursor, `kalem export --subtree HEADLINE` (a title or `#CUSTOM_ID`), with the headline's `EXPORT_TITLE`, `EXPORT_OPTIONS`, `EXPORT_FILE_NAME` and other `EXPORT_` properties.
- Switching to the file manager is one step from everywhere: toolbar buttons, entries in the list of open files, a clickable hint in the terminal's status line, Ctrl+Alt+D (again to come back), `-`, `:Ex` and `:Projects` with Vim keys, and the palette also finds commands by their English names and IDs (`dired`).
- Being in a project's file makes that project the current one: Switch Project lists it first, and its file list starts building so Find File and Search in Project are ready. A folder under version control joins the project list when one of its files is opened, as in Projectile (`projects.auto_add`).
- The terminal editor indents text under a heading to the heading's title, as `org-indent-mode` does (`editor.outline_indent`, `#+STARTUP: indent`/`noindent`).
- A file manager like Emacs's Dired, in both editors (Ctrl+Alt+D, `SPC o -`): a folder is a read-only listing with permissions, sizes and times; Dired's keys to open, go up, mark, flag, copy, rename, move, make files and folders, link, change permissions and move to the trash (Delete for Good asks); copies and moves run in the background with progress, cancellation and a choice for each file already at the destination. A projects view lists every project as if in one folder: opening one lists its folder, going up from there shows the projects again. The `kalem-fs` crate does the file work (D20: the `trash` crate, own copy and move).
- Release preparation: the terminal-only build (`--no-default-features --features tui`), cargo-dist configuration, `Kalem.app` for macOS (files open from Finder), the user manual (`docs/manual.org`) and `docs/releasing.md`.
- TODO dependencies as in Emacs: `org.enforce_todo_dependencies` (open subtasks and the `ORDERED` property), `org.enforce_todo_checkbox_dependencies`, `NOBLOCKING`, and tags changed with the state (`org.todo_state_tags_triggers`); Toggle Ordered Subtasks and Delete Property.
- LaTeX export as Emacs's `ox-latex` writes it (`kalem export --to latex`, Export as LaTeX), with Kalem's formatting, and `%% org:LINE` comments with `--source-lines`.
- `tools/fetch-org.sh`: Org 9.7 for the differential tests on a machine whose Emacs has an older Org.
- Plain text export as Emacs's `ox-ascii` writes it, paragraphs filled as `fill-region` fills them (`kalem export --to txt`, `--to utf8`, Export as Plain Text).
- PDF export through LaTeX (`kalem export --to pdf`, Export as PDF): `latexmk`, the TeX engine or `tectonic`, compiled in the background in the editors, with LaTeX's errors at the lines of the Org file.
- Word, OpenDocument, EPUB and RTF through pandoc, and `kalem import` (Import as Org) from Word, OpenDocument, Markdown, HTML, EPUB and RTF, with a clean-up pass.
- Citations: the `org-cite` crate reads BibTeX, BibLaTeX and CSL-JSON bibliographies as Org does, and `kalem check` warns about unreadable bibliographies and unknown citation keys.
- Citation export: citations and `#+PRINT_BIBLIOGRAPHY:` in HTML, Markdown, LaTeX and plain text as Org's `basic` processor writes them (every style and variant, notes with punctuation moved per language, author-year and numeric bibliographies), identical to Emacs on the test cases.
- CSL citation styles (`#+CITE_EXPORT: csl apa`), rendered with hayagriva: the styles Kalem ships or a `.csl` file, locators from reference suffixes, note styles as footnotes, and bibliographies in HTML, LaTeX and text as Org's `csl` processor writes them.
- `#+CITE_EXPORT: biblatex` and `natbib`: citation commands, `\printbibliography` with its options, `\bibliography`, and the package and resources in the preamble, identical to Emacs; PDF export runs `biber` or `bibtex` when it compiles without `latexmk`.
- Insert Citation: a picker over the document's bibliography (searched by key, author and title) that writes `[cite:@key]` or extends the citation at the cursor; the entry cited under the cursor in the status bar of both editors, and under the mouse in a tooltip of the graphical editor.
- Pictures in the graphical editor: image links draw their picture (PNG, JPEG, GIF, WebP, BMP, TIFF, SVG) fitted to the text, at the width `#+ATTR_ORG: :width` asks for (pixels or a share); `attachment:` links find `org-attach` folders in both editors; pictures pasted or dropped are copied into `NAME_assets/` and linked.
- Footnotes: New Footnote (Ctrl+Alt+F), going between references and definitions, renumbering, sorting, normalizing and deleting, ported from `org-footnote.el` and identical to Emacs on 1,268 cases; `org.footnote_section`; the footnote's text shown for the reference at the cursor (and under the mouse in the graphical editor).
- Schedule (Ctrl+Alt+S), Set Deadline, Remove Schedule and Remove Deadline in both editors, with the date picker, times of day and repeaters, identical to Emacs's `org-schedule` and `org-deadline` on 434 cases.
- Edit Properties in both editors: the entry's properties as a key–value list to change, add to and remove from, the old value offered when a value is asked for.
- Export blocks show their back-end and are colored in its language, comment blocks are dimmed, and Insert Drawer (at the cursor or around the selection, as `org-insert-drawer`, checked on 163 cases) is in both editors.
- `#+TOC: headlines N` (and `local`) shows the table of contents in both editors, numbered as the export numbers it, each row leading to its heading.
- Set Caption, Set Name and Insert Cross Reference in both editors: `#+CAPTION:` and `#+NAME:` of the element at the cursor, and a picker over named elements, headings and targets; `[[` completion offers named elements.
- Macros show their expansion away from the cursor in both editors (`#+MACRO:` definitions and the built-in ones), and export snippets their back-end and contents; `org_export::macros::expansions` computes them from the tree in one pass.
- Refile (Ctrl+Alt+W) to a heading of the same document, Archive to Sibling and Toggle Archive Tag in both editors, ported from `org-refile.el` and `org-archive.el` and identical to Emacs on 434 cases.
- Copy as Rich Text (HTML and plain text on the macOS clipboard) and Copy as HTML (the markup as text) in both editors, from the HTML export of the selection.
- HTML paste: merged cells (`colspan`, `rowspan`), tables inside cells, zero-width spaces around emphasis inside words, and the HTML clipboard read on Linux through `wl-paste` or `xclip` (UTF-16 from Firefox too).
- `examples/book`: a sample book (a part, included chapters, a figure, a computed table, an equation, citations, footnotes, cross references) exported to HTML, LaTeX, text and Markdown exactly as Emacs exports it, and to PDF and Word without errors (checked when TeX and pandoc are installed).
- HTML: a link to a named math environment is `\eqref{…}` with MathJax (`#+HTML_EQUATION_REFERENCE_FORMAT`), as in Org.
- Word, OpenDocument, EPUB and RTF through pandoc: citations rendered with `--citeproc`, math environments kept as displayed formulas, and cross references labelled ("Figure 1", "Table 2", "(3)", a heading's title).
- Word targets for the document (`#+KALEM: word_target=`) and for sections (`WORD_TARGET`), shown with the counts in both status bars; Word Count by Chapter; Go to Line.
- Encodings: UTF-16 files with a byte order mark, legacy encodings guessed from the bytes (Windows-1254, ISO-8859-9, Shift_JIS…) and saved in the same encoding, Reopen with Encoding and Save with Encoding in both editors, the encoding in the status bar when it is not UTF-8.
- Fixed: a panic on lines whose first bytes end inside a non-ASCII character (`#+ATTR_KALEM:` and block checks).
- Multiple cursors and column selection in both editors: Add Cursor Above and Below (Ctrl+Alt+Up and Down), Alt-click, Add Next Occurrence (Ctrl+D), Select All Occurrences, Column Selection (Ctrl+Alt+Shift+Down and Up); typing, deleting, moving, copying and pasting at every cursor.
- Large files: files over 4 MB are syntax colored a window at a time, a line longer than 16 KiB shows the part around the cursor (in Org documents too), and a keystroke in a 100 MB file stays under a frame (2.7 ms median).
- LaTeX: `pdflang` names the language as Org 9.7 does ("English" for `en-us`).

### Changed

- The text column starts at the left edge of the window in both editors; `editor.center_text = true` centers it as before.
- The toolbar has no heading buttons any more, since it has font and size menus; headings stay on Ctrl+1 to Ctrl+6, the Format menu and the palette.

- The terminal editor has a quieter, colored look: the status line on the theme's bar color with Vim's mode as a colored label, the project in the accent color, errors in red, and the command palette's key when there is no message; the palette marks the chosen line with the selection color.
- In terminals the command palette is also Ctrl+G, since Ctrl+Shift+P arrives as Ctrl+P and macOS terminals turn Alt+P into a character.

- Faster parsing: the Org manual parses in 42 ms, about 40% faster, with the same trees as Emacs. `kalem check` and `kalem fmt --check` read a 1 MB file in under 100 ms. The graphical editor starts in about 150 ms and opens a 1 MB document in under 200 ms, because it parses the file while the window system starts. Every target of design section 15 is measured in `docs/performance.md`, with `tools/bench-phase1.py` and latency benchmarks for both editors.
- Word counts in the status bars are updated after a pause in typing rather than on every keystroke.

### Fixed

- The message for a file that cannot be opened had two definitions with different arguments; a test now keeps message names unique.
- Entities Org names twice (`\deg`, `\sup`) export as Org's first one (°, ⊃); tables of contents use a headline's `ALT_TITLE`; Markdown anchors headlines listed by `#+TOC: headlines`; `#+TOC: tables` and `#+TOC: listings` list captioned tables and code in HTML.
- Formatting next to emphasis no longer breaks it in Emacs: spans go inside `*bold*` and `/italic/` and take in the spaces around `=code=`, and formatting that would still change what Org reads as emphasis is refused.
- Save as Org keeps paragraphs apart when it takes out a `#+KALEM:` or `#+ATTR_KALEM:` line between them.
- `kalem fmt` aligns tables that have `#+NAME:` or other affiliated keywords.
- A crash when Vim keys that edit (such as `o`) were used in the file manager, which left the cursor past the end of the listing.
- A hang when the file watcher's thread reported a change while a file was being added to it.
- The file manager is listed by its own name in the list of open files, not under its project with the project's name again.

- Enter on the line below a table (or on a blank line after it) makes a new line again; only a table's rows count as being in the table, as `org-at-table-p` has it.

- `kalem fmt` no longer crashes on a table cell with a multibyte character before a number, such as `±0.5`.
- `kalem fmt` formats CRLF files like their line-feed text: tags are aligned, and no blank line is added before headlines on each run. Formatting is now idempotent on the whole test corpus.
- Typing in the graphical editor works while the line with the caret is scrolled out of view.
