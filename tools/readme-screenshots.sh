#!/usr/bin/env bash
# The README's pictures (docs/todo.md T2.10.13 and T1.8.7): the graphical
# editor opened on one file per format and on a folder (the file
# manager), its window captured to assets/screenshot-NAME.png; the
# terminal editor on the Org file, on the projects view and on the
# settings panel, run in a Terminal window and captured; and
# assets/kalem.gif, the captures one after another, the Org file in the
# window first and in the terminal second. With the Accessibility
# permission as well, the projects view and the settings panel are
# captured in the graphical editor instead, the keys sent by System
# Events. The README keeps the image lines in comments under its tables;
# take the comment markers away once the files exist.
#
# macOS only. A window is captured from the screen, which needs the Screen
# Recording permission (System Settings > Privacy & Security > Screen
# Recording) for the program that runs this script: the terminal, or the
# app the terminal runs in. Without it macOS hands back the wallpaper and
# hides window titles; the script notices the hidden title and stops. The
# terminal pictures drive Terminal.app through AppleScript, which asks for
# the Automation permission the first time, and run the editor under
# expect to press its keys.
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
mkdir -p assets "$KALEM_CONFIG_DIR"
printf '[editor]\ntheme = "dark"\n' >"$KALEM_CONFIG_DIR/settings.toml"
# Three projects for the projects view, all inside the repository.
cat >"$KALEM_CONFIG_DIR/projects.toml" <<EOF
[[project]]
name = "kalem"
path = "$(pwd)"

[[project]]
name = "book"
path = "$(pwd)/examples/book"

[[project]]
name = "notes"
path = "$(pwd)/tests/corpus/markdown/vault/foam-docs"
EOF

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
\date{October 2026}
\begin{document}
\maketitle

\begin{abstract}
A document is a text file. An editor that keeps it one can show it as it reads and still write back only what changed. We state the rule, give the one equation it rests on, and list what it costs.
\end{abstract}

\section{Introduction}
\label{sec:intro}
Every format Kalem opens is read into \emph{ranges} of the file's text, never into a tree that is written back. An edit replaces only the characters it changes, so the rest of the file stays byte for byte as it was, including spacing, comments and line endings~\cite{knuth}. Section~\ref{sec:method} gives the rule; equation~\eqref{eq:gauss} is the example every reader knows.

\section{Method}
\label{sec:method}
Let $f$ be the parse of a file $x$ and $g$ the text it gives back. The rule is $g(f(x)) = x$ for every $x$, whatever the input. An incremental reparse after an edit must equal a full parse of the new text:
\begin{equation}\label{eq:gauss}
  \int_0^\infty e^{-x^2}\,dx = \frac{\sqrt{\pi}}{2}.
\end{equation}
The numbers of sections, equations and floats are the ones \LaTeX{} prints, checked against the \texttt{.aux} files it writes:
\begin{align}
  \sum_{k=1}^{n} k &= \frac{n(n+1)}{2}, \\
  \sum_{k=1}^{n} k^2 &= \frac{n(n+1)(2n+1)}{6}.
\end{align}

\subsection{What it costs}
\begin{itemize}
  \item A keystroke reparses the paragraph it is in, not the file.
  \item What the parser does not understand stays visible as source.
  \item No command is added to the language: the file is standard \LaTeX{}.
\end{itemize}

\begin{table}[htbp]
  \centering
  \begin{tabular}{lrr}
    \hline
    Corpus & Files & Edits \\
    \hline
    Real projects & 1,669 & 50,010 \\
    arXiv papers & 925 & 96,900 \\
    \hline
  \end{tabular}
  \caption{The round trip, byte for byte, after random edits.}\label{tab:corpus}
\end{table}

\section{Conclusion}
Table~\ref{tab:corpus} is the whole argument: the file is yours before and after.

\begin{thebibliography}{1}
\bibitem{knuth} D.~E. Knuth, \emph{The \TeX book}, Addison-Wesley, 1984.
\end{thebibliography}
\end{document}
TEX
  (cd "$tmp" && pdflatex -interaction=nonstopmode -halt-on-error sample.tex >sample.log 2>&1 && pdflatex -interaction=nonstopmode -halt-on-error sample.tex >>sample.log 2>&1) \
    || { echo "pdflatex failed ($tmp/sample.log): the PDF picture is skipped" >&2; pdf=""; }
else
  echo "no pdflatex: the PDF picture is skipped" >&2; pdf=""
fi

# FORMAT|FILE: one file per picture of the README.
shots=(
  "markdown|tests/corpus/markdown/vault/foam-docs/principles.md"
  "org|tests/corpus/org-mode/org-guide.org"
  "latex|$tmp/sample.tex"
  "csv|tests/csv/libreoffice.csv"
  "pdf|$pdf"
  "xlsx|$workbook"
  "files|."
)

