#!/usr/bin/env python3
"""Random Org tables with #+TBLFM formulas of the kinds people write,
recalculated by Emacs (tests/emacs/table.el) for the org-table corpus
test: tests/corpus/tables/random-SEED.org and
crates/org-table/tests/tables/random-SEED.expected.

Usage: tools/table-cases.py [COUNT] [SEED]"""

import os, random, subprocess, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
COUNT = int(sys.argv[1]) if len(sys.argv) > 1 else 300
SEED = int(sys.argv[2]) if len(sys.argv) > 2 else 1
rng = random.Random(SEED)

WORDS = ["apple", "Total", "x", "n/a", "Qty", "b2", "Price", "foo bar"]


def cell(kind):
    k = rng.random()
    if kind == "dur":
        return f"{rng.randint(0, 30)}:{rng.randint(0, 59):02d}" + (f":{rng.randint(0, 59):02d}" if rng.random() < 0.3 else "")
    if kind == "date":
        y, m, d = rng.randint(2020, 2027), rng.randint(1, 12), rng.randint(1, 28)
        wd = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][(__import__("datetime").date(y, m, d)).weekday()]
        o, c = ("<", ">") if rng.random() < 0.5 else ("[", "]")
        t = f" {rng.randint(0, 23):02d}:{rng.randint(0, 59):02d}" if rng.random() < 0.2 else ""
        return f"{o}{y}-{m:02d}-{d:02d} {wd}{t}{c}"
    if k < 0.08:
        return ""
    if k < 0.13:
        return rng.choice(WORDS)
    if k < 0.5:
        return str(rng.randint(-20, 200))
    if k < 0.8:
        return f"{rng.randint(0, 999)}.{rng.randint(0, 99):02d}"
    if k < 0.9:
        return str(rng.randint(0, 10**6))
    return f"{rng.randint(1, 9)}.{rng.randint(0, 9)}e{rng.randint(-3, 5)}"


EXPRS = [
    "$1+$2", "$1-$2", "$1*$2", "$2/$1", "($1+$2)/2", "$1^2", "round($1)", "abs($1-$2)",
    "if($1>$2,$1,$2)", "max($1,$2)", "min($1,$2)", "$1%7", "vsum($1..$2)", "vmean($1..$2)",
    "sqrt(abs($1))", "floor($1/3)", "$1*1.2", "$1+$2*$1", "-$1", "$1/($2+1)", "exp(1)*$1",
    "$1*100/$2", "@#", "$#*$1", "@-1$1", "$-1*2", "if($1==0,0,$2/$1)", "vcount($1..$2)",
    "log10(abs($1)+1)", "idiv($1,3)", "trunc($1)", "ceil($2)",
]
FLAGS = ["", "", "", ";%.2f", ";%.1f", ";N", ";E", ";NE", ";f2", ";n4", ";%d", ";p20", ";s3", ";%.3f"]
FIELD = ["vsum(@I..@II)", "vmean(@2..@-1)", "vmax(@I..@II)", "vmin(@2..@-1)", "vmedian(@I..@II)",
         "@2$1+@3$1", "vsum(@I$1..@II$2)", "vsdev(@I..@II)", "@-1+@-2", "vsum(@I..@II)/vcount(@I..@II)"]


def table():
    ncol = rng.randint(2, 5)
    rows = rng.randint(2, 7)
    kinds = ["num"] * ncol
    special = rng.random()
    if special < 0.12:
        kinds[0] = kinds[1] = "dur"
    elif special < 0.2:
        kinds[0] = kinds[1] = "date"
    header = rng.random() < 0.7
    footer = header and rng.random() < 0.5
    lines = []
    if header:
        lines.append("| " + " | ".join(f"h{i + 1}" for i in range(ncol)) + " |")
        lines.append("|" + "+".join(["---"] * ncol) + "|")
    for _ in range(rows):
        lines.append("| " + " | ".join(cell(k) for k in kinds) + " |")
    if footer:
        lines.append("|" + "+".join(["---"] * ncol) + "|")
        lines.append("| " + " | ".join("" for _ in range(ncol)) + " |")
    eqs = []
    target = rng.randint(2, ncol) if ncol > 1 else 1
    if special < 0.12:
        e = rng.choice(["$2-$1", "$1+$2", "vsum($1..$2)", "$2-$1+0:30"])
        flag = rng.choice([";T", ";U", ";t", ";T", ""])
        eqs.append(f"${ncol if ncol > 2 else 2}={e}{flag}")
    elif special < 0.2:
        e = rng.choice(["$2-$1", "$1+7", "$1-30", "if($1<$2,1,0)", "$2-$1+0.5"])
        eqs.append(f"${ncol if ncol > 2 else 2}={e}")
    else:
        for _ in range(rng.randint(1, 2)):
            eqs.append(f"${target}={rng.choice(EXPRS)}{rng.choice(FLAGS)}")
            target = rng.randint(1, ncol)
    if footer:
        c = rng.randint(1, ncol)
        eqs.append(f"@>${c}={rng.choice(FIELD)}{rng.choice(FLAGS)}")
    # Distinct left-hand sides.
    seen, uniq = set(), []
    for e in eqs:
        lhs = e.split("=", 1)[0]
        if lhs not in seen:
            seen.add(lhs)
            uniq.append(e)
    return "\n".join(lines) + "\n#+TBLFM: " + "::".join(uniq) + "\n"


def main():
    name = f"random-{SEED}"
    org = os.path.join(ROOT, "tests/corpus/tables", name + ".org")
    with open(org, "w") as f:
        f.write(f"#+TITLE: Random tables (seed {SEED})\n\n")
        for i in range(COUNT):
            f.write(f"* Table {i + 1}\n\n{table()}\n")
    out = os.path.join(ROOT, "crates/org-table/tests/tables", name + ".expected")
    subprocess.run(["emacs", "-Q", "--batch", "-l", os.path.join(ROOT, "tests/emacs/table.el"), org, out],
                   check=True, stderr=subprocess.DEVNULL)
    print(f"wrote {COUNT} tables to {org}")


if __name__ == "__main__":
    main()
