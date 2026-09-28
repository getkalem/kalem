#!/usr/bin/env python3
"""Measures the performance targets of design §15 that a script can:
start-up and memory of both editors, binary sizes, the command-line tools
on a 1 MB file, opening a 10 MB document and a 100 MB plain text file.
Keystroke latency and saving are
`cargo test --release -p kalem-tui --test latency -- --ignored`, the
incremental parse `cargo bench -p org-syntax --bench parse`.

Usage: tools/bench-phase1.py  (from the repository; builds release
binaries; the graphical runs open a window for a moment)."""

import os, pty, resource, select, statistics, struct, subprocess, sys, tempfile, termios, fcntl, time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
os.chdir(ROOT)
FULL = "target/release/kalem"
TERM_ONLY = "target/terminal/release/kalem"
RUNS = 3


def build():
    subprocess.run(["cargo", "build", "--release", "-q", "-p", "kalem-editor"], check=True)
    subprocess.run(["cargo", "build", "--release", "-q", "-p", "kalem-editor", "--no-default-features",
                    "--features", "tui", "--target-dir", "target/terminal"], check=True)


def files(tmp):
    manual = open("tests/corpus/org-mode/org-manual.org").read()
    def sized(n):
        text = ""
        while len(text) < n:
            text += manual
        return text[: text.rfind("\n", 0, n) + 1]
    paths = {}
    for name, n in [("1mb", 1 << 20), ("10mb", 10 << 20)]:
        p = os.path.join(tmp, f"{name}.org")
        open(p, "w").write(sized(n))
        paths[name] = p
    # Plain text: the manual as a `.txt` file, 100 MB.
    p = os.path.join(tmp, "100mb.txt")
    chunk = sized(10 << 20)
    with open(p, "w") as out:
        for _ in range(10):
            out.write(chunk)
    paths["100mb"] = p
    return paths


def wait(pid):
    _, status, usage = os.wait4(pid, 0)
    # ru_maxrss is in bytes on macOS, kilobytes on Linux.
    rss = usage.ru_maxrss if sys.platform == "darwin" else usage.ru_maxrss * 1024
    return status, rss


def gui(args):
    env = dict(os.environ, KALEM_EXIT_AFTER_START="1")
    t = time.perf_counter()
    p = subprocess.Popen([FULL, "gui", *args], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    _, rss = wait(p.pid)
    return time.perf_counter() - t, rss


def tui(binary, args):
    t = time.perf_counter()
    pid, fd = pty.fork()
    if pid == 0:
        os.environ["TERM"] = "xterm-256color"
        os.environ["KALEM_EXIT_AFTER_START"] = "1"
        os.execv(binary, [binary, "tui", *args])
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
    while True:
        r, _, _ = select.select([fd], [], [], 5)
        if not r:
            break
        try:
            data = os.read(fd, 65536)
        except OSError:
            break
        if not data:
            break
        # A terminal answers the capability query (DA1) at once.
        if b"\x1b[c" in data:
            os.write(fd, b"\x1b[?62;c")
    _, rss = wait(pid)
    return time.perf_counter() - t, rss


def cli(args):
    t = time.perf_counter()
    subprocess.run([FULL, *args], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return time.perf_counter() - t


def median(f):
    runs = [f() for _ in range(RUNS)]
    if isinstance(runs[0], tuple):
        return tuple(statistics.median(r[i] for r in runs) for i in range(len(runs[0])))
    return statistics.median(runs)


def main():
    build()
    tmp = tempfile.mkdtemp(prefix="kalem-bench-")
    f = files(tmp)
    ms = lambda s: f"{s * 1000:.0f} ms"
    mb = lambda b: f"{b / (1 << 20):.0f} MB"
    rows = []
    t, rss = median(lambda: gui([]))
    rows.append(("Cold start, empty document (graphical, to first frame)", ms(t), "under 300 ms"))
    rows.append(("Memory, empty document (graphical)", mb(rss), "under 80 MB"))
    t, _ = median(lambda: gui([f["1mb"]]))
    rows.append(("Opening a 1 MB document (graphical, to first frame)", ms(t), "under 200 ms"))
    t, rss = median(lambda: gui([f["10mb"]]))
    rows.append(("10 MB document, until interactive (graphical)", ms(t), "under 1 s"))
    rows.append(("Memory, 10 MB document (graphical)", mb(rss), "under 500 MB"))
    t, _ = median(lambda: tui(TERM_ONLY, []))
    rows.append(("Terminal frontend startup (terminal-only build)", ms(t), "under 50 ms"))
    t, _ = median(lambda: tui(TERM_ONLY, [f["10mb"]]))
    rows.append(("10 MB document, until interactive (terminal)", ms(t), "under 1 s"))
    t, _ = median(lambda: gui([f["100mb"]]))
    rows.append(("100 MB plain text file, until interactive (graphical)", ms(t), "under 1 s"))
    t, _ = median(lambda: tui(TERM_ONLY, [f["100mb"]]))
    rows.append(("100 MB plain text file, until interactive (terminal)", ms(t), "under 1 s"))
    rows.append(("CLI `check` on a 1 MB file", ms(median(lambda: cli(["check", f["1mb"]]))), "under 100 ms"))
    rows.append(("CLI `fmt --check` on a 1 MB file", ms(median(lambda: cli(["fmt", "--check", f["1mb"]]))), "under 100 ms"))
    rows.append(("Binary size (full)", mb(os.path.getsize(FULL)), "under 40 MB"))
    rows.append(("Binary size (terminal-only)", mb(os.path.getsize(TERM_ONLY)), "under 15 MB"))
    print("| Metric | Measured | Target |")
    print("|---|---|---|")
    for r in rows:
        print(f"| {r[0]} | {r[1]} | {r[2]} |")


if __name__ == "__main__":
    main()
