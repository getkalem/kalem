# Plugins as WebAssembly only

Today the three plugins Kalem ships (pictures, PDF files, Excel
workbooks, from `getkalem/plugins`) are compiled into the binary as native
Rust crates (`kalem-cli`'s feature `viewers`, pinned to one revision of
`getkalem/plugins`), and the same crates are built as WebAssembly
components that users install. The design (D28, D29) wants one kind: every
plugin a component, the ones everyone expects shipped inside the binary as
embedded components, loaded on first use. This list is the way there, the
most basic first. A task is done when it works in both editors, has its
tests, and leaves nothing a native build did that the component does not.

Why the native copies are still there, as found on 2026-10-05:

- The plugin API is not versioned. Every WIT file says
  `kalem:plugin@0.1.0`, yet the grid contract has changed with almost every
  spreadsheet task; a component binds only to the exact WIT it was built
  against, so every change breaks the installed ones (xlsx 0.0.1 to 0.0.4),
  and the native copy, always built from the same commit, is the fallback
  (`ComponentViewer::with_fallback`).
- Changing the contract takes three pushes in order (the contract, the
  plugin, the pins), because the native copy must build against Kalem's
  `main`.
- Nothing builds the components inside Kalem's own build; the binary has
  no component to embed.
- The editors' tests (`kalem-ui/tests`, `kalem-tui/tests`: workbook, pdf,
  viewer, grid speed, custom lists) call the native crates directly; only
  `kalem-cli/tests/xlsx_component.rs` runs a component, and only when told
  where one is.
- Speed and limits were measured with the native copy only (a million
  cells, held arrow keys, a 100,000-cell paste), never with a component and
  its 64 MB and 100 ms limits.
- `.xls`, `.xlsb` and `.ods` were native only. Not so: calamine builds for
  `wasm32-unknown-unknown` and imports nothing; with it the component reads
  them alike (checked 2026-10-05, the parity test over the corpus).

## W1. The legacy formats in the component

- [x] W1 calamine an ordinary dependency of the xlsx plugin; `.xls`,
  `.xlsb` and `.ods` detected and opened by the component, listed in its
  manifest's `opens`; `xlsx_component.rs` comparing them with the native
  copy.
  (done 2026-10-05, plugins 9874945: the component builds for
  `wasm32-unknown-unknown` with calamine and imports no WASI; the parity
  test over the plugin's corpus finds every workbook alike, one `.xls`
  and two `.ods` among them; the corpus has no `.xlsb` yet. Installed
  components get it with the next xlsx release.)

## W2. A versioned plugin API

