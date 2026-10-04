# Kalem: A Rendered Editor for Plain-Text Documents — Design Document (RFC 0001)

> The Kalem format (`.klm`, RFC 0003, Part III of the Book) was removed on 2026-10-04 by the owner's decision; the sections about it are kept as the design record.

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

This document defines what Kalem is, what it is not, how it will be built and in what order. It is updated as decisions change.

**Scope note (2026-09-30).** This document was written on 2026-09-27 for an editor of Org files. RFC 0002 (`design_doc2.md`, accepted by the owner on 2026-09-30) generalized Kalem into an editor of plain-text formats, each faithful to its standard and checked against a reference implementation (Org, LaTeX, CSV, BibTeX, Markdown, plain text; sections 2.6 and 9.5 and RFC 0002 section 2), with a document format of its own (RFC 0003) and Emacs's file manager and projects (2.7, 2.8). Section 1 states the vision as it now stands. Later sections keep their Org wording where the Org mode is meant; the Book (`book/`) is the reference for every format as implemented. Unresolved decisions are tracked in section 21 with numbered IDs (D1, D2, ...). When a decision is made it is moved into the relevant section and marked "decided" in section 21.

Audience: the project owner, future contributors, plugin authors.

The words **MUST**, **SHOULD** and **MAY** are used in the RFC 2119 sense.

The application is called **Kalem** ("pen" in Turkish). Naming conventions:

| Artifact | Name |
|---|---|
| Product | Kalem |
| Binary | `kalem` |
| Application package on crates.io | `kalem-editor` (`kalem` is taken) |
| GitHub organization | `getkalem` (`kalem-editor` until 2026-09-28) |
| Library crates (UI independent, reusable) | `org-*` |
| Application crates | `kalem-*` |
| Graphical frontend | `kalem-ui` (gpui) |
| Terminal frontend | `kalem-tui` (ratatui) |
| Candidate domain | `kalemeditor.org` |

---

## 1. Vision and scope

### 1.1 One sentence

A fast, single-binary, open source editor that shows plain-text documents the way they read and keeps them plain text: Org, LaTeX, CSV, BibTeX, Markdown and code, each opened as itself, edited in place and written back exactly as its standard defines it, nothing added and nothing dropped; a document format of Kalem's own, `.klm`, for what those formats cannot carry; Emacs's file manager, projects and keys without Emacs; and the same editor in a window and in a terminal, with every document operation available from the command line. (The one sentence of 2026-09-27 was "Typora for Org"; the Org mode remains the first and most complete of the standard modes.)

### 1.2 Problem

