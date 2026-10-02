#!/usr/bin/env python3
"""Random lines of LaTeX text typeset by pdflatex and shown by Kalem.

    tools/latex-typeset-fuzz.py [N] [SEED] [--keep DIR]
    tools/latex-typeset-fuzz.py --each [--keep DIR]

Each line mixes, at random, words and punctuation, dashes and quotes,
accents (`\\'e`, `\\c{c}`, `\\v{s}`...), letters (`\\ss`, `\\o`, `\\ae`),
symbols (`\\S`, `\\dag`, `\\copyright`, `\\%`...), spacing commands,
font changes (`\\emph`, `\\textbf`, `{\\itshape ...}`), boxes, case
changes and `\\verb`; `--each` takes every construct once instead.
pdflatex typesets every line in a box and its glyphs are read back from
`\\showbox`. Kalem's rendered view of the same line (its text, not the
markup it shows dimmed) is typeset the same way, as UTF-8 text, so both
sides go through the same font encoding and ligatures and no glyph
table is needed. What only TeX's boxes decide is compared loosely: the
f-ligatures as letters, a double quote as two single ones, the dotless
i as an i. A line whose glyphs differ is reported, with the differences
kept in the --keep folder (default a scratch folder, printed).
"""

import os
import random
import re
import shutil
import subprocess
import sys
import tempfile
import unicodedata

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.path.join(ROOT, "target", "release", "examples", "latex_shown")

PREAMBLE = r"""\documentclass{article}
\usepackage[T1]{fontenc}
\usepackage[utf8]{inputenc}
\usepackage{textcomp}
"""

WORDS = ["word", "office", "flow", "Text", "AVA", "naïve", "café", "x", "1990", "a.b.", "e.g.",
         "Mr.", "end", "fifty", "baffle"]
PUNCT = [",", ".", ";", ":", "!", "?", "(", ")", "[", "]", "/", "*", "+", "=", "@"]
# Not `<<`, `>>` and `,,`: ligatures of T1 fonts only.
DASHES = ["-", "--", "---", "``", "''", "`", "'", "?`", "!`"]
ACCENTS = ["'", "`", "^", '"', "~", "=", ".", "c", "v", "u", "H", "r", "k"]
LETTERS = [r"\ss", r"\o", r"\O", r"\ae", r"\AE", r"\oe", r"\OE", r"\l", r"\L", r"\aa", r"\AA",
           r"\i", r"\dh", r"\DH", r"\th", r"\TH", r"\ng", r"\NG", r"\dj", r"\DJ"]
SYMBOLS = [r"\S", r"\P", r"\dag", r"\ddag", r"\copyright", r"\textregistered",
           r"\texttrademark", r"\pounds", r"\textbullet", r"\textendash", r"\textemdash",
           r"\textquoteleft", r"\textquoteright", r"\textquotedblleft", r"\textquotedblright",
           r"\guillemotleft", r"\guillemotright", r"\textless", r"\textgreater", r"\textbar",
           r"\&", r"\%", r"\$", r"\#", r"\_", r"\{", r"\}", r"\textbackslash",
           r"\textasciitilde", r"\textasciicircum", r"\ldots", r"\dots", r"\textellipsis",
           r"\textdegree", r"\textperiodcentered", r"\textexclamdown", r"\textquestiondown",
           r"\textsection", r"\textparagraph", r"\textdagger", r"\textcent", r"\texteuro",
           r"\textyen", r"\textmu", r"\texttimes", r"\textdiv", r"\textonehalf",
           r"\textpm", r"\textordfeminine", r"\textordmasculine", r"\textbrokenbar",
           r"\slash", r"\textvisiblespace",
           r"\textasteriskcentered", r"\textquotesingle", r"\textquotedbl",
           r"\textunderscore", r"\textbraceleft", r"\textbraceright", r"\textnumero",
           r"\textcelsius", r"\textohm", r"\textleftarrow", r"\textrightarrow"]
SPACES = [r"\,", r"\;", r"\:", r"\!", r"\ ", "~", r"\quad", r"\qquad", r"\@", r"\/", r"\-",
          r"\hspace{1em}", r"\enspace", r"\thinspace", r"\space", r"\nobreakspace", r"\relax",
          r"\leavevmode"]
# Not the typewriter fonts: which ligatures they have (`--`, `?``) depends
# on the font encoding.
FONTS = [r"\emph", r"\textbf", r"\textit", r"\textsc", r"\textsf", r"\textrm",
         r"\textup", r"\textsl", r"\textmd", r"\textnormal", r"\mbox", r"\textsuperscript",
         r"\textsubscript", r"\MakeUppercase", r"\MakeLowercase", r"\uppercase", r"\lowercase",
         r"\underline", r"\hbox", r"\makebox", r"\text"]
DECLS = [r"\bfseries", r"\itshape", r"\scshape", r"\sffamily", r"\em", r"\bf",
         r"\it", r"\sc", r"\upshape", r"\normalfont", r"\small", r"\large", r"\Huge",
         r"\slshape", r"\mdseries", r"\rmfamily"]


