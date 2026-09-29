# Kalem — Work Breakdown

Derived from `design_document.md`. Every item references the relevant section with `§`. When the design changes, update the document first, then this list.

Markers: `[ ]` to do · `[x]` done · `[-]` cancelled · `[~]` in progress
IDs: `T<phase>.<group>.<n>`; decisions are `D<n>` (§21).

Phases are sequential. The next phase does not start before the current phase's **exit criteria** are met.

---

## Phase 0: Discovery and foundation (§20, 2 to 3 months)

### 0.1 Project setup (§18)

- [x] T0.1.1 Name check: crates.io, GitHub, domain. Result: `kalem` crate and GitHub user are taken; product stays **Kalem**, package `kalem-editor`, binary `kalem`, GitHub org `kalem-editor`, candidate domain `kalemeditor.org` (D7)
- [x] T0.1.2 License decision D1; `LICENSE-MIT`, `LICENSE-APACHE` (MIT OR Apache-2.0)
- [x] T0.1.3 Git repository and Cargo workspace: `crates/org-syntax`, `crates/kalem-cli`, `crates/kalem`; `rust-toolchain.toml`; `rustfmt.toml`; lints
- [x] T0.1.4 CI: GitHub Actions, Linux + macOS + Windows matrix; fmt, clippy, test, MSRV; caching; dependabot (not yet run on GitHub: the repository has not been pushed)
- [x] T0.1.5 `README.md` (vision, "Typora for Org"), `CONTRIBUTING.md`, code of conduct, PR and issue templates, `CHANGELOG.md`
- [x] T0.1.6 `rfcs/README.md`: RFC process; the design document is RFC 0001
- [x] T0.1.7 `tests/corpus/LICENSES.md`: corpus license register
- [x] T0.1.8 Draft of the first announcement to the Org mailing list (sent at the end of phase 1): `docs/announcements/org-mailing-list-draft.md`
- [ ] T0.1.9 Maintainer contact address for the README and code of conduct (needs a decision by the project owner)

### 0.2 Parser foundation decision (§5.6, D2)

- [x] T0.2.1 Add orgize as a trial dependency in an evaluation crate; confirm version and rowan basis
- [x] T0.2.2 Round-trip test on the corpus (`parse(x).to_string() == x`): 32/32 files, 0 panics on 6,400 mutated inputs
- [x] T0.2.3 Coverage audit against the table in §3.2: no citations, inlinetasks or diary sexps; 72.5% structural agreement with Emacs
- [x] T0.2.4 Incremental reparsing: present, or can it be added (architecture review)
- [x] T0.2.5 Maintenance status, open PR latency, license: dormant since mid-2024, MIT
- [x] T0.2.6 Decision note: dependency / fork / new parser; close D2 → new parser (`docs/decisions/D2-parser-foundation.md`)

### 0.3 org-syntax (§5)

