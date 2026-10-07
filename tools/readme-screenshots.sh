#!/usr/bin/env bash
# The README's pictures (docs/todo.md T2.10.13 and T1.8.7): the graphical
# editor opened on one file per format, its window captured to
# assets/screenshot-FORMAT.png; the terminal editor on the Org file, run in
# a Terminal window and captured to assets/screenshot-terminal.png; and
# assets/kalem.gif, the captures one after another, the Org file in the
# window first and in the terminal second. The README keeps the image
# lines in comments under its tables; take the comment markers away once
# the files exist.
#
# macOS only. A window is captured from the screen, which needs the Screen
# Recording permission (System Settings > Privacy & Security > Screen
# Recording) for the program that runs this script: the terminal, or the
# app the terminal runs in. Without it macOS hands back the wallpaper and
# hides window titles; the script notices the hidden title and stops. The
# terminal picture drives Terminal.app through AppleScript, which asks for
# the Automation permission the first time.
#
# Two sample files are not in the repository and are made on the way: a
# workbook by Python's openpyxl and a one-page PDF by pdflatex. Where
# either is missing, its picture is skipped with a note.
#
# Usage: tools/readme-screenshots.sh [KALEM]
#   KALEM is the binary to run, target/debug/kalem by default. Settings
#   and state go to a temporary folder, so your own are not touched.
set -euo pipefail
cd "$(dirname "$0")/.."

KALEM=${1:-target/debug/kalem}
[ -x "$KALEM" ] || { echo "no binary at $KALEM; cargo build -p kalem-editor first" >&2; exit 1; }
[ "$(uname)" = Darwin ] || { echo "this script captures with macOS's screencapture" >&2; exit 1; }
KALEM=$(cd "$(dirname "$KALEM")" && pwd)/$(basename "$KALEM")

tmp=$(mktemp -d /tmp/kalem-shots.XXXXXX)
trap 'rm -rf "$tmp"' EXIT
export KALEM_CONFIG_DIR="$tmp/config" KALEM_STATE_DIR="$tmp/state"
mkdir -p assets

# The workbook: a budget with formulas and a chart.
workbook="$tmp/budget.xlsx"
python3 - "$workbook" <<'PY' || { echo "no openpyxl: the workbook picture is skipped" >&2; workbook=""; }
import sys
from openpyxl import Workbook
from openpyxl.chart import BarChart, Reference
wb = Workbook(); ws = wb.active; ws.title = "Budget"
ws.append(["Month", "Income", "Expenses", "Savings"])
rows = [("January", 4200, 3100), ("February", 4200, 2950), ("March", 4350, 3300),
        ("April", 4200, 2800), ("May", 4500, 3600), ("June", 4200, 3050)]
for i, (m, inc, exp) in enumerate(rows, start=2):
    ws.append([m, inc, exp, f"=B{i}-C{i}"])
n = len(rows) + 1
ws.append(["Total", f"=SUM(B2:B{n})", f"=SUM(C2:C{n})", f"=SUM(D2:D{n})"])
chart = BarChart(); chart.title = "Income and expenses"
chart.add_data(Reference(ws, min_col=2, max_col=3, min_row=1, max_row=n), titles_from_data=True)
chart.set_categories(Reference(ws, min_col=1, min_row=2, max_row=n))
ws.add_chart(chart, "F2")
wb.save(sys.argv[1])
PY

# The PDF: one page of standard LaTeX, so that only the base classes are needed.
pdf="$tmp/sample.pdf"
if command -v pdflatex >/dev/null; then
  cat > "$tmp/sample.tex" <<'TEX'
\documentclass{article}
\usepackage{amsmath}
\title{A note on plain text}
\author{Kalem}
\date{}
\begin{document}
\maketitle
\section{Introduction}
A document is a text file. An editor that keeps it one can show it as it reads and still write back only what changed:
\begin{equation}
  \int_0^\infty e^{-x^2}\,dx = \frac{\sqrt{\pi}}{2}.
