# Kalem: A WYSIWYG Document Editor Built on Org Mode — Design Document

| Field | Value |
|---|---|
| Version | 0.2 (draft) |
| Date | 2026-09-27 |
| Status | Living document. Serves as RFC 0001 (see `rfcs/README.md`). |
| Related | `todo.md` (work breakdown derived from this document) |

## Contents

0. About this document
1. Vision and scope
2. Product definition
3. Org format support
4. Architecture
5. Parser: org-syntax
6. Document model and editing: org-model, org-edit
7. User interfaces: kalem-ui, kalem-tui, CLI
8. Table engine: org-table
9. LaTeX and mathematics
10. Export and import
11. Extension system
12. Babel: source blocks
13. Agenda and workspaces
14. Settings and configuration
15. Performance targets
16. Testing strategy
17. Packaging and distribution
18. Open source and community
19. Risks and mitigations
20. Roadmap
21. Open decisions
22. Glossary
23. References

---

## 0. About this document

This document defines what Kalem is, what it is not, how it will be built and in what order. It is updated as decisions change. Unresolved decisions are tracked in section 21 with numbered IDs (D1, D2, ...). When a decision is made it is moved into the relevant section and marked "decided" in section 21.

Audience: the project owner, future contributors, plugin authors.

The words **MUST**, **SHOULD** and **MAY** are used in the RFC 2119 sense.

The application is called **Kalem** ("pen" in Turkish). Naming conventions:

| Artifact | Name |
|---|---|
| Product | Kalem |
| Binary | `kalem` |
| Application package on crates.io | `kalem-editor` (`kalem` is taken) |
| GitHub organization | `kalem-editor` |
| Library crates (UI independent, reusable) | `org-*` |
| Application crates | `kalem-*` |
| Graphical frontend | `kalem-ui` (gpui) |
| Terminal frontend | `kalem-tui` (ratatui) |
| Candidate domain | `kalemeditor.org` |

---

## 1. Vision and scope

### 1.1 One sentence

A lightweight, fast, single-binary, open source desktop editor that lets people who do not know Emacs write and edit Org files through a Word-like interface. In short: **Typora for Org**. The same editor also runs in the terminal, every document operation is available from the command line, and files that are not Org open as plain text, so Kalem also works as a light, general purpose text editor in the spirit of Sublime Text.

### 1.2 Problem

- Org is one of the most mature plain-text formats for documents, outlines, tasks and tables, but in practice it is locked into Emacs.
- Emacs's learning curve keeps most people who would benefit from the format out.
- Existing alternatives are incomplete: Organice (web, limited), Orgzly and beorg (mobile, task-focused), Logseq (Org is second-class), VS Code extensions (source view, no WYSIWYG). There is no full WYSIWYG Org editor on the desktop.
- Office and Electron-based note apps are heavy, not plain text, or use closed formats.

### 1.3 Goals

| ID | Goal |
|---|---|
| G1 | **Lossless.** When a file created in Emacs is opened and saved, every untouched byte stays the same. |
| G2 | **Usable without Emacs.** A user can write documents, lists, tables, tasks and formulas without ever seeing Org syntax. |
| G3 | **Light and fast.** Single binary. Cold start under 300 ms, a 10 MB file under 1 s, keystroke latency under 16 ms. |
| G4 | **Org core built in.** The element table in section 3.2, following the phase plan. |
| G5 | **LaTeX.** Inline math preview and LaTeX/PDF export, good enough for writing books and papers. |
| G6 | **Extensible.** Plugins add real features (block types, link types, views, exporters, checks) through Org's own extension points, so documents stay valid Org. Command registry, events, JavaScript plugins; later Lua and WASM. |
| G7 | **Coexists with Emacs.** The same file can be edited alternately in both applications without diff noise. |
| G8 | **Reusable.** The parser and exporters are published as independent crates. |
| G9 | **Usable from the terminal.** A terminal frontend with the same editing semantics, plus a scriptable command line and batch mode. |
| G10 | **A general purpose text editor too.** Markdown files open in a WYSIWYG view like Org's, CSV files in an editable grid, and every other text file as plain text with syntax highlighting, so Kalem can be the only editor someone needs for notes, tables, configuration files and small code edits. |
| G11 | **Beyond Emacs's limits.** Every file Emacs opens, Kalem opens with the same meaning; the reverse is not required. Kalem does not inherit Emacs's implementation limits (size, speed, nesting depth, regexp length, blocking work). See 3.6. |

### 1.4 Non-goals

- **Microsoft Office compatibility.** docx is not a native format. Import and export go through pandoc.
- **Page layout editor.** Margins, columns, page breaks, page number placement. Org is a semantic format; presentation is decided at export time.
- **A full spreadsheet.** Pivot tables, a charting engine, hundreds of thousands of rows.
- **A visual slide designer.** Presentations are export targets plus a simple presentation mode.
- **Emulating Emacs.** Elisp, the Emacs key language, every agenda setting.
- **Real-time collaboration and cloud sync.** Git and file sync are considered sufficient.
- **Mobile platforms.**
- **A full IDE.** Language servers, debuggers and build integration are not built in. Plain text mode stays an editor; LSP support may come later as an out-of-process plugin (11.1).
- **100% of Org in the first release.** Scope is split into phases.

### 1.5 Target users

| ID | Persona | Need |
|---|---|---|
| P1 | Co-author | Must edit the same .org file with a co-author who uses Emacs; does not want to learn Emacs. |
| P2 | Plain-text knowledge worker | Leaving Obsidian or Notion; wants tasks, notes and documents in one place, with files on their own disk. |
| P3 | Academic, book author | LaTeX output, citations, formulas, long documents, chapter files. |
| P4 | Former Emacs user | Has years of .org files but has left Emacs. |
| P5 | Plugin developer | Knows JS/TS, has written Obsidian or VS Code extensions, expects a similar API. |
| P6 | Terminal user | Works over SSH or in tmux; wants a friendlier Org editor than Emacs in the terminal, and scriptable export and formatting. |
| P7 | Everyday editor user | Wants one light editor for everything: notes in Org, plus a quick edit of a config file, a CSV or a script, without opening a second application. Some of them want Vim keys. |

### 1.6 Success criteria

- 100% byte equality on the round-trip test corpus.
- The Org Manual's .org source can be opened, edited and saved without producing a diff.
- At least 99% structural agreement with Emacs org-element in differential tests; known differences documented.
- Performance targets in section 15 measured and met.
- At 1.0: packaged on three platforms, at least five community plugins, real use by at least one Emacs user's co-author.

---

## 2. Product definition

### 2.1 Core experience

- **WYSIWYG view by default.** The Typora model: markers (`*`, `/`, `=`, `[[ ]]`, `#+`) are hidden; they appear when the cursor enters the element and disappear when it leaves.
- **Source view** toggles with one key. Same document, same cursor position, same undo history.
- **Split view** is optional: source on the left, WYSIWYG on the right.
- **Outline sidebar:** headline tree, click to jump, drag and drop to move.
- **Toolbar:** bold, italic, underline, strike-through, code, heading level, list types, checkbox, table, link, image, footnote, TODO, tags, date, formula.
- **Command palette** (Ctrl/Cmd+Shift+P): access every command by name.
- **Foldable headlines:** Tab cycles visibility, same behavior as Emacs.
- **Status bar:** word count, cursor position, save state, document language, running background jobs.

### 2.2 Office equivalents

What users expect from Word, Excel and PowerPoint, the Org equivalent and its status in Kalem.

| User expectation | Org equivalent | In Kalem | Phase |
|---|---|---|---|
| Bold, italic, underline, strike-through, code | `*b*` `/i/` `_u_` `+s+` `~c~` `=v=` | Toolbar, shortcuts | 1 |
| Heading styles | `*` `**` `***` | Heading level picker, Ctrl+1..6 | 1 |
| Bulleted and numbered lists | `-` `+` `1.` `1)` | Toolbar, autoformat | 1 |
| Checkboxes | `- [ ]` `[X]` `[-]` | Clickable box, statistics `[2/5]` | 1 |
| Tables | `\| a \| b \|` | Grid editing, Tab navigation | 1 |
| Table formulas | `#+TBLFM:` | Formula bar, automatic recalculation | 2 |
| Images | `[[file:x.png]]` | Inline display, paste | 2 |
| Footnotes | `[fn:1]` | Insert, renumber | 2 |
| Table of contents | `#+TOC:` or export | Live preview | 2 |
| Links | `[[url][description]]` | Ctrl+K, click | 1 |
| Formulas | `$x^2$` `\begin{equation}` | Live preview | 2 |
| Comments | `# ...` or `:COMMENT:` | Dimmed display | 2 |
| Track changes | none | Not a goal; git recommended | – |
| Spell check | none (Emacs flyspell) | spellbook + Hunspell dictionaries | 3 |
| Word count | none | Status bar, per subtree | 1 |
| Printing | Export → PDF | Produce PDF and use system print | 2 |
| Templates, styles | `#+SETUPFILE`, export classes | Template picker | 3 |
| Spreadsheet | Table + `#+TBLFM` | Formula bar, references | 2 |
| Sorting | `org-table-sort-lines` | Sort from column header | 2 |
| CSV import and export | `org-table-import` / `export` | Menu | 2 |
| Charts | Babel + gnuplot | Run source block | 3 |
| Presentations | reveal.js, Beamer export | Export | 3 |
| Presentation mode | none | Simple in-app full-screen presentation | 4 |

### 2.3 File model

- **A document is a single .org file.** UTF-8. Line endings are taken from the file (LF or CRLF) and preserved. A BOM is preserved.
- **Attachments are side files.** Org does not embed binary data. Images and attachments are linked with relative paths. A pasted or dropped image is written to `<document-name>_assets/` and a relative link is inserted; the folder name is configurable. Compatibility with org-attach's `data/` layout is provided (`:ATTACH_DIR:` and `attachment:` links are resolved).
- **A workspace folder is optional.** It is needed for the agenda, multi-file search and `id:` link resolution.
- **Saving is atomic.** Write to a temporary file, then rename. Optional `.bak`.
- **External changes are detected** with a file watcher. If the document has no unsaved changes it reloads silently; otherwise the user is asked.
- **Encryption is not a goal.** org-crypt may come later as a plugin.

### 2.4 Platforms

macOS 12+, Linux (X11 and Wayland), Windows 10+. Single binary, no installation required. The terminal frontend also runs on any Unix-like system with a modern terminal, including headless servers. Packaging is covered in section 17.

### 2.5 Coexisting with Emacs

- In-buffer settings (`#+TODO`, `#+TAGS`, `#+STARTUP`, `#+PROPERTY`) are honored. Kalem never writes its own settings into a document unless the user explicitly asks.
- Newly generated syntax follows the document's existing style: indentation, blank-line rules, upper or lower case `#+` keywords, the TODO keyword sequence.
- Table alignment is identical to Emacs's `org-table-align`; otherwise every save would produce table diffs.
- No file locking.

### 2.6 Other files: a general purpose text editor

Kalem opens any text file. Four document modes decide how:

| Mode | Files | View |
|---|---|---|
| Org | `.org`, `.org_archive` | The WYSIWYG editor (the rest of this document) |
| Markdown | `.md`, `.markdown`, `.mdown`, `.mkd` | A WYSIWYG view like the Org one: hidden markers revealed at the cursor, rendered headings, lists, task lists, tables, images and math (2.6.1) |
| CSV | `.csv`, `.tsv`, `.tab` | An editable grid, like a light spreadsheet (2.6.2) |
| Plain text | Everything else | The plain text editor, which is the same editor as Org's source view |

Every mode can switch to its source text ("Open as text"), and the choice is remembered per file.

**Mode selection**, in order:

1. An explicit choice by the user (command "Set document mode"), remembered per file in the workspace settings.
2. A mode line on the first line, as in Emacs: `-*- mode: org -*-`.
3. The file extension: Org, Markdown and CSV extensions as in the table above. Other extensions select a language for syntax highlighting.
4. Content sniffing for files without an extension (shebang lines such as `#!/usr/bin/env python`).
5. Otherwise: plain text.

**Plain text mode features:**

| Feature | Phase |
|---|---|
| Syntax highlighting for common languages, from the same engine that highlights Org source blocks (D16) | 1 |
| Line numbers, current line highlight, soft wrap toggle, indentation guides | 1 |
| Find and replace with regular expressions, go to line | 1 |
| Preserved line endings, BOM and indentation style; tabs or spaces detected from the file | 1 |
| Encodings: UTF-8 by default, UTF-16 with BOM, and "reopen with encoding" for legacy encodings (encoding_rs) | 2 |
| Multiple cursors and column selection | 2 |
| Optional Vim mode (7.3.1) | 2 |
| Large files: 100 MB logs open quickly (rope, lazy highlighting, no whole-file layout); very long lines do not freeze the view | 2 |
| Workspace sidebar, fuzzy "open file" and "find in files" (per project: 2.8) | 2 |
| Bracket matching, auto-indent, comment toggling per language | 2 |
| Plugin-defined languages and modes (11.10) | 3 |

#### 2.6.1 Markdown mode