def atom(rng, depth):
    r = rng.random()
    if r < 0.25:
        return rng.choice(WORDS)
    if r < 0.33:
        return rng.choice(PUNCT)
    if r < 0.43:
        return rng.choice(DASHES)
    if r < 0.53:
        a = rng.choice(ACCENTS)
        base = rng.choice("aeiouncsgzAEOCSZ") if rng.random() < 0.9 or a in "ck" else r"\i"
        form = rng.choice(["{%s}", "%s", "{%s}"]) if a.isalpha() else rng.choice(["{%s}", "%s"])
        if a.isalpha() and form == "%s":
            form = " %s"
        out = "\\" + a + (form % base)
        return "{" + out + "}" if rng.random() < 0.2 else out
    if r < 0.58:
        return rng.choice(LETTERS) + rng.choice(["{}", " ", "\\ "]) 
    if r < 0.70:
        s = rng.choice(SYMBOLS)
        # (`\textendash` and a hyphen after it make a ligature: an em dash.)
        if s in (r"\textendash", r"\textemdash"):
            return s + "{}"
        return s + (rng.choice(["{}", " ", "\\ "]) if s[-1].isalpha() else "")
    if r < 0.76:
        return rng.choice(SPACES)
    if r < 0.88 and depth < 3:
        f = rng.choice(FONTS)
        inner = phrase(rng, depth + 1, 1, 3)
        if f in (r"\hbox", r"\makebox", r"\text"):
            f = r"\mbox"
        return f"{f}{{{inner}}}"
    if r < 0.95 and depth < 3:
        return "{" + rng.choice(DECLS) + " " + phrase(rng, depth + 1, 1, 3) + "}"
    if r < 0.97 and depth == 0:
        return r"\verb|" + rng.choice(["a_b", "\\x", "{y}", "% z", "~#"]) + "|"
    return rng.choice(WORDS)


def phrase(rng, depth, lo, hi):
    parts = []
    for _ in range(rng.randint(lo, hi)):
        parts.append(atom(rng, depth))
        parts.append(rng.choice([" ", " ", "", "  "]))
    s = "".join(parts).strip()
    # A trailing `\ ` would lose its space and escape what follows.
    return s + "{}" if s.endswith("\\") else s


COMBINING = {"\u0300": "`", "\u0301": "'", "\u0302": "^", "\u0303": "~", "\u0304": "=",
             "\u0306": "u", "\u0307": ".", "\u0308": '"', "\u030a": "r", "\u030b": "H",
             "\u030c": "v", "\u0327": "c", "\u0328": "k"}


def each():
    """Every construct once, between two words: which ones differ."""
    out = []
    for x in SYMBOLS + LETTERS:
        out += [f"a {x}{{}} b", f"a {x} b", f"a{x}b" if not x[-1].isalpha() else f"a {x}\\ b"]
    for a in ACCENTS:
        for b in ["e", "o", "C", r"\i", "s"]:
            out += [f"a \\{a}{{{b}}} b", f"a {{\\{a} {b}}} b"]
    for d in DASHES + PUNCT + SPACES:
        out += [f"a {d} b", f"a{d}b"]
    for f in FONTS:
        out += [f"a {f}{{x y}} b"]
    for d in DECLS:
        out += [f"a {{{d} x y}} b", f"a {d} x"]
    out += [r"a \verb|x_y| b", r"a \verb+{z}+ b", "a % comment", "a~b", "x \\S\\"]
    return out


def escape(s):
    """Kalem's shown text as LaTeX input that typesets those characters:
    accented letters as accent commands, so a letter the font has no
    glyph for is built as LaTeX builds it."""
    s = unicodedata.normalize("NFD", s)
    out = []
    for c in s:
        if c in COMBINING and out:
            base = out.pop()
            if c not in ("\u0327", "\u0328"):
                base = {"i": r"\i", "j": r"\j"}.get(base, base)
            out.append("{\\" + COMBINING[c] + "{" + base + "}}")
            continue
        out.append({
            "#": r"\#", "$": r"\$", "%": r"\%", "&": r"\&", "_": r"\_", "{": r"\{", "}": r"\}",
            "~": r"\textasciitilde{}", "\\": r"\textbackslash{}", "^": r"\textasciicircum{}",
            "\u00a0": "~", "<": r"\textless{}", ">": r"\textgreater{}", "|": r"\textbar{}",
            "-": "{}-{}",
            # Not a ligature with what is before it (`!`, `?`, a quote).
            "\u2018": "{}\u2018", "\u201c": "{}\u201c",
            "\u2013": "{}\u2013", "\u2014": "{}\u2014", "`": r"\textasciigrave{}", "'": r"\textquotesingle{}",
            '"': r"\textquotedbl{}", "\u2009": r"\,",
            "\u2003": r"\quad{}", "\u2002": r"\enspace{}", "\u2005": r"\;", "\u2217": r"\textasteriskcentered{}",
            "\u2126": r"\textohm{}",
            # NFD makes the ohm sign a capital omega.
            "\u03a9": r"\textohm{}", "\u205f": r"\:", " ": " ",
        }.get(c, c))
    return "".join(out)


