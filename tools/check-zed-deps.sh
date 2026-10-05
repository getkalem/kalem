#!/bin/sh
# Nothing comes from Zed's repository (roadmap R2.7): gpui and its platform
# layer come from crates.io (`gpui-unofficial`, a snapshot of Zed's release
# tags), so a first build clones no 400 MB of Zed's history. A change that
# names Zed's repository in a manifest, or brings a crate from it into the
# lock file through a dependency, fails here.
found=$(grep -rn --include=Cargo.toml 'zed-industries/' . --exclude-dir=target --exclude-dir=.git --exclude-dir=spikes)
locked=$(grep -n 'source = "git+https://github.com/zed-industries/' Cargo.lock)
if [ -n "$found$locked" ]; then
  echo "A dependency on Zed's repository (gpui comes from crates.io as gpui-unofficial):" >&2
  [ -n "$found" ] && printf '%s\n' "$found" >&2
  [ -n "$locked" ] && printf 'Cargo.lock:%s\n' "$locked" >&2
  exit 1
fi
echo "Zed's repository: nothing from it"
