# D14: Terminal UI stack

- Status: **Decided** (2026-09-28)
- Decision: **ratatui 0.30 + crossterm 0.29**, with **ratatui-image 11** for graphics (kitty, iTerm2, sixel, half-block fallback).
- Tasks: T0.8.1 to T0.8.4
- Evaluation code: `spikes/tui-editor` (standalone Cargo project, not part of the workspace)

## What was built

A terminal Org editor of about 1,200 lines. It compiles the gpui spike's `view.rs` unchanged through `#[path]`, so both frontends draw from one display model. That is the frontend-neutral view model planned in T1.3.8.

| File | Content |
|---|---|
| `render.rs` | Display segments to terminal cells: graphemes with styles, level glyphs for headlines, checkbox glyphs, Unicode formulas, OSC 8 link targets, wrapping at spaces |
| `caps.rs` | Capability detection: environment hints, then XTVERSION, DECRQM 2026 (synchronized output), kitty keyboard flags and DA1, answered by the terminal |
| `math.rs` | Inline formulas as one-line Unicode (`x² + y²`, `∑ᵢ₌₁ⁿ`, `1/2`); display formulas as RaTeX images in the terminal's foreground color |
| `app.rs` | Editing with incremental reparse, grapheme motion, mouse (click to place the cursor or toggle a checkbox, wheel scrolling), save, images for display formulas and image links, status line |

Hidden markers follow the GUI rule: away from the cursor, `*bold*` shows as bold, headline stars become `◉ ○ ◈ ◇` with indentation, and links show their description underlined. On the cursor's line the source appears.

## Results

| Task | Result |
|---|---|
| T0.8.1 Styled, editable paragraphs with hidden markers and cursor reveal | Works. Verified with rendering tests on ratatui's test backend (styles, marker reveal, OSC 8 cells, checkbox clicks) and end to end through a pseudo-terminal: typed text, a mouse click that toggles a checkbox, save and quit, checked against the saved file. |
| T0.8.2 Capability detection | Implemented; reply parsing is unit-tested with kitty and xterm replies. The test caught a bug: the DA1 reply was never recognized because another `ESC [ ?` reply came first. In the pseudo-terminal the spike detected synchronized output and used it. **Not yet run in a real terminal** (see below). |
| T0.8.3 Images and formula images | ratatui-image picks the protocol from the terminal's answers. Impersonating kitty, iTerm2 and a sixel terminal in the pseudo-terminal, the spike emitted kitty graphics, iTerm2 `OSC 1337 File=` and sixel `DCS … q` sequences respectively, two images each (a display formula and a PNG). The iTerm2 payloads decode to the expected images. **Not yet looked at in a real terminal.** |

### Performance

Frame cost measured with ratatui's test backend at 120 × 50 cells: building the display, wrapping and filling the buffer, without terminal I/O. Apple M1 Max.

| Scenario | p50 | p99 | Max |
|---|---|---|---|
| Scroll the Org manual one line per frame, 2,000 frames | 0.50 ms | 0.71 ms | 0.84 ms |
| Scroll the 4.2 MB, 117,850-line file | 0.61 ms | 0.95 ms | 1.19 ms |
| Scroll a file with two formulas per line (Unicode) | 1.67 ms | 1.90 ms | 3.51 ms |
| Type 300 characters, Org manual (reparse p50 75 µs) | 1.06 ms | 1.23 ms | 1.33 ms |
| Type 300 characters, 4.2 MB file (reparse p50 253 µs) | 1.38 ms | 1.81 ms | 1.88 ms |

A first version of the follow-the-cursor logic measured every line between the top of the view and the cursor on every frame. That was quadratic on long jumps, and it also snapped wheel scrolling back to the cursor. It now walks up from the cursor at most one screen, and only when the cursor moved.

### Size

The stripped spike binary (ratatui, crossterm, ratatui-image, RaTeX with its fonts, resvg, `org-syntax`) is 11.3 MB with the spike's release settings and **6.7 MB with fat LTO**. The design's budget for a terminal-only build is 15 MB (§15).

## Findings that shape the design

- **One view model, two frontends.** The GUI and terminal spikes share `view.rs` verbatim. Only widget presentation differs: a checkbox is a drawn box in the GUI and a glyph in the terminal, and a formula is an image in the GUI and Unicode or an image in the terminal.
- **Hyperlinks.** ratatui has no hyperlink API. OSC 8 works by putting the escape sequence into the first and last cell of a link with `CellDiffOption::ForcedWidth`. Because ratatui redraws only changed cells, a link must be redrawn as a whole when any of its cells changes; `kalem-tui` needs a small helper for that.
- **Images are block-level.** Terminal images occupy whole cells. Display formulas and image links become image blocks; inline formulas stay Unicode.
- **Transfer size.** An uncompressed kitty transmission of two small images was 200 KB, against 7 KB for iTerm2 PNGs and 2 KB for sixel. Over SSH, ratatui-image's opt-in kitty compression should be enabled.
- **Queries need care.** Terminals answer in order, and ratatui-image stops reading at the status report. Kalem's own queries must finish before ratatui-image's, and every query needs a timeout with DA1 as the sentinel.

## Alternatives

- **termwiz** (WezTerm's library) has hyperlinks and images built in, but a much smaller ecosystem. ratatui can switch to it later through its termwiz backend if crossterm falls short.
- **A custom renderer** would repeat what ratatui's buffer diffing already does.

## Still to verify by hand

The spike could not drive a real terminal here: the app's terminal panel had a busy shell, and driving iTerm2 needs a macOS automation permission that only the user can grant. Checklist:

```sh
cd spikes/tui-editor && cargo build --release
./target/release/tui-editor-spike --detect          # in iTerm2, kitty, WezTerm, Terminal.app
./target/release/tui-editor-spike /path/to/file.org # images, links, mouse, editing
```

Expected: iTerm2 reports `Iterm2` graphics and OSC 8 hyperlinks; kitty reports `Kitty` and kitty keyboard support; Terminal.app falls back to `Halfblocks` without hyperlinks.