# The first window of the process (by PID) or of the application (by
# owner name), once it is up (30 s at most), as X,Y,W,H,NUMBER: its
# screen rectangle and its window number for screencapture -l. Prints "no-permission" when macOS hides the title, which it does
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
            print(f"{int(b['X'])},{int(b['Y'])},{int(b['Width'])},{int(b['Height'])},{w['kCGWindowNumber']}"); sys.exit(0)
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
  screencapture -x -o -l "${bounds##*,}" "$out"
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  echo "$out  ($file, window at $bounds)"
done

# A Terminal window of 140 by 42 running COMMAND, captured to OUT once the
# editor has drawn; then the editor is ended and the window closed.
terminal_shot() {
  local command=$1 out=$2
  osascript - "$command" <<'AS' >/dev/null
on run argv
  tell application "Terminal"
    activate
    set t to do script (item 1 of argv)
    set number of columns of t to 140
    set number of rows of t to 42
    set position of front window to {80, 80}
  end tell
end run
AS
  local bounds
  bounds=$(window_bounds name Terminal)
  case "$bounds" in
    no-permission|no-window) echo "Terminal: $bounds" >&2; exit 1 ;;
  esac
  sleep 7   # the terminal's resize, the editor's start, the keys and the draw
  screencapture -x -o -l "${bounds##*,}" "$out"
  # The window's shell and everything under it (expect, the editor), so
  # that Terminal closes the window without asking.
  local tty
  tty=$(osascript -e 'tell application "Terminal" to tty of selected tab of front window' 2>/dev/null)
  [ -n "$tty" ] && ps -o pid= -t "${tty#/dev/}" | xargs kill -9 2>/dev/null || true
  sleep 1
  osascript -e 'tell application "Terminal" to close front window' >/dev/null 2>&1 || true
  echo "$out  ($command, Terminal window at $bounds)"
}

# The editor's keys are pressed by expect: FILE, then the KEYS sent
# after three seconds of quiet. The terminal is connected to the editor
# from the start (interact), so that its answers to the editor's
# questions (colors, capabilities) reach it as answers and not, late, as
# typed keys.
expect_script() {
  local file=$1 keys=$2 name=$3
  cat >"$tmp/$name.exp" <<EOF
set sent 0
spawn env KALEM_CONFIG_DIR=$KALEM_CONFIG_DIR KALEM_STATE_DIR=$KALEM_STATE_DIR $KALEM tui $file
interact {
  timeout 3 { if {!\$sent} { set sent 1; send "$keys" } }
}
EOF
  echo "expect $tmp/$name.exp"
}

org_file=$(pwd)/tests/corpus/org-mode/org-guide.org
terminal_shot "$(expect_script "$org_file" "" org)" assets/screenshot-terminal.png

# The projects view and the settings panel: in the graphical editor when
# System Events may press its keys (the Accessibility permission), else
# in the terminal editor.
if python3 -c 'import sys; from ApplicationServices import AXIsProcessTrusted; sys.exit(0 if AXIsProcessTrusted() else 1)' 2>/dev/null; then
  gui_key_shot() {   # FILE KEYSTROKE-APPLESCRIPT OUT
    local file=$1 keystroke=$2 out=$3
    "$KALEM" "$file" >"$tmp/keys.log" 2>&1 &
    local pid=$!
    local bounds
    bounds=$(window_bounds pid "$pid")
    case "$bounds" in
      no-permission|no-window) kill "$pid" 2>/dev/null || true; echo "$file: $bounds" >&2; exit 1 ;;
    esac
    sleep 3
    osascript -e "tell application \"System Events\"
      set frontmost of (first process whose unix id is $pid) to true
      delay 0.5
      $keystroke
    end tell"
    sleep 2
    screencapture -x -o -l "${bounds##*,}" "$out"
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    echo "$out  ($file, $keystroke)"
  }
  gui_key_shot . 'keystroke "d" using {control down, option down, shift down}' assets/screenshot-projects.png
  gui_key_shot "$org_file" 'keystroke "," using {command down}' assets/screenshot-settings.png
else
  terminal_shot "$(expect_script "$(pwd)" "P" projects)" assets/screenshot-projects.png
  terminal_shot "$(expect_script "$org_file" '\033,' settings)" assets/screenshot-settings.png
fi

# The GIF: the Org file in the window, then in the terminal, then the
# other formats, three seconds each, 1000 pixels wide.
frames=(assets/screenshot-org.png assets/screenshot-terminal.png)
for f in markdown latex csv xlsx pdf files projects settings; do
  [ -f "assets/screenshot-$f.png" ] && frames+=("assets/screenshot-$f.png")
done
if command -v magick >/dev/null; then
  magick -delay 300 "${frames[@]}" -resize 1000x -colors 128 -layers Optimize -loop 0 assets/kalem.gif
  echo "assets/kalem.gif  (${#frames[@]} frames, $(du -h assets/kalem.gif | cut -f1))"
else
  echo "no magick (ImageMagick): the GIF is skipped" >&2
fi
