//! Formatters by file type (Emacs's format-all; asked by the owner,
//! 2026-10-10): Format Document runs the formatter of the document's
//! type with the text on its standard input and takes the formatted text
//! from its output, as a language plugin's `commands.format` runs. Which
//! program: the one the setting `formatters.TYPE` names, else the
//! language's usual one ([`DEFAULTS`]: `rustfmt`, `mix format`, `ruff`,
//! `prettier`, `gofmt`, `clang-format`…), the first installed where
//! several are usual. The setting's value is the command line, `{file}`
//! standing for the file's path; `off` turns the type's formatter off.
//!
//! A type is named as `files.modes` names it: the language (`rust`,
//! `python`, `elixir`), a file extension (`rs`, `ex`), a mode (`markdown`,
//! `org`), or a file name (`cmakelists.txt`), in lower case.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde_json::{Map, Value};

use crate::document::DocumentState;
use crate::mode::DocumentMode;

/// A formatter to run: the program with its arguments, `{file}` filled
/// in, and the folder it runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formatter {
    /// The program and its arguments.
    pub command: Vec<String>,
    /// Where it runs: the project's root, else the file's folder.
    pub dir: PathBuf,
}

/// A language's usual formatters.
struct Default {
    /// The names the type goes by (lower case).
    names: &'static [&'static str],
    /// Command lines, in order of preference: the first installed runs.
    commands: &'static [&'static str],
    /// Files that mark the folder the formatter runs in (its project).
    roots: &'static [&'static str],
}

/// The usual formatter of each language, run from the project's root
/// (the nearest folder above the file holding one of the marker files,
/// else the file's folder). Every one reads the text on its standard
/// input and writes the formatted text to its output.
const DEFAULTS: &[Default] = &[
    Default {
        names: &["rust", "rs"],
        commands: &["rustfmt --edition 2024"],
        roots: &["Cargo.toml"],
    },
    Default {
        names: &["elixir", "ex", "exs", "eex", "heex"],
        commands: &["mix format --stdin-filename {file} -"],
        roots: &["mix.exs"],
    },
    Default {
        names: &["python", "py", "pyi"],
        commands: &[
            "ruff format --stdin-filename {file} -",
            "black -q --stdin-filename {file} -",
        ],
        roots: &["pyproject.toml", "ruff.toml", "setup.cfg"],
    },
    Default {
        names: &["go"],
        commands: &["gofmt"],
        roots: &["go.mod"],
    },
    Default {
        names: &[
            "javascript",
            "js",
            "mjs",
            "cjs",
            "jsx",
            "javascriptreact",
            "typescript",
            "ts",
            "mts",
            "cts",
            "tsx",
            "typescriptreact",
            "css",
            "scss",
            "less",
            "html",
            "vue",
            "svelte",
            "json",
            "jsonc",
            "yaml",
            "yml",
            "graphql",
            "gql",
        ],
        commands: &["prettier --stdin-filepath {file}"],
        roots: &["package.json"],
    },
    Default {
        names: &[
            "c",
            "h",
            "c++",
            "cpp",
            "cc",
            "cxx",
            "hpp",
            "hh",
            "hxx",
            "objective-c",
            "objective-c++",
            "objc",
            "m",
            "mm",
            "cuda",
            "cu",
            "protocol buffer",
            "proto",
        ],
        commands: &["clang-format --assume-filename={file}"],
        roots: &[".clang-format", "_clang-format"],
    },
    Default {
        names: &["java"],
        commands: &["google-java-format -"],
        roots: &["pom.xml", "build.gradle", "build.gradle.kts"],
    },
    Default {
        names: &["kotlin", "kt", "kts"],
        commands: &["ktlint --stdin --format"],
        roots: &["build.gradle.kts", "build.gradle"],
    },
    Default {
        names: &[
            "bourne again shell (bash)",
            "shell script",
            "shell",
            "sh",
            "bash",
            "zsh",
        ],
        commands: &["shfmt -filename {file}"],
        roots: &[".editorconfig"],
    },
    Default {
        names: &["fish"],
        commands: &["fish_indent"],
        roots: &[],
    },
    Default {
        names: &["lua"],
        commands: &["stylua --stdin-filepath {file} -"],
        roots: &["stylua.toml", ".stylua.toml"],
    },
    Default {
        names: &["zig"],
        commands: &["zig fmt --stdin"],
        roots: &["build.zig"],
    },
    Default {
        names: &["nix"],
        commands: &["nixfmt", "alejandra --quiet -"],
        roots: &["flake.nix"],
    },
    Default {
        names: &["ocaml", "ml", "mli"],
        commands: &["ocamlformat --name {file} -"],
        roots: &["dune-project", ".ocamlformat"],
    },
    Default {
        names: &["haskell", "hs"],
        commands: &[
            "ormolu --stdin-input-file {file}",
            "fourmolu --stdin-input-file {file}",
        ],
        roots: &["stack.yaml", "cabal.project"],
    },
    Default {
        names: &["elm"],
        commands: &["elm-format --stdin"],
        roots: &["elm.json"],
    },
    Default {
        names: &["purescript", "purs"],
        commands: &["purs-tidy format"],
        roots: &["spago.yaml", "spago.dhall"],
    },
    Default {
        names: &["dart"],
        commands: &["dart format"],
        roots: &["pubspec.yaml"],
    },
    Default {
        names: &["swift"],
        commands: &["swift-format"],
        roots: &["Package.swift"],
    },
    Default {
        names: &["scala", "sc", "sbt"],
        commands: &["scalafmt --stdin"],
        roots: &["build.sbt", ".scalafmt.conf"],
    },
    Default {
        names: &["ruby", "rb", "rake", "gemspec"],
        commands: &["rufo"],
        roots: &["Gemfile"],
    },
    Default {
        names: &["perl", "pl", "pm"],
        commands: &["perltidy -st -se"],
        roots: &[".perltidyrc"],
    },
    Default {
        names: &["erlang", "erl", "hrl"],
        commands: &["erlfmt -"],
        roots: &["rebar.config"],
    },
    Default {
        names: &["gleam"],
        commands: &["gleam format --stdin"],
        roots: &["gleam.toml"],
    },
    Default {
        names: &["crystal", "cr"],
        commands: &["crystal tool format -"],
        roots: &["shard.yml"],
    },
    Default {
        names: &["d"],
        commands: &["dfmt"],
        roots: &["dub.json", "dub.sdl"],
    },
    Default {
        names: &["terraform", "tf", "tfvars", "hcl"],
        commands: &["terraform fmt -"],
        roots: &[],
    },
    Default {
        names: &["toml"],
        commands: &["taplo fmt -"],
        roots: &["taplo.toml", ".taplo.toml"],
    },
    Default {
        names: &["cmake", "cmakelists.txt"],
        commands: &["cmake-format -"],
        roots: &[".cmake-format", ".cmake-format.yaml"],
    },
    Default {
        names: &["sql"],
        commands: &["pg_format -"],
        roots: &[],
    },
    Default {
        names: &["xml", "xsd", "xsl", "xslt", "svg"],
        commands: &["xmllint --format -"],
        roots: &[],
    },
    Default {
        names: &["jsonnet", "libsonnet"],
        commands: &["jsonnetfmt -"],
        roots: &[],
    },
    Default {
        names: &["dhall"],
        commands: &["dhall format"],
        roots: &[],
    },
];

