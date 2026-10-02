#!/usr/bin/env python3
"""Random sequences of Kalem's LaTeX editing commands, compiled.

    tools/latex-edit-fuzz.py [VARIANTS] [SEED] [COUNT] [--keep DIR]

`tests/latex/edits/template.tex` (sections, nested lists, equations,
floats, citations) is edited by COUNT commands at random cursors and
selections (bold, emphasis, Enter, nesting items, promoting and moving
sections, display math, inserting figures, tables, equations and
citations, quick fixes; `examples/latex_edits`), VARIANTS times. Every
variant must still compile with pdflatex, without a warning the template
does not have (T2.7h.33). A variant that fails is kept in the --keep
folder (default a scratch folder, printed) with its commands and log.
"""

import os
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.path.join(ROOT, "target", "release", "examples", "latex_edits")
TEMPLATE = os.path.join(ROOT, "tests", "latex", "edits", "template.tex")


def compile_tex(path):
    """None when pdflatex fails, else the set of its warnings."""
    d, name = os.path.split(path)
    # As latexmk: again while LaTeX asks for it.
    for run in range(4):
        r = subprocess.run(["pdflatex", "-interaction=nonstopmode", "-halt-on-error", name],
                           cwd=d, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                           stderr=subprocess.STDOUT, timeout=120)
        if r.returncode != 0:
            return None
        log = open(path[:-4] + ".log", encoding="latin-1").read()
        if run >= 1 and "Rerun to get" not in log:
            break
    # A warning's text, unwrapped, without where it was.
    log = re.sub(r"\n(?=[^\n]{0,79}\n)", "\n", log)
    warnings = set()
    for m in re.finditer(r"(LaTeX|Package \w+) Warning: (.*?)(?:\n\n|\.\n)", log, re.S):
        w = re.sub(r"\s+", " ", m.group(2))
        w = re.sub(r" on input line \d+", "", w)
        w = re.sub(r"\(\w+\) ", "", w)
        warnings.add(w.strip())
    # (Overfull and underfull boxes depend on the words: not counted.)
    return warnings


def main():
    argv = sys.argv[1:]
    keep = None
    if "--keep" in argv:
        i = argv.index("--keep")
        keep = argv[i + 1]
        del argv[i:i + 2]
    variants = int(argv[0]) if argv else 100
    seed = int(argv[1]) if len(argv) > 1 else 1
    count = int(argv[2]) if len(argv) > 2 else 4
    keep = keep or tempfile.mkdtemp(prefix="latex-edits-")
    os.makedirs(keep, exist_ok=True)
    work = tempfile.mkdtemp()
    shutil.copy(TEMPLATE, os.path.join(work, "template.tex"))
    base = compile_tex(os.path.join(work, "template.tex"))
    if base is None:
        sys.exit("the template does not compile")
    subprocess.run([EXE, TEMPLATE, work, str(variants), str(seed), str(count)], check=True,
                   stdout=subprocess.DEVNULL)
    bad = 0
    for v in range(variants):
        tex = os.path.join(work, f"edit-{seed}-{v}.tex")
        got = compile_tex(tex)
        if got is None:
            why = "does not compile:\n" + "".join(
                l for l in open(tex[:-4] + ".log", encoding="latin-1") if l.startswith("!")
                or l.startswith("l."))
        elif got - base:
            why = "new warnings:\n" + "\n".join(sorted(got - base))
        else:
            continue
        bad += 1
        name = os.path.basename(tex)[:-4]
        shutil.copy(tex, os.path.join(keep, name + ".tex"))
        commands = open(tex[:-4] + ".log.txt").read()
        with open(os.path.join(keep, name + ".why"), "w") as f:
            f.write(commands + "\n" + why + "\n")
        if bad <= int(os.environ.get("SHOW", "5")):
            print(f"== {name}\n{commands}{why}\n")
    print(f"{variants} variants of {count} commands; {bad} fail"
          + (f" (in {keep})" if bad else ""))
    shutil.rmtree(work, ignore_errors=True)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
