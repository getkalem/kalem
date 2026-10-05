# Release checklist

The automated tests cover the editing logic and the widgets: `cargo test`
runs the Emacs comparisons, the view model, the terminal editor on
ratatui's test backend and the graphical editor in gpui's headless test
platform. Headless tests cannot see real fonts, the system clipboard, input
methods, screen readers, file dialogs or the speed of a real window. This
list covers those by hand before a release.

Run it on macOS, on Linux (one X11 and one Wayland session) and on
Windows. Note the platform, the version and anything that fails in the
release issue.

## Installing

- [ ] On a clean machine of each platform (no Rust, no Kalem settings): the shell or PowerShell installer puts `kalem` in `~/.cargo/bin` and it runs; `kalem --version` says the release's version.
- [ ] The terminal archive (`kalem-terminal-TARGET`) unpacks and runs on a server without a display or the windowing libraries (Linux); it refuses a PDF, a picture or a workbook as not text.
- [ ] A first run with no configuration folder starts, writes nothing it does not need, and Settings creates `settings.toml`.
- [ ] The Turkish interface (`ui.language = "tr"`): menus, the palette, messages and the settings panel are in Turkish, nothing cut off.

## Start and files

- [ ] `kalem FILE` opens the graphical editor; without a display, the terminal editor.
- [ ] File > Open… shows the system dialog and opens the chosen file in a new window.
- [ ] Save As… shows the system dialog; the new name shows in the title.
- [ ] Saving keeps the file's permissions, a symbolic link stays a link, CRLF files stay CRLF, a byte order mark stays.
- [ ] Another program changes an open file without unsaved edits: it reloads within a second, and undo brings back the old text.
- [ ] With unsaved edits: the status bar warns; Save asks before overwriting; File > Revert to Saved takes the file on disk.
- [ ] Two windows on two files; closing one leaves the other.
- [ ] Quit with unsaved changes asks first.

## Display

- [ ] The body font, headings at their sizes, bold, italic, underline, strike-through, code, links and tags look right on a normal and a high-density screen.
- [ ] Settings (Ctrl+, or Cmd+,): theme, keys, text size and font apply to every window at once and persist after a restart; the file keeps its comments.
- [ ] With the theme set to System, switching the system between light and dark follows.
- [ ] Tables draw as grids; code blocks have their syntax colors; formulas show their Unicode form.
- [ ] Scrolling the Org manual (`tests/corpus/org-mode/org-manual.org`) is smooth; typing in it has no visible delay.

## Input

- [ ] Typing in both keymap profiles; menus show the keys of the current profile.
- [ ] Input methods: Japanese, Chinese and Korean compose under the caret and commit into the text, the palette and the find bar.
- [ ] Dead keys and AltGr (Linux, Windows) type accented and special characters.
- [ ] The emoji and symbols picker inserts its characters.

## Clipboard and drag and drop

- [ ] Copy in Kalem, paste into another program, and back.
- [ ] Paste a web page selection and a Word or Google Docs selection (macOS): headings, emphasis, links and lists become Org.
- [ ] Paste cells from Excel, Numbers or LibreOffice Calc: an aligned table.
- [ ] Paste as Plain Text (Ctrl+Shift+V) inserts the text as it is.
- [ ] Drag headings in the outline sidebar: before a heading, after its subtree, as a first child.

## Panels

- [ ] Command palette: fuzzy search, keys shown, commands that need an argument ask for it.
- [ ] Find and replace: matches marked, regular expressions (Alt+R) with `$1`, Replace All undone in one step.
- [ ] Insert Date: the calendar by keys and by mouse, typed dates such as `+3d` and `fri 10:00`.
- [ ] Split view: both views follow edits; each scrolls on its own.

## Markdown and CSV

- [ ] A README with a table, task list, code fences and front matter: the grid, the checkboxes and the fences drawn away from the cursor, the source with it there; a `#heading` link jumps.
- [ ] A CSV and a TSV of a few thousand rows exported from Excel and LibreOffice: the delimiter, the header and the numbers' format read right; sort, filter, and the statistics in the status bar; a cell edited and the file saved differ by that cell only.

## Viewers and plugins

- [ ] A PDF (a book), a PNG, an animated GIF, a JPEG with EXIF orientation and a large photograph open in the graphical and the terminal editor; search in the PDF; a password-protected PDF asks for its password.
- [ ] An `.xlsx` with formulas, styles and a chart: a cell edited and saved, then opened in Microsoft Excel without a repair prompt, the chart and the other sheets as they were; an `.ods` opened, and Kalem asks before saving it back.
- [ ] `kalem plugin browse` and `kalem plugin install elixir` from the index; `kalem plugin list` marks the built-in viewers; Install Plugin from GitHub… with a repository's link.
- [ ] A language server (Expert for Elixir): diagnostics, completion, hover and rename in a project.

## LaTeX

- [ ] Build PDF (F5) on a paper and a thesis from the corpus, with TeX Live on macOS, Linux and Windows and with MiKTeX on Windows: the PDF appears, the first error lands at its file and line, a missing package names its install command.
- [ ] Tectonic on a clean machine (no TeX installed): `latex.engine = "tectonic"` builds, fetching what it needs on the first build.
- [ ] SyncTeX both ways: Show in PDF opens the PDF at the page of the cursor's line, in the root file and an included one; Ctrl-click (Cmd-click on macOS) on a line of the PDF opens its source line.
- [ ] An input method inside math (Japanese or Pinyin in `\text{…}`, Turkish dead keys in a formula): the composition shows at the caret, the formula renders when the cursor leaves.
- [ ] A screen reader on a rendered document: citations, references and formulas are read as their text (the formula's source), not skipped.
- [ ] The terminal editor over SSH (`kalem tui` on a remote machine): a LaTeX file renders its headings, lists and Unicode math; formulas as images where the local terminal has graphics.
- [ ] Overleaf: a project cloned through Overleaf's Git, edited and pushed; the co-author sees no difference beyond the edit.

## Accessibility

- [ ] VoiceOver (macOS), Orca (Linux) and NVDA or Narrator (Windows) read the text, follow the caret and announce the selection.
- [ ] Every command is reachable from the keyboard, through the menus or the palette.

## Terminal editor

- [ ] The terminal checks of the D14 checklist (T1.4.10): iTerm2, kitty, WezTerm, Terminal.app, Windows Terminal and a Linux VTE terminal.
- [ ] Bracketed paste of spreadsheet cells makes a table.

## Failures

- [ ] A crash writes a report to the log directory (design document, section 14) and restores the terminal.
