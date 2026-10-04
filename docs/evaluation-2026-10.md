# Kalem: an evaluation (2026-10-04)

An assessment of the code base at commit `cf73e7d` (697 commits, seven days
of history since 2026-09-27) in three parts: code quality, performance and
functionality. Every number below was measured on this checkout; where a
number comes from the Book or the old task list, it says so. The roadmap
that follows from it is `docs/roadmap.md`.

## Summary

Kalem is a large, well-documented and well-tested code base that already
does more than its README says: Org, Markdown, CSV, LaTeX and BibTeX as
themselves, Excel, PDF and pictures through bundled plugins, a WASM plugin
host, two frontends with the same commands, differential tests against
Emacs, pdflatex, pandoc, Vim and KaTeX. In seven days it has grown to
247k lines of Rust and 1,190 tests.

Its weaknesses are the ones a week of very fast growth leaves:

1. **Nothing is shippable yet.** No release, no tag, no release workflow,
   both binaries over their size targets (the terminal one three times), 32 manual release checks never run, and
   on `main` CI has not completed a single run in the last 100: 95 were
   cancelled by the next push, 4 failed. There is no green signal.
2. **The core carries the architecture debt.** The mode contract that the
   design calls the heart of the system is implemented by four modes and
   used by none of the editors; both frontends dispatch by hand in
   duplicated code; `kalem-core` is 95k lines in one flat folder with
   35 global mutexes; errors are strings and most are not translated.
3. **Performance is measured by hand and only on one machine.** Every
   target that is measured is met except the binary size, a LaTeX project
   keystroke and a `.klm` keystroke; but nothing runs in CI, several
   numbers have under 10% headroom, and the hot paths have known
   whole-file work per keystroke.
4. **The product is wide, not finished.** Each area has a tail of
   partial items; the writer's core workflows (agenda, capture, spell
   checking) and the programmer's (rename, code actions, language
   plugins) are missing; the Kalem format is advertised but opens as Org.

The recommendation: stop widening for one cycle, make `main` green and
releasable (0.1), pay the architecture debt that every later feature would
otherwise copy (the mode contract in the editors, the frontend duplication,
the error type), put the performance numbers in CI, then resume features
in the order users ask for them.

## 1. Code quality

### Size and shape

| | |
|---|---|
| Rust lines (crates/, tests/, fuzz/, spikes/) | ~247,000 |
| Crates | 24 |
| `kalem-core` | 95,578 lines, 118 files, one flat folder, 1,270 `pub fn` |
| Frontends | `kalem-ui` 22,070, `kalem-tui` 19,009 |
| Files over 3,000 lines | 14 (9 hand-written source, 2 generated, 3 tests) |

The largest hand-written files: `kalem-core/src/viewer.rs` 11,048 lines,
`latex_view.rs` 9,310, `builtin.rs` 9,223, `kalem-tui/src/app.rs` 5,049,
`kalem-ui/src/editor.rs` 4,587, `vim.rs` 4,516, `dired.rs` 4,341.

The longest functions are command tables written as one `vec![]` literal
with inline closures: `plain_commands` (builtin.rs:4151) is about 3,740
lines, `grid_commands` (viewer.rs:8851) 1,680, `csv_commands` 820. The
longest piece of real logic is `unflagged_line_view` (latex_view.rs:3512),
about 1,500 lines in one match. These are the files every feature touches
and the ones that make merges between two working sessions collide.

### What is good

- **Lints and CI are strict**: `unsafe_code = "deny"`, clippy `all` at
  warn, `-D warnings` in CI, `unreachable_pub`, `rustfmt --check`, an MSRV
  job, a terminal-only build that asserts no gpui in the tree. 4 `unsafe`
  blocks in the whole workspace, each small and justified. Clippy on
  Rust 1.99 reports 1 warning across the workspace.
- **Documentation on code is excellent**: 1,264 of 1,270 `pub fn` in
  `kalem-core` carry a doc comment; every large module opens with a `//!`
  that cites the design document. The Book (77 pages) is built and
  checked by the project's own exporter, and `kalem book check --changed`
  now fails a pull request that changes mapped code without its chapter.
- **Tests are many and of the right kind**: 1,190 `#[test]` (461 in
  core, 201 TUI, 149 GUI), 111 insta snapshots, 7 proptest blocks, 4
  cargo-fuzz targets (short run on every push, 15 minutes each nightly),
  a parity test that compares the two editors on the same corpus, and
  differential jobs against Emacs, pdflatex, Vim, pandoc and KaTeX.
  Corpora are licensed and registered (`tests/corpus/LICENSES.md`).
- **Localization is enforced**: 1,258 Fluent keys in English and Turkish,
  a test that keeps the two complete and in step with command titles.
