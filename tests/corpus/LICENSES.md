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
| `model/*.org`, `tables/*.org` | Written for Kalem (the tables most of them generated at random by `tools/table-cases.py`) | – | MIT OR Apache-2.0 |
| `latex/synthetic/*.tex` | Written for Kalem | – | MIT OR Apache-2.0 |
| `fetch-extended.sh`, `fetch-latex.sh` | Written for Kalem | – | MIT OR Apache-2.0 |
| `../csv/*` | Written for Kalem; the `libreoffice*` files exported by LibreOffice Calc from data written for Kalem | – | MIT OR Apache-2.0 |
| `../latex/**` | Written for Kalem; the `*.labels` files are the numbers pdflatex wrote for those documents (`tools/latex-labels.sh`) | – | MIT OR Apache-2.0 |
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

## arXiv sources

The LaTeX sources of fifty arXiv papers of January 2024, each submitted under CC BY 4.0 (http://creativecommons.org/licenses/by/4.0/); only their text sources were kept, figures left out (`latex/arxiv/README.md`). Each work remains its authors'. The rest of the study's sample, which may not be redistributed, stays out of the repository, in the draft release `arxiv-sample-2024`.

| Path | Source | Authors | License |
|---|---|---|---|
| `latex/arxiv/README.md` | Written for Kalem | – | MIT OR Apache-2.0 |
| `latex/arxiv/biology/2401.00743/**` | arXiv:2401.00743 (https://arxiv.org/abs/2401.00743) | Kristina Crona, Devin Greene | CC BY 4.0 |
| `latex/arxiv/biology/2401.00746/**` | arXiv:2401.00746 (https://arxiv.org/abs/2401.00746) | Zhichao Zhu, Yang Qi, Wenlian Lu, Jianfeng Feng | CC BY 4.0 |
| `latex/arxiv/biology/2401.01489/**` | arXiv:2401.01489 (https://arxiv.org/abs/2401.01489) | Jason Moore, Alexander Genkin, Magnus Tournoy, Joshua Pughe-Sanford, Rob R. de Ruyter van Steveninck, Dmitri B. Chklovskii | CC BY 4.0 |
| `latex/arxiv/biology/2401.01786/**` | arXiv:2401.01786 (https://arxiv.org/abs/2401.01786) | Diogo Pratas, Armando J. Pinho | CC BY 4.0 |
| `latex/arxiv/biology/2401.01811/**` | arXiv:2401.01811 (https://arxiv.org/abs/2401.01811) | Andrij Rovenchak, Maksym Druchok | CC BY 4.0 |
| `latex/arxiv/biology/2401.02739/**` | arXiv:2401.02739 (https://arxiv.org/abs/2401.02739) | Wasu Top Piriyakulkij, Yingheng Wang, Volodymyr Kuleshov | CC BY 4.0 |
| `latex/arxiv/biology/2401.02756/**` | arXiv:2401.02756 (https://arxiv.org/abs/2401.02756) | Daniel Górniak, Robert Nowak | CC BY 4.0 |
| `latex/arxiv/biology/2401.02989/**` | arXiv:2401.02989 (https://arxiv.org/abs/2401.02989) | Robin Zbinden, Nina van Tiel, Benjamin Kellenberger, Lloyd Hughes, Devis Tuia | CC BY 4.0 |
| `latex/arxiv/biology/2401.03036/**` | arXiv:2401.03036 (https://arxiv.org/abs/2401.03036) | Catarina Dias, Rui Dilão | CC BY 4.0 |
| `latex/arxiv/biology/2401.03248/**` | arXiv:2401.03248 (https://arxiv.org/abs/2401.03248) | Siavash Golkar, Jules Berman, David Lipshutz, Robert Mihai Haret, Tim Gollisch, Dmitri B. Chklovskii | CC BY 4.0 |
| `latex/arxiv/computer-science/2401.00616/**` | arXiv:2401.00616 (https://arxiv.org/abs/2401.00616) | Xiao Pan, Zongxin Yang, Shuai Bai, Yi Yang | CC BY 4.0 |
| `latex/arxiv/computer-science/2401.00632/**` | arXiv:2401.00632 (https://arxiv.org/abs/2401.00632) | Zixu Zhang, Guangsheng Yu, Caijun Sun, Xu Wang, Ying Wang, Ming Zhang, Wei Ni, Ren Ping Liu, Andrew Reeves, Nektarios Georgalas | CC BY 4.0 |
| `latex/arxiv/computer-science/2401.00652/**` | arXiv:2401.00652 (https://arxiv.org/abs/2401.00652) | Xueying Mao, Xiaoxiao Hu, Wanli Peng, Zhenliang Gan, Qichao Ying, Zhenxing Qian, Sheng Li, Xinpeng Zhang | CC BY 4.0 |
| `latex/arxiv/computer-science/2401.00653/**` | arXiv:2401.00653 (https://arxiv.org/abs/2401.00653) | Xuntao Liu, Yuzhou Yang, Qichao Ying, Zhenxing Qian, Xinpeng Zhang, Sheng Li | CC BY 4.0 |
| `latex/arxiv/computer-science/2401.00663/**` | arXiv:2401.00663 (https://arxiv.org/abs/2401.00663) | Zhuoyan Luo, Yicheng Xiao, Yong Liu, Yitong Wang, Yansong Tang, Xiu Li, Yujiu Yang | CC BY 4.0 |
| `latex/arxiv/computer-science/2401.00678/**` | arXiv:2401.00678 (https://arxiv.org/abs/2401.00678) | Samuel Schmidgall, Ji Woong Kim, Alan Kuntz, Ahmed Ezzat Ghazi, Axel Krieger | CC BY 4.0 |
| `latex/arxiv/computer-science/2401.00685/**` | arXiv:2401.00685 (https://arxiv.org/abs/2401.00685) | Mohamed Elmahallawy, Tie Luo, Khaled Ramadan | CC BY 4.0 |
| `latex/arxiv/computer-science/2401.00689/**` | arXiv:2401.00689 (https://arxiv.org/abs/2401.00689) | Mahek Vora, Tom Blau, Vansh Kachhwal, Ashu M. G. Solo, Rohitash Chandra | CC BY 4.0 |
| `latex/arxiv/computer-science/2401.00691/**` | arXiv:2401.00691 (https://arxiv.org/abs/2401.00691) | Xin Chen, Jason M. Klusowski | CC BY 4.0 |
| `latex/arxiv/computer-science/2401.00692/**` | arXiv:2401.00692 (https://arxiv.org/abs/2401.00692) | Hamish Haggerty, Rohitash Chandra | CC BY 4.0 |
| `latex/arxiv/economics/2401.00748/**` | arXiv:2401.00748 (https://arxiv.org/abs/2401.00748) | Vladimir I. Danilov | CC BY 4.0 |
| `latex/arxiv/economics/2401.01804/**` | arXiv:2401.01804 (https://arxiv.org/abs/2401.01804) | Lujie Zhou | CC BY 4.0 |
| `latex/arxiv/economics/2401.02819/**` | arXiv:2401.02819 (https://arxiv.org/abs/2401.02819) | Peter Christensen | CC BY 4.0 |
| `latex/arxiv/economics/2401.02867/**` | arXiv:2401.02867 (https://arxiv.org/abs/2401.02867) | Maxim Senkov, Toygar T. Kerman | CC BY 4.0 |
| `latex/arxiv/economics/2401.03607/**` | arXiv:2401.03607 (https://arxiv.org/abs/2401.03607) | Benjamin Davies | CC BY 4.0 |
| `latex/arxiv/economics/2401.03671/**` | arXiv:2401.03671 (https://arxiv.org/abs/2401.03671) | Itai Arieli, Ivan Geffner, Moshe Tennenholtz | CC BY 4.0 |
| `latex/arxiv/economics/2401.04200/**` | arXiv:2401.04200 (https://arxiv.org/abs/2401.04200) | Thomas van Huizen, Madelon Jacobs, Matthijs Oosterveen | CC BY 4.0 |
| `latex/arxiv/economics/2401.04273/**` | arXiv:2401.04273 (https://arxiv.org/abs/2401.04273) | Maxim Senkov, Arseniy Samsonov | CC BY 4.0 |
| `latex/arxiv/economics/2401.05210/**` | arXiv:2401.05210 (https://arxiv.org/abs/2401.05210) | Enzo Brox, Daniel Goller | CC BY 4.0 |
| `latex/arxiv/economics/2401.06257/**` | arXiv:2401.06257 (https://arxiv.org/abs/2401.06257) | Yaron Azrieli | CC BY 4.0 |
| `latex/arxiv/mathematics/2401.00621/**` | arXiv:2401.00621 (https://arxiv.org/abs/2401.00621) | Xue Zhang, Marco Squassina, Jianjun Zhang | CC BY 4.0 |
| `latex/arxiv/mathematics/2401.00623/**` | arXiv:2401.00623 (https://arxiv.org/abs/2401.00623) | Liejun Shen, Marco Squassina | CC BY 4.0 |
| `latex/arxiv/mathematics/2401.00630/**` | arXiv:2401.00630 (https://arxiv.org/abs/2401.00630) | Colby Austin Brown | CC BY 4.0 |
| `latex/arxiv/mathematics/2401.00648/**` | arXiv:2401.00648 (https://arxiv.org/abs/2401.00648) | Tanya Kaushal Srivastava | CC BY 4.0 |
| `latex/arxiv/mathematics/2401.00665/**` | arXiv:2401.00665 (https://arxiv.org/abs/2401.00665) | Oriol Solé-Pi | CC BY 4.0 |
| `latex/arxiv/mathematics/2401.00666/**` | arXiv:2401.00666 (https://arxiv.org/abs/2401.00666) | Wipawee Tangjai, Witsarut Pho-on, Panupong Vichitkunakorn | CC BY 4.0 |
| `latex/arxiv/mathematics/2401.00702/**` | arXiv:2401.00702 (https://arxiv.org/abs/2401.00702) | Lin Chang, Lin He, Jin Ma | CC BY 4.0 |
| `latex/arxiv/mathematics/2401.00705/**` | arXiv:2401.00705 (https://arxiv.org/abs/2401.00705) | Sanchita Paul, Bapan Das, Avishek Adhikari, Laxman Saha | CC BY 4.0 |
| `latex/arxiv/mathematics/2401.00716/**` | arXiv:2401.00716 (https://arxiv.org/abs/2401.00716) | Stephan Mertens | CC BY 4.0 |
| `latex/arxiv/mathematics/2401.00718/**` | arXiv:2401.00718 (https://arxiv.org/abs/2401.00718) | Peter Olamide Olanipekun | CC BY 4.0 |
| `latex/arxiv/physics/2401.00675/**` | arXiv:2401.00675 (https://arxiv.org/abs/2401.00675) | Parvinder Solanki, Midhun Krishna, Michal Hajdušek, Christoph Bruder, Sai Vinjanampathy | CC BY 4.0 |
| `latex/arxiv/physics/2401.00699/**` | arXiv:2401.00699 (https://arxiv.org/abs/2401.00699) | Euijun Song | CC BY 4.0 |
| `latex/arxiv/physics/2401.00732/**` | arXiv:2401.00732 (https://arxiv.org/abs/2401.00732) | Mojtaba Shahbazi, Mehdi Sadeghi | CC BY 4.0 |
| `latex/arxiv/physics/2401.00750/**` | arXiv:2401.00750 (https://arxiv.org/abs/2401.00750) | Yu-Bo Liu, Jing Zhou, Fan Yang | CC BY 4.0 |
| `latex/arxiv/physics/2401.00796/**` | arXiv:2401.00796 (https://arxiv.org/abs/2401.00796) | Pharnam Bakhshinezhad, Mohammad Mehboudi, Carles Roch i Carceller, Armin Tavakoli | CC BY 4.0 |
| `latex/arxiv/physics/2401.00826/**` | arXiv:2401.00826 (https://arxiv.org/abs/2401.00826) | Arthur Witt, Jangho Kim, Christopher Körber, Thomas Luu | CC BY 4.0 |
| `latex/arxiv/physics/2401.00931/**` | arXiv:2401.00931 (https://arxiv.org/abs/2401.00931) | Anjie Gao, Ian Moult, Sanjay Raman, Gregory Ridgway, Iain W. Stewart | CC BY 4.0 |
| `latex/arxiv/physics/2401.00943/**` | arXiv:2401.00943 (https://arxiv.org/abs/2401.00943) | Suman Das, Sabyasachi Maulik | CC BY 4.0 |
| `latex/arxiv/physics/2401.00954/**` | arXiv:2401.00954 (https://arxiv.org/abs/2401.00954) | Satyam Shekhar Jha, Tal Carmon, Fan Cheng, Lev Deych | CC BY 4.0 |
| `latex/arxiv/physics/2401.00960/**` | arXiv:2401.00960 (https://arxiv.org/abs/2401.00960) | Adam M. Ritchey, S. R. Federman, David L. Lambert | CC BY 4.0 |

### Class and style files inside the arXiv sources

The papers' sources carry class and style files that are not their
authors' work: journals', conferences' and packages' files, which keep
their own terms rather than the paper's CC BY. Those kept are below,
each under the terms it states. The 14 that stated none, and
`aaai25.sty`, which reserved its rights, were taken out on 2026-10-06; a
paper that used one is kept without it (the tests read the `.tex`
files).

| Path | Terms |
|---|---|
| `latex/arxiv/biology/2401.01489/pnas-new.cls` | LPPL (as the file states) |
| `latex/arxiv/biology/2401.02739/fancyhdr.sty` | LPPL (as the file states) |
| `latex/arxiv/computer-science/2401.00663/cvpr.sty` | LPPL (as the file states) |
| `latex/arxiv/computer-science/2401.00678/jabbrv.sty` | LPPL (as the file states) |
| `latex/arxiv/computer-science/2401.00689/acmart.cls` | May be distributed with its source, `acmart.dtx` (LPPL, on CTAN), as the file states |
| `latex/arxiv/computer-science/2401.00689/sn-jnl.cls` | LPPL (as the file states) |
| `latex/arxiv/computer-science/2401.00692/sn-jnl.cls` | LPPL (as the file states) |
| `latex/arxiv/physics/2401.00826/latexml.sty` | Public domain (as the file states) |
| `latex/arxiv/physics/2401.00960/mnras.cls` | LPPL (as the file states) |
