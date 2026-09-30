#!/bin/sh
# Fetch the LaTeX corpus (T2.7h.30): real books, papers, theses, journal
# templates and the test files of other LaTeX tools, at pinned commits,
# into .cache/latex. The files are not committed (their licenses vary);
# the synthetic edge cases in tests/corpus/latex/synthetic are.
#
# Then: cargo run --release -p kalem-core --example latex_corpus -- .cache/latex
set -eu

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CACHE="$ROOT/.cache/latex"
mkdir -p "$CACHE"

for entry in \
  "ElegantLaTeX/ElegantBook 8b90c11e4a5ffd9d1e07174011303c133093d09c" \
  "HoTT/book 578b85cc8d586b1677ec4335148adeb443057d24" \
  "James-Yu/LaTeX-Workshop c5bdf430a1577e2df28139ed4b1bd5c9ad859865" \
  "OpenLogicProject/OpenLogic 1e960beff9ed7835bf3e3f1335e21af3439cd107" \
  "PetarV-/TikZ 86aee049f5355d77c598b936399e42caf8941e60" \
  "cplusplus/draft fc466c52db728f5dcd47a6b6d11e5a0f9e2fb444" \
  "exacity/deeplearningbook-chinese a03e98d2298528a13779f9fc4c8e7f8c1bd204f7" \
  "google-research/arxiv-latex-cleaner bcc1460cc4be72ddac08f22c54b23f23f14b102d" \
  "jgm/pandoc 7347d5e6eb9bbd36b12d6c724523d0c57083dbdc" \
  "kks32/phd-thesis-template 8d6c2d59790d93ef58e50f908d98d49e00b4fb1d" \
  "latex-lsp/texlab 4cc18b37c0b46baf39189f173d1bd7468d3f56e1" \
  "latex3/latex2e 656049bc4e8aa77b1fd4a2d41762a1cff44db356" \
  "overleaf/overleaf e039ad26c5bf5422eb57b89fc7e57c75055e631d" \
  "posquit0/Awesome-CV 6701180c71479588dae5d895c4a10a6a572a40a0" \
  "rstudio/rticles 2a0ee075435f5f4081145e18fa39aed6d15ed935" \
  "sb2nov/resume 7b70fe14876f97180034787f2a7f661597416a17" \
  "stacks/stacks-project a04446e57ec1fbc252a871afcec7752fb2807b14" \
  "suchow/Dissertate 2e92853c603cc9c8f56598dea26c5a0bce0f440b" \
  "tectonic-typesetting/tectonic d2224d9ba4185f952fd3d982eccd1f444dbdf895" \
  "tuna/thuthesis fd3be474b1e66be85bcf286ea4be524b26c7e512" \
  "vdumoulin/conv_arithmetic af6f818b0bb396c26da79899554682a8a499101d" \
; do
  set -- $entry
  dir="$CACHE/$(echo "$1" | tr / _)"
  if [ ! -d "$dir/.git" ]; then
    git init --quiet "$dir"
    git -C "$dir" remote add origin "https://github.com/$1"
  fi
  if [ "$(git -C "$dir" rev-parse HEAD 2>/dev/null || true)" != "$2" ]; then
    git -C "$dir" fetch --quiet --depth 1 origin "$2"
    git -C "$dir" -c advice.detachedHead=false checkout --quiet FETCH_HEAD
  fi
done

echo "LaTeX corpus ready in $CACHE"
