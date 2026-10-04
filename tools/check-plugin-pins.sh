#!/bin/sh
# The bundled plugins come from one repository (getkalem/plugins) and are
# built against this checkout's contract (the [patch] in Cargo.toml): they
# move together, at one revision, so a contract change and the plugins
# that follow it land in one bump (roadmap R1.3).
revs=$(grep 'git = "https://github.com/getkalem/plugins"' Cargo.toml | sed 's/.*rev = "\([0-9a-f]*\)".*/\1/' | sort -u)
n=$(printf '%s\n' "$revs" | grep -c .)
if [ "$n" -ne 1 ]; then
  echo "The bundled plugins are pinned at $n revisions of getkalem/plugins; pin them at one:" >&2
  printf '  %s\n' $revs >&2
  exit 1
fi
echo "Bundled plugins at getkalem/plugins $revs"
