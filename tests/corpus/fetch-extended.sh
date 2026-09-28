#!/bin/sh
# Fetch the extended test corpus (the full Org and Worg repositories) at pinned
# commits into .cache/. The extended corpus is used by the differential tests
# and the round-trip tests when KALEM_EXTENDED_CORPUS=1 is set.
set -eu

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CACHE="$ROOT/.cache"
mkdir -p "$CACHE"

ORG_REPO="https://git.savannah.gnu.org/git/emacs/org-mode.git"
ORG_REF="release_9.7.11"
WORG_REPO="https://git.sr.ht/~bzg/worg"
WORG_COMMIT="22fc063138eb48facd235093bb7e20fe3c53a0bd"

if [ ! -d "$CACHE/org-mode/.git" ]; then
  git clone --quiet --depth 1 --branch "$ORG_REF" "$ORG_REPO" "$CACHE/org-mode"
fi

if [ ! -d "$CACHE/worg/.git" ]; then
  git clone --quiet "$WORG_REPO" "$CACHE/worg"
fi
git -C "$CACHE/worg" -c advice.detachedHead=false checkout --quiet "$WORG_COMMIT"

echo "Extended corpus ready in $CACHE"
