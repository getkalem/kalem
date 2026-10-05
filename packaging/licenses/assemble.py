"""THIRD-PARTY-LICENSES.md from the parts tools/third-party-licenses.sh
gathers in the folder it names: cargo-about's JSON for Kalem's crates
and for each plugin built in (crates-*.json), the highlighter's syntax
acknowledgements (syntaxes.md) and `cargo metadata` (metadata.json),
through which the licence files of the works crates embed are found.
"""

import json
import pathlib
import re
import sys

here = pathlib.Path(__file__).parent
tmp = pathlib.Path(sys.argv[1])
out = []
w = out.append


def fence(text):
    """`text` in a code fence longer than any run of backticks in it."""
    run = max((len(r) for r in re.findall(r"`+", text)), default=0)
    ticks = "`" * max(3, run + 1)
    return f"{ticks}text\n{text.rstrip()}\n{ticks}\n"


# The crates: one section a licence text, the crates under it listed.
licences = {}
for f in sorted(tmp.glob("crates-*.json")):
    for lic in json.loads(f.read_text())["licenses"]:
        key = (lic["id"], lic["text"].strip())
        entry = licences.setdefault(key, {"name": lic["name"], "crates": set()})
        for u in lic["used_by"]:
            entry["crates"].add(f"{u['crate']['name']} {u['crate']['version']}")
plugins = sorted(f.stem.removeprefix("crates-") for f in tmp.glob("crates-*.json") if f.stem != "crates-kalem")

w("# Third-party licences\n")
w(
    "Kalem is under the MIT license or the Apache License 2.0, at your "
    "choice. The program is built from the Rust crates below, its own "
    "and those of the plugins it has built in ("
    + ", ".join(f"`{p}`" for p in plugins)
    + "), each under the licence named; it also embeds the syntax "
    "definitions, fonts, colour profiles, character maps, styles and "
    "tables listed after them. Made by `tools/third-party-licenses.sh`.\n"
)
w("## Rust crates\n")
for (lid, text), e in sorted(licences.items(), key=lambda kv: (kv[1]["name"], sorted(kv[1]["crates"]))):
    crates = sorted(e["crates"], key=str.lower)
    w(f"### {e['name']}\n")
    w("Used by: " + ", ".join(crates) + ".\n")
    w(fence(text))

# The syntaxes and themes, as two-face lists them.
w("## Syntax definitions and themes\n")
w(
    "The highlighter embeds the syntax definitions and themes `bat` "
    "curates, through the `two-face` crate; their licences:\n"
)
for line in (tmp / "syntaxes.md").read_text().splitlines():
    w("##" + line if line.startswith("#") else line)
w("")

# The works crates embed that are not code, and Kalem's own tables.
meta = json.loads((tmp / "metadata.json").read_text())
dirs = {p["name"]: pathlib.Path(p["manifest_path"]).parent for p in meta["packages"]}


def file_of(crate, rel):
    return (dirs[crate] / rel).read_text()


w("## Other works\n")
w("### KaTeX's fonts\n")
w(
    "The formula renderer (the `ratex-katex-fonts` crate) embeds KaTeX's "
    "fonts, under the SIL Open Font License 1.1.\n"
)
w(fence(file_of("ratex-katex-fonts", "fonts/FONT_NOTICE.txt")))
w(fence(file_of("ratex-katex-fonts", "fonts/OFL.txt")))
w("### PDFium's base-14 fonts\n")
w(
    "The PDF reader (the `hayro-interpret` crate, in Kalem and in the PDF "
    "viewer) embeds the Foxit fonts PDFium carries for PDF's standard "
    "fonts, under PDFium's licence.\n"
)
w(fence(file_of("hayro-interpret", "assets/LICENSE_FOXIT")))
w("### Colour profiles\n")
w(
    "`hayro-interpret` embeds a compact CMYK profile, "
    "`CGATS001Compat-v2-micro.icc` from saucecontrol's Compact-ICC-Profiles "
    "(CC0 1.0), and a Lab profile its authors generated with Little CMS.\n"
)
w("### Adobe's character maps\n")
w(
    "The `hayro-cmap` crate embeds Adobe's CMap resources, under Adobe's "
    "licence.\n"
)
w(fence(file_of("hayro-cmap", "assets/LICENSE.txt")))
w("### Citation styles\n")
w(
    "The citation styles and locales of the Citation Style Language "
    "project, which the `hayagriva` crate embeds, are under the Creative "
    "Commons Attribution-ShareAlike 3.0 license "
    "(https://creativecommons.org/licenses/by-sa/3.0/); each style names "
    "its authors (https://github.com/citation-style-language/styles).\n"
)
w("### Vim's digraphs\n")
w(
    "The digraphs of the Vim keys (`crates/kalem-core/src/vim/digraphs.txt`) "
    "are Vim's default table, as `:digraphs` lists it, under Vim's licence.\n"
)
w(fence((here / "vim-LICENSE.txt").read_text()))
w("### Org's entities\n")
w(
    "The table of Org's entities (`crates/org-syntax/src/tables/entities.rs`: "
    "`\\alpha`, `\\nbsp` and 412 more, with their LaTeX, HTML, ASCII, "
    "Latin-1 and UTF-8 forms) is generated from `org-entities.el`, part of "
    "GNU Emacs, under the GNU General Public License, version 3 or later "
    "(https://www.gnu.org/licenses/gpl-3.0.html). Decision D18 of the "
    "Kalem Book is about it.\n"
)

print("\n".join(out).replace("\r\n", "\n").replace("\r", "\n"))
