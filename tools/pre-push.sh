#!/bin/sh
# What to run before pushing to main (roadmap R1.4): formatting, the
# plugin pins, and clippy and the tests of every crate the change
# touches. `tools/pre-push.sh` compares with origin/main; pass another
# base as the first argument. Install as a hook with
#   ln -s ../../tools/pre-push.sh .git/hooks/pre-push
set -e
base=${1:-origin/main}
case "$base" in origin/*) git fetch -q origin "${base#origin/}" || true ;; esac

cargo fmt --all --check
sh tools/check-plugin-pins.sh

# The crates whose files changed since the base, committed or not.
changed=$( { git diff --name-only "$base"...HEAD; git diff --name-only; git diff --name-only --cached; } \
  | sed -n 's#^crates/\([^/]*\)/.*#\1#p' | sort -u)
# A change to the workspace manifest or lock file touches every crate.
if { git diff --name-only "$base"...HEAD; git diff --name-only; } | grep -qx 'Cargo.toml\|Cargo.lock'; then
  changed=all
fi
if [ -z "$changed" ]; then
  echo "No crate changed."
  exit 0
fi

export CARGO_INCREMENTAL=0
if [ "$changed" = all ]; then
  cargo clippy -q --workspace --all-targets -- -D warnings
  cargo test -q --workspace
else
  pkgs=""
  for c in $changed; do
    # The package name is the crate's `name`, not always its folder's.
    name=$(sed -n 's/^name = "\(.*\)"/\1/p' "crates/$c/Cargo.toml" | head -1)
    [ -n "$name" ] && pkgs="$pkgs -p $name"
  done
  echo "Checking:$pkgs"
  # shellcheck disable=SC2086
  cargo clippy -q $pkgs --all-targets -- -D warnings
  # shellcheck disable=SC2086
  cargo test -q $pkgs
fi
echo "Ready to push."