/// Files that mark a project's root for a formatter the user set: the
/// nearest folder above the file holding one is where it runs.
const COMMON_ROOTS: &[&str] = &[
    ".git",
    "Cargo.toml",
    "mix.exs",
    "package.json",
    "pyproject.toml",
    "go.mod",
    ".editorconfig",
];

/// The user's `formatters` table.
static USER: RwLock<Option<Map<String, Value>>> = RwLock::new(None);

/// Takes the user's `formatters` setting (a table from types to command
/// lines); called when the settings load and whenever they change.
pub fn set_user_table(table: Option<&Value>) {
    let t = table.and_then(Value::as_object).cloned();
    if let Ok(mut u) = USER.write() {
        *u = t;
    }
}

/// The user's command line for one of `names`, if the table has one:
/// `Some(None)` when it turns the formatter off.
fn user_entry(names: &[String]) -> Option<Option<String>> {
    let u = USER.read().ok()?;
    let table = u.as_ref()?;
    for name in names {
        let Some(v) = table.get(name).or_else(|| {
            table
                .iter()
                .find(|(k, _)| k.to_lowercase() == *name)
                .map(|(_, v)| v)
        }) else {
            continue;
        };
        let line = v.as_str().unwrap_or("").trim();
        return Some(if line.is_empty() || line.eq_ignore_ascii_case("off") {
            None
        } else {
            Some(line.to_string())
        });
    }
    None
}

