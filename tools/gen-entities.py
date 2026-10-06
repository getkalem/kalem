#!/usr/bin/env python3
"""crates/org-syntax/src/tables/entities.rs: Org's entities, each with its
LaTeX, HTML, ASCII, Latin-1 and UTF-8 renderings, made from standards
(decision D18, Part IV of the Book).

The names are Org's, the interface: `crates/org-syntax/data/entity-names.txt`.
What each stands for comes, in this order, from

1. `crates/org-syntax/data/entities-kalem.tsv`, written for Kalem, for the
   names no standard below defines and where Kalem chooses otherwise;
2. LaTeX's log-like operators (`\\sin`, `\\deg`; `latex.ltx`);
3. the HTML Standard's named character references (`entities.json`,
   WHATWG, CC BY 4.0) and, for the rest, unicode-math's symbol table
   (`unicode-math-table.tex`, LPPL 1.3c), which give the character;

and the renderings of a character from LaTeX's math symbols (`fontmath.ltx`,
`amssymb.sty`, `latexsym.sty`) and text encodings (`*enc.dfu`), LPPL 1.3c,
and the Unicode Character Database (Python's `unicodedata`). No part of
Emacs or Org is read.

    tools/gen-entities.py [--html-entities FILE] [--unicode-math FILE]

downloads what it is not given into .cache/entities/ and finds the LaTeX
files with `kpsewhich` (TeX Live).
"""

import argparse
import json
import pathlib
import re
import subprocess
import unicodedata

ROOT = pathlib.Path(__file__).resolve().parent.parent
DATA = ROOT / "crates/org-syntax/data"
OUT = ROOT / "crates/org-syntax/src/tables/entities.rs"
HTML_URL = "https://html.spec.whatwg.org/entities.json"
UNICODE_MATH_COMMIT = "184a23b0cb259d4dc9848ec3db0aa2cd383cae99"
UNICODE_MATH_URL = (
    "https://raw.githubusercontent.com/latex3/unicode-math/"
    f"{UNICODE_MATH_COMMIT}/unicode-math-table.tex"
)


def fetch(url, name, given):
    if given:
        return pathlib.Path(given).read_text(encoding="utf-8")
    cache = ROOT / ".cache/entities" / name
    if not cache.exists():
        cache.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["curl", "-sSfL", "-o", str(cache), url], check=True)
    return cache.read_text(encoding="utf-8")


def tex_file(name):
    path = subprocess.run(["kpsewhich", name], capture_output=True, text=True).stdout.strip()
    if not path:
        raise SystemExit(f"kpsewhich finds no {name}: a TeX Live installation is needed")
    return pathlib.Path(path).read_text(encoding="latin-1")


def read_sources(args):
    html = {
        k[1:-1]: v["characters"]
        for k, v in json.loads(fetch(HTML_URL, "entities.json", args.html_entities)).items()
        if k.endswith(";")
    }
    # unicode-math: command -> character, and character -> commands.
    um_cmd, um_char = {}, {}
    table = fetch(UNICODE_MATH_URL, "unicode-math-table.tex", args.unicode_math)
    for m in re.finditer(r'\\UnicodeMathSymbol\{"([0-9A-F]+)\}\{\\(\w+)\s*\}', table):
        ch, cmd = chr(int(m.group(1), 16)), m.group(2)
        um_cmd.setdefault(cmd, ch)
        um_char.setdefault(ch, []).append(cmd)
    # LaTeX's math commands, by name.
    math = set()
    for f in ["fontmath.ltx", "amssymb.sty", "latexsym.sty"]:
        text = tex_file(f)
        math |= set(
            re.findall(
                r"\\(?:DeclareMathSymbol|DeclareMathDelimiter|DeclareMathAccent|DeclareRobustCommand)\s*\{?\\(\w+)",
                text,
            )
        )
    # Commands the kernel defines otherwise, which unicode-math names too
    # (`\\cdots`, `\\gets`).
    kernel = tex_file("latex.ltx")
    math |= {
        n
        for n in re.findall(r"\\(?:DeclareRobustCommand|def|let)\s*\{?\\(\w+)", kernel)
        if n in um_cmd
    }
    # LaTeX's log-like operators: name -> its text.
    operators = {
        m.group(1): m.group(2).replace("\\,", " ")
        for m in re.finditer(
            r"\\DeclareRobustCommand\\(\w+)\{\\mathop\{\\operator@font ([^}]*)\}",
            kernel,
        )
    }
    # LaTeX's text encodings: character -> its command.
    text_cmds = {}
    for f in ["t1enc.dfu", "ts1enc.dfu", "omsenc.dfu", "ot1enc.dfu", "utf8enc.dfu"]:
        for m in re.finditer(r"\\DeclareUnicodeCharacter\{([0-9A-F]+)\}\{(.*)\}\s*$", tex_file(f), re.M):
            text_cmds.setdefault(chr(int(m.group(1), 16)), m.group(2).strip())
    return html, um_cmd, um_char, math, operators, text_cmds


