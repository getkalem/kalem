# klm-syntax

The parser of the Kalem format (RFC 0003; Part III of the Book is the
specification): a lossless tree with the byte range of every node
(commands, their names, attributes and content, text runs and formulas),
recovery from ill-formed input exactly as chapter 15 says, `reparse` from
the enclosing top-level block after an edit (the same tree as a full
parse), the canonical form of chapter 14 (`fmt`) and a plain semantic HTML.
It grew out of the spike of task T2.13.2; what the spike found is appendix
B of the RFC.

```sh
cargo run -p klm-syntax -- parse FILE.klm     # the model as JSON
cargo run -p klm-syntax -- fmt FILE.klm       # the canonical form
cargo run -p klm-syntax -- html FILE.klm      # plain HTML
cargo run -p klm-syntax -- check FILE.klm…    # recoveries, and whether it is canonical
cargo run -p klm-syntax -- examples tests/klm-spec/spec book/part-3/*.org
cargo test -p klm-syntax
```

The model is what two parses must agree on: no ranges, runs of whitespace
in text as one space, attributes in the serializer's order of kinds
(`#id`, `.style`s, the positional value, keys as written), empty content
the same as none, a block's verbatim content without its last line feed.

`tests/spec.rs` checks, on every example of the RFC and on the three
samples of `tests/klm-spec/samples/`, that `fmt(fmt(x)) = fmt(x)` and that
`parse(fmt(x))` has the model of `parse(x)`, and that the samples and the
examples are canonical; it runs one malformed input for each recovery rule
of §15 (an unclosed inline command, block, `$` and verbatim block, a stray
`}`, an unknown command, a duplicate `#id`) and one input for each known
ambiguity (a `[` after a command name, letters after it, `]` in a quoted
value, timestamps as values, unbalanced braces in verbatim, `$` in text,
nested inline commands).

`tests/ranges.rs` checks what the editor needs: every node's range inside
its parent and the text it covers written there, an incremental parse
after random edits equal to a full one, no panic on thousands of mangled
inputs, and a megabyte parsed in time (about 50 ms in a release build; a
keystroke about 2.6 ms, most of it moving the blocks after the edit).

Not yet: the per-command attribute order (RFC §21, question 8), attached
blocks as their owner's children in the model (here they are siblings, as
written), and ranges relative to their block, which would make a
keystroke independent of the document's length.