/// The usual formatters of one of `names`.
fn default_entry(names: &[String]) -> Option<&'static Default> {
    DEFAULTS
        .iter()
        .find(|d| names.iter().any(|n| d.names.contains(&n.as_str())))
}

/// The names a document's type goes by, lower case, the most specific
/// first: a plugin's language id, the language hint (`rs`, `python`), the
/// highlighter's name for it (`rust`), a mode's name (`markdown`), the
/// file's extension and its name.
pub fn names_of(path: Option<&Path>, mode: &DocumentMode, first_line: Option<&str>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut add = |n: String| {
        let n = n.trim().to_lowercase();
        if !n.is_empty() && !names.contains(&n) {
            names.push(n);
        }
    };
    if let Some(p) = path
        && let Some((_, lang)) = crate::languages::for_path(p, first_line)
    {
        add(lang.id.clone());
    }
    match mode {
        DocumentMode::Text { language: Some(l) } => {
            add(l.clone());
            if let Some(lang) = kalem_highlight::Language::find(l) {
                add(lang.name().to_string());
            }
        }
        DocumentMode::Text { language: None } => {}
        // No text of its own to format: a viewer's document, a document
        // of flowing text (its plugin writes it), a listing.
        DocumentMode::Binary
        | DocumentMode::Viewer
        | DocumentMode::Flow
        | DocumentMode::Directory => return Vec::new(),
        m => add(m.name().to_string()),
    }
    if let Some(p) = path {
        if let Some(e) = p.extension().and_then(|e| e.to_str()) {
            add(e.to_string());
        }
        if let Some(n) = p.file_name().and_then(|n| n.to_str()) {
            add(n.to_string());
        }
    }
    names
}

/// The names of `doc`'s type ([`names_of`]).
pub fn names(doc: &DocumentState) -> Vec<String> {
    let first = doc.text().as_str().lines().next();
    names_of(doc.meta.path.as_deref(), &doc.meta.mode, first)
}

/// The formatter the user set for a file's type (`formatters.TYPE`),
/// `{file}` filled in, run from the nearest folder above the file that
/// holds a marker of the type's project (else a common one, else the
/// file's folder).
pub fn user_for_file(
    path: &Path,
    mode: &DocumentMode,
    first_line: Option<&str>,
) -> Option<Formatter> {
    let names = names_of(Some(path), mode, first_line);
    let line = user_entry(&names)??;
    let roots = default_entry(&names).map_or(COMMON_ROOTS, |d| d.roots);
    Some(Formatter {
        command: fill(&split(&line), path),
        dir: root_for(path, roots),
    })
}

/// The usual formatter of a file's type, unless the user set one or
/// turned it off: the first installed, else the first (so that the
/// message names it).
pub fn default_for_file(
    path: &Path,
    mode: &DocumentMode,
    first_line: Option<&str>,
) -> Option<Formatter> {
    let names = names_of(Some(path), mode, first_line);
    if user_entry(&names).is_some() {
        return None;
    }
    let d = default_entry(&names)?;
    let dir = root_for(path, d.roots);
    let commands: Vec<Vec<String>> = d.commands.iter().map(|c| fill(&split(c), path)).collect();
    let command = commands
        .iter()
        .find(|c| {
            c.first()
                .is_some_and(|p| kalem_lsp::find_program(p, Some(&dir), &[]).is_some())
        })
        .or_else(|| commands.first())?
        .clone();
    Some(Formatter { command, dir })
}

/// The formatter for a file: the user's, else the usual one of its type.
pub fn for_file(path: &Path, mode: &DocumentMode, first_line: Option<&str>) -> Option<Formatter> {
    user_for_file(path, mode, first_line).or_else(|| default_for_file(path, mode, first_line))
}

/// The formatter the user set for `doc`'s type.
pub fn user_formatter(doc: &DocumentState) -> Option<Formatter> {
    let path = doc.meta.path.as_deref()?;
    user_for_file(path, &doc.meta.mode, doc.text().as_str().lines().next())
}

/// The usual formatter of `doc`'s type ([`default_for_file`]).
pub fn default_formatter(doc: &DocumentState) -> Option<Formatter> {
    let path = doc.meta.path.as_deref()?;
    default_for_file(path, &doc.meta.mode, doc.text().as_str().lines().next())
}

/// A formatter is known for `doc`'s type and `doc` has a file to format:
/// the user's, or a usual one not turned off (the `hasFormatter` of
/// when-clauses; whether the program is installed is seen when it runs).
pub fn has_formatter(doc: &DocumentState) -> bool {
    doc.meta.path.is_some() && known(doc)
}

