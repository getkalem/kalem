#!/bin/sh
# packaging/macos/Kalem.icns and assets/kalem-256.png (the README's logo)
# from assets/kalem.svg, with the tools macOS has: AppKit draws the SVG,
# through JavaScript for Automation, at each size on a transparent
# square, and iconutil packs the icon set. Run it again when the logo
# changes, and commit both results. (Quick Look, which drew it before,
# gives its thumbnails an opaque white ground: the icon had white corners.)
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cat > "$tmp/render.js" <<'EOF'
// render.js SVG OUT PIXELS: the SVG drawn by AppKit on a transparent
// square of PIXELS by PIXELS, in sRGB, written as a PNG. Drawn into a Core
// Graphics context of its own: through an NSBitmapImageRep the PNG's
// edge pixels came out with their alpha divided out twice (too light).
ObjC.import('AppKit');
ObjC.import('CoreGraphics');
function run(argv) {
  const [src, out, px] = [argv[0], argv[1], Number(argv[2])];
  const img = $.NSImage.alloc.initWithContentsOfFile(src);
  if (!img.isValid) throw new Error(`AppKit cannot read ${src}`);
  const cs = $.CGColorSpaceCreateWithName($.kCGColorSpaceSRGB);
  const ctx = $.CGBitmapContextCreate(null, px, px, 8, 0, cs, $.kCGImageAlphaPremultipliedLast);
  $.NSGraphicsContext.saveGraphicsState;
  $.NSGraphicsContext.setCurrentContext($.NSGraphicsContext.graphicsContextWithCGContextFlipped(ctx, false));
  img.drawInRectFromRectOperationFraction($.NSMakeRect(0, 0, px, px), $.NSZeroRect, $.NSCompositingOperationSourceOver, 1);
  $.NSGraphicsContext.restoreGraphicsState;
  const rep = $.NSBitmapImageRep.alloc.initWithCGImage($.CGBitmapContextCreateImage(ctx));
  const png = rep.representationUsingTypeProperties($.NSBitmapImageFileTypePNG, $({}));
  if (!png.writeToFileAtomically(out, true)) throw new Error(`cannot write ${out}`);
}
EOF
render() { osascript -l JavaScript "$tmp/render.js" "$root/assets/kalem.svg" "$1" "$2"; }
set_dir="$tmp/Kalem.iconset"
mkdir "$set_dir"
for size in 16 32 128 256 512; do
  render "$set_dir/icon_${size}x${size}.png" $size
  render "$set_dir/icon_${size}x${size}@2x.png" $((size * 2))
done
iconutil -c icns "$set_dir" -o "$root/packaging/macos/Kalem.icns"
echo "packaging/macos/Kalem.icns"
render "$root/assets/kalem-256.png" 256
echo "assets/kalem-256.png"