- [~] W2 The WIT package given a real version and a rule written down
  (`docs/` and the Book's plugin part): a released version is never
  changed; new functions go into new interfaces (`grid-2`, …) or new
  versions of the package, records never gain fields after release, a
  removed function stays as a stub that refuses. `kalem-plugin` and
  `kalem-viewer` published at that version (T3.1.3's open part) so plugins
  depend on a release, not on Kalem's `main`.
  (done 2026-10-05 but the publishing: the package is `kalem:plugin@0.2.0`
  (`kalem_script::API_VERSION`); the released files (`viewer`, `files`,
  `clock`, `grid`) are copied in `crates/kalem-plugin/wit-frozen/0.2.0/`
  and `tests/frozen.rs` fails a change to them; the worlds moved to
  `worlds.wit`; the rule is in the Book's "Versions of the plugin API"
  and CONTRIBUTING: new functions in new interfaces with a patch version,
  removals as refusing stubs until a 0.3. A component whose manifest
  names another API (`^0.1`, everything released so far) is not tried:
  `plugin-api-mismatch` says so and the bundled viewer opens its files;
  the template says `^0.2`. Open, the owner's: publishing `kalem-plugin`
  and `kalem-viewer` on crates.io, and releasing the three viewer
  plugins built against 0.2.0.)

## W3. The host binding older components

- [~] W3 The host links a component by what it exports, not by the whole
  world: functions it lacks answer as the contract's defaults do
  (`ViewerDocument`'s default methods already say "not edited" or give
  nothing), so a component built against an older API version still opens
  its files and offers what it has. Tested with a component built against
  the previous version kept in `tests/plugins`.
  (done 2026-10-05 at the granularity W2's rule leaves: a released
  interface never loses a function, so a component has an interface or
  not. `Viewer::new` binds `viewer`, then each further interface on its
  own (`grid::GuestIndices`), left out when the component does not export
  it, refused when it exports it differently; names are looked up
  semver-compatibly by Wasmtime (`grid@0.2.0` finds `grid@0.2.1`, either
  way), so a 0.2.0 component runs on a 0.2.x Kalem. Test
  `interfaces_bound_as_the_component_has_them`. Open: the fixture of a
  component built against the previous version, which needs a 0.2.1 to
  exist (the first interface added).)

## W4. Components built by Kalem's build

- [ ] W4 A build step (a `build.rs` of a small crate, or `xtask`) that
  builds the bundled plugins for `wasm32-unknown-unknown`, wraps them with
  `wasm-tools component new`, refuses one importing WASI, and precompiles
  them with Wasmtime's `Engine::precompile_component` for the binary's own
  engine; the plugins' sources from `getkalem/plugins` at a pinned tag (a
  release, per W2), or the released `.wasm` checked against its signature
  and `SHA256SUMS`. CI caches the result.

## W5. Embedded components loaded on first use

- [ ] W5 The precompiled components embedded (`include_bytes!`) and
  registered as viewers like installed ones (`ComponentViewer`), compiled
  code deserialized on first use, so a file opens without a compile wait;
  an installed newer version of the same plugin preferred over the
  embedded one; `kalem plugin list` showing which is used.

## W6. Speed and limits measured on components

- [ ] W6 The workbook, PDF and picture measurements of the native copy
  repeated through the component: opening and scrolling a million-cell
  workbook, a held arrow key, a 100,000-cell paste, sorting, a long PDF
  scrolled; the costs found removed (cells asked for in larger batches and
  kept between frames, nothing asked of the component on every frame that
  has not changed, the memory limit raised by the manifest where a
  workbook needs it); each measurement a test with its ceiling, as
  `grid_speed.rs`.

## W7. The editors' tests through components

- [ ] W7 Every editor test that opens a picture, a PDF or a workbook run
  against the embedded component (the native crate no longer a dependency
  of `kalem-ui` and `kalem-tui`); the fixtures registering the component
  viewer; the parity test of `xlsx_component.rs` run in CI always, not
  only when told where a component is, and widened to the PDF and picture
  viewers.
  (Found on the way, 2026-10-05, and fixed: a component read its file by
  path only, so a workbook Kalem holds in memory (an `.ods` converted as
  it opens, `workbook_io::open_bytes`) reached it as a missing file; the
  host's file resource now carries the `FileHandle` itself, test
  `bytes_the_host_holds_reach_the_plugin`. Such paths are what W7's
  editor tests through components will find.)

## W8. Errors and a component that fails

- [ ] W8 A component that traps, runs out of time or memory, or cannot be
  linked: its document closed with a message naming the plugin and what
  went wrong, the file offered as text or not at all, never the editor
  stopped; repeated failures disabling the plugin until it is updated
  (T3.1.13); the log keeping the plugin's version and the trap.

## W9. The native copies removed

- [ ] W9 `kalem-cli`'s feature `viewers` and its three `register` calls
  removed, and with them the git pins of `getkalem/plugins` in Kalem's
  `Cargo.toml`, `tools/check-plugin-pins.sh` and the three-push order; the
  terminal-only build's size measured again (components embedded or left
  out by a feature); a contract change then needs a plugin release only.

## W10. Plugins developed and debugged as components

- [ ] W10 What a plugin author needs without the native copy: `kalem
  plugin build` and `kalem plugin dev DIR` (the component built and loaded
  again when its sources change), a panic's message and backtrace from the
  component in Kalem's log, the plugin's own unit tests run natively in
  its crate as now; the plugin template and the Book's plugin part say so.