/// A formatter is known for `doc`'s type, whether or not `doc` has a
/// file yet.
pub fn known(doc: &DocumentState) -> bool {
    let names = names(doc);
    match user_entry(&names) {
        Some(set) => set.is_some(),
        None => default_entry(&names).is_some(),
    }
}

/// The nearest folder above `path` holding one of `roots`, else the
/// file's folder.
fn root_for(path: &Path, roots: &[&str]) -> PathBuf {
    let real = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let real = std::path::absolute(&real).unwrap_or(real);
    let roots: Vec<String> = roots.iter().map(|r| r.to_string()).collect();
    kalem_lsp::find_root(&real, &roots, false)
        .or_else(|| real.parent().map(Path::to_path_buf))
        .unwrap_or_default()
}

/// `{file}` in each word replaced by the file's path (absolute, links
/// followed, as the folder it runs in is).
fn fill(words: &[String], path: &Path) -> Vec<String> {
    let real = dunce::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf());
    let file = real.to_string_lossy();
    words.iter().map(|w| w.replace("{file}", &file)).collect()
}

/// A command line split into words, as a shell would without expanding
/// anything: spaces separate, quotes (`"` or `'`) keep, a backslash
/// escapes the next character outside single quotes.
pub fn split(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => {
                if let Some(n) = chars.next() {
                    word.push(n);
                }
            }
            (Some(_), c) => word.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                in_word = true;
            }
            (None, '\\') => {
                if let Some(n) = chars.next() {
                    word.push(n);
                    in_word = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            (None, c) => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(word);
    }
    words
}

