# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed
- Search Lines (Doom's `SPC s b`) did nothing in a file manager listing, in both editors. A listing is now searched like any document: the lines are its entries, so the search finds a file, the cursor following the entry while typing; Enter stays on it and `l` or Enter opens the file. Across the open documents (`SPC s B`), the listings' lines take part too.

## [0.6.3] - 2026-10-10

### Added
- Formatting in documents of flowing text (a Word document), as a word processor's: Bold (Ctrl+B), Italic (Ctrl+I), Underline (Ctrl+U), Strike Through, Superscript and Subscript turn a mark on, or off where the whole selection has it; Font, Font Size, Text Color and Highlight Color offer lists (the document's typefaces and common ones, common sizes, a word processor's colors) or a value typed; Paragraph Style gives the paragraphs one of the document's styles; Clear Formatting gives the text back its style's look. They act on the selection, or the word at the cursor, and go through the plugin API's `flow` interface (`set-marks`, `set-style`). They are in the Format menu, and a new Review menu holds the comments' and the tracked changes' commands. The graphical editor's toolbar shows, for such a document, the paragraph's style, the typeface and the size at the cursor, each opening its list, the marks pressed where the text has them, the colors, Clear Formatting and New Comment.

## [0.6.2] - 2026-10-10

### Added
- Edit Comment (Review): in a document of flowing text, changes the text of the comment at the cursor; the palette starts with its text, a paragraph a line (shown as ↵). It goes through the `annotations` interface's `set-text`, which the docx plugin writes as Word does.

### Fixed
- A viewer turned off after stopping three times gave way to nothing until Kalem was started again: a newer copy of the plugin installed meanwhile (the workbook plugin's, say) opened no file, and every workbook said there was no workbook viewer. The viewers are now chosen again while Kalem runs: at the third stop the files go at once to a newer copy installed, else to the copy built in, else to the native viewer; a viewer installed, updated or removed from the plugin list or with `kalem plugin install` in a terminal, and `kalem plugin enable`, take effect within a second or two, and the status bar says which copy opens the files now. At startup a copy turned off is mentioned only when nothing opens its files in its place.

## [0.6.1] - 2026-10-09

### Fixed
- Spreadsheets: a word typed into a cell of a new workbook (New Workbook) and Enter stopped the workbook viewer, which closed the workbook with what was typed. Typing into new rows of any workbook no longer reads the sheet again at each Enter, and text that is not ASCII after the typed cell could stop the viewer the same way (xlsx 0.0.9).

## [0.6.0] - 2026-10-09

### Added
- The plugin API is 0.2.7: a viewer of documents of flowing text (word processing documents, e-books, web pages, e-mail) gives its paragraphs through the `flow` interface instead of rendering pages, and Kalem lays them out itself in both editors, as a document of the editor whose text is the paragraphs': styles, sizes in proportion to the body text's, list labels, tables (a row of one-paragraph cells a line), notes, headers and breaks shown with the plugin's look; typing, Enter, Backspace and deleting across paragraphs become the plugin's edits, which it writes in its own format or refuses, saying why; undo and redo are the plugin's. `kalem-plugin`'s feature `flow` exports it from the Rust contract (the `flow-viewer` world). For the docx plugin of getkalem/plugins.
- Comments and tracked changes (the `annotations` interface of API 0.2.7, which every viewer world exports): text a comment is on is highlighted, inserted text underlined and deleted text struck through in their colors, and the status bar says the comment or the change at the cursor. The Review commands add (Ctrl+Alt+M), answer, resolve and delete comments, go to the next and previous comment or change, accept or reject a change or all of them, and turn Track Changes on and off. The setting `user.name` is the name they carry (the system account's when empty).

### Fixed
- The graphical editor: going up from the end of a long file, the text scrolled under a cursor stuck on the bottom row until the file's start was reached (since the CSV grid's cursor work, every move marked the cursor's line for measuring again, and the editor took a line without measurements for one far away). The window now stays where it is while the cursor's line is wholly in it and scrolls by one line when the cursor leaves it; typing and a new line in the middle leave it in place too.

## [0.5.2] - 2026-10-09

### Added
- A plugin's manifest may add buttons (`"buttons"`: a title, one of the plugin's commands, a when-clause on the document), beside the File Manager and Projects buttons on the window's toolbar and in the terminal's list of open files, where the when-clause holds; a plugin's button is pressed while one of its documents is shown. The git plugin's Git button opens its status, as `SPC g g` does, in a repository.

### Changed
- The `vcs` when-clause key of a document without a file (a new document, a plugin's document, the projects view) is the first project's folder's, where a plugin's programs run for it: the git plugin's Git menu and button show there when that folder is a repository.
- A link in a PDF that leaves the document (a web address, a file, another program) asks before it opens outside Kalem, showing where it goes: a click no longer starts a program at once.

### Fixed
- Windows: moving or renaming a file (in the file manager, or this file's own) that another program had open for a moment (a virus scanner reading a file just written, an indexer, a backup) failed at once with "access denied" or a sharing violation. The move now tries again for up to a second before it reports the error.

## [0.5.1] - 2026-10-08

### Added
- The `diff` highlighter colors its lines: added lines green, removed lines red, a hunk's `@@` line as a heading, the file headers dimmed, in both editors; the added, removed and hunk lines have a wash of their color behind them too, as magit shows a diff. The theme's `[syntax]` table gains `inserted` and `deleted` for the two. The git plugin's status and commit views, highlighted as `diff`, show their hunks in these colors.
- Lines the git plugin marks as added or changed are tinted under their text, beside their mark in the gutter, in both editors (green for added, orange for changed); `editor.highlight_changes` turns it off.

### Fixed
- The graphical editor's margin: line numbers, the fold arrows of headings and the plugins' change marks, painted left of the lines, had been clipped away since the move to gpui from crates.io (its list clips what it paints to its bounds). Each line now carries the margin as its own padding, the number left of the arrow and the mark; an Org document shows no line numbers, as in the terminal editor.

## [0.5.0] - 2026-10-08

### Added
- The plugin API is 0.2.6: a plugin's document may be styled (`styled-documents`), stretches of its text in a color and bold, italic or underlined, written with the text. Colors are named (red, green, yellow, blue, magenta, cyan, muted, accent) and take each theme's shade, so the document reads in the light and the dark theme; a terminal shows the theme's shade with full colors, its own named colors otherwise. For the git plugin's status, colored as lazygit is.

### Fixed
- LaTeX: an edit between a `\def` and the name it defines across blank lines (`\def`, paragraphs, `\ee`) changed what the document defines, and the edited paragraph's quick reparse kept the old definitions, so `\be … \ee` equations could parse as they no longer should until the next full parse. A quick reparse now checks the document's definitions and parses it whole when they changed. (Found by the fuzz tests.)
- LaTeX: the view showed the blanks and `\space`s a `\textcolor`'s text starts with (`\textcolor{red}{\space x}`), which LaTeX drops, as the text starts after xcolor's `\ignorespaces`. (Found by the typeset fuzz test.)

## [0.4.2] - 2026-10-08

### Fixed
- A plugin's keys (the git plugin's `SPC g g`) were not bound from the second start on: a plugin loaded from the compiled components' cache registers them at once, before the editor had counted the plugins' registrations, so the editor never saw them. Both editors count them before building their keymap.
- A file opened by a relative path (`kalem notes.org`) kept that path, so a plugin was given `notes.org` and the git plugin said "`.` is not an absolute path"; an opened document's path is absolute.

## [0.4.1] - 2026-10-08

### Added
- A plugin's manifest may add menus to the menu bar (`"menus"`: a title, a when-clause on the document, the plugin's commands), in the window and in the list F10 shows in both editors. A when-clause may name `vcs`, the version control holding the document's file (`git`, `hg`, `jj`, `svn`…, by the nearest folder above it with its marker): the git plugin's Git menu shows in a repository only, its Status the same as `SPC g g`.

## [0.4.0] - 2026-10-08

### Added
- The plugin API is 0.2.5: an extension plugin may show documents of its own through the `documents` interface, read-only text the editor shows in a tab as it shows the file manager's listing, with a title, a highlighter (`diff`) and a kind (`git-status`) that the plugin's commands and keys are scoped to; the plugin writes it again as it changes, the view keeping its place and the cursor its line. It has no file to save, no line numbers and no indentation guides, and once the user closes it the plugin's next write is refused. A plugin's keys in its own documents come before the profile's and Vim's, so that Tab, Enter, the arrows and single letters there are the plugin's, in Vim's command mode and in the Word-like profile. A plugin built against 0.2.4 still binds. For the git plugin's status of getkalem/plugins.
- Marks beside the lines (plugin API 0.2.5, `decorations`): a plugin marks the added, changed and removed lines of a file, and both editors draw them in the gutter, a colored bar beside the line (in the terminal `▎`, `▁` for removed lines; `+`, `~` and `_` without Unicode); they move with the edits made since, until the plugin sets them again. Next Change and Previous Change (Doom's `SPC g ]` and `SPC g [`, Alt+F5 and Shift+Alt+F5) go from one change to the next. The git plugin's diff of a file against the index.
- Open File takes a line and a column (`file.open` with `{"path": …, "line": 12}`), as a plugin opens a diff's line.

### Fixed
- A plugin's keys under the leader (the git plugin's `SPC g g`) were bound in every mode, so in Vim's insert mode Space opened the key hints instead of typing a space. They apply in Vim's command mode only, as Kalem's own leader keys do, and follow `editor.vim.leader` when it is not Space.

## [0.3.0] - 2026-10-07

### Added
- The plugin API is 0.2.4: an extension plugin may run the programs its manifest names (`subprocess:git`) through the `process` interface, without a shell or a terminal, in a project's folder, the run answered later as a fetch is; the user's setting `programs.NAME` of the plugin says where a program is when it is not on the PATH, and the installer names the programs ("Runs programs on this computer: git"). For the git plugin of getkalem/plugins.

### Fixed
- The palette's questions in the window: a long one (Install from a GitHub link's, in Turkish above all) ran out of the palette and pushed what was typed, and the cursor, out of sight; the question now stands on its own line and wraps, and the typed text wraps under it. A link pasted with Cmd+V (Ctrl+V), or with Edit > Paste, goes into the palette; it went nowhere.
- Workbooks on the dark theme: a cell's automatic text color, which Excel writes as black, was drawn black on the dark background and could not be read. Black text on an unfilled cell is drawn in the theme's foreground (white text on the light theme likewise); the terminal editor leaves it the terminal's own color, as it does the borders.
- Opening a new file in a folder that does not exist (`SPC .`, `SPC f f` or Open File with a typed path, from the file manager too: `new/a.txt`) offers to make the folder, as Doom Emacs does; the file opened, and saving it then failed with "No such file or directory".
- The terminal editor's Save As to a folder that does not exist (`new/b.org`) asks to make it, as Emacs does, and so does Save of a file whose folder is not there (`kalem tui new/a.txt`, or the folder deleted since); both failed with "No such file or directory".
- The first start after an update, or after a language plugin changed, no longer waits about a second before showing anything (`kalem tui` took 1.1 s with the Elixir plugin installed, later starts 0.04 s): the plugins' syntaxes, when not cached yet, are built on a thread of their own while the editor starts; only a file in a plugin's language waits for its colors.

## [0.2.1] - 2026-10-07

### Fixed
- CSV: with cells of several rows or columns selected, Enter and Tab move the active cell within them (down each column, along each row, wrapping), as Excel does, the cells staying selected while typing into each; they left the selection.
- The terminal editor no longer waits half a second at every start in a terminal that answers some of its questions and not the last (iTerm2 started some 0.4 s later than the graphical editor): once the terminal has begun to answer, a short silence ends the wait. `kalem tui --detect` and the log say how long the questions took.

## [0.2.0] - 2026-10-07

### Added
- A settings panel in both editors (Settings: Ctrl+, or Cmd+,, Alt+, in a terminal that cannot send Ctrl+,, `SPC h v`, the Kalem menu): every setting grouped by its table, changed in place with lazygit-like keys (`j`/`k` choose, `h`/`l` or Space change, Enter types a text, `/` filters, `d` back to the default, `e` opens `settings.toml`), and lists and tables item by item (Space puts a choice in or out; Enter edits, `a` adds, `x` removes, `J`/`K` move a text; a table's entries typed as `path = mode`), so that no setting needs `settings.toml`. A text steps through its usual values with `h`/`l` or the arrows instead of being typed: the fonts installed, the TeX engines, search sites, `LOGBOOK`… In the graphical editor it takes the place of the panel of six settings, and the usual values holding what is typed are offered under it. Installed plugins (under `plugins`) lists the plugins installed, each with a page of its settings (a language plugin's server, the settings its manifest describes under `"settings"`, its other keys typed as JSON) and actions (update, its folder, remove).
- More of Doom Emacs's leader keys with Vim keys, checked against Doom's own map: `SPC b -` (narrow or widen), `SPC b C`, `SPC b I`, `SPC c S` (the outline), `SPC p X`, `SPC p &`, `SPC s O`, `SPC n F` (Browse Notes, new), `SPC o o` and `SPC o O` (Show in System File Manager, new: Finder on macOS), `SPC o i`, `SPC o I`, `SPC TAB D`, `SPC TAB R`, `SPC TAB 0`, the window map's `SPC w S`, `V`, `W`, `C-h`/`C-j`/`C-k`/`C-l`, `C-w`, `C-o`, `C-u` and `C-r`, and `SPC h c`, `o`, `V`, `O`, `p` (the installed plugins), `b t`, `b m`, `r t`, `r f`; in Org `SPC m @` (cite), `SPC m ,`, `SPC m +`, `SPC m g G`, `SPC m c E`, `SPC m b i H`, and `SPC m h`, `SPC m *`, `SPC m i` and `SPC m l d` (Toggle Heading, Toggle Item, Remove Link); in Markdown `SPC m i e`, `SPC m i s`, `SPC m t x`; in LaTeX `SPC m ;` (the outline).
- A test presses every leader key the Doom tables bind, in the terminal editor in a project, and checks that it runs its command and that the command applies there.
- CSV: Line Break in Cell (Alt+Enter, as in Excel; Ctrl+J in a terminal) puts a line break into a cell's value, which no key did; the Frequency Table's choice filters on that column's value (an empty one too); Sort File by Columns takes the header's names.
- Markdown: Next Row (`markdown.table.nextRow`) aligns the table and goes to the same column a row down, adding a row past the last, as Enter in an Org table; it has no key (Enter stays a line break for typing rows), and `keymap.json` can bind it to Enter in tables.
- Org: Insert Heading (Alt+Enter outside lists, tables and blocks: Emacs's `M-RET`), Insert Heading After Subtree (`C-RET`), Insert Subheading and Insert TODO Heading, which Kalem did not have: Alt+Enter on a heading broke its line. 350 cases compare them with Emacs.
- Org: Shift with the arrows changes the date under the cursor, as `org-shiftup` and the others: Shift+Up and Shift+Down the year, month, day, hour, the minutes by five, a repeater's number or unit, and on a bracket active and inactive; Shift+Right and Shift+Left a day. A `CLOCK:` line's duration follows; with a selection Shift extends it. 1,965 cases compare them with Emacs.
- Org: Toggle Heading, Toggle Item (Emacs's `org-toggle-heading` and `org-toggle-item`) and Remove Link (Doom's `+org/remove-link`), on Doom's `SPC m h`, `SPC m i` and `SPC m l d`.
- Vim keys in Org, as Doom Emacs and evil-org have them: `za`, `zc`, `zo`, `zO`, `zM`, `zR` and `zA` fold, and a folded heading is one line for `j` and `k` and whole for `dd` and `yy`, as Vim's closed folds; `M-h`, `M-l`, `M-k` and `M-j` promote, demote and move a heading, an item or a table column or row, with Shift the subtree; Enter in Normal mode acts on what is at the cursor (`+org/dwim-at-point`: a TODO done, a checkbox, a link, a footnote, a table); `]h`, `[h`, `gj`, `gk`, `gh` and `gl` move by headings and elements, `]l` and `[l` by links, `]c` and `[c` by source blocks; C-RET and C-S-RET insert an item, a table row or a heading below or above and start Insert mode; C-S-h, C-S-j, C-S-k and C-S-l are Org's Shift with the arrows.
- Org: Set TODO State and Set Priority without an argument (Doom's `SPC m t`) offer the document's keywords and priorities to choose from; before, a prompt asked for one to type with nothing shown.

### Changed
- Projects are added by hand by default: opening a file in a folder under version control no longer adds that folder to the project list. Toggle Adding Projects Automatically (Project menu) or the settings panel turns it back on (`projects.auto_add`).
- Leader keys that did something else than Doom's now do what Doom's do: `SPC q F` closes every document (it quit), `SPC TAB x` is Doom's "kill session" and not there yet (Delete Saved Workspace moved to `SPC TAB D`), `SPC b X` is the scratch document (the project's is `SPC p X`), `SPC o P` reveals the file in the folder tree (the projects view moved to `SPC p P`), `SPC o o` shows the file in Finder (the outline is `SPC c S`); in Org `SPC m n` stores a link (narrowing stays on `SPC m s n`, and `SPC m N` is gone for `SPC m s N`), `SPC m l S` inserts the stored link and `SPC m l i` stores one, `SPC m g r` no longer refiles; in LaTeX `SPC m c` builds (Complete stays on Ctrl+Space), `SPC m p` toggles the formulas' preview and Next Problem moved to `SPC m n`.

### Fixed
- Vim keys, checked against Vim itself with 15,000 generated key sequences (848 of them kept in `tests/vim`): crashes on Turkish and other multibyte letters (case changes, CTRL-T, `1v`); sentences and paragraphs (`(`, `)`, `{`, `}`, `is`, `as`, `ip`, `ap`) ported from Vim; quotes with backslash escapes; search offsets (`/pat/e+1`) and `//`; `:` from visual mode giving `'<,'>`; CTRL-V blocks over tabs; the cursor after undo and redo where Vim puts it; `U` no longer rewriting another line after lines were deleted; 'autoindent' with 'expandtab' making spaces; `:%s/\n//` joining lines; `:g` skipping lines its command deleted; ranges past the last line refused; macros stopping at a command that fails; `.` repeating visual changes, numbered registers (`"1p..`) and changes made with a search; `[(`, `])`, `[[`, `]]`, `]p`, `dv`, `dV`, `d<C-v>`, `g-`, `g+`, CTRL-G u, `:right` and `:center`, which were missing.
- Opening a file in the terminal editor no longer waits for the whole file to be colored, only the lines shown are: 220 ms before the first frame for a 10 KB Markdown file in a release build and 3 s in a debug one, 0.7 s for a 6,000-line Rust file. The set of syntaxes built with the plugins' is no longer built again at every start when two versions of Kalem take turns (an installed one and one built from source), and debug builds color with syntect optimized.
- Speed: footnote previews no longer parse the whole document at every key or cursor move, nor the key contexts walk it or search folders; an edit on many lines (Replace All, line endings changed, many cursors) is applied in one pass (11 s for 100,000 lines); a Markdown, LaTeX or code document open no longer draws the window again at every tick; the palette matches once per input; LaTeX lines hidden away from the cursor are taken out in one pass; source blocks are no longer colored again after an edit elsewhere; workbook filters hiding many rows; two Markdown documents side by side; language servers' diagnostics and formatting edits; typing in long Org titles and sections, brackets, long tag lines and paragraphs with radio links; exports with many footnotes and links (2,000 footnotes to HTML: 209 s to 0.08 s).
- Memory and wakeups: the outline's cache no longer grows at every keystroke (1 GB after 400 in a document of 5,000 headlines under one); the cache of compiled plugins is pruned (unused for 30 days, past 200 MB); a tab closed in the graphical editor is let go; the plugins' clock no longer wakes 100 times a second while no plugin runs; a project's file list is no longer walked again at every save or build.
- Crashes: a table formula with a letter beyond ASCII (`$2="ş"`) or `$0=`; a power such as `9^99999999` held the editor; a .bib file whose text before the first entry starts with a letter beyond ASCII; a Markdown table row wider than its delimiter row; the palette in a terminal a few cells big.
- The terminal editor's which-key panel shows once its delay has passed, without another key; lines scrolled sideways keep their columns after a glyph two cells wide; the palette names the Help, Search, Tasks, Window and Workspace categories (it showed `category-tasks`).
- The test plugins' builds no longer leave 150 MB in the temporary folder for every test run.
- The terminal editor applies a changed setting at once (line numbers, wrapping, line width, centering, where the open files show, the keys and the Vim layer, the theme), from the settings panel, a command such as Toggle Line Numbers, or Reload Settings and Keys; before, only a restart showed it.
- CSV: a value such as `5 kişi` aborted the editor at open and `kalem check` on the file.
- CSV, data changed out of sight: with a filter or a sorted view on, Delete Row, Fill, Delete, Copy, Cut and Paste over a selection changed the hidden rows between its corners and rows the sort showed elsewhere; Delete Column, Cut and Fill on a short record's missing cells took every column from A; Backspace over several cells and then Escape wrote one row over another; typing in Vim's insert mode replaced the cell (Excel's Ready mode); in the terminal editor Down on the last row went to its last cell, where typing replaced the value; F2 on a missing cell typed into the cell before; a paste with a line break into a cell being edited overwrote the cells below; the arrows and Tab went into hidden columns; Replace in Column replaced in a `sep=` line and the header after it; a file of one column got no row from Insert Row, Enter or Tab, and the typing went into the last value; Sort File and Remove Duplicates lost a blank line before a last record without a line ending.
- CSV, wrong results: Fill Series kept no decimals of `0.125` or `1.500`, counted `2026-01-31` on to `2026-01-32`, could not count text down and rounded integers past 2^53; dates and amounts such as `10%` and `$20` sorted as text; Sum Column wrote over the header, used a decimal point in `;` files and counted the rows a filter hides, as the status bar did; Move Row swapped with a hidden row; hidden columns, widths and a sorted view's column stayed by number when columns were inserted, deleted or moved; Vim's `j` and `k` left the column and went onto hidden rows; a header of years (`name,2024,2025`) was taken for data; an Emacs mode line was read as the header; the pinned header row showed a `sep=` line; Widen Column made a narrow column narrower; Copy Cells lost line breaks, tabs and a leading quote; commands that changed nothing left the document modified; the entries of a row were one undo step.
- An edit on every line of a large file and its undo took seconds in the terminal editor (an inserted CSV column and its undo: 168 s for 100,000 rows), and the caret at the end of a grid cell's text was drawn at the cell's edge.
- Markdown: Enter on an empty list item or quoted line ends the list with a blank line after it, so that the text typed next is a paragraph and not a lazy continuation of the item before; on an empty nested item it takes the item out a level, as in Org. Over a selection Enter replaces it and continues the item, in one undo step.
- Markdown: Alt+Up and Alt+Down on a numbered list's later items kept numbering it from the moved item's number (`1. 2. 3.` became `2. 3. 4.`); the list keeps its first number.
- Markdown: Enter in a CR LF file inserted bare line feeds in lists and did not close a code fence; a tab after a list marker is continued as a tab.
- Markdown: Bold, Italic, Code and Strike-through work as Org's: with the cursor or the selection in bold text the bold goes (it put `****` inside it), blanks at the ends of a selection stay outside the markers (`**word **` is not bold), and nothing is wrapped in code, across blocks, or cutting a link in two.
- Markdown: a code span's one space each side (``` `` `x` `` ```) is no longer shown as its text.
- `kalem export FILE.md --to html` titles the page with the front matter's `title` and typesets formulas with MathJax, as the Org export does.
- Word completion no longer offers the word the cursor is typing into (typing `mid` before `2` offered `mid2`, which made `mid22`).
- Org: Set Tags started empty and replaced the heading's tags with what was typed; it starts with them, as in Emacs.
- Org: with `#+STARTUP: overview` the first Shift+Tab left the overview as it was instead of showing the contents; the graphical editor's Shift+Tab had no contents step at all.
- Org: Ctrl+B and the other emphasis keys dropped the selection, so a second press inserted a pair of markers instead of taking the bold away, and Ctrl+B, a word, Ctrl+B left the cursor inside the markers; they work as a word processor's now.
- Org: a stored link to a heading of the same document was inserted with its file (`[[file:notes.org::*Top][Top]]`); as in Emacs it is `[[*Top][Top]]`.
- Recent files that no longer exist leave the list when Kalem saves it, as Doom's recentf cleans up: temporary files that the editors' tests wrote into the user's list before 0.1.0 fixed that filled most of it.
- CSV, as a spreadsheet does: a sorted or filtered view stays as it was worked out until Sort View or Filter Rows is run again (a row typed into jumped away or vanished, an inserted one went to the top), and a keystroke under it no longer reads the whole file; Undo after Escape no longer brings the cancelled entry back; Enter after a run of Tabs goes back to the column they started from; Ctrl+Home and Ctrl+End in Edit mode stay in the cell; Insert Row and Insert Column add as many as are selected; quotes an entry no longer needs go (a space typed and deleted left `""`).
- CSV: an Emacs `Local Variables:` block ending the file is not read as records (sorting moved its lines); `10%`, `$20` and `(5)` count as numbers in the statistics, Sum Column, the alignment and the header's detection, as they did in sorting; a tab in a value no longer pushes the grid's bars out of line in the terminal; a header of two lines is pinned whole.

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
