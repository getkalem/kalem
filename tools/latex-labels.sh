#!/bin/sh
# The numbers pdflatex gives the labels of tests/latex/model/NAME.tex,
# written to NAME.labels ("key number" per line) for
# crates/latex-model/tests/model.rs.
set -eu
dir="$(cd "$(dirname "$0")/../tests/latex/model" && pwd)"
tmp="$(mktemp -d)"
for name in "$@"; do
  cp "$dir/$name.tex" "$tmp/"
  (cd "$tmp" && pdflatex -interaction=nonstopmode "$name.tex" >/dev/null 2>&1 || true
   pdflatex -interaction=nonstopmode "$name.tex" >/dev/null 2>&1 || true)
  # hyperref's and subcaption's own labels (`sub@x`) left out.
  sed -n 's/^\\newlabel{\([^}@]*\)}{{\([^}]*\)}.*/\1 \2/p' "$tmp/$name.aux" > "$dir/$name.labels"
  echo "$dir/$name.labels"
done
rm -rf "$tmp"
