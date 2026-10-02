//! Syntaxes from plugins (D16, T3.8.4): Sublime syntax files added to the
//! set at run time, with `extends` resolved here, since syntect does not
//! know it and the syntaxes Sublime Text ships today use it (HTML extends
//! HTML (Plain); HEEx extends HTML).
//!
//! Inheritance as Sublime Text defines it: the child's variables replace
//! the base's of the same name; a context of the child replaces the
//! base's, unless it starts with `meta_prepend: true` (its patterns go
//! before the base's) or `meta_append: true` (after); its meta keys
//! (`meta_scope`, `clear_scopes`…) replace the base's; the other keys of
//! the file (`name`, `scope`, `file_extensions`) are the child's. A base
//! is found by its file name, the folder of `Packages/HTML/` ignored.

use std::collections::HashMap;
use std::path::Path;

use syntect::parsing::{SyntaxDefinition, SyntaxSet};
use yaml_rust::yaml::Hash;
use yaml_rust::{Yaml, YamlEmitter, YamlLoader};

/// A syntax file a plugin ships.
#[derive(Debug, Clone)]
pub struct SyntaxSource {
    /// Its file name: `HTML (HEEx).sublime-syntax`.
    pub file: String,
    /// Its YAML.
    pub text: String,
    /// Only a base for `extends`, not a language of its own (a plugin's
    /// copy of HTML must not take `.html` files from the core's).
    pub base_only: bool,
}

/// What [`register`] did.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Registered {
    /// The syntaxes added: the file and the syntax's name.
    pub names: Vec<(String, String)>,
    /// The files that could not be loaded, and why.
    pub errors: Vec<(String, String)>,
}

fn key(s: &str) -> Yaml {
    Yaml::String(s.to_string())
}

fn file_key(name: &str) -> String {
    name.rsplit(['/', '\\']).next().unwrap_or(name).to_string()
}

/// A context's meta items (maps of `meta_*` and `clear_scopes`) and its
/// patterns.
fn split_meta(list: &[Yaml]) -> (Vec<Yaml>, Vec<Yaml>) {
    let is_meta = |y: &Yaml| {
        y.as_hash().is_some_and(|h| {
            !h.is_empty()
                && h.keys().all(|k| {
                    k.as_str()
                        .is_some_and(|k| k.starts_with("meta_") || k == "clear_scopes")
                })
        })
    };
    let n = list.iter().take_while(|y| is_meta(y)).count();
    (list[..n].to_vec(), list[n..].to_vec())
}

fn merge_context(base: Option<&Yaml>, child: &Yaml) -> Yaml {
    let Some(child_list) = child.as_vec() else {
        return child.clone();
    };
    let (child_meta, child_patterns) = split_meta(child_list);
    let mut prepend = false;
    let mut append = false;
    let mut meta: Hash = Hash::new();
    let base_list = base.and_then(Yaml::as_vec).cloned().unwrap_or_default();
    let (base_meta, base_patterns) = split_meta(&base_list);
    for m in &child_meta {
        for (k, v) in m.as_hash().into_iter().flatten() {
            match k.as_str() {
                Some("meta_prepend") => prepend = v.as_bool().unwrap_or(false),
                Some("meta_append") => append = v.as_bool().unwrap_or(false),
                _ => {
                    meta.insert(k.clone(), v.clone());
                }
            }
        }
    }
    if !prepend && !append {
        let mut out: Vec<Yaml> = meta.into_iter().map(|(k, v)| single(k, v)).collect();
        out.extend(child_patterns);
        return Yaml::Array(out);
    }
    let mut merged: Hash = Hash::new();
    for m in &base_meta {
        for (k, v) in m.as_hash().into_iter().flatten() {
            merged.insert(k.clone(), v.clone());
        }
    }
    for (k, v) in meta {
        merged.insert(k, v);
    }
    let mut out: Vec<Yaml> = merged.into_iter().map(|(k, v)| single(k, v)).collect();
    if prepend {
        out.extend(child_patterns);
        out.extend(base_patterns);
    } else {
        out.extend(base_patterns);
        out.extend(child_patterns);
    }
    Yaml::Array(out)
}

fn single(k: Yaml, v: Yaml) -> Yaml {
    let mut h = Hash::new();
    h.insert(k, v);
    Yaml::Hash(h)
}

