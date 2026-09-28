#!/usr/bin/env python3
"""Random Calc formulas of the kind Org table formulas produce, evaluated
by Emacs (tests/emacs/calc.el), for crates/org-table/tests/calc-cases.txt.

Usage: tools/calc-cases.py [COUNT] [SEED]"""

import os, random, subprocess, sys, tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
COUNT = int(sys.argv[1]) if len(sys.argv) > 1 else 3000
SEED = int(sys.argv[2]) if len(sys.argv) > 2 else 1
rng = random.Random(SEED)

VARS = ["a", "b", "x", "Qty", "Price"]
FUNCS1 = ["abs", "sqrt", "exp", "ln", "log10", "sin", "cos", "tan", "floor", "ceil", "round", "trunc", "sign", "fact"]
FUNCS2 = ["mod", "idiv", "fdiv", "choose", "gcd", "lcm", "max", "min"]
VFUNCS = ["vsum", "vmean", "vmax", "vmin", "vmedian", "vsdev", "vvar", "vcount", "vprod", "vlen", "vpvar", "vpsdev"]


def number():
    k = rng.random()
    if k < 0.35:
        return str(rng.randint(0, 20))
    if k < 0.5:
        return str(rng.randint(0, 100000))
    if k < 0.75:
        return f"{rng.randint(0, 999)}.{rng.randint(0, 99):02d}".rstrip("0") or "0"
    if k < 0.82:
        return f"{rng.randint(1, 9)}.{rng.randint(0, 9)}e{rng.choice(['', '-'])}{rng.randint(1, 15)}"
    if k < 0.88:
        return str(rng.randint(10**12, 10**20))
    if k < 0.92:
        return "0." + "0" * rng.randint(1, 6) + str(rng.randint(1, 999))
    return str(rng.randint(1, 60)) + "." + str(rng.randint(0, 999999))


def atom(depth):
    k = rng.random()
    if k < 0.55:
        s = number()
        if rng.random() < 0.15:
            s = "-" + s
        return f"({s})" if rng.random() < 0.3 else s
    if k < 0.65:
        return rng.choice(VARS)
    if k < 0.68:
        return rng.choice(["nan", "inf", "-inf"])
    if k < 0.85 and depth < 3:
        return vector(depth + 1)
    return number()


def vector(depth):
    n = rng.randint(0, 5)
    items = [expr(depth + 1) if rng.random() < 0.2 else atom(depth + 1) for _ in range(n)]
    return "[" + ",".join(items) + "]"


def expr(depth=0):
    k = rng.random()
    if depth > 2 or k < 0.3:
        return atom(depth)
    if k < 0.6:
        op = rng.choice(["+", "-", "*", "/", "*", "+", "^", "%"])
        a, b = expr(depth + 1), expr(depth + 1)
        if op == "^":
            b = rng.choice(["2", "3", "0.5", "-1", "1/2", "10", "0"])
        return f"{a}{op}{b}" if rng.random() < 0.6 else f"{a} {op} {b}"
    if k < 0.7:
        return f"{rng.choice(FUNCS1)}({expr(depth + 1)})"
    if k < 0.78:
        return f"{rng.choice(FUNCS2)}({expr(depth + 1)},{expr(depth + 1)})"
    if k < 0.9:
        return f"{rng.choice(VFUNCS)}({vector(depth + 1)})"
    if k < 0.95:
        c = rng.choice(["<", ">", "<=", ">=", "==", "!="])
        return f"if({expr(depth + 1)}{c}{expr(depth + 1)},{expr(depth + 1)},{expr(depth + 1)})"
    return f"({expr(depth + 1)})"


def main():
    cases = []
    seen = set()
    while len(cases) < COUNT:
        e = expr()
        if e not in seen:
            seen.add(e)
            cases.append(e)
    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "in.txt")
        out = os.path.join(tmp, "out.txt")
        with open(src, "w") as f:
            f.write("\n".join(cases) + "\n")
        subprocess.run(["emacs", "-Q", "--batch", "-l", os.path.join(ROOT, "tests/emacs/calc.el"), src, out],
                       check=True, stderr=subprocess.DEVNULL)
        dest = os.path.join(ROOT, "crates/org-table/tests/calc-cases.txt")
        with open(out) as f, open(dest, "w") as d:
            d.write(f.read())
    print(f"wrote {COUNT} cases")


if __name__ == "__main__":
    main()
