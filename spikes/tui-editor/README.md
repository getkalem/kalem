# Terminal editor spike (T0.8, D14)

A terminal Org editor on ratatui + crossterm, used to decide the terminal UI
stack. The findings are in `docs/decisions/D14-terminal-ui-stack.md`. It
compiles the gpui spike's `view.rs` unchanged, so both spikes share one
display model. Throwaway code, not part of the workspace.

```sh
cargo run --release -- FILE.org                          # interactive
cargo run --release -- --detect                          # terminal capabilities
cargo run --release -- FILE.org --snapshot 80x24         # render once, print
cargo run --release -- FILE.org --bench-scroll 2000      # headless frame cost
cargo run --release -- FILE.org --bench-type 300
```

Keys: arrows, Home/End, PageUp/PageDown, Backspace/Delete, Enter, `ctrl-s`
save, `ctrl-q` quit. Click a checkbox to toggle it; the wheel scrolls.