/// `child` over `base`.
fn inherit(base: &Hash, child: &Hash) -> Hash {
    let mut out = base.clone();
    for (k, v) in child {
        match k.as_str() {
            Some("extends") => {}
            Some("variables") => {
                let mut vars = base
                    .get(&key("variables"))
                    .and_then(Yaml::as_hash)
                    .cloned()
                    .unwrap_or_default();
                for (vk, vv) in v.as_hash().into_iter().flatten() {
                    vars.insert(vk.clone(), vv.clone());
                }
                out.insert(k.clone(), Yaml::Hash(vars));
            }
            Some("contexts") => {
                let base_ctx = base
                    .get(&key("contexts"))
                    .and_then(Yaml::as_hash)
                    .cloned()
                    .unwrap_or_default();
                let mut ctx = base_ctx.clone();
                for (ck, cv) in v.as_hash().into_iter().flatten() {
                    ctx.insert(ck.clone(), merge_context(base_ctx.get(ck), cv));
                }
                out.insert(k.clone(), Yaml::Hash(ctx));
            }
            _ => {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    out.remove(&key("extends"));
    // A base that is hidden does not make its children hidden.
    if !child.contains_key(&key("hidden")) {
        out.remove(&key("hidden"));
    }
    out
}

fn resolve(
    file: &str,
    docs: &HashMap<String, Hash>,
    done: &mut HashMap<String, Hash>,
    stack: &mut Vec<String>,
) -> Result<Hash, String> {
    if let Some(h) = done.get(file) {
        return Ok(h.clone());
    }
    if stack.iter().any(|f| f == file) {
        return Err(format!("`extends` loops through {file}"));
    }
    let doc = docs
        .get(file)
        .ok_or_else(|| format!("extends {file}, which the plugin does not ship"))?;
    let bases: Vec<String> = match doc.get(&key("extends")) {
        None => Vec::new(),
        Some(Yaml::String(s)) => vec![file_key(s)],
        Some(Yaml::Array(a)) => a.iter().filter_map(Yaml::as_str).map(file_key).collect(),
        Some(_) => return Err("`extends` is neither a file nor a list".into()),
    };
    stack.push(file.to_string());
    let mut out = Hash::new();
    let mut first = true;
    for b in &bases {
        let base = resolve(b, docs, done, stack)?;
        out = if first { base } else { inherit(&out, &base) };
        first = false;
    }
    stack.pop();
    let out = if bases.is_empty() {
        doc.clone()
    } else {
        inherit(&out, doc)
    };
    done.insert(file.to_string(), out.clone());
    Ok(out)
}

/// Pairs of a file's name and a text: its YAML, or why it failed.
pub type Files = Vec<(String, String)>;

/// `sources` with `extends` resolved, as YAML syntect reads, and the
/// files that failed; the bases only for `extends` are left out.
pub fn flatten(sources: &[SyntaxSource]) -> (Files, Files) {
    let mut docs: HashMap<String, Hash> = HashMap::new();
    let mut errors = Vec::new();
    for s in sources {
        match YamlLoader::load_from_str(&s.text) {
            Ok(d) => match d.into_iter().next() {
                Some(Yaml::Hash(h)) => {
                    docs.insert(file_key(&s.file), h);
                }
                _ => errors.push((s.file.clone(), "not a syntax".to_string())),
            },
            Err(e) => errors.push((s.file.clone(), e.to_string())),
        }
    }
    let mut done = HashMap::new();
    let mut out = Vec::new();
    for s in sources.iter().filter(|s| !s.base_only) {
        let f = file_key(&s.file);
        if !docs.contains_key(&f) {
            continue;
        }
        match resolve(&f, &docs, &mut done, &mut Vec::new()) {
            Ok(h) => {
                let mut text = String::new();
                let doc = Yaml::Hash(h);
                match YamlEmitter::new(&mut text).dump(&doc) {
                    Ok(()) => out.push((s.file.clone(), text)),
                    Err(e) => errors.push((s.file.clone(), format!("{e:?}"))),
                }
            }
            Err(e) => errors.push((s.file.clone(), e)),
        }
    }
    (out, errors)
}

/// Sets the plugins' syntaxes: the built-in set with `sources` added,
/// replacing those added before. A plugin's syntax comes first for its
/// file extensions, so a plugin's Elixir replaces the built-in one.
pub fn register(sources: &[SyntaxSource]) -> Registered {
    register_cached(sources, None)
}

/// [`register`], with the built set kept in `cache` (a folder) under a
/// key of the sources: building takes most of a second, loading the
/// cached set a few milliseconds.
pub fn register_cached(sources: &[SyntaxSource], cache: Option<&Path>) -> Registered {
    let key = {
        let mut h = Fnv(0xcbf2_9ce4_8422_2325);
        h.write(env!("CARGO_PKG_VERSION").as_bytes());
        for s in sources {
            h.write(s.file.as_bytes());
            h.write(&[u8::from(s.base_only)]);
            h.write(s.text.as_bytes());
        }
        h.0
    };
    let file = cache.map(|c| c.join(format!("syntaxes-{key:016x}.bin")));
    if let Some(f) = &file
        && let Ok(bytes) = std::fs::read(f)
        && let Ok((set, registered)) =
            syntect::dumps::from_reader::<(SyntaxSet, Registered), _>(&bytes[..])
    {
        super::replace_set(Box::leak(Box::new(set)));
        return registered;
    }
    let (flat, mut errors) = flatten(sources);
    let mut defs = Vec::new();
    let mut names = Vec::new();
    for (file, text) in flat {
        match SyntaxDefinition::load_from_str(&text, true, None) {
            Ok(d) => {
                names.push((file, d.name.clone()));
                defs.push(d);
            }
            Err(e) => errors.push((file, e.to_string())),
        }
    }
    let mut builder = super::defaults().clone().into_builder();
    for d in defs {
        builder.add(d);
    }
    let set = builder.build();
    let registered = Registered { names, errors };
    if let (Some(f), Some(dir)) = (&file, cache) {
        // Old sets go; a failed write only costs the next start its time.
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.filter_map(Result::ok) {
                let n = e.file_name();
                let n = n.to_string_lossy();
                if n.starts_with("syntaxes-") && n.ends_with(".bin") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        let _ = std::fs::create_dir_all(dir);
        let tmp = f.with_extension("tmp");
        if std::fs::write(&tmp, syntect::dumps::dump_binary(&(&set, &registered))).is_ok() {
            let _ = std::fs::rename(&tmp, f);
        }
    }
    super::replace_set(Box::leak(Box::new(set)));
    registered
}

/// FNV-1a, a hash that is the same from one build to the next.
struct Fnv(u64);

impl Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
        // A separator, so that ("ab", "c") and ("a", "bc") differ.
        self.0 = (self.0 ^ 0xff).wrapping_mul(0x0100_0000_01b3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"%YAML 1.2
---
name: Base
scope: source.base
file_extensions: [base]
variables:
  word: '[a-z]+'
contexts:
  main:
    - match: '{{word}}'
      scope: keyword.base
  other:
    - meta_scope: string.base
    - match: x
      pop: true
"#;

    const CHILD: &str = r#"%YAML 1.2
---
name: Child
scope: source.child
extends: Packages/Base/Base.sublime-syntax
file_extensions: [child]
variables:
  word: '[0-9]+'
contexts:
  main:
    - meta_prepend: true
    - match: '!'
      scope: keyword.operator.child
"#;

    #[test]
    fn extends_resolved() {
        let sources = [
            SyntaxSource {
                file: "Base.sublime-syntax".into(),
                text: BASE.into(),
                base_only: true,
            },
            SyntaxSource {
                file: "Child.sublime-syntax".into(),
                text: CHILD.into(),
                base_only: false,
            },
        ];
        let r = register(&sources);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(
            r.names,
            [("Child.sublime-syntax".to_string(), "Child".to_string())]
        );
        let lang = crate::Language::find("child").unwrap();
        assert_eq!(lang.name(), "Child");
        let spans = crate::highlight(lang, "!12ab");
        // `!` from the child, the number by the child's `word`, the
        // letters not a keyword any more.
        assert_eq!(spans[0][0].kind, crate::Kind::Operator);
        assert_eq!(spans[0][1].range, 1..3);
        assert_eq!(spans[0].len(), 2);
        assert!(crate::Language::find("base").is_none());
    }

    #[test]
    fn missing_base_reported() {
        let (flat, errors) = flatten(&[SyntaxSource {
            file: "Child.sublime-syntax".into(),
            text: CHILD.into(),
            base_only: false,
        }]);
        assert!(flat.is_empty());
        assert!(errors[0].1.contains("Base.sublime-syntax"));
    }

    #[test]
    fn cached() {
        let dir = std::env::temp_dir().join(format!("kalem-syntax-cache-{}", std::process::id()));
        let sources = [
            SyntaxSource {
                file: "Base.sublime-syntax".into(),
                text: BASE.into(),
                base_only: true,
            },
            SyntaxSource {
                file: "Child.sublime-syntax".into(),
                text: CHILD.into(),
                base_only: false,
            },
        ];
        let first = register_cached(&sources, Some(&dir));
        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(files.len(), 1);
        let again = register_cached(&sources, Some(&dir));
        assert_eq!(first.names, again.names);
        assert_eq!(crate::Language::find("child").unwrap().name(), "Child");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
