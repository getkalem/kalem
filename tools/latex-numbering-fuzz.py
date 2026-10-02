#!/usr/bin/env python3
"""Random LaTeX documents numbered by pdflatex and by Kalem's model.

    tools/latex-numbering-fuzz.py [N] [SEED] [--keep DIR]

Each document mixes, at random, the classes (article, report, book,
amsart), the sectioning commands (starred, `\\appendix`, `\\part`,
`secnumdepth`), displayed equations (`equation`, `align` with
`\\nonumber`, `gather`, `multline`, `subequations`, `\\tag`), theorems
declared every way `\\newtheorem` allows (numbered within a section,
sharing a counter, subordinate to another), floats with captions,
nested `enumerate` items, footnotes and counter commands
(`\\setcounter`, `\\addtocounter`, `\\stepcounter`, `\\numberwithin`).
pdflatex runs twice; every `\\label`'s number in the `.aux` file must be
the number Kalem's model gives it, a label LaTeX does not write must
have none, and amsmath's "Multiple \\label's" errors (LaTeX carries on
past them) must be the label clashes Kalem reports. Documents with
other errors are left out. A document that differs is written to the --keep folder (default
the scratch folder printed) with its differences, to become a fixture of
`tests/latex/model` when it shows a bug.
"""

import os
import random
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def document(rng):
    cls = rng.choice(["article", "article", "report", "book", "amsart"])
    chapters = cls in ("report", "book")
    pre = [f"\\documentclass{{{cls}}}", "\\usepackage{amsmath,amsthm}"]
    top = "chapter" if chapters else "section"
    within = rng.choice([None, "section", top])
    if within:
        pre.append(f"\\numberwithin{{equation}}{{{within}}}")
    thms = []
    style = rng.choice(["section", top, None])
    pre.append(f"\\newtheorem{{thm}}{{Theorem}}" + (f"[{style}]" if style else ""))
    thms.append("thm")
    if rng.random() < 0.7:
        pre.append("\\newtheorem{lem}[thm]{Lemma}")
        thms.append("lem")
    if rng.random() < 0.5:
        pre.append("\\newtheorem{cor}{Corollary}[thm]")
        thms.append("cor")
    if rng.random() < 0.5:
        pre.append("\\newtheorem{defn}{Definition}")
        thms.append("defn")
    if rng.random() < 0.3:
        pre.append("\\newtheorem*{rem}{Remark}")
    if rng.random() < 0.3:
        pre.append(f"\\setcounter{{secnumdepth}}{{{rng.randint(0, 3)}}}")
    if rng.random() < 0.2:
        pre.append(f"\\counterwithin{{figure}}{{{top}}}")
    if rng.random() < 0.15:
        pre.append(f"\\counterwithout{{equation}}{{{top}}}")
    if rng.random() < 0.2:
        pre.append(rng.choice([
            "\\renewcommand{\\thesection}{\\Roman{section}}",
            "\\renewcommand{\\theequation}{\\alph{equation}}",
            "\\renewcommand{\\thefigure}{F\\arabic{figure}}",
        ]))
    body = []
    n = 0

    def label():
        nonlocal n
        n += 1
        return f"\\label{{k{n}}}"

    appendix = False
    for _ in range(rng.randint(8, 30)):
        r = rng.random()
        if r < 0.25:
            levels = (["part", "chapter"] if chapters else ["part"]) + [
                "section", "subsection", "subsubsection", "paragraph"]
            weights = [1, 4, 6, 4, 2, 1] if chapters else [1, 6, 4, 2, 1]
            lvl = rng.choices(levels, weights)[0]
            star = "*" if rng.random() < 0.15 else ""
            body.append(f"\\{lvl}{star}{{T}}" + (label() if rng.random() < 0.6 else ""))
        elif r < 0.27 and not appendix:
            appendix = True
            body.append("\\appendix")
        elif r < 0.45:
            kind = rng.choice(["equation", "equation", "align", "gather", "multline", "sub",
                               "tag", "align*", "eqnarray", "flalign", "alignat",
                               "equation*", "gather*", "multline*"])
            if kind in ("equation*", "multline*"):
                body.append(f"\\begin{{{kind}}}x{label()}\\end{{{kind}}}")
            elif kind == "equation":
                body.append(f"\\begin{{equation}}x{label()}\\end{{equation}}")
            elif kind == "tag":
                body.append(f"\\begin{{equation}}x\\tag{{T{n}}}{label()}\\end{{equation}}")
            elif kind == "multline":
                body.append(f"\\begin{{multline}}a\\\\b{label()}\\end{{multline}}")
            elif kind == "sub":
                inner = "".join(
                    f"\\begin{{equation}}y{label()}\\end{{equation}}"
                    for _ in range(rng.randint(1, 3)))
                body.append(f"\\begin{{subequations}}{label()}{inner}\\end{{subequations}}")
            else:
                cell = {"gather": "a=b", "gather*": "a=b", "eqnarray": "a&=&b"}.get(kind, "a&=b")
                rows = []
                for _ in range(rng.randint(1, 4)):
                    x = rng.random()
                    end = ("\\nonumber" if x < 0.2 else f"\\tag{{R{n}}}" if x < 0.3
                           and kind not in ("eqnarray", "align*", "gather*") else "")
                    rows.append(cell + end + (label() if rng.random() < 0.7 else ""))
                args = "{1}" if kind == "alignat" else ""
                body.append(f"\\begin{{{kind}}}{args}" + "\\\\".join(rows) + f"\\end{{{kind}}}")
        elif r < 0.62:
            t = rng.choice(thms)
            title = "[Name]" if rng.random() < 0.3 else ""
            body.append(f"\\begin{{{t}}}{title}x{label()}\\end{{{t}}}")
        elif r < 0.72:
            f = rng.choice(["figure", "table"])
            if rng.random() < 0.5:
                body.append(f"\\begin{{{f}}}\\caption{{C}}{label()}\\end{{{f}}}")
            else:
                body.append(f"\\begin{{{f}}}{label()}\\caption{{C}}\\end{{{f}}}")
        elif r < 0.82:
            def items(depth):
                out = []
                for _ in range(rng.randint(1, 3)):
                    s = "\\item x" + (label() if rng.random() < 0.6 else "")
                    if depth < 3 and rng.random() < 0.3:
                        s += items(depth + 1)
                    out.append(s)
                return "\\begin{enumerate}" + "".join(out) + "\\end{enumerate}"
            body.append(items(1))
        elif r < 0.88:
            body.append(f"Text\\footnote{{N{label()}}}.")
        else:
            c = rng.choice(["section", "equation", "thm", "figure", "footnote"]
                           + (["chapter"] if chapters else []))
            op = rng.choice(["setcounter", "addtocounter", "stepcounter"])
            if op == "stepcounter":
                body.append(f"\\stepcounter{{{c}}}")
            else:
                body.append(f"\\{op}{{{c}}}{{{rng.randint(0, 5)}}}")
        body.append("")
    # Text last: a label on a page TeX never ships (after `\\part` at the
    # end of a report) is not written, which is page building, not
    # numbering.
    return "\n".join(pre + ["\\begin{document}"] + body + ["End.", "\\end{document}", ""])


