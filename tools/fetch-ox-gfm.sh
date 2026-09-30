#!/bin/sh
# Fetch the ox-gfm package (GPL-3.0, not part of Org, so not in this
# repository) into .cache/ox-gfm, pinned: the reference for Kalem's gfm
# back-end. tools/export-expected.sh uses it when it is there.
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REV=4f774f13d34b3db9ea4ddb0b1edc070b1526ccbb
mkdir -p "$ROOT/.cache/ox-gfm"
curl -sSfL -o "$ROOT/.cache/ox-gfm/ox-gfm.el" \
  "https://raw.githubusercontent.com/larstvei/ox-gfm/$REV/ox-gfm.el"
echo "ox-gfm $REV -> .cache/ox-gfm"
