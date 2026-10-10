//! When a plugin serves a file (T3.7.9): one declaration for every kind of
//! plugin, and one test of it, used wherever Kalem hands a file to a
//! plugin and wherever it names a plugin for a file.
//!
//! A plugin's declaration ([`Applies`]) is a list of rules, and the plugin
//! serves a file one of its rules matches. A rule says what the file is
//! (its extension, its whole name, its first bytes, the interpreter of its
//! `#!` line: any one of those it lists) and where it is (a marker, a file
//! or folder in the file's folder or one above it: any one of those it
//! lists); a rule that lists both asks both. A viewer's rules hold for
//! files that are not text only: Kalem opens a text file in a mode, even
//! one a viewer could show (an SVG drawing is XML).
//!
//! The declaration is read from a manifest, installed or listed in the
//! index alike ([`Applies::of`]): the manifest's own `applies`, and what
//! its other parts already say: the extensions a viewer `opens`; the
//! extensions, whole names and interpreters of the `languages` it serves;
//! the markers of its `layers`. So the plugin Kalem names for a file is
//! the one it hands that file to once it is installed:
//!
//! - a viewer opens the files its declaration matches, the surest match
//!   first ([`crate::viewer::find_at`]); the viewer is never asked itself;
//! - a language plugin serves the files its declaration matches, its
//!   language then chosen among its own ([`crate::languages::for_path`]);
//! - an extension plugin with no `activation` starts when a file its
//!   declaration matches opens;
//! - the index's plugins are named by the same test: for a file Kalem
//!   cannot open, the viewers that would open it
//!   ([`crate::plugin_store::opening`]); for one it opens, a plugin not
//!   installed that would serve it ([`crate::plugin_store::suggestion`]).

use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// How much of a file is read for its first bytes and its `#!` line.
pub const HEAD: usize = 8192;

/// First bytes a rule asks for: each byte, or any byte (`??`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Magic(Vec<Option<u8>>);

