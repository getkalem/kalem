# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- Rename Symbol (`SPC c r`, F2) offers the name at the cursor to edit, as other editors do, where its prompt was empty.

### Fixed
- The status bar names the most serious problem of the cursor's line, or of those under the cursor: it named the first on the line, so that rust-analyzer's “expected due to this” beside the type stood for the error “mismatched types” after it.
- A document whose edits are undone back to the text it was saved (or opened) with is no longer modified: its title has no mark, closing and quitting do not ask, and Vim's `:q` quits. It stayed modified until saved again. Line endings, a byte order mark or an encoding changed since still count.
- Prompts say what they ask for, in the interface's language and with the forms they take where it helps ("cell (B12 or @12$2)", "date (2026-10-14, +3d)"): 79 of them, the file manager's among them, showed the argument's internal English name, such as `target`, `name` or `path`.
- A table formula's Calc product of many symbolic factors (`vprod` over a column of words) is multiplied, evaluated and written factor by factor: Calc keeps a product nested to the right, and each went a level deeper for each factor, so that a thousand words ran out of a small thread's stack. It is a fifth faster too, a thousand words in 230 ms rather than 290.
- A table formula adding a vector and a multiple of it after another term, such as `x + [1, 2] + (3 mod 5) [1, 2]`, no longer crashes Kalem: Calc took the two for like terms, and adding them left them as they were, so that it went round for ever. They are combined, `x + (4 mod 5) [1, 2]`.
- A table formula's `vpvar` or `vpsdev` of one decimal value is `0.`, as in Emacs, where it was the integer 0, which formulas around it then computed as integers.

## [0.6.24] - 2026-10-14

### Added
- Narrowing in any mode: Narrow to Lines (`view.narrowToLines`) shows only the selected lines, or the lines `start` to `end` a plugin names, in Markdown, code and plain text as in Org, so that an outliner's plugin zooms into a block (the graph plugin's Zoom). In a narrowed document of any mode the cursor stays in the part shown (a motion, Go to Line or Ctrl+End past it stops at its edge), Find and Replace All work in it, and with Vim's keys `gg`, `G`, searches, an operator's motion and the command line's ranges (`%`, a line number) keep to it, as Emacs keeps a narrowed buffer; a document stays narrowed when its mode changes.

### Changed
- A table formula's Calc sum of symbolic terms is built in a time proportional to its terms: `vsum` over a column of 1,000 words took 332 ms and takes 1.3 ms, over 2,000 words 1.34 s and 2.5 ms. The sum was copied whole at each term it gained.
- The sidebar no longer lists the project's folders and files below the open files at first: the file manager lists a folder (`SPC o -` with Vim's keys). Toggle Folder Tree shows the tree again, saving `ui.folder_tree`, now off by default, and Reveal in Folder Tree shows it too.

### Fixed
- A table formula whose Calc sum grows long no longer crashes Kalem by running out of stack: `vsum` over a column of several thousand words, or a column formula over its own column iterated (`C-u C-u C-c *`), whose sum doubles at each pass, went a level deeper for each term. Past 5,000 symbolic terms, or a formula nested as deep, the field shows `#ERROR`, as Emacs errs (at about a thousand, Lisp's `max-lisp-eval-depth`); a vector of numbers of any length is summed as before.
- A plugin built for a later plugin API than Kalem's is not installed, and one installed already does not start, with a message that says so: “pta 0.5.0 needs plugin API ^0.2.14, and this Kalem has 0.2.13: update Kalem to use it”. An extension plugin was installed and then refused at start as asking for “what it was not granted”, which read as a permission to give, and a viewer was to be updated from Browse Plugins.
- In the terminal editor's palette and prompts, Ctrl+U deletes to the start of the line, as on Vim's command line and in a shell, so that a value a prompt offers is cleared before typing another: it stayed, and what was typed went after it. Ctrl+K deletes to the end, Ctrl+W the word before the cursor, and Ctrl+A and Ctrl+E go to the start and the end.
- Focus mode (F8) in Markdown shows the section of the `#` heading the cursor is under: it took Org's headlines, so that a `*` bullet began a section and the headings did not.
- A language server's documentation (`K`, and beside the completion list) shows its Markdown as text: a link as its text, inline code without its backticks, a heading without its marks, and a rule as a blank line, as rust-analyzer puts between a signature and its documentation. A link showed whole, its address and brackets with it.

## [0.6.23] - 2026-10-14

### Fixed
- With Vim's keys, a saved file ends with a line feed, as Vim writes it ('fixendofline'; `:set nofixeol` keeps a file without one): a new file typed and saved, or one whose only line was deleted and typed again, lost its final line feed. The default keys still save the text as it is.
- With Vim's keys, `:qa!` quits at once, dropping the unsaved changes, as Vim does: the bang is the consent. It asked first, as Quit Without Saving from the palette (Doom's `SPC q Q`) still does.
- In the graphical editor, plugins hear that a document open in the background is current again as soon as it is shown, not at the cursor's next motion: a plugin that opened its file with `file.open` and then worked on it found another document current.

## [0.6.22] - 2026-10-14

### Added
- Plugin API 0.2.14, the `problems` import: an extension plugin tells what its tool finds wrong in a file, each problem at a line, a column and a length, with a severity, a message and the tool's name, and both editors draw it as a language server's diagnostic: the wavy line, the line number's color, the status bar on the cursor's line, the hover card and List Problems, beside the server's own, or alone in a language without a server. A plugin sets a file's problems whole and sets them again when they change; they ask for no permission.
- Inlay hints from language servers: the short texts a server would show in a line (a type, a parameter's name, a journal's elided amount or running balance) drawn in place, dimmed and in italics, in both editors, the cursor passing over them. Kalem asks for the whole file's when the server is ready and after each change, and again when the server says they changed; meanwhile the hints shown move with the edits. The setting `editor.inlay_hints` (on) and Toggle Inlay Hints show or hide them.

