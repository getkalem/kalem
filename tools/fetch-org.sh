#!/bin/sh
# Fetch Org 9.7 (as Emacs 30 ships it) into .cache/org-lisp and compile it,
# for machines whose Emacs has an older Org: `ORG_LISP=.cache/org-lisp
# tools/export-expected.sh` (and edit-expected.sh) then use it, and
# `kalem diff-emacs --emacs` takes a script running `emacs -L .cache/org-lisp`.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CACHE="$ROOT/.cache"
SRC="$CACHE/emacs-30"
OUT="$CACHE/org-lisp"
mkdir -p "$CACHE"

if [ ! -d "$SRC/.git" ]; then
  git clone --quiet --depth 1 --branch emacs-30 --filter=blob:none --sparse \
    https://github.com/emacs-mirror/emacs.git "$SRC"
  git -C "$SRC" sparse-checkout set lisp/org
fi
rm -rf "$OUT"
cp -r "$SRC/lisp/org" "$OUT"
cd "$OUT"
# org-loaddefs.el is generated when Emacs is built.
emacs -Q --batch --eval '(loaddefs-generate "." (expand-file-name "org-loaddefs.el"))'
emacs -Q --batch -L . -f batch-byte-compile ./*.el >/dev/null 2>&1
emacs -Q --batch -L . --eval '(progn (require (quote org)) (princ (format "Org %s in %s\n" (org-version) default-directory)))'
