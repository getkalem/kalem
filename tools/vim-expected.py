#!/usr/bin/env python3
"""Vim's results for the Vim cases (tests/vim/cases.json ->
tests/vim/expected.json).

Each case is a text and keys in Vim's notation (`<Esc>`, `<CR>`, `<BS>`,
`<Tab>`, `<C-w>`, `<lt>`); the cursor starts on the first line's first
non-blank, where Vim opens a file ('startofline').
`<sync>` ends an undo step: Vim reading keys from a script makes one undo
step of them all, where typed keys make one per command. Vim
runs without a vimrc (`vim -Nu NONE`), so with its own defaults, typing the
keys from a script (`-s`); the result is the text and the cursor's line and
column (bytes, from 1) after a final Escape. DEFAULTS are Kalem's own
settings, set first.

    tools/vim-expected.py            # every case
    tools/vim-expected.py --check    # fail if the recorded results differ
"""

import json
import os
import subprocess
import sys
import tempfile

# Kalem's defaults where Vim's own differ (`vim -u NONE` has an empty
# 'backspace' and no 'autoindent'); a case's "set" comes after them.
DEFAULTS = ["backspace=indent,eol,start", "autoindent", "shiftwidth=2", "expandtab",
            "softtabstop=-1", "nrformats=bin,hex", "nojoinspaces"]

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CASES = os.path.join(ROOT, "tests", "vim", "cases.json")
EXPECTED = os.path.join(ROOT, "tests", "vim", "expected.json")


def raw(keys):
    """`keys` in Vim's notation as the bytes typed."""
    out = []
    i = 0
    names = {"sync": ":let &ul=&ul\r", "esc": "\x1b", "cr": "\r", "enter": "\r", "bs": "\x08", "tab": "\t",
             "lt": "<", "space": " ", "nl": "\n", "del": "\x1b[3~"}
    while i < len(keys):
        c = keys[i]
        if c == "<":
            end = keys.find(">", i)
            name = keys[i + 1:end] if end > 0 else ""
            low = name.lower()
            if low in names:
                out.append(names[low])
                i = end + 1
                continue
            if low.startswith("c-") and len(name) == 3:
                ch = name[2]
                out.append(chr(ord(ch.upper()) & 0x1F) if ch.isalpha() else
                           {"[": "\x1b", "]": "\x1d", "^": "\x1e", "@": "\x00",
                            "\\": "\x1c", "_": "\x1f"}[ch])
                i = end + 1
                continue
        out.append(c)
        i += 1
    return "".join(out)


def run(case):
    with tempfile.TemporaryDirectory() as d:
        text = os.path.join(d, "t.txt")
        with open(text, "w", encoding="utf-8", newline="") as f:
            f.write(case["text"])
        result = os.path.join(d, "out.json")
        script = os.path.join(d, "keys")
        settings = "".join(f":set {s}\r" for s in DEFAULTS + case.get("set", []))
        tail = ("\x1b\x1b:call writefile([json_encode({'lines': getline(1, '$'),"
                " 'bytes': wordcount().bytes,"
                " 'line': line('.'), 'col': col('.')})], '" + result + "')\r:qa!\r")
        with open(script, "w", encoding="utf-8", newline="") as f:
            f.write(settings + raw(case["keys"]) + tail)
        subprocess.run(
            ["vim", "-Nu", "NONE", "-i", "NONE", "-n", "--not-a-term", "-s", script, text],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, timeout=20, check=False)
        with open(result, encoding="utf-8") as f:
            r = json.load(f)
        # The text as written: lines joined, the last with its line feed.
        # An empty buffer is no text, not one empty line.
        text = "" if r["bytes"] == 0 else "\n".join(r["lines"]) + "\n"
        return {"text": text, "line": r["line"], "col": r["col"]}


def main():
    with open(CASES, encoding="utf-8") as f:
        cases = json.load(f)
    expected = {c["name"]: run(c) for c in cases}
    if "--check" in sys.argv:
        with open(EXPECTED, encoding="utf-8") as f:
            recorded = json.load(f)
        differ = [n for n in expected if recorded.get(n) != expected[n]]
        if differ:
            print("recorded results differ from Vim's:", differ)
            sys.exit(1)
        print(f"{len(expected)} cases agree with Vim")
        return
    with open(EXPECTED, "w", encoding="utf-8") as f:
        json.dump(expected, f, indent=1, ensure_ascii=False, sort_keys=True)
        f.write("\n")
    print(f"Vim results -> tests/vim/expected.json ({len(expected)} cases)")


if __name__ == "__main__":
    main()