def math_char(name, um_cmd):
    """The character unicode-math gives LaTeX's `\\NAME` (upright for a
    letter, as `\\mupalpha`)."""
    return um_cmd.get("mup" + name) or um_cmd.get(name)


# A Greek capital LaTeX has no command for is the Latin letter it looks
# like, as LaTeX's own fonts set it.
GREEK_AS_LATIN = {
    "Α": "A", "Β": "B", "Ε": "E", "Ζ": "Z", "Η": "H", "Ι": "I", "Κ": "K",
    "Μ": "M", "Ν": "N", "Ο": "O", "Ρ": "P", "Τ": "T", "Χ": "X", "Υ": "Y",
    "ο": "o",
}

# LaTeX for the ASCII characters it gives a meaning of its own.
ASCII_LATEX = {
    "#": "\\#", "$": "\\$", "%": "\\%", "&": "\\&", "_": "\\_", "{": "\\{",
    "}": "\\}", "~": "\\textasciitilde{}", "^": "\\textasciicircum{}",
    "\\": "\\textbackslash{}", "<": "\\textless{}", ">": "\\textgreater{}",
    "|": "\\textbar{}", '"': "\\textquotedbl{}",
}


def latex_text(definition):
    """A LaTeX encoding's definition as one writes it in a document."""
    if definition == "\\nobreakspace":
        return "~"
    d = definition.replace("\\@tabacckludge", "\\")
    # An accent and its letter: `\'A`, `\r A`, `\c C` as `\'{A}`.
    m = re.fullmatch(r"(\\(?:[^a-zA-Z]|[a-zA-Z]+))\s*(\\?[a-zA-Z]+|.)", d)
    if m and not re.fullmatch(r"\\[a-zA-Z]+", d):
        return f"{m.group(1)}{{{m.group(2)}}}"
    if re.fullmatch(r"\\[a-zA-Z]+", d):
        return d + "{}"
    return d


