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

- [~] R1.1 CI completes on `main`: keep `cancel-in-progress` for pull
  requests, drop it for `main` (`cancel-in-progress: ${{ github.event_name == 'pull_request' }}`),
  and add a 90-minute timeout to every job. Then watch the first ten
  runs. S
  (done 2026-10-04: `cancel-in-progress` only for pull requests, `timeout-minutes: 90` on every job. Open: watch the first ten runs on `main`.)
- [~] R1.2 Windows tests: `lsp_service` and `plugin_install` fail on
  every Windows run; read the logs, fix the path or process handling,
  make the fake server start on Windows, and mark nothing `#[ignore]`. M
  (done 2026-10-04: four causes, each fixed: the plugin cache was mapped, so Windows refused to rewrite it while in use (and Linux got SIGBUS) — now read into memory; `canonicalize` gives verbatim paths where `..` is a name, so the plugin file check missed it — refused as a name too; the fake language server's verbatim URIs did not match the client's — URIs normalized to one spelling (`kalem_lsp::uri::normalize`), which also covers servers writing `file:///c%3A/`; the plugin test wrote a Windows path into a TOML basic string, where `\U` is an escape — a literal string now, and `file:///C:/` index URLs are read. Open: a green Windows run on `main`.)
- [ ] R1.3 Plugin pins cannot break `main`: the three `getkalem/plugins`
  revisions move together, in one `[workspace.dependencies]` entry; a CI
  job builds the plugins against this checkout's `kalem-viewer` contract
  on every push, so a contract change that is not matched by a plugin
  bump fails the pull request rather than `main`. S
- [ ] R1.4 Two sessions, one `main`: a push cadence rule in CONTRIBUTING
  (rebase on `origin/main`, run the changed crate's tests, push at most
  once an hour unless CI is green), and a `tools/pre-push.sh` that runs
  fmt, clippy on the changed crates and the quick tests. S
- [ ] R1.5 Stale pages fixed so the Book and the code agree:
  `book/part-2/latex.org` "Limits and known gaps" (SyncTeX, the PDF panel,
  `\multirow`, the corpus are done), `performance.org` binary size (49.7
  MB measured, 13 MB written), `todo.md` T4.3.2 (hayro, not pdfium). S
- [ ] R1.6 Repository hygiene: move `docs/todo_old.md`, `excel_todo.md`,
  `excel_todo2.md` under `docs/history/`; move `spikes/` out of the tree
  or into a `spikes` branch; the gpui git revision in one place in
  `[workspace.dependencies]` (it is in six). S

## M2 Release 0.1 (installable)

Exit: a tagged `v0.1.0` with binaries for macOS (arm64, x86-64), Linux
(arm64, x86-64) and Windows from a release workflow; a reader of the
README installs it in two commands; the release checklist run once on
each platform with the results in the release issue.

- [ ] R2.1 Binary size under the target: measure the contribution of
  each bundled plugin and of Wasmtime (`cargo bloat`), then take the
  cheapest of: `panic = "abort"`, `opt-level = "s"` on cold crates (the
  decoders, Wasmtime, fonts), the viewers as features that the full
  build turns on, a dependency audit of the 101 crates present twice.
  Target: full build under 40 MB, terminal-only under 15 MB, both
  recorded by a CI step that fails over the target. M (T2.9.11, D28)
- [ ] R2.2 Crash debt (T1.8.11, TS.10): turn on `clippy::unwrap_used`
  and `expect_used` for `kalem-core`, `org-edit`, `kalem-ui` and
  `kalem-tui` with `#[expect]` on each justified site; replace the 313
  `.expect(` and 32 `panic!(` on keystroke and command paths; the
  when-clause `expect` at startup (`builtin.rs:34`) becomes a test. The
  crash report (design §14) is checked by a test that panics on purpose.
  M
- [ ] R2.3 `release.yml` from `dist generate`, a `v0.1.0` tag, release
  notes cut from the Unreleased section of the changelog, which becomes
  `## 0.1.0`. The `kalem gui --help` bug (T1.8.12) fixed first. S
  (T1.8.1)
- [ ] R2.4 The release checklist run by hand on macOS, Linux (X11 and
  Wayland) and Windows, results in the release issue, each failure
  either fixed or recorded as a known issue in the README. L, owner's
  machines (T1.5.20, T1.4.10, T1.5.9a, T2.7h.37)
