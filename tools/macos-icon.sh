#!/bin/sh
# packaging/macos/Kalem.icns from assets/kalem.svg, with the tools macOS
# has (Quick Look draws the SVG, sips scales it, iconutil packs it). Run
# it again when the logo changes, and commit the result.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
qlmanage -t -s 1024 -o "$tmp" "$root/assets/kalem.svg" > /dev/null
png="$tmp/kalem.svg.png"
set_dir="$tmp/Kalem.iconset"
mkdir "$set_dir"
for size in 16 32 128 256 512; do
  sips -z $size $size "$png" --out "$set_dir/icon_${size}x${size}.png" > /dev/null
  double=$((size * 2))
  sips -z $double $double "$png" --out "$set_dir/icon_${size}x${size}@2x.png" > /dev/null
done
iconutil -c icns "$set_dir" -o "$root/packaging/macos/Kalem.icns"
echo "packaging/macos/Kalem.icns"
