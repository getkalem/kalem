# Draft: announcement to emacs-orgmode@gnu.org

Status: draft. To be sent at the end of phase 1 (task T1.8.5), when there is something people can try.

---

Subject: [ANN] Kalem: a standalone WYSIWYG editor for Org files (for your non-Emacs co-authors)

Hello everyone,

I would like to introduce Kalem, an open source (MIT/Apache-2.0) editor for Org files aimed at people who do not use Emacs. It shows Org documents formatted, the way a word processor would, and lets people edit them without learning Org syntax or Emacs.

The motivation is simple. Many of us write in Org and then have to share documents with colleagues, co-authors or students who will never install Emacs. Kalem is meant to be the tool we can point them to, while the file stays plain Org.

Some design points that may interest this list:

- Kalem does not invent any syntax and never writes its own settings into files. Files saved by Kalem differ from the originals only where the user edited them. Table alignment follows `org-table-align`, so shared files do not produce diff noise.
- The parser (`org-syntax`, a Rust crate) is lossless and is tested against `org-element-parse-buffer` on a corpus that includes the Org manual. Where the Org Syntax document is ambiguous we follow org-element and report the ambiguity here.
- In-buffer settings such as `#+TODO`, `#+TAGS`, `#+STARTUP` and `#+PROPERTY` are honored.
- It has a graphical frontend, a terminal frontend and a command line (`kalem check`, `kalem fmt`, `kalem query` with Org match strings). Export and table formulas come in the next phase.
- Elisp is not evaluated.
- Keys are Word-like by default; Vim keys are built in, and the Org mode keys of Emacs come as a keymap file.

Kalem is not meant to replace Emacs. If you use Org in Emacs, you already have the best tool. Kalem is for the people around you.

Feedback on compatibility is especially welcome: if Kalem changes something in one of your files that it should not, that is a bug.

Repository: https://github.com/kalem-editor/kalem

Thank you for Org.