- Plain-text formats carry most serious writing: Org for notes, outlines and tasks, LaTeX for papers and theses, Markdown for documentation, CSV for data, BibTeX for references. Each has its own tools, and none of the tools shows the file as it reads while leaving the file exactly as it was: editors rewrite spacing and markup (Orgzly, MarkText, Obsidian's front matter), convert the file into their own model to open it (LibreOffice), or show only the source with a preview beside it (VS Code, TeX editors).
- Org in particular is locked into Emacs, whose learning curve keeps out most people who would benefit from the format; the alternatives are partial (Organice, Orgzly, beorg, Logseq, VS Code extensions), and there is no full rendered Org editor on the desktop.
- LaTeX is edited as source everywhere; a rendered editor that stays byte-faithful to standard LaTeX does not exist.
- Office suites and Electron-based note applications are heavy, not plain text, or use closed formats; and no plain-text format covers the whole range of a word processor (styles, page layout, page-quality output) with Org's structure and LaTeX's mathematics.
- A rendered, lossless editor needs one engine (ranges into the file, never a regenerated tree) and one proof per format (a reference implementation to agree with). Built once, that engine serves every format the same way.

### 1.3 Goals

| ID | Goal |
|---|---|
| G1 | **Lossless.** When a file created in Emacs is opened and saved, every untouched byte stays the same. |
| G2 | **Usable without Emacs.** A user can write documents, lists, tables, tasks and formulas without ever seeing Org syntax. |
| G3 | **Light and fast.** Single binary. Cold start under 300 ms, a 10 MB file under 1 s, keystroke latency under 16 ms. |
| G4 | **Org core built in.** The element table in section 3.2, following the phase plan. |
| G5 | **LaTeX.** Inline math preview and LaTeX/PDF export, good enough for writing books and papers; and a rendered editor for `.tex` files themselves, faithful to standard LaTeX (9.5). |
| G6 | **Extensible.** Plugins add real features (block types, link types, views, exporters, checks) through Org's own extension points, so documents stay valid Org. Command registry, events, and plugins written in Rust that run as sandboxed WebAssembly components (D28). |
| G7 | **Coexists with Emacs.** The same file can be edited alternately in both applications without diff noise. |
| G8 | **Reusable.** The parser and exporters are published as independent crates. |
| G9 | **Usable from the terminal, never second class.** A terminal frontend with the same editing semantics and the same features wherever a terminal can carry them (4.1, principle 7), plus a scriptable command line and batch mode. |
| G10 | **A general purpose text editor too.** Markdown files open in a WYSIWYG view like Org's, CSV files in an editable grid, and every other text file as plain text with syntax highlighting, so Kalem can be the only editor someone needs for notes, tables, configuration files and small code edits. |
| G11 | **Beyond Emacs's limits.** Every file Emacs opens, Kalem opens with the same meaning; the reverse is not required. Kalem does not inherit Emacs's implementation limits (size, speed, nesting depth, regexp length, blocking work). See 3.6. |

### 1.4 Non-goals

- **Microsoft Office compatibility.** docx is not a native format. Import and export go through pandoc.
- **Page layout editor.** Margins, columns, page breaks, page number placement. Org is a semantic format; presentation is decided at export time. For the Kalem format, layout is a property of its stylesheet and its typeset output (RFC 0003, sections 11 and 13), still not of the editing surface.
- **A full spreadsheet.** Pivot tables, a charting engine, hundreds of thousands of rows.
- **A visual slide designer.** Presentations are export targets plus a simple presentation mode.
- **Emulating Emacs.** Elisp, the Emacs key language, every agenda setting.
- **Real-time collaboration and cloud sync.** Git and file sync are considered sufficient.
- **Mobile platforms.**
- **A full IDE.** Debuggers and build systems are not built in. Language servers arrive through language plugins (11.14; Python, Elixir, the web languages, PHP, Go, Rust, C and C++ first, group 3.8), which make Kalem a complete editor for a language on the core's language server client; the core itself knows no language.
- **100% of Org in the first release.** Scope is split into phases.

### 1.5 Target users

| ID | Persona | Need |
|---|---|---|
| P1 | Co-author | Must edit the same .org file with a co-author who uses Emacs; does not want to learn Emacs. |
| P2 | Plain-text knowledge worker | Leaving Obsidian or Notion; wants tasks, notes and documents in one place, with files on their own disk. |
| P3 | Academic, book author | LaTeX output, citations, formulas, long documents, chapter files. |
| P4 | Former Emacs user | Has years of .org files but has left Emacs. |
| P5 | Plugin developer | Has written Obsidian or VS Code extensions; writes Rust, or writes it with AI assistance; expects a typed API, a template and a conformance suite. |
| P6 | Terminal user | Works over SSH or in tmux; wants a rendered editor for Org, LaTeX and CSV in the terminal, friendlier than Emacs, and scriptable export and formatting. |
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
| Predictive text, AutoComplete | none (Emacs dabbrev, company) | Completers (11.12): the words of the document, the dictionary of the document's language, later a model | 2, 3 |
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

- **A document is a single `.org` or `.klm` file (RFC 0003 for `.klm`).** UTF-8. Line endings are taken from the file (LF or CRLF) and preserved. A BOM is preserved.
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
- A `.org` file never receives Kalem's own markup, with no opt-in. Word-like formatting belongs to the Kalem format (RFC 0003).
- No file locking.

### 2.6 Other files: a general purpose text editor

Kalem opens any file: a text file in one of the document modes below, every other file through a viewer or editor plugin (11.13). Kalem never converts a file to another format in order to open it (D55). The document modes:

| Mode | Files | View |
|---|---|---|
| Org | `.org`, `.org_archive` | The WYSIWYG editor (the rest of this document); strict Org, Kalem writes nothing Org does not define |
| Kalem format | `.klm`; stylesheets `.klms` | The rendered editor of the Kalem format (RFC 0003): one command syntax, Org's structure, LaTeX mathematics, styles and layout, canonical serialization |
| Markdown | `.md`, `.markdown`, `.mdown`, `.mkd` | A WYSIWYG view like the Org one: hidden markers revealed at the cursor, rendered headings, lists, task lists, tables, images and math (2.6.1) |
| CSV | `.csv`, `.tsv`, `.tab` | An editable grid, like a light spreadsheet (2.6.2) |
| LaTeX | `.tex`, `.ltx` (`.sty`, `.cls`, `.bst` as plain text) | The rendered editor for LaTeX documents (9.5): standard LaTeX stays standard LaTeX |
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
| Plugin-defined highlighters and document modes with a renderer (11.11) | 3 |

#### 2.6.1 Markdown mode

- **Dialect:** CommonMark with the GitHub extensions (tables, task lists, strikethrough, autolinks, footnotes), YAML or TOML front matter, and `$...$` / `$$...$$` math (rendered with the D4 engine).
- **Lossless editing, as for Org:** the file is edited as text, never regenerated from a tree. The parser only produces source ranges for the view, so untouched bytes stay as they were.
- **View:** the inline model of the Org mode (6.3): emphasis, code and link markers hidden away from the cursor, headings by level, clickable task list checkboxes, rendered images and math, code blocks with highlighting (D16), front matter folded.
- **Editing:** autoformat triggers (`#`, `-`, `1.`, `>`, `` ``` ``), Enter continues lists and quotes, tables edited in the grid shared with Org tables (8), outline sidebar from headings, "Convert to Org" and "Convert from Org" through the exporter (10) or pandoc.
- **Parser (D19, decided by the owner on 2026-10-01):** comrak, the Rust port of GitHub's `cmark-gfm`, so a file reads exactly as on GitHub, in Kalem's fork `getkalem/comrak` where its defects for an editor are fixed (source positions as byte ranges, every node inside its parent) and offered upstream; reparsed from the enclosing top-level block on each edit.

#### 2.6.2 CSV mode

- **Grid view:** a virtualized table with a header row (detected, can be toggled), column widths, frozen header, sorting and filtering in the view (the file is not reordered unless the user asks), cell editing, inserting, deleting and moving rows and columns, copy and paste of ranges (TSV on the clipboard, so spreadsheets interoperate).
- **Dialect detection and preservation:** delimiter (`,` `;` tab `|`), quoting (RFC 4180), line endings, encoding and BOM are detected and kept. Only edited records are rewritten; quoting is added only where a value needs it.
- **Large files:** 100,000+ rows open quickly; records are indexed lazily and only visible rows are parsed.
- **Beyond the grid:** "Open as text"; "Convert to Org table" (for formulas, Org tables and TBLFM are the place to compute, 8); column statistics (count, sum, average) in the status bar; the same grid in the terminal frontend.
- **Library:** the `csv` crate reads records with byte positions, so edits map back to exact ranges of the file.

**Files that are not text** (detected by NUL bytes or invalid UTF-8 in the first block) open through a viewer or editor plugin when one is installed (11.13; D54): PDF, Word, Excel, PowerPoint, images. Without one, the user is told what the file is and which plugin of `getkalem/plugins` opens it, and may open it with the system application. Kalem never converts a file in order to open it (D55).

**Architecture.** A document has a mode. `kalem-core` defines a `DocumentMode` interface, the same contract plugins use (11.11): Org mode provides the view model, commands and structural editing; Markdown mode provides its own view model on the shared inline editing model; CSV mode provides a grid model; plain text mode provides the text view model and language-specific commands (comment toggling, indentation). Commands declare where they apply through their scope (11.2), a list of text types (`org`, `csv`, `python`), and structural context through when-clauses (`inTable`). Both frontends render every mode. The CLI accepts Markdown for conversion (`kalem export README.md --to org`) and CSV for conversion to an Org table; `kalem check` checks Org files only and refuses others with a clear message.

### 2.7 File manager (Dired)

Kalem has a directory editor modeled on Emacs's Dired: a directory opens as a document listing its entries, driven from the keyboard, and the same view works in both frontends. It is the file side of workspaces (13) and of the workspace sidebar (2.6).

**Listing.** One line per entry with type, permissions, size, modification time and name, like Dired's `ls -l` view, or a compact names-only view. Sorting by name, time, size or extension; directories first or mixed; hidden files toggled. Subdirectories can be inserted inline under their line (Dired's `i`) and collapsed again. The listing refreshes itself through the file watcher (`notify`) and keeps marks and the cursor across refreshes.

**Navigation.** Enter (or a click on a name) opens a file in its document mode (2.6), a file that is not text in its viewer or editor plugin (11.13), or descends into a directory; `^` goes to the parent; a filter narrows the listing; "jump to file" from any document opens its directory with the cursor on it (Dired's `dired-jump`). A window has one file manager document, which moves from folder to folder.

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
| `#+STARTUP` | Folding (overview, content, showall, showeverything), indent, odd levels, logging (logdone, logrepeat, logreschedule, logredeadline, logrefile, logdrawer, logstatesreversed), footnotes (fninline, fnlocal, fnauto, fnprompt, fnconfirm, fnanon, fnadjust); display-only values such as hidestars are ignored |
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

**Superseded (owner, 2026-09-30).** The mechanism of this section, Kalem's additions written through Org's extension points, and the `.klm`-as-Org-superset of D24 are retired by RFC 0003: `.org` is strict Org with no opt-in, and `.klm` is the Kalem format, a specification of its own with one command syntax, Org's structure, LaTeX mathematics, styles and layout. The text below stays for the record until T2.13.13 removes the code; the formatting commands it describes are reimplemented as the Kalem format's styles and spans.

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
7. **The terminal is never second class** (asked by the owner, 2026-09-28). Wherever it is possible, the terminal frontend supports a feature as strongly as the graphical one: the same commands, keymaps, settings, panels and plugins, and a terminal form for everything a character grid can carry (text, glyphs, colors, images through the graphics protocols, OSC 8 links, OSC 52 clipboard). A feature lands in both frontends together and is done only when it works in both; what the terminal cannot show (fonts, sizes, pixel layout) gets its nearest honest form and is listed in `book/part-5/terminal-parity.org`, never dropped silently. People over SSH and in tmux (P6) are first-class users.
8. **Only the Kalem format is ours** (owner, 2026-09-30). `.klm` is the one format Kalem designs; every other format, Org, Markdown, CSV, LaTeX, and the files plugins open, is implemented to its own specification with nothing added, removed or changed, and nothing of Kalem's written into it. Anything Kalem wants to add to a document lives in `.klm` alone (D24, D53, D55).

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
  klm-syntax/     The Kalem format's lossless parser (RFC 0003)
  klm-model/      Its document model, on the semantics org-model computes
  klm-edit/       Tree operations with the editor's well-formedness guarantee
  klm-style/      Stylesheets (.klms), compiled to Typst, CSS, LaTeX and DOCX styles
  klm-export/     HTML, PDF through Typst and LaTeX, DOCX, Org and Markdown with loss reports
  org-cite/       org-cite parsing, CSL and BibTeX through hayagriva
  org-babel/      Source block execution (subprocess), result insertion, tangling
  org-agenda/     Workspace index, agenda queries
  kalem-cli/      Command-line subcommands and batch mode: parse, check, fmt, export, diff-emacs, run
  kalem-core/     Editor state, command registry, keymap, settings, event bus
  kalem-script/   WASM host, WIT API and generated bindings, plugin loader
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
kalem-ui / kalem-tui ── command call ──▶ kalem-core: Command Registry ◀── kalem-script (WASM)
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
| Script | Plugin calls from the UI thread have a 100 ms synchronous budget enforced by fuel metering; parsers, renderers and completers run as instances on other threads. Heavy work goes to worker plugins with their own the plugin host runtime, communicating by messages. |
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
| Markdown | comrak, Kalem's fork (D19) | Markdown mode (2.6.1): source ranges only, text edited directly |
| CSV | csv | CSV mode (2.6.2): records with byte positions |
| TUI | ratatui, crossterm, ratatui-image | Terminal frontend; images through kitty, iTerm2 or sixel protocols; D14 decided (7.6) |
| CLI | clap | Subcommands and batch mode |
| Plugins | wasmtime or wasmi (D28, chosen by the spike T3.1.0) | WASM component runtime |
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

What the idea gets right: Emacs's real strength is a live, inspectable, hot-reloadable runtime, and BEAM is the closest modern equivalent. Kalem meets part of this need through inspection and reload (11.9): an inspection panel, hot reloading of plugin components, and a debug socket for the test driver; it ships no scripting engine (D28). Elixir would be the right tool if a multi-user server product (collaboration, web) is ever considered.

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
- **Pre-pass:** keywords at the top of the document and `#+SETUPFILE` content are read to produce `ParseContext { todo_keywords, done_keywords, todo_sequences, link_types, link_abbrevs, radio_targets, inlinetask_min_level, odd_levels_only, footnote_section, list_allow_alphabetical, item_terminator }`: what changes how the text parses. Tags, macros, constants and the other startup options are read from the keywords by the model, the editing commands and the exporter.
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

**Outcome (D2 decided, 2026-09-27):** a new parser. orgize round-trips every corpus file and never panicked, but agreed with org-element on only 72.5% of element and object positions, lacks citations and inlinetasks, and has been dormant since mid-2024. `org-syntax` follows `org-element.el` function by function and reproduces its tree shape and ranges. Details: `book/part-5/decisions/D2-parser-foundation.org`.

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

**Decided: gpui** (spike report: `book/part-5/decisions/D3-ui-framework.org`). Rationale: pure Rust, proven for text editing by Zed, GPU accelerated, single binary. Risk: the API still moves, documentation is sparse, and its editor is not a reusable component; the editing engine is written from scratch.

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
- **Doom Emacs leader keys (Vim profile):** in normal and visual mode the leader (Space unless `editor.vim.leader` says otherwise) starts Doom's sequences: `SPC p p` switches project, `SPC p f` and `SPC SPC` find a file in the project, `SPC ,` and `SPC b b` switch document, `SPC s p` and `SPC /` search the project, `SPC f f` opens a file, `SPC :` is the command palette. A which-key panel lists what may follow a half-typed sequence. They live in `keymaps/vim.json` like any binding, so `keymap.json` changes or removes them (and can use `leader` in its own keys). The goal is Doom's whole basic leader map, prefix by prefix, with a which-key popup showing the continuations of a prefix, and a published table of the keys Kalem does not bind and why (asked by the owner, 2026-09-30; group 2.7i).
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
| Org text objects | Headline (`ih ah`), subtree (`iR aR`), list item (`ii ai`), table cell (`ic ac`) and emphasis (`ie ae`) | 2 |
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

`kalem tui file.org` (or `kalem -t file.org`) opens the same editor in the terminal. It uses the same `kalem-core`, the same commands, keymaps, settings and plugins as the graphical frontend. Only the rendering differs. The terminal is never second class (4.1, principle 7): a feature is done when it works in both frontends, and what the grid cannot show is listed in `book/part-5/terminal-parity.org`.

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

**Stack (D14, decided):** ratatui + crossterm, with ratatui-image for graphics. The spike (`book/part-5/decisions/D14-terminal-ui-stack.org`) draws from the same display model as the GUI spike, costs 0.5 to 1.9 ms per 120 × 50 frame on a 4 MB file, and builds to 6.7 MB with LTO. Consequences:

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
| `kalem run PLUGIN COMMAND [FILE...]` | Batch mode: run a plugin's command against documents with the full `kalem` and `editor` API, without a UI; the equivalent of `emacs --batch` |
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
| C. KaTeX through an embedded script engine → HTML/MathML | Very broad coverage | No native rendering; eliminated |

**Outcome (D4): RaTeX.** On a 100-formula corpus it rendered 98 formulas with no visible errors; typst + MiTeX rendered 88, three of them wrongly, and several failures came from MiTeX lagging typst's symbol renames. RaTeX adds 4.6 MB to the binary against about 35 MB for typst, and its start-up is 1 ms against 11 to 17 ms. Details: `book/part-5/decisions/D4-math-engine.org`. Typst stays a candidate for whole-document export (9.3), which is a separate decision.

### 9.3 Full LaTeX and PDF

- The `org-export` LaTeX backend (ox-latex behavior) produces `.tex`.
- Compilation: `latexmk` or `xelatex` from the system if available. Otherwise **tectonic** can be downloaded on demand (user consent, network access). Bundling is D5; the recommendation is an optional separate download, because tectonic increases binary size considerably and fetches packages from the network.
- PDF preview: the `pdf-viewer` plugin (11.13, group 3.7) in a split when it is installed; the system viewer otherwise. No pdfium in the core.
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

### 9.5 LaTeX documents: a rendered editor for `.tex` files

Asked by the owner (2026-09-28): what Kalem does for Org, done for LaTeX. A `.tex` file opens rendered and is edited in place, with the rigor of the Org mode: the file stays standard LaTeX, byte for byte where untouched, compiled by the real engines, exchanged with journals, arXiv, co-authors and Overleaf without anyone noticing. Kalem does not replace LaTeX and defines no dialect: no new syntax, no rewriting, no "Kalem LaTeX". It is the third pillar of the product next to Org and Markdown (D21): notes and tasks in Org, documentation in Markdown, papers and books in LaTeX, one editor.

**Fidelity.** LaTeX has no reference parser: TeX is a macro language, and the structure of a document depends on the packages and macros it loads. So this mode cannot claim what Org's parser claims. It claims three things instead: it never changes a byte the user did not edit, because it returns ranges into the text (11.11); on the subset it renders, its tree agrees with pandoc's LaTeX reader, the most widely used structural reading of LaTeX, checked on a corpus of real papers; and anything it does not understand stays visible as source, never hidden, never guessed. Unknown macros, `\def`s and package-specific environments are shown as they are, highlighted, and folded when long.

**What is rendered.** Sectioning (`\part` to `\subparagraph`, numbered and starred) as headings with the outline; `\emph`, `\textbf`, `\texttt` and their kin with markers hidden away from the cursor; `itemize`, `enumerate`, `description`; math in `$…$`, `\(…\)`, `\[…\]` and the AMS environments through org-math (9.2), typeset in the line and as displayed formulas; `\cite`, `\ref`, `\eqref` and `\label` as chips with hover (the referenced caption, the BibTeX entry); `figure` and `table` with captions and `\includegraphics` shown inline; `tabular` in the shared grid where its columns are simple, as source otherwise; `verbatim` and `lstlisting` with highlighting; comments dimmed; `\input` and `\include` followed as links and in the outline; the preamble folded; `\newcommand` and `\def` read for the math renderer as `#+LATEX_HEADER` is (9.2).

**Editing.** Enter and Tab in lists, `\begin{…}` completed with its `\end`, brackets balanced, citation and label completers (11.12) fed by the BibTeX grid (2.7h.19 of the work breakdown) and by the document's own labels, input rules for environments, the formatting toggles of the Org mode mapped to `\emph` and `\textbf`; the source view and the split view as in Org.

**Compiling.** The real engines: `latexmk` or `xelatex` when installed, tectonic on demand (9.3), in the background on save or on request, the log's errors mapped to lines and shown in the editor; a PDF panel beside the text (pdfium, 4.3.2 of the work breakdown) with SyncTeX in both directions; Typst as a second target for new documents, never as a translation of `.tex`.

**Verification.** A corpus of real papers (arXiv sources whose licenses allow redistribution, or fetched in CI as the Worg corpus is), and four checks: byte-exact round trip after random edit sequences; structural agreement with pandoc's reader on the rendered subset, with a known-differences document; math snapshots against KaTeX as today; and compile-and-compare, a paper compiled by tectonic before and after an edit round trip giving the same PDF. Regressions block pull requests as the Org differential does.

**Where it lives.** In the core (D29, amended by the owner on 2026-09-28), beside Org and Markdown, on the same mode contract (11.11) and developed in phase 2 with the same weight as Markdown mode: the two are the first consumers of the contract after Org, and the contract is not done until both fit. Typst stays a plugin. The terminal editor renders LaTeX too (4.1, principle 7): headings, emphasis, lists, Unicode math, images where the terminal draws them.

---

## 10. Export and import

### 10.1 Built-in backends

| Backend | Reference | Phase |
|---|---|---|
| HTML | ox-html classes, single file, CSS theme, MathJax or embedded SVG (`tex:svg`, drawn by the D4 engine; Emacs's `dvisvgm`, `dvipng` and `imagemagick` map to it, since they need LaTeX) | 2 |
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

**Small core (decided by the owner, 2026-09-28; D29).** The core is five things: Org (parser, model, editing, tables, the export engine), the Markdown, CSV and LaTeX modes (9.5), the Kalem format (RFC 0003), the text engine with the view model, the two frontends, and the infrastructure that runs before any plugin (commands, keymaps, settings, files, projects, search, the plugin loader). Everything else is a plugin on the public contracts (11.10 to 11.12): every other mode (2.7g of the work breakdown), every other completer, every export back-end beyond HTML, the views, the diagram renderers, the language server bridge. The ones everyone expects ship inside the binary as bundled plugins: embedded WASM components (D28), loaded on first use, so the user sees no difference and the API is proven complete. Markdown and CSV are written against the mode contract too and could move out; they stay in for speed, and because they are the second and third format people bring. New modes and file types are developed in the plugin repository `getkalem/plugins` (11.8), never in the core. The Kalem format is the one place where Kalem itself defines syntax, in a specification with a conformance suite; plugins still extend semantics, not syntax.

### 11.1 Layers

| Layer | Technology | Purpose | Phase |
|---|---|---|---|
| Command registry and events | Rust, `kalem-core` | Foundation for everything | 1 |
| User configuration | `settings.toml`, `keymap.json`, `projects.toml`: data, not code (D9) | Keys, settings, projects | 1 |
| Plugin package | A WASM component (D28) written in Rust against the contracts the core itself uses, with a manifest; any language with a WIT binding is accepted, but Kalem ships no runtime for it | Distributable features: modes, completers, block types, link types, views, exporters (11.10 to 11.12) | 3 |
| Bundled plugins | The same components, embedded in the binary, loaded on first use | Every feature outside the small core (11.0, D29) | 3 |
| Threads | One component instance per thread; several instances of one plugin for parallel work; messages through the host | Parsers, renderers and completers off the UI thread | 3 |
| Out-of-process | JSON-RPC over stdio | Language servers through the core's client (11.14), external tools | 3 (group 3.8), 4 |
| Compiled distribution | Community plugins compiled into a user's own Kalem binary (`kalem build --with`): native speed, full threads, no sandbox, by choice | Power users and servers | 4 |

The API is defined once, in WIT (D6); the Rust bindings are generated from it and published as the `kalem-plugin` crate. **Rust is the plugin language** (owner, 2026-09-28): the contract a plugin implements is the trait the core's own modes and completers implement, so a plugin author reads the same types as a core contributor, the compiler checks the code, and the same crate builds as a bundled plugin inside the binary or as a sandboxed component. Kalem ships no scripting engine and no second language. What that costs, accepted knowingly: no REPL and no instant reload (11.9 shrinks to inspection and reload), a toolchain for plugin authors (hidden by `kalem plugin new` and `kalem plugin build`), a smaller long tail of tiny plugins, and libraries that exist only in other languages (Mermaid, for example) reachable only through a port or a build of that library to WASM inside the plugin. What it buys: native-class speed for parsers and completers, threads by instances, a capability sandbox with fuel and memory limits, one artifact for every platform, and one typed definition that a compiler checks, which matters more as plugin code is written with AI assistance.

### 11.2 Command registry

```rust
pub struct Command {
    pub id: String,                  // "org.todo.cycle", "table.insertRow", "<plugin>.<name>"
    pub title: String,               // for the palette and menus, localized
    pub category: String,
    pub default_keys: Vec<KeyChord>,
    pub scope: Scope,                // "all", or text types with exceptions (Scope, below)
    pub when: Option<WhenClause>,    // structural context: "inTable", "hasSelection"
    pub handler: CommandHandler,     // Rust fn or a plugin's export
    pub args_schema: Option<JsonSchema>,
}

pub enum CommandHandler {
    Native(fn(&mut EditorContext, serde_json::Value) -> CommandResult),
    Plugin(PluginExportId),          // a WASM component's function (D28)
}
```

- Commands run inside a transaction; the command is the unit of undo.
- Plugin commands appear in the same palette, menus and keymap as built-in commands.
- **Offered only where they work (owner, 2026-09-29).** A menu item, toolbar button or palette entry whose command cannot run in the document is not shown, not merely greyed out: Italic is absent from a LaTeX document's menus and toolbar. Menus and toolbars ask with the document's keys alone (`DocumentState::document_context`: its mode, file kind and type) and keep a command whose `when` could still hold, so they do not change as the cursor moves; the palette evaluates the full clause at the cursor. Separators and menus left empty go too. Conversely every mode fills the same places with its own commands: Format, Insert and the toolbar hold LaTeX's formatting, sections and insertions in a `.tex` file where they hold Org's in an Org document, a CSV file gets a Table menu, code files the comment and line commands. New menus, toolbars and context menus follow the same rule, and a new mode adds its commands to them.
- ID convention: `area.action`; plugins use `pluginId.action`.

**Scope (decided by the owner, 2026-09-28).** Every command, built in or from a plugin, says where it applies. `scope` is `"all"` or a list of **text types**, with an optional `except` list; `when` stays for structural context (`inTable`, `hasSelection`, `inHeading`) and fine conditions. A text type is the type of the text at the cursor: the file's type (`org`, `klm`, `markdown`, `csv`, or the language of a plain text file such as `python`, `html`, `json`, from the one vocabulary that `DocumentMode::detect` and `kalem-highlight` share and that plugin modes and highlighters extend), or, inside nested content, the innermost type: a source block or code fence (its language), a LaTeX fragment (`latex`), an export block (its back-end's language), front matter (`yaml`, `toml`), to any depth. Subtypes: `klm` is a subtype of `org`, so `["org"]` applies in `.klm` files too and `["klm"]` only there. Nothing else is an axis: a type's mode (rich view, grid, plain text) follows from the type, so a command never names a mode, and the file kind is the type. Rules: registration refuses a command without a scope, and "everywhere" is spelled `"all"`; the scope compiles to the when-clause key `textType` and joins `when`, so the palette, menus, keymaps, batch mode (`kalem run`, T3.1.16, not built yet) and the documentation evaluate one expression; the palette shows only the commands in scope, the manual and the plugin documentation list commands by type, `kalem commands --type csv` prints them, and the report of bindings that never apply lists unknown types. Completers (11.12) use the same key.

```ts
scope: "all"
scope: ["csv"]
scope: ["org"]                    // .org and .klm
scope: ["klm"]                    // Kalem documents only: the formatting commands of 3.7
scope: ["python", "javascript"]   // a file, or a source block at the cursor
scope: { all: true, except: ["csv", "directory"] }
```

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

### 11.4 API surface

The API is defined in WIT (D6); the Rust bindings are generated from it and published as the `kalem-plugin` crate, which a plugin implements and calls. Sketch, in the shape of those bindings:

```rust
pub mod kalem {
    pub const VERSION: &str;
    pub fn command(id: &str, spec: CommandSpec) -> Disposable;        // title, run, scope (11.2), when, keys
    pub fn run(id: &str, args: &[Value]) -> Result<()>;                // queued: runs once the plugin's call returns (T3.1.12)
    pub fn keymap(keys: &str, command_id: &str, when: Option<&str>) -> Disposable;
    pub fn on<E: Event>(handler: impl Fn(E) + 'static) -> Disposable;

    pub mod ui {
        pub fn notify(message: &str, level: Level);
        pub fn prompt(title: &str, opts: PromptOptions) -> Future<Option<String>>;
        pub fn confirm(message: &str) -> Future<bool>;
        pub fn quick_pick<T>(items: &[PickItem<T>], opts: PickOptions) -> Future<Option<T>>;
        pub mod status_bar { pub fn set(id: &str, text: &str, opts: StatusOptions) -> Disposable; }
        pub mod panel { pub fn register(id: &str, spec: PanelSpec) -> Disposable; }   // widget tree, rendered by both frontends (D11)
    }
    pub mod settings { pub fn get<T>(key: &str) -> T; pub fn set(key: &str, value: Value); pub fn on_change(key: &str, f: impl Fn()) -> Disposable; }
    pub mod fs  { pub fn read(path: &Path) -> Future<String>; pub fn write(path: &Path, text: &str) -> Future<()>; pub fn list(dir: &Path) -> Future<Vec<PathBuf>>; } // permission required
    pub mod net { pub fn fetch(request: Request) -> Future<Response>; }                          // permission required
    pub mod babel { pub fn register_language(name: &str, runner: impl BabelRunner) -> Disposable; }
    pub mod exporter { pub fn register_backend(name: &str, backend: impl ExportBackend) -> Disposable; pub fn add_filter(stage: Stage, f: impl ExportFilter) -> Disposable; }
    pub mod tables { pub fn register_function(name: &str, f: impl Fn(&[Number]) -> Number) -> Disposable; }

    // Extension points for new features (11.10)
    pub mod links { pub fn register(kind: &str, spec: impl LinkType) -> Disposable; }           // resolve, open, hover, complete, render, export
    pub mod blocks { pub fn register(name: &str, spec: impl Block) -> Disposable; }             // special blocks and src languages: render, edit, export
    pub mod decorations { pub fn create(spec: DecorationSpec) -> DecorationSet; }              // highlights, badges, gutter marks, virtual text
    pub mod completers { pub fn register(spec: impl Completer) -> Disposable; }                 // triggers, context, items (11.12)
    pub mod hover { pub fn register(provider: impl Hover) -> Disposable; }
    pub mod input_rules { pub fn register(rule: InputRule) -> Disposable; }                     // for example "->" becomes "→"
    pub mod views { pub fn register(id: &str, spec: impl View) -> Disposable; }                 // alternative document views: kanban, timeline, mind map
    pub mod diagnostics { pub fn register(id: &str, checker: impl DocumentChecker) -> Disposable; } // also run by `kalem check`
    pub mod importer { pub fn register(extensions: &[&str], convert: impl Fn(&[u8]) -> Future<String>) -> Disposable; }
    pub mod paste { pub fn register(mime: &str, handler: impl PasteHandler) -> Disposable; }
    pub mod dynamic_blocks { pub fn register(name: &str, generate: impl DynamicBlock) -> Disposable; }
    pub mod agenda { pub fn register_view(id: &str, spec: impl AgendaView) -> Disposable; }
    pub mod capture { pub fn register_template(id: &str, spec: CaptureTemplate) -> Disposable; }
    pub mod cli { pub fn register(subcommand: &str, spec: impl CliCommand) -> Disposable; }     // `kalem <subcommand>` in batch mode
    pub mod themes { pub fn register(id: &str, theme: ThemeSpec) -> Disposable; }
    pub mod modes { pub fn register(id: &str, spec: impl DocumentMode) -> Disposable; pub fn register_highlighter(syntax: SyntaxSource) -> Disposable; } // renderers and highlighters (11.11)
    pub mod viewers { pub fn register(spec: impl DocumentViewer) -> Disposable; }             // files that are not text: PDF, Office, images (11.13); `DocumentEditor` on top where the write-back is faithful
}

pub mod editor {
    pub fn document() -> Document;
    pub fn selection() -> Selection;
    pub fn insert(text: &str, at: Option<usize>);
    pub fn replace(range: Range, text: &str);
    pub fn transact(label: &str, f: impl FnOnce());
}

pub trait Document {
    fn path(&self) -> Option<&Path>;
    fn text(&self, range: Option<Range>) -> String;
    fn headlines(&self) -> Vec<Headline>;
    fn headline_at(&self, offset: usize) -> Option<Headline>;
    fn headline_by_id(&self, id: &str) -> Option<Headline>;   // ID or CUSTOM_ID property
    fn todo_keywords(&self) -> Vec<String>;
    fn node_at(&self, offset: usize) -> Node;
    fn find(&self, query: Query) -> Vec<Headline>;             // tag, todo, property
    fn keywords(&self) -> BTreeMap<String, Vec<String>>;
    fn save(&self) -> Future<()>;
}

pub trait Headline {
    fn id(&self) -> String;                                    // ID property, created on demand
    fn level(&self) -> u8; fn title(&self) -> String; fn todo(&self) -> Option<String>; fn priority(&self) -> Option<char>;
    fn tags(&self) -> Vec<String>; fn properties(&self) -> BTreeMap<String, String>;
    fn scheduled(&self) -> Option<Timestamp>; fn deadline(&self) -> Option<Timestamp>;
    fn range(&self) -> Range; fn parent(&self) -> Option<Headline>;
    fn children(&self) -> Vec<Headline>; fn body(&self) -> String;
    fn set_todo(&self, state: Option<&str>); fn set_title(&self, title: &str); fn set_tags(&self, tags: &[&str]);
    fn set_property(&self, key: &str, value: Option<&str>);
    fn promote(&self); fn demote(&self); fn move_up(&self); fn move_down(&self);
}

pub trait Table {
    fn rows(&self) -> usize; fn cols(&self) -> usize;
    fn cell(&self, row: usize, col: usize) -> String; fn set_cell(&self, row: usize, col: usize, value: &str);
    fn formulas(&self) -> Vec<String>; fn recalc(&self);
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
  "main": "dist/wordcount.wasm",
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
- Lifecycle: the component exports `activate(ctx)` and `deactivate()`, written in Rust against the `kalem-plugin` crate (`pub fn activate(ctx: &mut PluginContext)`). Disposables are collected in `ctx.subscriptions`.
- Build: one `.wasm` component per plugin, built with `kalem plugin build` (Cargo for the `wasm32-wasip2` target) from the template repository that `kalem plugin new` copies; its imports resolve only against the WIT interfaces the manifest's permissions grant.
- Plugins run unchanged in the graphical frontend, the terminal frontend and batch mode (`kalem run`, T3.1.16). UI calls degrade gracefully in batch mode (prompts return defaults, notifications go to stderr).
- A plugin is a WASM component, not a program with an operating system: no file system, network, clock or threads of its own beyond what the API grants. This is stated on the first page of the plugin documentation.

### 11.6 Security and resource limits

- **Sandbox:** every plugin is a WebAssembly component (D28). It sees only the imports the WIT API grants, no file system, network or clock unless a permission below adds them (one exception, decided by the owner on 2026-10-04: viewers get the `clock` interface, the time, the user's time zone and random bits, so that a workbook's `NOW()`, `TODAY()` and `RAND()` compute as in the program the file comes from; it tells the plugin nothing of the user's files); its memory is its own linear memory; it never touches the document text, only ranges and edits (11.11).
- **Permissions** are declared in the manifest, shown to the user on first run and approved. Scopes: `fs:read:workspace`, `fs:write:workspace`, `fs:read:all`, `net:fetch:<domain>`, `subprocess` (separate, explicit warning).
- **Time limit:** fuel metering; a synchronous call exceeding its budget (100 ms by default) is cancelled with a warning, and a parse that misses it drops the file to plain text (11.11). Long work uses async APIs and further instances on other threads.
- **Memory limit:** per instance, 64 MB by default, enforced by the engine.
- A plugin error never crashes the application; it is shown in the plugin console; a plugin that fails repeatedly is disabled.
- Code inside documents (Babel) has a separate trust model from plugins (section 12).

### 11.7 User configuration

| File | Contents |
|---|---|
| `settings.toml` | Static settings |
| `keymap.json` | Keymap overrides |
| `plugins.toml` | Permission decisions; the enabled plugins are the setting `plugins.enabled` in `settings.toml` |
| `themes/*.toml` | User themes |

### 11.8 Distribution

**Repositories (owner, 2026-09-28).** `getkalem/kalem` holds the core (Org, `.klm`, Markdown, CSV, LaTeX, the text engine, the frontends, the infrastructure of 11.0) and the plugin contracts with their tests. `getkalem/plugins` holds every other plugin: one Cargo workspace, a crate per plugin, a `template/` that `kalem plugin new` copies, CODEOWNERS per plugin, and `index.json`. From now on a new mode, file type, language pack or completer beyond the core is developed there, never in the core.

**Source in, WASM out.** Compiled components are never committed. The repository's CI builds every plugin against the current WIT on each pull request and runs the conformance suite; on a tag it builds each `.wasm` from the tagged source, hashes and signs it (sigstore through GitHub OIDC, or minisign), publishes it as a release asset and as a ghcr.io package, and regenerates `index.json`. Kalem reads the index as a static file, never through the GitHub API, downloads the component, checks the hash and the signature, and shows the permissions before installing. `kalem plugin install NAME|URL|FILE` covers the index, a release asset and a local file for offline use; `kalem plugin build GIT_URL` builds from source for those who have a toolchain or do not trust binaries; `kalem plugin verify` rebuilds and compares. Whether a plugin from this repository ships inside the Kalem binary is a release decision: the release workflow may embed a pinned set (tag and hash) so that the download works out of the box; the code still lives in `getkalem/plugins`. When outside authors multiply, the index also lists plugins kept in their own repositories.

### 11.9 Live runtime

Emacs's "reach into the running program and change it" experience is provided in part. Kalem ships no scripting engine (D28), so there is no REPL; what remains is inspection and reload:

- **Inspection panel:** the loaded plugins with their permissions, budgets and recent errors, the command registry, and the durations of recent operations, in both frontends.
- **Hot reloading:** a plugin's component is reloaded without a restart when its file changes (the plugin folder is watched during development); old Disposables are cleaned up.
- **Debug socket:** `kalem --debug-socket` exposes the inspection commands and the test driver over a local Unix socket or TCP. Localhost only, off by default.
- **Inspection commands:** `kalem::inspect::tree(offset)` dumps the CST, `kalem::inspect::commands()` the command registry, `kalem::inspect::timings()` the durations of recent operations.
- **Test hook:** end-to-end tests drive the running application through the same socket (open, edit, save, verify).

### 11.10 Extension points

What a plugin can add, and how each extension point appears in the two frontends and in batch mode:

| Extension point | Plugin adds | Graphical | Terminal | Batch / CLI |
|---|---|---|---|---|
| Commands, keybindings | Actions | Palette, menus, toolbar, context menu | Palette, keys | `kalem run` |
| Link types | `[[type:...]]` behavior | Click, hover card, custom inline rendering | Click, hover line | Export output |
| Block renderers | Special blocks, src languages | Widget tree or SVG image in place of the block | Widget tree, or image through the graphics protocol, or text | Export output |
| Decorations | Highlights, badges, gutter marks, virtual text | Yes | Yes (colors, glyphs) | – |
| Completers and hover | Completions for a language or a mode: after trigger characters, after a word prefix, or on request (11.12); hover providers | Menu | Menu | `kalem complete FILE:LINE:COL` |
| Input rules | Text replacements and autoformat | Yes | Yes | – |
| Highlighters and document modes | Syntax definitions, comment tokens and indentation rules; document modes with a renderer over the contract of 11.11 | Plain text with the plugin's colors; rendered modes in the editor area | The same | `kalem check`, `kalem fmt`, `kalem export` through the mode's hooks |
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

**Example: a kanban board plugin.** It registers a view that reads TODO headlines under a headline tagged `:board:`, shows them as columns by TODO state, and turns drag and drop into `set_todo` calls. The document stays ordinary Org; in Emacs it is a normal outline.

```rust
use kalem_plugin::{editor, kalem, CommandSpec, PluginContext, Query, Scope, ViewSpec, Widget};

pub fn activate(ctx: &mut PluginContext) {
    ctx.subscriptions.push(kalem::views::register("kanban", ViewSpec {
        title: "Board",
        when: "documentHasTag:board",
        render: |doc| Widget::columns(doc.todo_keywords().iter().map(|state| {
            let cards = doc.find(Query::tag("board")).iter().flat_map(|h| h.children())
                .filter(|h| h.todo().as_deref() == Some(state))
                .map(|h| Widget::card(&h.title()).on_drop("kanban.move", [h.id()]));
            Widget::column(state, cards)
        })),
    }));
    ctx.subscriptions.push(kalem::command("kanban.move", CommandSpec {
        title: "Move card",
        scope: Scope::types(["org"]),
        run: |(id, state): (String, String)| {
            if let Some(h) = editor::document().headline_by_id(&id) { h.set_todo(Some(&state)); }
        },
        ..CommandSpec::default()
    }));
}
```


**Example: a diagram block.** It registers a renderer for `#+BEGIN_SRC mermaid` that returns an SVG. The GUI draws the SVG; the terminal draws it through a graphics protocol or falls back to the source; the HTML exporter embeds the SVG.

**Limits.** Plugins cannot change the parser grammar, cannot draw arbitrary pixels outside the widget tree and SVG, and cannot block the UI thread beyond the time budget (11.6). Heavy features (layout engines, large computations) run in further instances of the plugin on other threads (11.1).

### 11.11 Document modes and highlighters from plugins

Kalem ships highlighters (D16, Sublime syntax definitions in `kalem-highlight`) and renderers for a fixed set of formats (2.6; group 2.7g of the work breakdown). A user must be able to add both for any other text format with a plugin, through one standard way (asked by the owner, 2026-09-28). The rule: **the contract built-in modes use is the contract plugins use.** Markdown (2.6.1) and CSV (2.6.2) are written against it, so a plugin can do everything a built-in mode does, and the contract is proven before the WIT binding exists.

**Highlighters.** `kalem::modes::register_highlighter` loads a Sublime syntax definition from the plugin folder into `kalem-highlight`, with comment tokens and indentation rules; the file opens in plain text mode with the plugin's colors, in both frontends. No code is needed.

**Modes with a renderer.** `kalem::modes::register(id, spec)` adds a document mode (2.6). The spec:

| Part | What the plugin gives | What the core does |
|---|---|---|
| `detect` | Extensions, mode lines, a sniff function over the first bytes | Mode selection as in 2.6, the user's choice first |
| `parse(text, edit?, previous?)` | A tree of nodes, each with a kind from a fixed vocabulary and a byte range: block kinds (heading with level, paragraph, list item with checkbox, quote, code with language, table row and cell, math block, rule) and inline kinds (emphasis, code, link with target, image, math, footnote reference, hidden marker) | Builds the view model of 7.2 from the ranges: markers hidden away from the cursor, widgets for checkboxes, formulas and images, folding and the outline. Both frontends render it; the plugin draws nothing |
| `grid(text)` | Rows and cells with ranges, for table-like formats | The grid of CSV mode (2.6.2): cell editing, sorting in the view, TSV on the clipboard |
| `edit` | Enter, Tab, input rules and toggles (emphasis, heading level, list), each returning text edits with ranges | Applies them through the transaction stack, one undo step each |
| `outline`, `format`, `complete`, `diagnostics` | The language pack hooks | Outline sidebar, Format Document and `kalem fmt`, completion menus, `kalem check` |
| `export` | Optional `toOrg` or `toHtml` | Without them, HTML from the tree; the exporters and pandoc follow |

Rules:

- **Ranges, never text.** `parse` returns ranges into the text and never regenerates it, so a mode cannot break the round-trip guarantee (3.3). The tree is plain data and crosses the component boundary as flat arrays of kinds and ranges, never as objects, so a parse per keystroke stays cheap.
- **Incremental.** `parse` receives the edit and the previous tree and reparses from the enclosing top-level block, as Markdown mode does (2.6.1); a mode without incremental parsing is reparsed whole and must fit the budget.
- **Budget.** The time and memory limits of 11.6 apply to each parse. A mode that exceeds them or traps drops the file to plain text with the plugin's highlighter, tells the user, and is disabled after repeated failures. Heavy parsers run in further instances on other threads (11.1).
- **Two levels.** Declarative: a syntax definition plus a mapping from its scopes to view kinds, no code; enough for gemtext, todo.txt or Fountain. Programmatic: a parser in Rust against the generated bindings; needed for AsciiDoc or Djot.
- **Batch.** `kalem check`, `kalem fmt` and `kalem export` call the mode's hooks, so a plugin mode works from the command line and in CI.

**The standard way** is more than the API: a template repository with a mode skeleton and tests, a conformance suite every mode runs (byte-exact round trip, incremental equals full parse, snapshots in both frontends, the budget), the page "Writing a mode" in the plugin documentation, and two reference plugins, one declarative and one programmatic (work breakdown: T2.7c.10, T3.1.9g, T3.3.1, T3.3.4, T3.3.6).

### 11.12 Completers

The third thing a plugin adds for a file type, next to a highlighter and a renderer (11.11), is a **completer** (asked by the owner, 2026-09-28). One contract serves very different sources: the words of the document and of a dictionary in prose, a language server in code, later a model. `kalem::completers::register(spec)`:

| Part | What the completer gives | What the core does |
|---|---|---|
| `when` | A scope as commands have (11.2): the text types it serves (`python` in a file or in a source block, `org`, `all`), and a when-clause for the rest: the document's language (`docLanguage == tr`), inside or outside prose | Runs only the completers that apply where the cursor is |
| `triggers` | Trigger characters (`[[`, `#+`, `@`), a word prefix of N letters, or on request only (Complete: Ctrl+Space, or Alt+/) | Opens the menu, keeps it updated as the user types, closes it on Escape or a key that matches nothing |
| `complete(ctx)` | Items, asynchronously and cancellable, from `ctx`: the prefix, the text before the cursor in the line and in the paragraph, the syntax node at the cursor from the mode's tree (in a link, in a table cell, in a source block of a language), the document's language and path | Merges the items of every completer that applies, ranks them (exact prefix first, then recently accepted, then the completer's priority), removes duplicates, shows one menu in both frontends |
| Items | A label, what to insert (text, or edits with ranges, with the cursor's place), a kind (word, keyword, link, tag, symbol, snippet), a detail line, an optional `resolve` for documentation fetched lazily | Applies the insertion as one undo step; shows the detail and the resolved documentation |
| `hover` (optional) | Text for the thing under the cursor | The hover card of both frontends |

Rules:

- **Never blocking.** A completer that misses its budget (11.6) shows nothing for that keystroke and the menu keeps the items of the others; results arrive as they come, as Search in Project does (2.8).
- **Built-ins on the same contract.** The Org completions of today (`#+` keywords and blocks, `[[` link targets, `[fn:` labels, tags; `kalem_core::input`) become completers, and so do the two every text file gets: the **words of the document** (dabbrev-style, from the first letters, no configuration) and the **dictionary** of the document's language, from the Hunspell word lists the spell checker loads (2.2), with a frequency list where one exists so that common words come first. The language comes from `#+LANGUAGE`, the setting, or detection.
- **Code.** In a source block or a file of a programming language, the completer is the core's language server client (11.14, D57) fed by a language plugin that declares the server; the plugin is installed by the user from `getkalem/plugins`, never bundled, and the core knows no language.
- **Models.** A completer may call a model through `kalem::net`, off by default, enabled per workspace through the permission model (11.6) with a visible indicator; document text leaves the machine only after that consent. Phase 4.
- **Batch.** `kalem complete FILE:LINE:COL` prints the items, for tests and scripts; a completer plugin's conformance suite checks its items on fixture files, its cancellation and its budget.

**The standard way**, as for modes (11.11): the contract in `kalem-core` first, the Org completers and the document-words completer on it in phase 2, the dictionary completer with spell checking in phase 3, then the WIT binding, a template, the page "Writing a completer", and two reference plugins, a word list (declarative) and the LSP bridge (programmatic) (work breakdown: T2.7a.8, T3.1.9c, T3.3.2, T3.6.1, T4.3.6b).

---

### 11.13 Files that are not text: viewers and editors from plugins

Kalem's aim is to open every file a click in the file manager lands on (asked by the owner, 2026-09-30). Text files open in a document mode (2.6, 11.11). Every other file opens through a plugin implementing the `document-viewer` contract and, where the plugin can write the format faithfully, the `document-editor` contract on top of it (D54). PDF, Word, Excel, PowerPoint, images and SQLite databases (a schema tree, a grid over the tables and a SQL editor, every grid edit shown as the statement it runs) are such plugins in `getkalem/plugins` (11.8); none is in the core. The core carries the contract, the two frontends' views of it (pages, grids, slides and images in the GUI; the same as terminal images or as extracted text in the terminal, principle 7) and the fallback when no plugin matches: the file's kind, the plugin that would open it, and the system application.

**The contract.** `detect` (extensions and magic bytes); `open` on a host file handle, read lazily; `structure` (the units: pages, sheets, slides, frames; the outline; labels); `render(unit, scale, theme)` returning a bitmap, a block tree or a display list that the host paints, never drawing itself; `text(unit)` with ranges for search, copy and the terminal; `search`, `links`, `close`. The editor half: `edits(unit)` listing the commands the format allows at a place, `apply(edit)` returning the changed units, `save` as a byte stream the host writes atomically, and a loss report. Decoders are pure Rust compiled to WebAssembly components and sandboxed (D28); C libraries belong to the compile-your-own tier only.

**Opened as itself (D55).** Kalem respects every format and never converts a file to another format in order to open or edit it: a Word file is edited as WordprocessingML, an Excel file as SpreadsheetML, a PDF as PDF, and no dialog asks to convert it to `.klm`. The three rules of the standard modes (2.6; the Book, Part II) apply to packaged and binary formats: *parts, never the package* (an edit rewrites only the part or object it touches; every other entry is copied byte for byte, so open and save without edits is the identity), *no extension* (nothing written that the format's specification does not define), *unknown constructs stay visible* (a placeholder naming what it is, kept in the file). Conversion is an explicit export command, as for every format. The specifications followed are ECMA-376 for the Office formats and ISO 32000-2 for PDF; each plugin has its chapter in Part IV of the Book, in the skeleton every format chapter shares (D53). The work is group 3.7 of the work breakdown.

### 11.14 Language plugins and language servers

A language plugin makes Kalem a complete editor for a programming language (asked by the owner, 2026-09-30; Python, Elixir, one plugin for HTML, CSS and JavaScript, PHP, Go, Rust and one plugin for C and C++ first, group 3.8) without making it an IDE (1.4): a highlighter (11.11), the language pack hooks (2.6) and the whole of what the language's server offers, from completion with snippets to rename across files, semantic tokens and inlay hints, in both frontends.

**One client in the core, the languages in the plugins (D57, owner, 2026-09-30).** `kalem-lsp` is the single language server client of Kalem; it implements the protocol once and runs one server process per language per project root: process lifecycle, JSON-RPC over stdio, position encoding mapped to Kalem's offsets, incremental synchronization from the transaction stack, workspace edits as one undo step, cancellation, restart on crash, logs. It knows no language. A language plugin declares, in its manifest: file types, the highlighter, comment and indentation rules, the server or servers, how to find one (a project-local install, the PATH, a Kalem-managed install with the user's consent, never a silent download), root markers, settings, a formatter, run and test commands, snippets; optional Rust code covers what a manifest cannot (virtual environments, Mix). A declarative plugin has no code, so the third language costs an afternoon. The features bind to the same contracts every mode uses (completers 11.12, hover, diagnostics, outline, format), so the keys of the Doom `SPC c` map and their Emacs and Word-like equivalents work the same in every language.

## 12. Babel: source blocks

- **Syntax:** `#+BEGIN_SRC lang :header args`, `#+CALL:`, `src_lang{...}`, `#+RESULTS:` blocks.
- **Header arguments:** `:results` (output, value; raw, table, list, verbatim, file, drawer; replace, append, prepend, silent), `:exports` (code, results, both, none), `:var`, `:dir`, `:cache`, `:tangle`, `:file`; `:session` and `:noweb` in phase 4.
- **Executors:** shell (sh, bash, zsh), python, javascript (node), R, gnuplot, sqlite, org, simple calc-like arithmetic. Plugins add languages with `kalem::babel::register_language`.
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

The user's files (`settings.toml`, `keymap.json`, `projects.toml`) live in `$KALEM_CONFIG_DIR`, or in `kalem` under `$XDG_CONFIG_HOME`, `%APPDATA%` on Windows, or `~/.config`. The workspace file is `.kalem/settings.toml` in the document's directory or the nearest ancestor that has one. Every value is checked; a wrong one is reported and the layer below applies (D9). Logs go to `kalem.log` in the state directory (`$KALEM_STATE_DIR`, or `kalem` under `$XDG_STATE_HOME`, `%LOCALAPPDATA%` on Windows, or `~/.local/state`); `KALEM_LOG` or `log.level` sets the level.

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
| kalem-script | API contract tests; the published Rust bindings against the WIT world; time and memory limits |
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
- Documentation: the Book (design_doc2.md, section 10), one source of truth on GitHub Pages: the manual, the standard formats as implemented, the Kalem format's specification (Part III, its home from draft 0.2 on), the formats plugins open, the plugin API and the design; every format Kalem opens has its chapter there, in one skeleton, changed with the code in the same pull request (D53); built by Kalem's own exporter, mdBook only as a bridge; written in Org until the Kalem format lands, then in `.klm`.
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
| Low adoption | Medium | High | Positioning by D21: standard formats opened as themselves and never rewritten, a claim no other editor makes and one that is measured; early access for P1, P3 and P4; announcements to the Org, LaTeX and plain-text communities |
| External tool dependencies (pandoc, TeX) tire users | Medium | Low | Detection and guidance; built-in HTML and Markdown; tectonic as an optional download |
| Two frontends double the UI work | Medium | Medium | All behavior lives in `kalem-core`; frontends only render and translate input; shared syntax highlighting and widget tree |
| Plugin security holes | Low | High | Permission model; sandbox; time and memory limits; security policy |

---

## 20. Roadmap

Durations are rough estimates for a single developer. The next phase does not start before the exit criteria of the current one are met.

Two tracks run beside the phases below (design_doc2.md, section 11; owner, 2026-09-30): **Track B, the Kalem format** (K0 specification and prototypes, K1 parser, model and editing, K2 styles, layout and export, K3 conversion and specification 1.0; work breakdown group 2.13), which never runs ahead of the standard modes; and **Track C, the Book** (group 2.10), started now.

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

- `kalem-script`: the WASM host, the WIT API and generated bindings, plugin loader, permissions, inspection; batch mode `kalem run`.
- Example plugins and a template repository.
- `org-babel`: shell, python, js, gnuplot; trust model; tangling.
- `org-agenda`: workspace, agenda views, capture, refile, clock.
- Spell checking, reveal.js and Beamer export.

**Exit:** three community plugins; the agenda in daily use; a gnuplot chart produced in a document through Babel.

### Phase 4: Maturity

- Out-of-process protocol; the compiled distribution (11.1).
- Babel `:session` and `:noweb`; column view; org-habit.
- Presentation mode; embedded PDF preview; automatic updates.
- Accessibility improvements; performance tuning; 1.0.

---

## 21. Open decisions

| ID | Decision | Options | Recommendation | Status |
|---|---|---|---|---|
| D1 | License | MIT OR Apache-2.0; GPL-3.0; mixed | MIT OR Apache-2.0 everywhere | **Decided:** MIT OR Apache-2.0 (18.1) |
| D2 | Parser foundation | orgize dependency; orgize fork; new parser | After evaluation | **Decided:** new parser following org-element (5.6, `book/part-5/decisions/D2-parser-foundation.org`) |
| D3 | UI framework | gpui; Tauri + ProseMirror; iced/floem | gpui, validated by the spike | **Decided:** gpui, with Kalem's own inline layout (7.1, `book/part-5/decisions/D3-ui-framework.org`) |
| D4 | Math engine | mitex + typst; ReX; RaTeX; KaTeX | After the corpus comparison | **Decided:** RaTeX (9.2, `book/part-5/decisions/D4-math-engine.org`) |
| D5 | tectonic | Bundle; separate download; system TeX only | Separate download | Open |
| D6 | API definition source | Rust macros; a separate IDL; hand-written declarations | A single definition, bindings generated | **Decided (D28, 2026-09-28):** WIT is the single definition; the Rust bindings are generated from it and published as `kalem-plugin` |
| D7 | Project name | – | – | **Decided:** Kalem; crate `kalem-editor`, binary `kalem`, GitHub `kalem-editor` (section 0) |
| D8 | Agenda index storage | In memory; SQLite; custom file | In memory, disk cache later | Open |
| D9 | Configuration formats | TOML and a script file; a script file only; JSON | TOML, a script file and keymap.json | **Decided:** `settings.toml`, `keymap.json` (comments allowed); no script file (D28, 2026-09-28), automation is a plugin (14, `book/part-5/decisions/D9-configuration-formats.org`) |
| D10 | A second scripting language | Phase 3; phase 4; never | Never, for now | **Closed (owner, 2026-09-28):** no scripting language ships; reopening needs an RFC |
| D11 | Webviews in plugin panels | Never; optional | Never; JSON widget tree | **Decided (owner, 2026-10-04):** no webview; a widget tree in typed WIT (a flat list, children by index), rendered by both frontends (`book/part-5/decisions/D11-plugin-panels.org`) |
| D12 | Multiple documents | One window one document; tabs; multiple windows | Tabs, phase 2 | **Decided (owner, 2026-09-28):** one window holds many documents, listed on the left or as tabs at the top, grouped by project (2.8) |
| D13 | Time library | jiff; chrono | jiff | **Decided:** jiff; date arithmetic follows Emacs's `encode-time` normalization on top of it (`org-model::time`) |
| D14 | Terminal UI stack | ratatui + crossterm; termwiz; custom | ratatui + crossterm, ratatui-image for graphics | **Decided:** ratatui + crossterm + ratatui-image (7.6, `book/part-5/decisions/D14-terminal-ui-stack.org`) |
| D15 | When to spin out ecosystem crates | From the start; when stable (4.7) | When the API is stable, per 4.7 | **Decided:** incubate in the monorepo, spin out when stable (4.7) |
| D16 | Syntax highlighting engine | syntect (Sublime syntax definitions, pure Rust with fancy-regex); tree-sitter (incremental, structural, C grammars) | syntect first for breadth and Sublime compatibility; tree-sitter later for structure-aware features | **Decided:** syntect with `regex-fancy`, in `kalem-highlight` (`book/part-5/decisions/D16-syntax-highlighting.org`) |
| D17 | Vim mode engine | Own engine in kalem-core; reuse an existing crate; embed Neovim | Own engine, spun out if it proves reusable (4.7); Neovim embedding rejected for size and dependency reasons | **Decided:** own engine, `kalem_core::vim`; the Vim profile replaces the Emacs Org profile (owner, 2026-09-28) |
| D18 | Entity table provenance | Keep with attribution; split (names and UTF-8 in `org-syntax`, export renderings elsewhere); ask the Org maintainers and the FSF; GPL for `org-syntax` | Split now, ask in parallel (`book/part-5/decisions/D18-entity-table-provenance.org`) | Open: owner decision, blocks publishing `org-syntax` |
| D19 | Markdown parser | pulldown-cmark (offset iterator); comrak (AST with source positions); tree-sitter-markdown; own parser | comrak: GitHub's own rules (a port of `cmark-gfm`), all of GFM; its position defects fixed in Kalem's fork (`spikes/md-parser`: both 639/648 CommonMark, comrak 24/24 GFM against 12/24, a few positions outside their parent, four times slower than pulldown-cmark) | **Decided (owner, 2026-10-01):** comrak in the fork `getkalem/comrak`, fixes offered upstream |
| D20 | File operations for the file manager (2.7) | `trash` crate plus std::fs with own copy, move and progress; `fs_extra`; shelling out to system tools | `trash` for deletion, own operations on std::fs for progress, cancellation and conflict handling | Decided 2026-09-28 (book/part-5/decisions/D20-file-operations.org) |
| D21 | Product positioning | Org editor first; Markdown editor too; a light Office replacement (fonts, colors, spreadsheet notation) | "Kalem edits plain-text files as they look, and keeps them plain text, byte for byte": Org first, Markdown next, the Word-like additions only in `.klm` (D24); the README, the launch and the order of phase 2 follow it | **Closed (owner, 2026-09-30):** faithful standard modes plus the Kalem format; four use cases: notes and tasks, documentation, scientific writing, printed documents (design_doc2.md) |
| D22 | PDF without TeX | System print to PDF; a bundled HTML renderer; typst | Decide with T2.3.13, after the HTML page template exists | Open (review, 2026-09-28) |
| D23 | UI framework revisited | Stay on gpui through a registry snapshot or vendoring; leave gpui | A registry snapshot first (`gpui-unofficial` or `gpui-pre`, T2.8.6a); the spike of T2.8.7 only if the snapshot lines fail twice or Zed's terms change | Open: owner decision (review, 2026-09-28) |
| D24 | File kinds | One `.org` that may carry Kalem's additions; `.org` strict and `.klm` a superset | `.org` is strict Org; `.klm` is the Kalem format of RFC 0003 | `.org` strict **decided**; the `.klm` half **superseded by RFC 0003** (owner, 2026-09-30); Kalem's additions removed from Org now, ahead of the Kalem format (owner, 2026-09-30; T2.13.13) |
| D25 | Plugin-provided highlighters, renderers and completers | Separate plugin APIs; the contracts built-in modes and completers use | One contract each, shared by built-ins and plugins, with a declarative and a programmatic level, a conformance suite and reference plugins (11.11, 11.12) | **Decided (owner, 2026-09-28)** |
| D26 | Terminal parity | The terminal as a reduced frontend; the terminal never second class | Principle 7 of 4.1: a feature is done when it works in both frontends, gaps listed in `book/part-5/terminal-parity.org` | **Decided (owner, 2026-09-28)** |
| D27 | Command scope | Keys for mode, language and file kind; one axis | One axis, the type of the text at the cursor, nesting by the innermost type, `klm` a subtype of `org`; structure stays in `when` (11.2) | **Decided (owner, 2026-09-28)** |
| D28 | Plugin ABI and language | A scripting engine embedded natively with WASM later; WASM components with a scripting runtime; WASM components with Rust as the only language | WASM components on a WIT-defined API; Rust is the plugin language, on the same traits the core uses, so one crate builds as a bundled plugin inside the binary or as a sandboxed component; no scripting engine ships (D10); power users may compile community plugins into their own Kalem; out-of-process JSON-RPC for language servers and external tools; the engine (wasmtime or wasmi) by the spike T3.1.0 | **Decided (owner, 2026-09-28)**, engine open |
| D29 | Small core | Everything built in; a small core with bundled plugins | The core: Org, Markdown, CSV and LaTeX (9.5), the Kalem format (RFC 0003), the text engine and view model, the two frontends, the infrastructure that runs before plugins; everything else a plugin, the expected ones bundled as embedded WASM components (11.0) | **Decided (owner, 2026-09-28)**, LaTeX added to the core the same day; new modes and file types live in `getkalem/plugins` (11.8) |
| D31 to D46 | The Kalem format's syntax and stylesheet decisions | See RFC 0003 | One command syntax, paragraphs by blank lines, `$…$` as the only shortcut, Djot-style attributes, `\props` for planning and properties, spreadsheet-style column formulas, TOML stylesheets, layout in the stylesheet with `\pagesetup` inline | **Decided (owner, 2026-09-30)** in RFC 0003 draft 0.2 |
| D47 to D52 | The command sigil `\`, implicit paragraphs, the single shortcut, the formula dialect, TOML stylesheets, Typst then LaTeX as PDF engines | See RFC 0003 | As RFC 0003 §4, §8, §9, §13, §17 | **Decided (owner, 2026-09-30)** |
| D53 | Specifications in the Book | Chapters where convenient; one skeleton for every format chapter, changed with the code | Org, Markdown, CSV and LaTeX as their standards define them and `.klm` as Kalem's own; Part II, Part III (the Kalem format's home from draft 0.2 on, RFC 0003 frozen as the record) and Part IV for the formats plugins open; a chapter changes in the same pull request as the code | **Decided (owner, 2026-09-30)** |
| D54 | Files that are not text | Refuse them; convert them into a document mode; a viewer and editor contract for plugins | The `document-viewer` and `document-editor` contract (11.13); PDF, Word, Excel, PowerPoint and images as plugins from `getkalem/plugins`, never core; pure-Rust decoders in the sandbox | **Decided (owner, 2026-09-30)** |
| D55 | Opened as itself | Convert on open, as LibreOffice offers; open and edit every format as its own specification says | No conversion in order to open or edit, no conversion prompt; edits rewrite only the part they touch; conversion only as an explicit export | **Decided (owner, 2026-09-30)** |
| D56 | PDF rasterizer and spreadsheet formula engine for the plugins | hayro or a pdf-rs based rasterizer; IronCalc or cached values only | Spikes on corpora with pdfium and LibreOffice as oracles | Open (T3.7.3a, T3.7.4a) |
| D57 | Where the language server client lives | A plugin (the "LSP bridge"); one client in the core as infrastructure with language plugins declaring the servers | One client in the core (`kalem-lsp`, 11.14): protocol plumbing shared by every language, tested once, no language knowledge in it | **Decided (owner, 2026-09-30)**: one client in the core, the plugins declare |
| D58 | Default language servers for the first language plugins | Python: basedpyright, pyright, pylsp, ty, jedi; ruff beside it. Elixir: Expert, ElixirLS, Lexical. Web: `vscode-langservers-extracted` and `typescript-language-server` with ESLint; Biome; Deno. PHP: Intelephense, phpactor. Go: gopls. Rust: rust-analyzer. C and C++: clangd, ccls | basedpyright with ruff; Expert with ElixirLS as the fallback; the VS Code servers and typescript-language-server with ESLint, HTML, CSS and JavaScript in one `web` plugin; Intelephense with phpactor as the alternative; gopls; rust-analyzer; clangd with ccls as the alternative | Proposed, confirmed by the CI corpus (T3.8.5, T3.8.6, T3.8.6a to T3.8.6e) |
| D59 | Formulas in CSV files | None that persist (a calculator writing values on command); persisted formulas in a Kalem sidecar beside the file | The calculator only: a sidecar is hidden state, and formulas that persist belong to the Kalem format and to Org tables | Open (owner; T2.7d.9) |
| D60 | The SQLite engine of the `sqlite` plugin | SQLite's amalgamation compiled into the component; Turso/Limbo (Rust); the `sqlite3` process | The amalgamation, sandboxed, the one exception to the no-C rule for plugins, because SQLite is the only correct writer of its format | **Decided in planning (2026-09-30)**; Turso revisited when it passes SQLite's tests (T3.7.6a) |
| D61 | Where the Book is published | `getkalem/kalem` public; a public `getkalem/getkalem.github.io` fed by the private repository's workflow; a paid plan | The public site repository now, `kalem` public when the owner opens the project | Open (owner; T2.10.12) |

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
