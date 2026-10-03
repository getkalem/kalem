# Test corpus license register

Every file in `tests/corpus` is listed here with its source and license. Corpus files are test data. They are not part of any published crate (`tests/` is outside every crate's package).

Rules:

- Only add files whose license allows redistribution.
- Never add personal documents or files containing personal data.
- Record the upstream version or commit, so the file can be refreshed.
- Synthetic files written for Kalem are licensed like the project (MIT OR Apache-2.0).

| Path | Source | Version | License |
|---|---|---|---|
| `org-mode/org-manual.org` | Org mode `doc/org-manual.org` | release_9.7.11 (6a5d0ed3) | GFDL-1.3-or-later |
| `org-mode/org-guide.org` | Org mode `doc/org-guide.org` | release_9.7.11 (6a5d0ed3) | GFDL-1.3-or-later |
| `org-mode/doc-setup.org` | Org mode `doc/doc-setup.org` (setup file of the manual) | release_9.7.11 (6a5d0ed3) | GFDL-1.3-or-later |
| `org-mode/ORG-NEWS.org` | Org mode `etc/ORG-NEWS` | release_9.7.11 (6a5d0ed3) | GPL-3.0-or-later (part of Org mode) |
| `org-mode/examples/**` | Org mode `testing/examples/` | release_9.7.11 (6a5d0ed3) | GPL-3.0-or-later (part of Org mode) |
| `worg/org-syntax.org` | Worg `org-syntax.org` (the Org Syntax specification) | 22fc0631 | GFDL-1.3-or-later (text), GPL-3.0-or-later (code examples) |
| `synthetic/*.org` | Written for Kalem | – | MIT OR Apache-2.0 |
| `markdown/readmes/ripgrep.md` | `README.md` of github.com/BurntSushi/ripgrep | 3fce3b5b | Unlicense OR MIT |
| `markdown/readmes/rust.md` | `README.md` of github.com/rust-lang/rust | b6e4b5c4 | MIT OR Apache-2.0 |
| `markdown/readmes/serde.md` | `README.md` of github.com/serde-rs/serde | 6693a89c | MIT OR Apache-2.0 |
| `markdown/readmes/tokio.md` | `README.md` of github.com/tokio-rs/tokio | 86678437 | MIT |
| `markdown/readmes/bat.md` | `README.md` of github.com/sharkdp/bat | 4608fc95 | MIT OR Apache-2.0 |
| `markdown/readmes/fd.md` | `README.md` of github.com/sharkdp/fd | 3460b1e9 | MIT OR Apache-2.0 |
| `markdown/readmes/fzf.md` | `README.md` of github.com/junegunn/fzf | b1be3a8b | MIT |
| `markdown/readmes/react.md` | `README.md` of github.com/facebook/react | 278794d7 | MIT |
| `markdown/readmes/vscode.md` | `README.md` of github.com/microsoft/vscode | 253b7648 | MIT |
| `markdown/readmes/mermaid.md` | `README.md` of github.com/mermaid-js/mermaid | 97b34515 | MIT |
| `markdown/readmes/kubernetes.md` | `README.md` of github.com/kubernetes/kubernetes | 12eb5840 | Apache-2.0 |
| `markdown/vault/foam-docs/**` | `docs/` of github.com/foambubble/foam, a Foam vault of notes with wiki links (images left out); its licence is `markdown/vault/foam-docs/LICENSE.txt` | 2a02ccd | MIT |
| `math/katex-corpus.txt` | 1,000 formulas from the Open Logic Project (github.com/OpenLogicProject/OpenLogic) and the HoTT book (github.com/HoTT/book), extracted by `tools/math-corpus.py` | OpenLogic 1e960bef, HoTT 578b85cc | CC BY-SA 3.0 (the HoTT book's formulas; the Open Logic Project's are CC BY 4.0) |

## Extended corpus

`fetch-extended.sh` clones the full Org mode repository (release_9.7.11) and Worg (commit 22fc0631) into `.cache/`. Those files are not committed. Worg text is GFDL-1.3-or-later and its code examples are GPL-3.0-or-later; Org mode is GPL-3.0-or-later and its manual is GFDL-1.3-or-later.
