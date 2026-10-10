# Roadmap and task list

The plan from `docs/evaluation-2026-10.md` (2026-10-04). It replaces
`docs/todo.md` as the live list; the old list stays as the record of what
was done and why, and its item ids (`T2.7h.24`, …) are cited here where a
new item continues an old one. The design decisions (`D29` small core, the
mode contract of §11.11, the round-trip rule, terminal parity) stand.

Conventions: `[ ]` open, `[~]` partly done, `[x]` done, with a `done:`
note. Effort is S (under a day), M (days), L (a week or more). Each
milestone has exit criteria; a milestone is not done while one is unmet.
Items are in the order to do them inside a milestone.

The order of the milestones is the point of this plan: **green, then
releasable, then the debt that every feature would copy, then
performance in CI, then features in the order users ask for them.**
Widening before M1–M3 are done repeats the week that produced the
evaluation's findings.

## Decisions the owner holds

Items marked `(owner)` below wait on one of these; nothing else should.

| | Decision | Blocks |
|---|---|---|
| D5 | Download tectonic on demand, or require a TeX installation | R2.6, T2.3.7 |
| D7 | The custom domain of the Book and the one-page site | R2.9 |
| D18 | Provenance of the Org entity table | publishing `org-syntax` |
| D21 | Phase-2 order; `A1` spreadsheet notation in Org tables | R5.x order |
| D22 | PDF without TeX (print to PDF, bundled renderer, or Typst) | R6.4 |
| D23 | A spike on leaving gpui, or staying | R3.9, group 10 of the old list |
| — | Typst as a plugin now, or after the plugin host matures | R6.7 |
| — | Maintainer contact address (T0.1.9) | R2.10 |

## M1 Green main (first)

Exit: `main` has a completed, green CI run on every push for a week;
the Windows job passes; no plugin pin can break `main`.