impl Magic {
    /// Pairs of hexadecimal digits, `??` for any byte, spaces ignored:
    /// `25 50 44 46 2D` is `%PDF-`, `?? ?? ?? ?? 66 74 79 70` is `ftyp`
    /// after four bytes. `None` for anything else, or for no byte known.
    pub fn parse(s: &str) -> Option<Magic> {
        let digits: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
        if digits.is_empty() || !digits.len().is_multiple_of(2) {
            return None;
        }
        let bytes = digits
            .chunks(2)
            .map(|p| match p {
                ['?', '?'] => Some(None),
                [a, b] if a.is_ascii_hexdigit() && b.is_ascii_hexdigit() => {
                    u8::from_str_radix(&format!("{a}{b}"), 16).ok().map(Some)
                }
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        bytes.iter().any(Option::is_some).then_some(Magic(bytes))
    }

    /// Whether a file starting with `head` starts so.
    pub fn matches(&self, head: &[u8]) -> bool {
        head.len() >= self.0.len()
            && self
                .0
                .iter()
                .zip(head)
                .all(|(m, b)| m.is_none_or(|m| m == *b))
    }

    fn text(&self) -> String {
        self.0
            .iter()
            .map(|b| b.map_or("??".to_string(), |b| format!("{b:02X}")))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// One rule of a declaration ([`Applies`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rule {
    /// Extensions, in lower case with their dot (`.xlsx`, `.html.eex`).
    pub extensions: Vec<String>,
    /// Whole file names, as written (`mix.lock`, `.formatter.exs`).
    pub filenames: Vec<String>,
    /// First bytes.
    pub magic: Vec<Magic>,
    /// Interpreters of a `#!` line (`elixir` for `#!/usr/bin/env elixir`).
    pub shebangs: Vec<String>,
    /// Files or folders that, in the file's folder or one above it, must
    /// be there (`logseq/config.edn`, `.obsidian`), as relative paths.
    pub markers: Vec<String>,
    /// For files that are not text only (a viewer's rules).
    pub binary: bool,
}

/// How sure a match is ([`Serves::strength`]): the greater, the surer.
pub type Strength = (u8, usize, bool);

/// Why a plugin serves a file ([`Applies::serves`]): what the file is,
/// where it is, or both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Serves {
    /// What in the file a rule matched; `None` for a rule of markers only.
    pub kind: Option<Kind>,
    /// The folder holding the marker a rule asked for, and the marker.
    pub place: Option<(PathBuf, String)>,
}

/// What in a file a rule matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// Its whole name.
    Filename(String),
    /// Its extension (`.rs`).
    Extension(String),
    /// Its first bytes.
    Magic,
    /// The interpreter of its `#!` line.
    Shebang(String),
}

impl Serves {
    /// How sure the match is, for choosing among plugins: a whole name,
    /// then an extension (the longer the surer: `.html.eex` over `.eex`),
    /// then first bytes, which formats share (every Office file is a ZIP
    /// archive), then a `#!` line, then a marker alone; a marker found
    /// besides surer than none.
    pub fn strength(&self) -> Strength {
        let (rank, len) = match &self.kind {
            Some(Kind::Filename(_)) => (5, 0),
            Some(Kind::Extension(x)) => (4, x.len()),
            Some(Kind::Magic) => (3, 0),
            Some(Kind::Shebang(_)) => (2, 0),
            None => (1, 0),
        };
        (rank, len, self.place.is_some())
    }
}

fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn push_new<T: PartialEq>(into: &mut Vec<T>, x: T) {
    if !into.contains(&x) {
        into.push(x);
    }
}

/// An extension as a rule keeps it: lower case, with its dot.
fn extension(x: &str) -> Option<String> {
    let x = x.trim().trim_start_matches('.').to_lowercase();
    (!x.is_empty()).then(|| format!(".{x}"))
}

/// A marker as a rule keeps it: a relative path that stays in its folder.
fn marker(m: &str) -> Option<String> {
    let m = m.trim().trim_end_matches('/');
    let p = Path::new(m);
    let inside = p
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)));
    (!m.is_empty() && inside).then(|| m.to_string())
}

/// The interpreter a `#!` line at the start of `head` names.
fn interpreter(head: &[u8]) -> Option<&str> {
    let line = head.split(|b| *b == b'\n').next()?;
    let line = std::str::from_utf8(line).ok()?.strip_prefix("#!")?;
    let mut words = line.split_whitespace();
    let mut prog = words.next()?.rsplit('/').next()?;
    if prog == "env" {
        prog = words.find(|w| !w.starts_with('-'))?;
    }
    Some(prog)
}

impl Rule {
    /// A rule as a manifest writes it: `extensions`, `filenames`, `magic`,
    /// `shebangs`, `markers` and `binary`.
    fn of(v: &Value) -> Rule {
        let mut r = Rule {
            binary: v["binary"].as_bool().unwrap_or(false),
            ..Rule::default()
        };
        r.add_extensions(&strings(&v["extensions"]));
        for f in strings(&v["filenames"]) {
            let f = f.trim().to_string();
            if !f.is_empty() && !f.contains('/') {
                push_new(&mut r.filenames, f);
            }
        }
        for m in strings(&v["magic"]) {
            if let Some(m) = Magic::parse(&m) {
                push_new(&mut r.magic, m);
            }
        }
        for s in strings(&v["shebangs"]) {
            let s = s.trim().to_string();
            if !s.is_empty() {
                push_new(&mut r.shebangs, s);
            }
        }
        r.add_markers(&strings(&v["markers"]));
        r
    }

    fn add_extensions(&mut self, xs: &[String]) {
        for x in xs.iter().filter_map(|x| extension(x)) {
            push_new(&mut self.extensions, x);
        }
    }

    fn add_markers(&mut self, ms: &[String]) {
        for m in ms.iter().filter_map(|m| marker(m)) {
            push_new(&mut self.markers, m);
        }
    }