/// Runs a formatter: `text` on its standard input, the formatted text
/// from its output, or its first line of errors.
pub fn run(program: &Path, args: &[String], dir: &Path, text: &str) -> Result<String, String> {
    use std::io::Write;
    let mut child = std::process::Command::new(program)
        .args(args)
        .current_dir(dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let input = text.to_string();
        // Written on its own thread, so a formatter that writes before it
        // has read everything cannot block both sides.
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let first = err
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim();
        return Err(if first.is_empty() {
            format!("{}", out.status)
        } else {
            first.to_string()
        });
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}

/// The text of `doc` formatted now, through the formatter Format Document
/// would run, but for a language server's (which answers later): the
/// user's, the language plugin's `commands.format`, a language pack's,
/// the type's usual program, or Kalem's own for Org and LaTeX. `Ok(None)`
/// when the type has none; `Err` why it failed.
pub fn format_sync(doc: &DocumentState) -> Result<Option<String>, String> {
    let text = doc.text().as_str();
    if let Some(f) = user_formatter(doc) {
        return format_now(&f, text).map(Some);
    }
    if let Some(f) = crate::lsp::plugin_formatter(doc) {
        return format_now(&f, text).map(Some);
    }
    if let Some(f) = crate::packs::format(doc) {
        return match f {
            crate::packs::Formatted::Text(t) => Ok(Some(t)),
            crate::packs::Formatted::Refused(d) => Err(d.message),
        };
    }
    if let Some(f) = default_formatter(doc) {
        return format_now(&f, text).map(Some);
    }
    Ok(match doc.meta.mode {
        DocumentMode::Latex => Some(match &doc.meta.path {
            Some(p) => crate::latex_fmt::format_file(p, text, false),
            None => crate::latex_fmt::format(text, false),
        }),
        DocumentMode::Org => Some(org_edit::format::format(&org_model::Document::new(
            org_syntax::parse(text),
        ))),
        _ => None,
    })
}

/// Formats `doc` before it is saved when `editor.format_on_save` is on
/// ([`format_sync`]). A failure leaves the text as it is and comes back
/// to be shown; the save goes on.
pub fn on_save(
    doc: &mut DocumentState,
    config: &crate::settings::Config,
    now: std::time::Instant,
) -> Option<String> {
    if !config.bool("editor.format_on_save")
        || doc.dired.is_some()
        || doc.viewer.is_some()
        || doc.meta.path.is_none()
    {
        return None;
    }
    match format_sync(doc) {
        Ok(Some(new)) => {
            if let Some(tx) =
                crate::lines::replace_differing(doc.text().as_str(), &new, "Format Document")
            {
                doc.apply(&tx, org_edit::ChangeKind::Command, now);
            }
            None
        }
        Ok(None) => None,
        Err(why) => Some(crate::tr!("fmt-on-save-failed", reason = why)),
    }
}

/// Formats `text` with `f` now (for `kalem fmt`): the formatted text, or
/// why not (the program not installed, or its first line of errors).
pub fn format_now(f: &Formatter, text: &str) -> Result<String, String> {
    let (program, args) = f
        .command
        .split_first()
        .ok_or_else(|| crate::tr!("lsp-formatter-missing", program = ""))?;
    let found = kalem_lsp::find_program(program, Some(&f.dir), &[])
        .ok_or_else(|| crate::tr!("lsp-formatter-missing", program = program.as_str()))?;
    run(&found, args, &f.dir, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_mode(l: &str) -> DocumentMode {
        DocumentMode::Text {
            language: Some(l.to_string()),
        }
    }

    #[test]
    fn command_lines_split() {
        assert_eq!(
            split("rustfmt --edition 2024"),
            ["rustfmt", "--edition", "2024"]
        );
        assert_eq!(
            split(r#"prettier --stdin-filepath "a b.js" 'c d' e\ f"#),
            ["prettier", "--stdin-filepath", "a b.js", "c d", "e f"]
        );
        assert_eq!(split("  "), Vec::<String>::new());
        assert_eq!(split(r#""""#), [""]);
    }

    #[test]
    fn names_of_a_type() {
        let p = Path::new("/tmp/proj/src/main.rs");
        assert_eq!(
            names_of(Some(p), &text_mode("rs"), None),
            ["rs", "rust", "main.rs"]
        );
        let p = Path::new("/tmp/proj/CMakeLists.txt");
        assert_eq!(
            names_of(Some(p), &text_mode("cmake"), None),
            ["cmake", "txt", "cmakelists.txt"]
        );
        let p = Path::new("/tmp/notes.org");
        assert_eq!(
            names_of(Some(p), &DocumentMode::Org, None),
            ["org", "notes.org"]
        );
        assert_eq!(
            names_of(None, &DocumentMode::Text { language: None }, None),
            Vec::<String>::new()
        );
        assert!(names_of(Some(p), &DocumentMode::Directory, None).is_empty());
    }

    #[test]
    fn user_table_over_the_defaults() {
        let dir = std::env::temp_dir().join(format!("kalem-formatters-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "").unwrap();
        let file = dir.join("src/main.rs");
        std::fs::write(&file, "fn main(){}\n").unwrap();
        let real = dunce::canonicalize(&dir).unwrap();
        // The usual one, run from the Cargo project's root.
        set_user_table(None);
        let f = for_file(&file, &text_mode("rs"), None).unwrap();
        assert_eq!(f.command[0], "rustfmt");
        assert_eq!(f.dir, real);
        // The user's, `{file}` filled in, from the same root.
        set_user_table(Some(&serde_json::json!({
            "rust": "my-fmt --name {file}",
            "Foo": "foo-fmt",
            "python": "off",
        })));
        let f = for_file(&file, &text_mode("rs"), None).unwrap();
        assert_eq!(f.command[0], "my-fmt");
        assert_eq!(
            f.command[2],
            dunce::canonicalize(&file).unwrap().to_string_lossy()
        );
        assert_eq!(f.dir, real);
        // A type of the user's own, matched without regard to case, run
        // from the nearest common root (the `Cargo.toml` above).
        let foo = dir.join("src/a.foo");
        std::fs::write(&foo, "").unwrap();
        let f = for_file(&foo, &text_mode("foo"), None).unwrap();
        assert_eq!(f.command, ["foo-fmt"]);
        assert_eq!(f.dir, real);
        // Turned off: neither the user's nor the usual one.
        let py = dir.join("src/a.py");
        std::fs::write(&py, "").unwrap();
        assert!(for_file(&py, &text_mode("py"), None).is_none());
        set_user_table(None);
        assert!(for_file(&py, &text_mode("py"), None).is_some());
        // A type with no formatter known.
        assert!(for_file(&foo, &text_mode("foo"), None).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_default_is_a_command() {
        for d in DEFAULTS {
            assert!(!d.names.is_empty());
            for c in d.commands {
                let words = split(c);
                assert!(!words.is_empty(), "{c}");
                assert!(!words[0].contains('{'), "{c}: the program is a name");
            }
            for n in d.names {
                assert_eq!(*n, n.to_lowercase(), "{n}");
                let others = DEFAULTS
                    .iter()
                    .filter(|o| !std::ptr::eq(*o, d) && o.names.contains(n))
                    .count();
                assert_eq!(others, 0, "{n} in two entries");
            }
        }
    }
}
