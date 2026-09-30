# org-syntax

A lossless, error tolerant, incremental parser for [Org mode](https://orgmode.org) files, written in Rust.

`org-syntax` is the parser of the [Kalem](https://github.com/kalem-editor/kalem) editor, published as a standalone crate.

- **Emacs compatible.** It follows `org-element.el` (Org 9.7) function by function. On the Org manual, all of Worg and hundreds of thousands of randomly mutated files, every element, object, boundary and property matches Emacs 30.1. The few deliberate differences, all on malformed input, are listed in [`book/part-2/org-known-differences.org`](https://github.com/kalem-editor/kalem/blob/main/book/part-2/org-known-differences.org).
- **Lossless.** The tree ([rowan](https://github.com/rust-analyzer/rowan)) contains every byte: `parse(text).syntax().to_string() == text`.
- **Incremental.** An edit reparses only the affected elements, with a result identical to a full parse. Typing into the 840 KB Org manual takes about 60 µs per keystroke.
- **Fast.** Full parses run at about 12 MB/s.
- **Typed.** `ast::Headline`, `ast::Link`, `ast::Timestamp` and friends expose the properties Emacs computes (`:todo-keyword`, `:path`, `:repeater-unit`, ...).
- **Diagnostics.** Unterminated blocks, stray end lines, invalid timestamps and similar mistakes, in the spirit of `org-lint`.
- **No arbitrary limits.** Deep nesting, huge files and thousands of radio targets are handled; Emacs's own parser stops after a few hundred nesting levels.

```rust
use org_syntax::ast::{AstNode, Headline};

let parse = org_syntax::parse("* TODO Write the parser :work:\n");
let headline = parse.syntax().descendants().find_map(Headline::cast).unwrap();
assert_eq!(headline.todo_keyword().unwrap().text(), "TODO");
```

In-buffer settings (`#+TODO`, `#+SEQ_TODO`, `#+TYP_TODO`, `#+LINK`, `#+STARTUP: odd`), radio targets and `#+SETUPFILE` (through a pluggable loader) are applied as in Emacs. Files with DOS line endings are parsed like Emacs parses them after decoding, and reparse incrementally too.

## Status

Pre-release. The API may change before 0.1.

## Data provenance

The character class, case and script tables and the entity table in `src/tables/` are generated from Emacs by `tools/gen-tables.el`, so that the parser classifies characters exactly as Org does. The entity table comes from Org's `org-entities.el` (GPL-3.0-or-later); how it is licensed in this crate is an open decision ([D18](https://github.com/kalem-editor/kalem/blob/main/book/part-5/decisions/D18-entity-table-provenance.org)) that must be settled before the crate is published.

## License

MIT OR Apache-2.0.
