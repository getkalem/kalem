# Test corpus license register

Every file in `tests/corpus` is listed here with its source and license. Corpus files are test data. They are not part of any published crate (`tests/` is outside every crate's package).

Rules:

- Only add files whose license allows redistribution.
- Never add personal documents or files containing personal data.
- Record the upstream version or commit, so the file can be refreshed.
- Synthetic files written for Kalem are licensed like the project (MIT OR Apache-2.0).

| Path | Source | Version | License |
|---|---|---|---|
| `org-mode/org-manual.org` | Org mode `doc/org-manual.org` | release_9.7.11 (6a5d0ed3) | GFDL-1.3-or-later |
| `org-mode/org-guide.org` | Org mode `doc/org-guide.org` | release_9.7.11 (6a5d0ed3) | GFDL-1.3-or-later |
| `org-mode/doc-setup.org` | Org mode `doc/doc-setup.org` (setup file of the manual) | release_9.7.11 (6a5d0ed3) | GFDL-1.3-or-later |
| `org-mode/ORG-NEWS.org` | Org mode `etc/ORG-NEWS` | release_9.7.11 (6a5d0ed3) | GPL-3.0-or-later (part of Org mode) |
| `org-mode/examples/**` | Org mode `testing/examples/` | release_9.7.11 (6a5d0ed3) | GPL-3.0-or-later (part of Org mode) |
| `worg/org-syntax.org` | Worg `org-syntax.org` (the Org Syntax specification) | 22fc0631 | GFDL-1.3-or-later (text), GPL-3.0-or-later (code examples) |
| `synthetic/*.org` | Written for Kalem | – | MIT OR Apache-2.0 |

## Extended corpus

`fetch-extended.sh` clones the full Org mode repository (release_9.7.11) and Worg (commit 22fc0631) into `.cache/`. Those files are not committed. Worg text is GFDL-1.3-or-later and its code examples are GPL-3.0-or-later; Org mode is GPL-3.0-or-later and its manual is GFDL-1.3-or-later.
