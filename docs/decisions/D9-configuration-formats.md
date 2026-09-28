# D9: Configuration formats

- Status: **Decided** (2026-09-28)
- Decision: **TOML for settings (`settings.toml`), JSON for key bindings (`keymap.json`), JavaScript for personal scripts (`init.js`)**, as recommended in §21.
- Tasks: T1.3.4 (keymap), T1.3.6 (settings)
- Code: `crates/kalem-core/src/settings.rs`, `crates/kalem-core/src/keymap.rs`

## Why

| Option | For | Against |
|---|---|---|
| TOML + JS + JSON (chosen) | Settings are flat values that people edit by hand; TOML reads well, allows comments and is the Rust ecosystem's format. Key bindings are a list of records, which JSON writes naturally; the format matches VS Code's `keybindings.json`, which many users know. Scripts need a language, and JavaScript is the plugin language (§11). | Three formats. |
| JS only | One format, programmable settings. | A settings file that runs code cannot be read without running it: no safe loading, no settings editor that writes values back, no validation before startup. |
| JSON only | One data format. | No comments in standard JSON; nested settings are harder to edit by hand. |

The settings UI must change a value without losing the user's comments and layout, which rules out writing files from a data structure. `toml_edit` edits TOML in place (`settings::set_in_toml`).

## Details

- **Layers:** built-in defaults, the user's `settings.toml`, the workspace's `.kalem/settings.toml` (found in the document's directory or an ancestor), and the document's own keywords for document behavior (`#+TODO` over `org.todo_keywords`, `#+STARTUP` over the logging settings).
- **Location:** `$KALEM_CONFIG_DIR`, else `kalem` in `$XDG_CONFIG_HOME`, `%APPDATA%` on Windows, or `~/.config` (as Zed and Helix do on macOS, so one dotfiles layout works on every Unix).
- **Validation:** every built-in setting has a type, a range or a set of values, and a default. A wrong value is reported and the layer below applies, so a typo never stops the editor. Unknown keys are reported and kept. Tables under `plugins.<id>` belong to plugins.
- **Keymap files** allow `//` and `/* */` comments, which VS Code's format allows too. An invalid entry is skipped and reported; the rest of the file applies.
