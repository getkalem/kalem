#!/usr/bin/env python3
"""The seams of kalem-core (roadmap R3.7): which modules can leave it.

Reads crates/kalem-core/src and builds the graph of its top-level modules:
an edge from A to B for each `crate::B` path in A outside its tests
(grouped `use crate::{…}` included), and for each call of a method that
B's own `impl DocumentState` block defines (`.dired()`, `.viewer_mut()`:
paths miss these). Prints the modules in the largest cycle, the ones
above it (they use it, nothing in it uses them: they can be a crate
above the core as they are) and below it (the core uses them, they use
nothing of it), and for each candidate group the references into it
from the rest and out of it: what a split must invert.

    python3 tools/core-seams.py [--groups latex=latex_*,synctex ...]

A group is NAME=module,module,…; a module may end in `*` (a prefix).
"""

import argparse
import collections
import os
import re
import sys

CORE = os.path.join(os.path.dirname(__file__), "..", "crates", "kalem-core", "src")
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
DEFAULT_GROUPS = {
    "dired": ["dired"],
    "latex": ["latex_*", "synctex", "tex_pictures", "siunitx"],
    "csv": ["csv", "csv_tools"],
    "viewer+sheet": [
        "viewer",
        "sheet_chart",
        "sheet_print",
        "workbook_io",
        "formulas",
        "formula_edit",
        "flash_fill",
        "chart_math",
    ],
    "markdown": ["markdown", "markdown_table", "markdown_org"],
    "flow": ["flow"],
    "lsp": ["lsp"],
}


def strip_comments(s):
    """`s` without its comments and string literals."""
    out, i, n = [], 0, len(s)
    while i < n:
        if s.startswith("//", i):
            j = s.find("\n", i)
            i = n if j < 0 else j
        elif s.startswith("/*", i):
            j = s.find("*/", i + 2)
            i = n if j < 0 else j + 2
        elif s[i] == '"':
            j = i + 1
            while j < n and s[j] != '"':
                j += 2 if s[j] == "\\" else 1
            out.append('""')
            i = j + 1
        else:
            out.append(s[i])
            i += 1
    return "".join(out)


def brace_items(s, i):
    """The first identifiers of a `{a, b::c, d::{e}}` group starting at `i`."""
    depth, items, expect, j = 0, [], True, i
    while j < len(s):
        c = s[j]
        if c == "{":
            depth += 1
            expect = depth == 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return items
        elif c == "," and depth == 1:
            expect = True
        elif depth == 1 and expect:
            m = IDENT.match(s, j)
            if m:
                items.append(m.group(0))
                expect = False
                j = m.end()
                continue
        j += 1
    return items


def paths(text):
    """The first segment after each `crate::` in `text`."""
    res = []
    for m in re.finditer(r"\bcrate::", text):
        k = m.end()
        if k < len(text) and text[k] == "{":
            res.extend(brace_items(text, k))
        else:
            mm = IDENT.match(text, k)
            if mm:
                res.append(mm.group(0))
    return res


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--groups", nargs="*", default=[], help="NAME=module,module,… (a module may end in *)")
    args = ap.parse_args()
    groups = dict(DEFAULT_GROUPS)
    for g in args.groups:
        name, mods = g.split("=", 1)
        groups[name] = mods.split(",")

    files = {}
    for dp, _, fs in os.walk(CORE):
        for f in fs:
            if f.endswith(".rs"):
                p = os.path.join(dp, f)
                files[os.path.relpath(p, CORE)] = open(p, encoding="utf-8").read()

    def module(rel):
        first = rel.split(os.sep)[0]
        return first[:-3] if first.endswith(".rs") else first

    def main_part(text):
        m = re.search(r"#\[cfg\(test\)\]\s*mod\s+tests\s*\{", text)
        return text[: m.start()] if m else text

    mods = {module(r) for r in files} - {"lib"}
    lines = collections.Counter()
    for rel, t in files.items():
        lines[module(rel)] += t.count("\n")

    # The methods each module's `impl DocumentState` block adds, by name,
    # where the name is defined once in the crate (so a call is its).
    fn_count = collections.Counter(m.group(1) for t in files.values() for m in re.finditer(r"\bfn\s+(\w+)", t))
    methods = {}
    for rel, t in files.items():
        if rel == "document.rs":
            continue
        for m in re.finditer(r"^impl\s+(?:crate::)?(?:document::)?DocumentState\s*\{", t, re.M):
            i, depth = m.end(), 1
            while depth and i < len(t):
                depth += {"{": 1, "}": -1}.get(t[i], 0)
                i += 1
            for fn in re.findall(r"^\s{4}pub(?:\([a-z]+\))?\s+fn\s+(\w+)", t[m.end() : i], re.M):
                if fn_count[fn] == 1:
                    methods[fn] = module(rel)

    edges = collections.defaultdict(collections.Counter)
    for rel, text in files.items():
        a = module(rel)
        if a == "lib":
            continue
        t = main_part(strip_comments(text))
        for b in paths(t):
            if b in mods and b != a:
                edges[a][b] += 1
        for fn, owner in methods.items():
            if owner != a:
                n = len(re.findall(r"\." + fn + r"\s*\(", t))
                if n:
                    edges[a][owner] += n

    def reach(src):
        seen, stack = set(), [src]
        while stack:
            for w in edges[stack.pop()]:
                if w not in seen:
                    seen.add(w)
                    stack.append(w)
        return seen

    reaches = {m: reach(m) for m in mods}
    hub = "document"
    cycle = {m for m in mods if m == hub or (hub in reaches[m] and m in reaches[hub])}
    above = sorted((m for m in mods - cycle if reaches[m] & cycle), key=lambda m: -lines[m])
    below = sorted((m for m in mods - cycle if any(m in reaches[c] for c in cycle)), key=lambda m: -lines[m])
    rest = sorted(mods - cycle - set(above) - set(below))

    total = sum(lines.values())
    print(f"kalem-core: {len(mods)} modules, {total:,} lines")
    print(f"\nin the cycle through {hub}.rs: {len(cycle)} modules, {sum(lines[m] for m in cycle):,} lines")
    print("  " + ", ".join(sorted(cycle)))
    print(f"\nabove it (a crate above the core as they are): {len(above)} modules, {sum(lines[m] for m in above):,} lines")
    for m in above:
        out = sum(n for b, n in edges[m].items() if b in cycle)
        print(f"  {m:16} {lines[m]:6} lines, {out} references into the cycle")
    print(f"\nbelow it (the core uses them, they use nothing of it): {len(below)} modules, {sum(lines[m] for m in below):,} lines")
    print("  " + ", ".join(below))
    if rest:
        print(f"\nneither: {', '.join(rest)}")

    print("\ngroups: references into the group from the rest of the core (what a split inverts), and out of it")
    for name, pats in groups.items():
        members = {m for m in mods if any(m == p or (p.endswith("*") and m.startswith(p[:-1])) for p in pats)}
        if not members:
            continue
        into, out = collections.Counter(), collections.Counter()
        for a, bs in edges.items():
            for b, n in bs.items():
                if a not in members and b in members:
                    into[a] += n
                if a in members and b not in members:
                    out[b] += n
        print(
            f"  {name:13} {sum(lines[m] for m in members):6} lines  in {sum(into.values()):3} from {len(into):2} modules"
            f"  out {sum(out.values()):3} to {len(out):2}"
        )
        print("     in: " + ", ".join(f"{a} {n}" for a, n in into.most_common(8)))


if __name__ == "__main__":
    sys.exit(main())