- **No TODO/FIXME debt in comments** and no `todo!()`: what is open is in
  the task list, not scattered in the code.

### What is not

- **The mode contract is not used.** `ModeSpec` (`modes.rs`) is
  implemented by CSV, Markdown, LaTeX and the Kalem format, not by Org;
  `Modes::with_builtins()` is called from one unit test and nowhere in
  production. The frontends dispatch by hand instead: the same outline
  fallback chain is copied into `kalem-tui/src/app.rs:3203` and
  `kalem-ui/src/outline.rs:196`; `line_view` dispatch is repeated at
  `kalem-tui/src/editor.rs:1111` and `kalem-ui/src/editor.rs:2561`;
  `blocks` dispatch likewise. Every mode has its own free `line_view`,
  `outline_items` and `blocks` instead of a trait method. The design
  document (§11.11) says plugins will bring modes through this contract;
  today a plugin mode would have nowhere to plug in.
- **The frontends duplicate each other.** `Request` has 66 variants, and
  each frontend handles them in its own 400-line function
  (`kalem-tui/src/app.rs:1570`, `kalem-ui/src/editor.rs:1159`); `DocumentMode`
  branching appears 33 times in the TUI and 37 in the GUI; 17 `latex().is_some()`
  checks serve as mode tests. The two editors also reach past the core into
  `org_edit`, `org_syntax` and `org_model` directly.
- **Errors are strings.** 264 functions return `Result<_, String>`, 196
  `map_err(|e| e.to_string())`, no `thiserror`; `CommandError` is a
  message. Of 225 `CommandError::new` sites, 52 go through `tr`, so most
  command errors a user sees are English only. Callers cannot match on an
  error's kind, which is why "no document" is a repeated literal.
- **Global state inside the core.** 35 `static Mutex/RwLock`, 26
  `thread_local!` and 10 `OnceLock/LazyLock` in `src/`, 32 of the mutexes
  in `kalem-core` (jobs, LSP service, the LaTeX root setting, memos, the
  file clipboard, bookmarks, sessions). It works because the editor is one
  process, but it makes tests order-dependent and the core hard to embed.
- **Crash surface.** 313 `.expect(` and 32 `panic!(` in non-test source;
  `builtin.rs:34` parses every when-clause with `.expect` at startup. The
  old task list carries this as T1.8.11 ("unwrap/expect on keystroke
  paths") and TS.10; neither `clippy::unwrap_used` nor `expect_used` is on.
- **Four table implementations.** Org tables (`org-table`), Markdown
  tables (`markdown_table.rs`), LaTeX tables (`latex_table.rs`, plus a
  second `cells()` in `latex_fmt.rs`) and CSV each split cells and align
  on their own.
- **Three `cmd()` helpers** with different signatures (builtin.rs:18,
  dired.rs:1480, viewer.rs:4340); command ids and when-clauses are strings
  compared at runtime (`id == "x.y"` nine times in the frontends).
- **Dependencies.** 1,067 packages in the lock file; 101 crates present in
  more than one version; 28 lock entries from Zed's git repository (a
  485 MB checkout) pinned in six places rather than once in
  `[workspace.dependencies]`; the three bundled plugins pinned at three
  different revisions of `getkalem/plugins`, which is how `main` stopped
  building twice on 2026-10-03 (a viewer type changed in core before the
  plugin pin moved).
- **Housekeeping.** No `README.md` for `kalem-core`, `kalem-ui` or
  `kalem-tui`; `docs/` carries `todo_old.md`, `excel_todo.md`,
  `excel_todo2.md` and six spike projects outside the workspace.

### Process

- 697 commits in 8 days, 60 to 140 a day, all to `main`, from two
  parallel sessions. CI has `cancel-in-progress: true` on the `main`
  group, so every push cancels the run before it: of the last 100 runs on
  `main`, 95 were cancelled, 4 failed and 0 succeeded. The last green run
  on `main` is from the morning of 2026-10-03.
- The Windows test job has failed on every completed run since at least
  the start of 2026-10-03 (`lsp_service`, `plugin_install`).
- The Book and the task list are mostly kept current, with known stale
  spots: `book/part-2/latex.org` "Limits" still says there is no SyncTeX,
  no PDF panel, no corpus and that `\multirow` is not drawn, all of which
  are done; `performance.org` gave the binaries as 13 MB and 7 MB (corrected since); `todo.md` says
  the PDF panel will use pdfium, but it is hayro.

## 2. Performance

### Measured against the targets (§15 of the design document)

From `book/part-5/performance.org` (an M1 Max, by hand) and this session's
measurements (an x86-64 container):

