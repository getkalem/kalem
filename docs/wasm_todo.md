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

- [~] W4 A build step (a `build.rs` of a small crate, or `xtask`) that
  builds the bundled plugins for `wasm32-unknown-unknown`, wraps them with
  `wasm-tools component new`, refuses one importing WASI, and precompiles
  them with Wasmtime's `Engine::precompile_component` for the binary's own
  engine; the plugins' sources from `getkalem/plugins` at a pinned tag (a
  release, per W2), or the released `.wasm` checked against its signature
  and `SHA256SUMS`. CI caches the result.
  (done 2026-10-05 but the precompiling: the crate `kalem-components`,
  feature `build`: its `build.rs` copies the plugins' workspace from the
  sources Cargo keeps at the pinned revision (only what changed is
  written), patches Kalem's crates to this checkout, builds each plugin
  on its own for `wasm32-unknown-unknown` with `+simd128` (built
  together, Cargo unified their features and the image viewer exported
  `grid`), wraps them with wit-component, refuses a WASI import, and
  embeds them with their manifests (`kalem_components::components()`).
  Test `the_bundled_components_bind`; CI's job "bundled plugins as
  components". The source is the pinned revision, as the native copies'
  is, until releases follow W2. Precompiling for the binary's engine
  goes with W5, where the engine's configuration is known; Wasmtime's
  cache compiles them once meanwhile.)

## W5. Embedded components loaded on first use

- [~] W5 The precompiled components embedded (`include_bytes!`) and
  registered as viewers like installed ones (`ComponentViewer`), compiled
  code deserialized on first use, so a file opens without a compile wait;
  an installed newer version of the same plugin preferred over the
  embedded one; `kalem plugin list` showing which is used.
  (done 2026-10-05 but the precompiling: the feature `components` of
  `kalem-cli` and `kalem-editor` builds them in (`kalem-components`) and
  registers each as a `ComponentViewer::embedded` in the place of the
  native viewer of the same name, which stays its fallback; the startup
  thread compiles them into the plugin cache, so after the first start
  a file opens without a compile; an installed copy is used only when
  newer (`embedded_is_newer`); `kalem plugin list` marks them "(built
  in)" and an installed copy not used, `kalem plugin check` runs them
  too. Test `crates/kalem/tests/components.rs`. Not default yet: W7 and
  W6 decide. Open: precompiling at build time for the binary's engine.)

## W6. Speed and limits measured on components

- [~] W6 The workbook, PDF and picture measurements of the native copy
  repeated through the component: opening and scrolling a million-cell
  workbook, a held arrow key, a 100,000-cell paste, sorting, a long PDF
  scrolled; the costs found removed (cells asked for in larger batches and
  kept between frames, nothing asked of the component on every frame that
  has not changed, the memory limit raised by the manifest where a
  workbook needs it); each measurement a test with its ceiling, as
  `grid_speed.rs`.
  (2026-10-05: `crates/kalem-cli/tests/component_speed.rs`, run by CI's
  components job in a debug build, as the owner runs Kalem: every
  measurement fails past four times the native copy's time unless done
  within a frame, and a million cells must fit in 512 MB. Debug build,
  Apple M1 Max:

  | Measurement | Native | Component |
  |---|---|---|
  | workbook of 36,000 cells opened | 24 ms | 31 ms |
  | 200 steps of a held arrow key | 204 ms | 280 ms |
  | 60 frames, nothing changed | 67 ms | 88 ms |
  | 100,000 cells pasted | 243 ms | 324 ms |
  | 3,000 rows sorted | 299 ms | 373 ms |
  | million cells opened | 650 ms | 855 ms |
  | million cells, a cell typed | 536 ms | 560 ms |
  | 6-megapixel PNG opened and drawn | 59 ms | 99 ms |
  | 50 PDF pages drawn | 921 ms | 1,614 ms |

  Found and removed: in a debug build the host's bindings (generic code
  of wasmtime's, instantiated in kalem-script) lifted a page's pixels
  byte by byte, 40 ms a PDF page; kalem-script is now optimized in the dev
  profile, as are ironcalc_base (a cell typed in a million-cell workbook
  recalculated in 3 s natively) and png's fdeflate, simd-adler32 and
  crc32fast (a 6-megapixel picture opened in 120 ms natively). Not done,
  as not found: what a frame asks is cached by generation already
  (layout, tabs, panes, outline, drawings, the cursor's validation), and
  the cells, asked anew each frame, cost 1.5 ms through the component for
  a screenful; a cache of them would risk cells shown stale for no frame
  saved. A million numeric cells need 256 MB inside the component, four
  million 768 MB, ten million (a million rows of ten) 2 GB: past a
  viewer's 1 GB. Built-in components now take their manifests' limits as
  installed ones do, and the workbook plugin's manifest asks for 4 GB, the
  most a 32-bit component addresses (getkalem/plugins 6d2a305). Open:
  Kalem's pins moved to that revision, so the built-in workbook component
  gets it, and the plugin's next release. Beyond 4 GB a workbook opens
  only natively, which W9 must weigh. Found in the plugin, for both
  copies: after every edit the first frame re-reads the sheet's data
  validations from its whole XML, 70 ms natively in a million-cell sheet,
  140 ms through the component.)

## W7. The editors' tests through components

- [~] W7 Every editor test that opens a picture, a PDF or a workbook run
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
  (2026-10-05: the parity test is `crates/kalem-cli/tests/component_parity.rs`,
  in CI's components job: every built-in component against its native
  copy on the terminal editor's fixtures, the xlsx plugin's own corpus
  (`Component::source`) and `KALEM_XLSX_CORPUS`, two PDF files and five
  kinds of picture made in the test, read, drawn to the pixel, edited and
  saved alike; all alike. `kalem-components`' feature `viewers` gives a
  component as the viewer Kalem registers (`kalem_components::viewer`,
  with its manifest's extensions and limits), used by Kalem and its
  tests. The editors' tests through it: kalem-tui's and kalem-ui's
  fixtures register the components and read saved workbooks back through
  the contract, the native crates gone from their dev-dependencies; all
  pass but one, `protection`, which found the component trapping where
  the native copy worked: std's clock panics on wasm32-unknown-unknown,
  and a password's salt, VBA's `Now` and `Rnd` and a macro's time budget
  read it. Fixed in getkalem/plugins f42b517 (they read Kalem's clock).
  Open: Kalem's pins moved to f42b517, then the editors' tests switched
  (ready, waiting for the pins), and the clippy job given the
  WebAssembly target.)

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
