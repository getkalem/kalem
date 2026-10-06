# gpui editor spike (T0.6, D3)

A minimal WYSIWYG Org editor used to decide on gpui as Kalem's UI framework.
The findings are in `book/part-4/decisions/D3-ui-framework.org`. This is throwaway
code: it is not part of the workspace and is not maintained.

```sh
cargo run --release -- FILE.org                  # interactive
cargo run --release -- FILE.org --bench-scroll 600
cargo run --release -- FILE.org --bench-jump 300
cargo run --release -- FILE.org --bench-type 300
cargo run --release -- FILE.org --script         # fold, click widgets, print layouts
```

Keys: arrows, Home/End, Backspace/Delete, Enter, `cmd-c`/`cmd-v`, `cmd-o` to
open, `cmd-shift-s` to save as. Click a checkbox to toggle it, a formula to
edit its source, a fold arrow to fold a subtree.

Benchmarks need a visible window: macOS stops frames while the screen is locked
or the window is hidden, which shows up as long frame intervals but not in the
"main-thread work" numbers.