- [ ] R2.5 README rewritten to what Kalem is (T2.10.11, T1.8.7): the
  positioning of D21; "works today" generated from the mode table; PDF,
  pictures and Excel mentioned; the install commands; a GIF of both
  editors on the first screen; the speed claims point at the CI
  benchmarks of M4 rather than at a hand measurement. M
- [ ] R2.6 TeX on a clean machine (owner, D5): either tectonic downloaded
  on demand with a prompt, or a clear "install TeX Live or MiKTeX"
  message with the link, in both editors. S after the decision
- [ ] R2.7 Building without Zed's repository (T2.8.6): pin to a
  `gpui-unofficial` snapshot or vendor the two crates; CONTRIBUTING says
  how large the clone is until then; a CI guard fails a pull request that
  adds a `zed-industries` git dependency. M
- [ ] R2.8 Signing: macOS notarization and a Homebrew cask; Windows
  code signing and an MSI; Linux AppImage. Each needs the owner's
  certificates and accounts. L, owner (T2.8.1, T2.8.2, T2.8.3)
- [ ] R2.9 The one-page site at the Book's domain (owner, D7) exported
  from `book/part-1` by Kalem itself (T1.8.10). M
- [ ] R2.10 Public repository settings: the contact address (owner,
  T0.1.9), issue labels `good first issue` and `help wanted`, the five
  dependabot pull requests merged or closed (T1.8.8). S
- [ ] R2.11 Early access and announcements: ten users from the personas,
  the Org list draft sent, then HN and the subreddits after R2.8.
  Owner (T1.8.4, T1.8.5, T1.8.9)

## M3 The core as designed (architecture debt)

Exit: both editors render every mode through `ModeSpec`; a mode can be
added without touching either frontend; `Request` is handled once;
errors are typed and translated; `kalem-core` is split along its seams;
the parity test still passes on the whole corpus.

- [ ] R3.1 Org on the mode contract: `OrgMode: ModeSpec` wrapping
  `org-syntax`/`org-model`/`org-edit` (tree in the contract's kinds,
  outline, format, diagnostics, edit keys), passing `modes::check` on the
  Org corpus. L (T2.7c.10)
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
- [ ] R4.7 `.klm` keystroke under 2 ms: relative ranges in `klm-syntax`
  (T2.13.3a) together with an incremental contract tree in `KlmMode`. M
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
- [ ] R5.3 The Kalem format as itself (T2.13.4–T2.13.7): `.klm` opens as
  `.klm`, rendered through `KlmMode` in both editors, with `klm-edit`'s
  guarantees (closing braces, atomic delimiters); the model shared with
  Org so the agenda sees both. L
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
- [ ] R5.12 Language plugins (T3.8.5–T3.8.6e): Python, Rust, Go, C/C++,
  web and PHP as data-only plugins (manifest, syntax, server); the LSP
  client gains rename, code actions, signature help and a snippet engine
  (T3.8.3); the `SPC c` map (T2.7i.10). L
- [ ] R5.13 The remaining extension points of `coverage.toml` in the
  order plugins ask for them: decorations and completers, link types,
  block renderers, exporters, table functions, themes, CLI subcommands
  (T3.1.9a–g); each with a contract test (T3.1.17) and an example plugin
  (T3.3.2). L
- [ ] R5.14 Plugin developer experience: the template (T3.3.1), hot
  reload (T3.2.2), the inspection panel (T3.2.1), API docs generated from
  the WIT into Part IV (T3.3.4). M
- [ ] R5.15 A `git` plugin (T2.7i.11): status, blame, stage, commit, log
  through the `process` permission, on Doom's `SPC g` keys. M

Viewers
- [ ] R5.16 docx, pptx and sqlite viewers (T3.7.5, T3.7.6, T3.7.6a) on
  the viewer contract; terminal parity for viewers (T3.7.8); the file
  manager opens everything (T3.7.9). L

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
- [ ] R6.7 The standard modes are never extended (TS.14); `.klm` is the
  one place Kalem defines syntax.

## What is deliberately not on this roadmap

- Leaving gpui (old group 10, T2.9.2–T2.9.11) beyond the seam of R3.9:
  it waits on D23, and the seam makes either answer cheap.
- Column view, habits, diary sexps, inline tasks, org-crypt, SFTP, a
  model-backed completer (old group 21): phase 4, after users ask.
- AsciiDoc, reStructuredText and the log view (T2.7g.1, T2.7g.2): plugin
  modes after R3.2 and R5.10 show the path works.
- An integrated terminal or a debugger: not planned, by design.