| Measurement | Measured | Target | |
|---|---|---|---|
| Cold start, GUI | 148 ms | < 300 ms | met |
| Open 1 MB Org, GUI | 184 ms | < 200 ms | met, 8% headroom |
| 10 MB until interactive, GUI / TUI | 783 / 714 ms | < 1 s | met |
| Keystroke to frame, Org, GUI / TUI p50 | 8.7 / 8.0 ms | < 16 ms | met |
| Org incremental parse | 0.05 ms | < 2 ms | met |
| Memory, empty / 10 MB | 47 / 252 MB | 80 / 500 MB | met |
| Save 10 MB | 20 ms | < 100 ms | met |
| `kalem check` / `fmt --check` 1 MB | 91 / 97 ms | < 100 ms | met, 3–9% headroom |
| 100 MB plain text, keystroke (core only) | 2.7 ms p50 | < 16 ms | met, frontend not measured |
| Markdown 10 MB keystroke p50 / p99 | 10 / 23 ms | 16 / 33 ms | met |
| LaTeX 1 MB open / keystroke p50 / p99 | 50 / 10.7 / 15.8 ms | 200 / 16 / 16 ms | met, p99 has 0.2 ms headroom |
| LaTeX 10 MB open / keystroke | 0.49 s / 78 ms | < 1 s / — | open met; keystroke 5 frames |
| LaTeX project keystroke (1.8 MB `algebra.tex`) | 2.2 ms | < 2 ms | **missed** |
| `.klm` 1 MB keystroke | 2.2–2.9 ms | < 2 ms | **missed** |
| SyncTeX lookup | 31 / 4 µs | < 10 ms | met |
| Binary, full build (macOS arm64, D28) | **48.9 MB** | < 40 MB | **missed**; the Book said 13 MB |
| Binary, terminal-only (Linux x86-64) | **49.7 MB** | < 15 MB | **missed**, over three times; the Book said 7 MB |

