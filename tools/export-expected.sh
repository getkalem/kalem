#!/bin/sh
# Compute Emacs's exports of the export cases (tests/export/expected/).
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# The Org the results come from: `ORG_LISP=.cache/org-lisp` after
# tools/fetch-org.sh, when the system Emacs has an older Org.
EMACS="emacs${ORG_LISP:+ -L $ORG_LISP}"
$EMACS -Q --batch -l "$ROOT/tests/emacs/export.el" "$ROOT/tests/export/cases" "$ROOT/tests/export/expected"
echo "Emacs exports -> tests/export/expected"
KALEM_EXPORT_FULL=1 KALEM_EXPORT_BACKENDS=html $EMACS -Q --batch -l "$ROOT/tests/emacs/export.el" "$ROOT/tests/export/cases" "$ROOT/tests/export/full"
echo "Emacs's whole HTML pages -> tests/export/full"
