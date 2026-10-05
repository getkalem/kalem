# Released interfaces of the plugin API

Each folder is a release of the WIT package `kalem:plugin`: copies of the
files whose interfaces that release made public. A released interface
never changes, as components built against it must keep binding
(`tests/frozen.rs` compares each copy with `../wit`, the `package` line
aside). A new function goes into a new interface (`grid-2`) in a file of
its own, released with the next patch version (`0.2.1`), and a world in
`worlds.wit` exports it; the host binds it when a component has it. See
the Book, Part III, "Versions of the plugin API".