    /// Whether it says what a file is.
    fn has_kind(&self) -> bool {
        !(self.extensions.is_empty()
            && self.filenames.is_empty()
            && self.magic.is_empty()
            && self.shebangs.is_empty())
    }

    /// Whether it asks for nothing, and so matches nothing.
    fn is_empty(&self) -> bool {
        !self.has_kind() && self.markers.is_empty()
    }

    /// What in the file named `name` (and `lower`) starting with `head`
    /// it matches, the surest first.
    fn kind(&self, name: &str, lower: &str, head: Option<&[u8]>) -> Option<Kind> {
        if let Some(f) = self.filenames.iter().find(|f| *f == name) {
            return Some(Kind::Filename(f.clone()));
        }
        if let Some(x) = self
            .extensions
            .iter()
            .filter(|x| lower.len() > x.len() && lower.ends_with(x.as_str()))
            .max_by_key(|x| x.len())
        {
            return Some(Kind::Extension(x.clone()));
        }
        let head = head?;
        if self.magic.iter().any(|m| m.matches(head)) {
            return Some(Kind::Magic);
        }
        let prog = interpreter(head)?;
        self.shebangs
            .iter()
            .find(|s| *s == prog)
            .map(|s| Kind::Shebang(s.clone()))
    }

    fn to_json(&self) -> Value {
        let mut o = serde_json::Map::new();
        if !self.extensions.is_empty() {
            o.insert("extensions".into(), json!(self.extensions));
        }
        if !self.filenames.is_empty() {
            o.insert("filenames".into(), json!(self.filenames));
        }
        if !self.magic.is_empty() {
            let m: Vec<String> = self.magic.iter().map(Magic::text).collect();
            o.insert("magic".into(), json!(m));
        }
        if !self.shebangs.is_empty() {
            o.insert("shebangs".into(), json!(self.shebangs));
        }
        if !self.markers.is_empty() {
            o.insert("markers".into(), json!(self.markers));
        }
        if self.binary {
            o.insert("binary".into(), json!(true));
        }
        Value::Object(o)
    }
}

/// When a plugin serves a file: its rules, one of which must match.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applies {
    /// The rules.
    pub rules: Vec<Rule>,
}

/// The folder nearest `path` (its own first) holding one of `markers`,
/// and the marker.
fn nearest(path: &Path, markers: &[String]) -> Option<(PathBuf, String)> {
    path.ancestors()
        .skip(1)
        .filter(|d| !d.as_os_str().is_empty())
        .find_map(|dir| {
            markers
                .iter()
                .find(|m| std::fs::symlink_metadata(dir.join(m.as_str())).is_ok())
                .map(|m| (dir.to_path_buf(), m.clone()))
        })
}

/// The first bytes of the file at `path`, up to [`HEAD`].
pub fn head_of(path: &Path) -> Option<Vec<u8>> {
    let mut head = Vec::with_capacity(HEAD);
    std::fs::File::open(path)
        .ok()?
        .take(HEAD as u64)
        .read_to_end(&mut head)
        .ok()?;
    Some(head)
}

impl Applies {
    /// A manifest's declaration, installed or as the index lists it: its
    /// own `applies` (a rule, or a list of them), then a rule of the
    /// extensions it `opens`, one of the extensions, whole names and
    /// interpreters of its `languages`, one of the markers of each of its
    /// `layers` (and of `markers`, as an index of 2026-10-10 wrote them).
    /// A viewer (a manifest that `opens` files) serves files that are not
    /// text only, by each of its rules.
    pub fn of(m: &Value) -> Applies {
        let mut rules: Vec<Rule> = match &m["applies"] {
            a @ Value::Object(_) => vec![Rule::of(a)],
            Value::Array(a) => a.iter().filter(|r| r.is_object()).map(Rule::of).collect(),
            _ => Vec::new(),
        };
        let opens = strings(&m["opens"]);
        let viewer = !opens.is_empty();
        let mut r = Rule::default();
        r.add_extensions(&opens);
        rules.push(r);
        let mut languages = Rule::default();
        for l in m["languages"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            languages.add_extensions(&strings(&l["extensions"]));
            let named = Rule::of(&json!({
                "filenames": l["filenames"], "shebangs": l["shebangs"],
            }));
            for f in named.filenames {
                push_new(&mut languages.filenames, f);
            }
            for s in named.shebangs {
                push_new(&mut languages.shebangs, s);
            }
        }
        rules.push(languages);
        for l in m["layers"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let mut r = Rule::default();
            r.add_markers(&strings(&l["markers"]));
            rules.push(r);
        }
        let mut r = Rule::default();
        r.add_markers(&strings(&m["markers"]));
        rules.push(r);
        let mut out = Applies::default();
        for mut r in rules.into_iter().filter(|r| !r.is_empty()) {
            r.binary |= viewer;
            push_new(&mut out.rules, r);
        }
        out
    }

