#!/usr/bin/env bash
# The README's pictures (docs/todo.md T2.10.13): the graphical editor
# opened on one corpus file per format, its window captured to
# assets/screenshot-FORMAT.png. The README lists the files in a comment
# under its mode table; take the comment markers away once they exist.
#
# macOS only. The window is captured from the screen, which needs the
# Screen Recording permission (System Settings > Privacy & Security >
# Screen Recording) for the program that runs this script: the terminal,
# or the app the terminal runs in. Without it macOS hands back the
# wallpaper and hides window titles; the script notices the hidden title
# and stops.
#
# Usage: tools/readme-screenshots.sh [KALEM]
#   KALEM is the binary to run, target/debug/kalem by default. Settings
#   and state go to a temporary folder, so your own are not touched.
set -euo pipefail
cd "$(dirname "$0")/.."

KALEM=${1:-target/debug/kalem}
[ -x "$KALEM" ] || { echo "no binary at $KALEM; cargo build -p kalem-editor first" >&2; exit 1; }
[ "$(uname)" = Darwin ] || { echo "this script captures with macOS's screencapture" >&2; exit 1; }

tmp=$(mktemp -d /tmp/kalem-shots.XXXXXX)
trap 'rm -rf "$tmp"' EXIT
export KALEM_CONFIG_DIR="$tmp/config" KALEM_STATE_DIR="$tmp/state"
mkdir -p assets

# FORMAT|FILE: one corpus file per format of the README's table.
shots=(
  "org|tests/corpus/org-mode/org-guide.org"
  "markdown|tests/corpus/markdown/vault/foam-docs/principles.md"
  "latex|tests/corpus/latex/arxiv/computer-science/2401.00632/main.tex"
  "csv|tests/csv/libreoffice.csv"
  "bibtex|examples/book/refs.bib"
)

# The window's screen rectangle as X,Y,W,H, once it is up (30 s at most).
# Prints "no-permission" when macOS hides the title, which it does
# without the Screen Recording permission.
window_bounds() {
  python3 - "$1" <<'PY'
import sys, time
from Quartz import CGWindowListCopyWindowInfo, kCGWindowListOptionOnScreenOnly, kCGNullWindowID
pid = int(sys.argv[1])
for _ in range(60):
    for w in CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly, kCGNullWindowID):
        if w.get("kCGWindowOwnerPID") == pid and w.get("kCGWindowLayer") == 0:
            if "kCGWindowName" not in w:
                print("no-permission"); sys.exit(0)
            b = w["kCGWindowBounds"]
            print(f"{int(b['X'])},{int(b['Y'])},{int(b['Width'])},{int(b['Height'])}"); sys.exit(0)
    time.sleep(0.5)
print("no-window")
PY
}

for shot in "${shots[@]}"; do
  format=${shot%%|*}; file=${shot#*|}
  out="assets/screenshot-$format.png"
  "$KALEM" "$file" >"$tmp/$format.log" 2>&1 &
  pid=$!
  bounds=$(window_bounds "$pid")
  case "$bounds" in
    no-permission)
      kill "$pid" 2>/dev/null || true
      echo "macOS hides the window's title: give the terminal the Screen Recording permission and run again" >&2
      exit 1 ;;
    no-window)
      kill "$pid" 2>/dev/null || true
      echo "$file: no window after 30 s; see $tmp/$format.log" >&2
      cat "$tmp/$format.log" >&2
      exit 1 ;;
  esac
  sleep 3   # the first draw, and the file's own parse
  screencapture -x -R "$bounds" "$out"
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  echo "$out  ($file, window at $bounds)"
done
