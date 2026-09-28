# D4: Math engine for inline formulas

- Status: **Decided** (2026-09-28)
- Decision: **RaTeX** (`ratex-parser`, `ratex-layout`, `ratex-svg`; KaTeX-compatible LaTeX math in pure Rust) renders formulas in the editor. The typst + MiTeX route is not used for inline math.
- Tasks: T0.7.1 to T0.7.4
- Evaluation code: `spikes/math-render` (standalone Cargo project); the corpus is `spikes/math-render/corpus.txt`

## Candidates

The design listed MiTeX + typst (A) and ReX (B). ReX has no maintained release: the `rex` crate on crates.io (4.0.0) is an unrelated programming language. Since the design was written, several pure-Rust TeX math engines have appeared. RaTeX was the only one with meaningful adoption and a release history, so it replaced ReX as candidate B.

| Crate | Status on 2026-09-28 | Considered |
|---|---|---|
| typst 0.15.1 + MiTeX 0.2.7 | typst very active (2.8 M downloads); MiTeX's last crates.io release is 0.2.4 (2024-06), the repository is at 0.2.7 | Candidate A |
| RaTeX 0.1.14 | Created 2026-03, 22 releases, last commit 2026-09-23, 1,500 GitHub stars, 80 k recent downloads | Candidate B |
| katex-rs 0.3.0 | A KaTeX port that produces HTML and MathML | No: needs a DOM |
| mathtex, latex-rust, leaf-math, formulary | Weeks old, under 2,000 downloads each | No: too young |

## Corpus

100 formulas in 11 groups: basics, fractions and roots, big operators, delimiters, accents and alphabets, matrices, amsmath environments, text and spacing, relations and arrows, braces and decorations, and a mixed group (physics, chemistry, probability). 46 are display formulas, 54 inline. Every formula was rendered by both engines and compared side by side by eye.

## Results

| Measure | typst + MiTeX | RaTeX |
|---|---|---|
| Rendered | 88 / 100 | 98 / 100 |
| Rendered but wrong (visual check) | 3: `array` loses its rules, `\tag` and equation numbers disappear, `\xleftarrow[g]{}` prints literal brackets | 0 found |
| Failures | 5 from symbol names MiTeX emits but typst 0.15 renamed (`\cap`, `\partial`, `\hbar`, `\middle`); 7 unsupported: `\mathscr`, `gather*`, `multline`, `alignat`, `\mbox`, `\color`, `\ce` | `multline`, `\mbox` |
| Start-up (first formula) | 11 to 17 ms | 1 ms |
| Layout and SVG per formula, p50 / p90 | 0.25 / 0.4 ms | 0.38 / 0.6 ms (glyphs as outlines) |
| SVG to pixels at 2×, p50 | 0.27 ms | 0.31 ms |
| Binary size added (stripped) | +40.1 MB with typst's bundled fonts; about +34.7 MB with only the New Computer Modern fonts it needs | +4.6 MB including the KaTeX fonts (548 KB) |
| Input language | Typst math: LaTeX must be translated, and the translation lags typst releases | LaTeX as KaTeX accepts it: the syntax Org documents already use |
| Output | typst frames, SVG | A display list of glyph outlines, rules and paths; SVG, PNG and PDF renderers |
| License | Apache-2.0; fonts under the GUST Font License | MIT; KaTeX fonts under SIL OFL 1.1 |

Layout quality is comparable where both succeed. Both follow TeX's rules: fraction bars, script placement, stretchy delimiters and big operators match closely.

End to end in the gpui spike (D3), RaTeX renders a formula to a painted image in 0.2 to 0.5 ms on a background thread. Scrolling through 9,000 distinct formulas keeps 120 Hz.

## Why RaTeX

- **Coverage of the syntax people write.** Org documents contain LaTeX, and KaTeX's dialect is the de facto standard for LaTeX math outside TeX. Translation to Typst adds a second failure point, and it fails silently in places.
- **Size.** Typst alone would take most of the 40 MB binary budget (§15). RaTeX costs 4.6 MB.
- **Start-up.** No library or font set-up at first use.
- **Output.** The display list can later be painted directly with gpui paths and glyphs, skipping SVG and rasterization, and it maps cleanly to terminal image protocols.

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| Young project (6 months), one main author (about 83% of commits) | Pin versions; keep RaTeX behind an `org-math` trait so an engine can be swapped; the MIT license allows forking. Contribute fixes upstream first (§4.7). |
| `multline` and `\mbox` are missing | Report upstream. Until fixed, `\mbox` can be mapped to `\text` and `multline` to `gather` in Kalem's pre-processing. |
| `\newcommand` from `#+LATEX_HEADER` (§9.2) | Verified when `org-math` was built: RaTeX takes macros, but like KaTeX it refuses `\newcommand` for a name KaTeX already defines (`\R`, `\N`), which papers often do. `org-math` turns every definition into `\def`, which may redefine. |
| A formula that fails to render | Show the source with a red frame (§9.2); never hide text. |

## Not decided here

Typst remains an option for **exporting whole documents** (a Typst backend or PDF through typst) under D5 and §9.3. That is a separate trade-off with different size and quality considerations, and it would be an optional component.
