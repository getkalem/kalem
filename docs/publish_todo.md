# Publish todo: what stands between `main` and a public 0.1

Written 2026-10-05 at commit `cfbb129`. This is not the roadmap
(`docs/roadmap.md`) and not the feature lists: it is the list of what is
**broken, loses data, crashes, or is documented wrongly in what Kalem
already ships**, so that the first public release works as advertised.
Nothing here adds a feature; several items remove a claim instead.

How it was made: the full test suite, clippy and rustfmt on this
checkout; the last three CI runs on `main`; the debug binary run over
`tests/corpus`, `tests/csv`, `tests/latex` and hand-made edge cases;
and a read of each mode's code, the viewers' code at the pinned plugin
revision (`getkalem/plugins@1e99e63`), the Book, the README, the CLI help
and the release workflows.

Conventions: `[ ]` open, `[x]` done. Severity: **Blocker** (0.1 does not
ship with it), **Major** (fix for 0.1, or record it as a known issue in
the README and the Book), **Minor** (fix when cheap, else leave), **Doc**
(text only). *verified* = reproduced on this machine (command or code
path given); *reported* = found by code reading, not yet run.

Work the sections in order. Section 1 is the release gate; sections 2
and 3 are what users hit first; sections 4 to 6 are text and release
plumbing and can go in parallel with the code fixes.

## 1. Release gates (green before anything else)

- [x] **Blocker, verified.** `main` CI is red on the last three completed
  runs (37274730669, 37276728208, 37277305942). Two causes:
  - The `binary size` job: the full release build is 85.4 MiB against the
    85 MiB ceiling in `tools/binary-size.txt` (terminal-only 42.3 of 42.5
    MiB, 0.2 MiB of margin). Either cut the growth (today's grid-contract
    additions) or raise the ceiling with the changelog line the script
    asks for. The 40 and 15 MiB targets of the Book stay open (R2.1).
  - `test (macos-latest)`: `ctrl_home_and_end_go_to_the_ends`
    (`crates/kalem-ui/tests/editor.rs:4712`) simulates `ctrl-end` and
    `ctrl-home`, but on macOS the document-end modifier is Command (the
    test file's own `primary()` helper, line 15). Use `primary()`.
    (done: that test and `csv_edits_as_excel_does`, which came later
    with the same keys, use `primary()`; both pass on macOS.)
  - Found on the run after the audit: Windows failed
    `sources_relative_to_their_index` (31215bf), whose test expected a
    Unix separator for an index on disk (done: joined as the code does).
  - The size (2026-10-05): the shader translator (naga), the
    accessibility bus (zbus, zvariant, atspi, accesskit_unix) and the
    reader of `.xls`/`.xlsb`/`.ods` (calamine), all cold, now built for
    size: 84.5 MiB on CI (from 85.4), the terminal build unchanged at
    42.3. `main` all green at d559c81 (run 37307808182), Windows and
    macOS included. The margins are thin (0.5 and 0.2 MiB): the next
    workbook feature passes the ceiling again.
- [x] **Major, verified.** `cargo test --workspace` is red on a developer
  machine for reasons that have nothing to do with the code, which hides
  real failures:
  - `builds_are_reproducible` (`crates/kalem-core/tests/latex_compile.rs:36`)
    fails on a TeX without the `example-image` files (BasicTeX, small
    MiKTeX): "File `example-image' not found". Skip the test when the
    package is missing, as the other TeX tests skip when pdflatex is.
  - `word_targets_and_chapters` (`crates/kalem-ui/tests/editor.rs:796`)
    and `this_file_keys` (`crates/kalem-tui/tests/app.rs:4013`) fail on a
    machine whose system language is Turkish: the tests compare English
    strings and answer prompts with `y`, but `ui.language = auto` follows
    `sys_locale` (`l10n.rs:66`). Both pass under `LANG=en_US.UTF-8`. Pin
    the language to `en` in the test harnesses.
  - Nine `#[ignore]`d tests are all benchmarks; none hides breakage.
  (done 2026-10-05: the reproducibility test skips pdfLaTeX when
  `kpsewhich example-image.pdf` finds nothing. The language flip came
  from tests that save or reload the settings: the terminal editor read
  and wrote the user's own `~/.config/kalem` there (so a test could
  change the developer's settings), and both editors' tests then
  applied `ui.language = auto`, the system's language, to the whole
  test process. The terminal `App` now has a `config_dir` (none for
  `App::with_keymap`, the user's folder for `App::new`), the graphical
  editor `kalem_ui::shared_in(config, settings_path)` with the keymap
  read beside the settings file, and the tests of both editors give a
  folder of their own whose settings say `language = "en"`;
  `custom_lists` no longer sets `KALEM_CONFIG_DIR` in `unsafe`. Both
  editors' 395 tests pass on a Mac whose system language is Turkish.
  Core paths that still read the user's folder in tests (themes,
  dictionaries, chart templates, plugins) are left as they are.)
- [x] clippy (`--workspace --all-targets`) and `cargo fmt --check` are
  clean on this checkout (2026-10-05).
- [ ] **Blocker, verified.** The index's `xlsx` component (0.0.4, tagged
  2026-10-05 00:44) cannot bind to this build: `grid.wit` changed four
  times after the tag (d062851, 1c38ecc, 35b4557, 5fddb20). The log says
  "does not have export `set-series-kind`; the bundled viewer opens its
  files". Release xlsx 0.0.5 against the final contract right before the
  tag, and do not move `grid.wit` again before 0.1 (or finally version
  the WIT package, `kalem:plugin@0.1.0` has never changed).
- [x] **Major.** The manifest's `"api": "^0.1"` is checked nowhere, and
  CI never runs the component tests against the index's artifacts
  (`xlsx_component.rs`). Add a CI job that installs each released
  component into a temporary config dir and opens a file with it.
  (done 2026-10-05: `kalem plugin check` binds every installed component
  viewer as opening a file would (`ComponentViewer::check`) and exits 1
  when one cannot run; `tools/check-released-plugins.sh` installs the
  index's component plugins into a folder of their own and runs it, from
  the *Released plugins* workflow (daily, on WIT changes, on tags, by
  hand) and before a release (`docs/releasing.md`). It is a workflow of
  its own so that a contract change does not hold up `main` until the
  plugin is released. `kalem view` now prints the fallback notice on
  standard error. Today it reports xlsx 0.0.4 as unable to run; the
  image and PDF viewers 0.0.1 run. The `api` field itself stays
  unchecked: the binding test is the stronger check, and the WIT package
  is still `@0.1.0`.)

## 2. Data loss and crashes (any format)

