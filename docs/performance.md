# Performance

The targets are in section 15 of the design document. This page says how each is measured and what the last measurement gave.

## Measuring

```sh
python3 tools/bench-phase1.py
cargo test --release -p kalem-tui --test latency -- --ignored --nocapture
cargo test --release -p kalem-ui --test latency -- --ignored
cargo bench -p org-syntax --bench parse
```

- `tools/bench-phase1.py` builds both release binaries and runs them as a user would. The editors quit after their first frame when `KALEM_EXIT_AFTER_START` is set; times are the median of three runs, memory is the peak resident size. The documents are the Org manual repeated to 1 MB and 10 MB, and to 100 MB as a `.txt` file.
- The terminal latency test types 400 keys in the middle of the Org manual and draws a frame after each, into ratatui's test backend. It also saves a 10 MB document five times.
- The graphical latency test does the same in gpui's test platform with the system's text system: from the key to the painted frame. The GPU's work and the wait for the display are not included.
- The parser benchmark measures full parses and an incremental reparse after a keystroke in a paragraph.

## Results

Measured on 2026-09-28 on an Apple M1 Max (macOS 15.1, Rust 1.98.1), with other work running on the machine.

| Target | Measured | Target |
|---|---|---|
| Cold start, empty document (graphical, to first frame) | 148 ms | under 300 ms |
| Opening a 1 MB document (graphical, to first frame) | 184 ms | under 200 ms |
| 10 MB document, until interactive (graphical) | 783 ms | under 1 s |
| 10 MB document, until interactive (terminal) | 714 ms | under 1 s |
| Keystroke to frame, graphical, p50 / p99 | 8.7 ms / 14.9 ms | under 16 ms / 33 ms |
| Keystroke to frame, terminal, p50 / p99 | 8.0 ms / 13.9 ms | under 16 ms / 33 ms |
| Incremental parse, keystroke in a paragraph | 0.05 ms | under 2 ms |
| Memory, empty document (graphical) | 47 MB | under 80 MB |
| Memory, 10 MB document (graphical) | 252 MB | under 500 MB |
| Binary size (full) | 13 MB | under 40 MB |
| Saving a 10 MB document | 20 ms | under 100 ms |
| Terminal frontend startup (terminal-only build) | 13 ms | under 50 ms |
| CLI `check` on a 1 MB file | 91 ms | under 100 ms |
| CLI `fmt --check` on a 1 MB file | 97 ms | under 100 ms |
| Binary size (terminal-only) | 7 MB | under 15 MB |
| 100 MB plain text file, until interactive (graphical) | 430 ms | under 1 s |
| 100 MB plain text file, until interactive (terminal) | 135 ms | under 1 s |
| Keystroke in a 100 MB plain text file (text, line index and history) | 2.7 ms median, 14 ms p99 | under 16 ms |
| LaTeX parse, 1 MB paper (`latex-syntax`, `--example timing`) | 32 ms | under 100 ms |
| LaTeX incremental parse, keystroke in a paragraph of a 1 MB paper, p50 / p99 | 0.64 ms / 0.83 ms | under 2 ms |
| LaTeX document model, 1 MB paper, first build / after an edit (`latex-model`, `--example timing`) | 21 ms / 6.7 ms | — |

Every target is met. The binary size target includes math fonts, which come with phase 2 (T2.2.1).

## Where the time goes

A full parse of the Org manual takes 42 ms (about 20 MB/s). Opening an Org document is mostly this parse, so the tightest targets are the ones that parse a 1 MB file: `check`, `fmt` and opening in the graphical editor. What made them fit:

- **Parser.** Element regexes run only on lines that start with a byte they can match. Emphasis closers are indexed from the marker bytes, and block ends from one pass per leading byte. Word boundaries between ASCII characters skip the script table, and plain links check the first byte of the link types.
- **Formatter.** `kalem fmt` parses once and maps positions through the table and tag alignments, where it used to parse four times.
- **Graphical editor.** The file given on the command line is read and parsed on another thread while the window system starts. Only a launch from an app bundle without a file waits for the files the system opens.

- **Large files (T2.7a.3).** A keystroke in a 100 MB plain text file costs 2.7 ms at the median and 14 ms at the 99th percentile (`cargo run --release -p kalem-core --example text_timing FILE.txt`), under a frame, so plain text mode keeps contiguous text with its line index rather than a rope or piece table (T1.3.1a). Files over 4 MB are colored a window of 400 lines at a time, from a fresh parser state 200 lines before it (`kalem_highlight::Windowed`), and a line longer than 16 KiB shows the 16 KiB around the cursor with `…` for the rest (`kalem_core::view::plain_line_view`), in every mode.

The CI job for benchmarks (a benchmark per target, regressions blocking a pull request) is not set up yet.
