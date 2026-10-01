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


def get(url):
    global last
    wait = last + DELAY - time.time()
    if wait > 0:
        time.sleep(wait)
    last = time.time()
    req = urllib.request.Request(url, headers={"User-Agent": AGENT})
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()


def listing(cat, year, n):
    """IDs of papers of `cat` submitted in `year`, the first `n` by date."""
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
    a = ap.parse_args()
    for field, cats in FIELDS.items():
        want = a.per_field
        share = -(-want // len(cats))
        got = 0
        for cat in cats:
            if got >= want:
                break
            # More than the share, for the PDF-only ones.
            ids = listing(cat, a.year, share * 2)
            taken = 0
            for i in ids:
                if taken >= share or got >= want:
                    break
                dest = os.path.join(a.out, field, i.replace("/", "_"))
                if os.path.isdir(dest):
                    taken += 1
                    got += 1
                    continue
                try:
                    ok = unpack(get(f"https://arxiv.org/e-print/{i}"), dest)
                except Exception as e:  # noqa: BLE001 (one paper's error)
                    print(f"{i}: {e}", file=sys.stderr)
                    continue
                if ok:
                    taken += 1
                    got += 1
            print(f"{field} {cat}: {taken}", flush=True)
        print(f"{field}: {got} papers", flush=True)


if __name__ == "__main__":
    main()