These break the one promise of the README ("never touches what you did
not edit") or kill the process. Fix all of them before 0.1.

- [x] **Blocker, verified (code).** A workbook edit can be silently not
  saved. "Modified" is `history_len() != saved_at` (plugins
  `xlsx/src/viewer.rs:2382`), a count, not a content check. Type in A1,
  Ctrl+S, Undo, type in B1: the document is "unmodified", Ctrl+S returns
  without writing (`kalem-core/src/document.rs:504`), closing does not
  ask, the B1 edit is lost and the file keeps the A1 edit the user undid.
  Compare content (or a saved-generation marker that Undo cannot reach).
  (done 2026-10-05, plugins 42168f3: every edit gives the workbook's
  content a new number (`Workbook::state`), undo and redo bring back the
  number of the state they return to, and the document compares it with
  the one saved; the plugin's `edits_undo_and_save` test covers the
  sequence.)
- [x] **Blocker, verified.** An `.ods` file is converted to xlsx on open
  (`kalem-core/src/viewer.rs:622-633`) and written back as a *new* ODS
  (`workbook_io::to_ods`) with only `content.xml`, `styles.xml`,
  `meta.xml` and the manifest. `kalem view --unit 2 …/libreoffice-budget.ods`
  shows the date as the text `2026-10-03T14:30:00`, the time as
  `PT09H15M00S`, 25.6 % as `0.256`. Lost on save: number formats, fonts,
  borders, row heights, hidden rows and columns, frozen panes, comments,
  validation, conditional formats, names, charts, pictures, macros. The
  loss report of `save_as_format` is dropped (`document.rs:521-523`).
  For 0.1: either open `.ods` read-only (view, Save As `.xlsx` with a
  warning that lists what is lost), or keep the original package and
  patch only `content.xml`. Say so in the README table.
  (done 2026-10-05, the part that loses data silently: editing `.ods`
  stays (the owner's E-list asked for it), but saving a file that was
  converted as it opened (`ViewerState::converted_from`) back in its own
  format is refused with `SaveError::Converted` until the user agrees,
  once a document; both editors ask with what the new file keeps and
  what it does not, and Save As `.xlsx` leaves the original alone
  (tests in both editors). ODS dates, date-times and times are read as
  numbers (plugins 42168f3), so they no longer turn into text. Still
  open: number formats (25.6 % shows `0.256`) are lost for `.ods` and
  `.xls` alike because calamine does not give them; the writer keeps
  what the prompt lists; the README caveat is section 4's.)
- [x] **Blocker, verified.** A lossy decode is saved back as U+FFFD.
  `DocumentState::save` (`document.rs:603-610`) checks `unencodable` but
  never `meta.lossy`; the GUI's Save writes even with no edits
  (`kalem-ui/src/editor.rs:1880`). Reopen with Encoding UTF-8 on a Latin-1
  file, Ctrl+S: the original bytes are gone. The message
  `msg-unencodable` ("Save with Encoding UTF-8 keeps it") and
  `book/part-2/plain-text.org:525` give the same destructive advice.
  Refuse a plain Save while `lossy` is set, and ask on Save with Encoding.
  (done 2026-10-05: `SaveError::Lossy` refuses saving over the file it
  was read from while `lossy` is set, `force` or not; the message says
  Reopen with Encoding reads it right and Save with Encoding or Save As
  write it as it shows (both explicit, Save As clears `lossy`);
  `msg-opened-lossy` and the Book no longer say Save writes �. Test
  `a_lossy_read_is_not_saved_over`.)
- [x] **Major, verified.** A CSV whose `sep=` line names a multi-byte
  delimiter aborts the process: `sep_line` slices `&rest[1..]` after one
  byte (`kalem-core/src/csv.rs:437`). `printf 'sep=ş\n…' > x.csv; kalem check x.csv`
  → "byte index 1 is not a char boundary … fatal runtime error". It runs
  from `detect`, the status bar and the index, outside any `catch_unwind`,
  and no unsaved buffer is rescued when the editor dies.
  (done: a delimiter outside ASCII makes no `sep=` line; `detect`'s doc
  comment, which sat on `sep_line`, moved back; tested.)
- [x] **Major, verified (code).** Memo caches without a document identity
  mix two open files of the same version and length:
  - CSV: `STATS` `(version, col, dialect)` (`csv.rs:2265`), `LAYOUT`
    `((version, len, dialect), view, columns)` (`csv.rs:1568`),
    `FILTERED` and `SHOWN` (`csv.rs:2141`, `2185`). Two fresh documents
    are both version 0: the status bar's Sum is the other file's, and two
    files of equal length draw one file with the other's record index,
    which can slice inside a multi-byte character and panic.
  - Markdown: `parsed` keyed by `(version, len)` (`markdown.rs:904`): two
    unedited notes of the same length (daily notes from one template)
    share a parse; Toggle Checkbox and `text[range]` act at the other
    file's offsets.
  - Also `bibtex::grid` (version only, `bibtex.rs:514`) and
    `code::pair_at_cursor` (`code.rs:119`).
  Add `doc.serial()` to every key.
  (done: the document's serial in the keys of CSV's layout, statistics,
  filter and shown lines, Markdown's parse, the BibTeX grid and the
  bracket pair; test `two_documents_do_not_share_memos` (the same
  length and dialect, both at version 0: each its own sum). Markdown's
  column widths already keyed on the text's address.)
- [x] **Major, reported.** A document toggled read-only "reloads" without
  reloading: `replace_from_disk` (`document.rs:670-704`) goes through
  `apply()`, which returns early when `read_only` is set, then records the
  new disk state and `mark_saved()`. Toggle read-only off and Save: the
  stale text overwrites the newer file with no conflict prompt. Reopen
  with Encoding has the same hole.
  (done: the reload lifts read-only for its own edit; Reopen with
  Encoding goes the same way; test `a_read_only_document_reloads`.)
- [x] **Major, verified (code).** The terminal editor's Save As writes
  over an existing file without asking (`kalem-tui/src/app.rs:4173-4191`
  → `save_as`, which forces the save, `document.rs:616-620`); no `~`
  expansion, relative paths against the process's folder. The GUI is
  covered by the system dialog.
  (done: `~` expanded, a relative name beside the document as Open
  takes it, and another existing file replaced only after
  `prompt-replace-file`; test `save_as_asks_before_replacing`.)
- [x] **Major, verified (code).** Saving a write-protected file succeeds:
  `files::write` (`files.rs:557-580`) creates a temp file and renames it
  over the target, which needs only the folder's permission. `chmod 444`,
  edit, Ctrl+S → "Saved" (and the file is still 0444). The opposite case,
  a writable file in a read-only folder, cannot be saved at all (no
  in-place fallback).
  (done: a file whose permissions say read-only is refused with
  `msg-file-read-only`; a temporary file the folder refuses means the
  file is written in place, as a hard-linked one is; test
  `permissions_on_save`.)
- [x] **Major, reported.** A component viewer is dead after one trap or
  timeout and then hides unsaved edits: `Instance::call` says the
  instance is spent (`kalem-script/src/lib.rs:409-411`), `ComponentDocument`
  never re-instantiates (`kalem-script/src/viewer.rs:450-464`); afterwards
  `modified()` returns `false` (`:612`) and `structure()` returns empty,
  which can index `structure.units[self.unit]` out of range
  (`kalem-core/src/viewer.rs:1568`, `1750`, `2345`). Re-instantiate, or
  mark the document failed and keep "modified" true.
  (done: a new instance would not have the document's edits, which live
  in the spent one; `ComponentDocument` now answers `modified` and
  `structure` with what they last were, so unsaved edits still make
  closing ask and the host keeps its units, and the failure is logged
  once. No test: no fixture traps on demand yet.)
- [x] **Major, reported.** The bundled viewers are native code with no
  `catch_unwind`: a panic in hayro, `image`, calamine or IronCalc on a bad
  file ends the editor (open runs on the UI thread,
  `kalem-ui/src/editor.rs:4015`); a panicking render thread is respawned
  on every frame, writing a crash report each time
  (`kalem-core/src/viewer.rs:779`, `798`).
  (done: opening and rendering go through `guarded` (`catch_unwind`):
  a panic is `msg-viewer-failed`, and a render that failed is
  remembered by its key and not started again; test
  `a_viewer_panic_is_an_error`. Other calls into a viewer on the UI
  thread (text, grid cells) are not guarded yet.)
- [x] **Major, reported.** Both editors close an unmodified document
  whose file was deleted on disk without a prompt, losing the only copy
  (`external_change`); the terminal editor also drops change events
  while a prompt is open (`app.rs:4475-4482`) and handles only
  `Reloaded`, not `Conflict`, on activation (`app.rs:755`). Symlinked
  files never auto-reload in the terminal (the watcher watches the
  link's folder, `files.rs:627-636`).
  (done: the editors did not close it, but closing did not ask: a clean
  text document whose file is deleted now counts as unsaved (its saved
  version kept aside and restored if the file comes back as it was), so
  closing asks and Save writes it again; test
  `a_deleted_file_leaves_an_unsaved_document`. The terminal editor puts
  back changes that arrive while a question is open, and handles every
  outcome on activation (`disk_outcome`). The watcher watches a link's
  target folder too; test `watching_a_link`.)
- [x] **Minor, reported.** `kalem fmt` and the editors re-encode legacy
  CJK files without checking `encode(decode(b)) == b` (Shift_JIS NEC
  rows, Big5 duplicates; `files.rs:504`): Ctrl+S on an unedited file
  changes bytes. Mark such files lossy at open.
  (done: `writes_back` encodes a legacy decode again at open and marks
  it lossy when the bytes differ; test
  `legacy_bytes_that_do_not_write_back` (Shift_JIS `ED 40`). The lossy
  messages say "do not read back as they are" for both cases.)
- [x] **Minor, reported.** The atomic rename drops extended attributes,
  ACLs, the creation date and a non-default group (`files.rs:557-600`
  copies the mode bits only). Say so in the Book or copy them.
  (done: the group is kept where the user belongs to it; extended
  attributes, ACLs and the creation date are named in the Book as not
  kept by the rename; files.org also corrected on renames by other
  programs (section 3.6's Doc item).)

## 3. Per format: bugs and important gaps

### 3.1 Org

What held up (checked against Emacs 30.1 / Org 9.7.11 with
`tests/emacs/dump.el`): the parser on 28 hand-made edge cases (empty
file, bare `*` at EOF, tabs, CRLF, BOM, Turkish tags, unclosed blocks),
the HTML, Markdown, text and LaTeX exports byte-identical on mixed
documents, `kalem check`'s round trip and diagnostics, a missing
`#+INCLUDE` failing as Emacs does, 10,000 nesting levels in release.

- [x] **Blocker, verified.** `kalem fmt` formats every file that is not
  LaTeX (and has no language pack) with the Org formatter
  (`kalem-cli/src/commands/fmt.rs:41-47`), code included:
  `kalem fmt --check crates/org-edit/src/format.rs tools/gen-edit-cases.py`
  lists both. In the Rust file the match arms starting with `|`
  (`| SyntaxKind::EXAMPLE_BLOCK`) would be aligned as an Org table and
  gain a closing `|`, which breaks compilation. The editor's Format
  Document is gated by `editorMode == org || latex || hasFormatter`
  (`builtin.rs:5075`); the CLI must be gated the same way. (Same root
  cause as 3.2's first item.)
  (done 2026-10-05: `kalem fmt` formats Org (with its setup files, as
  `parse_file` reads them), LaTeX and code whose language has a
  formatter; any other file is left as it is with a note on standard
  error and does not count as changed; the help says so.)
- [x] **Major, verified.** Table formulas break in CRLF files:
  `tblfm::active_line` keeps the trailing `\r` on the last formula
  (`org-table/src/tblfm.rs:161-177`) and Calc's tokenizer does not skip
  it (`calc/parse.rs:141`); recalculation then rewrites rows with bare
  `\n` (`org-edit/src/recalc.rs:114-131`).
  `kalem table recalc --check` lists `| 2 | 4 |\n#+TBLFM: $2=$1*2\r\n`
  as changed and not its LF twin; F9 and automatic recalculation in both
  editors run the same code.
  (done: the `#+TBLFM` readers drop the CR, and recalculated rows keep
  the file's CR LF, alignment included; test `crlf_tables`.)
- [x] **Major, verified.** Tag commands do not see the tags of a CRLF
  headline: `tags_start`/`heading_end` trim spaces and tabs only
  (`org-edit/src/tags.rs:32-47`, `org-model/src/properties.rs:110-121`).
  `kalem fmt --check` on `* H :a:\r\n` reports nothing while the LF
  version is aligned; from the code, Set Tags on such a line inserts the
  new tags after the `\r` and keeps the old (`* H :a:\r      :b:`).
  (done: the tag helpers' line end stops before a CR, and
  `heading_end` trims it; test `tags_of_a_crlf_heading`.)
- [x] **Major, verified (timed by the audit).** Export of a document
  with many macros is very slow and freezes the editors: `next_macro`
  re-parses the whole document for every `{{{…}}}` and `property_at`
  again (`org-export/src/macros.rs:190`, `261`, loop at `535`). Release
  build: `org-manual.org` (820 KB, 1,101 macros) takes 56 s, 1.85 s with
  the macros stripped; `org-guide.org` 1.2 s release / 13–16 s debug.
  Export runs synchronously inside the command (`builtin.rs:781-826`), so
  the window hangs for the whole time. Parse once, and run exports as a
  job.
  (done: the document is parsed once and offsets shifted across
  replacements, parsed again only when a value holds a macro or a line
  break, so the structure stays right; `property` macros read the same
  parse. org-guide's HTML export in a debug build: 17 s to 1 s, the
  same bytes; the whole Org manual: 11 s in a debug build. Export still
  runs inside the command in the editors.)
- [x] **Major, verified.** `#+INCLUDE` reads the included file with a
  plain `read_to_string` (`org-export/src/include.rs:486`) while setup
  files are normalized (`org-syntax/src/prepass.rs:270`): a BOM is
  inserted literally (the included file's first headline becomes
  paragraph text), `\r` leaks into the output, and a non-UTF-8 include
  fails with "Cannot include file" although it exists. Emacs decodes all
  three.
  (done: an included file's byte order mark is dropped, CR LF read as
  LF, bytes that are not UTF-8 read as Latin-1, as Emacs inserts them;
  setup files read the same; test `includes_files_as_emacs_reads_them`.)
- [x] **Major, verified.** `kalem query` and `kalem fmt` build the model
  with `org_syntax::parse`, not `parse_file` (`commands/fmt.rs:80-84`,
  `46`), so `#+SETUPFILE` is ignored: TODO keywords, FILETAGS and tag
  groups from a setup file are lost (`kalem query q.org '/REVIEW'` prints
  nothing where `kalem export` recognizes REVIEW; the Book's
  `org.org:465` says the model reads it).
  (done: both parse with `parse_file`; `kalem query` finds REVIEW.)
- [ ] **Minor, verified.** An undefined footnote reference exports as an
  empty footnote with exit 0 (`See [fn:9].` → `[1]` with no text);
  Emacs stops with "Definition not found for footnote 9". `kalem check`
  reports neither undefined nor duplicate footnotes; add it to the
  known-differences page or fail.
- [ ] **Minor, verified.** `kalem check` does not report a missing
  `#+INCLUDE`, a missing `#+SETUPFILE` or links to missing local files
  (exit 0; the export then fails); org-lint does, and Markdown files get
  `markdown-missing-file`.
- [x] **Minor, verified.** The CLI refuses non-UTF-8 Org files that the
  editors, `fmt` and `table recalc` open (`read_to_string` at
  `commands/mod.rs:28-30`, exit 2; see 3.3 for the same in LaTeX).
  `kalem check` on a BOM file reports column 2 on line 1 (`line_col`,
  `commands/mod.rs:204-237`). `kalem export --to org` drops the BOM
  (`export.rs:106`, `158-165`) while saying "no Kalem formatting".
  (done: the CLI reads files as the editors do (`read_doc`, through
  `kalem_core::files::read`): legacy encodings decoded and the BOM
  dropped, so the columns are right; `--to org` writes the BOM back.)
- [~] **Minor, reported.** CLI unevenness: `fmt`, `export` and `query`
  reject folders ("Is a directory"), `check` accepts them; `check` and
  `export` stop at the first unreadable file; `kalem query FILE 'TODO="'`
  (malformed) prints nothing and exits 0.
  (partly: `kalem check` reports a file it cannot read and checks the
  others. Open: folders for `fmt`, `export` and `query`; a malformed
  match string.)
- [ ] **Minor, measured by the audit.** HTML export is quadratic in the
  links of one paragraph (4,000 links 1.7 s, 8,000 7 s; md and latex
  0.1 s). A list nested 2,000 levels overflows the stack in a debug build
  (release survives 10,000); the fuzz job runs debug.
- [ ] **Doc.** `book/part-2/org.org:901-902` says "the interim Kalem
  formatting is written by the HTML and LaTeX back-ends" while 118-122
  says exports drop it as Emacs does (the code drops it everywhere).
  The missing default author (Emacs uses `user-full-name` for
  `\author`, `pdfauthor`, the HTML meta and the text title block) is not
  in `org-known-differences.org`, which contradicts the "defaults of
  `emacs -Q`" claim at `org.org:32-36`. `org.org:635` lists
  `org-change-tag-in-region` as reproduced; `org_edit::tags::change_tag_in_region`
  exists but no command calls it.

### 3.2 Markdown

- [x] **Blocker, verified.** `kalem fmt` formats Markdown, CSV and `.bib`
  files with Org's formatter (`kalem-cli/src/commands/fmt.rs:39-46`:
  everything that is not LaTeX or a language pack). On a GFM table it
  rewrites `|---|---|` as `|---+---|` (GitHub stops reading it as a
  table), strips whitespace-only lines inside fences, inserts blank
  lines before `* ` items, aligns `:tag:`.
  `kalem fmt --check tests/corpus/markdown/readmes/*.md` lists bat.md,
  ripgrep.md, tokio.md and exits 1; without `--check` it rewrites them.
  Refuse every file type `fmt` has no formatter for (the help already
  says "Org, LaTeX or code files").
  (done with 3.1's: other files are left as they are.)
- [x] **Major, verified.** `kalem export notes.md --to html|latex|pdf|docx`
  parses the Markdown as Org, prints a warning and exits 0 with broken
  output (serde.md → an HTML with no `<h*>` and no `<pre>`). Either
  route Markdown through `markdown::to_html` or fail with exit 1. In the
  editors Export and Print are Org-only by `when`; say so in the Book.
  (done: Markdown to HTML through comrak (`markdown::to_html`, raw HTML
  left out, a page around it unless `--body-only`); Markdown to any
  other format, CSV, LaTeX and code are refused with exit 1.)
- [x] **Major, reported.** Links to headings do nothing: `[x](#install)`
  returns `LinkAction::Missing` (`markdown.rs:2488-2490`), and
  `[x](GUIDE.md#section)` opens the file at the top because both editors
  use the `search` only when it is a line number
  (`kalem-ui/src/editor.rs:1809`, `kalem-tui/src/app.rs:2049`). The Book
  promises "a `#heading` after it searched for".
  (done: `#anchor` jumps to the heading whose GitHub anchor it is
  (`anchor_of`: lower case, `-` for spaces, punctuation dropped,
  repeats numbered `-1`, `-2`); `OTHER.md#anchor` opens the file at that
  heading's line; test `links_to_headings_and_encoded_names`.)
- [x] **Major, reported.** Percent-encoded destinations are never decoded
  (`markdown.rs:2484-2520`, `images.rs:22-35`): `docs/My%20Note.md` and
  `img/my%20pic.png` (as VS Code and Obsidian write them) open the
  literal path; `missing_files` decodes only `%20` (`markdown.rs:182`),
  so Turkish names give false "No such file" warnings, and Remove Unused
  Images can trash a picture linked that way.
  (done: links, pictures (`images::resolve`) and the missing-file check
  decode `%XX` when the name as written is no file; Remove Unused
  Images knows a percent-encoded name.)
- [x] **Major, reported.** Tab or Align Table on a table inside a block
  quote destroys the quote: `table_at` takes the whole line, `> `
  included (`markdown_table.rs:24-35`); `cells()` also splits at `\|`,
  where comrak does not.
  (done: a line's container prefix (indentation, `>` markers) is no
  cell and stays on every aligned line; a pipe after a backslash stays
  in its cell, as GFM splits rows; test
  `quoted_tables_and_escaped_pipes`.)
- [x] **Major, reported (freeze).** Wiki-link completion and Open Link on
  `[[…]]` walk the whole project on the UI thread on every keystroke:
  `WikiCompleter` is not marked slow (`completers.rs:614-660`),
  `project_pages` reads the first 8 KB of every file; with no project
  marker the walk covers the document's whole folder tree
  (`kalem-project/src/list.rs:96-113`).
  (done: the pages are listed by name only (no file read), hidden
  folders, `node_modules` and `target` left out, at most 50,000 entries
  looked at, the list kept five seconds; the completer runs on another
  thread (`slow`).)
- [x] **Major, gap.** Tab in a list item inserts spaces instead of
  nesting the item (Org and LaTeX have `list.indent`).
  (done: Tab nests an item with what it holds under the one before it,
  at that item's text (`markdown.list.indent`), Shift+Tab takes it out
  (`markdown.list.outdent`); test `nesting_list_items`.)
- [ ] **Minor, reported.** TOML front matter (`+++`) is read by Edit
  Properties only; the view draws it as a paragraph and a `# comment` in
  it becomes an H1 in the outline (`markdown.rs:155-157`).
- [ ] **Minor, reported.** Display math over several lines
  (`$$\n…\n$$`) is never drawn (`markdown.rs:1598-1601`); only one-line
  `$$…$$` is.
- [ ] **Minor, reported.** A picture that cannot be loaded (every remote
  badge) shows `[image: <full URL>]` instead of its alt text
  (`kalem-ui/src/line.rs:1006`, `kalem-tui/src/render.rs:415`); every
  README starts with such lines.
- [ ] **Minor, reported.** Grid view: the delimiter row is found by
  content, so a body row `| - | - |` is drawn as a rule
  (`is_delimiter_row`, `markdown.rs:1056`); misaligned rule when the
  outer pipes differ.
- [ ] **Minor, reported.** Convert to Org (`markdown_org.rs`) does not
  escape text (`\*x\*`, `/x/`, `=x=` become emphasis; `\# text` becomes
  a comment and is dropped on export; `[[Page|Title]]` is written as a
  broken Org link; a heading in a quote splits the block).
- [ ] **Minor, reported.** Any `[^` or `[x]:` anywhere (even inside a
  code block) makes every keystroke a full parse (`has_globals`,
  `markdown.rs:834`): about 1.4 s per keystroke at 10 MB. The Book's
  "10 ms at 10 MB" does not hold for such files.
- [~] **Minor, reported.** `kalem export FILE.md --to org` overwrites an
  existing `FILE.org` without asking (`export.rs:192`); Convert to Org
  and `kalem import` refuse. Table Sort is not Turkish-aware (CSV's is).
  Front-matter list values with commas are split when edited
  (`front_matter.rs:46-55`).
  (partly: `kalem export FILE.md --to org` no longer replaces an
  existing `FILE.org` without `--output`; table sort and front matter
  open.)

### 3.3 LaTeX

- [x] **Major, reported (stub latexmk).** Building an unchanged document
  with latexmk reports a failure: latexmk exits 0 without rewriting the
  `.log`, `build_inner` accepts only a log written during this build
  (`latex_build.rs:566-576`, `621-628`), so F5 twice shows "The PDF
  could not be made: latexmk: …".
  (done: latexmk that exits 0 leaving an older log is up to date: that
  log's problems and the PDF are the build's.)
- [x] **Major, reported.** LaTeX errors are lost when the file name has
  a space: `file_line_error` rejects names with a blank
  (`latex_build.rs:238`), `track` cuts at the first blank (`:260-264`).
  `my paper.tex` with an undefined macro builds "without errors".
  (done: a path (`./`, `../`, `/`) may have blanks in an error line, and
  the file stack runs a path cut at a blank on to its extension; tested
  with pdflatex on `my paper.tex`.)
- [x] **Major, reported.** Without latexmk (BasicTeX, MiKTeX without
  Perl, minimal Linux): with `--outdir` bibtex runs inside the output
  folder and cannot find `refs.bib` (`latex_build.rs:601-609`): exit 0,
  empty bibliography; `\include{chapters/intro}` fails because only the
  top output folder is created (`:542-544`); bibtex/biber output is
  discarded and a missing biber is never reported.
  (done: bibtex gets the document's folder in `BIBINPUTS` and biber
  `--input-directory`; the folders of `\include{sub/x}` are made under
  the output folder; a missing bibtex or biber is a warning; tested with
  pdflatex and `--outdir build`.)
- [x] **Major, reported.** Pictures and files are resolved from the
  *edited* file's folder, LaTeX resolves from the root's: the view
  (`latex_view.rs:6318-6328`), Insert Figure
  (`kalem-ui/src/editor.rs:1549`), completion (`latex_complete.rs:953`)
  and the missing-picture check (`latex_check.rs:691`). In the usual
  thesis layout (`main.tex`, `chapters/`, `figures/`) a chapter's figure
  does not show, and Insert Figure writes a path the build cannot find.
  (done: the view, the missing-picture check, completion and Insert
  Figure resolve from the root document's folder; tested on a chapter
  in a subfolder.)
- [~] **Major, verified.** `kalem check`, `kalem parse` and
  `kalem latex build` read with `read_to_string`
  (`kalem-cli/src/commands/mod.rs:28-30`, `606`): a Latin-1 file stops
  the whole run with "stream did not contain valid UTF-8", exit 2, and
  JSON mode prints nothing (3 of the 50 corpus papers, and
  `…/2401.00748/emlines2.sty`). A Latin-1 `.bib` is
  `bibliography-unreadable` (`org-cite/src/bib.rs:95`), after which
  unknown keys are not reported and citation completion is empty. The
  editors decode these files correctly; the CLI should use the same
  decoder.
  (done for the CLI: `read_doc` decodes as the editors do; a file it
  cannot read is reported and the others checked. The `.bib` part
  (`bibliography-unreadable` for Latin-1) is open.)
- [x] **Major, verified.** `kalem fmt --align` is not idempotent: a table
  row whose first cell is empty starts with alignment padding, which
  `step()` (`latex_fmt.rs:69-97`) reads as the indentation step, so the
  indent grows on every run (35 of 171 corpus files never settle; the
  example in the audit goes 1 → 12 → 23 spaces).
  (done 2026-10-05: `step` skips a line starting with `&`; such a row
  takes the indentation of the environment's other rows, and when no
  row has a first cell the `&` starts the line with no space before it.
  All 181 files of the corpus settle at the first run, with and without
  `--align`, which `latex_corpus.rs`'s `arxiv_papers` now checks.)
- [x] **Major, reported (confirmed with pdflatex by the audit).**
  `kalem fmt` changes verbatim output: the `\end{verbatim}` line counts
  as outside the environment and is indented (`latex_fmt.rs:105`,
  strict `<`), which adds a blank last line to the typeset listing;
  verbatim-like environments not on the fixed list (`protected`,
  `:16-44`: fancyvrb's `\DefineVerbatimEnvironment`, `pycode`,
  `luacode`) get their bodies re-indented.
  (done 2026-10-05: the line where verbatim text ends, its `\end` line,
  is kept as it is; the bodies kept are also those of fancyvrb's,
  PythonTeX's, LuaLaTeX's, SageTeX's and tcolorbox's listing
  environments and of those `\DefineVerbatimEnvironment` (and its
  kin), `\newtcblisting` and minted's `\newminted` declare, which no
  longer vote for the indentation step either. Test
  `verbatim_end_lines_and_verbatim_like_environments_kept`.)
- [x] **Major, verified (code).** `kalem fmt` runs the Org formatter on
  `.bib`, `.sty` and `.cls` (same cause as the Markdown blocker): a
  `.bib` abstract with `| x |` lines is "aligned".
  (done: such files are left as they are.)
- [x] **Major, reported.** F5 while a build runs starts a second build on
  the same `.aux`/`.pdf` (`builtin.rs:1369-1408` has no running-job
  check); `CURRENT` keeps only the newest (`latex_build.rs:519`), so
  Cancel stops the second only; build-on-save is skipped silently while
  a job runs and nothing is queued, so the PDF can be older than the
  last save. The Book says "one build at a time".
  (done 2026-10-05: Build PDF while a build runs queues the build
  (`latex_build::queue`, the last asked wins) and says so
  (`msg-build-queued`); the running job builds it when it ends; a save
  with build-on-save goes the same way instead of being skipped; Cancel
  Build forgets the queued one. Test
  `a_build_asked_for_during_one_waits_for_it`.)
- [x] **Major, verified (code).** Quick Fix for `\bf`, `\rm`, `\it` is
  offered inside math (`latex_check.rs:146-165`, no `in_math` check):
  `$\bf x$` → `$\bfseries x$`, which pdflatex rejects.
  (done 2026-10-05: in math the note names the `\math…` command to put
  around what it applies to (`latex-deprecated-font-math`) and offers no
  Quick Fix, as nothing replaces it word for word; outside math as
  before. Test `an_old_font_command_in_math_has_no_text_fix`.)
- [x] **Major (R2.6, owner's D5).** On a machine without TeX both
  editors say only "No LaTeX found: install TeX Live, MacTeX, MiKTeX or
  tectonic" (`msg-no-latex`, `latex_build.rs:540`): no link, no install
  command. `pdf::detect` (`pdf.rs:125-138`) gives the same message when
  TeX is there but the chosen engine is not (a fontspec document on a
  pdflatex-only install). `kalem latex build --format json` prints text
  in this case. TikZ pictures are always compiled with pdflatex
  (`tex_pictures.rs:218`), so they stay source in fontspec documents.
  (done 2026-10-05: `pdf::missing` tells the two apart: without TeX the
  message gives the command that installs TeX Live here (Homebrew's
  MacTeX, winget's MiKTeX, apt, dnf, pacman or zypper by
  `/etc/os-release`, else tug.org's link) and Tectonic's site; with TeX
  but not the engine, it names both (`msg-latex-engine-missing`). Every
  build path uses it (Build PDF, Org's PDF export, a workbook's print,
  `kalem export`). `kalem latex build --format json` answers with JSON
  and an `error` field. A picture is compiled with the engine its
  preamble needs (`engine_of`). Tests `what_to_install_without_tex`,
  `a_picture_takes_its_documents_engine`,
  `a_build_that_cannot_start_says_so_in_json`.)
- [ ] **Minor, reported.** `\nocite{*}` is flagged "No bibliography has
  the key @*" (`latex_check.rs:591` lacks the `*` filter of `:503`);
  fails `--deny-warnings`. The LaTeX messages use Org's `@key` spelling.
- [x] **Minor, reported.** Every "File `x' not found" gets "Install it
  with: tlmgr install x" (`latex_build.rs:286-291`), pictures and
  `\input` files included ("tlmgr install ../ch/pic").
  (done: only a package's files (`.sty`, `.cls`, `.def`, …) get it.)
- [x] **Minor, reported.** Wrapped log lines are re-joined by counting
  characters, pdfTeX wraps at 79 bytes (`latex_build.rs:148`): a
  warning about `\ref{şekil:…}` loses its line number.
  (done: 79 bytes or 79 characters; test `wrapped_lines_by_bytes`.)
- [ ] **Minor, reported.** A BOM hides `% !TEX program` and `% !TEX root`
  when read from disk (`magic_program`, `latex_build.rs:90-98`,
  `trim_start` does not strip U+FEFF).
- [ ] **Minor, reported.** A CLI path with `..` breaks root detection
  (`commands/mod.rs:602` uses `absolute`, `project.rs:229` compares as
  written): `kalem latex build ../paper/ch/one.tex` compiles the chapter
  alone.
- [ ] **Minor, reported.** Show in PDF says "no SyncTeX file: build it
  again" for every failed forward search (`builtin.rs:729-733`).
- [ ] **Minor, reported.** Citation previews in the view show raw TeX
  accents (`B\"uy\"uk`) or drop them (`cite.rs:62-100`,
  `bibstyle.rs:204-231`); the `.bib` grid's `bibtex::plain` does it right.
- [ ] **Minor, reported.** The `\numberthis` macro gives false
  `latex-label-unwritten`/`latex-label-clash` warnings; a central
  `~/texmf/bibtex/bib` library is not found (no `kpsewhich`/`BIBINPUTS`);
  `cite-unused-entry` floods a shared library (826 messages over the
  corpus).

### 3.4 BibTeX

- [x] **Major, verified (code).** `kalem check` and the top-level help
  say BibTeX files are checked; a `.bib` only goes through the Org
  parser (`commands/mod.rs:324-370`). An unbalanced brace gives exit 0
  and no diagnostics; there are no BibTeX diagnostics in the editor
  either. Either add a check (balanced braces, duplicate keys, missing
  required fields) or drop the claim.
  (done 2026-10-05: `bibtex::problems` finds an entry not closed before
  the next one, an entry without a key, a key two entries use (as BibTeX,
  regardless of case) and a field the standard styles require of the
  type (biblatex's `date` and `journaltitle` counting); `bibtex::Pack`
  serves them as BibTeX's language pack when no plugin does, so `kalem
  check` reports them (exit 1) and the status bar says them. On the
  corpus's bibliographies it finds what BibTeX warns about. Test
  `a_bib_files_problems`.)
- [x] **Major.** `kalem fmt` on `.bib` (see 3.3).
  (done.)
- [x] **Minor, reported.** Two `.bib` files open show the first one's
  grid (`bibtex::grid` memo keyed by version only, see section 2).
  (done there: the grid's memo keys on the document's serial.)

### 3.5 CSV and TSV

- [x] **Major, verified.** `sep=` with a multi-byte delimiter aborts the
  process (section 2).
  (done there.)
- [x] **Major, verified (code).** The memos shared between documents
  (section 2).
  (done there: the document's serial in every key.)
- [x] **Major, reported (large files).** Whole-file work on every
  keystroke and on every cursor line: `column_status` re-runs
  `column_stats` → `rows()` (a String per field of the whole file) on
  every version and the status bar asks every frame (`csv.rs:2263-2291`,
  `workspace.rs:2038`); the Sort View and Filter memos key on the
  cursor's line, so Up/Down re-indexes, re-filters and re-sorts the
  file; `shown_lines`'s `kept` is O(records × ranges). The grid_speed
  tests time only the first layout. Measure with a 100k-row file before
  release and fix the status-bar path at least.
  (done 2026-10-05, measured by `a_large_file_at_each_keystroke_and_step`
  in `tests/csv.rs` (100,000 rows of eight columns, a filter and a sort
  on): the filter's memo no longer keys on the cursor's line, and the
  view's records, their order and what the filter keeps are worked out
  once a version, the cursor's record added by a search (`Shown`); the
  status bar's numbers read the column's field of each record instead
  of every field as a `String`. Twenty steps of the cursor went from
  9.7 s to 38 ms in a release build (92 s to 0.3 s in a debug one), the
  status bar after a keystroke from 113 ms to 40 ms; the test fails
  past its ceilings.)
- [ ] **Minor, reported.** The dialect is detected once at first layout
  and frozen (`csv.rs:1560-1566`): a new or empty `.tsv` locks in `,`
  (detection ignores the extension, `csv.rs:457`), so Insert Column in a
  new `.tsv` writes commas; a header-only file is "no header", and after
  rows are added Sort File sorts the header into the data.
- [ ] **Minor, reported.** Enter, Shift+Enter, Tab and Shift+Tab step by
  file row and ignore the filter and the sort view (`builtin.rs:2815`,
  `1159`): with a filter on, Enter lands on a hidden row.
- [ ] **Minor, reported.** Paste misreads spreadsheet clipboard text:
  `block_rows` ignores quotes (`csv_tools.rs:381-398`), so a copied cell
  with a line break shifts the rows and keeps literal quotes; a pasted
  line with no tab is split with the file's delimiter (`Smith, John`
  becomes two cells).
- [~] **Minor, verified.** `kalem check` on CSV hard-codes
  `roundtrip = true` (`commands/mod.rs:336-353`), keeps the BOM as part
  of the first field, and does not recognise `sep=` after a BOM.
  (partly: the BOM is dropped before checking, so the first field and
  a `sep=` line after it are right; the round trip is still assumed.)
- [ ] **Minor, reported.** A regex `$` never matches before `\r\n` in a
  CRLF file (`find.rs:41-44`, no `.crlf(true)`).
- [ ] **Minor, gap.** No command to change line endings or to add or
  remove a UTF-8 BOM (`builtin.rs:4996` keeps the old BOM); the status
  bar does not show the line ending.

### 3.6 Plain text and code

- [x] **Major, reported.** Case-insensitive Find allocates a
  `Vec<(usize, char)>` of the whole text on every query keystroke and
  after every edit while the bar is open (`find.rs:90-107`,
  `panels.rs:841`, `kalem-ui/src/editor.rs:1036`, `kalem-tui/src/app.rs:3212`):
  about 800 MB per keystroke in a 50 MB file. Use a case-folded search
  without the vector (or `regex` with `(?i)`).
  (done 2026-10-05: `find_all` folds character by character as it walks
  the text, ASCII without a table, and allocates only the matches; the
  folding and the offsets are as before (`finding`).)
- [ ] **Minor, reported.** Save As does not re-detect the mode
  (`document.rs:616-620`): Ctrl+N, type Python, Save As `x.py` stays Org
  until reopened.
- [ ] **Minor, reported.** The GUI's forced Overwrite after "changed on
  disk" skips the after-save steps (no `DocumentAfterSave`, no
  `lsp::saved`, no "Saved", no build-on-save; `editor.rs:1933-1943`);
  `save_as` and `save_quietly` skip `before_save` and the events.
- [ ] **Minor, reported.** A UTF-8 file with one stray byte is guessed
  whole as Windows-1252 (mojibake everywhere); a file with a BOM and one
  invalid byte cannot be opened at all (`files.rs:188-194`).
- [ ] **Minor, risk.** Syntax highlighting runs synchronously on open and
  after each edit up to 4 MB with no per-line length cap
  (`kalem-highlight/src/lib.rs:313-403`): a 1 MB minified line is parsed
  whole. Measure with one such file.
- [x] **Doc.** `book/part-1/files.org:15-17` says documents follow
  renames "from Kalem or from the file manager"; an external rename
  (`mv`, Finder, `git mv`) shows "deleted on disk" and Save recreates the
  old path (`workspace.rs:843-875`).

### 3.7 Workbooks (xlsx, xlsm, xls, xlsb, ods)

- [ ] **Blocker.** The silent "unmodified after undo" and the `.ods`
  rewrite (section 2).
- [ ] **Major, verified (code).** Formulas typed in Kalem are written
  without Excel's `_xlfn.` prefix (nothing adds it; `calc.rs:61` only
  strips it), so `=XLOOKUP`, `=CONCAT`, `=TEXTJOIN`, `=IFS`, `=STDEV.S`,
  `=FILTER`, `=UNIQUE` show `#NAME?` in Excel after a save; dynamic-array
  formulas are written without `t="array"`/`cm` and become `@`-formulas.
  Kalem shows the right values (IronCalc accepts the names), so the user
  finds out in Excel.
- [ ] **Major, reported.** Save As to another workbook extension keeps
  the old content types (`save_as_format("xlsx")` returns the same
  package, `kalem-core/src/viewer.rs:1723`): `.xlsm` → `.xlsx` keeps
  `vbaProject.bin` and the macro content type, `.xltx` → `.xlsx` keeps
  the template type; Excel answers "file format or extension is not
  valid". `template_to_workbook` exists but is used only by New from
  Template.
- [ ] **Major, reported.** A password-protected `.xlsx` (an OLE
  container) fails with "malformed package: no end of central directory
  record" (`xlsx/src/workbook.rs:365`, `package.rs:192`); detect
  `EncryptionInfo` and say "protected by a password".
- [ ] **Major, reported.** Installing any component from the index
  replaces the newer bundled viewer (`kalem-cli/src/lib.rs:510-522` maps
  `org.kalem.xlsx` to the bundled `xlsx`): a working xlsx 0.0.4 is OOXML
  only, so `.xls`, `.xlsb` and `.ods` would stop opening; pdf-viewer
  0.0.1 and image-viewer 0.0.1 replace newer bundled code, the install
  summary does not say so, and `updates()` never notices components
  (`plugin_store.rs:858`). For 0.1: prefer the bundled viewer when it is
  newer, or keep the index's viewers in step with the binary.
- [ ] **Major, reported.** Memory: every edit pushes a `Snapshot` that
  clones every loaded sheet's XML and cell model with no limit on the
  undo stack (`xlsx/src/workbook.rs:1360-1375`; roughly 150–200 MB per
  edit on a million-cell sheet); the first edit loads the whole workbook
  into IronCalc and each edit recalculates the whole workbook
  (`calc.rs:191`); the host parses every sheet on open, on the UI thread
  (`viewer.rs:640-643`). Cap the undo depth by bytes, and measure a
  1M-cell file before release.
- [ ] **Major, reported.** Opening a formatted `.ods`, or Save As `.xlsx`
  from `.xls`, calls `change_style` once per styled run per row
  (`workbook_io.rs:382-402`), each taking a full snapshot: thousands of
  rows with currency or date formats mean thousands of whole-sheet
  copies (hang or out of memory). An `.ods` whose painted extent exceeds
  5M cells is refused ("too large to convert", `workbook_io.rs:293`),
  and one fully colored row makes the extent 1024 columns wide
  (`ods_style.rs:280-298`), so data past about row 4,900 cannot be viewed.
- [ ] **Major, reported (Turkish and EU users).** Open as Workbook turns
  decimal-comma numbers (`1,5`, `1.234,56`), `50%` and `$12` into text
  (`workbook_io.rs:112-120`, `1202-1223`): the wizard offers Turkish
  encodings and `;` but no decimal separator; it reads the file from disk
  and ignores unsaved edits (`builtin.rs:4633`); a target named
  `out.csv` gets xlsx bytes (`:4610`). In the cell editor a Turkish user
  typing `1.500` gets 1.5 (`workbook.rs:151-181`).
- [ ] **Minor, reported.** 1904-date-system workbooks get no
  recalculation (`workbook.rs:694-696`): a typed formula shows blank.
  Dynamic and CSE array results are never recalculated; the anchor of a
  dynamic array cannot be edited (E48 open).
- [ ] **Minor, reported.** A running macro blocks the UI for up to 60 s.
- [ ] **Doc.** Even for `.xlsx` a save sets `fullCalcOnLoad`, may drop
  `calcChain`, and rewrites or drops cached formula results: say so in
  the Book's (missing) workbook chapter. Unedited parts are copied byte
  for byte; VBA, pivot caches, external links, printer settings, custom
  XML survive (checked).

### 3.8 PDF

- [ ] **Major, reported.** A PDF with a user password cannot be opened:
  the plugin answers `PASSWORD_REQUIRED` and offers
  `open_with_password` (`pdf-viewer/src/lib.rs:41-43`, `79-93`), nothing
  in the host calls it and the WIT `open` has no password. The user sees
  "protected by a password" and no prompt (T3.7.3).
- [ ] **Major, reported (memory).** Six cached renders of up to 160 MB
  each (`pdf-viewer/src/lib.rs:35-39`) approach the 1 GB component limit
  past about 450 % zoom on a Retina display; with the "dead after one
  trap" bug of section 2 that kills the document.
- [ ] **Minor, reported.** Any link target with `://` opens with the
  system without confirmation (`kalem-ui/src/viewer.rs:429-433`), so a
  `file:///…/x.app` or `smb://` link in a PDF launches on one click;
  GoToR and Launch links do nothing; outline entries jump to the page
  top, not the position.
- [ ] **Minor, reported.** While neighbouring pages render ahead they
  hold the document lock, and `text_hit`/`link_under` use `try_lock`
  (`viewer.rs:788-795`, `1099-1137`): a drag over text pans instead of
  selecting and the hover cursor misses links. Find builds a new
  `InterpreterCache` per page per call (`pdf-viewer/src/lib.rs:186-200`),
  so search is slower than the plugin's README says.
- [ ] **Minor, gap.** Save As on a PDF or picture fails with "This
  format is not edited" (offer a copy instead).

### 3.9 Pictures

- [ ] **Major, reported (memory).** An animated GIF, APNG or WebP is
  decoded whole at full size before the 40 MP budget applies
  (`image-viewer/src/lib.rs:303-338`; `GifDecoder::new` has no limits):
  a 600-frame 720p screen recording needs about 2.2 GB of RGBA.
- [ ] **Major, verified (code).** The README lists SVG as a viewer
  format; `viewer::for_file` (`viewer.rs:156-161`) returns `None` for
  anything that is text, so an SVG opens as XML source. Either add a
  "View as Picture" command for SVG or drop SVG from the README row.
- [ ] **Minor, verified (Cargo.toml).** resvg is built with
  `default-features = false` (`image-viewer/Cargo.toml:22`,
  `kalem-core/Cargo.toml:30`), which drops `text` and `raster-images`:
  SVG text labels and embedded rasters are never drawn (inline pictures
  in Org and Markdown too).
- [ ] **Minor, gaps.** HEIC and AVIF (phone photos) are not supported;
  multi-page TIFF shows page 1 only; ICC profiles are reported but not
  applied; `kalem view --to png book.xlsx` writes a 1×1 PNG and exits 0.

### 3.10 Plugin host and `kalem plugin`

- [ ] **Major.** The three items of sections 1 and 2 (unbindable xlsx
  0.0.4, dead instance after a trap, bundled viewers replaced).
- [ ] **Minor, reported.** The plugins README says releases are signed;
  Kalem checks only the index's SHA-256, which comes from the same
  mutable `main`-branch `index.json` that names the download. Say
  "checksum from the index" until signing exists, and consider pinning
  the index to a tag for 0.1.
- [x] Checked and fine: downloads verified against the SHA-256,
  archive unpacking rejects `..`, absolute paths and links and caps the
  size, the component path and id are validated, removal does not follow
  symlinks.

## 4. Documentation to fix (README, Book, CLI help, docs/)

Wrong or stale text a first reader meets. Each is a text change.

- [ ] **README.md**
  - Line 46 and `book/part-1/installing.org:1`: "Rust 1.88 or later" is
    false. `cargo metadata` gives `rust_version` 1.96.0 for wasmtime
    49.0.2 and cranelift-codegen 0.136.2, 1.92 for hayro 0.7.1; the
    workspace's `rust-version = "1.88"` is wrong too, and the CI job
    "minimum supported Rust version" installs 1.88 but `rust-toolchain.toml`
    overrides it to stable (the job's log says so), so it has never
    tested 1.88. Set `rust-version = "1.96"`, both texts, and run the
    job with `RUSTUP_TOOLCHAIN=1.96`.
  - Line 26: SVG is not viewed (3.9). Line 27: `.xlsx` row should read
    `.xlsx .xlsm .xltx .xltm .xls .xlsb .ods` (`xlsx/src/viewer.rs:104`),
    with the `.ods`/`.xls` caveat of section 2; the CSV row should
    mention `.tab`; LaTeX `.latex .ltx`; Markdown `.markdown .mdown .mkd`.
  - Line 30: "The viewers are plugins: WebAssembly components with their
    own memory and only the permissions they declare, bundled into the
    binary" is false for the bundled viewers, which are native code
    (D28) with no sandbox, no memory limit and no panic isolation; the
    components are what `kalem plugin install` adds. Say that.
  - Line 3 and 11 ("never touches what you did not edit", "no import, no
    save as") need the workbook caveat until section 2 is fixed.
  - Line 26 and 3: "in the terminal too where it draws pictures", "One
    program, in a window or in a terminal" against the terminal-only
    release archive, which is built with `--features tui` alone
    (`release-terminal.yml`) and refuses PDF, pictures and workbooks
    (`installing.org:28-31`). Say which archive has what.
  - Missing: the default keys are Word-like (`settings.rs:211`); where
    settings, logs and crash reports live (`~/.config/kalem` on every
    Unix including macOS, `%APPDATA%\kalem`; `~/.local/state/kalem`,
    `%LOCALAPPDATA%\kalem`; `KALEM_CONFIG_DIR`, `KALEM_STATE_DIR`,
    `KALEM_LOG`); that the installers put `kalem` in `~/.cargo/bin`
    (`install-path = "CARGO_HOME"`) and how to uninstall; that the full
    Linux binary links the windowing libraries (a headless server needs
    libxkbcommon, Wayland, fontconfig and so on even for `kalem tui`, so
    it should take the terminal archive); glibc 2.35 (ubuntu-22.04
    runners); macOS Gatekeeper for an unsigned app; a contact address
    (CODE_OF_CONDUCT.md:40 points at one "in the README"; R2.10).
  - The from-source instructions lack `git clone … && cd kalem` and
    `--locked`. (The 400 MB Zed fetch is gone: gpui comes from crates.io
    since 54c9427, R2.7.)
- [ ] **CLI help** (`crates/kalem-cli/src/lib.rs`): `parse` (line 200)
  and `check` (204) and the errors at `commands/mod.rs:155`, `193` still
  name a "Kalem" file type, removed on 2026-10-04; `cmd-edit-repairDocument`
  is an orphan in both `.ftl` files. `fmt` says "Org, LaTeX or code
  files" but formats everything (3.2). `check` says BibTeX is checked
  (3.4). `latex-coverage`, `dump` (whose only format is
  `tests/emacs/dump.el`'s), `diff-emacs` and `diff-pandoc` are
  development tools shown to users: `#[command(hide = true)]` or a
  `kalem dev` group. `view` cites "(design §11.13)". `lsp`'s line omits
  `ask`; `query`'s usage renders as `<FILE... MATCH> <FILE... MATCH>...`;
  `kalem gui --help` says "(or the last session)" but
  `editor.restore_session` defaults to false. `--engine` help omits
  `tectonic`, which the code takes (`pdf.rs:24-31`). No ENVIRONMENT
  section for `KALEM_CONFIG_DIR`, `KALEM_STATE_DIR`, `KALEM_LOG`.
- [ ] **The Book**
  - `book/part-4/overview.org:3-7, 19-20`, `book/part-4/plugins.org:1-12`
    and `book/appendices/glossary.org:15` say the plugin runtime "is not
    built yet", "everything in this chapter is planned", getkalem/plugins
    is a "skeleton only". The runtime ships, `kalem plugin install`
    works, the repository holds four plugins. `plugins.org:20` ("a
    plugin that fails repeatedly is disabled") is not implemented for
    viewers.
  - `book/part-5/decisions.org`: D31–D53 still read "*Decided* … RFC 0003",
    D52 "Typst embedded as the default", D53 "Part III is the home of the
    Kalem format"; D21, D24, D59 mention the format. Mark them withdrawn
    (owner, 2026-10-04). `docs/design_document.md` lines 177, 193, 203,
    486 describe `.klm` in the present tense under a removal banner.
  - `book/part-5/decisions/D30-latex-subset.org` is not in `index.org`
    (never published; `decisions.org:34` names it as text).
  - Folders `part-4` and `part-5` hold Parts III and IV; there is no
    `part-3`. Rename before the URLs are public, or accept it.
  - No Part I chapter for the PDF, picture and workbook viewers (keys,
    passwords, what a save keeps, `.ods`/`.xls` conversion) and none for
    Markdown files; `book/part-1/the-command-line.org` omits `export`,
    `import`, `view`, `plugin`, `lsp`, `book`, `table` and lists the dev
    tools instead; lines 3 and 10 still say "Kalem" files.
  - `book/part-2/markdown.org:30-46`: the oracle section says 632 of 648
    CommonMark examples and "sixteen" differences; the test runs
    CommonMark 0.31.2 (652 examples, all must pass with core options,
    639 with extensions) and "The specification in CI" is done
    (`ci.yml:80-90`). The `#heading` promise, the display-math and the
    TOML front matter sentences overstate the code (3.2).
  - `book/part-2/csv.org`: 255 says a lone CR ends a record (the scanner
    treats it as data, `csv.rs:74-80`); 257 says the header is "not
    settable yet" (`csv.toggleHeader` exists, and line 187 says so);
    349-354 says Enter inserts a line break (Enter is `csv.cellBelow`);
    172-182 describes a float test the code does not use (`number()`);
    136-150 omits the first delimiter criterion (fewest malformed
    fields); 607 and 741 say a source-view paste inserts text as is (a
    normal paste still converts tabs to the delimiter, `document.rs:938`);
    823 understates the per-cursor-move rescans (3.5).
  - `book/part-2/bibtex.org`: 129-131 and 365 say `@string` and `#` are
    not expanded (they are, `cells_with` → `expand`; the `tug # { 1}`
    example is stale); 366 says one error rejects the whole file (only
    the entry is skipped, `org-cite/src/bib.rs:90`); 370 says `\'{\i}`
    is not combined (it is).
  - `book/part-2/latex.org`: 1508 says bibliographies resolve from the
    edited file's folder (the root's, `latex_view.rs:338`); the
    diagnostics table at 1192 omits `latex-label-clash`,
    `latex-label-unwritten`, `latex-label-before-caption`,
    `bibliography-entry-skipped`; 1389 and 1479 omit `tectonic`; 1309
    says verbatim is untouched (3.3); 1349 says one build at a time
    (3.3); `latex-files.org:32` says pictures are found "as LaTeX finds
    it" (3.3).
  - `book/part-1/settings.org:12-14` says to restart after hand edits
    (Reload Settings and Keys exists); line 16 links
    `../appendices/settings.org`, which exists only on the built site.
  - `book/part-2/plain-text.org:525` and `msg-unencodable` give the
    destructive encoding advice (section 2).
  - `book/part-5/performance.org:30,35` gives 83.3 and 40.8 MiB; CI
    measures 85.4 and 42.3. The Book is published from `main` only
    (`index.org:21-23`), with no version banner and no link back to the
    repository or the releases: after 0.1 it describes unreleased code.
  - The three plugin READMEs are stale (xlsx "not yet bundled", pdf
    "search reads every page once", image "text needs fonts").
- [ ] **docs/**
  - `docs/README.md:9` lists `excel_todo.md` and `excel_todo2.md` but
    not `excel_todo3.md`; R1.6 is ticked although only `todo_old.md`
    moved to `docs/history/`. Move the finished lists.
  - References to the superseded `docs/todo.md` as the task list:
    `rfcs/README.md:17`, `.github/pull_request_template.md:5`,
    `book/part-5/design-documents.org:11`, `book/part-4/overview.org:7`,
    `plugins.org:9`, `kalem-and-emacs.org:33`, `part-2/org.org:959`,
    `csv.org:51`, `bibtex.org:30`, `markdown.org:441`.
  - `docs/announcements/org-mailing-list-draft.md:11,21` says "Markdown
    is next" and "Export and table formulas come in the next phase";
    both shipped.
  - `docs/release-checklist.md` has no step for the installers and the
    terminal archives on a clean machine, the three viewers and a
    workbook round trip opened in Excel afterwards, `kalem plugin
    install`, the Markdown and CSV grids, language servers, the Turkish
    UI, a first run with no config directory. Add them before R2.4.
  - `CONTRIBUTING.md:45` says "published as `kalem-editor`" (nothing is
    published); its Setup omits the Linux libraries, the wasm32 target,
    `wasm-tools` and the CommonMark/GFM spec download that CI performs,
    without which some tests pass without checking.
  - The bug template does not ask for `kalem.log` or `crash-DATE.txt`
    (which `when-something-goes-wrong.org:14-18` asks users to attach)
    and does not mention viewers, plugins or language servers.
  - `misplaced doc comments` in `markdown.rs` (around 1966, 2382, 2589):
    rustdoc shows the wrong text for three functions.

## 5. Release machinery

- [ ] **Blocker, verified (config).** No `[profile.dist]` in
  `Cargo.toml` and none mentioned in `release.yml`. cargo-dist 0.28
  builds with `--profile dist`; only `dist init` adds the profile and
  `docs/releasing.md:7-10` says to run `dist generate`. Add
  `[profile.dist] inherits = "release"`, point the `binary size` job at
  it, and run `dist plan` and `dist build` locally once (cargo-dist is
  not installed on this machine).
- [ ] **Blocker, verified.** The Unreleased section of `CHANGELOG.md` is
  136,534 characters and repeats every heading (Removed 9/444, Added
  12/76, Changed 17/448, Fixed 29/464). cargo-dist takes the release
  body from it: over GitHub's 125,000-character body limit, and over the
  128 KiB environment-variable limit `release.yml` passes it through.
  Write a short `## [0.1.0]` (the net state, not the added-then-removed
  history of the Kalem format, which lines 21, 209-211, 223-224, 301,
  304-305, 357, 446, 453, 456, 529-530 still describe) and move the
  history to `docs/history/`. Line 163 says "Part V"; 461 cites
  `docs/performance.md`, which does not exist.
- [ ] **Blocker.** The Release workflow has never run (`gh run list -w
  Release` is empty): `dist plan` is unvalidated and
  `aarch64-unknown-linux-gnu` and `x86_64-apple-darwin` have never been
  built in any CI. Push a prerelease tag (`v0.1.0-rc.1`) first.
- [ ] **Major.** The terminal-only archives are never attached:
  `release-terminal.yml` triggers on `release: published`, but
  `release.yml` creates the release with `GITHUB_TOKEN`, whose events do
  not start workflows. Wire it as a dist `post-announce-jobs` entry with
  `workflow_call`, or document the manual dispatch. Its Windows build
  also lacks `+crt-static` (dist's default), it publishes no checksums,
  and it uses `.tar.gz` where dist uses `.tar.xz`.
- [ ] **Major.** `installers = ["shell", "powershell", "homebrew"]` has
  no `tap` and no `publish-jobs`; `getkalem/homebrew-tap` does not exist,
  so the generated release notes will say `brew install kalem-editor`,
  which fails. Drop `homebrew` for 0.1 or create the tap and the
  `HOMEBREW_TAP_TOKEN` secret.
- [ ] **Major.** `docs/releasing.md:19` says the workflow "makes a draft
  GitHub release"; `release.yml` runs `gh release create` without
  `--draft`. Step 1 omits `Cargo.lock` (the builds use `--locked`) and
  the 22 workspace-dependency `version` fields; the workspace is still
  0.0.1 (`kalem --version` prints "kalem 0.0.1"). Step 4's
  `git push --tags` pushes every tag (use `git push origin v0.1.0`).
  Missing steps: `dist plan`, green CI on `main`, updating the README
  ("once 0.1 is out") and `installing.org` ("no releases yet"), the
  terminal build, releasing the plugins (section 1).
- [ ] **Major.** Dependabot bumps the actions in the generated
  `release.yml` (its `@v4` actions already log Node 20 warnings), and
  dist refuses a hand- or bot-edited workflow without
  `allow-dirty = ["ci"]`. Exclude it or set `allow-dirty`.
- [ ] **Minor.** `packaging/macos/Info.plist`: bundle id
  `io.github.kalem-editor.Kalem` does not match the `getkalem` org (the
  id is sticky), no icon, and it declares only Org, text, Markdown and
  CSV (not `.tex`, `.pdf`, `.xlsx`, pictures). No workflow builds or
  attaches the app.
- [ ] **Minor.** `publish = false` on every crate that cannot be
  published (kalem-core depends on the comrak fork and includes
  `docs/keymaps/emacs.json` and `assets/kalem.svg` from outside the
  crate; kalem-cli on the git plugins; org-edit declares a `README.md`
  that does not exist; org-syntax waits on D18). Today only
  gpui-rich-text, latex-syntax and latex-model have it.
- [ ] **Minor.** `.github/ISSUE_TEMPLATE/config.yml` has no
  `contact_links`, Discussions are off, the repository description is
  empty; no `SECURITY.md` although `kalem plugin install` fetches and
  runs code from URLs (R5.11 puts it after 0.1; a one-paragraph file
  before is cheap).

## 6. Licensing

- [ ] **Major.** No third-party notices ship with the binaries: the
  archives carry README, CHANGELOG and the two LICENSE files only
  (`release-terminal.yml`'s `cp`, dist's defaults), while the binary
  embeds a few hundred MIT/Apache/BSD crates, the KaTeX fonts (OFL-1.1,
  licence must accompany), hayro's Foxit base-14 fonts and ICC profiles
  (their own licences), the CSL styles (CC BY-SA 3.0), the bat and
  Sublime syntaxes. Add `cargo about` (or `cargo deny`) and a
  `THIRD-PARTY-LICENSES` file to both archives.
- [ ] **Major, owner (D18).** `crates/org-syntax/src/tables/entities.rs`
  is generated from GPL-3.0-or-later `org-entities.el` and ships in the
  MIT/Apache binaries. `releasing.md:29` and the D18 page treat it as a
  crates.io question only; it is a binary-distribution question first.
  Decide D18 (regenerate the table from the Org manual's public list, or
  state the provenance and licence) before tagging.
- [ ] **Minor.** `tests/corpus/LICENSES.md`: no rows for `model/*.org`,
  `tables/*.org`, `latex/synthetic/*.tex`, the fetch scripts, `tests/csv`
  and `tests/latex` (CONTRIBUTING.md:36 requires them); the 51 arXiv
  rows after "## Extended corpus" have no header row, so GitHub renders
  them as a paragraph; the arXiv folders contain 22 third-party class
  and style files (acmart.cls, natbib.sty, fancyhdr.sty, mnras.cls,
  sn-jnl.cls, pnas-new.cls, aaai25.sty, cvpr.sty, wlscirep.cls, …) under
  LPPL or publisher terms, not the papers' CC BY; CC BY attribution
  wants the authors' names, the register gives only arXiv ids. The
  non-redistributable sample stays the draft release `arxiv-sample-2024`.
- [ ] **Minor.** `book/appendices/licenses.org` omits the Foxit fonts,
  ICC profiles and CMaps, the comrak and ironcalc forks, gpui, wasmtime,
  the Vim digraph table (`vim/digraphs.txt`, no stated provenance), the
  Markdown READMEs, the Foam docs, the math corpus and the arXiv papers.

## 7. Known limitations to state in the README for 0.1 (not to fix)

Decide which of these ship as written limitations rather than fixes;
each needs one sentence in the README's table or a "Known issues" list,
and the matching Book sentence.

- `.ods` and `.xls`/`.xlsb` are converted, not edited in place (until
  section 2 is fixed).
- The bundled viewers are native, not sandboxed; the sandbox applies to
  installed components.
- SVG opens as XML; SVG text is not drawn in inline pictures.
- A password-protected PDF or workbook cannot be opened.
- Markdown: Export, Print and `kalem fmt` are Org and LaTeX only;
  heading links do not jump; TOML front matter is not folded.
- LaTeX: building needs a TeX installation (D5); without latexmk,
  `--outdir` and bibliographies do not mix; figures are resolved from
  the edited file's folder.
- BibTeX files are shown as a grid but not checked.
- The terminal-only archive has no viewers and no plugin host.
- Line endings and the BOM cannot be changed from inside Kalem.
- Big files: a case-insensitive search, a CSV status bar and a Markdown
  file with footnotes do whole-file work per keystroke (M4 fixes it).