\end{equation}
\section{Method}
The parse reads the file into ranges; an edit replaces only the characters it changes, so the rest of the file stays byte for byte as it was.
\subsection{Numbering}
Sections, equations and floats carry the numbers \LaTeX{} prints, checked against the \texttt{.aux} files it writes.
\end{document}
TEX
  (cd "$tmp" && pdflatex -interaction=nonstopmode -halt-on-error sample.tex >sample.log 2>&1) \
    || { echo "pdflatex failed ($tmp/sample.log): the PDF picture is skipped" >&2; pdf=""; }
else
  echo "no pdflatex: the PDF picture is skipped" >&2; pdf=""
fi

# FORMAT|FILE: one file per picture of the README.
shots=(
  "markdown|tests/corpus/markdown/vault/foam-docs/principles.md"
  "org|tests/corpus/org-mode/org-guide.org"
  "latex|tests/corpus/latex/arxiv/computer-science/2401.00632/main.tex"
  "csv|tests/csv/libreoffice.csv"
  "pdf|$pdf"
  "xlsx|$workbook"
)

# The screen rectangle, as X,Y,W,H, of the first window of the process
# (by PID) or of the application (by owner name), once it is up (30 s at
# most). Prints "no-permission" when macOS hides the title, which it does
# without the Screen Recording permission.
window_bounds() {
  python3 - "$1" "$2" <<'PY'
import sys, time
from Quartz import CGWindowListCopyWindowInfo, kCGWindowListOptionOnScreenOnly, kCGNullWindowID
kind, key = sys.argv[1], sys.argv[2]
for _ in range(60):
    for w in CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly, kCGNullWindowID):
        if w.get("kCGWindowLayer") != 0:
            continue
        mine = w.get("kCGWindowOwnerPID") == int(key) if kind == "pid" else w.get("kCGWindowOwnerName") == key
        if mine:
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
  [ -n "$file" ] || continue
  out="assets/screenshot-$format.png"
  "$KALEM" "$file" >"$tmp/$format.log" 2>&1 &
  pid=$!
  bounds=$(window_bounds pid "$pid")
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

# The terminal editor on the Org file, in a Terminal window of 140 by 42.
org_file=$(pwd)/tests/corpus/org-mode/org-guide.org
out=assets/screenshot-terminal.png
command="KALEM_CONFIG_DIR='$KALEM_CONFIG_DIR' KALEM_STATE_DIR='$KALEM_STATE_DIR' '$KALEM' tui '$org_file'"
osascript - "$command" <<'AS' >/dev/null
on run argv
  tell application "Terminal"
    activate
    set t to do script (item 1 of argv)
    set number of columns of t to 140
    set number of rows of t to 42
  end tell
end run
AS
bounds=$(window_bounds name Terminal)
case "$bounds" in
  no-permission|no-window) echo "Terminal: $bounds" >&2; exit 1 ;;
esac
sleep 4   # the terminal's resize, the editor's start and its first draw
screencapture -x -R "$bounds" "$out"
pkill -f "kalem.* tui .*org-guide.org" 2>/dev/null || true
sleep 1
osascript -e 'tell application "Terminal" to close front window' >/dev/null 2>&1 || true
echo "$out  (kalem tui, Terminal window at $bounds)"

# The GIF: the Org file in the window, then in the terminal, then the
# other formats, three seconds each, 1000 pixels wide.
frames=(assets/screenshot-org.png assets/screenshot-terminal.png)
for f in markdown latex csv xlsx pdf; do
  [ -f "assets/screenshot-$f.png" ] && frames+=("assets/screenshot-$f.png")
done
if command -v magick >/dev/null; then
  magick -delay 300 "${frames[@]}" -resize 1000x -colors 128 -layers Optimize -loop 0 assets/kalem.gif
  echo "assets/kalem.gif  (${#frames[@]} frames, $(du -h assets/kalem.gif | cut -f1))"
else
  echo "no magick (ImageMagick): the GIF is skipped" >&2
fi
