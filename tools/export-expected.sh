#!/bin/sh
# Compute Emacs's exports of the export cases (tests/export/expected/).
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# The Org the results come from: `ORG_LISP=.cache/org-lisp` after
# tools/fetch-org.sh, when the system Emacs has an older Org.
EMACS="emacs${ORG_LISP:+ -L $ORG_LISP}"
# ox-gfm, for the gfm back-end, when tools/fetch-ox-gfm.sh has fetched it.
if [ -f "$ROOT/.cache/ox-gfm/ox-gfm.el" ]; then
  export KALEM_OX_GFM="$ROOT/.cache/ox-gfm/ox-gfm.el"
fi
$EMACS -Q --batch -l "$ROOT/tests/emacs/export.el" "$ROOT/tests/export/cases" "$ROOT/tests/export/expected"
echo "Emacs exports -> tests/export/expected"
KALEM_EXPORT_FULL=1 KALEM_EXPORT_BACKENDS=html KALEM_OX_GFM= $EMACS -Q --batch -l "$ROOT/tests/emacs/export.el" "$ROOT/tests/export/cases" "$ROOT/tests/export/full"
echo "Emacs's whole HTML pages -> tests/export/full"
