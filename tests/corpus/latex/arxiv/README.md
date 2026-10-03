# arXiv papers under CC BY 4.0 and CC0

Fifty LaTeX sources from the arXiv sample of the coverage study (T2.7h.1,
T2.7h.30): ten per field (biology, computer science, economics,
mathematics, physics), papers submitted in 2024 whose authors released
them under CC BY 4.0 or CC0 1.0, as arXiv's metadata says
(`tools/arxiv-licences.py` in the arXiv workflow).

Each folder is named after the paper's arXiv identifier; the paper, its
authors and its licence are at `https://arxiv.org/abs/IDENTIFIER`. Each
work remains its authors'. The only change: only the text sources were
kept (`.tex`, `.bib`, `.bbl`, `.sty`, `.cls`, `.bst`); figures and other
files were left out. Every paper is listed with its licence in
`tests/corpus/LICENSES.md`.

The rest of the sample is not redistributable and stays out of the
repository (the workflow keeps it as a draft release).