- [x] R1.1 CI completes on `main`: keep `cancel-in-progress` for pull
  requests, drop it for `main` (`cancel-in-progress: ${{ github.event_name == 'pull_request' }}`),
  and add a 90-minute timeout to every job. Then watch the first ten
  runs. S
  (done 2026-10-04: `cancel-in-progress` only for pull requests, `timeout-minutes: 90` on every job. Runs on `main` now complete; the first all-green one is c5b8209, run 37215001817. A newer push replaces only the pending run, so `main` gets a finished run every hour or so while two sessions push. What stays open is M1's exit: a week of green.)
- [x] R1.2 Windows tests: `lsp_service` and `plugin_install` fail on
  every Windows run; read the logs, fix the path or process handling,
  make the fake server start on Windows, and mark nothing `#[ignore]`. M
  (done 2026-10-04: four causes, each fixed: the plugin cache was mapped, so Windows refused to rewrite it while in use (and Linux got SIGBUS) — now read into memory; `canonicalize` gives verbatim paths where `..` is a name, so the plugin file check missed it — refused as a name too; the fake language server's verbatim URIs did not match the client's — URIs normalized to one spelling (`kalem_lsp::uri::normalize`), which also covers servers writing `file:///c%3A/`; the plugin test wrote a Windows path into a TOML basic string, where `\U` is an escape — a literal string now, and `file:///C:/` index URLs are read. A second round found three more: the drive letter was upper-cased as its code (`file:///67:/`); a verbatim path does not split on `/`, so `..` after one passed the check; and the test built its `..` path with `Path::join`, which on Windows resolves it before the plugin sees it. Also a PDF drag-selection race that showed on Linux CI only. Windows has passed on `main` since c5b8209.)
- [x] R1.3 Plugin pins cannot break `main`: the three `getkalem/plugins`
  revisions move together, in one `[workspace.dependencies]` entry; a CI
  job builds the plugins against this checkout's `kalem-viewer` contract
  on every push, so a contract change that is not matched by a plugin
  bump fails the pull request rather than `main`. S
  (done 2026-10-04: the three plugins at one revision of getkalem/plugins, checked by `tools/check-plugin-pins.sh` in CI's rustfmt job; the plugins were already built against this checkout's `kalem-viewer` through the `[patch]`, so every test job catches a contract change without its plugin bump. What let that reach `main` was the cancelled runs, fixed by R1.1.)
- [x] R1.4 Two sessions, one `main`: a push cadence rule in CONTRIBUTING
  (rebase on `origin/main`, run the changed crate's tests, push at most
  once an hour unless CI is green), and a `tools/pre-push.sh` that runs
  fmt, clippy on the changed crates and the quick tests. S
  (done 2026-10-04: CONTRIBUTING's "Pushing to main"; `tools/pre-push.sh` runs fmt, the plugin pins, and clippy and tests for the crates changed since `origin/main`, the whole workspace when the manifest or lock file changed; it can be linked as the git pre-push hook.)
- [x] R1.5 Stale pages fixed so the Book and the code agree:
  `book/part-2/latex.org` "Limits and known gaps" (SyncTeX, the PDF panel,
  `\multirow`, the corpus are done), `performance.org` binary sizes (48.9
  MB full and 49.7 MB terminal-only measured, 13 and 7 MB written), `todo.md` T4.3.2 (hayro, not pdfium). S
  (done 2026-10-04: LaTeX's "Limits and known gaps" rewritten from the code (corpus, SyncTeX, `\multirow` and Overleaf done; the PDF not yet a panel; the 10 MB keystroke); `performance.org` binary rows corrected and found worse than the evaluation said: the terminal-only build is 49.7 MB against 15 MB, the full 48.9 MB (D28) against 40; the evaluation corrected; `todo.md`'s pdfium mentions annotated.)
- [x] R1.6 Repository hygiene: move `docs/todo_old.md`, `excel_todo.md`,
  `excel_todo2.md` under `docs/history/`; move `spikes/` out of the tree
  or into a `spikes` branch; the gpui git revision in one place in
  `[workspace.dependencies]` (it is in six). S
  (done 2026-10-04: `todo_old.md` under `docs/history/`, its links updated; gpui and `gpui_platform` at one revision in `[workspace.dependencies]`, the lock file unchanged. Kept where they are, on purpose: `spikes/`, the evaluation code cited by the decision records D2, D3, D4, D14 and D28 and holding the Markdown specifications the conformance test reads; `excel_todo*.md`, the spreadsheet viewer's working lists, updated every few minutes by the session working on it — `docs/README.md` now lists them. On 2026-10-06 the two finished ones, `excel_todo.md` and `excel_todo2.md`, moved under `docs/history/`.)

## M2 Release 0.1 (installable)

Exit: a tagged `v0.1.0` with binaries for macOS (arm64, x86-64), Linux
(arm64, x86-64) and Windows from a release workflow; a reader of the
README installs it in two commands; the release checklist run once on
each platform with the results in the release issue.

- [~] R2.1 Binary size under the target: measure the contribution of
  each bundled plugin and of Wasmtime (`cargo bloat`), then take the
  cheapest of: `panic = "abort"`, `opt-level = "s"` on cold crates (the
  decoders, Wasmtime, fonts), the viewers as features that the full
  build turns on, a dependency audit of the 101 crates present twice.
  The terminal-only build (49.7 MB) carries the viewers and Wasmtime
  too; its target is the harder one.
  Target: full build under 40 MB, terminal-only under 15 MB, both
  recorded by a CI step that fails over the target. M (T2.9.11, D28)
  Measured 2026-10-04 (Linux x86-64, stripped): the terminal-only build
  was 62.5 MiB with the viewers and Wasmtime (the workbook viewer and
  its engine had grown it since D28). Done: the viewers and the plugin
  host are features (`viewers`, `plugins`) the full build turns on and
  the terminal-only build leaves out (40.8 MiB; 60.3 MiB with them);
  Wasmtime, Cranelift, the workbook engine and the citation styles at
  `opt-level = "s"` (2 MiB). Tried and left: every dependency at `"s"`
  (4 MiB more, unbenchmarked), unifying the crates present twice (small
  ones). Not taken: `panic = "abort"`, which would end the program on a
  plugin handler's panic that `catch_unwind` now contains. CI's
  `binary size` job holds both builds to ceilings
  (`tools/binary-size.txt`), lowered as sizes drop; the full build is
  83.3 MiB on Linux. What remains of the
  terminal-only 40.8 MiB: 25 MiB of code (kalem-core 4 MiB, std 3 MiB,
  the citation styles 2 MiB, the exporters, the LaTeX pictures' PDF and
  image decoders) and 10 MiB of data (the syntax definitions). The
  15 MiB target needs one of those left out of the terminal build, or
  a new target: the owner's call.
- [x] R2.2 Crash debt (T1.8.11, TS.10): turn on `clippy::unwrap_used`
  and `expect_used` for `kalem-core`, `org-edit`, `kalem-ui` and
  `kalem-tui` with `#[expect]` on each justified site; replace the 313
  `.expect(` and 32 `panic!(` on keystroke and command paths; the
  when-clause `expect` at startup (`builtin.rs:34`) becomes a test. The
  crash report (design §14) is checked by a test that panics on purpose.
  M
  Done 2026-10-04: the four crates warn on `unwrap_used`, `expect_used`
  and `panic` (tests allowed, `clippy.toml`). Of the 153 sites outside
  tests, 45 edits became `Transaction::edit` (a release drops a
  transaction whose edits overlap instead of crashing), 29 poisoned
  locks are used as they are, the built-in commands' keys and
  when-clauses are checked by `every_builtin_parses`, most of the rest
  became `let … else`, and 13 invariants carry `#[expect]` with the
  reason. `kalem-core/tests/crash_report.rs` panics on purpose.
- [~] R2.3 `release.yml` from `dist generate`, a `v0.1.0` tag, release
  notes cut from the Unreleased section of the changelog, which becomes
  `## 0.1.0`. The `kalem gui --help` bug (T1.8.12) fixed first. S
  (T1.8.1)
  Done: T1.8.12; `release.yml` from `dist generate` with a runner per
  target (`ubuntu-20.04` is retired, gpui is not cross-compiled) and the
  windowing libraries; `release-terminal.yml` attaches the terminal-only
  archives. The prerelease `v0.1.0-rc.1` (2026-10-06) ran the release
  workflow end to end, all 15 jobs green. Open: the tag `v0.1.0` and the
  changelog's `## 0.1.0`, after R2.4.
- [ ] R2.4 The release checklist run by hand on macOS, Linux (X11 and
  Wayland) and Windows, results in the release issue, each failure
  either fixed or recorded as a known issue in the README. L, owner's
  machines (T1.5.20, T1.4.10, T1.5.9a, T2.7h.37)
- [~] R2.5 README rewritten to what Kalem is (T2.10.11, T1.8.7): the
  positioning of D21; "works today" generated from the mode table; PDF,
  pictures and Excel mentioned; the install commands; a GIF of both
  editors on the first screen; the speed claims point at the CI
  benchmarks of M4 rather than at a hand measurement. M
  Done: the viewers and plugins, the install commands, the speed claim
  pointing at the performance page. Open: the GIF (a recording of both
  editors), "works today" generated from the mode table, and the M4
  benchmarks.
- [ ] R2.6 TeX on a clean machine (owner, D5): either tectonic downloaded
  on demand with a prompt, or a clear "install TeX Live or MiKTeX"
  message with the link, in both editors. S after the decision
- [x] R2.7 Building without Zed's repository (T2.8.6): pin to a
  `gpui-unofficial` snapshot or vendor the two crates; CONTRIBUTING says
  how large the clone is until then; a CI guard fails a pull request that
  adds a `zed-industries` git dependency. M
  (done 2026-10-05: gpui and `gpui_platform` are `gpui-unofficial` and
  `gpui-platform-gpui-unofficial` 1.22.0 from crates.io, renamed `gpui`
  and `gpui_platform` in the workspace so no code changed, pinned with
  `=`; the snapshot is of Zed's `v1.22.0` tag, AccessKit included, four
  days older than the revision it replaces and with the same API. The
  lock file lost its 22 crates from Zed's repository and Zed's forks of
  font-kit, xim, scap, wasm_thread and proptest (they come from crates.io
  too). `tools/check-zed-deps.sh` now also fails on a Zed git source in
  `Cargo.lock`; dependabot moves the two crates together. Vendoring
  (T2.8.6b) stays the fallback should the snapshot line stop.)
- [ ] R2.8 Signing: macOS notarization and a Homebrew cask; Windows
  code signing and an MSI; Linux AppImage. Each needs the owner's
  certificates and accounts. L, owner (T2.8.1, T2.8.2, T2.8.3)
- [ ] R2.9 The one-page site at the Book's domain (owner, D7) exported
  from `book/part-1` by Kalem itself (T1.8.10). M
- [~] R2.10 Public repository settings: the contact address (owner,
  T0.1.9), issue labels `good first issue` and `help wanted`, the five
  dependabot pull requests merged or closed (T1.8.8). S
  Done: the labels exist and no dependabot pull request is open. Open:
  the contact address (owner).
- [ ] R2.11 Early access and announcements: ten users from the personas,
  the Org list draft sent, then HN and the subreddits after R2.8.
  Owner (T1.8.4, T1.8.5, T1.8.9)

## M3 The core as designed (architecture debt)

Exit: both editors render every mode through `ModeSpec`; a mode can be
added without touching either frontend; `Request` is handled once;
errors are typed and translated; `kalem-core` is split along its seams;
the parity test still passes on the whole corpus.

- [x] R3.1 Org on the mode contract: `OrgMode: ModeSpec` wrapping
  `org-syntax`/`org-model`/`org-edit` (tree in the contract's kinds,
  outline, format, diagnostics, edit keys), passing `modes::check` on the
  Org corpus. L (T2.7c.10)
  Done 2026-10-04: `kalem_core::org_mode::OrgMode` maps `org-syntax`
  (headings by `org-level`, lists ordered by their bullet, checkboxes,
  source blocks with their language, tables to cells, links and
  pictures with their targets); outline from the view, Format Document
  from `org-edit`, diagnostics from `org-lint`, Enter as the editor's
  Enter and Tab as the table's next field. `modes::check` passes on the
  32 files of `tests/corpus/org-mode`.
- [ ] R3.2 The editors render from the contract: `line_view`, `blocks`
  and `outline_items` become trait methods with the per-mode free
  functions behind them; the hand-written dispatch in
  `kalem-tui/src/editor.rs`, `kalem-tui/src/app.rs`, `kalem-ui/src/editor.rs`
  and `kalem-ui/src/outline.rs` goes; the 70 `DocumentMode::` branches in
  the frontends fall to the ones about chrome. The parity test and the
  snapshot tests guard it. L (T2.7c.10)
- [ ] R3.3 One `Request` handler: the shared parts of the two 400-line
  `request` functions move into a core `requests` module that the
  frontends call with a small platform trait (clipboard, open URL, file
  dialog, focus); the frontends keep only what draws. M
- [ ] R3.4 Typed errors: an `Error` enum per crate boundary with
  `thiserror` (`OpenError`, `EditError`, `CommandError` with a kind and a
  Fluent key), `Result<_, String>` gone from public signatures (264
  today), every `CommandError` the user can see created through `tr`
  (173 of 225 are not). M
- [ ] R3.5 Command tables as data, not 3,700-line vectors: each command
  a named function registered by a small macro or a `const` table, so
  `plain_commands`, `grid_commands`, `csv_commands` and the three `cmd()`
  helpers become one shape; command ids as constants checked at compile
  time where the frontends name them. M
- [ ] R3.6 Global state out of the core: the 32 `static Mutex` in
  `kalem-core` (jobs, LSP service, LaTeX root setting, memos, file
  clipboard, bookmarks, sessions) become fields of an `Editor`/`Session`
  context passed down, or at least of one `Globals` struct with a test
  reset; tests stop depending on order. L
- [ ] R3.7 `kalem-core` split along its seams: `kalem-latex`
  (`latex_*`, `synctex`, `tex_pictures`, `siunitx`, ~15k lines),
  `kalem-viewer-host` (`viewer.rs`, 11k), `kalem-dired`, `kalem-vim`;
  `kalem-core/src` gets subfolders for what stays (`markdown/`, `csv/`,
  `plugins/`, `keys/`). Each new crate gets a README. L
- [ ] R3.8 One table engine: `markdown_table`, `latex_table`,
  `latex_fmt::cells` and the CSV tools share `org-table`'s cell model
  (split, align, move, recalc), each format keeping only its syntax. M
- [ ] R3.9 (owner, D23) The seam before any toolkit move (T2.9.1): the
  editor logic of `kalem-ui` (rows from the view model, hit testing,
  panel state) separated from gpui calls behind a thin layer, measured by
  how much of `kalem-ui/src` imports `gpui`. L; do R3.2 and R3.3 first,
  they are most of it.

## M4 Performance in CI

Exit: every §15 target has a benchmark that runs in CI on each pull
request and fails on a regression over its budget; the three missed
targets are met; no whole-file work on the keystroke path of any mode.

- [ ] R4.1 The benchmark job (TS.3): the existing examples
  (`text_timing`, `markdown_timing`, `latex_timing`, `latex_keystroke`,
  `synctex_timing`, `highlight_timing`, `org-syntax` criterion) run on a
  pinned runner with `--release`, print one JSON line each, and a script
  compares them with budgets in `benchmarks.toml` (a target and a
  tolerance); the Book's performance table is generated from the last
  run. M
- [ ] R4.2 Frontend frame times measured, not only the core: the
  `#[ignore]` latency tests of both editors un-ignored in the benchmark
  job for Org, LaTeX, Markdown and CSV at 1 MB and 10 MB. M
- [ ] R4.3 LaTeX `blocks` incremental: keep block starts and shift them
  through the transaction, rescanning only the edited lines (26 ms → ~0
  at 10 MB); the screen model's rebuild reused across keystrokes that
  only move events (41 ms); the project keystroke under 2 ms. M
  (T2.7h.35)
- [ ] R4.4 GUI frame: a `LineView` cache keyed by (version, line,
  cursor line) so a line is built once per version, not twice per frame;
  `compute_visible` as ranges when nothing is folded; the character width
  measured once per font change. S/M
- [ ] R4.5 Highlighter without full passes: `Highlighter::update` takes
  the transaction's range instead of diffing two strings, uses `memchr`
  for line counts, and keeps no copy of the text. S/M
- [ ] R4.6 Markdown and CSV keystrokes: Markdown keeps a diff of the
  edit rather than a copy of the text, with relative positions per
  top-level block (the open half of T2.7c.6); CSV updates widths and the
  index from the edited record on, not from the start. M
- R4.7 Withdrawn (owner, 2026-10-04) with Kalem's own format.
- [ ] R4.8 A worker pool for background parses, renders and searches
  instead of a thread per task, with `Arc<str>` snapshots instead of
  copies; the viewer's render and search no longer share one mutex. M
- [ ] R4.9 Startup and binary measured in CI with the rest (cold start,
  terminal start, both binary sizes). S

## M5 What users ask for (features)

Exit per item. Order within the milestone follows the personas of the
design document (writers, scientists, programmers) and the owner's
answer to D21; nothing here starts before M1 and M2 are done, and R3.1–R3.4
come before any item that adds a mode or a frontend branch.

Writers (Org and Markdown)
- [ ] R5.1 Spell checking (T3.6.1, T2.7h.29): `spellbook` with Hunspell
  dictionaries, English and Turkish first, inline marks in both editors,
  a personal dictionary, the language from `#+LANGUAGE`, `babel`'s
  options or the setting; math, commands, labels, keys, verbatim and
  URLs skipped in LaTeX and Markdown. L
- [ ] R5.2 Agenda and capture (T3.5.1–T3.5.10): a workspace index of
  Org files, the agenda views (day, week, TODO list, tags), actions on
  entries, capture templates, refile across files, `id:` links,
  clocking, `kalem agenda` on the command line. L (two or three
  cycles; the index first)
- R5.3 Withdrawn (owner, 2026-10-04) with Kalem's own format.
- [ ] R5.4 Presentations: Beamer and reveal.js export (T3.6.2, T3.6.3),
  a template picker (T3.6.4). M
- [ ] R5.5 Obsidian vaults as themselves (T2.7c.12): wiki links across
  the vault, backlinks, the vault's folder as a project. M
- [ ] R5.6 Babel (T3.4.1–T3.4.9): header arguments, executors for shell,
  Python, R and gnuplot behind the plugin host's trust model, results
  blocks, tangling. L (after the plugin permissions, R5.11)

Scientists (LaTeX)
- [ ] R5.7 The LaTeX exit criteria (T2.7h.39): a paper and a thesis from
  the corpus edited end to end and compiled; a co-author in Overleaf
  notices no diff; recorded in the Book. M, owner's time
- [ ] R5.8 PDF beside the text (T2.7h.24, T2.7h.40): the built PDF in a
  second pane that follows the cursor's page after each build and keeps
  its place; the dark inversion; the refresh under 100 ms. M
- [ ] R5.9 DOI to BibTeX (T2.7h.19, needs the `net` permission of
  R5.11); texlab as an LSP server through the language-plugin manifest
  once R5.12 lands. M
- [ ] R5.10 Typst (owner; T2.7h.25) as the first plugin mode on the
  contract, after R3.2 and R5.11: `.typ` highlighting is already there;
  outline, `typst compile` with its problems in the text, the PDF pane. M

Programmers (LSP and plugins)
- [ ] R5.11 Plugin permissions and isolation (T3.1.10, T3.1.13,
  T3.1.11): the consent prompt on first use of `fs`, `net` and `process`,
  per-plugin limits, a plugin's panic or timeout reported and the plugin
  disabled without taking the editor down; `SECURITY.md`. M
- [~] R5.12 Language plugins (T3.8.5–T3.8.6e): Python, Rust, Go, C/C++,
  web and PHP as data-only plugins (manifest, syntax, server); the LSP
  client gains rename, code actions, signature help and a snippet engine
  (T3.8.3); the `SPC c` map (T2.7i.10). L (partly done, 2026-10-10:
  Rust, `plugins/rust` in getkalem/plugins, its list `rust_todo.md`:
  rust-analyzer at the nearest `Cargo.lock`, Sublime Text's current
  Rust syntax over the built-in one, its settings for the panel,
  rustfmt, and the client's features checked on a corpus workspace.
  The client, from it, for every server: a server that ends before
  it answers `initialize` is not restarted and says why in its own
  words with the plugin's install text, and a server's `version` is
  run by `kalem lsp status`; a file a server named outside its root
  is served by that server; diagnostics given only when asked (the
  pull model, rust-analyzer's own) are asked for; a request cancelled
  while the server loads is asked again; a formatting answer of
  `null` is said as no change; a server's own requests by Kalem's
  commands (the manifest's `requests`: Expand Macro, Open
  Documentation in the Browser, Go to Parent Module, Go to Project
  File, Reload Project, Join Lines, Move Item Up and Down). Open for
  Rust: automatic imports, lenses, run and test, servers in source
  blocks.)
- [x] R5.12e The Elixir plugin and the language server client,
  reviewed (owner, 2026-10-04: "code quality, performance, what is
  missing; a list, then in order"). Bugs first, then speed, then
  quality, then features; each item names its files.
  Bugs:
  - [x] E1 Answers nobody takes: `lsp::tick` reports a change while any
    answer waits, so an answer for a closed or hidden document makes
    every editor redraw every 50 ms; old jumps fire when the document
    comes back; server messages are taken only by served documents.
    Answers carry their document's version and expire; `tick` reports
    real changes only; messages go to the active editor. S (done 2026-10-04: answers carry the document's version and expire after 15 s; closing a document drops its answers; documentation for older text is dropped; `tick` reports real changes only; what servers say on their own goes through `jobs::notice` to the active editor; tested)
  - [x] E2 Writes to a server never block the editor: a writer thread
    per client fed by a channel (today `didChange` writes on the UI
    thread under the service's lock, and `initialized` writes the queue
    under the state lock on the reader thread, so a busy server can
    freeze the editor or deadlock both sides). S (done: a writer thread per client, `send` only queues)
  - [x] E3 Before a server is ready, one queued text per document, not
    a full copy per keystroke. S (done: the queued `didOpen` takes the latest text, or the queued change is replaced; tested)
  - [x] E4 A `null` formatting answer is "already formatted", not an
    error. S (done; tested)
  - [x] E5 Settings reach running servers: `server`, `settings` and a
    newly installed server program take effect without reopening files
    (`didChangeConfiguration`, documents opened again); Restart Language
    Server works for a file that had no server; failed starts are tried
    again. S (done: `lsp::settings_changed` from `apply_process_settings`: `Client::set_settings` sends `didChangeConfiguration`, documents whose server changed (or was found, or turned off) open again, failed starts forgotten; Restart works without a server; tested)
  - [x] E6 A renamed, moved or saved-as file closes its old document in
    the server. S (done: documents carry a serial (`DocumentState::serial`), and the same document under a new path closes the old one in the server, for every way the path changes; tested with Save As)
  - [x] E7 After the last allowed crash the status says why the server
    stopped, not "restarting"; the crash count resets after a while
    running. S (done: documents of a server given up on are detached and say why; crashes forgotten after 10 minutes running)
  - [x] E8 A failed `initialize` ends the process and marks the server
    failed, instead of "starting" for ever. S (done: a refused `initialize` kills the process, one unanswered for 2 minutes too; tested (`refuse`))
  - [x] E9 Diagnostics for an older version are not drawn on newer
    text (`versionSupport` is announced but unused). S (done: diagnostics stored with their version; stale ones are not placed (the cursor line's message, `lsp::diagnostics`), counts still shown)
  - [x] E10 Places in other open documents read their text from the
    editor, not the disk. S (done: open documents' texts copied for the answers that point into them)
  - [x] E11 The manifest's comment tokens drive Toggle Comment (HEEx and
    EEx have none in the built-in table). S (done: `languages::comment_style` before the built-in table; tested)
  Speed:
  - [x] E12 The status bar's word on diagnostics computed once per
    change, not per frame (today every render clones the text and
    converts every diagnostic from the start of the text). S (done: per document, the diagnostics read once per publication and text change, with a line index; the problems list uses it too)
  - [x] E13 `sync` cheap: files no plugin serves remembered (not looked
    up every tick), and changes sent from the editor's transactions
    instead of whole-text comparisons. S (done: unserved files remembered (cleared when plugins or settings change); one comparison and one copy per change, no copy in the client for one edit. Changes from the editor's transactions left: not worth the API change now)
  - [x] E14 Completion without a 5 ms polling loop, and without cloning
    every item's JSON. S (done: `Pending::wait_for` in 20 ms slices; items' JSON kept only for servers that resolve)
  - [x] E15 No disk reads under the service's lock; servers stopped in
    parallel on quit. S (done: answers read outside the lock (the open texts they need copied first); servers stopped in parallel on quit)
  Quality:
  - [x] E16 Every message of `lsp`, `languages` and `plugin_store`, and
    the frontends' language server messages, through the interface
    language (`tr!`, Turkish included). M (done 2026-10-04: every message of `lsp`, `languages`' server resolution, `plugin_store`'s summary, notices and errors a user meets, the plugin commands and the frontends' language server message, in English and Turkish (`lsp-*`, `plugin-*`); manifest parse errors stay technical)
  - [x] E17 Loose ends: the wake hook wired (the terminal editor sees a
    server's answer at once, not at its 500 ms timeout); `did_close`
    drops diagnostics under the normalized URI; the format request's
    indentation from the document; caps on a message's
    `Content-Length` and on an archive's unpacked size; Windows absolute
    paths refused in `main`; abandoned staging folders removed;
    `newer("1.0.0", "1.0")` false; unused functions and manifest fields
    removed or used. S (done: the terminal editor polls every 100 ms while a server runs (`lsp::active`), the unused wake hook removed; the format request uses the document's indentation; messages over 256 MB and archives unpacking to more refused; staging folders older than a day removed; `newer` pads versions; `Client::is_open` removed; the component path check refuses rooted and absolute paths on Windows too. Kept: `Plugin.commands` (E22) and the comment fields (E11, used))
  - [x] E18 `lsp::tick` split (restart, messages, answers); the
    start-and-reopen logic in one place. S (done: `tick` is `Service::watch_servers`, `restart_slot`, `finished_actions`, `answer_context` and `slot_exited`)
  Features:
  - [x] E19 Diagnostics in the text: underlined in both editors, a mark
    in the gutter, the message on hover; the problems list includes the
    project's files the server reports. M (done 2026-10-04: `lsp::flag_diagnostics` through LaTeX's flagging (now `latex_view::flag_ranges`) in both editors' plain lines, errors and warnings as probably wrong, the rest as style; the line number colored in both gutters (`lsp::line_mark`); the messages under the mouse in the graphical editor (`lsp::diagnostic_at`); `all_problems` adds the project's files the servers report, read outside the lock; tested)
  - [x] E20 Expert's work shown: "indexing" in the status bar from its
    log until its first diagnostics (it reports no progress). S (done: a server's `busyLog` (`start`, `done` substrings) makes its log lines between them the status bar's progress, ended too by its first diagnostics; Expert's is "Starting project" to "Compiled " (the plugin's manifest); tested)
  - [x] E21 Files changed outside the editor told to the server
    (`workspace/didChangeWatchedFiles`, from Kalem's file watcher), so
    `mix deps.get` or a checkout recompiles. S (done: each server's root watched (`notify`, build and tool folders left out), changes sent in batches 250 ms after the first; `didChangeWatchedFiles` announced; tested)
  - [x] E22 Format without a server: the manifest's formatter command
    (`mix format --stdin-filename {file} -`). S (done: `lsp::format_with_command` runs the manifest's `commands.format` (`{file}` filled in) in the background when no server formats, the result applied as one change for its version; Format Document and `hasFormatter` use it; tested with the test binary as the formatter; the Elixir manifest's command now passes `--stdin-filename {file}`)
  - [x] E23 Completion: incomplete lists asked again as the word grows;
    `additionalTextEdits` (aliases added) applied; signature help while
    typing arguments. M (done: incomplete lists are asked again already (a new request each keystroke); `additionalTextEdits` applied in the same change; signature help asked as `(` or `,` is typed and closed by `)`, shown beside the cursor in both editors while the cursor stays on the line (`kalem lsp ask signature`); tested. Expert 0.1.11 has no signature help; ElixirLS has)
  - [x] E24 Rename and code actions with `workspace/applyEdit`, as one
    transaction with a preview. M (done: `code.rename` (`SPC c r`, F2 where a server serves the file) asks for the name and offers the edit to confirm ("Apply: N changes in M files"); `code.actions` (`SPC c a`, Ctrl+.) lists the actions, an action's edit applied and its command run, `codeAction/resolve` when it has neither; `workspace/applyEdit` from servers applied and answered; edits of open documents go to their editors with the version check (background buffers in the terminal editor too), files not open are written; file operations refused with the reason; tested)
  - [x] E25 The manifest's defaults: dialyzer off by default (a PLT
    build on first start is heavy), ElixirLS's `language_server.bat` on
    Windows, one server for scripts outside a Mix project; the plugin's
    conformance test runs Kalem's `extends` resolution on HEEx. S (done 2026-10-04: dialyzer off by default, `language_server.bat` among ElixirLS's candidates, `requireRoot` (no server outside a Mix project, the reason said; tested); the plugin's conformance test loads its syntaxes through Kalem's `kalem-highlight` (a git dependency pinned to a commit) and highlights HEEx, `~H` and EEx; found that a language's `syntax` never reached the highlighter (`.eex` files were not highlighted): `kalem_highlight::set_aliases` maps each plugin language's names and extensions to its syntax)
- [ ] R5.13 The remaining extension points of `coverage.toml` in the
  order plugins ask for them: decorations and completers, link types,
  block renderers, exporters, table functions, themes, CLI subcommands
  (T3.1.9a–g); each with a contract test (T3.1.17) and an example plugin
  (T3.3.2). L
- [ ] R5.14 Plugin developer experience: the template (T3.3.1), hot
  reload (T3.2.2), the inspection panel (T3.2.1), API docs generated from
  the WIT into Part III (T3.3.4). M
- [ ] R5.15 A `git` plugin (T2.7i.11): status, blame, stage, commit, log
  through the `process` permission, on Doom's `SPC g` keys. M

Viewers
- [ ] R5.16 docx, pptx and sqlite viewers (T3.7.5, T3.7.6, T3.7.6a) on
  the viewer contract; terminal parity for viewers (T3.7.8); the file
  manager opens everything (T3.7.9). L
  (docx's part done 2026-10-09, released as `docx-v0.0.1`: the `docx`
  plugin of getkalem/plugins on plugin API 0.2.7's `flow` and `annotations`
  interfaces, in both editors, T3.7.5's progress and the Book's "Word
  documents" saying what is open. pptx and sqlite are open.)

## M6 Continuous

These never close; they are checked on every pull request or release.

- [ ] R6.1 Every pull request: the changelog entry, the Book chapter
  (`kalem book check --changed`), fmt and clippy clean, tests in both
  editors, terminal parity recorded (TS.2, TS.6, TS.13).
- [ ] R6.2 The benchmarks of M4 block a pull request over budget (TS.3).
- [ ] R6.3 Corpora and licences: every file registered; arXiv's
  non-redistributable sample stays a draft release (TS.4).
- [ ] R6.4 Dependencies: dependabot merged monthly; the duplicate-version
  count in `Cargo.lock` reported and not growing (TS.7); the gpui pin
  moved on purpose, never by accident.
- [ ] R6.5 The Book replaces `docs/` as the reference (T2.10.6); Part II
  chapters reordered to the skeleton (T2.10.8); the Turkish Part I when
  the manual settles (T2.10.7); Book versioning with releases (T2.10.5).
- [ ] R6.6 Users before features (TS.11): each release's plan starts from
  what the early-access users asked for, not from this list.
- [ ] R6.7 The standard modes are never extended (TS.14).

## What is deliberately not on this roadmap

- Leaving gpui (old group 10, T2.9.2–T2.9.11) beyond the seam of R3.9:
  it waits on D23, and the seam makes either answer cheap.
- Column view, habits, diary sexps, inline tasks, org-crypt, SFTP, a
  model-backed completer (old group 21): phase 4, after users ask.
- AsciiDoc, reStructuredText and the log view (T2.7g.1, T2.7g.2): plugin
  modes after R3.2 and R5.10 show the path works.
- An integrated terminal or a debugger: not planned, by design.
