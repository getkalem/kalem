# Kalem RFCs

Large or hard-to-reverse changes to Kalem go through a short written proposal, a "request for comments" (RFC). Small fixes and features that fit the existing design do not need one.

## When to write an RFC

- A change to the architecture, crate boundaries or public APIs of `org-*` crates.
- A new extension point or a change to the plugin API.
- A change that affects file compatibility with Emacs.
- Adding a heavy dependency or a new external tool.
- Resolving one of the open decisions (D1, D2, ...) in section 21 of the design document.

## Process

1. Copy the template below to `rfcs/NNNN-short-title.md`, using the next free number.
2. Open a pull request. Discussion happens on the pull request.
3. When there is rough consensus, a maintainer merges it as accepted, or closes it as declined with a short explanation.
4. Accepted RFCs are reflected in `design_document.md` and `todo.md`.

## Index

| Number | Title | Status |
|---|---|---|
| 0001 | [Kalem design document](../design_document.md) | Accepted, living document |

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

## Compatibility with Emacs Org

## Unresolved questions
```
