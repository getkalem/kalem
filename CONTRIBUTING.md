# Contributing to Kalem

Thank you for your interest in Kalem. This document explains how to set up a development environment and how changes are made.

## Before you start

- Read [the Kalem Book](https://getkalem.github.io/kalem) (source in `book/`): Part I is the manual, Part II says exactly what Kalem does with each format it opens and against which reference that is tested, Part III is the specification of the Kalem format, Part IV covers extending Kalem and Part V the design. The design documents, [`design_document.md`](design_document.md) (RFC 0001) and [`design_doc2.md`](design_doc2.md) (RFC 0002), describe what Kalem is, what it is not, and how it is built.
- Look at [`todo.md`](todo.md) for the open tasks in the order they are to be done, and at [`todo_old.md`](todo_old.md) for the done tasks, the decisions and the history. Task IDs such as `T2.7h.4` are the same in both and are used in issues and pull requests.
- For larger changes, open an issue first. Changes to the design go through an RFC (see [`rfcs/README.md`](rfcs/README.md)).

## Development setup

1. Install Rust with [rustup](https://rustup.rs). The toolchain is selected by `rust-toolchain.toml`.
2. Install the reference tools of the formats you work on: Emacs 29 or newer with Org 9.7 or newer for Org (the differential tests compare Kalem with `org-element`, Org's commands and its exporters), pandoc and a TeX distribution for LaTeX (`kalem diff-pandoc`, `kalem latex build`). CSV, BibTeX and plain text need nothing.
3. Build and test:

```bash
cargo test --workspace
```

## Rules that keep Kalem working

- **Round-trip is sacred.** For every input, in every format, parsing and printing must return the input unchanged. Never normalize text the user did not edit. A mode returns ranges into the file; it never regenerates the file from a tree.
- **Each format's oracle is the reference.** Org follows what `org-element.el` does where the Org Syntax document is ambiguous; LaTeX follows what the TeX engines accept, checked against pandoc's reader; CSV follows RFC 4180 and the files spreadsheets write; Markdown follows the CommonMark and GFM suites, with comrak as the reference where they are silent. Record intentional differences in the format's known-differences chapter of the Book (`book/part-2/org-known-differences.org`, `book/part-2/latex-known-differences.org`), and make each entry a test.
- **Standard formats are never extended.** Kalem writes nothing into a `.org`, `.tex`, `.csv`, `.bib` or `.md` file that its standard does not define. What a format cannot express is not offered in it; it belongs to the Kalem format, `.klm`, whose changes go through its specification (Part III) and its conformance suite (`tests/klm-spec`).
- **Unknown constructs stay visible.** What a mode does not understand is shown as source, never hidden or guessed.
- **The core has no UI dependencies.** The `org-*`, `latex-*` and `kalem-core` crates must not depend on a GUI or terminal library.
- **Both editors, or the gap recorded.** A feature is done when it works in the graphical and the terminal editor; what the terminal cannot show is listed in `book/part-5/terminal-parity.org`.
- **The Book changes with the code.** A pull request that changes behavior changes the Book (`book/`) in the same pull request; `kalem book check` must pass.
- **Every user action is a command.** Frontends and plugins go through the command registry, and every key is configurable.

## Code style

- `cargo fmt --all` and `cargo clippy --workspace --all-targets` must be clean. CI treats warnings as errors.
- Public items have documentation comments.
- Prefer small, focused pull requests.
- Tests go next to the code (`#[cfg(test)]`) or in the crate's `tests/` directory. Parser changes need snapshot tests and, where possible, a corpus or differential test against the format's oracle.

## Test corpus

Real-world files live in `tests/corpus` (Org, LaTeX, Kalem format), `tests/latex` and `tests/csv`. Only add files whose license allows redistribution, and record each file in `tests/corpus/LICENSES.md`. Never add personal data.

## Commit messages

Use the imperative mood ("Add headline parser", not "Added"). Reference the task ID or issue when there is one. Keep the first line under 72 characters.

## Changelog

Add a line to the "Unreleased" section of [`CHANGELOG.md`](CHANGELOG.md) for every user-visible change.

## License

By contributing you agree that your contributions are dual licensed under MIT OR Apache-2.0, as described in the [README](README.md#license).
