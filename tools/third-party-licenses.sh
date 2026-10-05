#!/bin/sh
# THIRD-PARTY-LICENSES.md: the licences of everything the release
# binaries are built from or embed (publish_todo 6): Kalem's crates and
# those of the plugins built in, at the tags crates/kalem-components/
# components.toml pins, through cargo-about
# (`cargo install --locked --features cli cargo-about`); the syntax
# definitions and themes of the highlighter; and the fonts, colour
# profiles, character maps, styles and tables that are not crates
# (packaging/licenses/assemble.py). Run before a release and commit the
# result; the release archives carry it.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
config=packaging/licenses/about.toml
cargo about generate --format json -c "$config" -m crates/kalem/Cargo.toml > "$tmp/crates-kalem.json"
python3 - "$tmp" <<'PY' | while read -r id tag dir repo; do
import sys, tomllib
c = tomllib.load(open("crates/kalem-components/components.toml", "rb"))
for comp in c["component"]:
    folder = comp["manifest"].rsplit("/", 1)[0]
    print(comp["id"], comp["tag"], folder, c["repository"])
PY
  if [ ! -d "$tmp/$tag" ]; then
    git clone -q --depth 1 --branch "$tag" "https://github.com/$repo" "$tmp/$tag"
  fi
  cargo about generate --format json -c "$root/$config" -m "$tmp/$tag/$dir/Cargo.toml" > "$tmp/crates-$id.json"
done
cargo run -q -p kalem-highlight --example acknowledgements > "$tmp/syntaxes.md"
cargo metadata --format-version 1 --locked > "$tmp/metadata.json"
python3 packaging/licenses/assemble.py "$tmp" > THIRD-PARTY-LICENSES.md
echo THIRD-PARTY-LICENSES.md
