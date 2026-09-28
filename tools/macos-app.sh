#!/bin/sh
# Builds Kalem.app, unsigned (§17: signing and notarization come in phase
# 2), from a release build: target/release/Kalem.app. Usage:
#
#   tools/macos-app.sh [--target TRIPLE]
#
# With a target (aarch64-apple-darwin, x86_64-apple-darwin), the app goes
# to target/TRIPLE/release/Kalem.app.
set -eu
cd "$(dirname "$0")/.."

target=""
if [ "${1:-}" = "--target" ]; then
  target="$2"
fi

if [ -n "$target" ]; then
  cargo build --release -p kalem-editor --target "$target"
  out="target/$target/release"
else
  cargo build --release -p kalem-editor
  out="target/release"
fi

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app="$out/Kalem.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$out/kalem" "$app/Contents/MacOS/kalem"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"
plutil -lint "$app/Contents/Info.plist" >/dev/null
echo "$app"
