#!/usr/bin/env python3
"""A stratified sample of arXiv's LaTeX sources (T2.7h.1, T2.7h.30).

    tools/arxiv-sample.py OUT [--per-field 200] [--year 2024]

For each field (mathematics, physics, computer science, biology,
economics) the papers of a few categories submitted in YEAR are listed
through arXiv's API, an even share of each category taken in a fixed
order (so the sample is the same every run), and each paper's source
(`https://arxiv.org/e-print/ID`) unpacked into OUT/FIELD/ID/. Papers
without a LaTeX source (PDF only) are skipped and others taken instead.
Only `.tex`, `.sty`, `.cls`, `.bib` and `.bbl` files are kept. The
sources stay out of the repository: most are not redistributable.

arXiv asks for one request every three seconds; the script keeps to it,
so a thousand papers take about an hour (a CI job, `.github/workflows/
arxiv.yml`; the development containers cannot reach arxiv.org).
"""

import argparse
import gzip
import io
import os
import sys
import tarfile
import time
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET

FIELDS = {
    "mathematics": ["math.AG", "math.PR", "math.CO", "math.AP", "math.NT"],
    "physics": ["hep-th", "cond-mat.str-el", "astro-ph.GA", "quant-ph", "physics.optics"],
    "computer-science": ["cs.LG", "cs.CL", "cs.DS", "cs.CR", "cs.CV"],
    "biology": ["q-bio.NC", "q-bio.PE", "q-bio.QM", "q-bio.BM", "q-bio.GN"],
    "economics": ["econ.EM", "econ.TH", "econ.GN"],
}
KEEP = (".tex", ".sty", ".cls", ".bib", ".bbl")
AGENT = "kalem-coverage/1.0 (https://github.com/getkalem/kalem)"
DELAY = 3.1
last = 0.0
# Where listings are remembered (set from the output folder).
LISTINGS = None


def get(url):
    """The body at `url`; on 429 and 5xx, waits (Retry-After, else a
    minute, doubling) and asks again, up to five times."""
    global last
    pause = 60
    for attempt in range(6):
        wait = last + DELAY - time.time()
        if wait > 0:
            time.sleep(wait)
        last = time.time()
        req = urllib.request.Request(url, headers={"User-Agent": AGENT})
        try:
            with urllib.request.urlopen(req, timeout=120) as r:
                return r.read()
        except urllib.error.HTTPError as e:
            if attempt == 5 or not (e.code == 429 or e.code >= 500):
                raise
            after = e.headers.get("Retry-After", "")
            time.sleep(int(after) if after.isdigit() else pause)
            pause *= 2
        except (urllib.error.URLError, TimeoutError):
            if attempt == 5:
                raise
            time.sleep(pause)
            pause *= 2
    raise RuntimeError("unreachable")


def listing(cat, year, n):
    """IDs of papers of `cat` submitted in `year`, the first `n` by date;
    remembered in `LISTINGS` (beside the sample, which CI caches), so that
    a later run asks arXiv's API, which throttles hard, only once."""
    cached = os.path.join(LISTINGS, f"{cat}-{year}-{n}.txt") if LISTINGS else None
    if cached and os.path.exists(cached):
        return open(cached).read().split()
    q = f"cat:{cat} AND submittedDate:[{year}01010000 TO {year}12312359]"
    url = "https://export.arxiv.org/api/query?" + urllib.parse.urlencode(
        {"search_query": q, "start": 0, "max_results": n,
         "sortBy": "submittedDate", "sortOrder": "ascending"})
    feed = ET.fromstring(get(url))
    ns = {"a": "http://www.w3.org/2005/Atom"}
    ids = []
    for e in feed.findall("a:entry", ns):
        i = e.find("a:id", ns).text.rsplit("/abs/", 1)[-1]
        ids.append(i.rsplit("v", 1)[0] if "v" in i.split(".")[-1] else i)
    if cached and ids:
        with open(cached, "w") as f:
            f.write("\n".join(ids) + "\n")
    return ids


def unpack(data, dest):
    """Writes the LaTeX files of an e-print; False when it has none."""
    try:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:*") as t:
            kept = 0
            for m in t.getmembers():
                if not m.isfile() or not m.name.lower().endswith(KEEP):
                    continue
                name = os.path.normpath(m.name).lstrip("/")
                if name.startswith(".."):
                    continue
                f = t.extractfile(m)
                if f is None:
                    continue
                path = os.path.join(dest, name)
                os.makedirs(os.path.dirname(path), exist_ok=True)
                with open(path, "wb") as out:
                    out.write(f.read())
                kept += 1
            return kept > 0
    except tarfile.TarError:
        pass
    # A single gzipped file: a .tex, or a PDF.
    try:
        raw = gzip.decompress(data)
    except OSError:
        raw = data
    if raw.startswith(b"%PDF") or b"\\" not in raw[:20000]:
        return False
    os.makedirs(dest, exist_ok=True)
    with open(os.path.join(dest, "main.tex"), "wb") as out:
        out.write(raw)
    return True


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--per-field", type=int, default=200)
    ap.add_argument("--year", default="2024")
    ap.add_argument("--complete", action="store_true",
                    help="ask for the missing papers of a field that is nearly whole")
    a = ap.parse_args()
    # Papers without a LaTeX source, remembered beside the sample so that a
    # later run does not ask arXiv for them again.
    os.makedirs(a.out, exist_ok=True)
    global LISTINGS
    LISTINGS = os.path.join(a.out, "listings")
    os.makedirs(LISTINGS, exist_ok=True)
    skip_file = os.path.join(a.out, "no-latex.txt")
    try:
        no_latex = set(open(skip_file).read().split())
    except OSError:
        no_latex = set()
    for field, cats in FIELDS.items():
        want = a.per_field
        share = -(-want // len(cats))
        got = 0
        # A field whose sample is nine tenths there already is not asked
        # for more: its last papers are those arXiv has no LaTeX of or
        # throttles, and asking for them again held runs up for an hour.
        have = os.path.join(a.out, field)
        held = len(os.listdir(have)) if os.path.isdir(have) else 0
        if held * 10 >= want * 9 and not a.complete:
            print(f"{field}: {held} papers (kept)", flush=True)
            continue
        for cat in cats:
            if got >= want:
                break
            # More than the share, for the PDF-only ones.
            try:
                ids = listing(cat, a.year, share * 2)
            except Exception as e:  # noqa: BLE001 (one category's error)
                print(f"{field} {cat}: listing failed: {e}", file=sys.stderr)
                continue
            taken = 0
            for i in ids:
                if taken >= share or got >= want:
                    break
                dest = os.path.join(a.out, field, i.replace("/", "_"))
                if os.path.isdir(dest):
                    taken += 1
                    got += 1
                    continue
                if i in no_latex:
                    continue
                try:
                    ok = unpack(get(f"https://arxiv.org/e-print/{i}"), dest)
                except Exception as e:  # noqa: BLE001 (one paper's error)
                    print(f"{i}: {e}", file=sys.stderr)
                    continue
                if ok:
                    taken += 1
                    got += 1
                else:
                    no_latex.add(i)
                    with open(skip_file, "a") as f:
                        f.write(i + "\n")
            print(f"{field} {cat}: {taken}", flush=True)
        print(f"{field}: {got} papers", flush=True)


if __name__ == "__main__":
    main()
