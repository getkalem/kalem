#!/bin/sh
# Compute the Emacs results for the editing cases (tests/edit/expected.json).
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
python3 "$ROOT/tools/gen-edit-cases.py"
emacs -Q --batch -l "$ROOT/tests/emacs/edit.el" "$ROOT/tests/edit/cases.json" "$ROOT/tests/edit/expected.json"
echo "Emacs results -> tests/edit/expected.json"
