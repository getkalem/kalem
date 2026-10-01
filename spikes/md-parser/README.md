# Markdown parser spike for D19 (T2.7c.1)

Two candidate parsers for Markdown mode, measured on the GitHub Flavored
Markdown specification (`cmark-gfm`'s `test/spec.txt`, version 0.29-gfm:
CommonMark's 648 examples and 24 of GitHub's extensions). Throwaway code,
not part of the workspace; it informs the owner's decision D19 and decides
nothing itself.

```sh
mkdir -p data && curl -sSo data/gfm-spec.txt \
  https://raw.githubusercontent.com/github/cmark-gfm/master/test/spec.txt
cargo run --release
```

## Results (2026-10-01, a 4-core Xeon at 2.8 GHz)

| | pulldown-cmark 0.13.4 | comrak 0.39.1 |
|---|---|---|
| CommonMark examples, HTML as the specification | 639 / 648 | 639 / 648 |
| GFM extension examples | 12 / 24 | 24 / 24 |
| Byte ranges | every event (4,887), none outside the text or its parent | 2,855 nodes; 4 without a position, 4 outside the text or their parent (empty table cells, fenced code in a list item) |
| Positions given as | byte ranges | line and column (converted with a line table) |
| 10 MB, a full parse | 0.39 s | 1.47 s |
| Tree | a flat event stream with ranges | an arena AST |
| License | MIT | BSD-2-Clause |

The nine CommonMark examples both miss are the same: runs of `**` and
`__` (`****foo****`), where both follow CommonMark 0.31's rule for nested
strong emphasis and the 0.29 suite expects the older one. They are the
suite's age, not a defect of either.

pulldown-cmark's twelve missing extension examples: GitHub's extended
autolinks (`www.example.com` and bare URLs, 11), which it does not
implement, and the tag filter (`<title>`, `<style>` escaped, 1), which
matters only for HTML output.

What the comparison means for Markdown mode (design 2.6.1, 11.11):

- **Ranges.** The mode contract wants a range for every block and inline
  node; pulldown-cmark gives exactly that, comrak gives line and column
  with a few positions missing or wrong, which would need correcting.
- **Speed.** Reparsing from the enclosing top-level block keeps a
  keystroke small with either; a full parse of a large file is four times
  faster with pulldown-cmark.
- **GFM.** comrak covers all of it; with pulldown-cmark, Kalem would add
  the extended autolinks itself (a scan of text runs, as Org's plain links
  are found) and filter tags only in its HTML export.
- **Link reference definitions** are global in both: a block reparsed on
  its own resolves `[foo]` against the document's definitions through the
  broken-link callback both parsers offer.

The design document's recommendation (pulldown-cmark, D19) holds on this
data; the extended autolinks are the cost. The choice is the owner's.
