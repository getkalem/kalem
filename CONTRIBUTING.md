# Contributing to Kalem

Thank you for your interest in Kalem. This document explains how to set up a development environment and how changes are made.

## Before you start

- Read the [design document](design_document.md). It describes what Kalem is, what it is not, and how it is built.
- Look at [`todo.md`](todo.md) for the current phase and open tasks. Task IDs such as `T0.3.4` are used in issues and pull requests.
- For larger changes, open an issue first. Changes to the design go through an RFC (see [`rfcs/README.md`](rfcs/README.md)).

## Development setup

1. Install Rust with [rustup](https://rustup.rs). The toolchain is selected by `rust-toolchain.toml`.
2. Install Emacs 29 or newer (Org 9.7 or newer) if you work on the parser. The differential tests compare Kalem with Emacs's `org-element`.
3. Build and test:

```bash
cargo test --workspace
```

## Rules that keep Kalem working

- **Round-trip is sacred.** For every input, parsing and printing must return the input unchanged. Never normalize text the user did not edit.
- **Emacs is the reference.** When the Org Syntax document is ambiguous, Kalem follows what `org-element.el` does. Record intentional differences in `book/part-2/org-known-differences.org`.
- **The core has no UI dependencies.** `org-*` crates and `kalem-core` must not depend on a GUI or terminal library.
- **The Book changes with the code.** A pull request that changes behavior changes the Book (`book/`) in the same pull request; `kalem book check` must pass.
- **Every user action is a command.** Frontends and plugins go through the command registry.
- **Documents stay valid Org.** Plugins and features may give meaning to Org's extension points, but never invent new syntax.

## Code style

- `cargo fmt --all` and `cargo clippy --workspace --all-targets` must be clean. CI treats warnings as errors.
- Public items have documentation comments.
- Prefer small, focused pull requests.
- Tests go next to the code (`#[cfg(test)]`) or in the crate's `tests/` directory. Parser changes need snapshot tests and, where possible, a corpus or differential test.

## Test corpus

Real-world Org files live in `tests/corpus`. Only add files whose license allows redistribution, and record each file in `tests/corpus/LICENSES.md`. Never add personal data.

## Commit messages

Use the imperative mood ("Add headline parser", not "Added"). Reference the task ID or issue when there is one. Keep the first line under 72 characters.

## Changelog

Add a line to the "Unreleased" section of [`CHANGELOG.md`](CHANGELOG.md) for every user-visible change.

## License

By contributing you agree that your contributions are dual licensed under MIT OR Apache-2.0, as described in the [README](README.md#license).
