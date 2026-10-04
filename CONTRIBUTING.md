# Contributing to Kalem

Thank you for your interest in Kalem. This page says how to set up, what the rules are, and how changes are made.

## Before you start

- Read [the Kalem Book](https://getkalem.github.io/kalem): Part I is the manual, Part II says what Kalem does with each format and how that is tested, Part III covers extending Kalem. The design is in [`docs/design_document.md`](docs/design_document.md) (RFC 0001) and [`docs/design_doc2.md`](docs/design_doc2.md) (RFC 0002).
- [`docs/roadmap.md`](docs/roadmap.md) is the plan: milestones with their tasks (`R1.1`, …) and exit criteria, from the evaluation in [`docs/evaluation-2026-10.md`](docs/evaluation-2026-10.md). [`docs/todo.md`](docs/todo.md) and [`docs/history/todo_old.md`](docs/history/todo_old.md) keep the record of what was done and why; their ids (`T2.7h.4`) are still cited.
- For a larger change, open an issue first. Changes to the design go through an RFC ([`rfcs/README.md`](rfcs/README.md)).

## Setup

1. Install Rust with [rustup](https://rustup.rs); `rust-toolchain.toml` selects the toolchain.
2. Install the reference tools of the formats you work on: Emacs 29 or newer with Org 9.7 for Org, pandoc and a TeX distribution for LaTeX. CSV, BibTeX, Markdown and plain text need nothing.
3. Build and test with `cargo test --workspace`.

The first build fetches gpui from Zed's repository: about 400 MB of git history and a 100 MB checkout in Cargo's cache, once. Only gpui and gpui_platform come from there, declared once in the workspace `Cargo.toml`; `tools/check-zed-deps.sh` (run by CI and `tools/pre-push.sh`) fails on any other. Building without that repository is roadmap item R2.7.

## The rules

- **Round trip is sacred.** Parsing and printing return the input unchanged, in every format. A mode returns ranges into the file; it never regenerates the file from a tree, and never normalizes text the user did not edit.
- **The reference decides.** Org follows `org-element.el`, LaTeX the TeX engines checked against pandoc, CSV RFC 4180 and the files spreadsheets write, Markdown the CommonMark and GFM suites. An intentional difference goes into the format's known-differences chapter of the Book, with a test.
- **Standard formats are never extended.** Nothing goes into a `.org`, `.tex`, `.csv`, `.bib` or `.md` file that its standard does not define. What a format cannot express is not offered in it.
- **Unknown constructs stay visible**, shown as source, never hidden or guessed.
- **The core has no UI dependencies.** The `org-*`, `latex-*` and `kalem-core` crates do not depend on a GUI or terminal library.
- **Both editors, or the gap recorded.** A feature is done when it works in both editors; what the terminal cannot show is listed in `book/part-5/terminal-parity.org`.
- **The Book changes with the code.** A pull request that changes behavior changes `book/` too, and `kalem book check` passes. `book/chapters.toml` maps code to the chapter that describes it; on a pull request the Book's workflow runs `kalem book check book --changed origin/main` and fails when mapped code changed without its chapter, unless the pull request carries the label `book-unchanged` and its description says why.
- **Every user action is a command**, reached through the command registry, with a configurable key.

## Code and tests

- `cargo fmt --all` and `cargo clippy --workspace --all-targets` are clean; CI treats warnings as errors.
- Public items have documentation comments. Prefer small, focused pull requests.
- Tests go next to the code or in the crate's `tests/`. Parser changes need snapshot tests and, where possible, a corpus or differential test against the format's reference.
- Real-world files live in `tests/corpus`, `tests/latex` and `tests/csv`. Add only files whose license allows redistribution, record each in `tests/corpus/LICENSES.md`, and never add personal data.

## Where things are

| Path | Contents |
|---|---|
| `crates/org-*` | Org: the lossless incremental parser (`org-syntax`), the document model, editing commands, tables and formulas, exporters, citations, formulas drawn natively |
| `crates/latex-*` | The LaTeX parser and model |
| `crates/kalem-core` | The editor's model, shared by both frontends: documents and modes, commands, keymaps, settings, the file manager, projects |
| `crates/kalem-ui`, `crates/kalem-tui` | The graphical and the terminal editor, with `gpui-rich-text` and `tui-rich-text` |
| `crates/kalem-cli`, `crates/kalem` | The command-line tools and the `kalem` binary (published as `kalem-editor`) |
| `tests/`, `fuzz/`, `tools/` | Corpora, conformance suites, the Emacs comparison scripts, fuzz targets, measurement scripts |
| `book/`, `docs/`, `rfcs/` | The Book; the design documents, task lists and release notes; the RFCs |

Markdown is parsed by [comrak](https://github.com/kivikakk/comrak) through Kalem's fork [`getkalem/comrak`](https://github.com/getkalem/comrak), which fixes the source positions an editor needs; the fixes are offered upstream.

## Commits and the changelog

- Imperative mood ("Add headline parser"), the first line under 72 characters, the task ID or issue when there is one.
- Every user-visible change gets a line under "Unreleased" in [`CHANGELOG.md`](CHANGELOG.md).

## Pushing to main

Several people (and agents) push to `main` the same day. CI lets every run on `main` finish, so a red run there is someone's to fix at once, and pushing on top of a red `main` hides whose it is. Before each push:

- Rebase on `origin/main` (`git fetch origin main && git rebase origin/main`), never merge it into a local branch of `main`.
- Run [`tools/pre-push.sh`](tools/pre-push.sh): formatting, the plugin pins, and clippy and the tests of every crate the change touches (all of them when `Cargo.toml` or `Cargo.lock` changed). Linking it as `.git/hooks/pre-push` runs it on every push.
- Push related commits together rather than one at a time; at most about once an hour while the last run on `main` is still going, so runs do not queue behind each other.
- When `main` is red from your push, fix it before anything else; when it is red from someone else's, say so to them rather than push on top.
- A change to the viewer contract (`crates/kalem-viewer`) and the bundled plugins' bump go in one push: the three plugins are pinned at one revision of `getkalem/plugins` ([`tools/check-plugin-pins.sh`](tools/check-plugin-pins.sh)).

## License

Contributions are dual licensed under MIT OR Apache-2.0, as the [README](README.md#more) says. Files under `tests/corpus` keep their original licenses, listed in `tests/corpus/LICENSES.md`.
