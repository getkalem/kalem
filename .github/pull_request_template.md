## Summary

## Related issue or task

Closes # / docs/todo.md task ID:

## Checklist

- [ ] `cargo fmt --all` and `cargo clippy --workspace --all-targets` are clean
- [ ] Tests added or updated
- [ ] `CHANGELOG.md` updated under "Unreleased"
- [ ] The Book (`book/`) updated in the same pull request; `kalem book check` passes
- [ ] The Book chapter of every format whose reading, showing, editing or writing changed is updated (`book/chapters.toml` maps the code to its chapter; otherwise the label `book-unchanged` and why, here)
- [ ] `docs/design_document.md` or `docs/design_doc2.md` updated if the design changed
- [ ] Round-trip guarantee preserved in every format (no untouched byte changes); nothing written into a standard file that its standard does not define
- [ ] Works in the terminal editor too, or the terminal form and the gap are recorded in `book/part-5/terminal-parity.org` (design 4.1, principle 7)
