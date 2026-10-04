#!/bin/sh
# Zed's repository costs every first build of Kalem a 400 MB fetch (roadmap
# R2.7, T2.8.6): only gpui and gpui_platform come from it, both declared
# once in the workspace's Cargo.toml. A pull request that adds another
# crate from it, or declares one in a crate's manifest, fails here.
found=$(grep -rn --include=Cargo.toml 'zed-industries/' . --exclude-dir=target --exclude-dir=.git \
  | grep -v '^\./Cargo\.toml:[0-9]*:gpui = ' \
  | grep -v '^\./Cargo\.toml:[0-9]*:gpui_platform = ')
if [ -n "$found" ]; then
  echo "A new dependency on Zed's repository (only gpui and gpui_platform, in the workspace Cargo.toml, may come from it):" >&2
  printf '%s\n' "$found" >&2
  exit 1
fi
echo "Zed's repository: gpui and gpui_platform only"
