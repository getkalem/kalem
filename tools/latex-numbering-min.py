#!/usr/bin/env python3
"""Shrinks a document where pdflatex's and Kalem's numbers differ.

    tools/latex-numbering-min.py DOC.tex...

Lines of the preamble and the body are taken out, one at a time, as long
as the document still compiles and some label still differs; the
smallest document is written to DOC.min.tex with its differences.
"""

import importlib.util
import os
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
spec = importlib.util.spec_from_file_location(
    "fuzz", os.path.join(ROOT, "tools", "latex-numbering-fuzz.py"))
fuzz = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fuzz)
EXE = os.path.join(ROOT, "target", "release", "examples", "labels")


def diffs(text):
    with tempfile.TemporaryDirectory() as d:
        return diffs_in(text, d)


def diffs_in(text, d):
    latex = fuzz.pdflatex(text, d)
    if latex is None:
        return None
    path = os.path.join(d, "t.tex")
    out = subprocess.run([EXE, path], stdout=subprocess.PIPE, text=True).stdout
    got = {"!clash": "0"}
    for line in out.splitlines():
        _, key, *num = line.split(" ")
        if key == "!clash":
            got[key] = str(int(got[key]) + 1)
        else:
            got[key] = " ".join(num)
    return [f"{k}: LaTeX {v!r}, Kalem {got.get(k)!r}" for k, v in latex.items()
            if got.get(k) != v] + [f"{k}: LaTeX writes nothing, Kalem {n!r}"
                                   for k, n in got.items() if k not in latex and n]


def main():
    for doc in sys.argv[1:]:
        lines = open(doc).read().split("\n")
        changed = True
        while changed:
            changed = False
            i = 1
            while i < len(lines):
                if lines[i].strip() in ("\\begin{document}", "\\end{document}", ""):
                    i += 1
                    continue
                trial = lines[:i] + lines[i + 1:]
                d = diffs("\n".join(trial))
                if d:
                    lines = trial
                    changed = True
                else:
                    i += 1
        text = "\n".join(l for l in lines if l.strip())
        out = doc[:-4] + ".min.tex"
        with open(out, "w") as f:
            f.write(text + "\n")
        print(f"== {out}\n{text}\n-- {diffs(text)}\n")


if __name__ == "__main__":
    main()
