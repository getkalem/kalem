#!/usr/bin/env python3
"""The licence of each paper of the arXiv sample (T2.7h.30), from arXiv's
OAI-PMH metadata: the papers under a licence that lets their sources be
committed to the corpus (CC BY, CC BY-SA, CC0) are the ones
`tests/corpus/latex/arxiv` may hold.

Usage: tools/arxiv-licences.py SAMPLE > licences.tsv

SAMPLE is the folder `tools/arxiv-sample.py` fills (`FIELD/ID/...`).
Prints `field<TAB>id<TAB>licence URL` for every paper, `none` when arXiv
gives the default licence (no redistribution), and a summary on stderr.
arXiv asks for a pause between requests: three seconds."""

import os
import re
import sys
import time
import urllib.request

OAI = "https://export.arxiv.org/oai2?verb=GetRecord&metadataPrefix=arXiv&identifier=oai:arXiv.org:"
FREE = (
    "creativecommons.org/licenses/by/",
    "creativecommons.org/licenses/by-sa/",
    "creativecommons.org/publicdomain/zero/",
)


def licence(paper):
    for attempt in range(4):
        try:
            with urllib.request.urlopen(OAI + paper, timeout=60) as r:
                xml = r.read().decode("utf-8", "replace")
            m = re.search(r"<license>([^<]*)</license>", xml)
            return m.group(1).strip() if m else "none"
        except Exception as e:  # arXiv's 503 asks to wait
            print(f"{paper}: {e}", file=sys.stderr)
            time.sleep(10 * (attempt + 1))
    return "unknown"


def main():
    sample = sys.argv[1]
    free = 0
    total = 0
    for field in sorted(os.listdir(sample)):
        d = os.path.join(sample, field)
        if not os.path.isdir(d) or field == "listings":
            continue
        for paper in sorted(os.listdir(d)):
            if not os.path.isdir(os.path.join(d, paper)):
                continue
            # Old-style identifiers are stored with `_` for `/`.
            lic = licence(paper.replace("_", "/"))
            total += 1
            if any(f in lic for f in FREE):
                free += 1
            print(f"{field}\t{paper}\t{lic}", flush=True)
            time.sleep(3)
    print(f"{free} of {total} papers under CC BY, CC BY-SA or CC0", file=sys.stderr)


if __name__ == "__main__":
    main()