# ASCII for a character no decomposition reaches, by Unicode's name (or
# the character), written for Kalem.
ASCII_LETTERS = {
    "ß": "ss", "Æ": "AE", "æ": "ae", "Ø": "O", "ø": "o", "Œ": "OE", "œ": "oe",
    "Ł": "L", "ł": "l", "Đ": "D", "đ": "d", "Þ": "TH", "þ": "th", "Ð": "D",
    "ð": "d", "ı": "i", "ȷ": "j", "ƒ": "f",
}
ASCII_SYMBOLS = {
    "→": "->", "←": "<-", "↔": "<->", "⇒": "=>", "⇐": "<=", "⇔": "<=>",
    "↑": "^", "↓": "v", "≤": "<=", "≥": ">=", "≠": "!=", "≈": "~=", "≡": "==",
    "±": "+-", "∓": "-+", "×": "*", "÷": "/", "·": ".", "•": "*", "…": "...",
    "‐": "-", "–": "-", "—": "--", "−": "-", "‘": "'", "’": "'", "‚": ",",
    "“": '"', "”": '"', "„": '"', "«": "<<", "»": ">>", "‹": "<", "›": ">",
    "\u00a0": " ", "\u2002": " ", "\u2003": " ", "\u2009": " ", "\u200b": "",
    "\u200c": "", "\u200d": "", "\u200e": "", "\u200f": "", "\u00ad": "-",
    "©": "(c)", "®": "(r)", "™": "TM", "°": "deg", "€": "EUR", "£": "GBP",
    "¥": "JPY", "¢": "cent", "¤": "currency", "§": "S", "¶": "P", "†": "+",
    "‡": "++", "′": "'", "″": "''", "∞": "infinity", "√": "sqrt", "∑": "Sum",
    "∏": "Prod", "∫": "integral", "∂": "d", "∇": "nabla", "∀": "for all",
    "∃": "exists", "∅": "empty set", "∈": "in", "∉": "not in", "∩": "intersection",
    "∪": "union", "⊂": "subset", "⊃": "superset", "¬": "not", "∧": "and",
    "∨": "or", "¡": "!", "¿": "?", "ª": "a", "º": "o", "¹": "1", "²": "2",
    "³": "3", "¼": "1/4", "½": "1/2", "¾": "3/4", "µ": "micro", "¦": "|",
    "¨": '"', "¯": "-", "´": "'", "¸": ",", "ˆ": "^", "˜": "~", "◊": "<>",
    "♠": "spades", "♣": "clubs", "♥": "hearts", "♦": "diamonds", "∗": "*",
    "∼": "~", "∝": "proportional to", "∠": "angle", "⊥": "perpendicular",
    "⌈": "[", "⌉": "]", "⌊": "[", "⌋": "]", "〈": "<", "〉": ">", "⟨": "<", "⟩": ">",
}


def ascii_of(text):
    out = []
    for c in text:
        if c.isascii():
            out.append(c)
            continue
        if c in ASCII_SYMBOLS:
            out.append(ASCII_SYMBOLS[c])
            continue
        if c in ASCII_LETTERS:
            out.append(ASCII_LETTERS[c])
            continue
        bare = "".join(x for x in unicodedata.normalize("NFKD", c) if not unicodedata.combining(x))
        if bare and bare.isascii():
            out.append(bare)
            continue
        name = unicodedata.name(c, "")
        g = re.fullmatch(r"GREEK (?:(SMALL|CAPITAL) LETTER )?([A-Z ]+?)(?: SYMBOL)?", name)
        if g:
            word = g.group(2).split()[-1].lower()
            out.append(word.capitalize() if g.group(1) == "CAPITAL" else word)
            continue
        out.append(f"[{name.lower()}]" if name else "?")
    return "".join(out)


def latin1_of(text):
    try:
        text.encode("latin-1")
        return text
    except UnicodeEncodeError:
        return ascii_of(text)


def html_of(name, ch, html):
    if html.get(name) == ch:
        return f"&{name};"
    return "".join(c if c.isascii() else f"&#{ord(c)};" for c in ch)


def kalem_rows():
    rows = {}
    for line in (DATA / "entities-kalem.tsv").read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        name, *cols = line.split("\t")
        cols += [""] * (6 - len(cols))
        # utf8, latex, math, html, ascii, latin1; an empty column is derived.
        # `""` is the empty string itself; `\\u{200E}` a character.
        cols = [re.sub(r"\\u\{([0-9A-Fa-f]+)\}", lambda m: chr(int(m.group(1), 16)), c) for c in cols]
        rows[name] = [None if c == "" else "" if c == '""' else c for c in cols]
    return rows


