#!/bin/sh
# Compute the Emacs results for the editing cases (tests/edit/expected.json).
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# The Org the results come from: `ORG_LISP=.cache/org-lisp` after
# tools/fetch-org.sh, when the system Emacs has an older Org.
EMACS="emacs${ORG_LISP:+ -L $ORG_LISP}"
python3 "$ROOT/tools/gen-edit-cases.py"
$EMACS -Q --batch -l "$ROOT/tests/emacs/edit.el" "$ROOT/tests/edit/cases.json" "$ROOT/tests/edit/expected.json"
echo "Emacs results -> tests/edit/expected.json"
