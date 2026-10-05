---
name: Bug report
about: Something does not work as expected
labels: bug
---

**What happened**

**What you expected**

**Steps to reproduce**

1.
2.

**A minimal file or snippet that shows the problem** (if relevant: Org, LaTeX, CSV, BibTeX, Markdown or plain text; or a PDF, a picture or a workbook a viewer opens; say which, and do not attach a file you may not share)

```

```

**What the reference does with it** (if relevant: Emacs for Org, a TeX engine or pandoc for LaTeX, a spreadsheet for CSV)

**Environment**

- Kalem version (`kalem --version`):
- Operating system:
- Frontend: graphical / terminal / command line
- Terminal emulator (for the terminal frontend):
- Plugins installed (`kalem plugin list`), if a viewer, a plugin or a language server is involved:

**The log**: attach `kalem.log`, and `crash-DATE.txt` if Kalem crashed. They are in `~/.local/state/kalem` (`%LOCALAPPDATA%\kalem` on Windows, or `$KALEM_STATE_DIR`). Look through them first: they name your files and folders. For a problem in the terminal, add the output of `kalem tui --detect`.
