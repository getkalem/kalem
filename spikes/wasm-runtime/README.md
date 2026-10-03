# WebAssembly runtime spike for D28 (T3.1.0)

Three engines run one guest component, a Markdown parser, and are measured
for Kalem's plugins. Throwaway code, not part of the workspace; the record
of the decision is `book/part-5/decisions/D28-plugin-abi.org`.

- `wit/parser.wit`: the guest's world: `parse` (Markdown to element spans,
  as a mode's parse would cross the boundary), `spin` (a loop, for the time
  budget) and `grow` (allocations, for the memory limit).
- `guest/`: the guest, Rust with wit-bindgen 0.57 and pulldown-cmark 0.13.
- `host/`: the measurements, one engine a build (`--features cranelift`,
  `pulley` or `wasmi`).
- `size/`: the least host that loads the guest and calls it, one build per
  engine, for what each engine adds to a binary.

```sh
cd guest
cargo build --release --target wasm32-unknown-unknown
wasm-tools component new target/wasm32-unknown-unknown/release/spike_guest.wasm \
  -o spike_guest.component.wasm
cd ../host
cargo run --release --features cranelift -- ../guest/spike_guest.component.wasm
cargo run --release --features pulley -- ../guest/spike_guest.component.wasm
cargo run --release --features wasmi -- ../guest/spike_guest.component.wasm
cd ../size
for f in cranelift runtime pulley-runtime wasmi; do
  cargo build --release --features $f && ls -l target/release/spike-size
done
```

The corpus is the repository's Markdown files repeated to 10 MB; the
keystroke case is its first 10 kB.

## Results (2026-10-03, Apple M1 Max)

| | wasmtime 49, Cranelift | wasmtime 49, Pulley | wasmi 0.40 + wasm_component_layer 0.1.18 |
|---|---|---|---|
| Added to a binary | 7.9 MB (0.86 MB without the compiler, loading precompiled components) | 0.96 MB (runtime) | 4.0 MB |
| Compile the 213 kB guest | 109 ms | 124 ms | 4 ms (translated lazily) |
| Load it precompiled (468 kB) | 0.19 ms | 0.15 ms | not offered |
| Instantiate | 0.10 ms first, 0.02 ms after | 0.09 ms, 0.02 ms | 0.21 ms, 0.02 ms |
| Parse 10 MB (native 40 ms) | 54 ms (1.4×) | 1,176 ms (29×) | 684 ms (17×) |
| Parse 10 kB, a keystroke (native 0.02 ms) | 0.04 ms | 1.07 ms | 0.56 ms |
| Fuel: cost on the parse; stops a loop | +31%; yes | +35%; yes | not reachable through the component layer |
| Epochs: cost on the parse; stops a loop | +21%; yes, at the tick | +24%; yes | not reachable |
| Memory limit (64 MB) | 100 MB refused, 10 MB allowed | the same | not reachable |
| 4 threads, an instance each, 10 MB each | 69 ms | 1,223 ms | 1,631 ms |
| Component model | native | native | a third-party layer that builds only against wasmi 0.40 (October 2024): its later releases fail against the runtime layer's 0.5 to 0.7 |

Guest toolchain: built for `wasm32-wasip2`, the same guest imports fifteen
WASI interfaces (stdin, stdout, environment, exit, clocks, random) though
it uses none; built for `wasm32-unknown-unknown` and wrapped by
`wasm-tools component new`, it imports nothing and is 213 kB instead of
249 kB.
