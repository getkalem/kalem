# Kalem format parser spike (T2.13.2)

A throwaway parser for the grammar of RFC 0003 (`rfcs/0003-kalem-format.md`,
draft 0.2): it parses to a tree with byte ranges, recovers from ill-formed
input as §15 says, writes the canonical form of §14 and a plain semantic
HTML. It exists to find where the grammar is wrong or silent; what it found
is appendix B of the RFC. `klm-syntax` (T2.13.3) replaces it. Throwaway
code, not part of the workspace.

```sh
cargo run -- parse FILE.klm          # the model as JSON
cargo run -- fmt FILE.klm            # the canonical form
cargo run -- html FILE.klm           # plain HTML
cargo run -- check FILE.klm…         # recoveries, and whether it is canonical
cargo run -- examples ../../rfcs/0003-kalem-format.md ../../tests/klm-spec/rfc
cargo test                           # the RFC's examples, the samples, §15, ambiguities
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

What it leaves to `klm-syntax`: the per-command attribute order (RFC §21,
question 8), attached blocks as their owner's children in the model (here
they are siblings, as written), incremental reparsing, and the lossless
tree for the editor (this one drops blank lines and indentation).