    /// A viewer's that names its extensions only (one built into a test).
    pub fn opening(extensions: &[&str]) -> Applies {
        let xs: Vec<String> = extensions.iter().map(|x| x.to_string()).collect();
        Applies::of(&json!({ "opens": xs }))
    }

    /// Whether it has no rule, and so serves no file.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The rules as a manifest writes them, which [`Applies::of`] reads
    /// back the same.
    pub fn to_json(&self) -> Value {
        Value::Array(self.rules.iter().map(Rule::to_json).collect())
    }

    /// Whether a rule needs a file's first bytes: to match them or its
    /// `#!` line, or to know it is not text.
    pub fn reads_head(&self) -> bool {
        self.rules
            .iter()
            .any(|r| r.binary || !r.magic.is_empty() || !r.shebangs.is_empty())
    }

    /// Whether the file at `path` starting with `head` is served, and why:
    /// the surest of the rules that match ([`Serves::strength`]). A head
    /// that is `None` or empty is not known, and a viewer's rule may then
    /// match by the name alone (New Workbook names a kind of file).
    pub fn serves(&self, path: &Path, head: Option<&[u8]>) -> Option<Serves> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let lower = name.to_lowercase();
        let head = head.filter(|h| !h.is_empty());
        let text = head.map(|h| !crate::mode::looks_binary(h));
        let mut best: Option<Serves> = None;
        for r in &self.rules {
            if r.binary && text == Some(true) {
                continue;
            }
            let kind = if r.has_kind() {
                match r.kind(&name, &lower, head) {
                    Some(k) => Some(k),
                    None => continue,
                }
            } else {
                None
            };
            let place = if r.markers.is_empty() {
                None
            } else {
                match nearest(path, &r.markers) {
                    Some(p) => Some(p),
                    None => continue,
                }
            };
            let s = Serves { kind, place };
            if best.as_ref().is_none_or(|b| s.strength() > b.strength()) {
                best = Some(s);
            }
        }
        best
    }

    /// [`Applies::serves`] for the file at `path`, its first bytes read
    /// when a rule needs them.
    pub fn serves_file(&self, path: &Path) -> Option<Serves> {
        let head = if self.reads_head() {
            head_of(path)
        } else {
            None
        };
        self.serves(path, head.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kalem-applies-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_manifest_says_once_what_it_serves() {
        let viewer = Applies::of(&json!({
            "opens": ["PDF", ".pdf"],
            "applies": {"magic": ["25 50 44 46 2D", "zz", "?? ??"]},
        }));
        assert_eq!(viewer.rules.len(), 2, "{viewer:?}");
        assert!(viewer.rules.iter().all(|r| r.binary), "a viewer's rules");
        assert_eq!(viewer.rules[0].magic.len(), 1, "bad magic dropped");
        assert_eq!(viewer.rules[1].extensions, [".pdf"]);
        let pdf = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n";
        let why = viewer.serves(Path::new("/a/report"), Some(pdf)).unwrap();
        assert_eq!(why.kind, Some(Kind::Magic));
        let why = viewer.serves(Path::new("/a/r.PDF"), Some(pdf)).unwrap();
        assert_eq!(why.kind, Some(Kind::Extension(".pdf".into())), "surer");
        // Text is a mode's, whatever its name; a name alone may do.
        assert!(
            viewer
                .serves(Path::new("/a/r.pdf"), Some(b"plain"))
                .is_none()
        );
        assert!(viewer.serves(Path::new("r.pdf"), Some(b"")).is_some());
        assert!(viewer.serves(Path::new("r.pdf"), None).is_some());

        let elixir = Applies::of(&json!({"languages": [
            {"id": "elixir", "extensions": ["ex", "exs"], "filenames": ["mix.lock"],
             "shebangs": ["elixir"]},
            {"id": "eex", "extensions": ["eex", "html.eex"]},
        ]}));
        assert_eq!(elixir.rules.len(), 1);
        assert!(!elixir.rules[0].binary);
        let kind = |p: &str, head: &[u8]| elixir.serves(Path::new(p), Some(head)).map(|s| s.kind);
        assert_eq!(
            kind("/p/a.html.eex", b"<p>"),
            Some(Some(Kind::Extension(".html.eex".into())))
        );
        assert_eq!(
            kind("/p/mix.lock", b"%{}"),
            Some(Some(Kind::Filename("mix.lock".into())))
        );
        assert_eq!(
            kind("/p/run", b"#!/usr/bin/env -S elixir\nIO.puts 1"),
            Some(Some(Kind::Shebang("elixir".into())))
        );
        assert_eq!(kind("/p/run", b"#!/bin/sh"), None);
        assert_eq!(kind("/p/eex", b""), None, "a name is not its extension");

        // What is read back is what was written.
        for a in [&viewer, &elixir] {
            assert_eq!(&Applies::of(&json!({ "applies": a.to_json() })), a);
        }
        assert!(Applies::of(&json!({"applies": {}})).is_empty());
        assert!(
            Applies::of(&json!({"applies": {"markers": ["../up", "/abs", " "]}})).is_empty(),
            "markers stay in their folder"
        );
    }

    #[test]
    fn markers_are_looked_for_above_the_file_and_asked_with_its_kind() {
        let d = temp("markers");
        std::fs::create_dir_all(d.join("Notes/logseq")).unwrap();
        std::fs::write(d.join("Notes/logseq/config.edn"), "{}").unwrap();
        std::fs::create_dir_all(d.join("Notes/pages")).unwrap();
        std::fs::create_dir_all(d.join("Site/content")).unwrap();
        std::fs::write(d.join("Site/hugo.toml"), "").unwrap();
        let graph = Applies::of(&json!({"layers": [
            {"id": "logseq", "markers": ["logseq/config.edn"], "modes": ["markdown"]},
            {"id": "obsidian", "markers": [".obsidian"], "modes": ["markdown"]},
        ]}));
        assert_eq!(graph.rules.len(), 2);
        let page = d.join("Notes/pages/Kalem.md");
        let why = graph.serves_file(&page).unwrap();
        assert_eq!(
            (why.kind, why.place),
            (
                None,
                Some((d.join("Notes"), "logseq/config.edn".to_string()))
            )
        );
        assert!(graph.serves_file(&d.join("Other/a.md")).is_none());
        // A rule of both: Markdown files of a site, nowhere else.
        let site = Applies::of(&json!({"applies":
            {"extensions": [".md"], "markers": ["hugo.toml"]}}));
        let markdown = Applies::of(&json!({"applies": {"extensions": [".md"]}}));
        let post = d.join("Site/content/post.md");
        let a = site.serves_file(&post).unwrap();
        let b = markdown.serves_file(&post).unwrap();
        assert!(a.strength() > b.strength(), "the site's plugin is surer");
        assert!(site.serves_file(&d.join("Site/content/a.txt")).is_none());
        assert!(site.serves_file(&page).is_none());
        // A bare name has no folder to look in (not the current one).
        assert!(graph.serves(Path::new("Kalem.md"), None).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