def entry(name, src, kalem):
    html, um_cmd, um_char, math, operators, text_cmds = src
    k = kalem.get(name, [None] * 6)
    utf8, latex, is_math, html_r, ascii_r, latin1_r = k
    spaces = re.fullmatch(r"_ (\d+)", name)
    if spaces:
        n = int(spaces.group(1))
        name = "_" + " " * n
        utf8 = utf8 or "\u2002" * n
        latex = latex or f"\\hspace*{{{n * 0.5:g}em}}"
        is_math = is_math or "false"
        html_r = html_r or "&ensp;" * n
    elif name in operators and utf8 is None:
        text = operators[name]
        utf8 = text
        latex = latex or f"\\{name}"
        is_math = is_math or "true"
        html_r = html_r or text
    elif utf8 is None and name in math and math_char(name, um_cmd):
        # A LaTeX command means what it means in LaTeX (`\\circ` is a ring).
        utf8 = math_char(name, um_cmd)
        latex = latex or f"\\{name}"
        is_math = is_math or "true"
    if utf8 is None:
        utf8 = html.get(name) or um_cmd.get(name)
    if utf8 is None:
        raise SystemExit(f"{name}: no standard defines it; give it a row in entities-kalem.tsv")
    if latex is None:
        if name in math and um_cmd.get(name) == utf8:
            latex, is_math = f"\\{name}", "true"
        elif len(utf8) == 1 and any(
            c in math or c.removeprefix("mup") in math for c in um_char.get(utf8, [])
        ):
            cmd = next(
                c if c in math else c.removeprefix("mup")
                for c in um_char[utf8]
                if c in math or c.removeprefix("mup") in math
            )
            latex, is_math = f"\\{cmd}", "true"
        elif len(utf8) == 1 and utf8 in text_cmds:
            latex, is_math = latex_text(text_cmds[utf8]), "false"
        elif utf8.isascii() and utf8.isprintable():
            latex, is_math = "".join(ASCII_LATEX.get(c, c) for c in utf8), "false"
        elif utf8 in GREEK_AS_LATIN:
            latex, is_math = GREEK_AS_LATIN[utf8], "false"
        else:
            raise SystemExit(f"{name}: no LaTeX command for {utf8!r}; give it one in entities-kalem.tsv")
    is_math = is_math or "false"
    html_r = html_r if html_r is not None else html_of(name, utf8, html)
    ascii_r = ascii_r if ascii_r is not None else ascii_of(utf8)
    latin1_r = latin1_r if latin1_r is not None else latin1_of(utf8)
    return (name, latex, is_math == "true", html_r, ascii_r, latin1_r, utf8)


def rust(s):
    """A Rust string literal; characters one cannot see (format characters,
    spaces other than the space) as `\\u{…}`."""
    out = []
    for c in s:
        if c in '"\\':
            out.append("\\" + c)
        elif c == " " or (c.isprintable() and unicodedata.category(c) not in ("Cf", "Zs")):
            out.append(c)
        else:
            out.append(f"\\u{{{ord(c):X}}}")
    return '"' + "".join(out) + '"'


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--html-entities")
    p.add_argument("--unicode-math")
    args = p.parse_args()
    src = read_sources(args)
    kalem = kalem_rows()
    names = [
        l for l in (DATA / "entity-names.txt").read_text(encoding="utf-8").splitlines()
        if l and not l.startswith("#")
    ]
    entries, missing = [], []
    for n in names:
        try:
            entries.append(entry(n, src, kalem))
        except SystemExit as e:
            missing.append(str(e))
    if missing:
        raise SystemExit("\n".join(missing))
    entries.sort(key=lambda e: e[0])
    lines = [
        "// @generated by tools/gen-entities.py (decision D18). Do not edit.",
        "//",
        "// Org's entity names (crates/org-syntax/data/entity-names.txt), each with",
        "// its renderings, made from the HTML Standard's named character",
        "// references (WHATWG, CC BY 4.0), unicode-math's symbol table and LaTeX's",
        "// math symbols and text encodings (LPPL 1.3c), the Unicode Character",
        "// Database, and crates/org-syntax/data/entities-kalem.tsv:",
        "// (name, latex, latex_math_p, html, ascii, latin1, utf8).",
        "",
        f"pub(crate) static ENTITIES: [(&str, &str, bool, &str, &str, &str, &str); {len(entries)}] = [",
    ]
    for e in entries:
        lines.append(
            f"    ({rust(e[0])}, {rust(e[1])}, {'true' if e[2] else 'false'}, "
            f"{rust(e[3])}, {rust(e[4])}, {rust(e[5])}, {rust(e[6])}),"
        )
    lines.append("];")
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8")
    subprocess.run(["rustfmt", "--edition", "2024", str(OUT)], check=True)
    print(f"{OUT.relative_to(ROOT)}: {len(entries)} entities")


if __name__ == "__main__":
    main()
