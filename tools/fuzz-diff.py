#!/usr/bin/env python3
"""Differential fuzzing: mutate corpus files, parse the mutants with Emacs
and Kalem, and compare the trees.

Usage: tools/fuzz-diff.py [COUNT] [SEED]

Mutants are written to .cache/fuzz/<seed>/. Files whose trees differ are
listed at the end; `kalem diff-emacs --emacs-dumps .cache/fuzz/<seed>/dumps FILE`
shows the details.
"""
import os, random, subprocess, sys, glob, shutil

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
COUNT = int(sys.argv[1]) if len(sys.argv) > 1 else 500
SEED = int(sys.argv[2]) if len(sys.argv) > 2 else 1

SNIPPETS = ["*", "* ", "** ", "\n", "\n\n", "#+", "#+BEGIN_SRC", "#+END_SRC", "#+begin_quote\n", "#+end_quote\n",
            "[[", "]]", "][", "|", "|-", "- ", "1. ", "+ ", ":", "::", "<", ">", "[", "]", "=", "~", "/", "+", "_",
            "^", "{", "}", "\\", "$", "$$", "@@", "[fn:", "[fn::", "<<", ">>", "<<<", ">>>", "\t", " ", ":END:\n",
            ":PROPERTIES:\n", "#+TBLFM: ", "src_", "call_", "[cite:@", ";", "\\begin{x}", "\\end{x}",
            "SCHEDULED: <2026-01-01 Thu>", "CLOCK: [2026-01-01 Thu 10:00]", "[X] ", "[@3] ", "#+NAME: x\n",
            "#+CAPTION: c\n", "%%(", "-----", ": ", "# ", "{{{m(a)}}}", "\\\\", "ç", "ı", "中", "é", " ",
            "#+RESULTS:\n", "[fn:1] ", "*************** ", "END", "https://x.org", "<2026-01-01>", "\\alpha", "x_1"]

def mutate(s, rng):
    for _ in range(1 + rng.randrange(4)):
        if not s:
            s = rng.choice(SNIPPETS); continue
        a = rng.randrange(len(s))
        op = rng.randrange(3)
        if op == 0:
            s = s[:a] + rng.choice(SNIPPETS) + s[a:]
        elif op == 1:
            s = s[:a] + s[a + rng.randrange(40):]
        else:
            b = min(len(s), a + rng.randrange(200))
            c = rng.randrange(len(s))
            s = s[:c] + s[a:b] + s[c:]
    return s

def main():
    rng = random.Random(SEED)
    sources = sorted(glob.glob(os.path.join(ROOT, "tests/corpus/**/*.org"), recursive=True))
    texts = [open(f, encoding="utf-8").read() for f in sources]
    out = os.path.join(ROOT, ".cache/fuzz", str(SEED))
    shutil.rmtree(out, ignore_errors=True)
    os.makedirs(out)
    files = []
    for i in range(COUNT):
        t = rng.choice(texts)
        if len(t) > 3000:
            a = rng.randrange(len(t) - 3000)
            t = t[a:a + 3000]
        f = os.path.join(out, f"m{i:05d}.org")
        with open(f, "w", encoding="utf-8", newline="") as fh:
            fh.write(mutate(t, rng))
        files.append(f)
    dumps = os.path.join(out, "dumps")
    for i in range(0, len(files), 200):
        chunk = files[i:i + 200]
        subprocess.run(["emacs", "-Q", "--batch", "-l", os.path.join(ROOT, "tests/emacs/dump.el"),
                        "--batch-dir", dumps] + chunk, stderr=subprocess.DEVNULL, check=False)
        # dump.el numbers its outputs; name them after the files for
        # `kalem diff-emacs --emacs-dumps`.
        for j, f in enumerate(chunk):
            numbered = os.path.join(dumps, f"{j:05d}.json")
            if os.path.exists(numbered):
                os.replace(numbered, os.path.join(dumps, os.path.basename(f) + ".json"))
    kalem = os.path.join(ROOT, "target/release/kalem")
    r = subprocess.run([kalem, "diff-emacs", "--emacs-dumps", dumps, "--show", "0"] + files,
                       capture_output=True, text=True)
    bad = [l.split()[1] for l in r.stdout.splitlines() if l.startswith("== ")]
    print("\n".join(l for l in r.stdout.splitlines() if l.startswith(("TOTAL", "files identical", "known"))))
    for b in bad[:30]:
        print("differs:", b)

if __name__ == "__main__":
    main()
