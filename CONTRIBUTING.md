# Contributing to Kalem

Thank you for your interest in Kalem. This page says how to set up, what the rules are, and how changes are made.

## Before you start

- Read [the Kalem Book](https://getkalem.github.io/kalem): Part I is the manual, Part II says what Kalem does with each format and how that is tested, Part III covers extending Kalem. The design is in [`docs/design_document.md`](docs/design_document.md) (RFC 0001) and [`docs/design_doc2.md`](docs/design_doc2.md) (RFC 0002).
- [`docs/roadmap.md`](docs/roadmap.md) is the plan: milestones with their tasks (`R1.1`, …) and exit criteria, from the evaluation in [`docs/evaluation-2026-10.md`](docs/evaluation-2026-10.md). [`docs/todo.md`](docs/todo.md) and [`docs/history/todo_old.md`](docs/history/todo_old.md) keep the record of what was done and why; their ids (`T2.7h.4`) are still cited.
- For a larger change, open an issue first. Changes to the design go through an RFC ([`rfcs/README.md`](rfcs/README.md)).

## Setup

1. Install Rust with [rustup](https://rustup.rs); `rust-toolchain.toml` selects the toolchain (1.96 at least). On Linux the graphical editor needs the development files of xkbcommon, Wayland, X11 (xcb), fontconfig, freetype and Vulkan; on Debian and Ubuntu, `sudo apt-get install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libxcb1-dev libfontconfig-dev libfreetype-dev libvulkan-dev`.
2. Install the reference tools of the formats you work on: Emacs 29 or newer with Org 9.7 for Org, pandoc and a TeX distribution for LaTeX. CSV, BibTeX, Markdown and plain text need nothing.
3. Build and test with `cargo test --workspace`. The first build downloads the bundled plugins, the components `getkalem/plugins` released (`crates/kalem-components/components.toml` pins each by its SHA-256); to build offline, put them in a folder named by `KALEM_COMPONENTS_DIR`.
4. For the plugin host: `rustup target add wasm32-unknown-unknown` and `cargo install --locked wasm-tools`, without which its tests of the WIT API pass without checking.
5. For Markdown: the CommonMark and GFM specifications, which are CC BY-SA and not in the repository, without which the conformance test passes without checking. CI downloads them so:

   ```sh
   mkdir -p spikes/md-parser/data
   curl -sSfL -o spikes/md-parser/data/commonmark-spec.txt https://raw.githubusercontent.com/commonmark/commonmark-spec/0.31.2/spec.txt
   curl -sSfL -o spikes/md-parser/data/gfm-spec.txt https://raw.githubusercontent.com/github/cmark-gfm/0.29.0.gfm.13/test/spec.txt
   ```

gpui, the graphical editor's toolkit, comes from crates.io as `gpui-unofficial`, a snapshot of each of Zed's release tags, pinned exactly in the workspace `Cargo.toml`; nothing is fetched from Zed's repository, and `tools/check-zed-deps.sh` (run by CI and `tools/pre-push.sh`) fails a change that brings anything from it back.

## The rules

- **Round trip is sacred.** Parsing and printing return the input unchanged, in every format. A mode returns ranges into the file; it never regenerates the file from a tree, and never normalizes text the user did not edit.
- **The reference decides.** Org follows `org-element.el`, LaTeX the TeX engines checked against pandoc, CSV RFC 4180 and the files spreadsheets write, Markdown the CommonMark and GFM suites. An intentional difference goes into the format's known-differences chapter of the Book, with a test.
- **Standard formats are never extended.** Nothing goes into a `.org`, `.tex`, `.csv`, `.bib` or `.md` file that its standard does not define. What a format cannot express is not offered in it.
- **Unknown constructs stay visible**, shown as source, never hidden or guessed.
- **The core has no UI dependencies.** The `org-*`, `latex-*` and `kalem-core` crates do not depend on a GUI or terminal library.
- **Both editors, or the gap recorded.** A feature is done when it works in both editors; what the terminal cannot show is listed in `book/part-4/terminal-parity.org`.
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
| `crates/kalem-cli`, `crates/kalem` | The command-line tools and the `kalem` binary (the package `kalem-editor`, not published yet) |
| `tests/`, `fuzz/`, `tools/` | Corpora, conformance suites, the Emacs comparison scripts, fuzz targets, measurement scripts |
| `book/`, `docs/`, `rfcs/` | The Book; the design documents, task lists and release notes; the RFCs |

Markdown is parsed by [comrak](https://github.com/kivikakk/comrak) through Kalem's fork [`getkalem/comrak`](https://github.com/getkalem/comrak), which fixes the source positions an editor needs; the fixes are offered upstream.

## Commits and the changelog

- Imperative mood ("Add headline parser"), the first line under 72 characters, the task ID or issue when there is one.
- Every user-visible change gets a line under "Unreleased" in [`CHANGELOG.md`](CHANGELOG.md).

## Pushing to main

Several people (and agents) push to `main` the same day. CI lets every run on `main` finish, so a red run there is someone's to fix at once, and pushing on top of a red `main` hides whose it is. Before each push:

- Rebase on `origin/main` (`git fetch origin main && git rebase origin/main`), never merge it into a local branch of `main`.
- Run [`tools/pre-push.sh`](tools/pre-push.sh): formatting, and clippy and the tests of every crate the change touches (all of them when `Cargo.toml` or `Cargo.lock` changed). Linking it as `.git/hooks/pre-push` runs it on every push.
- Push related commits together rather than one at a time; at most about once an hour while the last run on `main` is still going, so runs do not queue behind each other.
- When `main` is red from your push, fix it before anything else; when it is red from someone else's, say so to them rather than push on top.
- The bundled plugins are released components of `getkalem/plugins`, pinned in `crates/kalem-components/components.toml`: a change to a plugin reaches Kalem as a release of it and a new line there. A change to the plugin API needs the plugins released against it before Kalem pins them.
- The plugin API's released WIT interfaces never change (`crates/kalem-plugin/tests/frozen.rs`): a new function goes into a new interface in a file of its own, exported by the worlds in `worlds.wit` (the Book, Part III, "Versions of the plugin API").
- An addition to the plugin API is general and needed: say in its commit what several plugins would do with it, and which plugin uses it now. What one plugin alone needs goes through what exists first (an editor command run with `kalem.run`, a panel, the manifest).

## License

Contributions are dual licensed under MIT OR Apache-2.0, as the [README](README.md#more) says. Files under `tests/corpus` keep their original licenses, listed in `tests/corpus/LICENSES.md`.