def boxed(lines):
    """A document that shows a box of each line: `\\showbox` per line."""
    body = []
    for i, l in enumerate(lines):
        body.append(f"\\typeout{{PROBE{{{i}}}}}\\setbox0\\hbox{{%\n{l}\n}}\\showbox0")
    return (PREAMBLE + "\\showboxdepth=100 \\showboxbreadth=100000 \\scrollmode\n"
            + "\\begin{document}\n" + "\n".join(body) + "\n\\end{document}\n")


def glyphs(d, name, text, n):
    """The glyphs of each probe box (None for a probe with an error)."""
    with open(os.path.join(d, name + ".tex"), "w") as f:
        f.write(text)
    subprocess.run(["pdflatex", "-interaction=scrollmode", name + ".tex"], cwd=d,
                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=120)
    log = open(os.path.join(d, name + ".log"), encoding="latin-1").read()
    out = [None] * n
    parts = re.split(r"PROBE\{(\d+)\}", log)
    for k in range(1, len(parts), 2):
        i, block = int(parts[k]), parts[k + 1]
        block = block.split("! OK", 1)
        if len(block) < 2 or re.search(r"^! ", block[0], re.M):
            continue
        s = ""
        skip = None
        for line in block[0].split("\n"):
            dots = len(line) - len(line.lstrip("."))
            if skip is not None and dots > skip:
                continue
            skip = None
            # What a discretionary would put at a break: not typeset.
            if re.match(r"^\.+\\discretionary", line):
                skip = dots
                continue
            line = re.sub(r" \(ligature [^)]*\)$", "", line)
            g = re.match(r"^\.+\\[A-Za-z0-9]+/[^ ]+ (.*)$", line)
            if g:
                s += g.group(1)
            # Interword glue; not the fill an accent's box is built with.
            elif re.match(r"^\.+\\glue", line) and "skip" not in line and "fil" not in line:
                s += " "
        # Ligatures as their letters (a typewriter or small caps font has
        # none, the roman one Kalem's text is set in has), and the dotless
        # i as an i (`\\~i` puts the tilde over the dot).
        for lig, letters in (("^^[", "ff"), ("^^\\", "fi"), ("^^]", "fl"), ("^^^", "ffi"),
                             ("^^_", "ffl"), ("^^Y", "i"),
                             # T1's capital sharp s, `\\SS`: the letters.
                             ("\xdf", "SS"),
                             # Double quotes as two single ones: whether two
                             # quotes join depends on the boxes between them.
                             ("^^Q", "''"), ("^^P", "``")):
            s = s.replace(lig, letters)
        # A cedilla or an ogonek, built beside its letter in either order.
        s = re.sub(r"\x0b|\^\^L", "", s)
        out[i] = re.sub(" +", " ", s).strip()
    return out


def main():
    argv = sys.argv[1:]
    keep = None
    if "--keep" in argv:
        i = argv.index("--keep")
        keep = argv[i + 1]
        del argv[i:i + 2]
    args = [a for a in argv if not a.startswith("--")]
    n = int(args[0]) if args else 300
    seed = int(args[1]) if len(args) > 1 else 1
    keep = keep or tempfile.mkdtemp(prefix="latex-typeset-")
    os.makedirs(keep, exist_ok=True)
    rng = random.Random(seed)
    if "--each" in sys.argv:
        lines = each()
        n = len(lines)
    else:
        lines = [phrase(rng, 0, 2, 8) for _ in range(n)]
    d = tempfile.mkdtemp()
    want = glyphs(d, "latex", boxed(lines), n)
    # Kalem's view of the same lines, one per line after \clearpage.
    src = os.path.join(d, "t.tex")
    with open(src, "w") as f:
        f.write(PREAMBLE + "\\begin{document}\n\\clearpage\n" + "\n".join(lines)
                + "\n\\end{document}\n")
    shown = [None] * n
    out = subprocess.run([EXE, src], stdout=subprocess.PIPE, text=True, check=True).stdout
    first = None
    for row in out.splitlines():
        _, line, text = row.split("\t", 2)
        first = int(line) if first is None else first
        shown[int(line) - first] = text
    got = glyphs(d, "kalem", boxed([escape(s or "") for s in shown]), n)
    bad = []
    compiled = 0
    for i in range(n):
        if want[i] is None:
            continue
        compiled += 1
        if got[i] != want[i]:
            bad.append(f"{lines[i]}\n  LaTeX {want[i]!r}\n  Kalem {got[i]!r}  (shown {shown[i]!r})")
    print(f"{compiled}/{n} lines typeset; {len(bad)} differ" + (f" (in {keep})" if bad else ""))
    if bad:
        with open(os.path.join(keep, f"typeset{seed}.diff"), "w", encoding="utf-8") as f:
            f.write("\n".join(bad) + "\n")
        for b in bad[:int(os.environ.get("SHOW", "15"))]:
            print(b)
    shutil.rmtree(d, ignore_errors=True)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
