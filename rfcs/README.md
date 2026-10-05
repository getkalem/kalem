# Kalem RFCs

Large or hard-to-reverse changes to Kalem go through a short written proposal, a "request for comments" (RFC). Small fixes and features that fit the existing design do not need one.

## When to write an RFC

- A change to the architecture, crate boundaries or public APIs of the `org-*` or `latex-*` crates.
- A new extension point or a change to the plugin API.
- A change that affects what Kalem writes into a standard format, or its agreement with a format's reference implementation (Emacs for Org, the TeX engines and pandoc for LaTeX, RFC 4180 for CSV, the CommonMark and GFM suites for Markdown).
- Adding a heavy dependency or a new external tool.
- Resolving one of the open decisions (D1, D2, ...) in section 21 of the design document.

## Process

1. Copy the template below to `rfcs/NNNN-short-title.md`, using the next free number.
2. Open a pull request. Discussion happens on the pull request.
3. When there is rough consensus, a maintainer merges it as accepted, or closes it as declined with a short explanation.
4. Accepted RFCs are reflected in `docs/design_document.md` and `docs/roadmap.md`.

## Index

| Number | Title | Status |
|---|---|---|
| 0001 | [Kalem design document](../docs/design_document.md) | Accepted, living document |
| 0002 | [Standard modes and the Book](../docs/design_doc2.md) | Accepted (owner, 2026-09-30); its sections on Kalem's own format withdrawn (2026-10-04) |
| 0003 | The Kalem format (`.klm`), specification | Withdrawn (owner, 2026-10-04): the Kalem format was removed |

## Template

```markdown
# RFC NNNN: Title

- Status: Draft
- Design document sections affected:
- Decision IDs affected:

## Summary

## Motivation

## Design

## Alternatives considered

## Compatibility with the standards and their reference implementations

## Unresolved questions
```
