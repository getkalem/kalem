#!/bin/sh
# Show how Kalem's export of a case differs from Emacs's:
#   tools/export-diff.sh NAME EXT   (after KALEM_EXPORT_DIFF=1 cargo test -p org-export)
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="${TMPDIR:-/tmp}"
norm() { python3 -c '
import re,sys
m={}
s=open(sys.argv[1]).read()
print(re.sub(r"org[0-9a-f]{7}(?![0-9a-zA-Z])", lambda x: "REF%d" % m.setdefault(x.group(0), len(m)), s), end="")' "$1"; }
diff <(norm "$TMP/kalem-export-$1.$2") <(norm "$ROOT/tests/export/expected/$1.$2") || true
