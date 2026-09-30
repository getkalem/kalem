# Math rendering spike (T0.7, D4)

Renders `corpus.txt` (100 LaTeX formulas) with typst + MiTeX and with RaTeX,
and reports coverage and timing. The findings are in
`book/part-5/decisions/D4-math-engine.org`. Throwaway code, not part of the workspace.

```sh
cargo run --release -- corpus.txt --out /tmp/math   # SVGs and a TSV per engine
cargo run --release -- corpus.txt --engine ratex
cargo build --release --no-default-features --features ratex   # size of one engine
```

`mitex-specs/` holds the Typst definitions that MiTeX output needs, copied from
MiTeX 0.2.7 (Apache-2.0).
