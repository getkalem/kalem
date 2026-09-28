#!/bin/sh
# Compute Emacs's exports of the export cases (tests/export/expected/).
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
emacs -Q --batch -l "$ROOT/tests/emacs/export.el" "$ROOT/tests/export/cases" "$ROOT/tests/export/expected"
echo "Emacs exports -> tests/export/expected"
