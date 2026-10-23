# Released interfaces of the plugin API

Each folder is a release of the WIT package `kalem:plugin`: copies of the
files whose interfaces that release made public. A released interface
never changes, as components built against it must keep binding
(`tests/frozen.rs` compares each copy with `../wit`, the `package` line
aside). A new function goes into a new interface (`grid-2`) in a file of
its own, released with the next patch version (`0.2.1`), and a world in
`worlds.wit` exports it; the host binds it when a component has it. See
the Book, Part III, "Versions of the plugin API".

## The core, frozen at 0.2.21

0.2.0 froze the viewers' interfaces only. The core every extension
plugin binds (`kalem.wit`: the `kalem` and `plugin` interfaces;
`editor.wit`, `ui.wit`, `access.wit`) stayed out, because `kalem.wit`
also held the `extension` world, which grows with each version, and
nothing checked the rest. So 0.2.4 could add `on-process` to the
released `plugin` export: a component built against 0.2.0 to 0.2.3
that exports `plugin` without it does not bind on a later Kalem. The
other three files did not change past their `package` line, nor did
`kalem` past a comment. History stays as it is.

Since the review of 23 October 2026 the worlds live in `worlds.wit`
alone, and those four files, with `executor.wit` of 0.2.21, are copied
in `0.2.21/`. `tests/frozen.rs` checks that every core file keeps a
copy here and that no file but `worlds.wit` declares a world. A
callback the editor needs from every plugin goes into an interface of
its own (as `focus`, 0.2.18), never into `plugin`; a new interface file
is frozen by the next API version's bump at the latest, as before.