Not measured at all: frame times of the frontends on LaTeX, Markdown and
CSV (only the core's keystroke is timed), the PDF panel refresh, the
terminal-only binary size on this checkout.

The design says "a benchmark exists for every target and runs in CI;
regressions block the pull request". None does. Every number above is a
manual run, and the ones with single-digit headroom will drift unnoticed.

### Where the time goes per keystroke

- **Text model.** One `String` plus a `Vec<usize>` of line starts
  (`text.rs`), chosen over a rope on purpose: a keystroke moves the tail
  of the text and shifts every later line start. Fine up to the measured
  sizes; it is the reason the 100 MB numbers are measured on the core
  only.
- **LaTeX.** `latex_view::blocks` (latex_view.rs:7517) scans the whole
  text for `\begin`, `\[` and `$$` on every version, 26 ms at 10 MB; the
  screen's model rebuild is another 41 ms. Both frontends cache blocks by
  version only, so every keystroke pays it.
- **Markdown.** Each keystroke keeps a full copy of the text
  (`markdown.rs:920`) so the next edit can be diffed, and documents over
  2 MiB compare prefix and suffix over the whole text.
- **CSV.** The `Layout` memo is keyed by version, so any edit rebuilds the
  index, re-measures the first 1,000 records and, with the gutter on,
  counts line feeds over the whole text.
- **Highlighting.** `Highlighter::update` (kalem-highlight/src/lib.rs:351–384)
  makes six or more full-text passes and keeps a copy of the text per
  keystroke for every non-Org file up to 4 MB.
- **GUI frame.** Each visible line's `LineView` is built at least twice a
  frame (`a11y_text` at editor.rs:3972 and `line.rs:921`), with no cache
  keyed by (version, line); `compute_visible` allocates a `Vec` of every
  line number on every edit (editor.rs:691), O(lines) in a 100 MB file;
  the editor shapes a "0" every frame to measure a character.
- **Background work** is a new OS thread per task with a copy of the text
  (Org full parse, Markdown parse, viewer render and search); the viewer's
  render and search share one mutex.

### Binary and build

- The full release build is 48.9 MB (macOS arm64, D28; 42.6 MB without
  the bundled viewers) and the terminal-only build 49.7 MB (Linux x86-64,
  measured here): the terminal build carries the same viewers, Wasmtime
  and decoders, so it is over its 15 MB target three times. Release
  profile: `lto = "thin"`, `codegen-units = 1`, symbols stripped,
  `opt-level` 3 everywhere, no `panic = "abort"`. Math fonts (T2.2.1) are
  still to come.
- A full test run of core and both frontends takes about 20 s of test
  time here on a warm build; the CI test job is dominated by compiling.
- Dev builds compile the rasterizer, image decoders and Wasmtime with
  `opt-level = 3`, which keeps debug builds slow to produce but fast to run.

## 3. Functionality

### What exists

- **Modes:** Org (not on the contract), Markdown (comrak fork), CSV/TSV,
  LaTeX, BibTeX grid, plain text and code (syntect), the Kalem format's
  parser and formatter; 8 `DocumentMode` kinds with detection by
  extension, mode line, shebang and content.
- **Commands:** about 560 (viewer 67, file manager 65, Org 61, CSV 55,
  LaTeX 31, Markdown 29, …), shared by both editors, two keymap profiles
  (Word-like, Vim with Doom's leaders), palette, menus, when-clauses.
- **Editors:** gpui window and ratatui terminal with panes, workspaces,
  sessions, projects, search, outline, file manager (65 commands, drag
  and drop, context menu), bookmarks, themes, English and Turkish, IME,
  AccessKit in the GUI, kitty/iTerm2/sixel pictures in the terminal.
- **Viewers (bundled plugins):** pictures (PNG to EXR), PDF (hayro,
  threaded, SyncTeX both ways), Excel with a very long feature list
  (formats, tables, sort and filter, names, notes, charts, grouping, page
  setup).
- **Org:** folding, TODO, tags, dates, tables with formulas (Calc parity
  including complex numbers, sets, bitwise, symbolic mod), footnotes,
  citations with CSL, formulas through RaTeX; export to HTML, Markdown,
  GFM, LaTeX, PDF, text, and docx/odt/epub/rtf through pandoc; import
  through pandoc; a differential job against Emacs.
- **LaTeX:** rendered view measured on 925 arXiv papers (99.9% of body
  text rendered, every paper of three fields at target), numbering
  checked against pdflatex, structural editing fuzzed against pdflatex,
  completion, formatter, diagnostics, templates, builds with latexmk and
  the engines, SyncTeX, reproducible builds, a math corpus against KaTeX,
  50 CC BY papers committed.
- **Markdown:** GFM, task lists, tables with `TBLFM`, front matter, wiki
  links, the CommonMark and GFM suites in CI, a README and vault corpus.
- **Plugins:** a wasmtime host with WIT as the single source of the API,
  `kalem` and `kalem.ui` namespaces, document read and edit, settings,
  files and net behind permissions, `kalem plugin new/build/install/browse`.
- **LSP:** completion with documentation, hover, definition, declaration,
  implementation, type definition, references, symbols, formatting,
  diagnostics; one language plugin (Elixir, partial).
- **CLI:** view, parse, check, fmt, export, import, query, table recalc,
  book build and check, plugin, lsp, latex build, plus the development
  tools (diff-emacs, diff-pandoc, latex-coverage).

### What is missing, by who misses it

- **Everyone:** an installable release (no tag, no `release.yml`, no
  signed installer; building needs Zed's repository); spell checking
  anywhere; git anywhere (`SPC g` is reserved for a plugin that does not
  exist); the manual release checklist (32 items: IME, screen readers,
  terminals, MiKTeX) never run on any platform.
- **Writers:** agenda, capture, clocking, workspace-wide `id:` links;
  Babel execution and tangling; Beamer and reveal.js; PDF without TeX
  (D22 undecided); `.klm` opens as Org although the Book describes it as
  a format of its own.
- **LaTeX authors:** Typst (owner's decision; kept out of the core by
  D29); texlab; DOI lookup for BibTeX; the exit criteria (a paper and a
  thesis edited and compiled end to end with a co-author in Overleaf) not
  attempted.
- **Programmers:** rename, code actions, signature help, a snippet
  engine; language plugins for Python, Rust, Go, C/C++, web and PHP; the
  `SPC c` code map.
- **Plugin authors:** 17 of 20 extension points in `coverage.toml` are
  still "planned" (modes, decorations, completers, exporters, table
  functions, themes, CLI subcommands…); no permission consent UI, error
  isolation, hot reload, template or `SECURITY.md`.

### Where the README and the Book are wrong

- The README's "Not yet: … plugins" understates the code; it never
  mentions that PDF, pictures and Excel open, the largest group of
  changes in the changelog. Its first-screen GIF is a placeholder.
- The README's speed claims cite the Book; the Book's numbers come from
  one machine and no benchmark runs in CI.
- `book/part-2/latex.org` "Limits and known gaps" contradicts the
  changelog on SyncTeX, the PDF panel, `\multirow` and the corpus.
- `performance.org` gave the binaries as 13 MB and 7 MB (48.9 MB and
  49.7 MB measured; corrected).

### The old task list

`docs/todo.md`: 47 done, 51 partial, 168 open, 22 groups. Of the open and
partial items, 38 are blocked on a decision of the owner or on a release
step only the owner can take (D5, D7, D18, D21, D22, D23, signing, crates.io,
announcements), 46 belong to the plugin system (group 14), and the rest
are the tails of features that shipped. It is a faithful record and it is
no longer a plan: it lists everything at once, in group order, with no
notion of what comes first. `docs/roadmap.md` replaces it as the plan.