### Changed
- With Vim's keys, `>>`, `<<`, and Ctrl+T and Ctrl+D in insert mode shift by the document's own indentation until `:set shiftwidth` sets it (as many spaces as it indents by, or a tab in a file indented with tabs), as Tab and a closing bracket do; it was always two spaces. `expandtab` follows the document the same way until set.

### Fixed
- In code, a closing bracket typed alone on its line no longer goes a level further back than the line of its opening bracket: with Vim's keys, whose Enter keeps the indentation, `}` ended every block one level to the left of its `if` or `fn`. The bracket now takes the indentation of its opening bracket's line, as Vim's `smartindent` does, and is typed as it is where there is no opening bracket.
- With Vim's keys, `:q` and the rest of the command line work while a plugin's graph is shown (Graph View): the graph took Enter, so the command never ran, and `0`, `+`, `-`, Tab and the arrows zoomed or moved it rather than going on the line or finishing a command such as `d0`. In insert mode the graph's keys stay its own.
- A plugin's own setting with a dotted key, such as `programs.python3`, is read and written in tables within the plugin's settings, where the settings page writes it and Kalem looks for a plugin's programs. A plugin's own get and set took the whole key as one name, so it missed what the settings page set and wrote a quoted key that nothing else read.
- In the terminal editor, plugins hear that a document open in the background is current again as soon as it is shown, not at the cursor's next motion: a plugin that opened its file with `file.open` and then worked on it found another document current.
- The terminal's status line names the key that opens the command palette in Vim's keys too: `Space :` (the leader's), where it said Ctrl+G, which Vim's normal mode takes for itself (it shows the file's name and length), so that typing a command's name after it ran its letters as Vim's commands in the document.

## [0.6.21] - 2026-10-14

### Changed
- In Vim's insert mode, a key binding written for it (its when-clause looks at `vimMode`, such as `vimMode == insert`) comes before the mode's own Enter, Tab and Backspace, as a binding that looks at `vimCommand` comes before the normal mode's keys: the plain text accounting plugin's Enter in a journal aligns the posting's amount before the new line in insert mode too. Other bindings leave insert mode's keys as they were.

## [0.6.20] - 2026-10-13

### Changed
- Typing in a long LaTeX document parses again from fewer of its parts: the paragraph around the edit is found by position among the body's children rather than by making every one of them (a body of 10 MB has tens of thousands). A keystroke's parse in a paper of 1 MB 0.22 ms rather than 0.99 ms; a keystroke in one of 10 MB 32 ms rather than 42 ms.
- Typing in a long LaTeX document updates its model (sections, labels, citations, numbers) from the paragraphs' parts the parse shares, without making a node for each of the body's paragraphs or moving every one's events to a new table at each keystroke: the model after a keystroke in a paper of 1 MB 0.37 ms rather than 1.0 ms, of 10 MB 11 ms rather than 24 ms; a keystroke in the 10 MB paper 17 ms rather than 32 ms.

### Fixed
- Pasting external text recognizes CR and Unicode line and paragraph separators as well as CRLF, keeping line breaks, blank lines and indentation. Rich HTML paste keeps empty paragraphs, separates multiple paragraphs from the surrounding text, and uses plain text for CSS `break-spaces` content so its whitespace is not collapsed.
- Saving tells what the file could not keep, as its plugin reports it: a workbook with a password to open saved as `.ods` says, after the save, that the `.ods` file is not encrypted (the losses were only logged). Save As under another kind than a plugin's document's own asks its plugin for that kind, in a document of flowing text too: Save As `report.docm` from a `.docx` wrote the `.docx` package under the new name, and is now refused with the plugin's reason, as Save As `.png` from a `.jpg` picture is. A refused Save As leaves the document's file as it was: it took the new name though nothing was written. `kalem run` prints the losses too.

## [0.6.19] - 2026-10-11

### Added
- The when-clause key `folderLayer`: the layer serving the folder of a document's file, or the folder the file manager shows, whatever the document's mode (`graph.logseq`). A plugin's button or menu can show on a notes graph's folder before any note of it is opened: the Note graphs plugin's Graph button did not show on a Logseq graph's folder until a page was opened.
- Vim keys: `gq` and `gw` format the lines of a motion or a selection to `textwidth` (the screen's width, up to 79 columns, without one), `gqq` and `gww` the line, as Vim's internal formatting does with its defaults: a paragraph's lines joined and broken again at blanks, comment leaders (`#`, `//`, `%`, `>`) and list items (`- `) kept; in Org and Markdown, headings, tables, keyword lines and blocks stay as they are, as Doom Emacs fills there. CTRL-W's window keys work the panes (`CTRL-W v`, `s`, `h`, `j`, `k`, `l`, `w`, `o`, `q`, `=`, `+`, `-`, `<`, `>`, `H`, `J`, `K`, `L`, `x`, `r` and the rest, a count resizing by that much), as `SPC w` does; `ZZ` and `ZQ` close. The command line keeps its history (Up and Down for the lines that start with what is typed, CTRL-P and CTRL-N for all), CTRL-R puts a register's text, CTRL-R CTRL-W the word at the cursor, and CTRL-W and CTRL-U take back a word and the line; `:w!`, `:wq!`, `:x!`, `:bd!` (an error before), `:wa`, `:update`, `:file`, `:e #` and `:b#`, `:sp`, `:vs` and `:new` with a file, `:vnew`, `:tabnew`, `:tabnext`, `:tabprevious`, `:tabclose` and `:tabonly`; `:set nu`, `nonu`, `wrap` and `nowrap` set the editor's settings, and `no` before an option Kalem does not keep is no error. `gf` opens the file named at the cursor, `gO` the outline, `:earlier` and `:later` go back and on by steps. 26 more cases run against Vim (`tests/vim/cases.json`).
- The Vim keys of the plugins Doom Emacs has on, where Vim leaves the key unused: vim-surround's `ds`, `cs`, `ys` and `yss` (Doom's evil-surround), with brackets, quotes, tags and any other sign for itself (`ysiw*` makes bold in Org); vim-commentary's `gc` and `gcc` (Doom's evil-nerd-commenter) with the language's markers, as Toggle Comment; vim-unimpaired's `] SPC` and `[ SPC`, `]b` and `[b`, `]w` and `[w`, `]d` and `[d` (the changes a plugin marks), `]e` and `[e` (a LaTeX document's problems), `]t` and `[t` (TODO, FIXME and the like); `*` and `#` on a selection search for it, as in Neovim and Doom.
- Copy Link (`org.link.copy`, Doom's `SPC m l y` in Org): the link's target at the cursor (not its description) on the clipboard.
- In the file manager, `o` opens the entry in a new pane beside the listing, as Emacs's dired and Doom have it.

### Changed
- Enter in Org, `C-c C-c` and the other commands that start from an Org document's text take it from its tree as fast as Org's editing commands do since 0.6.18 (`org_syntax::Parse::text`): Enter at the end of a paragraph of the Org manual (820 KB) 1.1 ms rather than 11 ms.
- Org tables in long documents: Enter in a table (the next row) parses the document again from the change rather than whole, 3.7 ms rather than 107 ms in a table of the Org manual (820 KB); aligning a table writes its changed rows into the text at once rather than one copy of the document per row, and reads its cells without a string for each piece: Tab in a table of 4,000 rows whose widths change 38 ms rather than 50 ms.
- The pickers (project files, recent files, the command palette) match what is typed twenty times as fast: the fuzzy matching reads a path's bytes as they come rather than copying it into lowered characters first, so the project file picker over 100,000 paths takes 11 ms a keystroke rather than 220 ms. The palette's copy of the matching is gone; it uses the projects'.
- Doom's leader table follows Doom: `SPC g b` switches branch and `SPC g B` blames (the table had blame and this line's blame), `SPC o a` is the org agenda's group; Doom's other `SPC g` keys are listed, the git plugin's and a later forge plugin's, and the key hints name `SPC o a`, `SPC g c`, `g f`, `g l` and `g o` as Doom does.
- Typing in a large file of code keeps its highlighting's line starts by moving them past the edit rather than finding them again in the whole text: a keystroke in a Rust file of 2.6 MB costs the highlighting 0.8 ms rather than 3.5 ms.
- Vim's Org motions (`]h`, `[h`, `]l`, `]c`, `gj`, `gk`, `gh`, `gl`) and `>>`/`<<` on a heading take the document's text from its tree as Org's editing commands do (`org_syntax::Parse::text`): about 7 ms less a key in the Org manual (820 KB).
- Typing in a long LaTeX document no longer finds its displayed formulas, tables and folded environments again in the whole text: the blocks the view lays out are kept from the last keystroke where the parse kept the tree, made again only on the lines it parsed again. A keystroke in a paper of 10 MB 48 ms rather than 75 ms (its blocks 0.7 ms rather than 29 ms), of 1 MB 10.9 ms rather than 14.2 ms.
- A keystroke in a LaTeX document no longer checks again whether the formula renderer reads each formula on the screen: what it reads depends on the document's definitions only, kept while they stay the same. A keystroke in a paper of 1 MB with a screen of formulas 3.9 ms rather than 10.9 ms.
- A keystroke in a large CSV file no longer reads the file from its start: the grid's record index and column widths are moved through the edit, and the status bar's count, sum, average, minimum and maximum of the column are kept record by record, the records from the edited one read again until they line up with the old ones. 100,000 rows: a keystroke's layout and status bar 3 ms rather than 50 ms.

### Fixed
- `kalem run PLUGIN COMMAND FILE` with a page of a notes graph (a folder a plugin's layer serves) lets the plugin read the graph's folder, not only the page's own: `kalem run graph graph.backlinks pages/Kalem.md` printed nothing, the plugin unable to read the graph's `logseq/config.edn`.
- The lists commands offer in the palette show each item's title first. Browse Plugins wrote a plugin's version before its name (`0.0.6: Word documents — …`); the install confirmation wrote what the plugin brings before `Install Rust 0.1.2`, which the graphical palette then cut off; Cancel showed as `: Cancel`; Chapters wrote each chapter's words before its title and its indent. An item's detail (`PaletteItem::detail`: a plugin's version, what an install brings, a chapter's words) is now drawn after its title, dimmed at the right and cut before the title is, in both editors; a list with details is as wide as a file list, and no list is wider than the window's text. A category the list's items share is still written before the title (`Org: Cycle TODO State`, a question before its Merge and Cancel); a title without one stands alone, without the colon, in the graphical editor as in the terminal. A plugin's quick-pick shows an item's detail so too, after its label, rather than in place of the list's title; the menus' items (F10) no longer name their menu twice (`Edit: Edit › Select All`).
- Recalculating a table whose fields' text refers to fields that refer back (`| $2 | $1 |` with `#+TBLFM: $3=$1+0`) stops with an error rather than freezing Kalem: a field's text is substituted into a formula as it is, references included, as Emacs substitutes it, so such fields were substituted forever (Emacs hangs on the table until `C-g`). Found by recalculating the corpus' tables with their formulas changed at random.
- In Org's Insert mode with Vim keys, C-RET and C-S-RET make an item (a row, a heading) below or above, as in Normal mode and in Doom Emacs; they cycled the TODO state, the Word-like keys'.
- After a definition of a LaTeX document changed (`\newcommand`), its formulas were sometimes still judged readable by the formula renderer, or not, with the old definitions: the cache of what it reads was known by the document model's address, which a new model could take after the old one was dropped. Now it holds a weak reference to the model.
- `kalem check` and `kalem table` could report a file's problems at the lines of the file checked before it: the line starts were kept for the text last asked about, known by its address and length, which the next file's text could take once the last was dropped. Each file's report now has its own.

## [0.6.18] - 2026-10-11

### Changed
- Exporting an Org file parses it fewer times: a step that read a text another had just parsed (macros, Babel, the document after Babel left it as it was) parses it no more. The Org manual (820 KB) exports to HTML in 475 ms rather than 555 ms, Worg's release notes (450 KB) in 200 ms rather than 240 ms.
- Org's editing commands take the document's text from its tree ten times as fast (`org_syntax::Parse::text`), as each of them starts from it: typing in a table of the Org manual (820 KB) costs 1.7 ms of the command's time a keystroke rather than 6.9 ms, and every Org command on a large file (TODO states, tags, properties, tables, lists, emphasis) gains about as much.

### Fixed
- `kalem run` waits for the programs a plugin's command runs and the requests it makes, and for what their answers do: a command that shows a document once its program ends (the plain text accounting plugin's Balance, Check and reports, run by hledger, Ledger or Beancount) printed nothing, since batch mode stopped as soon as the command returned.
- A component plugin installed from the index by its name (`kalem plugin install pta`, Browse Plugins) gets the files its manifest names with it, its syntaxes and their bases, from its source folder at the release's tag, as its `plugin.json` comes: only the component and the manifest were written, and a plugin with a language half (the plain text accounting plugin) then loaded no language at all, its syntax files missing. Installed from its folder's link, it had them.
- An extension plugin installed while Kalem runs (Install Suggested Plugin, Browse Plugins, `kalem plugin install` in a terminal) starts at once, as it would have with Kalem: its commands, keys, menus and layers are there without a restart, and it is told of the documents open. One removed stops, what it registered taken back; one updated is replaced by its new version. Before, only viewers were chosen again: a Logseq graph opened right after the Note graphs plugin was suggested and installed stayed plain Markdown until Kalem was started again.
- A plugin that starts after the editors opened their first documents (the first start after an install compiles it for a few seconds) is told of them, and of `app:ready`, rather than missing them; and a notes graph's folder is offered as a project once its plugin's layer arrives, not only when the layer was there before the document.

## [0.6.17] - 2026-10-11

### Added
- A file another program changed while its document has unsaved changes is merged into them when the two changed different lines (`kalem_core::merge`, a three-way merge with the file as last read): both changes are kept, the document stays unsaved, and the status bar says so; only when both changed the same lines does Kalem ask as before. For Logseq or Obsidian open on the same notes, a sync, or git. Files up to 4 MB.

### Changed
- A graph drawn (`graph-view`) looks as Logseq's and Obsidian's: the most common group's nodes grey, the other groups colored; as many names as fit are written, the heaviest first, a name left out where it would cover one already written until zooming in makes room (the node chosen, the one under the mouse and its neighbors always named; of a large graph, the 300 heaviest on the screen tried); the names smaller and softer. The terminal places its names the same way, in cells.

### Fixed
- Dragging a node of a graph drawn in the window (`graph-view`, the graph plugin's Graph View) no longer crashes Kalem: the mouse handler asked gpui for an animation frame outside a view's drawing, which aborts the process (0.6.14 to 0.6.16).
- A layer's hidden lines (a Logseq block's folded children) stay hidden with the cursor at the start of the line after them: moving up onto them from the next block opened them, and the cursor stepped into the block instead of over it.

## [0.6.16] - 2026-10-11

### Added
- A user's `servers.KEY.initializationOptions` for a language plugin's server, merged over the manifest's as its `settings` are: a server that reads part of its configuration only when it starts is told the user's (beancount-language-server's main journal, which the plain text accounting plugin's journals need: an included file checked alone has its accounts reported unknown). A plugin's manifest may describe such a setting, so that its page in the settings panel shows it.
- A language plugin's server may have capabilities Kalem leaves unused: `"ignore": ["documentFormattingProvider"]` in the manifest's server, or the user's `servers.KEY.ignore` in its place (`[]` for every capability used), taken out of the server's answer to `initialize`, so that nothing asks the server for them. For a server's feature that is wrong, or that the plugin does otherwise: ptimoney's hledger-lsp 0.7.0 formats a journal into one that means something else (an automated posting's `*-1` dropped, `==` written `=`), and Kalem formatted with a server whenever it offered formatting.

### Changed
- A plugin's keys in the files of a language its manifest declares (`textType == journal`) come before the profile's there, as its keys in its own documents do, so that Enter in a journal can be the plain text accounting plugin's (a posting's amount aligned, then the new line, as in ledger-mode); the profile's `enter` came first and the plugin's never ran. Such keys are no longer reported as bound for a text type Kalem does not know.
- `formatters.TYPE = "off"` turns formatting of the type off altogether: Format Document says so rather than asking the language server, and format on save runs no plugin's formatter command either. It turned off only the type's usual program, and a server's or a plugin's formatter ran all the same.

### Fixed
- Org's entities in HTML and Markdown are written with the names HTML 4.01 gives their characters, as Emacs writes them (`\infty` as `&infin;`, `\Rightarrow` as `&rArr;`, `\dots` as `&hellip;`): since 0.1.0, whose entity table is made from standards (D18), many were numeric references (`&#8734;`) or the HTML Standard's other names (`&Rightarrow;`), the same characters spelled otherwise, and seven of Worg's files no longer exported as Emacs exports them. Five do again; the two that list Org's entities in a table differ where D18 chose another character.

## [0.6.15] - 2026-10-11

### Added
- The plugin API is 0.2.13. A plugin's layer may draw lines under a line of the text that are not part of it (the `layer-2` export, `kalem-plugin`'s feature `layer-2`): styled pieces, indented from where the line's text starts, in both editors. The cursor steps over them, a click on one puts it at the end of their line, a search and a copy leave them out, the source view does not show them. For the graph plugin's embeds and queries, shown in their note as Logseq shows them: an embedded block's children, an embedded page's blocks, a query's results. `graph-view.wit` and `fs-2.wit` of 0.2.12 are frozen.
- A folder a plugin's layer serves (a Logseq graph, an Obsidian vault) is offered as a project when one of its notes is opened outside the projects, once a session, and added at once with `projects.auto_add`: its plugin reads projects only, and a graph's folder left out made the plugin see nothing. The layers are asked again when a project is added.
- The folder of a plugin's layer (a Logseq graph, an Obsidian vault) is watched whole once it is a project: a file another program writes, renames or removes there reaches the plugins as `workspace-file-changed`. The editors watched only the files they held open, and a page Logseq or Obsidian wrote while Kalem was open stayed unknown to the graph plugin (its backlinks, its pages, its search) until Kalem started again.

## [0.6.14] - 2026-10-11

### Added
- The plugin API is 0.2.12. A plugin's document may be drawn as a graph of nodes and edges (the `graph-view` import): the plugin gives the nodes (an id, a label, a group, a weight) and the edges, and a command; the editor lays them out with forces a few steps a frame until the layout settles, draws them (the window with colored dots, lines and labels, the terminal with Braille dots), lets the user pan, zoom, drag a node and choose one (a click, Tab), and opens a node (a double click, Enter) by running the command with its id. The document's text, which the plugin writes with the graph, is what Show Source, search and a narrow terminal show; written again, the nodes still there keep their places. For the graph plugin's Graph View and Local Graph, as Logseq's and Obsidian's.
- The `fs-2` import (API 0.2.12): a plugin with `fs:write:workspace` moves files and folders and puts them in the trash; open documents follow their files, as after a move in the file manager.
- A layer of a plugin's manifest may name the plugin's command that opens links in the documents it serves (`"open"`), which a Ctrl-click runs in place of the mode's: a link followed by the graph's own rules.

### Fixed
- A plugin may run its own command with `kalem.run`: it is queued, run once the plugin's call returns, as another's is. It was refused, and a plugin that asks a question and then edits the document (which only its commands reach) could not: the graph plugin's Insert Link, Insert Block Reference, Set Priority, Schedule, Deadline and Rename Page stopped after the answer with "is the plugin's own".
- A plugin's own settings reach it in the editors. Kalem starts the plugins while the editor reads its settings, and a plugin that read them as it started got nothing and never heard of them (the graph plugin's `graphs` was never seen in `kalem tui`, only in `kalem run`). Until the editor gives its settings, a plugin now reads Kalem's defaults, and the plugins watching a setting that differs from its default are told once the settings come.

