# org-model

The document model of [Org mode](https://orgmode.org) files, computed from an
[`org-syntax`](../org-syntax) tree the way Emacs computes it.

`org-model` is part of the [Kalem](https://github.com/getkalem/kalem)
editor.

- **Outline:** headlines and inlinetasks with levels, ranges, TODO states,
  priorities and tags.
- **Tags:** local and inherited, `#+FILETAGS`, exclusion settings.
- **Properties:** drawers, `#+PROPERTY`, `KEY+` accumulation, selective and
  full inheritance, special properties (`ITEM`, `ALLTAGS`, `TIMESTAMP`, ...),
  categories and allowed values (`_ALL`).
- **Match strings:** `+work-boss|TODO="NEXT"`, `LEVEL>1/!`, `{regexp}`,
  `SCHEDULED<"<today>"`, as `org-make-tags-matcher` reads them.
- **Links and names:** where fuzzy, custom-id, coderef, radio and `id:` links
  lead (`org-link-search`); `#+NAME` lookups; footnote definitions.
- **Statistics cookies** (`[2/5]`, `[40%]`) and **clock totals**
  (`org-clock-sum`).
- **Timestamps** on [jiff](https://docs.rs/jiff), with Emacs's date arithmetic
  (January 31 plus one month is March 3).
- **Caching** across document versions keyed by green-node identity: after
  an incremental reparse, only the headlines on the path to the edit are
  computed again.

Every value is compared with Emacs 30.1 / Org 9.7 by
`kalem diff-emacs --model` over the Org manual, all of Worg and dedicated test
files (329,000+ checks, all identical). Where Emacs has quirks that change what
documents mean, `org-model` reproduces them; they are listed in
[`book/part-2/org-known-differences.org`](https://github.com/getkalem/kalem/blob/main/book/part-2/org-known-differences.org).

## License

MIT OR Apache-2.0.