- **Dialect:** CommonMark with the GitHub extensions (tables, task lists, strikethrough, autolinks, footnotes), YAML or TOML front matter, and `$...$` / `$$...$$` math (rendered with the D4 engine).
- **Lossless editing, as for Org:** the file is edited as text, never regenerated from a tree. The parser only produces source ranges for the view, so untouched bytes stay as they were.
- **View:** the inline model of the Org editor (6.3): emphasis, code and link markers hidden away from the cursor, headings by level, clickable task list checkboxes, rendered images and math, code blocks with highlighting (D16), front matter folded.
- **Editing:** autoformat triggers (`#`, `-`, `1.`, `>`, `` ``` ``), Enter continues lists and quotes, tables edited in the grid shared with Org tables (8), outline sidebar from headings, "Convert to Org" and "Convert from Org" through the exporter (10) or pandoc.
- **Parser (D19):** a CommonMark parser that reports source offsets (pulldown-cmark's offset iterator is the recommended candidate), reparsed from the enclosing top-level block on each edit.

#### 2.6.2 CSV mode

- **Grid view:** a virtualized table with a header row (detected, can be toggled), column widths, frozen header, sorting and filtering in the view (the file is not reordered unless the user asks), cell editing, inserting, deleting and moving rows and columns, copy and paste of ranges (TSV on the clipboard, so spreadsheets interoperate).
- **Dialect detection and preservation:** delimiter (`,` `;` tab `|`), quoting (RFC 4180), line endings, encoding and BOM are detected and kept. Only edited records are rewritten; quoting is added only where a value needs it.
- **Large files:** 100,000+ rows open quickly; records are indexed lazily and only visible rows are parsed.
- **Beyond the grid:** "Open as text"; "Convert to Org table" (for formulas, Org tables and TBLFM are the place to compute, 8); column statistics (count, sum, average) in the status bar; the same grid in the terminal frontend.
- **Library:** the `csv` crate reads records with byte positions, so edits map back to exact ranges of the file.

**Binary files** are detected (NUL bytes, invalid UTF-8 in the first block) and are not opened for editing; the user is told what the file is.

**Architecture.** A document has a mode. `kalem-core` defines a `DocumentMode` interface: Org mode provides the view model, commands and structural editing; Markdown mode provides its own view model on the shared inline editing model; CSV mode provides a grid model; plain text mode provides the text view model and language-specific commands (comment toggling, indentation). Commands declare the modes they apply to through when-clauses (`editorMode == org`, `editorMode == csv`). Both frontends render every mode. The CLI accepts Markdown for conversion (`kalem export README.md --to org`) and CSV for conversion to an Org table; `kalem check` checks Org files only and refuses others with a clear message.

### 2.7 File manager (Dired)

Kalem has a directory editor modeled on Emacs's Dired: a directory opens as a document listing its entries, driven from the keyboard, and the same view works in both frontends. It is the file side of workspaces (13) and of the workspace sidebar (2.6).

**Listing.** One line per entry with type, permissions, size, modification time and name, like Dired's `ls -l` view, or a compact names-only view. Sorting by name, time, size or extension; directories first or mixed; hidden files toggled. Subdirectories can be inserted inline under their line (Dired's `i`) and collapsed again. The listing refreshes itself through the file watcher (`notify`) and keeps marks and the cursor across refreshes.

**Navigation.** Enter (or a click on a name) opens a file in its document mode (2.6) or descends into a directory; `^` goes to the parent; a filter narrows the listing; "jump to file" from any document opens its directory with the cursor on it (Dired's `dired-jump`). A window has one file manager document, which moves from folder to folder.

**Two views.** Besides the normal listing of a folder, the file manager has a projects view (asked by the owner, 2026-09-28): every project of the project list (2.8), and only the projects, listed as if they were all folders in one big folder, with their paths and missing folders marked. Opening a project lists its folder in the normal view; going up from a project's folder shows the projects again. `P` switches between the two views, going back to the folder shown before.

**Marks.** As in Dired: mark, unmark, unmark all, toggle; mark by regular expression, by extension, directories, or files changed since a date; flag for deletion (`d`) and execute (`x`). Commands act on the marked entries, or on the entry at point when nothing is marked.

**Operations.** Copy, rename and move (across directories and devices), delete, create directory, new file (a name ending with a slash makes a folder), symbolic link, change permissions, touch. Deletion moves to the system trash by default; permanent deletion needs an explicit command and a confirmation. Long operations run in the background with progress and can be cancelled; name conflicts ask per file (overwrite, skip, rename, apply to all). Operations are undoable through the normal undo stack where the file system allows it (renames, moves, trash).

**Editable listing (wdired).** The listing can be switched to text editing: names are changed with any editing command (multiple cursors, find and replace, Vim mode), and committing applies all renames at once, including cycles (a → b, b → a) and moves into other directories. Invalid results (duplicates, empty names) are reported before anything changes.

**Search.** Find by name (`find-name-dired`) and find in files (`find-grep-dired`) produce a listing of the results with the same marks and operations; find in files also has a results view with lines that jump to the match.

**Org integration.** Store a link to the entry at point and insert it as `[[file:…]]` (relative when the document is in the same tree); attach marked files to the current heading (org-attach, phase 3); drag files from the listing into a document in the graphical frontend. Image files show thumbnails in the graphical frontend (Dired's `image-dired`), and a preview pane shows Org, Markdown, text and images.

**Outside Kalem.** Open with the system application, reveal in the system file manager, copy the path, and run a shell command on the marked files (`!`, with the command shown and confirmed first).

**Extension.** Plugins add commands that act on the marked files (11.10), such as converters or batch exports. Remote directories (SFTP, as Emacs's TRAMP does) are left to a later phase and to plugins.

**Keys.** Dired's keys in both profiles, where they apply only in a listing (which is read-only, so single letters are free): Enter open, `^`, `-` and Backspace up, `n` and `p` down and up, `g` read again, `(` details, `.` dot files, `s` sort order, `F` filter, `m` mark, `u` unmark, `U` unmark all, `t` toggle marks, `d` flag, `x` delete flagged, `% m` mark by regular expression, `* /` mark folders, `* .` mark by extension, `C` copy, `R` rename or move, `D` move to the trash, `+` new folder, `c` new file, `S` symbolic link, `M` permissions, `T` touch, `w` copy names, `q` close, `P` projects view; F2 renames and Delete moves to the trash as in graphical file managers. With Vim keys these come before Vim's, except `n`, `p`, `w` and Backspace; `g r` reads again, `g g` goes to the first entry and `Y` copies names (evil-collection). Ctrl+Alt+D (Doom: `SPC o -`) opens the file manager at the document's folder, Ctrl+Alt+Shift+D (`SPC o P`) the projects view, `SPC p D` the project's folder.

**Architecture.** The listing is a document mode (`directory`) of `kalem-core`: a read-only text document, as Dired's buffer is, so the cursor, search, Vim motions and scrolling work unchanged. `kalem_core::dired` keeps what each line stands for, the marks and the options, and says how each part of a line is styled; frontends pick the colors. File system work lives in a separate crate (`kalem-fs`: listing, sorting, operations with trash, conflicts, progress and cancellation; D20), usable without the editor. Both frontends render it; the terminal one follows Dired's layout closely. The graphical frontend asks questions in dialogs, the terminal one in the prompt line.

### 2.8 Projects (a simple Projectile)

A project is a folder in the user's project list. Any folder can be added: no `.git` directory, configuration file or build file is needed, and nothing is written into it. The list lives in the user's settings (14), not in the projects. As in Projectile, a folder under version control (`.git`, `.hg`, `.svn`) or with a `.projectile` or `.kalem` marker joins the list by itself when one of its files is opened (asked by the owner, 2026-09-28; `projects.auto_add` turns it off); the home folder never does.

**Managing projects.** "Add project" adds the current file's folder or a chosen folder; "Remove project" removes one from the list (its files are untouched); the list shows each project's name (the folder name, editable) and path. Projects whose folder has disappeared are shown as missing and can be removed.

**Project mode.** When a file inside a listed project is the active document, that project is the current one: it counts as switched to (first in "Switch project", its last file this one), its file list starts building at once so "Find file" and "Search in project" are ready, the status bar shows it, and the project commands work on its folder. It turns off for files outside every project. With nested projects, the innermost one wins.

**Switching.** "Switch project" lists the projects, most recently used first, with fuzzy matching; choosing one opens the file last used in it, or its file picker (Projectile's `projectile-switch-project`).

**Finding files.** "Find file in project" is a fuzzy picker over every file of the project (`projectile-find-file`), recently opened files first. The walk skips `.git` and similar folders, honors `.gitignore` and `.ignore` files when present, skips binary files, and can be tuned per project in the settings (extra ignore patterns). The file list is built in the background and kept current by the file watcher.

**Searching text.** "Search in project" searches every file of the project for a string or a regular expression (`projectile-grep` / `projectile-ripgrep`), with case and whole-word toggles; results stream into a list grouped by file with the matching lines, and choosing a result opens the file at the match. The search runs in the background and can be cancelled; the same ignore rules apply.

**Also:** recent files per project, and "Open project in the file manager" (2.7). Both frontends provide every project command. Later additions stay optional: project-wide replace, per-project settings, running a command in the project's folder.

**Where things show.** The open documents of a window are listed on the left, or as tabs at the top (`ui.open_files`); a project with open files is a heading with its files under it, and files outside every project follow one by one (D12). The status bar shows the project. The project list, with the recent files overall and per project, is kept in `projects.toml` beside the user's `settings.toml`.

**Keys.** Word-like keys: Ctrl+Alt+P switch project, Ctrl+P find file, Ctrl+Shift+F search. In the Vim profile, Doom Emacs's `SPC p` keys (7.3). All of them can be changed in `keymap.json`.

**Implementation.** The walk and the search use the crates behind ripgrep (`ignore` for walking with ignore rules, `grep-searcher` and `grep-regex` for searching), in a `kalem-project` crate usable without the editor.

---

## 3. Org format support

### 3.1 References

| Source | Role |
|---|---|
| Org Syntax (Worg) | Treated as normative |
| `org-element.el` | Behavioral reference for disputed cases |
| Org Manual | Semantics and user-visible behavior |
| Target version | Org 9.7 behavior; version differences documented |

### 3.2 Element coverage

Columns: Parse (lossless parsing), Render (WYSIWYG display), Edit (structural editing support). The number is the phase in which support lands. The parser **MUST** recognize every element from the first release; anything it does not recognize is preserved as paragraph text. Rendering and editing follow the phase plan.

**Greater elements**

| Element | Parse | Render | Edit |
|---|---|---|---|
| Headline and section | 0 | 1 | 1 |
| Planning line (SCHEDULED, DEADLINE, CLOSED) | 0 | 1 | 2 |
| Property drawer | 0 | 1 | 2 |
| Generic drawer | 0 | 1 | 2 |
| Plain list (unordered, ordered, descriptive) | 0 | 1 | 1 |
| Item and checkbox | 0 | 1 | 1 |
| Table (org) | 0 | 1 | 1 |
| Table (table.el) | 0 | 2 (source) | – |
| Footnote definition | 0 | 2 | 2 |
| Greater block (center, quote, special) | 0 | 1 | 2 |
| Dynamic block | 0 | 2 | 3 |
| Inlinetask | 0 | 3 | 3 |

**Lesser elements**

| Element | Parse | Render | Edit |
|---|---|---|---|
| Paragraph | 0 | 1 | 1 |
| Src block | 0 | 1 | 1 |
| Example block | 0 | 1 | 1 |
| Export block | 0 | 2 | 2 |
| Verse block | 0 | 2 | 2 |
| Comment block | 0 | 2 | 2 |
| Fixed-width (`: `) | 0 | 1 | 2 |
| Horizontal rule | 0 | 1 | 1 |
| Keyword (`#+...`) | 0 | 1 | 2 |
| Affiliated keyword (CAPTION, NAME, ATTR_*, HEADER, RESULTS, ...) | 0 | 2 | 2 |
| Babel call (`#+CALL:`) | 0 | 3 | 3 |
| Clock | 0 | 2 | 3 |
| Diary sexp | 0 | 2 (source) | – |
| LaTeX environment | 0 | 2 | 2 |
| Node property | 0 | 1 | 2 |
| Comment (`# `) | 0 | 1 | 2 |
| Table row, table cell | 0 | 1 | 1 |

**Objects**

| Object | Parse | Render | Edit |
|---|---|---|---|
| Bold, italic, underline, strike-through | 0 | 1 | 1 |
| Code, verbatim | 0 | 1 | 1 |
| Link (file, http/https, id, custom-id, fuzzy, radio, coderef, `#+LINK` abbreviations, attachment) | 0 | 1 | 1 |
| Plain link, angle link | 0 | 1 | 1 |
| Timestamp (active, inactive, range, repeater, warning delay) | 0 | 1 | 2 |
| Footnote reference (named, inline, anonymous) | 0 | 2 | 2 |
| Inline src block, inline babel call | 0 | 2 | 3 |
| LaTeX fragment | 0 | 2 | 2 |
| Entity (`\alpha`) | 0 | 1 | 2 |
| Subscript, superscript | 0 | 1 | 2 |
| Line break (`\\`) | 0 | 1 | 1 |
| Macro (`{{{x}}}`) | 0 | 2 | 3 |
| Export snippet (`@@html:...@@`) | 0 | 2 | 2 |
| Target (`<<x>>`), radio target (`<<<x>>>`) | 0 | 2 | 2 |
| Statistics cookie (`[1/3]`, `[50%]`) | 0 | 1 | 1 |
| Citation (org-cite `[cite:@key]`) | 0 | 2 | 2 |

### 3.3 Round-trip guarantee

Formally: for every input `text`, `parse(text).to_string() == text` **MUST** hold. This is continuously verified by fuzzing and corpus tests.

Edits change text ranges; untouched ranges stay byte-identical. The document is reformatted only by an explicit user command ("Reformat document").

The single exception is tables. Org itself realigns a table when it is edited. Kalem likewise realigns only the table the user edited, using the same algorithm as Emacs.

### 3.4 In-buffer settings

The following keywords are read in a pre-pass and affect parsing, display or export:

| Keyword | Effect |
|---|---|
| `#+TODO`, `#+SEQ_TODO`, `#+TYP_TODO` | TODO keywords in headlines; multiple sequences; `(t@/!)` logging markers |
| `#+TAGS` | Tag completion, mutually exclusive groups `{ }` |
| `#+STARTUP` | overview/content/showall, indent, hidestars, logdone, folding behavior, etc. |
| `#+PROPERTY` | Document-wide properties and inheritance |
| `#+PRIORITIES` | Priority range |
| `#+FILETAGS` | File tags |
| `#+LINK` | Link abbreviations |
| `#+MACRO` | Macro definitions |
| `#+CONSTANTS` | Table formula constants |
| `#+SETUPFILE`, `#+INCLUDE` | Settings and content from external files |
| `#+OPTIONS`, `#+TITLE`, `#+AUTHOR`, `#+DATE`, `#+LANGUAGE`, `#+EXPORT_FILE_NAME`, `#+EXCLUDE_TAGS`, `#+SELECT_TAGS` | Export |
| `#+LATEX_CLASS`, `#+LATEX_HEADER`, `#+HTML_HEAD`, `#+CITE_EXPORT`, `#+BIBLIOGRAPHY` | Backend-specific export |
| `#+ARCHIVE`, `#+CATEGORY`, `#+COLUMNS` | Archiving, agenda, column view |
| `#+TBLFM` | Table formulas (table level) |

`#+SETUPFILE` loading is injected through a filesystem abstraction; tests use a fake loader.

### 3.5 Deliberate limitations

- `#+TBLFM` formulas containing Elisp (`'(...)`) are preserved but not evaluated; a warning is shown.
- Diary sexp timestamps are preserved; they are not evaluated in the agenda in the first releases.
- table.el tables are preserved, not edited.
- org-crypt, org-columns view and the org-habit graph are not in the first releases.
- Inlinetasks get basic rendering.

### 3.6 Beyond Emacs's limits

Compatibility with Emacs is **one-directional**. Every file that Emacs opens, Kalem opens, and gives it the same meaning, so that co-authors see the same document. The reverse is not required: Kalem may handle files, sizes and workloads that Emacs cannot.

Kalem is written in Rust, not Elisp, and uses that advantage. Emacs's **implementation limits** are not carried over. Only where a limit is part of the **meaning** of the syntax does Kalem keep Emacs's behavior by default, because changing it would make an existing Emacs file look different in Kalem.

The rule for every limit:

1. If exceeding the limit makes Emacs fail, slow down, block or truncate, Kalem removes the limit.
2. If the limit decides what a piece of text *means*, Kalem keeps it by default and may offer an opt-in extension that never changes the meaning of a file Emacs already understands.

| Emacs limit | Kind | Kalem |
|---|---|---|
| Deep nesting stops parsing with `max-lisp-eval-depth` | Failure | No depth limit; the parser grows its stack on demand. Twenty thousand nested list levels parse in seconds. |
| Radio targets are split into several regexps above 8 KB (`org-target-link-regexp-limit`) | Implementation | No limit; one matcher for any number of radio targets: a trie of case-folded targets, 0.2 s for 1 MB with 10,000 targets. |
| Large files make font-lock, folding and the element cache slow | Performance | Targets in section 15: 10 MB Org files interactive in under a second, 100 MB plain text files open. |
| Very long lines freeze redisplay (`so-long-mode`) | Performance | Long lines are laid out lazily; the UI never blocks on them. |
| The agenda, export, Babel and tangling block the editor | Blocking | Background workers with progress and cancellation (4.4). |
| `org-table-convert-region-max-lines` (999) | Arbitrary cap | No cap. |
| Table alignment and recalculation slow down on large tables | Performance | Incremental alignment and dependency-graph recalculation (8.2). |
| Undo history truncated by `undo-limit` | Arbitrary cap | Undo bounded only by a configurable memory budget; optional persistent history. |
| Clock sums work up to 29 inlinetask levels | Arbitrary cap | No cap. |
| Tab width must be 8 for list indentation (`org-current-text-column`) | Meaning | Kept: it decides which list an item belongs to. |
| Sub- and superscript braces nest at most 3 levels (`org-match-sexp-depth`) | Meaning | Kept by default; deeper nesting stays plain text, exactly as in Emacs. |
| Emphasis markers and their surrounding characters | Meaning | Kept (3.2). |
| Table formulas: Calc precision (12 significant digits by default, 8 displayed; integers of any size) | Meaning and precision | Calc's arithmetic reproduced to the digit, integers of any size (8.2). |

Pathological inputs that make Emacs quadratic (for example thousands of nested blocks of the same type, or thousands of unmatched emphasis markers in one paragraph) are tracked as performance work; they must never make Kalem hang or crash.

---

### 3.7 Kalem's own features beyond Org

The owner set the rule on 2026-09-28: everything that works in Emacs works in Kalem, and Kalem adds features that Org mode does not have. Every standard Org file opens in Kalem with its Emacs meaning; a file that uses Kalem's additions may not look the same in Emacs.

Kalem writes its additions in syntax that Emacs already parses, so that such a file still opens, edits and exports in Emacs, only without the addition. Checked with Emacs 30.1 and Org 9.7.11: the HTML and ASCII exports of a file with every addition below leave them out and keep the text.

**Word processor formatting (first addition).**

| Feature | Written as | In Emacs |
|---|---|---|
| Font family, size, text color, highlight color of a span | an export snippet for the `kalem` back-end that starts the span, `@@kalem:font="Georgia" size=14 color=#c00000 bg=#fff2a8@@`, and `@@kalem:end@@` that ends it | the snippets show as text; exports drop them |
| Right-aligned or justified paragraph | `#+ATTR_KALEM: :align right` (or `justify`) above the paragraph | an attribute line for a back-end Emacs does not have |
| Centered paragraph | Org's own `#+begin_center` block around it (Align Left takes a block holding only that paragraph away; one holding more gets `:align left` for the paragraph) | centered in exports |
| The document's font, size and line spacing | `#+KALEM: font="Georgia" size=12 spacing=1.5` | a keyword Emacs does not use |

Rules: spans stay inside one paragraph or heading title; nested spans combine, the inner one winning; a span without an end runs to the end of its paragraph; unknown keys are kept and ignored. Sizes are in points (tenths allowed), colors `#rrggbb` or the names of `kalem_core::rich::COLORS`. Kalem rewrites the spans of a paragraph it formats in their simplest form (no nesting), with the smallest edit.

In the rich view the snippets and `#+ATTR_KALEM:` lines never show; the source view shows them. Deleting text never deletes half of a span: the snippets stay, and a span whose text is all deleted goes with its snippets. Backspace and Delete next to a snippet delete the character beyond it. Formatting inside code, verbatim text, links, timestamps and other atomic objects covers the whole object; tables and blocks are not formatted.

The terminal shows colors and highlights and aligns short lines; it cannot show fonts or sizes. Justified text shows flush left in the editor for now; exports (10) will justify it.

## 4. Architecture

### 4.1 Principles

1. **Text is the source of truth.** The CST is a lossless view of the text; the model is derived from the CST. Data flows one way: text → CST → model → view. Edits are applied to the text, never to the model.
2. **The core is UI independent.** `org-*` and `kalem-core` contain no GUI or terminal dependencies. Two frontends, graphical and terminal, sit on the same core from the start; this keeps the boundary honest.
3. **Every user action is a command.** Menus, shortcuts, the palette and plugins all go through the same command registry.
4. **Deterministic and testable.** Time, filesystem and randomness are injected.
5. **Lightness is a feature.** Every new dependency is justified by its effect on binary size and startup time.
6. **Superset of Emacs, not a copy of its limits.** Same meaning for every file Emacs opens; none of Emacs's implementation limits (3.6).

### 4.2 Crate map

Cargo workspace layout:

```
crates/
  org-syntax/     Lossless CST, lexer and parser, incremental reparsing
  org-model/      Semantic layer: headline tree, TODO, tags, time, property queries
  org-edit/       Edit transactions, undo/redo, structural commands
  org-table/      Table alignment, TBLFM parsing and evaluation
  org-math/       LaTeX math → vector image (preview)
  org-export/     HTML, LaTeX, Markdown, reveal.js, Beamer; pandoc bridge
  org-cite/       org-cite parsing, CSL and BibTeX through hayagriva
  org-babel/      Source block execution (subprocess), result insertion, tangling
  org-agenda/     Workspace index, agenda queries
  kalem-cli/      Command-line subcommands and batch mode: parse, check, fmt, export, diff-emacs, run
  kalem-core/     Editor state, command registry, keymap, settings, event bus
  kalem-script/   QuickJS host, API bindings, d.ts generation, plugin loader
  kalem-highlight/ Syntax highlighting shared by Org source blocks and plain text mode (D16)
  kalem-ui/       Graphical frontend (gpui): editor view, outline, panels
  kalem-tui/      Terminal frontend (ratatui + crossterm)
  gpui-rich-text/ Inline layout of rich text lines on gpui (ecosystem component, 4.7)
  tui-rich-text/  Rich text lines on ratatui (ecosystem component, 4.7)
  kalem/          The single `kalem` binary (published as kalem-editor); dispatches to CLI, TUI or GUI
tests/
  corpus/         Real-world .org files (licensed)
  emacs/          Elisp scripts for differential testing
docs/             User manual, plugin API docs (written in Org)
```

Dependency rules:

- `org-*` crates never depend on `kalem-*` crates.
- `kalem-ui` and `kalem-tui` may read `org-*` crates directly but perform every change through `kalem-core` commands.
- `kalem-ui` and `kalem-tui` never depend on each other.
- `kalem-script` binds only to the `kalem-core` API; it never sees `org-*` types directly.

### 4.3 Layers and data flow

```
User input (keyboard, mouse, IME)
        │
        ▼
kalem-ui / kalem-tui ── command call ──▶ kalem-core: Command Registry ◀── kalem-script (QuickJS)
        ▲                                          │
        │ render                                   ▼
        │                              org-edit: Transaction { edits: [(Range, String)] }
        │                                          │
        │                                          ▼
        │                        Rope (text) ── incremental parse ──▶ org-syntax CST
        │                                                              │
        │                                                              ▼
        └──────────── view model ◀── org-model (derived, lazy) ◀────────┘
```

- Text lives in a `ropey` rope. The CST speaks in byte offsets.
- An edit: the rope is updated → affected sections are reparsed → model caches are invalidated → the UI redraws only the changed blocks.

### 4.4 Threading model

| Thread | Responsibility |
|---|---|
| UI | gpui event loop, rendering, command execution (short-lived) |
| Parser | Synchronous and incremental, runs on the UI thread. Budget per keystroke is 2 ms; if exceeded, work moves to the background and the old tree is rendered. Large files are parsed in the background on first open. |
| Script | QuickJS runs on the UI thread with a 100 ms synchronous budget enforced by an interrupt handler. Heavy work goes to worker plugins with their own QuickJS runtime, communicating by messages. |
| Worker pool | Export, math rendering, indexing, spell checking, image loading |
| Subprocesses | Babel, pandoc, tectonic/latexmk |

### 4.5 Key dependencies

| Area | Crate | Note |
|---|---|---|
| CST | rowan | rust-analyzer's lossless tree library |
| Text | ropey | Rope |
| Unicode | unicode-segmentation, unicode-width | Graphemes and widths |
| GUI | gpui | Zed's framework; D3 decided (7.1) |
| Highlighting | syntect or tree-sitter | D16 |
| Encodings | encoding_rs | Legacy encodings in plain text mode |
| Markdown | pulldown-cmark (D19) | Markdown mode (2.6.1): source ranges only, text edited directly |
| CSV | csv | CSV mode (2.6.2): records with byte positions |
| TUI | ratatui, crossterm, ratatui-image | Terminal frontend; images through kitty, iTerm2 or sixel protocols; D14 decided (7.6) |
| CLI | clap | Subcommands and batch mode |
| JS | rquickjs | Based on quickjs-ng |
| Regex | regex | |
| Serialization | serde, toml, serde_json | |
| Math | RaTeX (ratex-parser, ratex-layout, ratex-svg) | KaTeX-compatible LaTeX math in pure Rust; D4 decided (9.2) |
| Citations | hayagriva | BibTeX and CSL |
| Spelling | spellbook | Hunspell-compatible engine used by Helix |
| File watching | notify | |
| System trash (file manager) | trash | D20 |
| Project file walk and text search | ignore, grep-searcher, grep-regex | ripgrep's crates |
| Time | jiff | Timestamps and repeaters (D13) |
| Decimal | a bignum/decimal crate (chosen in phase 2) | Table arithmetic at least as precise as Calc (3.6) |
| Logging | tracing | |
| Testing | insta, proptest, cargo-fuzz, criterion | |
| Localization | fluent | |
| Distribution | cargo-dist | |

External, optional tools: pandoc, tectonic or latexmk, python and other Babel interpreters, gnuplot.

### 4.6 Alternatives considered and rejected

**Elixir/BEAM as the main engine, with Rust as NIFs.** The idea: BEAM is the application's "operating system"; heavy work such as parsing and layout is done in Rust through Rustler NIFs; the plugin language is naturally Elixir; one attaches to the running application with `iex` for live inspection and testing. **Rejected:**

- **It conflicts with G3.** The BEAM runtime has to be bundled (20 MB and more). Even as a single file with Burrito, it is unpacked to disk on first run. Cold start and memory baseline targets cannot be met.
- **Elixir has no native desktop GUI.** Scenic is not sufficient for rich text editing and `:wx` is dated. LiveView plus a webview moves the editor into JavaScript and takes Rust out of the UI loop. gpui wants to own the main thread and event loop, and BEAM cannot be embedded in another process. The result is two processes with IPC between them. On the keystroke-to-screen path, BEAM contributes nothing but latency and complexity.
- **We are not where BEAM shines.** BEAM is designed for long-lived servers with many clients. A single-user, single-document editor with one UI thread has no natural use for supervision trees and thousands of lightweight processes.
- **Elixir plugins cannot be sandboxed.** Every module can reach `File` and `System`; the permission model in section 11.6 cannot be built. The target plugin author audience (P5) does not know Elixir.
- **Two languages, two build systems.** mix and cargo; contributors need Erlang/OTP, Elixir, Rust and Zig.

What the idea gets right: Emacs's real strength is a live, inspectable, hot-reloadable runtime, and BEAM is the closest modern equivalent. Kalem meets this need through the QuickJS layer (see 11.9): an in-app JS console, hot reloading of `init.js` and plugins, and a debug REPL that attaches to the running application over a socket. Elixir would be the right tool if a multi-user server product (collaboration, web) is ever considered.

**Embedded Python as the plugin language.** Rejected because of size, the lack of a sandbox, the GIL and distribution burden (section 11). Python is supported through Babel source blocks and out-of-process plugins.

**Electron or Tauri as the primary UI.** Deferred, not rejected: if the D3 spike fails, Tauri + ProseMirror is the fallback.

### 4.7 Ecosystem components

Some building blocks Kalem needs do not exist in the Rust ecosystem, or exist only in immature form. Such components are published as **independent open source projects** and Kalem depends on them like any other crate.

**Policy:**

1. **Upstream first.** If a suitable project exists, contribute to it. A new project is started only when none exists, the existing one is unmaintained, or upstream declines the change.
2. **Incubate in the monorepo.** A new component starts as a workspace crate in the Kalem repository, so that API changes and Kalem changes land in one commit. Coordinating several repositories while the API is still moving is expensive.
3. **Spin out when stable.** When the API has settled and at least one outside user or clear outside use exists, the crate moves to its own repository under the `kalem-editor` organization, with its own README, CHANGELOG, semver, CI and docs.rs documentation. Kalem then depends on the published version.
4. **Generic names.** Components useful outside Kalem get neutral names (`org-syntax`, not `kalem-syntax`). Their public API never mentions Kalem types.
5. **Same license.** MIT OR Apache-2.0, so they can be adopted anywhere.

**Gap analysis (initial):**

| Component | Ecosystem status | Plan |
|---|---|---|
| Lossless, incremental Org parser | orgize 0.10 is alpha, not incremental (D2) | `org-syntax`, published from phase 0 |
| Org table formula (TBLFM) engine | No Rust implementation | `org-table`, published in phase 2 |
| Org timestamps, repeaters, agenda queries | No Rust implementation | Part of `org-model`; split out if there is demand |
| Org exporters (HTML, LaTeX, Markdown) | orgize has a basic HTML exporter | `org-export`, published in phase 2 |
| Rich text editing widget for gpui | gpui's editor lives inside Zed and is not reusable; `shape_text` cannot reserve space for inline boxes | `gpui-rich-text`, incubating in the workspace: the inline layout from the D3 spike (text pieces, widget boxes, scripts, spacers, row breaking with hanging indents, painting, carets, hit testing, selection rectangles) on `shape_line`; published once gpui is released with the APIs it uses |
| Rich text editing widget for ratatui | tui-textarea is plain text only | `tui-rich-text`, incubating in the workspace (`ratatui-rich-text` is reserved by the ratatui project) |
| Modal (Vim) editing engine, independent of the UI | Implementations live inside editors (Helix, Zed) | Candidate standalone crate from `kalem-core` (7.3.1) |
| LaTeX math layout to vectors | RaTeX (chosen in D4); typst for whole documents | Use RaTeX; contribute fixes upstream (D4) |
| Emacs differential test harness | None | Published as a tool that Org developers can use too |
| CSL citations, BibTeX | hayagriva | Use as is |
| Spell checking | spellbook | Use as is |

---

## 5. Parser: org-syntax

### 5.1 Requirements

| ID | Requirement |
|---|---|
| R1 | Lossless: every token is preserved, `to_string()` equals the input |
| R2 | Error tolerant: no input causes a panic; malformed structures are preserved as paragraphs |
| R3 | Incremental: a typical edit is reparsed in under 2 ms |
| R4 | Context aware: keywords such as `#+TODO` affect headline parsing |
| R5 | Fast: full parse at least 10 MB/s |
| R6 | Byte offsets; line and column conversion through the rope |

### 5.2 Approach

- **rowan** GreenNode and SyntaxNode. The `SyntaxKind` enum contains both token kinds (WHITESPACE, NEWLINE, STAR, TEXT, ...) and node kinds (DOCUMENT, SECTION, HEADLINE, PARAGRAPH, ...).
- **Two stages**, as in org-element: (1) element level, line based: headlines, blocks, lists, tables, drawers, keywords, paragraphs; (2) object level, inside paragraphs, headlines and cells: emphasis, links, timestamps, footnotes, formulas.
- **Pre-pass:** keywords at the top of the document and `#+SETUPFILE` content are read to produce `ParseContext { todo_keywords, tags, link_abbrevs, macros, startup, constants }`.
- **Object rules** follow Org's regular expressions exactly: pre and post character constraints and at most two newlines for emphasis; link types; timestamp formats.

### 5.3 Incremental reparsing

- The granularity is the section (headline plus its contents). Only sections touched by an edit are reparsed.
- Inserting or removing a starred line changes section boundaries; neighboring sections are reparsed too.
- When block boundaries (`#+BEGIN` / `#+END`) change, the enclosing section is reparsed fully.
- When pre-pass keywords change, the whole document is reparsed.
- A full reparse runs on a background thread and the UI keeps the old tree until it finishes; it costs about 330 ms for 4 MB on an M1 Max (D3 report).
- Documents with CRLF line endings or a byte order mark keep a second tree of their normalized text (the text Emacs sees). Edits are reparsed there, and only the replaced section or headline is converted back to the original line endings.
- rowan's green tree shares unchanged subtrees.

### 5.4 Error tolerance

- An unterminated block becomes a paragraph, as org-element does.
- An unknown `#+` keyword is preserved as a KEYWORD node.
- A malformed timestamp becomes plain text.
- Fuzzing verifies that no input panics.

### 5.5 API sketch

```rust
pub struct Parse { green: GreenNode, errors: Vec<SyntaxError> }

pub fn parse(text: &str, ctx: &ParseContext) -> Parse;
pub fn reparse(old: &Parse, edit: &TextEdit, ctx: &ParseContext) -> Parse;
pub fn context_from(text: &str, loader: &dyn SetupFileLoader) -> ParseContext;

/// Typed AST wrappers, like rust-analyzer's `ast` layer.
pub mod ast {
    pub struct Headline(SyntaxNode);
    impl Headline {
        pub fn level(&self) -> usize;
        pub fn todo_keyword(&self) -> Option<SyntaxToken>;
        pub fn priority(&self) -> Option<char>;
        pub fn title(&self) -> Option<Title>;
        pub fn tags(&self) -> impl Iterator<Item = SyntaxToken>;
        pub fn planning(&self) -> Option<Planning>;
        pub fn properties(&self) -> Option<PropertyDrawer>;
        pub fn section(&self) -> Option<Section>;
        pub fn children(&self) -> impl Iterator<Item = Headline>;
    }
}
```

### 5.6 Evaluating orgize (D2)

The `orgize` crate has been rowan-based since 0.10. A short evaluation **SHOULD** happen before writing a parser from scratch:

- Losslessness: round-trip test on the corpus.
- Coverage: missing elements against the table in 3.2.
- Incremental reparsing: present, or can it be added.
- Maintenance status, response time, license (MIT).
- API fit: typed AST layer, error reporting.

The outcome is one of three paths: direct dependency, fork, or a new parser. For a fork or a new parser, the design in 5.2 applies.

**Outcome (D2 decided, 2026-09-27):** a new parser. orgize round-trips every corpus file and never panicked, but agreed with org-element on only 72.5% of element and object positions, lacks citations and inlinetasks, and has been dormant since mid-2024. `org-syntax` follows `org-element.el` function by function and reproduces its tree shape and ranges. Details: `docs/decisions/D2-parser-foundation.md`.

### 5.7 Testing

- Round-trip fuzzing (`cargo-fuzz`): `parse(x).to_string() == x` and no panics.
- Snapshot tests (`insta`): tree output for every element.
- **Emacs differential testing:** the `tests/emacs/dump.el` script dumps the result of `org-element-parse-buffer` as JSON (type, begin, end, key properties). The Rust side produces the same JSON. Both are normalized and compared. CI runs a container with Emacs installed. Example invocation:

```bash
emacs --batch -l tests/emacs/dump.el tests/corpus/org-manual.org
```

- Corpus: the Org Manual source, Worg pages, permissively licensed community files, synthetic edge cases.
- Benchmarks (`criterion`): full parse of 1 MB and 10 MB files; incremental parse of a typical edit.

---

## 6. Document model and editing: org-model, org-edit

### 6.1 org-model

Lazily computed, cached views derived from the CST:

| View | Contents |
|---|---|
| Outline | Headline tree, levels, IDs, ranges |
| TodoState | A headline's TODO state, its position in the document's sequence, whether it is "done" |
| Tags | Direct and inherited tags, `#+FILETAGS` |
| Properties | `:PROPERTIES:` and `#+PROPERTY` inheritance, `_ALL` syntax |
| Timestamps | Parsed dates, repeaters (`+1w`, `++1m`, `.+1d`), warning delays |
| Links | Links with resolved types, target ranges |
| Footnotes | Definition and reference mapping |
| Names | `#+NAME`, `<<target>>`, `CUSTOM_ID`, `ID` map (cross references) |
| Statistics | Checkbox and TODO statistics cookies, computed |
| Clock | CLOCK lines, total durations |

Caches are invalidated by CST node identity. Queries: `headlines_with_tag`, `scheduled_between`, `find_by_id`, `find_by_name`.

### 6.2 org-edit

```rust
pub struct Transaction {
    pub edits: Vec<(TextRange, String)>,   // non-overlapping, sorted
    pub selection_after: Option<Selection>,
    pub label: String,                      // for the undo menu
}
```

- **Undo and redo:** a stack of transactions; inverse edits are stored; typing is grouped within 300 ms; cursor position is restored.
- **Structural operations** (each is a command that produces a transaction):
  - Headlines: promote, demote (with subtree), move up and down, cut, copy and paste subtree, archive (`org-archive` compatible: separate file or under an `:ARCHIVE:` headline), refile, sort.
  - TODO: cycle (with the document's sequence), set directly, raise and lower priority, insert `CLOSED` and log (`#+STARTUP: logdone`, `LOGBOOK` notes).
  - Tags: add, remove, completion, mutually exclusive groups.
  - Scheduling: set SCHEDULED and DEADLINE, date picker, repeaters.
  - Clock: clock in, clock out, CLOCK line under `:LOGBOOK:`.
  - Lists: indent and outdent, change type, toggle checkbox and update statistics, renumber.
  - Tables: section 8.
  - Insertion: link, footnote (with renumbering), source block, timestamp, image, macro, entity.
  - Emphasis: wrap selection, remove markers, respect nesting rules.
  - Narrow and widen: work on a single subtree.
- **Style inference:** the document's indentation style (org-indent or flat), blank line after headlines, TODO sequence, and `#+` keyword casing are read from the document. New content is produced in that style.
- **Positions:** everything is a byte offset. The UI converts to lines and columns through the rope. Cursor motion is by grapheme cluster.

### 6.3 WYSIWYG editing semantics

- **Hidden marker model:** an element's marker tokens are visible only while the cursor is inside the element. Hidden tokens are skipped by cursor motion; selection and deletion treat hidden tokens according to element integrity.
- **Emphasis while typing:** Ctrl+B wraps the selection if there is one, otherwise it enters "bold mode" and typed text goes between the markers.
- **Autoformat:** at line start `- ` makes a list, `* ` a headline, `| ` a table; `#+` opens a completion menu, `[[` a link menu, `$` a formula preview, `[fn:` a footnote menu.
- **Enter:** new item in a list, leaving the list on an empty item, next row in a table, new paragraph on a headline line, plain line in a source block.
- **Paste:** HTML from the clipboard is converted to Org; plain text is inserted as is; images are saved to a file and linked; TSV becomes a table.
- **Destructive edits:** a deletion that would break a marker token (for example deleting a star of `*bold*`) turns the element into plain text and shows that to the user; structure is never lost silently.

---

## 7. User interfaces: kalem-ui, kalem-tui, CLI

### 7.1 Framework decision (D3)

**Decided: gpui** (spike report: `docs/decisions/D3-ui-framework.md`). Rationale: pure Rust, proven for text editing by Zed, GPU accelerated, single binary. Risk: the API still moves, documentation is sparse, and its editor is not a reusable component; the editing engine is written from scratch.

The risk was measured with a **spike**. Spike goals:

1. Editable paragraphs with inline styles, backed by a rope.
2. IME composition on macOS, Windows and Linux.
3. 60 fps scrolling in a 100,000-line document (virtualization).
4. Custom inline widgets: SVG formula, checkbox, fold arrow.
5. The state of the accessibility API (AccessKit integration).
6. Clipboard, drag and drop, file dialogs.

Go/no-go: if 1, 2 and 3 do not work, switch to the **Tauri + ProseMirror** fallback. The core crates do not change; only `kalem-ui` is rewritten.

**Outcome.** Go. Scrolling and typing hold 120 Hz on a 117,850-line file with no frame over 17 ms and under 7 ms of main-thread work per frame, including page jumps through 9,000 formulas. Editing reparses incrementally in about 0.3 ms per keystroke. IME is implemented on gpui's platform input handler; manual verification with CJK input methods and dead keys is still due. Consequences for the design:

- `kalem-ui` owns its **inline layout**: gpui shapes text, Kalem breaks rows, places widget boxes (checkboxes, formulas, images), paints glyphs and does hit testing.
- Expensive inline content renders **off the main thread** with a placeholder of estimated size.
- Accessibility needs a gpui revision after 0.2.2: AccessKit landed on gpui's main branch in May 2026 but is not released on crates.io (7.4).
- A full reparse (for example after a context keyword edit) takes about 330 ms on 4 MB and must run in the background, keeping the old tree on screen (5.3).

### 7.2 View components

**Editor view:** a virtualized list of blocks. Every CST element is rendered as a block.

| Element | Display |
|---|---|
| Headline | Stars hidden, font size by level, TODO badge, priority, tags right-aligned, fold arrow, statistics cookie |
| Paragraph | Inline runs |
| Plain list | Indentation, bullet or number, checkbox |
| Table | Grid, column alignment, selected cell highlight |
| Src block | Syntax-highlighted code, language label, "run" and "copy" buttons |
| Example, fixed-width | Monospace block |
| Quote, center, verse | Styled block |
| LaTeX environment | Rendered formula; click to show source |
| Image link | Inline image, size from `#+ATTR_ORG: :width` |
| Drawer | Foldable; a key-value table for `:PROPERTIES:` |
| Keyword | `#+TITLE` as a large title; `#+AUTHOR`, `#+DATE` as a byline; others as a dimmed, folded "document settings" block |
| Footnote definition | Numbered list at the end of the document |
| Horizontal rule | Line |
| Comment | Dimmed, optionally hidden |

**Inline objects:** emphasis, links (clickable, Ctrl/Cmd+click), timestamps (badge, click for a calendar), footnote references (superscript, content on hover), formulas (rendered), entities (symbols), statistics cookies, citations (formatted), targets and radio targets (dimmed anchors).

**Panels:** outline sidebar, properties panel (right), agenda panel, find and replace bar, command palette, status bar, plugin console.

**Source view:** a plain text editor with Org syntax highlighting, sharing the same rope and the same undo stack.

### 7.3 Keyboard

- **Default profile, Word-like:** Ctrl+B/I/U, Ctrl+L/E/R/J alignment, Ctrl+] and Ctrl+[ font size, Ctrl+Space clear formatting (3.7), Ctrl+1..6 heading level, Ctrl+Shift+L list, Tab and Shift+Tab for indentation or folding (by context), Ctrl+Enter TODO cycle, Ctrl+K link, Ctrl+Shift+T table, Alt+arrows move subtree.
- **Vim profile (optional):** modal editing, off by default, with the Word-like keys in insert mode and for chords Vim does not use (Ctrl+S saves). It can be limited to some document modes, for example only plain text files (`editor.vim.modes = ["plain"]`). See 7.3.1.
- **Emacs keys:** not a built-in profile (the owner chose Vim over Emacs keys, 2026-09-28). `docs/keymaps/emacs.json` gives the Emacs Org keys (`C-c C-t`, `C-x C-s`, `M-RET`, …) as a user keymap to copy into `keymap.json`.
- **Doom Emacs leader keys (Vim profile):** in normal and visual mode the leader (Space unless `editor.vim.leader` says otherwise) starts Doom's sequences: `SPC p p` switches project, `SPC p f` and `SPC SPC` find a file in the project, `SPC ,` and `SPC b b` switch document, `SPC s p` and `SPC /` search the project, `SPC f f` opens a file, `SPC :` is the command palette. A which-key panel lists what may follow a half-typed sequence. They live in `keymaps/vim.json` like any binding, so `keymap.json` changes or removes them (and can use `leader` in its own keys).
- **Documents and projects (Word-like keys):** Ctrl+O open, Ctrl+N new, Ctrl+W close, Ctrl+Tab and Ctrl+PageDown next document, Ctrl+Alt+O switch document, Ctrl+Alt+R recent files, Ctrl+P find file in project, Ctrl+Shift+F search in project, Ctrl+Alt+P switch project, Ctrl+Shift+E the list of open files.
- **Terminals:** keys a terminal cannot send get terminal keys (`terminalKeys`): the command palette is Ctrl+G there (Ctrl+Shift+P arrives as Ctrl+P, and macOS terminals turn Alt+P into a character unless Option is set to send Meta).
- The keymap is stored in JSON, bound to the command registry, and supports when-clauses (`editorFocus && inTable`, `vimCommand`, `inProject`).

#### 7.3.1 Vim mode

Vim mode is an input layer in `kalem-core`, not a separate editor. It turns key sequences into ordinary commands from the registry, so undo, macros, plugins and both frontends work unchanged.

| Area | Scope | Phase |
|---|---|---|
| Modes | Normal, insert, visual (character, line), replace, command line | 1 |
| Motions | `hjkl`, words (`w b e W B E`), line (`0 ^ $`), `gg G`, `f t F T ; ,`, `%`, paragraphs `{ }`, search `/ ? n N * #` | 1 |
| Operators | `d c y > <`, `gu gU g~`, with counts and motions | 1 |
| Text objects | `iw aw`, quotes, brackets, `ip ap` | 1 |
| Registers | Unnamed, named `a-z`, append `A-Z`, system clipboard `+` | 1 |
| Repeat | `.` for the last change | 1 |
| Command line | `:w :q :q! :wq :x`, `:N`, `:noh` | 1 |
| Block selection, `=` | Visual block mode, reindenting | 2 |
| Org text objects | Headline (`ih ah`), subtree, list item, table cell and emphasis objects | 2 |
| Macros | `q` recording and `@` replay | 3 |
| Marks and jumps | `m`, `'`, `` ` ``, jump list | 3 |
| Documents | `:e FILE`, `:e!`, `:bn :bp :bd :ls :enew`, `gt gT`, Doom's leader keys (7.3) | 2 |
| Command line, more | `:s/.../.../g`, ranges, `:set` for common options | 3 |
| Configuration | Key remapping in `keymap.json`, and custom motions, operators and text objects from plugins (11.10) | 3 |

**In Org documents.** Vim mode also works in the WYSIWYG view. Motions move over the visible text; hidden markers are skipped the same way as with the arrow keys, and an edit that would break a marker follows the rules in 6.3. Org-aware text objects and a few evil-org style bindings (`<<`/`>>` promote and demote headlines, `t` cycles TODO in normal mode) are provided but can be disabled.

**What it is not.** Kalem does not embed Neovim and does not run Vimscript. The goal is the editing grammar most Vim users rely on every day, not full compatibility.

The modal engine is independent of any UI and of Org. If no suitable crate exists in the ecosystem, it becomes a standalone component under the policy in 4.7.

### 7.4 IME, language, accessibility

- An IME composition layer is **REQUIRED** for CJK and dead keys.
- UI language: English and Turkish with fluent; community translations. The strings live in `crates/kalem-core/locales/<language>/kalem.ftl`, built into the binary and shared by both frontends; the `ui.language` setting picks one (the system's language by default), and a missing string falls back to English. Org command messages stay those of Emacs; plugins bring their own strings.
- Accessibility: gpui 0.2.2, the latest crates.io release, has no accessibility support; AccessKit integration was merged into gpui's main branch in May 2026 (D3 report). Kalem uses a pinned revision of Zed's main branch (T1.5.2b); the editor is a multiline text input node whose text runs are the lines on screen, with the caret and the selection. Because of the git dependency, `kalem-editor` is distributed as binaries, not on crates.io, until gpui publishes a release with AccessKit. Target: basic screen reader support at 1.0 (block structure, heading levels, editing).
- Right-to-left languages: not a goal for the first releases; the parser and rope must not prevent RTL. gpui has no bidirectional text support, but Kalem's own inline layout can reorder shaped runs (Unicode bidi algorithm); this needs a spike before RTL is promised.

### 7.5 Visual design

- Theme: light and dark, following the system. Themes are TOML files (`crates/kalem-core/themes/light.toml` and `dark.toml`: colors by role, headline levels, syntax colors); a file of the same name in `themes/` of the user's settings directory changes any of them. The graphical editor follows the window's appearance; the terminal editor asks the terminal for its background (OSC 11) and uses the theme's colors only where the terminal has true color, keeping the terminal's own text and background colors. There is no user CSS.
- Fonts: serif or sans for body text, monospace for code, an embedded math font for formulas.
- Readable line width (configurable), focus mode (only the current subtree), typewriter mode (optional).

### 7.6 Terminal frontend: kalem-tui

`kalem tui file.org` (or `kalem -t file.org`) opens the same editor in the terminal. It uses the same `kalem-core`, the same commands, keymaps, settings and plugins as the graphical frontend. Only the rendering differs.

**Rendering in a character grid:**

| Element | Terminal rendering |
|---|---|
| Headline | Stars replaced by a level glyph and indentation (`◉ ○ ◈ ◇`, configurable, ASCII fallback); bold and a per-level color; TODO keyword as a colored badge; tags right-aligned |
| Emphasis | Terminal attributes: bold, italic, underline, strike-through; markers hidden with the same cursor-reveal rule as the GUI |
| Code, verbatim | Background color |
| Link | Underlined description; OSC 8 hyperlink so it is clickable in supporting terminals |
| Checkbox | `☐ ☑ ◐` with ASCII fallback `[ ] [X] [-]` |
| Table | Box-drawing characters, aligned columns; editing in the grid |
| Src block | Syntax highlighting (tree-sitter or syntect, shared with the GUI) inside a framed block |
| Entities, sub and superscripts | Unicode where possible (`\alpha` → α, `x^2` → x²) |
| LaTeX fragment | Unicode approximation inline; rendered image on terminals with a graphics protocol |
| Image | Inline through kitty, iTerm2 or sixel protocols when available; otherwise a `[image: name.png]` placeholder |
| Drawers, keywords | Folded, dimmed |

**Interaction:** mouse support (click, scroll, drag selection), the outline and agenda as side panels, the command palette as an overlay, the Word-like and Vim keymap profiles (with terminal-safe defaults, since some Ctrl combinations do not reach terminal applications). Plugin panels use the same JSON widget tree as the GUI (D11), rendered with ratatui widgets.

**Constraints:** no proportional fonts or font sizes, so heading levels are distinguished by glyphs and colors. Terminal capabilities are detected (true color, italics, graphics protocol, OSC 8) with graceful fallbacks, and `NO_COLOR` is respected.

**Stack (D14, decided):** ratatui + crossterm, with ratatui-image for graphics. The spike (`docs/decisions/D14-terminal-ui-stack.md`) draws from the same display model as the GUI spike, costs 0.5 to 1.9 ms per 120 × 50 frame on a 4 MB file, and builds to 6.7 MB with LTO. Consequences:

- Hyperlinks are OSC 8 sequences in every cell of a link, grouped by an `id` parameter, so a cell redrawn alone keeps its link.
- Terminal images are block-level: display formulas and image links become image blocks, inline formulas stay Unicode.
- Capability queries are answered in order and end with DA1. Kalem sends one query (XTVERSION, synchronized output, kitty keyboard, cell size, DA1) with a timeout and picks the image protocol from the answers; ratatui-image's own query is not used, because its reader thread outlives its timeout and takes keystrokes from terminals that do not answer.
- Over SSH, kitty transfers use compression.

### 7.7 Command line and batch mode

The same binary is a scriptable command-line tool. Every subcommand works without a display:

```bash
kalem export book.org --to pdf
```

| Subcommand | Purpose |
|---|---|
| `kalem check FILE...` | Syntax diagnostics and round-trip verification; non-zero exit on errors (for CI and pre-commit hooks) |
| `kalem fmt FILE...` | Realign tables, normalize blank lines per the document's style; `--check` mode for CI |
| `kalem export FILE --to html\|latex\|pdf\|md\|txt\|docx` | Export, same engine as the editor |
| `kalem table recalc FILE` | Recalculate all TBLFM formulas |
| `kalem import FILE --from docx\|md\|html\|...` | Convert other formats to Org (pandoc or plugin importers) |
| `kalem agenda [--day\|--week\|--todo] DIR` | Print the agenda |
| `kalem tangle FILE` | Tangle source blocks |
| `kalem query FILE 'TODO="NEXT"+work'` | Headlines matching an Org match expression, as text or JSON |
| `kalem parse FILE` | Dump the syntax tree (debugging) |
| `kalem diff-emacs FILE` | Compare the parse with Emacs org-element (development) |
| `kalem run SCRIPT.js [FILE...]` | Batch mode: run a JS script against documents with the full `kalem` and `editor` API, without a UI; the equivalent of `emacs --batch` |
| `kalem repl` | Connect to a running Kalem through the debug socket (11.9) |

Output formats are stable (`--format json` everywhere) so Kalem composes with shell pipelines. Plugins can add subcommands (11.10).

---

## 8. Table engine: org-table

### 8.1 Scope

- Org tables: rows, `|---|` horizontal rules, cell alignment (numbers right, text left), `<r>` `<l>` `<c>` alignment cookies, `<N>` column width and shrinking.
- Editing: in-cell text, Tab and Shift+Tab, Enter to the next row, insert, delete and move rows and columns, insert horizontal rules, sort (alphabetic, numeric, time), align.
- Alignment **MUST** be identical to Emacs's `org-table-align` (see 2.5 and 3.3).
- table.el tables are out of scope.

### 8.2 TBLFM

**Grammar:** `#+TBLFM: LHS=RHS;flags::LHS=RHS;flags...`

**Left-hand side:** `$3` (column formula), `@2$3` (field formula), `@>$2`, `$<`, `$>`, `$name` (named column, `!` row), `@I..@II$3` (hline references).

**Right-hand side:**
- Arithmetic: `+ - * / ^ %`, parentheses.
- References: `$1`, `@2`, `@2$3`, `@-1`, `@+1`, `@<`, `@>`, `$<`, `$>`, ranges `@2$1..@5$1`, `$1..$3`, `@I..@II`.
- Remote references: `remote(TABLE_NAME, @2$1)`.
- Constants: `$PI`, `$e`, `#+CONSTANTS`.
- Parameters: `$name` from `^` and `_` rows.
- Functions: `vsum vmean vmax vmin vprod vcount vlen vmedian vsdev vvar`, `abs sqrt exp ln log sin cos tan floor ceil round trunc mod`, `if`, `min max`, basic string operations, duration arithmetic (HH:MM, HH:MM:SS).
- **Flags:** `%.2f` (printf), `N` (numeric, empty is zero), `E` (keep empty), `L` (literal), `t` `T` `U` (durations), `f3` (digits), `p10` (precision), `s` (scientific), `e` (engineering).
- **Elisp formulas** `'(...)`: preserved, not evaluated, warning icon in the cell.

**Evaluation engine:**
- Emacs evaluates a formula by writing the referenced fields into its text (`(5)`, `[1,2,3]`) and handing the text to Calc. `org-table` takes the same steps: Org's substitution of references, names, constants and remote tables, then the part of Calc that formulas use, with Calc's algebraic notation parsed by a Pratt parser.
- Calc's arithmetic exactly: integers of any size, fractions, and decimal floats rounded to 12 significant digits after each operation (`calc-internal-prec`); results displayed with 8 significant digits (`org-calc-default-modes`: `(float 8)`), or as the flags ask (`p20`, `n3`, `f2`, `s3`, `e3`, printf formats). Unknown names stay symbolic and are simplified as Calc simplifies them (`a + 3`, `2 a`); dates and durations are computed as in Emacs.
- Evaluation order is Emacs's, not a dependency graph: column formulas row after row, then field formulas, each in the order of their left-hand sides, every formula seeing what the ones before it wrote. A graph would give other results where formulas depend on each other. "Recalculate until stable" iterates at most 10 times (`C-u C-u C-c *`) and reports a table that does not converge.
- Emacs Lisp formulas (`'(...)`) are kept and not evaluated; the fields they would set keep their values and a warning names them.
- Compatibility test: a corpus of tables computed in Emacs; identical results are **REQUIRED**.

### 8.3 Interface

- Formula bar: the formula for the selected cell or column; editing updates the `#+TBLFM` line.
- Error display: `#ERROR` cell with an explanation.
- Reference highlighting: cells referenced by the formula being edited are colored.
- CSV and TSV import and export; pasting TSV from the clipboard makes a table.

---

## 9. LaTeX and mathematics

### 9.1 LaTeX inside Org

Fragments: `$x$`, `$$...$$`, `\(...\)`, `\[...\]`, `\begin{env}...\end{env}` (equation, align, gather, matrix and variants), entities such as `\alpha`, `#+BEGIN_EXPORT latex`, `#+LATEX:` lines, `#+LATEX_HEADER`, `#+LATEX_HEADER_EXTRA`, `#+LATEX_CLASS`, `#+LATEX_CLASS_OPTIONS`, `#+ATTR_LATEX`.

### 9.2 Inline preview: org-math

- Input: a subset of LaTeX math at the amsmath and amssymb level. `\newcommand` definitions in `#+LATEX_HEADER` are applied to a limited extent.
- Output: vectors (glyph outlines and rules). The first implementation rasterizes the engine's SVG on a background thread and paints an image; painting the display list directly with gpui paths is a later optimization. Cache: hash of formula text and size → image.
- Errors: a formula that cannot be rendered is shown as source text with a red frame; it never disappears.

**Options (D4):**

| Option | Pros | Cons |
|---|---|---|
| A. mitex (LaTeX → Typst math) + the typst library for layout + typst-svg | Production-quality layout, actively maintained, pure Rust | mitex's LaTeX coverage; Typst font and world setup; a few MB of fonts |
| B. RaTeX (KaTeX-compatible layout in pure Rust, display list output) | Reads LaTeX directly, small, fast start-up | Young project with one main author |
| (B, earlier) ReX and its forks | Small, TeX algorithms | No maintained release; the `rex` crate on crates.io is unrelated; replaced by RaTeX |
| C. KaTeX inside QuickJS → HTML/MathML | Very broad coverage | No native rendering; eliminated |

**Outcome (D4): RaTeX.** On a 100-formula corpus it rendered 98 formulas with no visible errors; typst + MiTeX rendered 88, three of them wrongly, and several failures came from MiTeX lagging typst's symbol renames. RaTeX adds 4.6 MB to the binary against about 35 MB for typst, and its start-up is 1 ms against 11 to 17 ms. Details: `docs/decisions/D4-math-engine.md`. Typst stays a candidate for whole-document export (9.3), which is a separate decision.

### 9.3 Full LaTeX and PDF

- The `org-export` LaTeX backend (ox-latex behavior) produces `.tex`.
- Compilation: `latexmk` or `xelatex` from the system if available. Otherwise **tectonic** can be downloaded on demand (user consent, network access). Bundling is D5; the recommendation is an optional separate download, because tectonic increases binary size considerably and fetches packages from the network.
- PDF preview: an external viewer at first; later an embedded panel (pdfium).
- Error mapping: line numbers in the LaTeX log are mapped back to Org positions through `%% org:LINE` comments inserted into the generated `.tex`.

### 9.4 Writing books and papers

- **Structure:** `#+LATEX_CLASS: book`; headline levels map to part, chapter, section; chapter files through `#+INCLUDE:`; `#+OPTIONS: toc:t num:t`.
- **Figures and tables:** `#+CAPTION`, `#+NAME`, `#+ATTR_LATEX: :width :placement`; cross references `[[fig:x]]` → `\ref`.
- **Citations:** org-cite syntax `[cite:@key]`, `[cite/t:@a;@b]`, `[cite:see @a p. 3]`; `#+bibliography: refs.bib`; `#+cite_export: csl apa.csl` or `biblatex` / `natbib`. The `org-cite` crate parses; `hayagriva` reads BibTeX and produces HTML and plain text through CSL; LaTeX output can delegate to biblatex or natbib.
- **Index:** `#+INDEX:` → `\index`.
- **Glossaries and acronyms:** via plugins.
- Footnotes, epigraphs, verse blocks.
- **Author tools:** word count per chapter, targets, draft sections with `:noexport:`.
- EPUB and DOCX: through pandoc.

---

## 10. Export and import

### 10.1 Built-in backends

| Backend | Reference | Phase |
|---|---|---|
| HTML | ox-html classes, single file, CSS theme, MathJax or embedded SVG | 2 |
| LaTeX | ox-latex | 2 |
| Markdown | ox-md, GFM tables | 2 |
| Plain text | ox-ascii | 2 |
| reveal.js | A subset of org-re-reveal options | 3 |
| Beamer | ox-beamer | 3 |

**Common behavior:** `#+OPTIONS` (toc, num, ^, _, *, ', -, todo, tags, pri, d, f, e, H, ...), `:noexport:` and `:export:` tags, `#+EXCLUDE_TAGS`, `#+SELECT_TAGS`, export snippets, `#+BEGIN_EXPORT`, macros (`{{{title}}}`, `{{{date(FORMAT)}}}`, `{{{n}}}`, `{{{property(X)}}}`), `#+INCLUDE`, `#+SETUPFILE`, subtree export, the `EXPORT_FILE_NAME` property.

**Architecture:** `org-model` → `ExportTree` → transcoder. The ox.el pattern: a backend function per element type, with pre and post filters. Plugins can register new backends and filters in JavaScript.

### 10.2 Pandoc bridge

- Output: DOCX, ODT, EPUB, RTF. Two paths: (a) hand the `.org` file directly to pandoc's Org reader; (b) hand Kalem's HTML output to pandoc. The default is (b), because Kalem's interpretation of Org is more complete; the user can choose (a).
- Input: DOCX, ODT, Markdown, HTML → Org (pandoc), followed by Kalem's "Org cleanup" pass (blank lines, headline format, table alignment).
- If pandoc is missing: detection, a download link, short instructions.

### 10.3 Clipboard

- "Copy as rich text": the selection goes to the clipboard as HTML, for pasting into Word and email.
- HTML paste → Org: an internal simple converter, no pandoc needed.

---

## 11. Extension system

### 11.0 Principle: new features, same format

Plugins **MUST** be able to add real features, not only shortcuts: new block types, new link types, new views, new export targets, new document checks. At the same time every document **MUST** stay valid Org that Emacs opens without Kalem.

The two goals meet in one rule: **plugins extend semantics, not syntax.** The parser grammar is fixed. Org already has extension points designed for exactly this, and Emacs packages use them the same way:

| Org extension point | Example | What a plugin gives it |
|---|---|---|
| Special blocks | `#+BEGIN_kanban` ... `#+END_kanban` | A renderer, an editor widget, export output |
| Source block languages | `#+BEGIN_SRC mermaid` | A renderer (diagram), a Babel executor |
| Link types | `[[jira:ABC-123]]`, `[[zotero:key]]` | Resolution, click action, hover, completion, rendering, export per backend (like `org-link-set-parameters`) |
| Drawers and properties | `:PROPERTIES: :ESTIMATE: 3h` | Views, computations, badges |
| Keywords | `#+KANBAN_COLUMNS: TODO DOING DONE` | Document-level configuration for a plugin |
| Tags and TODO keywords | `:urgent:`, `WAITING` | Views, highlighting, automation |
| Export snippets, macros | `@@myformat:...@@`, `{{{badge(new)}}}` | Backend-specific output, expansion |
| Dynamic blocks | `#+BEGIN: word-report` | Generated content, refreshed on demand |

A document that uses a plugin still opens in Emacs and in Kalem without the plugin; the plugin's content is shown as ordinary Org (for example as a plain special block).

Kalem's own optional features are built on the same extension points wherever possible and shipped as **bundled plugins** (for example the kanban view and the word count panel). This keeps the API honest: if a built-in feature needs something, plugins get it too.

### 11.1 Layers

| Layer | Technology | Purpose | Phase |
|---|---|---|---|
| Command registry and events | Rust, `kalem-core` | Foundation for everything | 1 |
| User script | `init.js`, QuickJS | Shortcuts, small commands, automation | 3 |
| Plugin package | JS/TS + manifest, QuickJS | Distributable features, including new block types, link types, views, exporters (11.10) | 3 |
| Bundled plugins | Same as plugin packages, shipped with Kalem | Optional built-in features built on the public API | 3 |
| Second scripting language | Lua (mlua), same API | For those who prefer it (D10) | 4 |
| Heavy and polyglot plugins | WASM (extism) | Compute-intensive work | 4 |
| Out-of-process | JSON-RPC over stdio | Python and other integrations | 4 |

Scripting languages sit behind a `ScriptHost` trait; the API definition is generated from a single source (D6). Adding Lua is therefore only a binding layer.

### 11.2 Command registry

```rust
pub struct Command {
    pub id: String,                  // "org.todo.cycle", "table.insertRow", "<plugin>.<name>"
    pub title: String,               // for the palette and menus, localized
    pub category: String,
    pub default_keys: Vec<KeyChord>,
    pub when: Option<WhenClause>,    // "editorFocus && inTable"
    pub handler: CommandHandler,     // Rust fn or script callback
    pub args_schema: Option<JsonSchema>,
}

pub enum CommandHandler {
    Native(fn(&mut EditorContext, serde_json::Value) -> CommandResult),
    Script(ScriptCallbackId),
}
```

- Commands run inside a transaction; the command is the unit of undo.
- Plugin commands appear in the same palette, menus and keymap as built-in commands.
- ID convention: `area.action`; plugins use `pluginId.action`.

### 11.3 Events

| Event | When | Veto |
|---|---|---|
| `app:ready` | Startup complete | – |
| `document:open`, `document:close` | | – |
| `document:before-save` | Before saving | yes |
| `document:after-save` | | – |
| `document:changed` | With range information, debounced | – |
| `selection:changed` | | – |
| `headline:todo-changed`, `headline:tags-changed`, `headline:scheduled` | | – |
| `table:before-recalc`, `table:recalculated` | | – |
| `babel:before-execute`, `babel:after-execute` | | yes |
| `export:before`, `export:after` | As filters; may change the output | yes |
| `workspace:file-changed` | File watcher | – |

Vetoable events have a timeout (500 ms); if it is exceeded the event proceeds and a warning is logged.

### 11.4 JavaScript API surface

A TypeScript definition (`kalem.d.ts`) is generated and distributed with the plugin template. Sketch:

```ts
declare namespace kalem {
  const version: string;
  function command(id: string, spec: { title: string; run: (...args: unknown[]) => unknown | Promise<unknown>; when?: string; keys?: string[] }): Disposable;
  function run(id: string, ...args: unknown[]): Promise<unknown>;
  function keymap(keys: string, commandId: string, opts?: { when?: string }): Disposable;
  function on<E extends keyof Events>(event: E, handler: (e: Events[E]) => void | Promise<void>): Disposable;

  namespace ui {
    function notify(message: string, level?: "info" | "warn" | "error"): void;
    function prompt(title: string, opts?: { default?: string; placeholder?: string }): Promise<string | null>;
    function confirm(message: string): Promise<boolean>;
    function quickPick<T>(items: { label: string; detail?: string; value: T }[], opts?: { placeholder?: string }): Promise<T | null>;
    namespace statusBar { function set(id: string, text: string, opts?: { tooltip?: string; command?: string }): Disposable; }
    namespace panel { function register(id: string, spec: PanelSpec): Disposable; } // JSON widget tree, rendered by both frontends (D11)
  }
  namespace settings { function get<T>(key: string): T; function set(key: string, value: unknown): void; function onChange(key: string, cb: () => void): Disposable; }
  namespace fs  { function read(path: string): Promise<string>; function write(path: string, text: string): Promise<void>; function list(dir: string): Promise<string[]>; } // permission required
  namespace net { function fetch(url: string, init?: RequestInit): Promise<Response>; } // permission required
  namespace babel { function registerLanguage(name: string, runner: BabelRunner): Disposable; }
  namespace exporter { function registerBackend(name: string, backend: ExportBackend): Disposable; function addFilter(stage: string, fn: ExportFilter): Disposable; }
  namespace tables { function registerFunction(name: string, fn: (...args: number[]) => number): Disposable; }

  // Extension points for new features (11.10)
  namespace links { function register(type: string, spec: LinkTypeSpec): Disposable; }            // resolve, open, hover, complete, render, export
  namespace blocks { function register(name: string, spec: BlockSpec): Disposable; }             // special blocks and src languages: render, edit, export
  namespace decorations { function create(spec: DecorationSpec): DecorationSet; }                // highlights, badges, gutter marks, virtual text
  namespace completion { function register(trigger: CompletionTrigger, provider: CompletionProvider): Disposable; }
  namespace hover { function register(provider: HoverProvider): Disposable; }
  namespace inputRules { function register(rule: InputRule): Disposable; }                       // e.g. "->" becomes "→"
  namespace views { function register(id: string, spec: ViewSpec): Disposable; }                 // alternative document views: kanban, timeline, mind map
  namespace diagnostics { function register(id: string, checker: DocumentChecker): Disposable; } // also run by `kalem check`
  namespace importer { function register(extensions: string[], convert: (bytes: Uint8Array) => Promise<string>): Disposable; }
  namespace paste { function register(mime: string, handler: PasteHandler): Disposable; }
  namespace dynamicBlocks { function register(name: string, generate: DynamicBlockGenerator): Disposable; }
  namespace agenda { function registerView(id: string, spec: AgendaViewSpec): Disposable; }
  namespace capture { function registerTemplate(id: string, spec: CaptureTemplate): Disposable; }
  namespace cli { function register(subcommand: string, spec: CliCommandSpec): Disposable; }     // `kalem <subcommand>` in batch mode
  namespace themes { function register(id: string, theme: ThemeSpec): Disposable; }
}

declare namespace editor {
  const document: Document;
  const selection: Selection;
  function insert(text: string, at?: number): void;
  function replace(range: Range, text: string): void;
  function transact(label: string, fn: () => void): void;
}

interface Document {
  readonly path: string | null;
  text(range?: Range): string;
  headlines(): Headline[];
  headlineAt(offset: number): Headline | null;
  headlineById(id: string): Headline | null;   // ID or CUSTOM_ID property
  todoKeywords(): string[];
  nodeAt(offset: number): Node;
  find(query: { tag?: string; todo?: string; property?: [string, string] }): Headline[];
  keywords(): Record<string, string[]>;
  save(): Promise<void>;
}

interface Headline {
  readonly id: string;  // ID property, created on demand
  readonly level: number; title: string; todo: string | null; priority: string | null;
  tags: string[]; readonly properties: Record<string, string>;
  scheduled: Timestamp | null; deadline: Timestamp | null;
  readonly range: Range; readonly parent: Headline | null;
  children(): Headline[]; body(): string;
  setTodo(state: string | null): void; setTitle(title: string): void; setTags(tags: string[]): void;
  setProperty(key: string, value: string | null): void;
  promote(): void; demote(): void; moveUp(): void; moveDown(): void;
}

interface Table {
  readonly rows: number; readonly cols: number;
  cell(row: number, col: number): string; setCell(row: number, col: number, value: string): void;
  formulas(): string[]; recalc(): void;
}
```

### 11.5 Plugin package

Manifest `plugin.json`:

```json
{
  "id": "com.example.wordcount",
  "name": "Word Count",
  "version": "0.1.0",
  "description": "Word count per subtree",
  "main": "dist/main.js",
  "api": "^1.0",
  "activation": ["onStartup"],
  "permissions": ["fs:read:workspace"],
  "contributes": {
    "commands": [{ "id": "com.example.wordcount.show", "title": "Show word count" }],
    "keybindings": [{ "command": "com.example.wordcount.show", "keys": "ctrl+shift+w" }],
    "settings": [{ "key": "com.example.wordcount.includeDrawers", "type": "boolean", "default": false }]
  }
}
```

- Location: `plugins/<id>/` under the platform's standard configuration directory.
- Activation events: `onStartup`, `onCommand:<id>`, `onLanguage:<babel-language>`, `onDocument`.
- Lifecycle: `export function activate(ctx)` and `deactivate()`. Disposables are collected in `ctx.subscriptions`.
- Module system: ES modules. `import` only resolves inside the plugin folder. Bundling into a single file with esbuild is recommended; a template repository is provided.
- Plugins run unchanged in the graphical frontend, the terminal frontend and batch mode (`kalem run`). UI calls degrade gracefully in batch mode (prompts return defaults, notifications go to stderr).
- QuickJS is a JS engine, not a browser: no DOM, no Node APIs, no native npm modules. This is stated on the first page of the plugin documentation.

### 11.6 Security and resource limits

- **Sandbox:** QuickJS's `std` and `os` modules are not loaded. Only the `kalem` and `editor` objects are visible.
- **Permissions** are declared in the manifest, shown to the user on first run and approved. Scopes: `fs:read:workspace`, `fs:write:workspace`, `fs:read:all`, `net:fetch:<domain>`, `subprocess` (separate, explicit warning).
- **Time limit:** interrupt handler; a synchronous call exceeding 100 ms is cancelled with a warning. Long work uses async APIs and worker plugins.
- **Memory limit:** per runtime, 64 MB by default.
- A plugin error never crashes the application; it is shown in the plugin console; a plugin that fails repeatedly is disabled.
- Code inside documents (Babel) has a separate trust model from plugins (section 12).

### 11.7 User configuration

| File | Contents |
|---|---|
| `settings.toml` | Static settings |
| `init.js` | Personal script run at startup; commands, shortcuts |
| `keymap.json` | Keymap overrides |
| `plugins.toml` | Enabled plugins and permission decisions |
| `themes/*.toml` | User themes |

### 11.8 Distribution

- First release: install from a git URL or a folder; `kalem plugin install <url>`.
- Later: a community index (JSON), an in-app plugin browser, version compatibility checks (the `api` field).

### 11.9 Live runtime

Emacs's "reach into the running program and change it" experience is provided in Kalem by:

- **JS console panel:** a REPL inside the application with access to the `kalem` and `editor` APIs; the equivalent of Emacs's `M-:` and `*scratch*`. Completion and history.
- **Hot reloading:** when `init.js` or plugin files change they are reloaded without a restart; old Disposables are cleaned up.
- **Debug socket:** `kalem --debug-socket` lets you evaluate JS in the running application over a local Unix socket or TCP; the `kalem repl` command connects to it. Localhost only, off by default.
- **Inspection commands:** `kalem.inspect.tree(offset)` dumps the CST, `kalem.inspect.commands()` the command registry, `kalem.inspect.timings()` the durations of recent operations.
- **Test hook:** end-to-end tests drive the running application through the same socket (open, edit, save, verify).

### 11.10 Extension points

What a plugin can add, and how each extension point appears in the two frontends and in batch mode:

| Extension point | Plugin adds | Graphical | Terminal | Batch / CLI |
|---|---|---|---|---|
| Commands, keybindings | Actions | Palette, menus, toolbar, context menu | Palette, keys | `kalem run` |
| Link types | `[[type:...]]` behavior | Click, hover card, custom inline rendering | Click, hover line | Export output |
| Block renderers | Special blocks, src languages | Widget tree or SVG image in place of the block | Widget tree, or image through the graphics protocol, or text | Export output |
| Decorations | Highlights, badges, gutter marks, virtual text | Yes | Yes (colors, glyphs) | – |
| Completion and hover | Suggestions after `[[`, `#+`, `:`, `@`, and custom triggers | Popup | Popup | – |
| Input rules | Text replacements and autoformat | Yes | Yes | – |
| Languages and modes | Syntax definitions, comment tokens, indentation rules, new document modes | Plain text mode | Plain text mode | – |
| Views | Alternative views of a document (kanban board over headlines, timeline, mind map) | Editor area or panel | Full-screen or panel | – |
| Panels | JSON widget tree (D11) | Side or bottom panel | Side or bottom panel | – |
| Diagnostics | Document checks with ranges and fixes | Squiggles, problems panel | Underline, problems list | `kalem check` |
| Export backends and filters | New formats, output changes | Export dialog | Export dialog | `kalem export --to <name>` |
| Importers, paste handlers | New input formats | Open and paste | Open and paste | `kalem import` |
| Babel languages | Executors | Run button | Run key | `kalem run`, `kalem tangle` |
| Table functions | TBLFM functions | Formula bar | Formula bar | `kalem table recalc` |
| Dynamic blocks | Generated content | Refresh button | Refresh key | `kalem fmt --update-dynamic` |
| Agenda views, capture templates | Queries, templates | Agenda panel | Agenda panel | `kalem agenda --view <id>` |
| CLI subcommands | Batch tools | – | – | `kalem <subcommand>` |
| Themes | Colors, glyph sets | Yes | Yes | – |

**Example: a kanban board plugin.** It registers a view that reads TODO headlines under a headline tagged `:board:`, shows them as columns by TODO state, and turns drag and drop into `setTodo` calls. The document stays ordinary Org; in Emacs it is a normal outline.

```ts
export function activate(ctx: kalem.PluginContext) {
  ctx.subscriptions.push(
    kalem.views.register("kanban", {
      title: "Board",
      when: "documentHasTag:board",
      render: (doc) => ({
        type: "columns",
        children: doc.todoKeywords().map((state) => ({
          type: "column",
          title: state,
          children: doc.find({ tag: "board" }).flatMap((h) => h.children())
            .filter((h) => h.todo === state)
            .map((h) => ({ type: "card", title: h.title, onDrop: { command: "kanban.move", args: [h.id] } })),
        })),
      }),
    }),
    kalem.command("kanban.move", {
      title: "Move card",
      run: (id: string, state: string) => editor.document.headlineById(id)?.setTodo(state),
    }),
  );
}
```

**Example: a diagram block.** It registers a renderer for `#+BEGIN_SRC mermaid` that returns an SVG. The GUI draws the SVG; the terminal draws it through a graphics protocol or falls back to the source; the HTML exporter embeds the SVG.

**Limits.** Plugins cannot change the parser grammar, cannot draw arbitrary pixels outside the widget tree and SVG, and cannot block the UI thread beyond the time budget (11.6). Heavy features (layout engines, large computations) go into worker plugins or, from phase 4, WASM plugins.

---

## 12. Babel: source blocks

- **Syntax:** `#+BEGIN_SRC lang :header args`, `#+CALL:`, `src_lang{...}`, `#+RESULTS:` blocks.
- **Header arguments:** `:results` (output, value; raw, table, list, verbatim, file, drawer; replace, append, prepend, silent), `:exports` (code, results, both, none), `:var`, `:dir`, `:cache`, `:tangle`, `:file`; `:session` and `:noweb` in phase 4.
- **Executors:** shell (sh, bash, zsh), python, javascript (node or the in-app QuickJS), R, gnuplot, sqlite, org, simple calc-like arithmetic. Plugins add languages with `kalem.babel.registerLanguage`.
- **Trust model:** consent on the first "run" request in a document; a "trust this document" decision is bound to the document path and content hash; no code runs automatically on open, not even through `#+STARTUP`.
- **Result insertion:** `#+RESULTS:` placement per Org rules, matching through `#+NAME`, replacement of old results.
- **Tangling:** writes to files, with a confirmation list for each write.
- Progress indicator and cancellation for running blocks.

---

## 13. Agenda and workspaces

- When a workspace folder is opened, `.org` files are indexed in the background and kept up to date with a file watcher. The index lives in memory; a disk cache for large folders (D8).
- **Views:** daily and weekly agenda (SCHEDULED, DEADLINE, active timestamps, repeaters, warning delays, hiding with `CLOSED`), TODO list, tag and property search (a subset of Org's match syntax: `+work-urgent/TODO`, `PRIORITY="A"`), full-text search.
- **Actions:** jump from the view to the document, change TODO, reschedule (drag and drop calendar), start the clock.
- **Capture:** quick notes with templates defined in TOML; target file and headline.
- **Refile:** headline picker across the workspace, fuzzy search.
- **Clock:** clock in and out, CLOCK lines under `:LOGBOOK:`, daily and weekly totals, running clock indicator.
- Single-file mode is the default; the agenda appears only when a workspace is opened.

---

## 14. Settings and configuration

Layers, later ones override earlier ones:

1. Built-in defaults
2. User `settings.toml`
3. Workspace `.kalem/settings.toml`
4. Document `#+` keywords (for document behavior only)

The user's files (`settings.toml`, `keymap.json`, `init.js`) live in `$KALEM_CONFIG_DIR`, or in `kalem` under `$XDG_CONFIG_HOME`, `%APPDATA%` on Windows, or `~/.config`. The workspace file is `.kalem/settings.toml` in the document's directory or the nearest ancestor that has one. Every value is checked; a wrong one is reported and the layer below applies (D9). Logs go to `kalem.log` in the state directory (`$KALEM_STATE_DIR`, or `kalem` under `$XDG_STATE_HOME`, `%LOCALAPPDATA%` on Windows, or `~/.local/state`); `KALEM_LOG` or `log.level` sets the level.

Example `settings.toml`:

```toml
[editor]
font_family = "Georgia"
font_size = 16
line_width = 80
keymap_profile = "word"        # "word" | "vim"
show_source_markers = "cursor" # "cursor" | "always" | "never"

[org]
todo_keywords = ["TODO", "NEXT", "|", "DONE", "CANCELLED"]  # when the document has no #+TODO
log_done = "time"
assets_dir = "{name}_assets"

[export]
pdf_engine = "auto"            # "auto" | "latexmk" | "tectonic"
pandoc_path = ""

[plugins]
enabled = ["com.example.wordcount"]
```

---

## 15. Performance targets

| Metric | Target |
|---|---|
| Cold start, empty document | under 300 ms |
| Opening a 1 MB document | under 200 ms |
| 10 MB document, until interactive | under 1 s |
| Keystroke → screen latency | under 16 ms, p99 under 33 ms |
| Incremental parse, typical edit | under 2 ms |
| Memory, empty document | under 80 MB |
| Memory, 10 MB document | under 500 MB |
| Binary size, including math fonts | under 40 MB |
| Saving a 10 MB document | under 100 ms |
| Terminal frontend startup | under 50 ms |
| CLI subcommand on a 1 MB file (`check`, `fmt`) | under 100 ms |
| Terminal-only build, binary size | under 15 MB |
| Opening a 100 MB plain text file, until interactive | under 1 s |

Strategy: contiguous text with an incremental line index for Org documents (the parser reads contiguous text; a rope for very large plain text files), incremental CST, virtualized rendering, lazy model, background work, formula and image caches. A benchmark exists for every target and runs in CI; regressions block the PR.

---

## 16. Testing strategy

| Layer | Method |
|---|---|
| org-syntax | Snapshots (insta), round-trip fuzzing, Emacs differential testing, random document generation with proptest, criterion benchmarks |
| org-model | Unit tests; table-driven tests for inheritance, repeaters, statistics cookies |
| org-edit | Before and after snapshots per command; undo property test (`undo(redo(x)) == x`) |
| org-table | Corpus of tables computed in Emacs; snapshots of the formula parser |
| org-math | Image snapshots of a formula corpus (pixel diff threshold) |
| org-export | Snapshots per backend; comparison corpus against ox.el output |
| org-babel | Unit tests with fake executors; integration with real interpreters (optional in CI) |
| kalem-core | Unit tests for the command registry, keymap and when-clauses |
| kalem-script | API contract tests; consistency of d.ts with the real bindings; time and memory limits |
| kalem-ui | Widget tests with the gpui test harness; manual release checklist |
| kalem-tui | Rendering snapshots with ratatui's test backend; capability fallback tests |
| kalem-cli | Golden tests for every subcommand's output and exit code |
| End to end | Open corpus files, edit them by script, save, verify with Emacs |

Licenses of corpus files are recorded in the repository. User data never enters the corpus.

---

## 17. Packaging and distribution

- Single binary; release automation with `cargo-dist`.
- Two build flavors: the full build (GUI, TUI and CLI) and a terminal-only build without GPU and windowing dependencies, for servers and minimal systems (`cargo install kalem-editor --no-default-features --features tui`). The binary starts the GUI when a display is available and no subcommand is given, and prints a hint to use `kalem tui` otherwise.
- macOS: `.app` bundle, signing and notarization, Homebrew cask.
- Windows: MSI and portable zip; signing.
- Linux: AppImage, Flatpak; `.deb` and `.rpm` secondary; AUR by the community.
- Automatic updates: phase 4; notifications and links in early releases.
- External tools (pandoc, tectonic, interpreters) are not bundled; they are detected and the user is pointed to downloads.
- Versioning: semver; 0.x before 1.0, breaking changes in CHANGELOG.

---

## 18. Open source and community

### 18.1 License (D1, decided)

Kalem is dual-licensed **MIT OR Apache-2.0**, the Rust ecosystem standard, for all crates.

| Option considered | Pros | Cons |
|---|---|---|
| MIT OR Apache-2.0 (chosen) | Widest reuse; `org-*` crates drop easily into other projects; low contribution friction | Closed commercial forks are possible |
| GPL-3.0 | License alignment with Emacs and Org; protection against closed forks | Narrows reuse as a library; plugin licensing debates |

Test corpus files keep their own licenses (for example the Org Manual is GFDL) and are recorded in `tests/corpus/LICENSES.md`. They are test data, not part of any distributed crate.

### 18.2 Repository and process

- Single repository (monorepo) on GitHub under `kalem-editor`. CI: Linux, macOS, Windows; Emacs differential tests on Linux.
- `CONTRIBUTING.md`, code of conduct, "good first issue" labels, PR template.
- An `rfcs/` folder for large decisions; this document is RFC 0001.
- `CHANGELOG.md`, semver, a regular release rhythm.
- Documentation: user manual, plugin API and architecture with mdBook. Documentation is written in Org and produced by Kalem's own exporter (dogfooding).
- Language: repository, code and documentation in English; the UI in English and Turkish.

### 18.3 Relationship with the Emacs community

- Announcement on the Org mailing list; no changes to the format are proposed.
- Ambiguities in the Org Syntax document are reported upstream.
- The differential testing infrastructure may be useful to Org itself; it is shared.

---

## 19. Risks and mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| A WYSIWYG editing engine in Rust is harder than expected | High | High | Three-week gpui spike with an explicit go/no-go; Tauri fallback; independent core |
| Breaking changes in gpui's API | Medium | Medium | Version pinning; thin UI layer; follow Zed releases |
| Ambiguities in Org syntax | High | Medium | Emacs differential testing; documenting ambiguities; org-element behavior as the reference |
| Scope creep | High | High | Non-goals list; phase exit criteria; a phase number for every feature |
| Single-developer burnout | Medium | High | Small shippable pieces (parser crate first); early community; buffer in the roadmap |
| Insufficient math rendering quality | Low | Medium | D4 measured on a 100-formula corpus: RaTeX 98/100; the engine sits behind an `org-math` trait so it can be swapped |
| Table results diverging from Emacs | Medium | Medium | Corpus computed in Emacs; decimal arithmetic; documenting deviations |
| Low adoption | Medium | High | "Typora for Org" positioning; early access for P1 and P4; announcement to the Emacs community |
| External tool dependencies (pandoc, TeX) tire users | Medium | Low | Detection and guidance; built-in HTML and Markdown; tectonic as an optional download |
| Two frontends double the UI work | Medium | Medium | All behavior lives in `kalem-core`; frontends only render and translate input; shared syntax highlighting and widget tree |
| Plugin security holes | Low | High | Permission model; sandbox; time and memory limits; security policy |

---

## 20. Roadmap

Durations are rough estimates for a single developer. The next phase does not start before the exit criteria of the current one are met.

### Phase 0: Discovery and foundation (2 to 3 months)

- Repository, CI, license, name.
- orgize evaluation and parser decision (D2).
- `org-syntax`: a complete lossless parser, Emacs differential testing infrastructure, corpus.
- The `kalem` binary with its first subcommands (`kalem-cli`): `parse`, `check`, `diff-emacs`.
- gpui spike and UI decision (D3).
- Math rendering prototype and decision (D4).

**Exit:** 100% round-trip on the corpus; 99% on the Emacs differential; spike report and decisions D2, D3, D4.

### Phase 1: MVP, "Typora for Org" (4 to 6 months)

- `org-model`, `org-edit` core; undo and redo.
- `kalem-core`: command registry, keymap, settings.
- Plain text mode for non-Org files: the source view editor with syntax highlighting (2.6).
- `kalem-tui`: the first frontend. It has no framework risk, gives a usable editor early for dogfooding, and proves that `kalem-core` is frontend independent before the GUI is built.
- `kalem-ui`: WYSIWYG editor (headlines, folding, paragraphs, emphasis, lists, checkboxes, links, basic tables, source blocks), source view, outline panel, command palette, find and replace, status bar.
- TODO cycling, priorities, tags, timestamp display.
- Open, save, external change detection.
- Theme and font settings; English and Turkish UI.
- CLI: `fmt`, `query`.
- First public release (0.1), with both frontends.

**Exit:** the Org Manual source opens, is edited and saves without a diff; the startup and keystroke latency targets hold; feedback from ten external users.

### Phase 2: Document author (4 months)

- `org-table`: TBLFM engine, formula bar, sorting, CSV.
- `org-math`: inline formula preview.
- `org-export`: HTML, LaTeX, Markdown, plain text; PDF generation; pandoc bridge (DOCX, ODT, EPUB); import; `kalem export` and `kalem table recalc`.
- `org-cite` and hayagriva.
- Images, footnotes, planning lines, property drawers, table of contents.
- Rich text copy and paste.

**Exit:** a book chapter (figures, tables, formulas, citations) exports to LaTeX and PDF without errors; the table corpus computes identically to Emacs.

### Phase 3: Extensibility and tasks (4 months)

- `kalem-script`: QuickJS, API, d.ts generation, plugin loader, permissions, console; `init.js`; batch mode `kalem run`.
- Example plugins and a template repository.
- `org-babel`: shell, python, js, gnuplot; trust model; tangling.
- `org-agenda`: workspace, agenda views, capture, refile, clock.
- Spell checking, reveal.js and Beamer export.

**Exit:** three community plugins; the agenda in daily use; a gnuplot chart produced in a document through Babel.

### Phase 4: Maturity

- Lua as a second scripting language (D10), WASM plugins, out-of-process protocol.
- Babel `:session` and `:noweb`; column view; org-habit.
- Presentation mode; embedded PDF preview; automatic updates.
- Accessibility improvements; performance tuning; 1.0.

---

## 21. Open decisions

| ID | Decision | Options | Recommendation | Status |
|---|---|---|---|---|
| D1 | License | MIT OR Apache-2.0; GPL-3.0; mixed | MIT OR Apache-2.0 everywhere | **Decided:** MIT OR Apache-2.0 (18.1) |
| D2 | Parser foundation | orgize dependency; orgize fork; new parser | After evaluation | **Decided:** new parser following org-element (5.6, `docs/decisions/D2-parser-foundation.md`) |
| D3 | UI framework | gpui; Tauri + ProseMirror; iced/floem | gpui, validated by the spike | **Decided:** gpui, with Kalem's own inline layout (7.1, `docs/decisions/D3-ui-framework.md`) |
| D4 | Math engine | mitex + typst; ReX; RaTeX; KaTeX | After the corpus comparison | **Decided:** RaTeX (9.2, `docs/decisions/D4-math-engine.md`) |
| D5 | tectonic | Bundle; separate download; system TeX only | Separate download | Open |
| D6 | API definition source | Rust macros; separate IDL; hand-written d.ts | Single definition in Rust, d.ts and Lua annotations generated | Open |
| D7 | Project name | – | – | **Decided:** Kalem; crate `kalem-editor`, binary `kalem`, GitHub `kalem-editor` (section 0) |
| D8 | Agenda index storage | In memory; SQLite; custom file | In memory, disk cache later | Open |
| D9 | Configuration formats | TOML + JS; JS only; JSON | TOML + init.js + keymap.json | **Decided:** `settings.toml`, `keymap.json` (comments allowed), `init.js` (14, `docs/decisions/D9-configuration-formats.md`) |
| D10 | When to support Lua | Phase 3; phase 4; never | Phase 4, on demand | Open |
| D11 | Webviews in plugin panels | Never; optional | Never; JSON widget tree | Open |
| D12 | Multiple documents | One window one document; tabs; multiple windows | Tabs, phase 2 | **Decided (owner, 2026-09-28):** one window holds many documents, listed on the left or as tabs at the top, grouped by project (2.8) |
| D13 | Time library | jiff; chrono | jiff | **Decided:** jiff; date arithmetic follows Emacs's `encode-time` normalization on top of it (`org-model::time`) |
| D14 | Terminal UI stack | ratatui + crossterm; termwiz; custom | ratatui + crossterm, ratatui-image for graphics | **Decided:** ratatui + crossterm + ratatui-image (7.6, `docs/decisions/D14-terminal-ui-stack.md`) |
| D15 | When to spin out ecosystem crates | From the start; when stable (4.7) | When the API is stable, per 4.7 | **Decided:** incubate in the monorepo, spin out when stable (4.7) |
| D16 | Syntax highlighting engine | syntect (Sublime syntax definitions, pure Rust with fancy-regex); tree-sitter (incremental, structural, C grammars) | syntect first for breadth and Sublime compatibility; tree-sitter later for structure-aware features | **Decided:** syntect with `regex-fancy`, in `kalem-highlight` (`docs/decisions/D16-syntax-highlighting.md`) |
| D17 | Vim mode engine | Own engine in kalem-core; reuse an existing crate; embed Neovim | Own engine, spun out if it proves reusable (4.7); Neovim embedding rejected for size and dependency reasons | **Decided:** own engine, `kalem_core::vim`; the Vim profile replaces the Emacs Org profile (owner, 2026-09-28) |
| D18 | Entity table provenance | Keep with attribution; split (names and UTF-8 in `org-syntax`, export renderings elsewhere); ask the Org maintainers and the FSF; GPL for `org-syntax` | Split now, ask in parallel (`docs/decisions/D18-entity-table-provenance.md`) | Open: owner decision, blocks publishing `org-syntax` |
| D19 | Markdown parser | pulldown-cmark (offset iterator); comrak (AST with source positions); tree-sitter-markdown; own parser | pulldown-cmark: fast, CommonMark and GFM, MIT, offsets are enough because editing stays text-based | Open |
| D20 | File operations for the file manager (2.7) | `trash` crate plus std::fs with own copy, move and progress; `fs_extra`; shelling out to system tools | `trash` for deletion, own operations on std::fs for progress, cancellation and conflict handling | Decided 2026-09-28 (docs/decisions/D20-file-operations.md) |

---

## 22. Glossary

| Term | Meaning |
|---|---|
| CST | Concrete Syntax Tree; a tree that preserves every token, including whitespace and markers |
| AST | Abstract Syntax Tree; a tree that preserves only meaning |
| Rope | A data structure for fast insertion and deletion in large texts |
| Round-trip | Parsing text and writing it back yields the same text |
| Element | A line-level structure in Org: headline, paragraph, table, block |
| Object | An inline structure in Org: emphasis, link, timestamp |
| TBLFM | The table formula line `#+TBLFM:` |
| Babel | Org's source block execution system |
| Tangle | Writing source blocks out to source files |
| Agenda | A multi-file view of scheduled items and tasks |
| Capture | Adding a quick note from a template |
| Refile | Moving a headline under another headline |
| Drawer | A foldable section between `:NAME:` and `:END:` |
| Planning | The SCHEDULED, DEADLINE, CLOSED line under a headline |
| org-cite | Org's built-in citation syntax |
| Backend | An export target (HTML, LaTeX, ...) |
| When-clause | An expression describing the context in which a command or shortcut applies |
| Spike | A time-boxed prototype that measures a technical risk |

---

## 23. References

- Org Syntax: https://orgmode.org/worg/org-syntax.html
- Org Manual: https://orgmode.org/manual/
- org-element.el: Org source tree, `lisp/org-element.el`
- orgize: https://github.com/PoiScript/orgize
- rowan: https://github.com/rust-analyzer/rowan
- gpui: https://github.com/zed-industries/zed/tree/main/crates/gpui
- rquickjs: https://github.com/DelSkayn/rquickjs
- quickjs-ng: https://github.com/quickjs-ng/quickjs
- typst: https://github.com/typst/typst
- mitex: https://github.com/mitex-rs/mitex
- hayagriva: https://github.com/typst/hayagriva
- tectonic: https://tectonic-typesetting.github.io/
- spellbook: https://github.com/helix-editor/spellbook
- extism: https://extism.org/
- pandoc: https://pandoc.org/
- Typora (product model): https://typora.io/
- ratatui: https://ratatui.rs/
- ratatui-image: https://github.com/benjajaja/ratatui-image
- Neovim Lua API (plugin model reference): https://neovim.io/doc/user/lua.html
- Figma plugin sandbox (QuickJS usage): https://www.figma.com/plugin-docs/how-plugins-run/