## [0.6.13] - 2026-10-10

### Added
- The plugin API is 0.2.11: an extension plugin may offer completions. Its manifest declares its completers (`"completers": [{"id", "types", "layers", "triggers", "word", "replaces", "priority"}]`) and it exports the `completer` interface (the world `extension-completer`, `kalem-plugin`'s feature `completer`); Kalem asks it when a trigger fires as the user types, or on Ctrl+Space, and shows its items in the one menu with its own completers', ranked with them. A completer may stand in for Kalem's own (`wiki`, `words`) where it gives items, and may serve only the documents its plugin's layers serve. For the graph plugin's pages, tags and blocks after `[[`, `#` and `((` in a Logseq graph or an Obsidian vault.
- The plugin API's `clock-2` import for extension plugins: the offset of the user's time zone at an instant, so that a plugin knows the local day and time (the graph plugin no longer asks for today's date). A plugin built against 0.2.10 loads as before; `layer.wit` of 0.2.10 is frozen (`wit-frozen/0.2.10`).

### Changed
- A plugin's keys in the documents its layer serves (a when-clause naming `editorLayer`, `graph.logseq`) come before the profile's and Vim's, as its keys in its own documents do, so that Enter or Tab in a Logseq note's list is the graph plugin's outliner's and not Markdown's list continuation.

### Fixed
- In a terminal without the kitty keyboard protocol, a binding to `alt+[` is known to be one the terminal cannot send (it is the start of an escape sequence, which waited for the rest and swallowed the next key), as `ctrl+i` is: it is reported, or left out where the command has another key.

## [0.6.12] - 2026-10-10

### Added
- Run Project and Test Project (`SPC p R`, `SPC p T` with Vim keys), and Test at Cursor: the language plugin's run and test commands (`cargo run` and `cargo test` for Rust, `mix test FILE:LINE` for the test at the cursor in Elixir) run in the project's root, their output in a read-only document as it comes and how they ended in its last line and the status bar. Running the same command again, or closing its document, stops the one under way. Where the file's language gives no command, one is asked for and run in the shell.
- Language plugins: servers beside a language's own (`alongside` in the manifest), a linter beside the type server: each gets the language's files, their problems are shown with the first's, completion and code actions join theirs, and a question one server answers goes to the first that has it. The Rust plugin serves `Cargo.toml` with Taplo (its keys from Cargo's schema) and crates-lsp beside it (the crates' versions). `servers.KEY.enabled = false` in a plugin's settings turns one off; Language Server Status and `kalem lsp status` name them.

### Changed
- A language server's problems are underlined in three colors: errors in red, warnings in orange (yellow in the terminal), information and hints in blue. Errors and warnings were both red. A problem over several lines is underlined on each of them, not on its first only. LaTeX's checks keep their red and blue.

### Fixed
- `kalem plugin check` checks the installed extension plugins too: each is loaded and bound to the plugin API as starting it would, without being activated, and reported as running or with why it cannot, the check failing with it. Only viewers were checked, so the *Released plugins* workflow installed an extension plugin such as graph and never tried it.
- LaTeX: case changes inside case changes show what pdflatex typesets. The innermost decides, a letter's command changing as a command (`\MakeUppercase{\MakeLowercase{\ss}}` keeps its ß), but Œ and œ take the outermost `\MakeUppercase` or `\MakeLowercase`'s case, as LaTeX lets `\OE` and `\oe` be one another for the whole of its argument (`\MakeLowercase{\MakeUppercase{\OE}}` is œ).

## [0.6.11] - 2026-10-10

### Added
- Layers over a mode's view (plugin API 0.2.10): a plugin changes how Markdown and Org documents are drawn without parsing them again, hiding, replacing or styling ranges of the text away from the cursor and hiding or folding whole lines, as its manifest's `layers` declare for the folders holding a marker (`logseq/config.edn`, `.obsidian`). The source view shows the text as it is. The plugin API's `layer` export and `layers.refresh` import; an extension plugin of an earlier version loads as before.
- Extension plugins may read the clock (plugin API 0.2.10's `clock`, as viewers do): the time, the time zone and random bits.
- Batch mode: `kalem run PLUGIN COMMAND [FILE...]` runs an installed plugin's command without a window, as the editors run it, against each file (a folder as its listing) or once without one. The documents the command writes are printed (`--format json` for scripts), its notices go to standard error, and a file it changed is saved. Its questions are answered as a script would leave them: a prompt takes the text it offers, a choice nothing, a confirmation no. `--args` gives the command's arguments. The folders named are the plugin's to read, as a project's are, for that run.
- Search in Folder takes `ignore`, patterns left out of the search besides the folder's own (a plugin searching a Logseq graph leaves out `logseq/`), and Show in System File Manager takes a `path`, a file Kalem does not open itself (a whiteboard, a canvas).

### Fixed
- The plugin `kalem plugin new` starts is a viewer of a format that is not text: a picture whose first bytes are `DOTS` and a NUL, which its manifest declares (`"applies": {"magic": ["44 4F 54 53 00"]}`), then rows of `#` and `.`. Its pictures were text before, and Kalem opens a text file in a mode, never in a viewer, so they opened as text and `kalem view` refused them. A test of the template checks that the manifest declares the bytes the viewer reads, and the Book's "Plugins in practice" says how to make a picture with `printf` and open it.

## [0.6.10] - 2026-10-10

### Added
- Language plugins: a server's state in the status bar. The manifest's `status` names the notification in which a server tells it (rust-analyzer's `experimental/serverStatus`) and where its text, its level and its idleness are; while the server works only in part or not at all, the status bar and Language Server Status say why (rust-analyzer's "cargo check failed to start"), and while it is not idle it counts as busy, so `kalem lsp check` waits for it and then says its state on standard error. A server's `capabilities` in the manifest are added to Kalem's where Kalem says nothing: the extensions a server sends only to a client that asks.
- Language plugins: Enter in code asks the server for its own new line when the plugin gives one (`requests` keyed `edit.newline`; rust-analyzer's `experimental/onEnter`, which continues `//` and `///` comments). Kalem's new line is made at once, and the server's takes its place as it answers, in the same undo step and with the cursor where the server puts it, unless the text changed meanwhile. Also in Vim's Insert mode.
- Language plugins: Structural Search and Replace (`code.structuralReplace`) asks for a pattern and its replacement and sends them in the server's request (`{search}`, `{replace}` and `{selections}` in the manifest's `extra`); the edits across the project's files are offered first, as a rename's are (`shape` `workspaceEdit`). The Rust plugin gives rust-analyzer's (`foo($a) ==>> bar($a)`). `kalem lsp ask` takes the pattern and the replacement as `--input NAME=TEXT`.
- Language plugins: a server's edits with a snippet in them (rust-analyzer's Move Item Up and Down) put the cursor where the snippet does.

## [0.6.9] - 2026-10-10

### Changed
- No plugin is built into Kalem any more: the viewers of pictures, PDF files and workbooks are installed as every plugin beyond the core (Org, Markdown, CSV and LaTeX) is, from the plugin list or when a file needs one, so a viewer's update reaches you as its release, without a new Kalem. The release is 4.4 MB smaller and compiles no component at its first start. The feature `components` still builds them in, for a build of one's own.
- Which installed plugin serves a file is what its manifest declares, the declaration the index names it by: a list of rules, each of extensions, whole file names, first bytes (`magic`), `#!` interpreters and markers in the file's folder or above it, gathered from the manifest's `opens`, `languages` and `layers` and its own `applies`. A viewer is no longer asked whether it opens a file, so a file without its extension opens by its first bytes when a plugin declares them; a language plugin serves the files its declaration matches; an extension plugin with no `activation` starts with the first file it serves (`onFile`). A viewer whose plugin was removed while Kalem ran is now taken away at once rather than left to fail on its files.

### Added
- A plugin of the index that would serve a file you open, and is not installed, is said once in the status bar: a language's for its files (the Rust plugin for a `.rs` file), a Logseq graph's for a page of a folder with `logseq/config.edn`; Install Suggested Plugin installs it, and the setting `plugins.suggest` turns this off. Every plugin says when it serves a file in one declaration, tested alike for every kind of plugin, here, for a file Kalem cannot open, and to hand a file to an installed plugin, so that the plugin named for a file is the one that serves it.
- A file that is not text and that no installed plugin opens, from the file manager, a link, `kalem FILE` or Open, names the released plugins of the index that open it, to install, after which the file opens, and offers the system's application beside them (Find a Plugin for the File), in both editors. New Workbook, New from Template and Open as Workbook offer the plugin that makes workbooks when it is missing.
- Workbooks in the menus, as Excel has them: a workbook's cells are formatted from Format (font, fill, alignment, wrap, merge, borders, number formats, cell styles, conditional formatting, row heights and column widths, hidden rows and columns, the sheets), filled, cleared, deleted, found and gone to from Edit, inserted from Insert (cells, rows, columns, sheets, functions, charts, pivot tables, sparklines, pictures, links, notes), checked and protected from Review, zoomed, frozen and split from View, saved as CSV or PDF and printed from File; the new menus Data (sorting, filters, Text to Columns, duplicates, validation, groups and subtotals, What-If analysis, pivot tables, tables), Formulas (functions, names, auditing, calculation) and Chart (the chart's kind, titles, legend, labels, trendlines, axes, colors, size). F10 lists them in both editors. A PDF's or a picture's menus carry none of them.
- The graphical editor's toolbar has a workbook's buttons: Bold, Italic, Underline, Borders, Fill Color, Font Color, the alignments, Wrap Text, Merge and Center, Currency, Percent and Comma Style, Increase and Decrease Decimal, Sort, Filter, AutoSum and Insert Chart.
- A right click on a workbook's cell offers what is there, as Excel does: Edit Note and Delete Note on a note, New Note elsewhere; Open, Edit and Remove Link on a link; Pick From Drop-down List on a cell with a list; a table's Total Row and Convert to Range; a pivot table's Refresh and Options; Show Comments on a comment; a chart's (its type, title, legend, labels, trendline, place, template, Delete) and a picture's first when one is over the cell; and always Format Cells, Filter by Selected Cell's Color, Reapply, Sort by Color and Custom Sort. The rows' and columns' menus have Paste Special, Format Cells and Ungroup.
- Row Height and Column Width, in a workbook's Format menu and on the rows' and columns' right click, as Excel's: the size typed (a row's in points, a column's in characters, the cursor's offered, a decimal comma read too) given to every row or column selected, in one undo step.
- Create from Selection (Ctrl+Shift+F3, in the Formulas menu), as Excel's: a name for each column of a workbook's selection from its top or bottom row's label, or for each row from its left or right column's, referring to the rest of it; a label made a name as Excel makes it (spaces as underscores, `_2024` for a number, `Q1_` for a label that reads as a cell); in one undo step.
- Export as CSV or Text (a workbook's File menu), as Excel's Save As types: CSV UTF-8 with commas or semicolons, Text with tabs, CSV in Windows-1254 (Turkish) or Windows-1252 with a `?` for a character the code page lacks; the sheet shown or every visible worksheet, each to its own file in a folder; files already there replaced after asking.
- The Book's chapter "Workbooks" (Part III): the standard and the oracle, what the workbook plugin reads, shows, edits, computes and writes, its macros, exports, known differences and limits, measured.
- A workbook's comment edited: Comments offers Edit Comment and Edit Reply for the thread at the cursor, the text it has offered, when the workbook's plugin gives its comments through the plugin API's annotations (xlsx 0.0.10).
- The status bar sums a workbook's selection as Excel's can: after its Average, Count and Sum, the Min and Max, and the Numerical Count when some of the values are not numbers (last, so that a narrow terminal cuts them first).

### Fixed
- A workbook's Edit menu offered the text's Find, Find and Replace, Select All and Toggle Comment, which did nothing in its cells: it now offers the grid's Find, Replace and Select All (Ctrl+F, Ctrl+H and Ctrl+A ran them already). The commands on a text (the line commands, Go to Matching Bracket, Trim Trailing Whitespace, Go to Line, Source View, Reopen and Save with Encoding) are no longer offered for a workbook, a PDF or a picture, in the menus or by their keys.

## [0.6.8] - 2026-10-10

### Added
- Pictures of documents of flowing text (a Word document) are drawn, in both editors, as large beside the text as in the document and no wider than the text, through the plugin's `render-picture`. In the terminal a picture alone on its line is drawn when the terminal draws pictures and the cursor is elsewhere; otherwise its alternative text shows, as before.
- The plugin API is 0.2.9: `flow-3`, exported with `flow`, says what changed in a unit's flow since a version (`changes`: a run of items replaced, the paragraphs after it numbered on), so that after an edit Kalem fetches the items that changed rather than the whole flow. A plugin without it has its flow read whole, as before. `flow-2.wit` of 0.2.8 is frozen.

### Fixed
- Typing in a long document of flowing text (a Word document) was slow: each keystroke read every paragraph again from the plugin and measured every line again, two thirds of a second in a document of 20,000 paragraphs. Kalem now fetches the paragraphs the plugin says changed and lays out their lines again, nothing else: with the Word plugin 0.0.6 a keystroke there takes 1.4 ms.
- LaTeX: Unnest Item (Shift+Tab) and Nest Item (Tab) moved only an item's first line. An item going on over more lines lost them to the item before, and one whose group or environment closed on a later line (`\texttt{…}` closing on the line of the list's `\end`) left a document LaTeX could not compile. The whole item now moves, with its lines and the lists inside it.

## [0.6.7] - 2026-10-10

### Added
- Language plugins: a server's `version` in the manifest (`["--version"]` for rust-analyzer). `kalem lsp status` runs it in the project's root and prints the server's version, or why the program found does not run (rustup's `rust-analyzer` when the component is not installed is on the PATH all the same), with the plugin's install text.
- Language plugins: a server's own requests through commands any server with such a request serves, given in the manifest's `requests`: Expand Macro, Open Documentation in the Browser, Go to Parent Module, Go to Project File, Reload Project, Join Lines (Language Server), Move Item Up and Down. The Rust plugin gives rust-analyzer's to them. `kalem lsp ask code.expandMacro FILE LINE:COL` asks one from the command line.

### Fixed
- Language servers: a server whose process ends before it answers `initialize` (rustup's proxy for a component not installed, a version manager's shim with no version chosen) was started again five times over half a minute, and Kalem then gave up saying only its exit code (`Some(1)`). It is not started again: the status bar, the `SPC c` keys and `kalem lsp check` say at once why, in the server's words (the last lines it wrote on its standard error), with the plugin's install text; Restart Server tries again. The give-up notice named a `kalem lsp log` command that does not exist; it names `kalem lsp check --log`.
- Language servers: a file a server named outside its project, opened from a definition or a reference (Rust's standard library, a dependency's source), started another server in the file's own folder. For Rust that was a rust-analyzer in the standard library's folder, which cannot load it, so nothing answered there. The server that named the file now serves it, unless one already runs in the file's own folder.
- Language servers: the diagnostics a server gives only when asked (the protocol's pull model) never reached Kalem. rust-analyzer gives its own diagnostics that way (type mismatches, naming, unresolved imports, a file no module includes), so Kalem showed only cargo's. Kalem now asks for them after a file opens and after its changes, and when the server says they changed, and shows them with the others.
- Language servers: a question asked while a server was loading the project (rust-analyzer, at its start) could come back as the error "content modified". It is asked again a few times, and only then said, as "still busy".
- Format Document said "Already formatted" when a server answered with no edits, also when it had not formatted at all: rust-analyzer answers so when rustfmt is not installed for the project's toolchain or the file has a syntax error. Such an answer is now said as the server having changed nothing.

## [0.6.6] - 2026-10-10

### Added
- Format Document (`SPC c f`, Shift+Alt+F) formats code of any language through the formatter of its type, as Emacs's format-all does: the one the new setting `formatters.TYPE` names (`[formatters] python = "black -q -"`; the type a language, an extension, a mode or a file name; `off` turns it off), else, after a language server's or a language plugin's formatter, the language's usual program when it is installed (rustfmt, mix format, ruff or black, prettier, gofmt, clang-format, shfmt, stylua, zig fmt and others), with the text on its standard input, run from the project's root. For a type without a formatter (a workbook, a picture) the command says so and names the setting that would add one. `kalem fmt` formats such files the same way. The setting `editor.format_on_save` (off by default) formats the document the same way at each save; a formatter that fails leaves the text as it is and the status bar says why.

## [0.6.5] - 2026-10-10

### Added
- The plugin API is 0.2.8: `flow-2`, exported with `flow`, changes paragraphs' look and lists (`set-paragraphs`: alignment, indents, spacing, line spacing, a list of bullets or of numbers in a CSS numbering style, its level, the look given directly cleared), in no format's words. `flow.wit` and `annotations.wit` of 0.2.7 are frozen.
- Paragraphs and lists in documents of flowing text (a Word document), as a word processor's: Align Left (Ctrl+L), Center (Ctrl+E), Align Right (Ctrl+R), Justify (Ctrl+J), Bullets (Ctrl+Shift+L), Numbering and Numbering Style, Increase and Decrease Indent (a list item a level deeper, another paragraph a half-inch step), Line Spacing, Space Before, Space After and Clear Paragraph Formatting, on the selection's paragraphs or the cursor's. They are in the Format menu, and the graphical editor's toolbar has the alignments, the lists, the indents and the line spacing, pressed as the cursor's paragraph has them.

### Fixed
- Documents of flowing text (a Word document) on a dark theme: text the document colors black, dark gray or another dark color, chosen for a white page, did not read on the background. Such a color is now shown with its lightness turned over and its hue kept, in both editors, and text on a highlight is drawn to read on it; the file keeps its colors. A separator drawn as line shapes is drawn as a horizontal rule across the text.

## [0.6.4] - 2026-10-10

### Fixed
- Search Lines (Doom's `SPC s b`) did nothing in a file manager listing, in both editors. A listing is now searched like any document: the lines are its entries, so the search finds a file, the cursor following the entry while typing; Enter stays on it and `l` or Enter opens the file. Across the open documents (`SPC s B`), the listings' lines take part too.
- `kalem complete` waited for each completer only as long as typing does (100 ms for the pages of wiki links), not the two seconds it gives: on a busy machine the wiki pages were missing from its list.

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
