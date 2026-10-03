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
| `latex/arxiv/README.md` | Written for Kalem | – | MIT OR Apache-2.0 |
| `latex/arxiv/biology/2401.00743/**` | arXiv:2401.00743 (https://arxiv.org/abs/2401.00743), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/biology/2401.00746/**` | arXiv:2401.00746 (https://arxiv.org/abs/2401.00746), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/biology/2401.01489/**` | arXiv:2401.01489 (https://arxiv.org/abs/2401.01489), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/biology/2401.01786/**` | arXiv:2401.01786 (https://arxiv.org/abs/2401.01786), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/biology/2401.01811/**` | arXiv:2401.01811 (https://arxiv.org/abs/2401.01811), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/biology/2401.02739/**` | arXiv:2401.02739 (https://arxiv.org/abs/2401.02739), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/biology/2401.02756/**` | arXiv:2401.02756 (https://arxiv.org/abs/2401.02756), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/biology/2401.02989/**` | arXiv:2401.02989 (https://arxiv.org/abs/2401.02989), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/biology/2401.03036/**` | arXiv:2401.03036 (https://arxiv.org/abs/2401.03036), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/biology/2401.03248/**` | arXiv:2401.03248 (https://arxiv.org/abs/2401.03248), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/computer-science/2401.00616/**` | arXiv:2401.00616 (https://arxiv.org/abs/2401.00616), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/computer-science/2401.00632/**` | arXiv:2401.00632 (https://arxiv.org/abs/2401.00632), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/computer-science/2401.00652/**` | arXiv:2401.00652 (https://arxiv.org/abs/2401.00652), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/computer-science/2401.00653/**` | arXiv:2401.00653 (https://arxiv.org/abs/2401.00653), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/computer-science/2401.00663/**` | arXiv:2401.00663 (https://arxiv.org/abs/2401.00663), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/computer-science/2401.00678/**` | arXiv:2401.00678 (https://arxiv.org/abs/2401.00678), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/computer-science/2401.00685/**` | arXiv:2401.00685 (https://arxiv.org/abs/2401.00685), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/computer-science/2401.00689/**` | arXiv:2401.00689 (https://arxiv.org/abs/2401.00689), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/computer-science/2401.00691/**` | arXiv:2401.00691 (https://arxiv.org/abs/2401.00691), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/computer-science/2401.00692/**` | arXiv:2401.00692 (https://arxiv.org/abs/2401.00692), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/economics/2401.00748/**` | arXiv:2401.00748 (https://arxiv.org/abs/2401.00748), its LaTeX source | 2024 submission | CC0 1.0 (http://creativecommons.org/publicdomain/zero/1.0/) |
| `latex/arxiv/economics/2401.01804/**` | arXiv:2401.01804 (https://arxiv.org/abs/2401.01804), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/economics/2401.02819/**` | arXiv:2401.02819 (https://arxiv.org/abs/2401.02819), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/economics/2401.02867/**` | arXiv:2401.02867 (https://arxiv.org/abs/2401.02867), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/economics/2401.03607/**` | arXiv:2401.03607 (https://arxiv.org/abs/2401.03607), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/economics/2401.03671/**` | arXiv:2401.03671 (https://arxiv.org/abs/2401.03671), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/economics/2401.04200/**` | arXiv:2401.04200 (https://arxiv.org/abs/2401.04200), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/economics/2401.04273/**` | arXiv:2401.04273 (https://arxiv.org/abs/2401.04273), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/economics/2401.05210/**` | arXiv:2401.05210 (https://arxiv.org/abs/2401.05210), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/economics/2401.06257/**` | arXiv:2401.06257 (https://arxiv.org/abs/2401.06257), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/mathematics/2401.00621/**` | arXiv:2401.00621 (https://arxiv.org/abs/2401.00621), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/mathematics/2401.00623/**` | arXiv:2401.00623 (https://arxiv.org/abs/2401.00623), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/mathematics/2401.00630/**` | arXiv:2401.00630 (https://arxiv.org/abs/2401.00630), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/mathematics/2401.00648/**` | arXiv:2401.00648 (https://arxiv.org/abs/2401.00648), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/mathematics/2401.00665/**` | arXiv:2401.00665 (https://arxiv.org/abs/2401.00665), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/mathematics/2401.00666/**` | arXiv:2401.00666 (https://arxiv.org/abs/2401.00666), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/mathematics/2401.00702/**` | arXiv:2401.00702 (https://arxiv.org/abs/2401.00702), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/mathematics/2401.00705/**` | arXiv:2401.00705 (https://arxiv.org/abs/2401.00705), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/mathematics/2401.00716/**` | arXiv:2401.00716 (https://arxiv.org/abs/2401.00716), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/mathematics/2401.00718/**` | arXiv:2401.00718 (https://arxiv.org/abs/2401.00718), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/physics/2401.00675/**` | arXiv:2401.00675 (https://arxiv.org/abs/2401.00675), its LaTeX source | 2024 submission | CC0 1.0 (http://creativecommons.org/publicdomain/zero/1.0/) |
| `latex/arxiv/physics/2401.00699/**` | arXiv:2401.00699 (https://arxiv.org/abs/2401.00699), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/physics/2401.00732/**` | arXiv:2401.00732 (https://arxiv.org/abs/2401.00732), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/physics/2401.00750/**` | arXiv:2401.00750 (https://arxiv.org/abs/2401.00750), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/physics/2401.00796/**` | arXiv:2401.00796 (https://arxiv.org/abs/2401.00796), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/physics/2401.00826/**` | arXiv:2401.00826 (https://arxiv.org/abs/2401.00826), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/physics/2401.00931/**` | arXiv:2401.00931 (https://arxiv.org/abs/2401.00931), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/physics/2401.00943/**` | arXiv:2401.00943 (https://arxiv.org/abs/2401.00943), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/physics/2401.00954/**` | arXiv:2401.00954 (https://arxiv.org/abs/2401.00954), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
| `latex/arxiv/physics/2401.00960/**` | arXiv:2401.00960 (https://arxiv.org/abs/2401.00960), its LaTeX source | 2024 submission | CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/) |
