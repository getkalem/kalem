# Kalem

Kalem is a text editor that shows a file the way it reads and never touches what you did not edit. Org, Markdown, LaTeX, CSV and BibTeX files open as documents and grids; PDF files, pictures and Excel workbooks open in viewers; every other text file opens as code. One program, in a window or in a terminal.

<!-- The GIF of the same file in both editors (docs/todo.md T1.8.7) goes here once it is recorded: ![Kalem: an Org file in a window and in a terminal](assets/kalem.gif) -->

**Status: alpha.** Release 0.1 is the first; it installs [below](#install). Rough edges are expected: an [issue](https://github.com/getkalem/kalem/issues) with the file, when you can share it, helps most.

## Why

- **The file stays yours.** Kalem edits the text in place and saves it byte for byte. No database, no import, no "save as". A co-author who opens the file in another editor, or in `git diff`, sees only your edits. Workbooks are the one exception, below: an `.xlsx` keeps the parts you did not edit byte for byte, but an `.ods`, `.xls` or `.xlsb` is converted as it opens.
- **Every format as itself.** Kalem writes nothing into a file that its format does not define, and shows what it does not understand as source.
- **Checked against the reference.** The Org parser, commands and exporters are compared with Emacs on thousands of files; Markdown with the CommonMark and GFM test suites; LaTeX with pandoc and a TeX engine; CSV with the files spreadsheets write.
- **Fast.** One binary, no runtime. A keystroke in a paragraph reparses in a twentieth of a millisecond; the [measurements](book/part-4/performance.org) give each number with the command that reproduces it.
- **In a window and in a terminal.** The same documents, commands and keys in the graphical editor and in a terminal, over SSH too; every operation on a document is also a command on the command line (`kalem check`, `kalem fmt`, `kalem export`…).

## What opens as what

| File | What you see |
|---|---|
| `.org` | A document: folding, TODO states, dates, tags, tables with formulas, footnotes, citations, typeset formulas. Export to HTML, Markdown, LaTeX, PDF and text as Emacs does; Word, OpenDocument and EPUB through pandoc. |
| `.md`, `.markdown`, `.mdown`, `.mkd`, `.gfm` | A document, GitHub flavor: task lists, tables with formulas, front matter, wiki links, pictures. |
| `.tex`, `.latex`, `.ltx` | A document: numbered sections, typeset formulas, references, citations, figures; PDF builds with the errors at their lines. |
| `.csv`, `.tsv`, `.tab` | A grid: sorting, filters, a record view, column statistics. |
| `.bib` | A grid of entries. |
| `.pdf` | A viewer, the PDF plugin: pages, zoom, search, and from a LaTeX build, Ctrl-click back to the source line. |
| Pictures | A viewer, the picture plugin, for PNG, JPEG, GIF, WebP, TIFF and a dozen more, in the terminal too where it draws pictures (kitty, Ghostty, WezTerm, iTerm2, or Sixel). An SVG file, being text, opens as its XML; it shows as a picture where a document links it. |
| `.xlsx`, `.xlsm`, `.xltx`, `.xltm` | A workbook, the workbook plugin: sheets as grids with their formulas and charts, edited and saved as itself. A save asks Excel to recalculate when it opens the file. |
| `.ods`, `.xls`, `.xlsb` | A workbook, the same plugin, converted as it opens: number formats are lost, and saving it back in its own format asks first, saying what it keeps. Save As `.xlsx` leaves the original alone. |
| Anything else | Code with highlighting. |

<!-- One picture per format, the graphical editor on a corpus file, made by tools/readme-screenshots.sh on a Mac with the Screen Recording permission (docs/todo.md T2.10.13). Take this comment away once the files exist:
![An Org file in Kalem: the Org Compact Guide as a document, with its headings, lists and tables](assets/screenshot-org.png)
![A Markdown file in Kalem: a note with its headings, links and task list](assets/screenshot-markdown.png)
![A LaTeX file in Kalem: an arXiv paper, its sections and equations typeset, the rest as source](assets/screenshot-latex.png)
![A CSV file in Kalem: a grid with the column statistics](assets/screenshot-csv.png)
![A BibTeX file in Kalem: a grid of entries](assets/screenshot-bibtex.png)
-->

Around them: a file manager, projects, find in files, a command palette, an outline, split views, themes, English and Turkish. The keys are Word's by default; Vim's are one setting away ([below](#for-emacs-and-vim-hands)). The three viewers are plugins: WebAssembly components released by [getkalem/plugins](https://github.com/getkalem/plugins), each with its own memory and only the permissions it declares, built into the binary; `kalem plugin install` adds others from the index.

**Not yet:** the agenda; signed installers and a macOS app in the release (the installer puts an unsigned `kalem` on the path, and a `Kalem.app` is built from source, below); a Windows MSI; Linux AppImage and Flatpak packages.

## Install

From a release, on macOS and Linux:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/getkalem/kalem/releases/latest/download/kalem-editor-installer.sh | sh
```

and on Windows, in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/getkalem/kalem/releases/latest/download/kalem-editor-installer.ps1 | iex"
```

The installers put `kalem` in `~/.cargo/bin`; delete it there to uninstall. The release also has terminal-only archives (`kalem-terminal-*`): the terminal editor and the command-line tools, without the graphical editor, the viewers of PDF files, pictures and workbooks, and plugins. On Linux the full binary links the windowing libraries (xkbcommon, Wayland, X11, fontconfig, freetype, Vulkan) even for `kalem tui`, so a server should take the terminal archive; both need glibc 2.35 or later (Ubuntu 22.04). On macOS, a `Kalem.app` made from source with `tools/macos-app.sh` is not signed yet: open it the first time with right-click and *Open*.

From source, with Rust 1.96 or later; on Linux the graphical editor also needs the development files of those libraries, listed in the Book's [Installing](book/part-1/installing.org):

```bash
git clone https://github.com/getkalem/kalem && cd kalem
cargo install --locked --path crates/kalem
```

```bash
kalem notes.org                     # the editor; `kalem tui notes.org` for the terminal
kalem export notes.org --to html    # the command-line tools: check, fmt, query, export…
```

Settings live in `~/.config/kalem` on Linux and macOS (`$XDG_CONFIG_HOME/kalem` if it is set) and in `%APPDATA%\kalem` on Windows; the log and crash reports in `~/.local/state/kalem` (`$XDG_STATE_HOME/kalem`), or `%LOCALAPPDATA%\kalem`. `KALEM_CONFIG_DIR` and `KALEM_STATE_DIR` move them, and `KALEM_LOG=debug` makes the log say more.

## For Emacs and Vim hands

Kalem has Emacs's architecture, every action a command and every keymap data, and leaves out Elisp. The Book's [Kalem and Emacs](book/part-4/kalem-and-emacs.org) says what is taken, what is left out and why. What you will look for first:

- **Vim keys.** `editor.keymap_profile = "vim"` in `settings.toml` makes every key Vim's: modes, motions, operators, text objects and counts, registers and macros, marks, search with Vim's patterns, and the command line with ranges, `:s`, `:g`, `:sort`, `:set` and the file and window commands. Some five hundred key sequences are run in Vim and in Kalem, and the text and cursor after them must agree ([Keys](book/part-1/keys.org), "Vim keys").
- **Doom's leader.** Space is the leader, in Doom Emacs's groups: `SPC p p` switches the project, `SPC SPC` finds a file in it, `SPC m` is the local leader of the document's mode. Pause after a prefix and a panel shows what can follow, as which-key does. Every key of Doom's leader map is listed with what it does in Kalem, or why not yet ([Keys](book/part-1/keys.org), "Doom's leader map in Kalem").
- **Dired.** The file manager has Dired's keys: Ctrl+Alt+D lists the document's folder with the cursor on its file, `e` edits the names in place as wdired does, `* t` and the other marks, `K` takes an entry out of the listing ([The file manager](book/part-1/the-file-manager.org)).
- **Emacs's Org keys.** [`docs/keymaps/emacs.json`](docs/keymaps/emacs.json) is a complete keymap with Org mode's Emacs keys (`C-c C-t`, `C-c C-c`…) to copy from into your `keymap.json`; the hint panel follows `C-x` and `C-c` too ([Keys](book/part-1/keys.org), "Your own keys").

## Known limitations

- An `.ods`, `.xls` or `.xlsb` workbook is converted to be edited (the table above), and one with a password to open is saved as `.ods` without its encryption.
- Markdown: export, printing and `kalem fmt` are for Org and LaTeX; TOML front matter (`+++`) and `$$` blocks over several lines show as text. In a large file with footnotes or link reference definitions, each keystroke parses the whole file again.
- LaTeX: building a PDF needs TeX Live, MiKTeX or Tectonic installed; Kalem does not download one.
- The terminal-only build has no viewers and no plugin host, and draws an SVG picture without its text.
- Line endings and a byte order mark stay as the file has them; there is no command to change them yet.

## More

- [The Kalem Book](https://getkalem.github.io/kalem): the manual, and what Kalem does with each format.
- [Contributing](CONTRIBUTING.md), the [design documents and task list](docs/README.md), the [changelog](CHANGELOG.md).
- License: MIT or Apache-2.0, at your option.
