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

## Accessibility

- [ ] VoiceOver (macOS), Orca (Linux) and NVDA or Narrator (Windows) read the text, follow the caret and announce the selection.
- [ ] Every command is reachable from the keyboard, through the menus or the palette.

## Terminal editor

- [ ] The terminal checks of the D14 checklist (T1.4.10): iTerm2, kitty, WezTerm, Terminal.app, Windows Terminal and a Linux VTE terminal.
- [ ] Bracketed paste of spreadsheet cells makes a table.

## Failures

- [ ] A crash writes a report to the log directory (design document, section 14) and restores the terminal.