CLASH = "Package amsmath Error: Multiple \\label's"


def pdflatex(text, d):
    """The numbers of the labels LaTeX writes, with the count of amsmath's
    "Multiple \\label's" errors under the key "!clash"; None when the
    document has another error."""
    with open(os.path.join(d, "t.tex"), "w") as f:
        f.write(text)
    for _ in range(2):
        r = subprocess.run(
            ["pdflatex", "-interaction=nonstopmode", "t.tex"], cwd=d, stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
        log = r.stdout.decode("latin-1")
        errors = re.findall(r"^! (.*)$", log, re.M)
        # Past amsmath's label clashes LaTeX carries on; not past others.
        if any(not e.startswith(CLASH) for e in errors) or not os.path.exists(
                os.path.join(d, "t.aux")):
            return None
    aux = open(os.path.join(d, "t.aux"), encoding="latin-1").read()
    out = {"!clash": str(len(errors))}
    for line in aux.splitlines():
        m = re.match(r"\\newlabel\{([^}@]*)\}\{\{", line)
        if not m:
            continue
        # The first group of the second argument, braces balanced.
        i, depth = m.end(), 1
        while i < len(line) and depth:
            depth += {"{": 1, "}": -1}.get(line[i], 0)
            i += 1
        num = line[m.end():i - 1]
        # `\tag{A}` is written `{A}`: what the reference prints.
        while num.startswith("{") and num.endswith("}"):
            num = num[1:-1]
        out[m.group(1)] = num
    return out


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    n = int(args[0]) if args else 50
    seed = int(args[1]) if len(args) > 1 else 1
    keep = None
    if "--keep" in sys.argv:
        keep = sys.argv[sys.argv.index("--keep") + 1]
    keep = keep or tempfile.mkdtemp(prefix="latex-numbering-")
    os.makedirs(keep, exist_ok=True)
    exe = os.path.join(ROOT, "target", "release", "examples", "labels")
    rng = random.Random(seed)
    work = tempfile.mkdtemp()
    docs = []
    for i in range(n):
        text = document(rng)
        d = os.path.join(work, str(i))
        os.makedirs(d)
        latex = pdflatex(text, d)
        if latex is not None:
            docs.append((i, text, latex, os.path.join(d, "t.tex")))
    files = [p for (_, _, _, p) in docs]
    got = {}
    clashes = {}
    if files:
        r = subprocess.run([exe] + files, stdout=subprocess.PIPE, text=True, check=True)
        for line in r.stdout.splitlines():
            f, key, *num = line.split(" ")
            if key == "!clash":
                clashes[f] = clashes.get(f, 0) + 1
            else:
                got[(f, key)] = " ".join(num)
    bad = 0
    for i, text, latex, path in docs:
        got[(path, "!clash")] = str(clashes.get(path, 0))
        diffs = [f"{k}: LaTeX {v!r}, Kalem {got.get((path, k))!r}"
                 for k, v in latex.items() if got.get((path, k)) != v]
        # A label LaTeX does not write has no number in Kalem either.
        diffs += [f"{k}: LaTeX writes nothing, Kalem {n!r}" for (f, k), n in got.items()
                  if f == path and k not in latex and n]
        if diffs:
            bad += 1
            base = os.path.join(keep, f"doc{seed}-{i}")
            with open(base + ".tex", "w") as f:
                f.write(text)
            with open(base + ".diff", "w") as f:
                f.write("\n".join(diffs) + "\n")
    labels = sum(len(l) - 1 for (_, _, l, _) in docs)
    clashed = sum(1 for (_, _, l, _) in docs if l["!clash"] != "0")
    print(f"{len(docs)}/{n} documents compiled ({clashed} past label clashes), {labels} "
          f"labels; {bad} documents differ"
          + (f" (in {keep})" if bad else ""))
    shutil.rmtree(work, ignore_errors=True)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