- [x] T0.3.1 `SyntaxKind` enum: token and node kinds for every element in §3.2
- [x] T0.3.2 Lexer: line-based tokens, whitespace and line endings preserved (LF and CRLF; CRLF is parsed like Emacs's DOS decoding)
- [x] T0.3.3 `ParseContext` and pre-pass: `#+TODO`, `#+TAGS`, `#+STARTUP`, `#+PROPERTY`, `#+LINK`, `#+MACRO`, `#+CONSTANTS`, `#+SETUPFILE` (injected loader)
- [x] T0.3.4 Element parser: headline and section
- [x] T0.3.5 Element parser: planning, property drawer, drawer
- [x] T0.3.6 Element parser: plain list, item, checkbox, descriptive list, indentation rules
- [x] T0.3.7 Element parser: org table rows and cells, hline, `#+TBLFM`; table.el recognition
- [x] T0.3.8 Element parser: blocks (src, example, export, verse, comment, center, quote, special), dynamic block, unterminated block tolerance
- [x] T0.3.9 Element parser: keyword, affiliated keyword, babel call, clock, diary sexp, latex environment, fixed-width, horizontal rule, comment, footnote definition, inlinetask
- [x] T0.3.10 Object parser: emphasis (Org's regexp rules exactly), code, verbatim
- [x] T0.3.11 Object parser: link types (file, http, id, custom-id, fuzzy, radio, coderef, abbreviation, attachment), plain and angle links
- [x] T0.3.12 Object parser: timestamps (active, inactive, range, repeater, warning, diary)
- [x] T0.3.13 Object parser: footnote reference, inline src, inline call, latex fragment, entity, sub/superscript, line break, macro, export snippet, target, radio target, statistics cookie, citation
- [x] T0.3.14 Typed AST layer (`ast::Headline` etc.), §5.5 API; every property Emacs computes is checked by `kalem diff-emacs`
- [x] T0.3.15a Unbounded nesting (§3.6): recursive walks grow the stack on demand; 20,000 nested lists, blocks, emphasis and footnotes parse without crashing
- [x] T0.3.15b Regex engine configuration: fast DFA engines enabled, line-local regexes bounded to the line, indentation skipped before capturing regexes (5x to 15x faster on deep indentation, 2x on the corpus)
- [x] T0.3.15c Remove quadratic cases found by the nesting tests: indexes for block end lines, bracket matching and emphasis closers (nested blocks 50x, drawers 50x, footnotes 40x faster); nesting capped at 4096 levels, more than 10x Emacs's own limit (§3.6)
- [x] T0.3.15d Radio targets without limits (§3.6): a trie of targets case-folded through Emacs's canon table replaces the regexp; 1 MB with about 20,000 radio links parses in 0.1 s with 100 to 5,000 targets and 0.2 s with 10,000 (`examples/radio_timing.rs`); radio-heavy files and 1,500 fuzzed files identical to Emacs
- [x] T0.3.15 Incremental reparsing: element splice inside a section, section reparse when end lines or footnote definitions are touched, full parse when headlines, in-buffer settings or radio targets change. Identical to a full parse on 99,000 random edits; typing in the 840 KB Org manual takes 67 µs per keystroke (full parse 55 ms)
- [x] T0.3.15e Fewer full reparses: headline-line edits rebuild only the line (subtree reused); edits at a section's first character stay incremental; full reparses down from 9.4% to about 5% of random edits. CRLF and BOM documents reparse incrementally too: the edit runs on a kept normalized tree and text, and only the replaced node is converted back (62 µs per keystroke on the CRLF Org manual, 0.5 ms on 4.3 MB; about 90% of typing edits incremental)
- [x] T0.3.16 Error tolerance and diagnostics: no-panic guarantee (tested); `Parse::diagnostics()` with org-lint style checks (unterminated blocks and drawers, stray end lines, orphaned affiliated keywords, invalid timestamps, unbalanced calls, duplicate names and custom IDs); `kalem check` prints them
- [x] T0.3.17 Snapshot tests (`insta`): at least three examples per element
- [x] T0.3.18 Round-trip fuzzing (`cargo-fuzz` targets for parsing and incremental reparsing, run in CI on nightly) and a `proptest` generator of Org-like documents (round-trip, incremental equals full parse)
- [x] T0.3.19 Benchmarks (`criterion`): full parse 11.6 to 12.1 MB/s (10 MB in 0.79 s), keystroke reparse 63 µs; targets in §15 met
- [x] T0.3.20 `org-syntax` crate documentation, README and docs.rs readiness as a standalone crate (§4.7); `cargo package` passes

### 0.4 Emacs differential testing (§5.7, §16)

- [x] T0.4.1 `tests/emacs/dump.el`: `org-element-parse-buffer` → JSON (type, begin, end, key properties)
- [x] T0.4.2 Rust-side JSON dump (`kalem dump --format emacs-json`)
- [x] T0.4.3 Normalization and comparison tool; difference report (`kalem diff-emacs`, properties included; `tools/fuzz-diff.py` for differential fuzzing)
- [x] T0.4.4 CI: Emacs job with the differential run over the core and extended corpus and differential fuzzing (not yet run on GitHub: the repository has not been pushed)
- [x] T0.4.5 Corpus: Org Manual source, Org's own test files, Worg pages (extended corpus via `tests/corpus/fetch-extended.sh`), synthetic edge cases (CRLF and BOM, CJK, Turkish, malformed input, nesting)
- [x] T0.4.6 Known differences document (`docs/known-differences.org`)

### 0.5 The kalem binary and first subcommands (§4.2, §7.7)

- [x] T0.5.1 `kalem` binary skeleton with `clap`; dispatch to subcommands; `--version`, `--help`
- [x] T0.5.2 `kalem parse FILE`: tree dump
- [x] T0.5.3 `kalem check FILE...`: syntax diagnostics, round-trip verification, exit codes (`--deny-warnings`)
- [x] T0.5.4 `kalem diff-emacs FILE`: differential comparison
- [x] T0.5.5 `--format json` convention and golden tests for subcommand output

### 0.6 gpui spike (§7.1, D3)

- [x] T0.6.1 Separate spike crate; pinned gpui version; build without full Xcode (runtime shader compilation)
- [x] T0.6.2 Editable paragraphs with inline styles, backed by a rope (the spike used a `String`; the rope moves to T1.3.1)
- [~] T0.6.3 IME composition: macOS, Windows, Linux (implemented on gpui's input handler; manual verification moved to T1.5.9a)
- [x] T0.6.4 60 fps scrolling in a 100,000-line document (virtualization): 120 Hz, work under 7 ms per frame on 117,850 lines
- [x] T0.6.5 Custom inline widgets: SVG formula, checkbox, fold arrow (own inline layout; formulas rendered off the main thread)
- [x] T0.6.6 Accessibility API status (AccessKit): none in 0.2.2; merged in gpui main 2026-05, unreleased
- [x] T0.6.7 Clipboard, drag and drop, file dialogs (APIs wired in the spike; exercised by hand in T1.5.20)
- [x] T0.6.8 Spike report; go/no-go; close D3 (if no-go, write the Tauri + ProseMirror plan): go, `docs/decisions/D3-ui-framework.md`

### 0.7 Math rendering prototype (§9.2, D4)

- [x] T0.7.1 mitex + typst + typst-svg prototype; 100-formula corpus
- [x] T0.7.2 ReX (or a current fork) prototype on the same corpus: ReX is unmaintained; RaTeX evaluated instead
- [x] T0.7.3 Comparison table: coverage, quality, binary size, render time
- [x] T0.7.4 Close D4: RaTeX, `docs/decisions/D4-math-engine.md`

### 0.8 Terminal spike (§7.6, D14)

- [x] T0.8.1 ratatui + crossterm prototype: styled, editable paragraphs with hidden markers and cursor reveal
- [~] T0.8.2 Capability detection: true color, italics, strike-through, OSC 8, kitty/iTerm2/sixel graphics (implemented and tested in a pseudo-terminal; real terminals in T1.4.10)
- [~] T0.8.3 ratatui-image: inline image and formula image in kitty and iTerm2 (correct protocol output verified; visual check in real terminals in T1.4.10)
- [x] T0.8.4 Close D14: ratatui + crossterm + ratatui-image, `docs/decisions/D14-terminal-ui-stack.md`

### Phase 0 exit criteria

- [x] 100% round-trip on the corpus
- [x] At least 99% structural agreement on the Emacs differential (100% on 367 corpus files and 60,000+ mutated files, properties included)
- [x] D1, D2, D3, D4, D14 decided
- [~] `org-syntax` 0.1 ready for crates.io: metadata, license files, docs and `cargo publish --dry-run` are ready; blocked on D18 (entity table provenance) and on the owner's go-ahead to publish

---

## Phase 1: MVP, "Typora for Org" (§20, 4 to 6 months)

### 1.1 org-model (§6.1)

- [x] T1.1.1 Outline view: headline tree, levels, ranges, identities (`EntryId` in document order)
- [x] T1.1.2 TodoState: document sequence, done detection, multiple sequences (`ParseContext::todo_sequences` in Emacs's order, fast keys, log specs)
- [x] T1.1.3 Tags: direct, inherited, `#+FILETAGS`
- [x] T1.1.4 Properties: drawer, `#+PROPERTY`, inheritance, `_ALL` (special properties, categories and allowed values too)
- [x] T1.1.5 Timestamps: date parsing (D13 library), repeaters, warning delays (jiff; Emacs-style field arithmetic)
- [x] T1.1.6 Links: type resolution, target ranges; Names map (`#+NAME`, target, CUSTOM_ID, ID) (`org-link-search` semantics; 2,034 link targets identical to Emacs)
- [x] T1.1.7 Footnote mapping; statistics cookie computation; clock totals (321 footnotes, 107 cookies and 7,535 clock totals identical to Emacs)
- [x] T1.1.8 Lazy caching and invalidation by CST identity (`ModelCache`: outline and keywords after a keystroke in 0.4 ms on the Org manual, 1.7 ms on 4 MB)
- [x] T1.1.0 Emacs differential test for the model (`tests/emacs/model.el`, `kalem diff-emacs --model`): 329,234 checks on 377 files, 100% agreement
- [x] T1.1.9 Queries: `headlines_with_tag`, `scheduled_between`, `find_by_id`, `find_by_name`; Org match expression parser (5,176 match results identical to Emacs; group tags from `#+TAGS` are expanded since T1.2.6)
- [x] T1.1.10 Unit tests (inheritance, repeaters, cookie tables), plus the Emacs differential and a cache consistency property test

### 1.2 org-edit (§6.2)

- [x] T1.2.0 Emacs differential harness for commands: `tests/emacs/edit.el` runs each case, `tools/edit-expected.sh` stores Emacs's results in `tests/edit/expected.json`, `crates/org-edit/tests/emacs_diff.rs` compares text, point and errors
- [x] T1.2.1 `Transaction` type; overlap checks; apply and invert
- [x] T1.2.2 Undo and redo stack; 300 ms grouping; cursor restoration; `undo(redo(x)) == x` property test
- [x] T1.2.3 Style inference: indentation, blank line rules, `#+` casing, TODO sequence (`org_edit::style::Style::infer`, also source block indentation, tags column and line endings; defaults are `emacs -Q`; 9 ms on the Org manual). Commands that create content take a `Style`
- [x] T1.2.4 Headline operations: promote, demote, move, cut, copy, paste, sort (`org-sort-entries` by every key except a custom function, which waits for the plugin API; regions and reversed sorts). 2,158 command cases identical to Emacs, `tests/edit/`
- [x] T1.2.5 TODO cycle and selection; priority; `CLOSED` and logdone (`org_edit::todo`: every `org-todo` argument, keyword sets and type keywords, `#+STARTUP` logging, `KEY(x@/!)` state logging with notes, log drawers, `LOGGING` and `LOG_INTO_DRAWER` properties, parent statistics cookies, repeaters with `LAST_REPEAT`, `org-priority`; also `org-entry-put` and `org-timestamp-change` with a field. 3,577 command cases identical to Emacs)
- [x] T1.2.5a TODO dependencies (`org-enforce-todo-dependencies`, `ORDERED`, checkbox dependencies) and `org-todo-state-tags-triggers` (`org_edit::todo`: the two blockers of `org-blocker-hook` and `NOBLOCKING`, the tag triggers; `org_edit::property::delete_property` and `toggle_ordered` (`org-entry-delete`, `org-toggle-ordered-property`); 675 more command cases identical to Emacs; the settings `org.enforce_todo_dependencies`, `org.enforce_todo_checkbox_dependencies` and `org.todo_state_tags_triggers`, and the commands Toggle Ordered Subtasks and Delete Property in both editors)
- [x] T1.2.6 Tag add and remove; exclusive groups (`org_edit::tags`: `org-set-tags`, `org-toggle-tag`, `org-change-tag-in-region`, align all, and tag selection with `{ }` groups; `org_model::TagTable` parses `#+TAGS`, and match strings expand group tags as `org-tags-expand` does, identical to Emacs on the model corpus)
- [x] T1.2.7 List operations: indent, change type, checkbox and cookie update, renumber (`org_edit::list`: a port of the list structure of `org-list.el`: indent and outdent items and trees, cycle bullets, toggle checkboxes on items, headings and regions with `ORDERED`, update statistics cookies, move items, insert items, repair numbering. With random lists, 5,532 command cases in all are identical to Emacs, plus 4 cases where Emacs has a bug, listed in `docs/known-differences.org`. Checkbox counting in `org-model` now follows org-element's list structure, which extends past a list when later items are less indented)
- [x] T1.2.8 Emphasis wrap and unwrap; nesting rules (`org_edit::emphasis`: `org-emphasize`, identical to Emacs on 490 cases, and `toggle_emphasis` for the editor: unwraps an emphasis the selection is in, nests inside other emphasis without breaking it, refuses code and verbatim and selections that cross formatting, and checks the result parses)
- [x] T1.2.9 Insert commands: link, source block, timestamp (basic), horizontal rule (`org_edit::insert`: `org-insert-link` with a target, `org-insert-structure-template`, `org-timestamp` with a date, and a horizontal rule; identical to Emacs on 452 cases, plus 6 where Emacs has a bug, see `docs/known-differences.org`)
- [x] T1.2.10 Basic table operations: insert, delete, move rows and columns, align (identical to Emacs `org-table-align`) (`org_edit::table`: align, insert, kill and move rows, horizontal rules, insert, delete and move columns, TAB, S-TAB and RET motions that add rows, and `#+TBLFM` renumbering; widths and columns count link markup as hidden, as fontified Emacs does. With random tables, 8,749 command cases in all are identical to Emacs)
- [x] T1.2.11 Narrow and widen (`org_edit::narrow`: the ranges of `org-narrow-to-subtree`, `-element` and `-block` with org-element's element-at-point rules, and `narrowed` to run any command on the narrowed part; identical to Emacs on 952 cases; the view keeps the range)
- [x] T1.2.12 Before and after snapshot tests for every command (`crates/org-edit/tests/snapshots.rs`: 67 insta snapshots with the cursor shown, reviewed; with the Emacs differential of 9,101 cases)

### 1.3 kalem-core (§4, §11.2, §11.3, §14)

- [x] T1.3.1 Editor state: rope, parse, model, selection, document metadata (`kalem_core::DocumentState`: text, incremental parse, lazily built model with the subtree cache, selection, undo history, metadata with mode, line endings and BOM; `DocumentMode::detect` for §2.6)
- [x] T1.3.1a Rope with a line index, and contiguous text for `org-syntax` without copying the whole buffer on every edit (the D3 spike's `String` costs about 2 ms per keystroke at 4 MB). Decided: contiguous text with an incrementally updated line index, no rope, since the parser needs contiguous text; the edit costs 53 µs p50 at 3.8 MB and a whole keystroke with reparse 0.1 ms p50 on the Org manual, 1.7 ms on a 3.8 MB file of 50,000 sibling headings (`examples/text_timing.rs`). Plain text mode revisits this for 100 MB files (T2.7a.3)
- [x] T1.3.1b Full reparses (context keyword edits, fallbacks) on a background thread; the view keeps the old tree until the new one arrives (§5.3) (`Parse::try_reparse` and `parse_again` in `org-syntax`; the old tree comes with the edits made since, for mapping positions)
- [x] T1.3.2 Command registry: `Command`, `CommandHandler::Native`, ID convention, categories (`kalem_core::CommandRegistry` with every `org-edit` command, undo and redo, the Word profile's default keys and when-clauses; commands run on the narrowed part when narrowed; `org.headline.setLevel` for Ctrl+1..6 and `table.create` from `org-table-create` added)
- [x] T1.3.3 When-clause parser and evaluator (`kalem_core::when`: `&&`, `||`, `!`, parentheses, `==` and `!=` against strings, numbers and booleans, errors with positions)
- [x] T1.3.4 Keymap: JSON loading, chords, profiles (word, org), conflict report; terminal-safe variants (`kalem_core::keymap`: `keymap.json` entries with `keys`, `command`, `when`, `args`, `terminalKeys` and `-command` removal, comments allowed; the Word profile is the commands' default keys plus `keymaps/word.json`, the Org profile `keymaps/org.json` with Emacs keys (later replaced by the Vim profile; the Emacs keys are `docs/keymaps/emacs.json`); lookup with prefixes for key sequences; reports invalid entries, unknown commands, bindings shadowed by later ones or by longer sequences, and keys a legacy terminal cannot send, which fall back to `terminalKeys` or Alt with the same key; `DocumentState::when_context` gives the context keys at the cursor)
- [x] T1.3.5 Event bus: §11.3 events (veto and timeout infrastructure; script side in phase 3) (`kalem_core::events`: every §11.3 event with its plugin name; handlers per kind or for all; vetoable events wait for late answers without blocking the UI thread (`PendingVeto::poll`, 500 ms, then proceed with a warning); a sender for other threads; handlers that panic three times are removed; `ChangeDebouncer` for `document:changed` with ranges moved through later edits. The places that send each event come with their features: saving in T1.3.7, TODO and tag changes with the frontends' command execution)
- [x] T1.3.6 Settings: layered loading (`settings.toml`, workspace, document keywords) (D9) (`kalem_core::settings`: built-in settings with types, ranges and defaults; user and workspace files merged, each checked, wrong values reported and replaced by the layer below; the document layer through the parse base (`org.todo_keywords` under `#+TODO`) and the TODO settings (under `#+STARTUP`); commands get the settings; `set_in_toml` changes a value and keeps comments; D9 decided)
- [x] T1.3.7 File operations: open, atomic save, `.bak`, line ending and BOM preservation, external change detection (`notify`) (`kalem_core::files` and `DocumentState::open`, `save`, `save_as`, `external_change`, `reload`: UTF-8 with BOM, CRLF kept and bare line feeds from commands given a carriage return on save; saving through a temporary file and a rename that keeps permissions and symbolic links, in place for hard-linked files or files of another owner; `files.backup` for `NAME.bak`; a content hash tells a touch from a change; a changed file reloads as one undo step when there are no unsaved changes, otherwise it is a conflict and saving needs `force`; `FileWatcher` watches directories and sends `workspace:file-changed`. Other encodings are T2.7a.1)
- [x] T1.3.8 Frontend-neutral view model: blocks, inline runs, hidden marker spans, cursor reveal rules, shared by both frontends (`kalem_core::view`, grown from the spikes' `view.rs`: blocks that cover the document (heading lines, section elements, top-level list items), lines as styled runs with source mapping, markers revealed while the cursor is in the object (trailing blanks excluded), stars, bullets and checkboxes on the cursor's line, `#+TITLE:` as a title, entities, sub- and superscripts, footnote labels, formulas and image links as widgets, delimiter lines marked, hidden spans and grapheme motion that skips them; folding with `org-cycle`'s three states and `#+STARTUP` overview and content, moved through edits; checked on every corpus file)
- [x] T1.3.9 Logging (`tracing`) and a diagnostics file (`kalem_core::logging`: `kalem.log` in the state directory (`$KALEM_STATE_DIR`, `$XDG_STATE_HOME`, `%LOCALAPPDATA%` or `~/.local/state`), rotated per session with two kept; level from `KALEM_LOG` or `log.level`; panics logged with a backtrace; `diagnostics_report` for bug reports; the core logs failing and timed-out event handlers, vetoes, in-place saves, settings and keymap problems, background parses and reloads. Found on the way: background full parses now run one at a time, with the edits made meanwhile kept for position mapping)

### 1.4 kalem-tui, the first frontend (§7.6)

- [x] T1.4.1 Application skeleton: terminal setup and restore, panic hook, resize, event loop (`crates/kalem-tui`, `kalem tui FILE` and `kalem -t FILE`; raw mode, alternate screen, mouse, bracketed paste, focus events and the kitty keyboard protocol when detected; a panic restores the terminal after the log records it; one loop for input, background parses, file watching and debounced events; the shared registry, keymap (terminal variant), settings and event bus; saving through `document:before-save`; argument prompts from command schemas; checked on a pseudo-terminal)
- [x] T1.4.2 Editor view: viewport over the view model; headline glyphs and colors; folding (visible lines from the folds, wrapping with hanging indents, scrolling that keeps the cursor in view, `…` after folded headlines with content, Tab and Shift+Tab cycling, `#+STARTUP` folding, unfolding when the cursor moves into folded text)
- [x] T1.4.3 Inline rendering: emphasis attributes, hidden markers with cursor reveal, code, OSC 8 links, entities and sub/superscripts as Unicode (web and mail links get OSC 8; formulas as Unicode approximations; control characters as `^M`; image links as `[image: …]` until T1.4.10)
- [x] T1.4.4 Lists and clickable checkboxes; tables with box drawing and grid editing (bullets as `•`, checkboxes as `☐ ☑ ◐` that toggle on click; tables away from the cursor as aligned grids from `kalem_core::view::table_view` (markup hidden, `org-table-align`'s column alignment), while editing as the source with box bars; typing keeps columns aligned and the first key after Tab replaces the field, as `org-self-insert-command`, `org-delete-char` and `org-delete-backward-char` do (`org_edit::typing`, checked against Emacs on 1,000 cases), and tags stay aligned while typing on headlines)
- [x] T1.4.5 Src blocks with syntax highlighting; drawers and keywords folded and dimmed (source blocks highlighted through `kalem-highlight`, cached per block; blocks away from the cursor framed with their language or type instead of `#+begin`/`#+end`; drawers folded to their first line with `…` and opened when the cursor enters; runs of setting keywords folded and dimmed; `#+TITLE` as the title and `#+AUTHOR`, `#+DATE`, `#+SUBTITLE`, `#+EMAIL` as a byline)
- [x] T1.4.6 Cursor, selection, grapheme motion, mouse (click, scroll, drag) (motion over the display text, skipping hidden markup; words with Control or Alt; Home, End, paging with a kept column; Shift selects; click, Shift+click, double-click for a word, drag, wheel; copying goes to the system clipboard through OSC 52)
- [x] T1.4.7 Autoformat triggers and Enter/Tab behaviors (shared rules from kalem-core) (`kalem_core::input`: Enter makes a new item (with a box if the item has one), leaves the list on an empty item or goes up a level, moves to the next table row, keeps indentation in source blocks and indented text; Shift+Enter (Ctrl+J in terminals) is a line break; completion menus after `#+` (keywords, block templates), `[[` (headings, custom IDs, targets, link types) and `[fn:` (labels and the next number); a Unicode preview under a formula at the cursor. `- `, `* ` and `| ` at a line start are Org syntax and show as a list, a heading or a table at once. Tab indents in lists, moves in tables and folds on headings through the keymap)
- [x] T1.4.8 Source view toggle (`view.toggleSource`, Ctrl+/: the plain text with the same undo history, without folding or hidden markup)
- [x] T1.4.9 Outline side panel; command palette overlay; find and replace; status bar (the palette lists the commands that apply, fuzzy-filtered, with their keys, and asks for arguments; find searches as you type from where it started, marks every match, and replaces one or all (`kalem_core::find`, ignoring case unless the search has capitals, with `İ` and `I` matching `i`); the outline panel lists headings with TODO states, follows the cursor and jumps by key or click; the status line shows the file, changes, mode, position, pending keys and messages)
- [~] T1.4.10 Capability detection and fallbacks; `NO_COLOR`; ASCII mode; verify detection and images by hand in iTerm2, kitty, WezTerm, Terminal.app, Windows Terminal and a Linux VTE terminal (D14 checklist) (done: one query with a timeout for the terminal name, synchronized output, the kitty keyboard, the cell size and DA1; true color, italics, strike-through and OSC 8 from the answers and the environment; `NO_COLOR`; ASCII glyphs for `TERM=linux` or `KALEM_ASCII`; image links alone on a line drawn through kitty, iTerm2 or sixel, else `[image: …]`; `kalem tui --detect`. Open: the checks by hand in the six terminals, which need the owner)
- [x] T1.4.10a OSC 8 helper: redraw a whole link when any of its cells changes; kitty compression over SSH (instead of redrawing whole links, every link cell opens and closes the link with a shared `id`, so a cell redrawn alone keeps it; kitty images are zlib-compressed over SSH when the terminal is kitty)
- [x] T1.4.11 Rendering snapshot tests with ratatui's test backend (`crates/kalem-tui/tests/snapshots.rs`: a document with every kind of block, as text and as a map of styles per cell, with the cursor away, in bold text, in a table and in code, and in ASCII without color)
- [x] T1.4.12 Extract the rich text editing widget as a candidate standalone crate (§4.7) (`crates/tui-rich-text`, no Kalem types in its API: glyphs mapped to source offsets with caller data, wrapping with hanging indents, a viewport that scrolls through lines of any height and moves vertically at a kept column, drawing with cursor, selection, marks and per-cell OSC 8 links, mouse hits; `kalem-tui` supplies its lines through the `Lines` trait. The name `ratatui-rich-text` is reserved by the ratatui project)

### 1.5 kalem-ui, the graphical frontend (§7.1 to §7.5)

- [x] T1.5.1 Application skeleton: window, menu bar, toolbar, status bar, theme infrastructure (`crates/kalem-ui`: windows per file, menus and toolbar that run registry commands (`RunCommand`), menu shortcuts from the keymap, a status bar with file, changes, position and words, light and dark themes that follow the system; keys go through Kalem's keymap, with Command in place of Control for the Word-like profile on macOS; unbound keys reach gpui's input handler, so IME works)
- [x] T1.5.2 Editor view: virtualized block list over the view model (a gpui list of the visible source lines from `kalem_core::view::visible` (folds, drawers, setting keywords), updated after edits by the smallest splice so measured heights stay; checked headlessly with gpui's test platform)
- [x] T1.5.2a Inline layout from the D3 spike (`spikes/gpui-editor/src/inline.rs`): text pieces, widget boxes, row breaking, glyph and decoration painting, hit testing; asynchronous widgets with estimated-size placeholders (`kalem-ui/src/inline.rs`, with selection rectangles; checkboxes, formulas (Unicode for now) and image names as widget boxes. Asynchronous rendering with placeholders comes with formula images in T2.2)
- [x] T1.5.2b Move to a pinned gpui revision with AccessKit; expose text, caret and selection as accessibility nodes (§7.4) (gpui and `gpui_platform` from Zed's main branch at 1a28cff4 (2026-09-27), still built with the Command Line Tools through `runtime_shaders`; the editor is a `MultilineTextInput` node whose text runs are the lines on screen as read aloud (markup hidden, checkboxes as ☐ ☑ ◐), with the caret and selection; checked in the headless tests. A git dependency cannot be published on crates.io: `kalem-editor` ships as binaries (T1.8.1) until gpui publishes a release with AccessKit)
- [x] T1.5.3 Block rendering: headline (hidden stars, level style, TODO, priority, tags, folding, cookie) (stars hidden away from the cursor, sizes and colors by level, TODO and priority as rounded badges, tags at the right edge through a spacer that fills the row, fold arrows in the margin, cookies dimmed)
- [x] T1.5.4 Block rendering: paragraphs and inline runs; hidden marker model (§6.3) (emphasis, code, links, timestamps, entities from the shared view model, markup revealed at the cursor; superscripts and subscripts smaller on a shifted baseline; a deletion that breaks a marker says which formatting became plain text (`DocumentState::delete_backward`, both frontends))
- [x] T1.5.5 Block rendering: lists, clickable checkboxes (bullets, painted checkboxes that toggle on click, wrapped items hanging after the bullet and box)
- [x] T1.5.6 Block rendering: table grid; cell editing; Tab navigation (away from the cursor, a grid in the body font: columns measured in pixels and aligned as `org-table-align` decides, the header bold, column edges and rules drawn, gaps mapped to the bars so clicks land in cells; while editing, the source in the code font; typing keeps columns, Tab, Shift+Tab and Enter move between fields)
- [x] T1.5.7 Block rendering: src and example blocks (highlighting, copy button), quote, center, horizontal rule, keyword block (`#+TITLE` as a large title) (away from the cursor a block's first line is its language or type with a copy button for code, and its last line a thin gap; code highlighted on its background; quotes and verse with a bar and muted text; center blocks centered; rules drawn as lines; `#+TITLE` large, the byline italic, setting keywords dimmed and folded)
- [x] T1.5.8 Inline: links (click, Ctrl/Cmd+click), timestamp badges (display only), entities, cookies, line breaks (a click places the cursor, Command-click (Control-click elsewhere) opens: web and mail addresses in the system, Org and text files in a new window, other files with their application, internal links jump as `org-open-at-point` would (`kalem_core::input::link_at`, `org.link.open`, `C-c C-o`; the terminal opens with the system's opener); timestamps as badges; entities as characters; cookies dimmed; `\\` as ↵ away from the cursor)
- [x] T1.5.9 Cursor, selection, IME, grapheme motion, double and triple click (caret and selection painted from the inline layout, motion over the display text, word motion with Option, line motion with Command, IME through gpui's input handler with the composition underlined and UTF-16 ranges for the input method; double click selects a word, triple click a line; checked headlessly)
- [ ] T1.5.9a Manual IME verification: macOS Japanese and Pinyin (composition, candidate window placement), Turkish and US-international dead keys, Windows Microsoft IME, Linux IBus and Fcitx on X11 and Wayland
- [x] T1.5.10 Autoformat triggers (§6.3): `- `, `* `, `| `, `#+`, `[[` (completion menus under the caret after `#+`, `[[` and `[fn:`, from `kalem_core::input`; a preview under a formula at the caret; `- `, `* ` and `| ` are Org syntax and show as they are typed)
- [x] T1.5.11 Enter behaviors; Tab folding and indentation by context (the shared rules and keymap: new and ending list items, next table row, kept indentation; Tab folds headings, indents items, moves in tables)
- [x] T1.5.12 Paste: plain text; HTML → Org converter (simple); TSV → table (`kalem_core::paste`: tab-separated values (quoted fields too) become an aligned table on lines of their own, or rows below the current row in a table; HTML becomes headings, paragraphs, line breaks, emphasis (with Google Docs styles), code, links, images, lists with checkboxes, description lists, tables, source and example blocks, quotes and rules, and is left alone when it says no more than the text (code editors); verbatim text and non-Org documents take plain text; one undo step; "Paste as Plain Text" on ctrl+shift+v. HTML is read from the macOS pasteboard; Linux and Windows readers wait for T2.6.2)
- [x] T1.5.13 Source view: plain text editor, Org highlighting, same rope and undo stack (`view::source_line_view`: every byte as it is, with the rich view's styles; one monospace size, no widgets, folds or tag alignment; source blocks keep their syntax colors; in both frontends, on ctrl+/; checked on every corpus line)
- [x] T1.5.14 Split view (the source beside the rich view, or two views of one kind: one document, selection and history; each pane scrolls on its own and follows the caret; a click makes a pane active; `view.split` on ctrl+\\, and `C-x 3` in the Emacs example keymap)
- [x] T1.5.15 Outline sidebar: tree, jump, drag and drop (headings with TODO keywords, the current one marked; the tree folds on its own; a click jumps; a heading dropped on the upper half of another goes before it at its level, on the lower half after its subtree or as its first child; `org_edit::headline::move_subtree_to`, one undo step; folded headings stay folded as they move)
- [x] T1.5.16 Command palette; find and replace bar (regex option) (the palette lists the commands that apply, fuzzy matched, with their keys, and asks for missing arguments; the find bar marks every match, steps with Enter and Shift+Enter, replaces one or all (Alt+Enter, one undo step); Alt+R or the `.*` button makes the query a regular expression with `$1` in replacements, in both frontends; `kalem_core::palette` and `find::find_with` are shared)
- [x] T1.5.17 Status bar: word count (document and subtree), cursor, save state (`kalem_core::stats`: the words a reader sees, without markup, keywords other than the title, drawers, planning, code blocks or timestamps; the document's and the current section's, counted again only after edits, 5 ms for the Org manual; in both status bars)
- [x] T1.5.18 Date picker (basic), tag completion popup (`org.insert.date`, Alt+Shift+D, and `C-c .` and `C-c !` in the Emacs example keymap: arguments whose schema says `format: date` get a month calendar in the graphical editor (arrows, Page Up/Down, clicks, typed expressions) and a text prompt in the terminal; `kalem_core::dates` reads `today`, `+3d`, `-1w`, weekday names, `10-01`, ISO dates and times; a timestamp under the cursor is the starting date and is replaced. Tags complete after a colon at the end of a headline from `#+TAGS` and the tags in use, and are aligned after the choice)
- [x] T1.5.19 Settings window (font, size, theme, profile) (a panel in the window, Ctrl+, and the Kalem menu: theme (system, light, dark; the new `editor.theme`), Word-like or Vim keys, text size, and the installed fonts filtered by typing; each change is written to the user's `settings.toml` with its comments kept (`settings::save_setting`) and applies to every window at once, keys and menus too; a link opens the file. In the terminal, the command names the file)
- [~] T1.5.20 Widget tests with the gpui test harness; manual release checklist (29 headless tests in `crates/kalem-ui/tests/editor.rs`: typing, keys, IME, clicks, folding, tables, links, completion, paste, source and split views, outline drag and drop, palette, find and replace, dates, tags, settings, saving and outside changes, toolbar, menus, clipboard and history; the file is compared with the disk every second, reloaded when unchanged in the editor, and File > Revert to Saved is new; `docs/release-checklist.md` lists what only a person can check. Open: running the checklist on macOS, Linux and Windows, which needs the owner)
- [x] T1.5.21 Extract the gpui rich text widget as a candidate standalone crate (§4.7) (`crates/gpui-rich-text`: the inline layout, with no Kalem types in its API, a README and its own headless tests; spacers no longer show as widgets, and a click in the left half of a line's last character lands before it (gpui's `closest_index_for_x` gives the end). `publish = false` until gpui is released with the APIs it uses)

### 1.6 Localization and themes (§7.4, §7.5)

- [x] T1.6.1 `fluent` infrastructure; English and Turkish strings, shared by both frontends (`kalem_core::l10n` and the `tr!` macro; `locales/en` and `locales/tr` with command titles and categories, menus, status bars, messages, prompts and dialogs, panels and the date picker; the `ui.language` setting (system, English, Türkçe) in the settings panel, applied to menus and every window at once; yes and no prompts take the language's letters too; a test checks that every key has a translation)
- [x] T1.6.2 Light and dark themes in TOML, mapped to both frontends; follow the system (`kalem_core::theme`: the built-in `themes/light.toml` and `dark.toml`, and the user's `themes/` files changing any color, with problems logged; the graphical editor's colors come from them and follow the window's appearance; the terminal asks for its background color (OSC 11, shown by `kalem tui --detect`) and on true-color terminals uses the theme for headings, keywords, links, tags, timestamps, code, syntax, search marks and panels)
- [x] T1.6.3 Font selection; readable line width; focus mode (the settings panel's font list and the new `editor.code_font_family`; `editor.line_width` makes a centered text column in both editors, 0 for the whole window; focus mode (F8, `view.focus`) shows only the section holding the cursor, with `view::limit`, which also makes narrowing visible in both editors)

### 1.6a Plain text mode (§2.6)

- [x] T1.6a.1 Document modes in kalem-core: `DocumentMode` interface; Org, Markdown, CSV and plain text (§2.6); mode selection (user choice, `-*- mode: org -*-`, extension, shebang, fallback); until 2.7c and 2.7d land, Markdown and CSV files open as highlighted plain text (`DocumentMode::detect`, `name`, `title`, `from_name`; `DocumentState::set_mode` starts or stops the Org parse; "Set Document Mode" (`view.setMode`) remembers the choice for the file in the workspace's `.kalem/settings.toml` (`files.modes`), which comes first when the file opens again, in both editors)
- [x] T1.6a.2 Highlighting engine decision (D16) and `kalem-highlight` crate, shared with Org source blocks (done with T1.4.5: syntect with pure Rust regexes, spans with kinds for themes, Org language names; line states for incremental highlighting come with T1.6a.3)
- [x] T1.6a.3 Plain text view in both frontends: line numbers, current line, soft wrap, indentation guides (monospace text colored by its language with `kalem_highlight::Highlighter`, which keeps the parser state at each line start and after an edit highlights again only from the changed lines until the state is as before; line numbers (`editor.line_numbers`, also in the source view), the cursor's line marked, a guide at each indentation step (`text::detect_indent`); `editor.soft_wrap` and Alt+Z: without wrapping, one row a line that scrolls sideways to keep the cursor, in both editors; `tui-rich-text` scrolls rows sideways)
- [x] T1.6a.4 Line ending, BOM and indentation style preservation; binary file detection (line endings and the byte order mark were kept since T1.3.6; `DocumentState::indent`: Tab inserts the file's own indentation (a tab, or the spaces to the next step), indents and outdents selected lines, Shift+Tab outdents, with tabs by default for Makefiles and Go; binary files (a NUL byte or invalid UTF-8) are not opened, and the graphical editor says so)
- [x] T1.6a.5 Commands scoped by mode (when-clause `editorMode`) (every Org, table and list command, folding and narrowing need `editorMode == org`; the palette offers only the commands that apply; a test checks that none applies in a text file)

### 1.7 CLI (§7.7)

- [x] T1.7.1 `kalem fmt` (tables, blank lines per document style) with `--check` (`org_edit::format`: tables and tags aligned, a blank line before every headline where the document puts them, blank-only lines emptied outside verbatim blocks, one final line feed; line endings and the byte order mark kept; idempotent on the corpus, where it keeps every headline, table and source block; `--check` lists the files that would change and exits 1)
- [x] T1.7.2 `kalem query` with Org match expressions; text and JSON output (`kalem query FILE... MATCH`: `org-map-entries` match strings with group tags, as `FILE:LINE: HEADLINE` or `--format json` with level, TODO, priority, title, inherited and local tags)
- [x] T1.7.3 GUI/TUI dispatch: GUI when a display exists and no subcommand, hint otherwise; `kalem tui`, `kalem -t` (done with T1.5.1: `kalem FILE` opens the graphical editor where there is a display and the terminal one otherwise; `kalem gui`, `kalem tui`, `kalem -t`; subcommands go to the command-line tools)

### 1.8 Release 0.1 (§17)

- [~] T1.8.1 `cargo-dist` configuration; binaries for three platforms; full and terminal-only flavors (done: `[workspace.metadata.dist]` for macOS, Linux and Windows targets with shell, PowerShell and Homebrew installers; the `gui` and `tui` features of `kalem-editor`, the terminal-only build without gpui checked in CI; Linux libraries for gpui in CI (not run yet); `docs/releasing.md`. Open, for the maintainers: `dist generate` for the release workflow, and the first release)
- [x] T1.8.2 macOS `.app` bundle (unsigned acceptable for the first release; signing in phase 2) (`tools/macos-app.sh` and `packaging/macos/Info.plist`: Org documents as the app's own type, text, Markdown and CSV as alternates; files opened from Finder open in windows)
- [x] T1.8.3 First user manual (`docs/`, written in Org) (`docs/manual.org`: installing, starting, writing in Org, views, finding, keys (Word-like, Vim, your own), other files, settings and themes, files, the command line, bug reports; checked and formatted with `kalem check` and `kalem fmt`)
- [ ] T1.8.4 Early access: at least ten users from personas P1, P4 and P6; feedback form (needs the owner)
- [ ] T1.8.5 Announcement to the Org mailing list and related communities (the draft `docs/announcements/org-mailing-list-draft.md` matches 0.1; sending it is the owner's)
- [ ] T1.8.6 Publish `org-syntax` to crates.io (§4.7) (waits for decision D18 and the owner)
- [~] T1.8.7 README status: replace "pre-alpha, phase 0, nothing to use yet" with the real state (phase 1 done, phase 2 half done), a screenshot or GIF of both editors at the top, and a short "works today / not yet" list (review, 2026-09-28: the README contradicts the changelog and is the first thing a visitor reads) (done: the status, a works-today and not-yet list, the real command line, the crates and the build; open: the screenshot or GIF, which needs a desktop to record)
- [ ] T1.8.8 Make the repository public: issues on, "good first issue" and "help wanted" labels, the contact address of T0.1.9 in place first; the five dependabot pull requests merged or closed (owner)
- [ ] T1.8.9 Launch beyond the Org list (T1.8.5): Show HN, r/emacs, r/orgmode, r/rust and Turkish developer communities, one post each, only after signed binaries (T2.8.1, T2.8.2) and the GIF of T1.8.7 exist; the framing stays "not a replacement for Emacs, for the people around you" (review, 2026-09-28: one launch brings most of a first year's users; comparable projects sit at 2.6k (organice) and 2.8k (Orgzly) stars, Markdown editors at 13k (Zettlr) to 61k (MarkText))
- [ ] T1.8.10 One-page site at the domain of D7, exported from `docs/manual.org` with Kalem's own HTML back-end (§18.2 dogfooding): what it is, a GIF, download links, the manual

### Phase 1 exit criteria

- [x] The Org Manual source opens, is edited and saves without a diff, in both frontends (`org_manual_round_trip` tests of `kalem-ui` and `kalem-tui`)
- [x] Startup and keystroke latency targets (§15) measured and met: every target in `docs/performance.md`; the CI benchmark job is TS.3
- [ ] Feedback collected from ten external users

---

## Phase 2: Document author (§20, 4 months)

### 2.1 org-table (§8)

- [x] T2.1.1 TBLFM parser: left-hand side forms, flags, multiple formulas (`org_table::tblfm`, checked against `org-table-get-stored-formulas`)
- [x] T2.1.2 Expression parser (Pratt): arithmetic, references, ranges, remote references, constants, parameters (Org's substitution of references into the text, then Calc's notation parsed by a Pratt parser, as Emacs does)
- [x] T2.1.3 Function library (§8.2 list); duration arithmetic, dates (string operations are Lisp formulas in Org)
- [x] T2.1.4 Arithmetic at least as precise as Calc: integers of any size, decimals; Calc's default output precision; precision and format flags (§3.6)
- [x] T2.1.5 Evaluation order, cycle detection, iteration limit: Emacs's order rather than a dependency graph (§8.2: a graph changes results); `iterate` stops after 10 passes and reports tables that do not converge
- [x] T2.1.6 Elisp formula detection: preserve, do not evaluate, warn (the warning icon in the table is T2.1.8)
- [x] T2.1.7 Corpus of tables computed in Emacs; identical-results test: 928 tables (`tests/corpus/tables`, `tools/table-cases.py`), 3000 Calc formulas (`tools/calc-cases.py`); differences in docs/known-differences.org
- [x] T2.1.8 GUI and TUI: formula bar, `#ERROR` display, reference highlighting (the status bar shows the field's formula, why it is `#ERROR` and Lisp formulas; F2 edits it, `=` for the column, `:=` for the field; referenced fields highlighted)
- [x] T2.1.9 Sorting, CSV and TSV import and export (Sort Rows, Import Table, Export Table, a selection made into a table; checked against Emacs)
- [x] T2.1.10 `kalem table recalc` (`--iterate`, `--check`, the F9 command); `kalem fmt` table alignment complete
- [~] T2.1.11 Publish `org-table` to crates.io (§4.7): the crate is ready (no workspace dependencies, metadata, README); publishing is the maintainers' step
- [ ] T2.1.12 Spreadsheet notation for formulas (§8, needs a design decision by the owner, D21): `A1`-style cell references and the common Excel function names (`SUM`, `AVERAGE`, `MIN`, `MAX`, `IF`, `ROUND`, `COUNT`) accepted in the formula bar and translated to Org's `#+TBLFM` form, so the file stays Org and Emacs computes the same result; the bar shows either notation (`org.table_formula_dialect`) (review, 2026-09-28: an Excel user cannot write a formula in Calc notation)
- [x] T2.1.13 Automatic recalculation after a field is edited, off by default (`org.table_auto_recalc`, or a document keyword), with the same results as F9 (review, 2026-09-28: the spreadsheet expectation) (done: Tab, Shift+Tab and Enter out of a field recalculate as F9 does when `org.table_auto_recalc` is on or the document says `#+KALEM: recalc=auto`; `recalc=manual` wins over the setting; `#+KALEM:` keeps keys that are not formatting when the document font changes)
- [x] T2.1.14 Column and range statistics in the status bar for the selection in an Org table: count, sum, average, min, max, as §2.6.2 plans them for CSV (review, 2026-09-28) (done: a selection in a table gives, in both status bars, the count of filled fields in the rectangle from the anchor's field to the cursor's, and the sum, average, smallest and largest of its numbers)

### 2.2 org-math (§9.2)

- [x] T2.2.1 Integrate the engine chosen in D4 (RaTeX) behind an `org-math` trait; embed the KaTeX fonts; report `multline` and `\mbox` upstream or map them (`org_math::MathEngine`, `Ratex`; RaTeX 0.1.14 with its `embed-fonts`, resvg for pixels. `multline` is mapped to `gather`, `\mbox` to `\text`; the upstream reports are the project owner's step)
- [x] T2.2.2 Render LaTeX fragments and environments; cache (fragments in the line; an environment away from the cursor shows on its first line as one displayed formula; `org_math::Cache`, least recently used, and the GUI's images by formula, size and color)
- [x] T2.2.3 `\newcommand` subset from `#+LATEX_HEADER` (`\newcommand`, `\renewcommand`, `\providecommand`, `\def` and `\DeclareMathOperator` from `#+LATEX_HEADER` and `#+LATEX_HEADER_EXTRA`, turned into `\def` since RaTeX refuses to redefine KaTeX's own macros such as `\R`)
- [x] T2.2.4 Error display (source + red frame)
- [x] T2.2.5 GUI: `$` trigger, click to edit source, preview toggle (inside a formula, a popup shows it typeset as it is typed; a click on a formula puts the cursor inside; `view.toggleMath`)
- [x] T2.2.6 TUI: Unicode approximation; image through the graphics protocol when available (displayed formulas alone on their line and environments, in the terminal's colors; `view.toggleMath` shows the sources)
- [x] T2.2.7 Image snapshot tests (`crates/org-math/tests/render.rs`: 25 formulas against PNG snapshots, `KALEM_UPDATE_SNAPSHOTS=1` to renew)

### 2.2a Word processor formatting, Kalem's own (§3.7, asked by the owner on 2026-09-28)

- [x] T2.2a.1 Syntax and model: `kalem_core::rich`, spans as `@@kalem:…@@` … `@@kalem:end@@` export snippets (nested spans combine, unclosed ones end with the paragraph), `#+ATTR_KALEM: :align` for paragraphs, `#+begin_center` for centering; Emacs 30.1 opens such files and its HTML and ASCII exports leave the additions out
- [x] T2.2a.2 Commands: font, font size, grow and shrink through Word's sizes, text color, highlight, clear formatting, align left, center, right, justify; on the word at the cursor without a selection; Word's keys (Ctrl+L/E/R/J, Ctrl+] and Ctrl+[, Ctrl+Space), terminal keys where terminals cannot send them
- [x] T2.2a.3 GUI: fonts, sizes (text in its own size in `gpui-rich-text`, rows growing to fit), colors, highlights, right and centered lines; toolbar with font and size menus, A+ and A−, color and highlight swatches, alignment buttons and Clear
- [x] T2.2a.4 TUI: colors (24-bit or the nearest of 256), highlights, right and centered short lines
- [x] T2.2a.5 Editing keeps spans whole: deleting text keeps the snippets, Backspace and Delete step over them, a span left empty goes; the snippets and attribute lines never show in the rich view
- [x] T2.2a.6 Document defaults: the font, size and line spacing of a whole document (`#+KALEM: font="Georgia" size=12 spacing=1.5`; Document Font, Document Font Size, Line Spacing, and the toolbar's spacing menu)
- [~] T2.2a.7 Exports honor the formatting: HTML styles, LaTeX sizes and colors, ODT and docx through pandoc (with 2.3) (HTML done: spans as `<span style>` with font, size, color and highlight, closed where their paragraph, title or cell ends; `#+ATTR_KALEM: :align` as `text-align`; `#+KALEM:` defaults as the page's style; the color names shared with the editor through `org_export::kalem`. Markdown leaves them out as Emacs does. LaTeX: spans as groups with `\fontsize`, `\color` and `\colorbox` (xcolor), `\fontspec` for XeLaTeX and LuaLaTeX, `#+ATTR_KALEM:` as `flushright`/`flushleft` and `\vspace`, `#+KALEM:` as the document's size, `setspace` and main font. Open: ODT and docx, which pandoc writes from Org without Kalem's formatting (custom styles in a reference document, or pandoc's JSON, would carry it))
- [x] T2.2a.8 Justified lines in the editor view; space before and after paragraphs; the font menu searches fonts; recently used colors (done: justified paragraphs in both editors (the graphical editor stretches spaces to the wrap width in `gpui-rich-text`, the terminal widens them by cells); `#+ATTR_KALEM: :before 12 :after 6` with Space Before and After Paragraph in the palette and the toolbar's spacing menu, drawn in the graphical editor and exported as HTML margins, the attribute line keeping its other attributes; typing in the font menu searches the fonts; `format.recent_colors` and `format.recent_highlights` in the color menus and as the color prompt's start; the terminal's gaps in `docs/terminal-parity.org`)
- [~] T2.2a.9 Two file kinds (decided by the owner, 2026-09-28; §3.7): `.klm` joins `DocumentMode::detect` as Org with Kalem's additions (`fileKind == klm` in when-clauses, the kind in the status bar); `.org` is strict: the formatting commands of 2.2a are off there and, when invoked, offer to make the document a Kalem document (rename to `.klm`, links to it in the project updated) or to opt in with `#+KALEM: markup=yes`; the workspace setting `org.allow_kalem_markup`; `Kalem.app` and the Linux and Windows packaging register `.klm` (T1.8.2, 2.8); the manual's "Writing in Org" explains the two kinds (done: `.klm` detected as Org, `fileKind` in when-clauses, the kind in both status bars; in `.org` the formatting commands write nothing and offer Make Kalem Document (saved as `.klm` beside it, the `.org` removed, links in the project rewritten) or Allow Kalem's Formatting in This File (`#+KALEM: markup=yes`); `org.allow_kalem_markup`; an unsaved Org document counts as a Kalem document; `Kalem.app` registers `.klm`; the manual explains the two kinds. Open: Linux and Windows packages register `.klm` with 2.8)
- [x] T2.2a.10 Save as Org (`.klm` to `.org` without the additions, listing what was dropped) and Make Kalem Document (`.org` to `.klm`); `kalem export FILE.klm --to org`; `kalem check` warns about Kalem markup in a `.org` file, an error with `--deny-warnings`; `kalem fmt` never adds or removes the markup (File: Save as Org writes the `.org` beside the document and lists what it dropped; File: Make Kalem Document renames to `.klm` and updates the links to it in the folder's Org files; `kalem export --to org` reports what it dropped on stderr; `kalem check` gives `kalem-markup-in-org` for each addition in a `.org` file without `markup=yes`; `kalem fmt` keeps the markup and now also aligns tables with affiliated keywords; both editors, the commands being frontend-neutral)
- [x] T2.2a.11 Tests: a `.klm` renamed to `.org` parses, round-trips and exports as Org in Emacs (the differential of T0.4 over a `.klm` corpus); a `.org` saved by Kalem never contains `@@kalem:`, `#+ATTR_KALEM:` or `#+KALEM:` unless it opted in (a property test over the formatting commands); new syntax in `.klm` only through an RFC, checked by a test that every construct Kalem writes is one Emacs's org-element parses (`tests/corpus/klm`: `written.klm`, every construct the formatting commands write, kept by a test and parsed by `kalem diff-emacs` in CI with the rest of the corpus, and `mixed.klm` by hand; `crates/kalem-core/tests/klm.rs`: each parses and writes back byte for byte, has no warnings, and Emacs's HTML and Markdown exports of it (`tests/export/klm`) match Kalem's of its strict Org form; property tests: formatting commands never write into a `.org` file that did not opt in, and formatting never changes the emphasis Org reads (spans now go inside `*bold*` and around `=code=`, where a snippet beside the marker made Emacs read plain text); an RFC process for new syntax is a documentation matter)

### 2.3 org-export (§10)

- [x] T2.3.1 `ExportTree` and transcoder framework; filter points (`crates/org-export`: a port of `ox.el` on the `org-syntax` tree: `org-export-data` with memo, pruning, numbering, footnotes, fuzzy and ID links, smart quotes, table info)
- [x] T2.3.2 Common behavior: `#+OPTIONS`, `:noexport:`, `EXCLUDE_TAGS`, `SELECT_TAGS`, macros, `#+INCLUDE`, `#+SETUPFILE`, subtree export (all done; `#+SETUPFILE` and subtree export checked against Emacs 29.3 with Org 9.6.15, whose output differs from 9.7's only by a blank line before headlines, since Emacs 30 was not reachable from the session; `tests/emacs/export.el` exports the subtree with a `KALEM_TEST_SUBTREE` property; `kalem export --subtree`, Export Subtree as HTML and as Markdown)
- [x] T2.3.3 HTML backend (ox-html classes, CSS theme, formulas as SVG or MathJax) (the body matches Emacs byte for byte on every test case and on all 287 Worg files Emacs exports; the whole page follows `org-html-template`: doctypes and HTML5 (`html5-fancy` elements), meta tags, `#+HTML_HEAD`, home and up links, preamble and postamble, MathJax set-up, `HTML_CONTAINER` and `HTML_CONTENT_CLASS`, checked against Emacs in `tests/export/full`; Kalem's own style sheet (light and dark, print) replaces Org's; `tex:svg` draws formulas with the editor's engine as inline SVG images)
- [x] T2.3.4 LaTeX backend (ox-latex behavior, `#+LATEX_CLASS`, `ATTR_LATEX`, `%% org:LINE` comments) (`org_export::Latex`, a port of `ox-latex.el` 9.7: every transcoder, labels and captions, floats and `ATTR_LATEX` for tables, images, lists and blocks, math blocks and matrices, the article, report and book classes, the default packages per compiler, Babel and Polyglossia `AUTO`, the template with `\hypersetup`; the source block back-end is `verbatim`, as Org's default; the 21 LaTeX test cases and whole documents agree with Emacs, and all of `tests/corpus` (the Org manual, ORG-NEWS) but the attachment example; `%% org:LINE` comments with `Latex { source_lines: true }` and `kalem export --to latex --source-lines`; Export as LaTeX in both editors. Along the way: `org-entities` names given twice (`deg`, `sup`) take the first, and tables of contents use `ALT_TITLE`)
- [x] T2.3.5 Markdown backend (GFM) (ox-md matches Emacs on every test case and on all 287 Worg files Emacs exports; `gfm`, derived from it as the `ox-gfm` package does: pipe tables with alignment, fenced code blocks, `~~strike-through~~`; `kalem export --to gfm`, Export as GitHub Markdown)
- [x] T2.3.6 Plain text backend (`org_export::Text`, a port of `ox-ascii.el` 9.7 in its ASCII and UTF-8 charsets, with Emacs's `fill-region` reproduced (`org_export::fill`: fill column, two spaces after sentences, no break after a period and one space, adaptive prefixes, hard newlines, centered and right lines with tabs); the 21 test cases and whole documents agree with Emacs, and so do the Org manual, ORG-NEWS and all of `tests/corpus` but the attachment example; `kalem export --to txt|utf8`, Export as Plain Text in both editors, `export.text_charset`)
- [~] T2.3.7 PDF generation: detect `latexmk` / `xelatex`; optional tectonic download (D5); error mapping (done: `kalem_core::pdf`: `latexmk`, else the engine of `#+LATEX_COMPILER` run twice, else `tectonic`, found on `PATH`; `-file-line-error` logs read for errors, undefined references and citations and bad boxes, each at its Org line through the `%% org:LINE` comments; log files removed after a clean run; `kalem export --to pdf` prints `FILE.org:LINE: error: …` and fails on errors; Export as PDF (LaTeX) in both editors compiles in the background (`kalem_core::jobs`, polled by both frontends), opens the PDF with `export.open_after`, and shows the first error at its line; guidance when no LaTeX is installed. Open: the optional tectonic download, which waits for D5)
- [x] T2.3.8 Pandoc bridge: DOCX, ODT, EPUB, RTF output; DOCX, ODT, MD, HTML import; "Org cleanup" pass; pandoc detection and guidance (`kalem_core::pandoc`: pandoc reads the document with its `#+INCLUDE` and macros expanded and Kalem's markup taken out, from the file's folder; import from Word, OpenDocument, Markdown, HTML, EPUB, RTF and LaTeX with `--wrap=none` and pictures extracted to `NAME_assets`; the clean-up pass drops unreferenced `CUSTOM_ID` drawers and `<<anchors>>`, shortens links whose description repeats the target, and formats as `kalem fmt`; `kalem export --to docx|odt|epub|rtf`, `kalem import FILE [-o OUT] [--force]`; Export as Word/OpenDocument/EPUB/RTF in the background and Import as Org in both editors; guidance when pandoc is missing; a round trip through Word tested when pandoc is installed)
- [x] T2.3.9 Export dialog and settings (GUI and TUI); `#+EXPORT_FILE_NAME` (Export… (Ctrl+Alt+E, File menu) lists the formats, the subtree exports and the export settings with their values in both editors; `export.body_only`, `export.open_after` and `export.math` (MathJax or SVG) in `settings.toml`, changed from the dialog; files go where `#+EXPORT_FILE_NAME` or the subtree's `EXPORT_FILE_NAME` says, else beside the document)
- [~] T2.3.10 Snapshot tests; comparison corpus against ox.el output (`tests/export/cases` and the Worg corpus against Emacs 30.1 with Org 9.7.11; the counts only go up)
- [x] T2.3.11 `kalem export` subcommand (`kalem export FILE... --to html|md|gfm|latex|txt|utf8|pdf|docx|odt|epub|rtf|org`, `-o`, `--body-only`, `--subtree`, `--source-lines`)
- [ ] T2.3.12 Publish `org-export` to crates.io (§4.7)
- [ ] T2.3.13 PDF without TeX (needs a decision by the owner, D22): "Export as PDF" from the HTML back-end through a bundled or system renderer, so printing (T2.5.11) and "send it as a PDF" work before the LaTeX back-end and pandoc land; the LaTeX path stays the one for books and papers (§9.3) (review, 2026-09-28: today a document cannot leave Kalem as PDF, DOCX or ODT)

### 2.4 org-cite (§9.4)

- [x] T2.4.1 org-cite syntax parsing (style, variant, prefix and suffix, multiple keys) (the parser had them, identical to Emacs on the corpus; now `ast::Citation::style_and_variant`, `prefix`, `suffix` and `keys`, `CitationReference::prefix` and `suffix`, and `org_model::cite`: the document's citations, the keys cited in order, `#+BIBLIOGRAPHY` files and `#+CITE_EXPORT`)
- [x] T2.4.2 Read BibTeX with `hayagriva`; `#+bibliography` (the new `org-cite` crate: BibTeX and BibLaTeX read as `bibtex-parse-entry` reads them, `@string` and month abbreviations expanded, `#` concatenation, blanks squeezed as `oc-basic` does, and CSL-JSON as `oc-basic` reads it; checked against Emacs on a sample; `kalem check` warns about unreadable `#+BIBLIOGRAPHY` files and keys none of them has. hayagriva is kept for CSL rendering, T2.4.3, since Org's own reading keeps the raw field text)
- [x] T2.4.3 CSL rendering (HTML, plain text); `#+cite_export` (`#+CITE_EXPORT` with Org's `basic` processor, identical to Emacs on nine cases, and `csl` through hayagriva: bundled styles or `.csl` files, BibTeX, CSL-JSON and YAML, locators, variants, note styles, `csl-bib-body` HTML, `cslbibliography` LaTeX with its preamble; `org_cite::csl`, `org_export::cite`. Org's `csl` needs citeproc-el, so it has no Emacs reference output)
- [x] T2.4.4 Delegation to biblatex and natbib in LaTeX (`org_export::cite_latex`: `oc-natbib` and `oc-biblatex` commands, multicite commands, style table and shortcuts, `\printbibliography` options, `\bibliography` with `\bibliographystyle`, and the finalizers that load the package and add `\addbibresource`; identical to Emacs on four cases in all back-ends, whole LaTeX documents included; PDF export runs `biber` or `bibtex` without `latexmk`)
- [x] T2.4.5 UI: citation insert dialog (key search), hover preview (Insert Citation, `org.cite.insert`: a picker over the `#+BIBLIOGRAPHY` entries, found by key, author and title, that inserts `[cite:@key]` or adds `; @key` to the citation at the cursor; palette items can now carry arguments. The preview of the entry under the cursor is in the status bar of both editors and, in the graphical editor, in a tooltip under the mouse; the terminal's gap is in `docs/terminal-parity.org`)

### 2.5 Editor features (§2.2, §3.2 phase 2 column)

- [x] T2.5.1 Images: inline display, `#+ATTR_ORG: :width`, paste and drop → `_assets/`, org-attach compatibility (review, 2026-09-28: the graphical editor shows `[image: PATH]` in place of the picture; the terminal editor already draws images through the graphics protocol) (done: the graphical editor draws image links' pictures in the line (`kalem_ui::pictures`, decoded by `kalem_core::images::decode`: PNG, JPEG, GIF, WebP, BMP, TIFF and SVG through resvg, reloaded when the file changes), no wider than the text; `#+ATTR_ORG: :width` in pixels or as a share (`300`, `300px`, `50%`, `0.5`) in both editors; `attachment:` links resolved through the heading's `DIR` or `ID` as `org-attach` does; `~/` paths; a picture pasted from the clipboard, picture files pasted or dropped (a terminal pastes a dropped file's path) are copied into `NAME_assets/` and linked, pictures in the document's folder linked where they are; the terminal's gaps in `docs/terminal-parity.org`)
- [x] T2.5.2 Footnotes: insert, renumber, list at the end of the document, hover (`org_edit::footnote`, a port of `org-footnote.el`: `org-footnote-new` with its definition in the `Footnotes` section or at the end of the section, `-renumber-fn:N`, `-sort`, `-normalize`, `-delete` and `-action`; 1,268 cases identical to Emacs, 14 more where Emacs fails, in `docs/known-differences.org`; the commands in both editors with Ctrl+Alt+F and Insert > Footnote, `org.footnote_section`; the footnote's text for the reference at the cursor in both status bars and under the mouse in the graphical editor)
- [x] T2.5.3 Planning lines: edit SCHEDULED, DEADLINE, CLOSED; date picker; repeaters (`org_edit::todo::schedule`, a port of `org--deadline-or-schedule`: set with or without a time of day, the old repeater and warning kept, a new one taken from the input, `CLOSED` removed, and removal as `C-u C-c C-s`; 434 cases identical to Emacs; Schedule (Ctrl+Alt+S), Set Deadline, Remove Schedule and Remove Deadline in both editors, the date asked through the date picker in the graphical editor and a prompt in the terminal, `2026-10-05 +1w` style repeaters; `CLOSED` is written by the TODO commands and its timestamp edited like any other)
- [x] T2.5.4 Property drawer: key-value table editing (Edit Properties, `org.property.edit`, in both editors: the entry's drawer as `KEY: value` rows (`kalem_core::properties`); choosing a row asks for its value with the old one filled in (argument prompts now see the arguments already given), a row adds a property and one per property removes it; the drawer is written by `org_edit::property`, identical to Emacs's `org-entry-put`)
- [x] T2.5.5 Render and edit generic drawers, verse, export blocks, comment blocks (export blocks are `BlockKind::Export` with their back-end: `export html` on the first line, contents colored in that language (HTML, LaTeX, Markdown…), a copy button; comment blocks `BlockKind::CommentBlock`, dimmed; verse and quote blocks with a bar, drawers folded to their first line, as before; Insert Drawer (`org_edit::insert::insert_drawer`, `org-insert-drawer` with a name, at point or around the region; 154 cases identical to Emacs, 9 region cases that Emacs re-indents in `docs/known-differences.org`) in both editors)
- [x] T2.5.6 Live table of contents preview (`#+TOC`) (`kalem_core::toc`: a `#+TOC: headlines [N] [local]` line away from the cursor shows the headings the export lists there, numbered as the export numbers them (`COMMENT` and `:noexport:` subtrees left out, `:UNNUMBERED:` ones without numbers; checked against Emacs's ASCII export), `local` limited to the keyword's subtree; in the graphical editor a framed list whose rows jump to their heading, in the terminal editor a row a heading, clicked the same way; `#+TOC: tables` and `listings` stay as text)
- [x] T2.5.7 Affiliated keywords: edit `#+CAPTION`, `#+NAME`; insert cross references (`kalem_core::affiliated`: Set Caption and Set Name write the keyword of the element at the cursor (a table for a cell, a paragraph in an item, from its keyword lines too), replacing its lines by one, above the element with its indentation and case, an empty value removing it; the prompt starts with the current value; Insert Cross Reference picks among named elements (with their captions), `CUSTOM_ID`s, headings and targets and writes the link; `[[` completion offers named elements; both editors, Insert menu in the graphical one)
- [x] T2.5.8 Display macros, export snippets, targets and radio targets (macros away from the cursor show their expansion, in the link color and italic (`Style::expansion`), computed once per tree by `org_export::macros::expansions` with the export's templates (`#+MACRO:` with `$N`, `title`, `author`, `date`, `email`, `keyword`, `property`, `n`, `time`), nested macros expanded, undefined or looping ones left as written; export snippets hide `@@`, show their back-end dimmed and their contents as code; targets and radio targets hide their brackets and radio links are links (as before), opened by Open Link; both editors)
- [x] T2.5.9 Tabs (D12): multiple documents (decided by the owner, 2026-09-28: one window holds many documents; the open files are listed on the left or as tabs at the top (`ui.open_files`), grouped under their project, files outside every project one by one. Files open in the window (Finder, links, Open); next and previous follow the list; closing asks about unsaved changes, quitting about every document)
- [x] T2.5.10 Archiving and refile (within one file) (`org_edit::archive`: `org-toggle-archive-tag`, `org-archive-to-archive-sibling` (the `Archive` sibling found or made, the subtree pasted as its last child, `ARCHIVE_TIME`, the parent's TODO statistics, the cursor as Emacs leaves it) and `org-refile` to a heading of the same buffer (last child, the cursor where the subtree was); 434 cases identical to Emacs (the edit harness now fixes `format-time-string`'s clock too); Refile (Ctrl+Alt+W) with a picker of outline paths (`kalem_core::refile`), Archive to Sibling and Toggle Archive Tag in both editors and the Org menu. Archiving to another file (`org-archive-subtree` with `org-archive-location`) is not done)
- [x] T2.5.11 Printing: produce PDF and open the system print dialog (`file.print`: the LaTeX PDF compiled in the background, then `kalem_core::print`: Preview's print dialog through `osascript` on macOS, `Start-Process -Verb Print` on Windows, `evince --preview` or `xdg-open` on Linux, and without a display only a message naming the PDF for `lp`; Org documents only, and TeX is needed until D22 (T2.3.13) gives a PDF without it)

### 2.6 Clipboard (§10.3)

- [x] T2.6.1 "Copy as rich text": selection → HTML clipboard (`kalem_core::rich_copy`: the selection, or the document, exported as HTML body without table of contents or numbers, formulas as SVG; Copy as Rich Text puts `public.html` and the Org text on the macOS pasteboard (`Request::CopyRich`, `kalem_ui::clipboard::write_rich`), Copy as HTML the markup as text; both editors, the Edit menu; X11, Wayland and Windows HTML writers and the terminal (OSC 52 is text only) copy the plain text with a message, see `docs/terminal-parity.org` and T2.9.6)
- [x] T2.6.2 Extend the HTML paste converter (tables, lists, links, emphasis; merged cells, nested tables, emphasis next to word characters; HTML clipboard readers for X11, Wayland and Windows) (`kalem_core::paste`: `colspan` and `rowspan` leave their cells empty in place, a table in a cell becomes `a, b; c, d`, emphasis and code next to word characters get U+200B on the sides Org's `org-emphasis-regexp-components` needs (link descriptions' ends count as boundaries); `kalem_ui::clipboard::html` reads `text/html` through `wl-paste` on Wayland and `xclip` on X11, with a half-second limit and UTF-16 (Firefox) decoded. The Windows `CF_HTML` reader stays with T2.9.6)

### 2.7 Book writing validation (§9.4)

- [x] T2.7.1 Sample book chapter: figures, tables, formulas, citations, footnotes, cross references, `#+INCLUDE` (`examples/book`: `book.org` with a part and two chapters included with `:minlevel 2`, a PNG figure, a table with `#+TBLFM`, a named equation, a source block with a caption, `[cite:…]` with `refs.bib` and `#+PRINT_BIBLIOGRAPHY:`, footnotes, links to figures, tables, equations and `CUSTOM_ID`s; its whole HTML, LaTeX, text and Markdown exports are identical to Emacs's (`tests/export/book`, `org-export`'s `sample_book` test), which found the missing `\eqref` for links to math environments in HTML)
- [x] T2.7.2 Error-free export to LaTeX and PDF; export to HTML and DOCX (`kalem-core/tests/book.rs`: the sample book compiles to PDF with pdfLaTeX and `latexmk` without errors or undefined references, and exports to Word through pandoc with citations (`--citeproc`), the figure, the equation (math environments as `\[…\]`) and labelled cross references (`kalem_core::pandoc::prepare`); each part runs when TeX or pandoc is installed, which CI does not install yet)
- [x] T2.7.3 Word count targets, per-chapter statistics (`kalem_core::stats`: the document's target in `#+KALEM: word_target=` and a section's in its heading's `WORD_TARGET` property (`80000`, `80,000`, `80k`), shown with the counts in both status bars ("1,200 of 80,000 (1%) words, 900 of 4,000 (22%) in section"); Set Document Word Target and Set Section Word Target; Word Count by Chapter lists the headings of the first two levels with words and progress and goes to the one chosen, through the new Go to Line (`edit.gotoLine`, part of T2.7a.6); both editors, the View menu)

### 2.7a General purpose editing (§2.6)

- [x] T2.7a.1 Encodings: UTF-16 with BOM, "reopen with encoding" (encoding_rs) (`kalem_core::files`: `Metadata::encoding`; UTF-8 and UTF-16LE/BE with a byte order mark read and written back byte for byte; other text that is not UTF-8 read in the encoding chardetng guesses (a message says so) and saved in it, a character it cannot hold stopping the save (`SaveError::Unencodable`); reloading keeps the encoding; `looks_binary` no longer calls legacy-encoded text binary; Reopen with Encoding and Save with Encoding (a list of 17 encodings, any `encoding_rs` label accepted) in both editors and the File menu, the encoding in both status bars when it is not UTF-8. Found and fixed on the way: slicing `#+ATTR_KALEM:`, `#+begin`, `#+end_` and `:END:` prefixes inside a multi-byte character panicked)
- [x] T2.7a.2 Multiple cursors and column selection (`kalem_core::cursors`: `DocumentState::extra` beside the primary selection, kept sorted and merged and moved through every edit; typing, Backspace, Delete, Enter and pasting at every cursor as one transaction and one undo step (a paste with as many lines as cursors puts one at each), Copy and Cut take every selection one a line; Add Cursor Above and Below (Ctrl+Alt+Up and Down), Alt-click to add or remove one, Add Next Occurrence (Ctrl+D; the word at the cursor first), Select All Occurrences, Column Selection Down and Up (Ctrl+Alt+Shift+Down and Up, columns clamped to short lines), Single Cursor and Escape; the arrow, Home, End and page keys move every cursor; both editors draw them (the terminal as selected cells); undo leaves one cursor. Org's table and tag alignment while typing applies with one cursor only)
- [x] T2.7a.3 Large files: 100 MB target (§15), lazy highlighting, long-line safety; a rope or piece table for plain text mode, where the parser needs no contiguous text (T1.3.1a) (measured: a keystroke in a 100 MB plain text file costs 2.7 ms median and 14 ms p99 with contiguous text and its line index, so no rope or piece table (`text_timing` example, `docs/performance.md`); lazy highlighting: files over 4 MB colored a window of 400 lines at a time from a fresh state 200 lines before (`kalem_highlight::Windowed`) in both editors; long-line safety: a line over 16 KiB shows the 16 KiB around the cursor with `…` for the rest (`view::plain_line_view`), in plain text and Org, in both editors; `detect_indent` reads at most the first megabyte)
- [x] T2.7a.4 Workspace sidebar, fuzzy open file, find in files (project scope: T2.7f) (all done: the list of open files, Find File in Project, Search in Project, and the current project's folder tree below the open files in both editors, from the project's file index (ignored files left out), folders opened and closed with a click, Reveal in Folder Tree, `ui.folder_tree`)
- [x] T2.7a.5 Bracket matching, auto-indent, comment toggling per language (`kalem_core::code`: the bracket next to the cursor and its match marked in both editors (`pair_at_cursor`, memoized per version and cursor, 256 KiB each way), Go to Matching Bracket (Ctrl+Alt+B); Enter in code keeps the indentation, one level more after `{`, `(`, `[` (and `:` in Python, YAML and Nim), `{|}` split over three lines, a closing bracket typed alone on its line one level back; Toggle Comment (Ctrl+Alt+C) with the line marker of about 90 languages or block markers per line (HTML, Markdown, CSS, OCaml), uncommenting when every line is commented, in Org source blocks with the block's language and Org `#` comments elsewhere)
- [x] T2.7a.6 Everyday text commands missing from plain text mode: go to line, duplicate line, move lines up and down, join lines, sort selected lines, trim trailing whitespace on save (`editor.trim_trailing_whitespace`), select word and expand selection (review, 2026-09-28: Sublime Text and VS Code users expect them; none is in the command registry) (`kalem_core::lines`: Duplicate Lines (Ctrl+Shift+D), Move Lines Up and Down (Alt+Up and Down, plain text only: Org keeps them), Join Lines, Sort Lines (ascending or `reverse`), Trim Trailing Whitespace and `editor.trim_trailing_whitespace` applied before saving in both editors, Select Word, Expand Selection (Ctrl+Alt+Right: word, inside and with each pair of brackets, line, paragraph, whole text; Org by the syntax tree) and Shrink Selection (Ctrl+Alt+Left), and Go to Line from T2.7.3; both editors, a terminal note for Ctrl+Shift+D)
- [ ] T2.7a.7 Language packs (review, 2026-09-28): formats whose view is their source get no mode, only four hooks per language on top of highlighting, the same hooks plugins get (T3.1.9c, T3.1.9e): an outline provider, a formatter (`Format Document`, `kalem fmt --check`), completion, and diagnostics. First packs: diff and patch (files in the outline, hunks folded, a jump from a hunk line to the file at that line in the project, T2.7f); JSON, YAML, TOML and XML (outline from the structure, folding by node, `serde_json`, `toml` and `serde_yaml` formatting, Sort Keys, syntax errors in the status bar); ledger, hledger and beancount (amounts aligned on save and by `kalem fmt` as `org-table-align` aligns tables, account and payee completion, the balance of the transaction at the cursor in the status bar, reports through the external tool with the command shown first); gettext `.po` and Fluent `.ftl` (highlighting, Next Untranslated, the translated share in the status bar; dogfooded on `crates/kalem-core/locales`); developed in `getkalem/plugins` as plugins once the hooks exist (D29)
- [x] T2.7a.8 Completers in `kalem-core` (§11.12; asked by the owner, 2026-09-28): the contract (a when-clause; triggers by character, word prefix or request; asynchronous, cancellable `complete` with the prefix, the line, the paragraph, the syntax node and the language as context; items with edits, kinds and lazy documentation; merging and ranking of several completers, results as they come); the Org completions of `kalem_core::input` moved onto it; the document-words completer for every text file; one menu in both frontends; `kalem complete FILE:LINE:COL` (`kalem_core::completers`: the `Completer` trait (`applies` as its when-clause over the context, `trigger`: strings, a word prefix or a request, `complete` with the text near the cursor, its line and paragraph, the language at the cursor (a source block's in Org), the path and a `Cancel`, `resolve` for documentation, `slow` completers run on another thread with a copy of the megabyte around the cursor and polled by both frontends as their items arrive); items with a replaced range, cursor, kind, detail and source; merged, deduplicated and ranked (exact prefix, recently accepted, priority); `Registry` with the built-ins and `register` for plugins; Org's `#+`, `[[`, `[fn:` and tag completions moved onto it (applied as before, tags aligned); the document-words completer for every text file (three letters, nearest first, a megabyte around the cursor); one `Menu` in both editors, Alt+/ to ask, Tab to take a word (Enter goes on writing); `kalem complete FILE:LINE:COL`. The dictionary completer waits for spell checking (T3.x) and the script binding for T3.1.9c)
- [x] T2.7a.9 Command scope (§11.2; decided by the owner, 2026-09-28): `scope` on every command, `all` or a list of text types with `except`, compiled to the when-clause key `textType`, the innermost type at the cursor (the file's type, or the language of a source block, code fence, formula, export block or front matter; `klm` a subtype of `org`); registration refuses a command without a scope; every built-in command migrated, the formatting commands of 2.2a scoped to `klm`; the palette, menus, keymaps and `kalem run` evaluate it; `kalem commands --type TYPE`; the report of bindings that never apply lists unknown types; the manual lists commands by type; a test that no command lacks a scope (`kalem_core::command::Scope` (`all`, `only`, `except`, `org` taking in `klm`), required on `Command`: `register` refuses a command without one and folds it into `when` as a clause over `textType`; `DocumentState::text_type` gives the innermost type (a source block's language inside its contents, an export block's back-end, `latex` in formulas, `org` or `klm` by file kind, a file's language, `markdown`, `csv`, `text`, `directory`); every built-in command given a scope by `builtin::default_scope` (Org's editing commands `org`, the file manager's `directory`, files, views, exports, projects and statistics `all`) with explicit ones where the rule is not enough (the line commands `all except org`, which now work in Org source blocks); the palette, menus and keymaps evaluate it through `when`; keymap entries naming unknown types reported (`IssueKind::UnknownTextType`); `kalem commands [--type TYPE]`; the manual explains scopes and points to the listing; tests that every command has one and that the scopes decide in a source block. One deviation: the formatting commands of 2.2a are scoped `org` rather than `klm`, since running them in a strict `.org` file offers to make it a Kalem document or allow the markup (`kinds::offer_items`); `kalem run` does not exist yet)

### 2.7c Markdown mode (§2.6.1, D19)

- [ ] T2.7c.0 Order of phase 2 (needs a decision by the owner, D21): Markdown mode before the remaining Emacs-style extras (wdired, Dired search, Org text objects for Vim). The editing engine (hidden markers, tables, formulas, outline, export) is shared and only the parser (D19) is missing; the Markdown audience is many times the Org audience; MarkText (61k stars, unmaintained since 2022), Typora (paid, closed) and Zettlr (Electron) leave room for a fast native open source editor (review, 2026-09-28)
- [ ] T2.7c.1 Decide D19 (Markdown parser); spike pulldown-cmark's offset iterator on a Markdown corpus (CommonMark spec examples, GitHub READMEs): every block and inline range, round-trip untouched
- [ ] T2.7c.2 Markdown view model on the shared inline editing model: hidden markers with cursor reveal, headings, emphasis, code, links, images, footnotes
- [ ] T2.7c.3 Lists and task lists (clickable checkboxes), block quotes, code fences with highlighting (D16), front matter folded
- [ ] T2.7c.4 GFM tables edited in the grid shared with Org tables; math with the D4 engine
- [ ] T2.7c.5 Editing behaviors: autoformat triggers, Enter continues lists and quotes, outline sidebar from headings
- [ ] T2.7c.6 Incremental reparse from the enclosing top-level block; performance on 10 MB files
- [ ] T2.7c.7 "Convert to Org" and "Convert from Org" (exporter or pandoc); `kalem export FILE.md --to org`
- [ ] T2.7c.8 Both frontends; snapshot tests; byte-exact round trip of untouched text
- [ ] T2.7c.9 What Obsidian and Logseq users expect in Markdown files: wiki links `[[Page]]` resolved and completed within the project (T2.7f), front matter edited as a form, "copy as HTML" and "copy as rich text" (T2.6.1) (review, 2026-09-28)
- [ ] T2.7c.10 Markdown, CSV and LaTeX modes written against the document mode contract of §11.11, defined in `kalem-core` in phase 2 (`DocumentModeSpec`: detect, parse to ranges with the fixed kind vocabulary, grid, edit hooks, the language pack hooks, export); the script binding (T3.1.9g) wraps the same contract, so a plugin mode can do everything Markdown mode does (asked by the owner, 2026-09-28)

### 2.7d CSV mode (§2.6.2)

- [x] T2.7d.1 Dialect detection (delimiter, quoting, header row, line endings, encoding, BOM) and preservation; lazy record index with byte positions (`csv` crate) (`kalem_core::csv`, an own scanner rather than the `csv` crate, since editing needs the byte range of every field: `detect` from a sample, `Index` finds record starts only as far as asked; the encoding and byte order mark come from `files` as for every file)
- [~] T2.7d.2 Grid view in both frontends: virtualized rows, column widths, frozen header, cell editing (done: `csv::line_view` draws each line as a grid row, fields verbatim and padded to their column (widths from the first 1,000 records), `│` for delimiters, the header bold, only the lines on screen laid out; cells are edited in place; open: the frozen header, which needs a row pinned above the scrolled text in both editors)
- [~] T2.7d.3 Row and column operations: insert, delete, move; view-only sorting and filtering; explicit "sort file" (done: `csv.insertRow`, `deleteRow`, `moveRowUp`/`Down`, `insertColumn`, `deleteColumn`, `moveColumnLeft`/`Right`, `csv.sortFile` with `reverse`, Tab and Shift+Tab between fields; open: view-only sorting and filtering, which need a view whose rows are not the text's lines (`csv::sorted_order` is ready for it))
- [x] T2.7d.4 Minimal rewriting: only edited records change, quoting only where needed; undo through the normal transaction stack (every operation is a transaction touching only the fields it changes; `csv::encode` quotes only for the delimiter, quotes, line breaks or edge blanks)
- [x] T2.7d.5 Clipboard as TSV (spreadsheet interoperability); column statistics in the status bar (`csv.copyAsTsv`; tab-separated rows pasted get the file's delimiter; count, sum, average, min and max of the cursor's column through `formulas::selection_stats`, decimal commas read, in both status bars)
- [x] T2.7d.6 "Open as text"; "Convert to Org table"; 100,000-row files open quickly (`csv.openAsText`, `csv.convertToOrg` writing `NAME.org` beside the file; the lazy index lays out a 100,000-row file without scanning it)
- [x] T2.7d.7 Tests: dialect round trips, RFC 4180 edge cases (quotes, embedded newlines), large files (`csv` unit tests, `kalem-core/tests/csv.rs`, `csv_commands` and the `csv_grid` tests of both editors)
- [x] T2.7d.8 Interoperability tests with files saved by Excel and LibreOffice Calc: UTF-8 BOM, CRLF, `;` as the delimiter and `,` as the decimal separator (Turkish and most European locales), quoted numbers, dates; each opens, edits and saves without changing the untouched records (review, 2026-09-28) (`tests/csv`: an Excel file in a Turkish locale, a LibreOffice file with quoted text cells and an RFC 4180 file, each edited and saved in `kalem-core/tests/csv.rs` with every other byte kept)

### 2.7g More document modes (§2.6; review, 2026-09-28)

A mode is built in only when all four hold: what the reader sees differs from the source text (markers hidden, objects drawn, a grid, a tree), otherwise the file is plain text with a language pack (T2.7a.7) and no mode; the format is text and edited losslessly; it reuses an engine Kalem has (the rich view with hidden markers, the table grid, the outline, org-math, syntect, the exporter); and it is too deep for a plugin. Every other format is a plugin (T3.3.6), so that the "Swiss army knife" does not become ten half-finished modes (§19, scope creep; §1.4, not an IDE). Each mode below ships in both frontends, with mode detection (extension, mode line, shebang), snapshot tests and a section in the manual's "Other files". Every mode beyond Org, `.klm`, Markdown, CSV and LaTeX is developed in `getkalem/plugins` as a WASM plugin (D29, §11.8, T3.3.7), not in the core repository, which keeps the contract and its tests. LaTeX is in the core, group 2.7h (owner, 2026-09-28; §9.5).

- [ ] T2.7g.1 AsciiDoc and reStructuredText on the Markdown engine, after 2.7c: a parser with source offsets per format (a decision like D19 each: the AsciiDoc and reST parsing crates, or own parsers on the Markdown view model), headings, emphasis, lists, tables in the shared grid, admonitions and includes shown, front matter and directives folded; the audience is technical documentation (Antora, Sphinx)
- [ ] T2.7g.2 Log view: ANSI escape colors rendered, a follow mode through the file watcher (`tail -f`), a filter line with include and exclude regular expressions, timestamp detection for jumping to a time, all without laying out the whole file (T2.7a.3)
- [ ] T2.7g.3 Mode detection and tests for the modes above: extensions and mode lines in `DocumentMode::detect`, when-clauses for the commands of each mode (`editorMode == bibtex`), snapshot tests in both frontends, manual sections

### 2.7h LaTeX mode (§9.5; in the core, decided by the owner, 2026-09-28)

A first-class rendered editor for `.tex` files with the rigor of the Org mode. Goals: the file stays standard LaTeX byte for byte where untouched; everything an editing command generates is conventional LaTeX that compiles; what is not understood is shown as source, never hidden or guessed; the same experience in the graphical and the terminal editor. Non-goals: a typesetter of our own, a dialect, a translation of `.tex` to anything. Order of work: coverage first (T2.7h.1), compiling before beauty (T2.7h.22 to T2.7h.24 before the long tail of rendering), verification from the first week.

**Parser and model**

- [ ] T2.7h.1 Coverage spike, two weeks: a throwaway tokenizer over 1,000 arXiv sources stratified by field (mathematics, physics, computer science, biology, economics); the share of body text that falls inside the rendered subset against what stays source; the 200 most frequent macros and environments; the rendered subset decided from that data and recorded in `docs/decisions/D30-latex-subset.md` with the coverage target (T2.7h.36) (blocked here: the development containers cannot reach arxiv.org; T2.7h.2 went first, since the parser does not depend on the subset)
- [x] T2.7h.2 Lossless tokenizer and parser to ranges: TeX lexing with the standard category codes, comments to the end of the line, `\verb` and verbatim-like environments taken whole, groups `{}` and `[]` balanced, commands with mandatory and optional arguments from a signature table, environments nested; error tolerance (an unbalanced group ends at the paragraph, an unclosed environment at the section, as org-element tolerates unterminated blocks), no panics under fuzzing; incremental reparse from the enclosing top-level construct (section, environment, paragraph), identical to a full parse on random edits; targets: a 1 MB thesis under 100 ms, a keystroke under 2 ms (the `latex-syntax` crate: rowan trees whose text is the input byte for byte; a pass before parsing pairs braces and environments as TeX pairs them, so parsing stays linear on unbalanced input; `\makeatletter` followed; verbatim environments, `\verb`, `\lstinline` and `\url` taken whole; nesting past 1,000 levels stays tokens, reported; `Parse::reparse` parses the paragraph around an edit in the innermost closed environment and splices it, and falls back to a full parse when anything outside could change; property tests: lossless on random input, reparse equal to a full parse on random edits; 32 ms for a 1 MB paper, 0.64 ms p50 a keystroke (`--example timing`, `docs/performance.md`); cargo-fuzz targets come with T2.7h.31)
- [ ] T2.7h.3 Document model: preamble and body; `\documentclass` with its options and the packages loaded; the sectioning tree (`\part` to `\subparagraph`, starred forms, `\appendix`, `\frontmatter`, `\mainmatter`, `\backmatter`) with numbers as LaTeX numbers them (`secnumdepth`, the class's defaults); the tables of labels, references and citations; floats with captions; equation numbering as LaTeX numbers it (`equation`, `align`, `gather`, `multline`, `subequations`, `\tag`, `\nonumber`, `\notag`); theorems from `\newtheorem` with their counters; footnotes; `\newcommand`, `\renewcommand`, `\providecommand`, `\def`, `\DeclareMathOperator` and `\newenvironment` collected for org-math and for display; bibliography sources (`\bibliography`, `\addbibresource`, `\bibliographystyle`); the model cached and reused across edits as `org-model` is
- [ ] T2.7h.4 Multi-file projects: the root document from `% !TEX root`, `.latexmain`, a `\documentclass` search up the folder or the workspace setting; `\input`, `\include`, `\includeonly`, `subfiles` and `\import` followed, shown in the outline and opened as links; labels, references and citations resolved across files; `\graphicspath`; the project's file list from T2.7f

**Rendering, graphical and terminal**

- [ ] T2.7h.5 Text: sectioning as headings with their numbers; `\emph`, `\textbf`, `\textit`, `\texttt`, `\textsc`, `\underline`, `\textsuperscript` and `\textsubscript` with markers hidden away from the cursor; quotes and dashes (``` `` '' ```, `--`, `---`) shown as typographic characters; `\&`, `\%`, `\$`, `~`, `\,`, `\\` and `\newline`; `\maketitle` as a title block from `\title`, `\author` and `\date`; `abstract`, `quote`, `quotation`, `center`, `flushleft`, `flushright`; `\tableofcontents` as a live table of contents from the outline; `\begin{document}` as the visible boundary of the folded preamble
- [ ] T2.7h.6 Lists: `itemize`, `enumerate` and `description` with nesting, `\item[label]`, the counters of `enumerate` (`\arabic`, `\alph`, `\roman`, the nested styles), `enumitem` options ignored gracefully
- [ ] T2.7h.7 Math: `$…$`, `\(…\)`, `\[…\]`, `$$…$$`, `equation`, `align`, `gather`, `multline`, `eqnarray`, `alignat`, `flalign`, `subequations` and their starred forms through org-math, typeset in the line and as displayed formulas with their numbers; the preamble's macros applied; `\label` inside equations; a formula RaTeX cannot lay out shown as source in a red frame, as in Org; the popup preview while the cursor is inside; the common macros of `amsmath`, `amssymb`, `physics`, `siunitx` (`\SI`, `\num`, `\qty`) and `bm` mapped or shown as text; a table of unsupported macros kept in the docs
- [ ] T2.7h.8 Floats and graphics: `figure`, `figure*`, `table`, `wrapfigure`, `subfigure` and `subcaption` with captions and numbers; `\includegraphics` shown inline with the width read from `width=`, `scale=` and `height=` (fractions of `\textwidth` and `\linewidth` approximated); PNG, JPEG, PDF (rasterized through pdfium), SVG and EPS (converted when a converter is installed, a placeholder otherwise); `\centering`; a missing file shown as a diagnostic
- [ ] T2.7h.9 Tables: `tabular`, `tabularx`, `tabular*`, `array` and `longtable` with `booktabs` rules in the shared grid when the column specification is simple (`l`, `c`, `r`, `p{}`, `|`); `\multicolumn` and `\multirow` spans shown; `\hline`, `\toprule`, `\midrule`, `\bottomrule` and `\cline` as rules; complex tables as source; editing a cell writes the smallest edit, Tab moves between cells, the alignment of `&` in the source kept as the file has it
- [ ] T2.7h.10 References and citations: `\ref`, `\eqref`, `\pageref`, `\autoref`, `\cref`, `\Cref` and `\nameref` as chips showing the resolved number and caption, with hover; `\cite`, `\citep`, `\citet`, `\citeauthor`, `\citeyear`, `\parencite`, `\textcite`, `\autocite` and `\footcite` with several keys and pre- and post-notes, as chips with a hover card rendered from the BibTeX entry; `\footnote` inline with hover and a list at the end; `\url` and `\href` as links; unresolved references and keys as diagnostics
- [ ] T2.7h.11 Code, verbatim and comments: `verbatim`, `lstlisting`, `minted`, `\verb` and `\lstinline` highlighted by `kalem-highlight` (the language from the options); `\texttt` monospaced; comments dimmed; `\iffalse` … `\fi` and `comment` environments dimmed and foldable
- [ ] T2.7h.12 Theorems and structure: `\newtheorem` environments rendered with their names and numbers (`theorem`, `lemma`, `proof` with its end mark, `definition`, `example`); `\appendix` numbering; `\part` and `\chapter` in `book` and `report`; `\paragraph` as a run-in heading
- [ ] T2.7h.13 Unknown constructs: any macro, environment or package construct outside the subset shown as highlighted source, folded with a summary line when long, never hidden or guessed; `kalem check FILE.tex --unrendered` lists what a document leaves as source, which feeds the coverage metric (T2.7h.36) and the known limitations page
- [ ] T2.7h.14 Terminal editor: headings with glyphs, emphasis, lists, the Unicode approximation of formulas and images through the graphics protocols as in Org, chips as text with their targets in the status line, folding, the outline panel, the PDF's pages as images where the terminal draws them (T2.7h.24)

**Editing**

- [ ] T2.7h.15 Structural editing: Enter continues `\item` and ends the list on an empty item; Tab and Shift+Tab nest and unnest lists; `\begin{env}` inserts its `\end{env}`, and renaming one end renames the other; the heading level picker (Ctrl+1 to Ctrl+6) maps to the class's sectioning commands; promote, demote and move sections with their subtrees as in Org; toggles for `\emph`, `\textbf`, `\textit` and `\texttt` on the selection or the word, with nesting rules; optional typographic quotes and dashes while typing; every generated construct follows the document's own style (indentation, blank lines, `\label` placement), inferred as Org's style inference does
- [ ] T2.7h.16 Math editing: `$` and `\(` paired, `\left(` with `\right)`, input rules for `\frac{}{}`, `\sqrt{}`, `\sum_{}^{}`, matrices and `cases` with tab stops, inline to display and back, numbering toggled through the starred form, the preview popup as in Org
- [ ] T2.7h.17 Completers (§11.12) for LaTeX: commands and environments with their argument signatures from a built-in table and from the packages the preamble loads; `\cite` keys from the BibTeX sources with fuzzy search and hover; labels for `\ref` and `\cref` from every file of the project with their kind (figure, table, equation, section) and caption; file paths for `\input` and `\includegraphics`; package names and `\usepackage` options; the document's own macros; document words and the dictionary as everywhere; a texlab bridge later through the language server plugin
- [ ] T2.7h.18 Insert dialogs and commands: a figure with caption and label, a table (from the grid, with `booktabs` when the document uses it), an equation, a citation (searching the BibTeX grid), a cross reference (searching labels by kind), a footnote, a section, a package; all producing conventional LaTeX in the document's style
- [ ] T2.7h.19 BibTeX grid and integration: `.bib` files as a grid (key, type, author, title, year, the other fields on demand) with sorting and field editing, read and written through hayagriva with the untouched text kept; `\bibliography` and `\addbibresource` resolved; an entry added from a DOI (`net` permission, asked) or from BibTeX on the clipboard; unused and missing keys as diagnostics; hover cards rendered with a CSL style through hayagriva; the grid in both frontends
- [ ] T2.7h.20 Diagnostics (`kalem check FILE.tex` and in the editor): unbalanced groups and environments, undefined and duplicate labels, undefined citation keys, missing files and graphics, deprecated commands (`\bf`, `\it`, `$$`), a chktex-like subset, the unrendered constructs report; quick fixes where obvious
- [ ] T2.7h.21 Formatting (`kalem fmt FILE.tex`, `--check`): indentation of environments and `\item` as the document has it, optional alignment of `&` in `align` and tables, blank line normalization, verbatim untouched, idempotent on the corpus

**Compiling and preview**

- [ ] T2.7h.22 Compile pipeline: `latexmk` preferred; `pdflatex`, `xelatex` or `lualatex` from `% !TEX program`, the workspace setting or the packages loaded (`fontspec` means XeLaTeX or LuaLaTeX); `biber` and `bibtex` runs; tectonic on demand (D5) for machines without TeX; TeX Live, MiKTeX and MacTeX detected, with a hint on how to install one; an output folder; a build on save, debounced, or on request; cancellation; progress in the status bar; the multi-file root; `kalem latex build FILE` on the command line
- [ ] T2.7h.23 Log parsing: errors and warnings with file and line (nested files tracked through the log's parentheses), overfull and underfull boxes as warnings, undefined references and citations, missing packages with the install hint of the detected distribution; shown inline, in a problems panel and in the terminal's prompt line; jump to the error; `kalem latex build --json`
- [ ] T2.7h.24 PDF panel (with T4.3.2, pdfium): beside the text, zoom, page navigation, a dark inversion option, a refresh after each build that keeps the position; SyncTeX forward (cursor to page) and inverse (a click on the page to the source line), across files; in the terminal editor the current page as an image through the graphics protocols, or the system viewer
- [ ] T2.7h.25 Typst as a sibling: `.typ` highlighting, math preview and outline as a smaller mode, `typst compile` and `typst watch` in the same pipeline, the same PDF panel; never a translation of `.tex`
- [ ] T2.7h.26 Interop: `.tex` to HTML, Markdown and DOCX through the pandoc bridge (T2.3.8) for co-authors on Word; Org to LaTeX export (T2.3.4) opened and edited in this mode; "Convert to Org" for those who want it, one way and said so

**Templates, onboarding, language**

- [ ] T2.7h.27 Templates and classes: New from Template with `article`, `report`, `book`, `beamer`, `letter`, a CV and a thesis, the first localized one a Turkish thesis in the YÖK format; the class-specific commands of IEEE, ACM, Elsevier, Springer, RevTeX and arXiv styles recognized enough to render their front matter, or shown as source, never rewritten; Kalem never modifies a template
- [ ] T2.7h.28 Working beside Overleaf and co-authors: an Overleaf project cloned with git, edited locally, pushed back; external changes reloaded as today; a manual page "Kalem beside Overleaf"; a `.gitignore` for build outputs offered
- [ ] T2.7h.29 Spell checking in LaTeX (with T3.6.1): math, commands, labels, keys, verbatim and the preamble skipped; the language from the `babel` and `polyglossia` options (`turkish`, `english`, …); Turkish and English dictionaries first; a personal dictionary

**Verification and quality**

- [ ] T2.7h.30 Corpus: arXiv sources stratified by field, the redistributable ones (CC BY) committed under `tests/corpus/latex` and recorded in the license register, the rest fetched in CI by a script as the Worg corpus is; the classic templates of T2.7h.27; synthetic edge cases (nesting, unbalanced input, CRLF, BOM, Turkish and CJK text, huge equations)
- [ ] T2.7h.31 Round trip and fuzzing: byte-exact round trip on the corpus after random edit sequences; a `proptest` generator of LaTeX-like documents; `cargo-fuzz` targets for the parser and the incremental reparse, nightly in CI; incremental equals full parse on random edits
- [ ] T2.7h.32 Differential against pandoc: `pandoc -f latex -t json` on the rendered subset, a normalization of both trees, `kalem diff-pandoc` with `--summary` as `kalem diff-emacs` has, `docs/known-differences-latex.org` for the deliberate differences; the agreement counts only go up
- [ ] T2.7h.33 Compile and compare: tectonic in CI with `SOURCE_DATE_EPOCH` for reproducible PDFs; every corpus document compiled before and after an edit round trip gives the same PDF; every construct an editing command generates (lists, environments, sections, figures, tables, citations, references) compiles without new warnings on the corpus templates
- [ ] T2.7h.34 Math rendering corpus: a thousand formulas from arXiv against KaTeX-based snapshots, extending the 25 formulas of `crates/org-math/tests/render.rs`; the unsupported macros listed with their fallbacks; `\newcommand` expansion covered
- [ ] T2.7h.35 Performance: a 1 MB thesis opens under 200 ms, a keystroke under 2 ms incremental, 10 MB under 1 s, the PDF panel refreshes under 100 ms after a build, SyncTeX lookups under 10 ms; measured in `docs/performance.md` with the same tools as Org
- [ ] T2.7h.36 Coverage metric: the share of body text rendered rather than shown as source, per corpus document and per field, tracked in CI and published in the docs; the target set by T2.7h.1 (the working assumption is nine tenths for computer science and mathematics papers); the mode is not released below the target
- [ ] T2.7h.37 Release checks: the release checklist gains a LaTeX section (compiling on macOS, Linux and Windows with TeX Live and MiKTeX, tectonic on a clean machine, SyncTeX both ways, IME inside math, screen readers on chips and formulas, the terminal editor over SSH); terminal parity recorded in `docs/terminal-parity.org`
- [ ] T2.7h.38 Documentation: the manual chapter "Writing LaTeX", the known limitations page generated from the unrendered report, the Overleaf page, the templates page, a "for AUCTeX users" page mapping the familiar commands
- [ ] T2.7h.39 Exit criteria: a paper and a thesis from the corpus edited end to end and compiled; a co-author working in Overleaf on the same project notices no diff; the coverage target met; every check of this group green in CI; ten LaTeX writers among the early users of T1.8.4


### 2.7e File manager, like Emacs's Dired (§2.7, D20)

- [x] T2.7e.1 Decide D20 (file operation libraries: `trash`, std::fs, progress and cancellation); create the `kalem-fs` crate (`trash` with `NSFileManager` on macOS, own copy and move on std::fs; docs/decisions/D20-file-operations.md)
- [x] T2.7e.2 Listing model in `kalem-fs`: entries with type, permissions, size, time and name; sorting (name, time, size, extension, directories first); hidden files; inline subdirectories (all done; `i` lists the folder at the cursor below the listing with its path as a header, `K` takes it out with the folders listed in it, `^` from inside goes to its line; entries there are marked, opened and operated on like the others, new files go into the folder of the cursor's line; the terminal watches the listed folders too)
- [x] T2.7e.3 `directory` document mode in `kalem-core` and its view in both frontends: Dired-style `ls -l` and compact views, cursor on names, refresh through the file watcher keeping marks and cursor (a read-only text document; the terminal watches the folder, the graphical frontend checks it every second)
- [x] T2.7e.4 Navigation: open in the file's document mode or descend, parent directory, filter as you type, jump to the current file's directory (`dired-jump`) (the filter is a prompt, not live yet; a click on a name opens it)
- [x] T2.7e.5 Marks: mark, unmark, unmark all, toggle; by regular expression, extension, directories, changed since; flag for deletion and execute (all done; `* t` marks what changed since an age (`2h`, `3d`, `1w`), `today`, `yesterday` or a date)
- [x] T2.7e.6 Operations: copy, rename and move (across devices), delete to trash, permanent delete with confirmation, new directory, new file, symlink, permissions, touch; background work with progress and cancellation; per-file conflict choices (new file: `c`, a name ending with a slash makes a folder; F2 renames and Delete trashes, asked by the owner)
- [ ] T2.7e.7 Undo for renames, moves and trash deletions through the transaction stack
- [ ] T2.7e.8 Editable listing (wdired): rename with any editing command, commit applies all renames at once, including cycles and moves; duplicates and empty names reported before anything changes
- [ ] T2.7e.9 Search into listings: find by name, find in files (results listing and a jump-to-match view)
- [ ] T2.7e.10 Org integration: store and insert `[[file:…]]` links (relative in the same tree), drag and drop into documents (graphical frontend); attach to headings waits for org-attach (T3.6)
- [ ] T2.7e.11 Graphical extras: image thumbnails (`image-dired`) and a preview pane for Org, Markdown, text and images
- [ ] T2.7e.12 System integration: open with the system application, reveal in the system file manager, copy path, shell command on marked files with confirmation
- [x] T2.7e.13 Keymaps: Dired's default keys, and Vim-style keys when Vim mode is on (T2.7b) (Dired's keys come before Vim's in a listing, except the ones Vim needs to move and search; `g r`, `g g`, `Y` as in evil-collection)
- [~] T2.7e.14 Tests: operations on temporary directory trees (including symlinks, permissions, cross-device moves), wdired rename cycles, listing snapshots in both frontends (operations, links, permissions, conflicts, cancellation and both frontends end to end are tested; cross-device moves need a second device, wdired is still to do)
- [x] T2.7e.16 Switching in one step (asked by the owner, 2026-09-28): toolbar buttons, entries in the list of open files, a clickable hint in the terminal's status line, Ctrl+Alt+D there and back, Vim's `-`, `:Ex`, `:Projects`; the palette finds commands by English name and ID too
- [x] T2.7e.15 Projects view (asked by the owner, 2026-09-28): every project, and only the projects, listed as if in one folder; opening one lists its folder in the normal view, going up from there shows the projects again; `P` switches views

### 2.7f Projects, a simple Projectile (§2.8)

- [x] T2.7f.1 Project list in the user settings: add (current folder or a chosen one), remove, rename; missing folders shown; no `.git` or marker file needed (kept in `projects.toml` beside `settings.toml`, since it also holds recent files; Add Project Folder… chooses a folder in the GUI)
- [x] T2.7f.2 Project mode: on when the open file is inside a listed project (the innermost one for nested projects); project shown in the status bar (`inProject` in when-clauses); being in a project's file makes it the current project (switched to, its files listed at once), and a folder under version control joins the list when one of its files opens, as in Projectile (`projects.auto_add`; asked by the owner, 2026-09-28)
- [x] T2.7f.3 Switch project: fuzzy list, most recently used first; opens the project's last file or its file picker
- [x] T2.7f.4 `kalem-project` crate: background file walk with `ignore` (skips `.git` and friends, honors `.gitignore` and `.ignore`, skips binaries, extra patterns per project), kept current by the file watcher
- [x] T2.7f.5 Find file in project: fuzzy picker, recent files first (outside a project it offers the projects first, as Projectile does)
- [x] T2.7f.6 Search in project: string or regular expression, case and whole-word toggles, streaming results grouped by file, jump to match, cancellable (`grep-searcher`, `grep-regex`; Alt+C, Alt+W, Alt+R toggle; the search restarts as the query changes)
- [x] T2.7f.7 Recent files per project; "Open project in the file manager" (T2.7e) (Project Folder in the File Manager, `SPC p D`; the projects view)
- [x] T2.7f.8 Both frontends; tests on temporary project trees (ignore rules, nested projects, large trees)
- [x] T2.7f.9 Doom Emacs keys in the Vim profile (asked by the owner, 2026-09-28): `SPC p p`, `SPC p f`, `SPC SPC`, `SPC ,`, `SPC b b/k/n/p`, `SPC f f/r/s`, `SPC s p`, `SPC /`, `SPC :` and more in `keymaps/vim.json`; the leader is the setting `editor.vim.leader`; a which-key panel shows what may follow; `:e FILE`, `:bn`, `:bp`, `:bd`, `:ls`, `gt`, `gT`. Every binding is changed or removed in `keymap.json`, where `leader` also works

### 2.7b Vim mode (§7.3.1)

Moved up to phase 1 (owner, 2026-09-28): the Vim profile replaces the Emacs Org profile; the Emacs keys are an example user keymap, `docs/keymaps/emacs.json`.

- [x] T2.7b.1 Decide D17; modal input layer in kalem-core that maps key sequences to registry commands (own engine, `kalem_core::vim`: it works on the document state and hands registry commands (`:w`, `:q`) and Org edits to the frontends; `editor.keymap_profile = "vim"`)
- [x] T2.7b.2 Normal, insert, visual (char, line, block), replace modes; mode indicator in both frontends (all done: normal, insert, visual by characters, lines and blocks (Ctrl+V: `d x y c s u U ~ > <`, `I` and `A` typing on every line, `A` padding short lines, `o`), replace, the command line; the mode or command line in both status bars, the block painted as selected in both editors; a block cursor in both editors (a terminal cursor shape))
- [x] T2.7b.3 Motions, operators and counts; `.` repeat (`h j k l w b e W B E 0 ^ $ gg G f t F T ; , % { } + - _ H M L n N`, Ctrl+D/U/F/B; `d c y > < gu gU g~`, doubled for lines; `x X D C Y s S p P J r ~ u` Ctrl+R; `.` with a new count)
- [x] T2.7b.4 Text objects, including Org-aware ones (headline, subtree, item, cell, emphasis) (all done: `iw aw iW aW`, quotes, brackets, `ip ap`; in Org documents `ih ah` headline, `iR aR` subtree, `ii ai` list item, `ic ac` table cell, `ie ae` emphasis and code, from the current parse)
- [x] T2.7b.5 Registers, including the system clipboard (unnamed, `"a`-`"z`, `"A` appends, `"+` and `"*` the system clipboard; in the terminal, Kalem's clipboard sent on with OSC 52)
- [x] T2.7b.6 Vim in the WYSIWYG view: hidden markers, structural edit rules (§6.3), evil-org style bindings (optional) (all done: `>>` and `<<` demote and promote headlines and indent list items in Org documents; the Word-like keys in insert mode and for chords Vim leaves alone; in the rich view `h`, `l` and the arrows move over the shown text as the arrow keys do, hidden text such as Kalem's formatting snippets skipped and the cursor kept on a shown character, while operators act on the characters themselves (`dl`, `x`))
- [x] T2.7b.7 Setting `editor.vim.modes` (for example only plain text files) (empty for all documents)
- [ ] T3.7b.8 (phase 3) Macros, marks and jump list, command line (`:w`, `:s`, ranges), plugin-defined motions and text objects

### 2.8 Distribution (§17)

- [ ] T2.8.1 macOS signing and notarization; Homebrew cask and formula (terminal-only)
- [ ] T2.8.2 Windows MSI and signing
- [ ] T2.8.3 Linux AppImage and Flatpak
- [ ] T2.8.5 Package managers beyond cargo-dist's installers: winget and Scoop on Windows, Flathub, an AUR package by the community; all listed in the README's install section (review, 2026-09-28)
- [ ] T2.8.6 Building without Zed's repository (review, 2026-09-28): `kalem-ui` and `gpui-rich-text` pin gpui and `gpui_platform` to a git revision of Zed, so Cargo fetches the whole Zed repository for every build of the workspace, the terminal-only build and `cargo test -p org-syntax` included (in a fresh environment the fetch did not finish in ten minutes; the core crates built and passed their tests in under a minute once copied out of the workspace). The official `gpui` on crates.io is 0.2.2 from 2025-10-22, without AccessKit; two third-party snapshot lines of Zed's main branch are published weekly: `gpui-unofficial` with `gpui-platform-gpui-unofficial` (1.22.0-pre, by a Zed team member) and `gpui-pre` (0.3.7, a snapshot of the very revision Kalem pins, with no separate platform crate). Steps:
  - [ ] T2.8.6a Trial of a registry snapshot: `gpui = { package = "gpui-unofficial", version = "=1.22.0-pre" }` and the matching platform crate, so the code keeps its `gpui::` paths; `gpui-pre` as the second candidate if it covers `gpui_platform`'s features (font-kit, runtime_shaders, wayland, x11). Passes when the three platforms build, the 42 gpui tests and the latency test hold, AccessKit still reports the text, and `cargo tree --workspace` shows no `git+` source
  - [ ] T2.8.6b If no snapshot fits: vendor gpui and the Zed crates it needs (`gpui`, `gpui_macros`, `gpui_platform`, `util`, `util_macros`, `collections`, `sum_tree`, `refineable`, `http_client`, `scheduler`, `shared_string`) under `vendor/` as path dependencies, with their Apache-2.0 notices, and `tools/vendor-gpui.sh REV` refreshing them from a blobless sparse checkout, so no build ever clones Zed
  - [ ] T2.8.6c Fallback if both fail: the graphical frontend in its own workspace (`gui/`, its own `Cargo.lock`) with path dependencies on the core crates; the full `kalem` binary is built there for releases, the main workspace builds the terminal-only binary and never resolves gpui
  - [ ] T2.8.6d Exact version pins for the snapshot (`=`), upgrades on a schedule through TS.7 with the latency and snapshot tests as the gate, and a note in `docs/decisions/D3-ui-framework.md` on which line Kalem follows and why
  - [ ] T2.8.6e CI: a job that fails when `cargo metadata` lists a `git+https://github.com/zed-industries/zed` source (the terminal-only job already checks that gpui is absent from its tree); `cargo test -p org-syntax` timed on a cold cache as the measure of contributor setup
  - [ ] T2.8.6f Until one of a to c lands: CONTRIBUTING states the clone size and time, and `docs/decisions/D3-ui-framework.md` records the git pin as a known cost
- [ ] T2.8.7 Leaving gpui altogether (needs a decision by the owner, D23): a bounded spike like T0.6, three weeks at most, only if the official crate stays unreleased and the snapshot lines break twice in a row, or Zed's terms change. What a replacement must give: windows on macOS, Windows, X11 and Wayland; GPU text rendering with shaping and fallback fonts; IME with marked text; clipboard, file dialogs and menus; AccessKit; a headless test platform for the 42 gpui tests; the latency targets of §15. What stays: `kalem-core` and the terminal editor untouched; `gpui-rich-text` (781 lines) re-implemented on the new text stack; `kalem-ui` (about 10,000 lines, 39 gpui types used) rewritten. Candidates: winit + wgpu + parley + vello (Linebender), floem, iced, egui, slint; Tauri + ProseMirror stays rejected (T0.6.8) The execution plan is 2.9 (review, 2026-09-28)
- [ ] T2.8.4 Release 0.2

### 2.9 Leaving gpui (§7.1, D23)

Runs only after T2.8.7 says go; it does not gate the phase 2 exit. Cost: about the work of T1.5 again, months for one developer. What `kalem-ui` (about 10,000 lines) and `gpui-rich-text` (781 lines) take from gpui today, counted in the code (review, 2026-09-28): windows with title bar options; `div` layout with `Styled`, `deferred` and `anchored` overlays, `list` for the virtualized lines, one `canvas`; text shaping through `shape_line` and the text system (16 and 12 uses); key events and keystrokes (22), mouse and scroll (9), focus handles (11), IME through the element input handler (7); native menus (32); clipboard items (8), file and message prompts (9), drag and drop of files and headings (13), system appearance (7), opening URLs and paths (3); formula and image rasters through `RenderImage` (7); AccessKit nodes (one site); the 42 tests in `crates/kalem-ui/tests` on gpui's headless test platform, and the latency benchmark.

- [ ] T2.9.1 A seam before the move: `kalem-ui` split into the editor logic (rows from the view model, hit testing, commands, the panels' state) and a thin platform layer, so that gpui types are named in at most three modules; the tests of `crates/kalem-ui/tests` run against the layer, not against gpui. This is the "thin UI layer" the design's risk table promises (§19) and it pays off whether or not the move happens
- [ ] T2.9.2 Stack spike on the D3 benchmark (T0.6.4): the same 117,850-line file at 120 Hz, typing, 9,000 formulas, IME, inline widgets, AccessKit, binary size, cold build time, license and maintenance, for winit + wgpu + parley + vello (Linebender), floem, iced, egui and slint; report in `docs/decisions/D23-ui-framework-revisited.md`; go or no-go on the goals of T0.6.8
- [ ] T2.9.3 Text stack: shaping, fallback fonts, emoji and CJK, subpixel positioning and hinting per platform, fonts from the system and the embedded KaTeX fonts (T2.2.1); `gpui-rich-text` becomes `kalem-rich-text` on the new shaper with the same interface (rows, widget boxes, hit testing, caret) and the same snapshot tests; right-to-left text, which gpui does not give (D3 risks), as a new goal
- [ ] T2.9.4 Windows and painting: windows on macOS, Windows, X11 and Wayland with HiDPI, the system's light and dark appearance, title bar options; painting of glyph runs, rectangles, underlines and strike-through, fold arrows, formula and image rasters; a line-granular virtualized list with today's scroll model; the frame budget of §15 kept
- [ ] T2.9.5 Input: key events with modifiers and the chords of `kalem_core::keys`, dead keys and AltGr, IME with marked text, replacement ranges and the candidate window position (the contract of the element input handler), mouse with double and triple click, wheel and trackpad scrolling, drag of headings in the outline and drop of files from the system
- [ ] T2.9.6 Platform services: clipboard with text and HTML on the three platforms (with T2.6.2), open and save dialogs, message prompts, native menus on macOS and a menu bar elsewhere, opening URLs and revealing files, the file associations of `Kalem.app` kept (T1.8.2)
- [ ] T2.9.7 Panels rebuilt on the new toolkit: toolbar, status bar, outline sidebar, open files list and tabs, command palette, find and replace bar, completion menus, formula popup, date picker, settings panel, split view, the dialogs for unsaved changes and file conflicts, the file manager and the project pickers; the JSON widget tree of D11 as the description they share with plugins
- [ ] T2.9.8 Accessibility: an AccessKit adapter for the new window stack (`accesskit_winit` if winit) with text, caret and selection as today; the screen reader rows of `docs/release-checklist.md` pass on VoiceOver, Orca and NVDA
- [ ] T2.9.9 Tests: a headless platform for the new stack equal to gpui's test platform, the 42 tests of `crates/kalem-ui/tests` ported, rendering snapshot tests as the terminal editor has, the latency benchmark on the new stack with every row of `docs/performance.md` met again
- [ ] T2.9.10 Cutover: the new frontend behind a Cargo feature (`gui-next`) beside `gui` until the release checklist passes on macOS, Linux X11 and Wayland, and Windows; then gpui, `gpui-rich-text` and the git pin removed, `cargo tree` free of gpui and Zed, the apt list in CI and CONTRIBUTING updated, D3 marked as superseded by D23, a CHANGELOG entry
- [ ] T2.9.11 Exit: no gpui or Zed source in the dependency graph, every §15 target met, the release checklist green on the four window systems, the binary under 40 MB

### Phase 2 exit criteria

- [ ] A book chapter exports to LaTeX and PDF without errors
- [ ] The table corpus computes identically to Emacs
- [ ] Signed packages on three platforms
- [ ] The LaTeX mode meets its coverage target on the corpus (T2.7h.36), and a paper and a thesis from the corpus are edited end to end and compiled with no diff a co-author would notice (T2.7h.39)

---

## Phase 3: Extensibility and tasks (§20, 4 months)

### 3.1 kalem-script (§11)

- [ ] T3.1.0 WASM runtime spike (D28): wasmtime (Cranelift, and the Pulley interpreter) against wasmi: binary size, cold instantiation, a Markdown parser component on the 10 MB corpus per keystroke, fuel and memory limits, the precompiled module cache, one instance per thread, component model and WIT tooling (wit-bindgen, jco); `docs/decisions/D28-plugin-abi.md`; go or no-go against the lightness target of §15, the full binary under 40 MB
- [ ] T3.1.1 WASM host in `kalem-script`: the engine of T3.1.0, the component model, the WIT API as the single definition (D6), capability imports only, fuel and memory limits per instance, a precompiled cache in the state directory, lazy instantiation on activation events
- [ ] T3.1.2 Guest runtimes: none (D28; D10 closed). `kalem plugin new` and `kalem plugin build` wrap the Rust toolchain and the WASM target
- [ ] T3.1.3 API definition source (D6, decided with D28): WIT as the single definition; the generated Rust bindings published as the `kalem-plugin` crate; a consistency test that every extension point of §11.10 to §11.12 is in the WIT
- [ ] T3.1.4 `kalem` namespace: `command`, `run`, `keymap`, `on` (`scope` required on `kalem.command`, §11.2)
- [ ] T3.1.5 `kalem.ui`: notify, prompt, confirm, quickPick, statusBar, panel (JSON widget tree rendered by both frontends, D11)
- [ ] T3.1.6 `kalem.settings`, `kalem.fs` (with permission), `kalem.net` (with permission)
- [ ] T3.1.7 `editor` namespace and the `Document`, `Headline`, `Table`, `Selection` interfaces
- [ ] T3.1.8 `kalem.exporter` (backend and filter registration), `kalem.tables.registerFunction`, `kalem.babel.registerLanguage`
- [ ] T3.1.9 Event bindings; veto and timeout (500 ms)
- [ ] T3.1.9a Extension points (§11.10): link types (resolve, open, hover, complete, render, export)
- [ ] T3.1.9b Extension points: block renderers for special blocks and src languages (widget tree and SVG, both frontends, export)
- [ ] T3.1.9c Extension points: decorations, completers and hover providers (§11.12, over the contract of T2.7a.8; the template and the page "Writing a completer"), input rules
- [ ] T3.1.9d Extension points: document views (editor area and panel) and dynamic blocks
- [ ] T3.1.9e Extension points: diagnostics (shown in both frontends, run by `kalem check`), importers, paste handlers
- [ ] T3.1.9f Extension points: agenda views, capture templates, themes, plugin CLI subcommands
- [ ] T3.1.9g Extension points: document modes and highlighters (§11.11): `kalem.modes.register` over the contract of T2.7c.10; `kalem.modes.registerHighlighter` loading Sublime syntax definitions from the plugin folder; declarative modes from a syntax definition and a scope-to-kind mapping; the tree crossing the component boundary as flat arrays of kinds and ranges; the budget with the plain text fallback and disabling after repeated failures; `kalem check`, `fmt` and `export` calling the hooks; the conformance suite (byte-exact round trip, incremental equals full parse, snapshots in both frontends, budget) (asked by the owner, 2026-09-28)
- [ ] T3.1.10 Permission model: manifest declaration, first-run consent, scopes, `plugins.toml` record
- [ ] T3.1.11 Time limit (fuel metering, 100 ms synchronous) and memory limit (64 MB per instance); the plain text fallback of §11.11 when a parse misses its budget
- [ ] T3.1.12 Plugin loader: manifest, activation events, `activate` and `deactivate`, Disposable collection, ES module resolution (plugin folder only)
- [ ] T3.1.13 Error isolation: plugin console, disabling after repeated failures
- [ ] T3.1.14 Load `keymap.json` (D9: no script file)
- [ ] T3.1.15 Threads: one component instance per thread, several instances of one plugin for parallel parsers, renderers and completers, messages through the host
- [ ] T3.1.16 Batch mode `kalem run PLUGIN COMMAND [FILE...]`: headless API, graceful UI degradation
- [ ] T3.1.17 API contract tests; d.ts consistency test; limit tests

### 3.2 Live runtime (§11.9)

- [ ] T3.2.1 Inspection panel in both frontends: loaded plugins with permissions, budgets and recent errors, the command registry, recent timings; `kalem.inspect.*` (no REPL, D28)
- [ ] T3.2.2 Hot reloading: a plugin's component reloaded without a restart, the plugin folder watched during development, old Disposables cleaned up
- [ ] T3.2.3 `kalem --debug-socket`: the inspection commands and the end-to-end test driver over a local socket; no `kalem repl` (D28); localhost only, off by default
- [ ] T3.2.4 `kalem.inspect.*`: tree, commands, timings
- [ ] T3.2.5 End-to-end test driver over the socket

### 3.3 Plugin ecosystem (§11.5, §11.8)

- [ ] T3.3.1 Plugin template in `getkalem/plugins` (`template/`: a Rust crate against the generated WIT bindings, manifest, conformance tests, CI); `kalem plugin new` copies it
- [ ] T3.3.2 Example plugins: word count, Pomodoro, a custom export filter, a table function, a custom link type (`jira:`), a mermaid block renderer; the two reference completers of §11.12: a word list (declarative) and the LSP bridge (an out-of-process plugin over JSON-RPC that registers a completer, hover and diagnostics for its languages, installed by the user)
- [ ] T3.3.2a Bundled plugins built only on the public contracts (§11.0, D29): the kanban board view and the word count panel in the core repository, embedded as WASM components loaded on first use; the modes of 2.7g, the language packs and the further completers developed in `getkalem/plugins` and, if the owner wants them out of the box, embedded by the release workflow as a pinned set (tag and hash, §11.8); a test that no bundled plugin uses anything outside the WIT API
- [ ] T3.3.3 `kalem plugin install NAME|URL|FILE` (the index, a release asset, a local `.wasm`; hash and signature checked, permissions shown), `kalem plugin build GIT_URL` from source, `kalem plugin verify` (rebuild and compare), `kalem plugin list`, `kalem plugin remove`
- [ ] T3.3.4 Plugin API documentation (mdBook chapter); a "A plugin is a component, not a program" page; the page "Writing a mode" (§11.11)
- [ ] T3.3.5 Security policy (`SECURITY.md`)
- [ ] T3.3.6 Modes as plugins, through the document mode contract of §11.11 (T3.1.9g) (review, 2026-09-28): the formats that fail one of the four rules of 2.7g come as example or community plugins: todo.txt and TaskPaper on the Org task model with checkboxes; SVG source with a live preview (the SVG block renderer of T3.1.9b); Mermaid, DOT, PlantUML and D2 previews through the external tools; Fountain, gemtext and Djot on the rich view; `.srt` and `.vtt` subtitles as a grid with a time column; `.ics` and `.vcf` fed into the agenda (T3.5); `.eml` and mbox as a read-only view; `.ipynb` imported to Org with source blocks through the importer extension point (T3.1.9e); a read-only hex view for binary files as the one exception to §2.6, if the owner wants it; the first two, gemtext (declarative) and Djot (programmatic), are the reference plugins of §11.11, built from the template of T3.3.1
- [~] T3.3.7 The `getkalem/plugins` repository (owner, 2026-09-28; §11.8): a Cargo workspace with a crate per plugin, `template/`, CODEOWNERS per plugin, the project's license; CI that builds every plugin against the current WIT on each pull request and runs the conformance suite, and on a tag builds each `.wasm` from the tagged source, hashes and signs it (sigstore through GitHub OIDC or minisign), publishes it as a release asset and a ghcr.io package, and regenerates `index.json`; compiled components are never committed; Kalem reads the index as a static file (skeleton in place, 2026-09-28: the workspace, the template crate with its manifest and conformance tests, `index.json`, `tools/build-index.py`, the CI and release workflows; the release workflow has not run yet, and the `kalem-plugin` bindings wait for T3.1.3)

### 3.4 org-babel (§12)

- [ ] T3.4.1 Header argument parsing (`:results`, `:exports`, `:var`, `:dir`, `:cache`, `:tangle`, `:file`)
- [ ] T3.4.2 Executor interface; subprocess management; cancellation; progress
- [ ] T3.4.3 Languages: shell, python, javascript (node), R, gnuplot, sqlite, org
- [ ] T3.4.4 Trust model: document consent, trust bound to path and hash, never automatic execution
- [ ] T3.4.5 `#+RESULTS:` insertion rules; `#+NAME` matching; replace/append/prepend
- [ ] T3.4.6 `#+CALL:` and inline src
- [ ] T3.4.7 Tangling: confirmation list, file writes; `kalem tangle`
- [ ] T3.4.8 UI: run button, result block rendering, gnuplot image
- [ ] T3.4.9 Unit tests with fake executors; integration tests with real interpreters (optional in CI)

### 3.5 org-agenda (§13)

- [ ] T3.5.1 Workspace concept; `.kalem/settings.toml`
- [ ] T3.5.2 Indexer: background scan, updates via `notify`; in-memory index (D8)
- [ ] T3.5.3 Daily and weekly agenda views: SCHEDULED, DEADLINE, active timestamps, repeaters, warning delays
- [ ] T3.5.4 TODO list; tag and property match syntax subset; full-text search
- [ ] T3.5.5 Actions from views: jump, change TODO, reschedule (drag and drop)
- [ ] T3.5.6 Capture templates (TOML) and a quick note window
- [ ] T3.5.7 Refile: headline picker across the workspace
- [ ] T3.5.8 Clock: clock in and out, `:LOGBOOK:`, totals, running clock indicator
- [ ] T3.5.9 `id:` link resolution (workspace-wide)
- [ ] T3.5.10 `kalem agenda` subcommand

### 3.6 Other (§2.2, §10.1)

- [ ] T3.6.1 Spell checking: `spellbook`, Hunspell dictionary loading, Turkish and English, inline marking; the dictionary completer of §11.12 on the same word lists, with frequency lists where they exist, by `#+LANGUAGE`, the setting or detection
- [ ] T3.6.2 reveal.js backend
- [ ] T3.6.3 Beamer backend
- [ ] T3.6.4 Template picker (`#+SETUPFILE` library)
- [ ] T3.6.5 Release 0.3

### Phase 3 exit criteria

- [ ] At least three community plugins
- [ ] The agenda in daily use (including our own)
- [ ] A gnuplot chart produced in a document through Babel and exported

---

## Phase 4: Maturity (§20)

### 4.1 Scripting and plugin layers

- [-] T4.1.1 A second scripting language: closed with D10 (owner, 2026-09-28); no scripting language ships
- [-] T4.1.2 WASM plugins through `extism` for heavy work: superseded by D28, WASM is the plugin ABI from phase 3 (T3.1.0, T3.1.1)
- [ ] T4.1.3 Out-of-process protocol: JSON-RPC over stdio; example Python plugin
- [ ] T4.1.4 Plugin index (JSON) and in-app browser; API version compatibility

### 4.2 Org coverage

- [ ] T4.2.1 Babel `:session` (persistent REPL) and `:noweb`
- [ ] T4.2.2 Column view (`#+COLUMNS`)
- [ ] T4.2.3 org-habit graph
- [ ] T4.2.4 Evaluate diary sexp timestamps in the agenda
- [ ] T4.2.5 Full inlinetask rendering; dynamic block updates (`#+BEGIN: clocktable`)
- [ ] T4.2.6 org-crypt plugin (community)

### 4.3 Product

- [ ] T4.3.1 Presentation mode: top-level headline = slide, full screen, read-only (both frontends)
- [ ] T4.3.2 Embedded PDF preview panel (pdfium) (moved up: needed by T2.7h.24)
- [ ] T4.3.3 Automatic updates
- [ ] T4.3.4 Accessibility: basic screen reader support; full keyboard navigation
- [ ] T4.3.5 Performance tuning: every target in §15; memory profiling
- [ ] T4.3.6 Typewriter mode; additional themes
- [ ] T4.3.6a Remote directories in the file manager (SFTP, like TRAMP), as a plugin or a built-in (§2.7)
- [ ] T4.3.6b Model-backed completer (§11.12): off by default, enabled per workspace through the permission model with a visible indicator, document text leaving the machine only after that consent; a `kalem.net` completer plugin as the reference
- [ ] T4.3.7 Release 1.0

---

## Continuous work (every phase)

- [x] TS.1 English translation of the design document and this list (done 2026-09-27; everything is English from now on)
- [ ] TS.2 `CHANGELOG.md` updated in every PR
- [ ] TS.3 Benchmark regressions block PRs in CI
- [ ] TS.4 Grow the corpus and keep the license register
- [ ] TS.5 Keep the known differences document current; report Org Syntax ambiguities upstream
- [ ] TS.6 User manual and plugin API docs (mdBook, Org sources, exported with Kalem)
- [ ] TS.7 Dependency updates; track gpui, ratatui and the WASM engine versions
- [ ] TS.8 Community: good first issues, PR reviews, release notes
- [ ] TS.9 Ecosystem components (§4.7): review spin-out readiness at every release; prefer upstream contributions
- [ ] TS.10 Hardening: `unwrap` and `expect` outside tests reviewed crate by crate, `clippy::unwrap_used` and `clippy::expect_used` warned in the library crates with `#[expect]` where a panic is the right answer; fuzz targets for `org-edit` commands and `org-table` formulas beside the parser's two (review, 2026-09-28: about 900 `unwrap` and `expect` calls under `crates/`, tests included)
- [ ] TS.11 Users before features: no phase starts before the previous phase's user criterion is met (phase 1 asks for ten external users, none so far, while phase 2 is half done); the remaining phase 2 items are ranked by what those users ask for, and Emacs-flavored extras (Dired, Projectile, Doom keys) wait behind them (review, 2026-09-28: the design's own scope creep risk, §19)
- [ ] TS.12 From the first public release on, commits stay small and are not squashed, so contributors can bisect and read why a change was made (review, 2026-09-28: the history holds two commits today)
- [ ] TS.13 Terminal parity (§4.1, principle 7; asked by the owner, 2026-09-28): every feature covers both frontends in the same change, or its terminal form and the remaining gap are recorded in `docs/terminal-parity.org` (fonts, sizes, pixel layout and the like); the pull request template asks; the terminal section of the release checklist walks the list; a review at every release of what the graphics protocols, OSC 8 and OSC 52 now make possible

---

## Decision tracking (§21)

| ID | Decision | Closed in | Status |
|---|---|---|---|
| D1 | License | T0.1.2 | **Decided:** MIT OR Apache-2.0 |
| D2 | Parser foundation | T0.2.6 | **Decided:** new parser |
| D3 | UI framework | T0.6.8 | **Decided:** gpui |
| D4 | Math engine | T0.7.4 | **Decided:** RaTeX |
| D5 | tectonic distribution | T2.3.7 | Open |
| D6 | API definition source | T3.1.3 | **Decided (D28):** WIT |
| D7 | Project name | T0.1.1 | **Decided:** Kalem / `kalem-editor` |
| D8 | Agenda index storage | T3.5.2 | Open |
| D9 | Configuration formats | T1.3.6 | **Decided:** TOML settings, JSON keymap; no script file (D28) |
| D10 | A second scripting language | T4.1.1 | **Closed (owner, 2026-09-28):** none ships |
| D11 | Webviews in plugin panels | T3.1.5 | Open |
| D12 | Multiple documents | T2.5.9 | **Decided:** one window, many documents, listed by project on the left or as tabs at the top |
| D13 | Time library | T1.1.5 | **Decided:** jiff |
| D14 | Terminal UI stack | T0.8.4 | **Decided:** ratatui + crossterm |
| D15 | Ecosystem crate spin-out timing | §4.7 | **Decided:** incubate, spin out when stable |
| D16 | Syntax highlighting engine | T1.6a.2 | **Decided:** syntect |
| D17 | Vim mode engine | T2.7b.1 | Decided: own engine; replaces the Emacs profile |
| D18 | Entity table provenance | Phase 0 exit | Open: owner decision |
| D19 | Markdown parser | T2.7c.1 | Open |
| D20 | File operations for the file manager | T2.7e.1 | Decided 2026-09-28 |
| D21 | Product positioning: Org editor first, Markdown editor too, or a light Office replacement (fonts, colors, spreadsheet notation); the README, the launch and the order of phase 2 follow it | T2.7c.0, T2.1.12 | Open: owner decision, narrowed by D24; scientific writing the third pillar (§9.5, owner, 2026-09-28) |
| D22 | PDF without TeX: the renderer for "Export as PDF" from HTML (system print to PDF, a bundled HTML renderer, or typst) | T2.3.13 | Open (review, 2026-09-28) |
| D23 | UI framework revisited: stay on gpui through a registry snapshot or vendoring (T2.8.6), or leave gpui (T2.8.7) | T2.8.6a, T2.8.7, 2.9 | Open: owner decision (review, 2026-09-28) |
| D24 | File kinds: `.org` is strict Org, `.klm` a Kalem document (Org plus Kalem's additions through Org's extension points; new syntax only by RFC) | T2.2a.9 | **Decided (owner, 2026-09-28)** |
| D25 | Highlighters, renderers and completers from plugins on the contracts built-ins use (§11.11, §11.12) | T2.7c.10, T2.7a.8, T3.1.9g, T3.1.9c | **Decided (owner, 2026-09-28)** |
| D26 | The terminal is never second class (§4.1, principle 7) | TS.13 | **Decided (owner, 2026-09-28)** |
| D27 | Command scope: one axis, the type of the text at the cursor (§11.2) | T2.7a.9 | **Decided (owner, 2026-09-28)** |
| D28 | Plugin ABI and language: WASM components on a WIT API, Rust the only language, no scripting engine; the engine by the spike | T3.1.0, T3.1.1, T3.1.2 | **Decided (owner, 2026-09-28)**, engine open |
| D29 | Small core: Org, Markdown, CSV and LaTeX (2.7h), the text engine, the two frontends, the infrastructure before plugins; the rest bundled or community plugins | T3.3.2a | **Decided (owner, 2026-09-28)**; new modes and file types in `getkalem/plugins` (T3.3.7) |

---

## Rejected and deferred ideas (§4.6)

- [-] Elixir/BEAM as the main engine with Rust NIFs; Elixir as the plugin language. Rationale in §4.6. The live runtime need is covered by T3.2.
- [-] Embedded Python as the plugin language. Python is supported through Babel (T3.4.3) and out-of-process plugins (T4.1.3).
- [~] Tauri + ProseMirror as the primary UI: only if T0.6.8 says no-go. (T0.6.8 said go.)
- [-] A built-in mode for every text format. A mode exists only where the view differs from the source; formats whose view is their source are language packs (T2.7a.7), and modes beyond Org, Markdown, CSV, plain text and 2.7g come as plugins (T3.3.6). The rules of 2.7g decide (review, 2026-09-28).
- [-] An embedded scripting engine as the plugin runtime: superseded by D28; no scripting engine ships (owner, 2026-09-28).
