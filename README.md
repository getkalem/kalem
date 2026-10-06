# Kalem

Kalem is a text editor that shows a file the way it reads and never touches what you did not edit. Org, Markdown, LaTeX, CSV and BibTeX files open as documents and grids; PDF files, pictures and Excel workbooks open in viewers; every other text file opens as code. One program, in a window or in a terminal.

<!-- A GIF of the editor goes here (docs/roadmap.md, R2.5): ![Kalem](assets/kalem.gif) -->

**Status: alpha.** The first release, 0.1, is being prepared ([roadmap](docs/roadmap.md), M2); until it is out, build from source, below.

## Why

- **The file stays yours.** Kalem edits the text in place and saves it byte for byte. No database, no import, no "save as". A co-author who uses Emacs, a TeX editor or Excel sees only your edits in the diff. Workbooks are the one exception, below: an `.xlsx` keeps the parts you did not edit byte for byte, but an `.ods`, `.xls` or `.xlsb` is converted as it opens.
- **Every format as itself.** Kalem writes nothing into a file that its format does not define, and shows what it does not understand as source.
- **Checked against the reference.** The Org parser, commands and exporters are compared with Emacs on thousands of files; Markdown with the CommonMark and GFM test suites; LaTeX with pandoc and a TeX engine; CSV with the files spreadsheets write.
- **Fast.** One binary, no runtime. A keystroke in a paragraph reparses in a twentieth of a millisecond; the [measurements](book/part-4/performance.org) give each number with the command that reproduces it.

## What opens as what

| File | What you see |
|---|---|
| `.org` | A document: folding, TODO states, dates, tags, tables with formulas, footnotes, citations, typeset formulas. Export to HTML, Markdown, LaTeX, PDF and text as Emacs does; Word, OpenDocument and EPUB through pandoc. |
| `.md`, `.markdown`, `.mdown`, `.mkd`, `.gfm` | A document, GitHub flavor: task lists, tables with formulas, front matter, wiki links, pictures. |
| `.tex`, `.latex`, `.ltx` | A document: numbered sections, typeset formulas, references, citations, figures; PDF builds with the errors at their lines. |
| `.csv`, `.tsv`, `.tab` | A grid: sorting, filters, a record view, column statistics. |
| `.bib` | A grid of entries. |
| `.pdf` | A viewer: pages, zoom, search, and from a LaTeX build, Ctrl-click back to the source line. |
| Pictures | A viewer for PNG, JPEG, GIF, WebP, TIFF and a dozen more, in the terminal too where it draws pictures (kitty, Ghostty, WezTerm, iTerm2, or Sixel). An SVG file, being text, opens as its XML; it shows as a picture where a document links it. |
| `.xlsx`, `.xlsm`, `.xltx`, `.xltm` | A workbook: sheets as grids with their formulas and charts, edited and saved as itself. A save asks Excel to recalculate when it opens the file. |
| `.ods`, `.xls`, `.xlsb` | A workbook converted as it opens: number formats are lost, and saving it back in its own format asks first, saying what it keeps. Save As `.xlsx` leaves the original alone. |
| Anything else | Code with highlighting. |

Around them: a file manager, projects, find in files, a command palette, Vim keys with Doom Emacs's leader, themes, English and Turkish. The viewers are plugins: WebAssembly components released by [getkalem/plugins](https://github.com/getkalem/plugins), each with its own memory and only the permissions it declares, built into the binary; `kalem plugin install` adds others from the index. The keys are Word's by default, and Vim's with Doom Emacs's leader one setting away (`editor.keymap_profile`). Not yet: the agenda, signed installers.

## Install

From a release, once 0.1 is out, on macOS and Linux:

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

## Known limitations

- A workbook protected by a password cannot be opened yet; an `.ods`, `.xls` or `.xlsb` is converted to be edited (the table above).
- Markdown: export, printing and `kalem fmt` are for Org and LaTeX; TOML front matter (`+++`) and `$$` blocks over several lines show as text. In a large file with footnotes or link reference definitions, each keystroke parses the whole file again.
- LaTeX: building a PDF needs TeX Live, MiKTeX or Tectonic installed; Kalem does not download one.
- The terminal-only build has no viewers and no plugin host, and draws an SVG picture without its text.
- Line endings and a byte order mark stay as the file has them; there is no command to change them yet.

## More

- [The Kalem Book](https://getkalem.github.io/kalem): the manual, and what Kalem does with each format. [Kalem and Emacs](book/part-4/kalem-and-emacs.org): what is taken from Emacs, and what is left out on purpose.
- [Contributing](CONTRIBUTING.md), the [design documents and task list](docs/README.md), the [changelog](CHANGELOG.md).
- License: MIT or Apache-2.0, at your option.
