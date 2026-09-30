//! `kalem diff-emacs`: compares Kalem's parse with Emacs's org-element.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde_json::Value;

use super::{Result, read};

pub(crate) struct DiffOptions {
    pub(crate) emacs_dumps: Option<PathBuf>,
    pub(crate) dump_el: Option<PathBuf>,
    pub(crate) emacs: String,
    pub(crate) show: usize,
    pub(crate) summary: bool,
    pub(crate) json: bool,
}

/// A flattened node: position in the tree and the compared fields.
#[derive(Debug, Clone, PartialEq)]
struct Flat {
    ty: String,
    begin: u64,
    end: u64,
    cb: Option<u64>,
    ce: Option<u64>,
    pa: Option<u64>,
    pb: u64,
    path: String,
    props: serde_json::Map<String, Value>,
    /// Emacs let this node run past its parent's end (a documented
    /// difference: Kalem keeps elements inside their container).
    overflow: bool,
}

/// Affiliated keyword properties other than `name` are not compared.
const SKIPPED_PROPS: &[&str] = &["caption", "header", "plot", "results"];

/// Parses JSON of any nesting depth (dumps of deeply nested documents).
fn parse_json(text: &str) -> std::result::Result<Value, serde_json::Error> {
    use serde::Deserialize;
    let mut de = serde_json::Deserializer::from_str(text);
    de.disable_recursion_limit();
    let de = serde_stacker::Deserializer::new(&mut de);
    Value::deserialize(de)
}

fn flatten(v: &Value, path: &str, out: &mut Vec<Flat>) {
    stacker::maybe_grow(64 * 1024, 4 * 1024 * 1024, || flatten_inner(v, path, out))
}

fn flatten_inner(v: &Value, path: &str, out: &mut Vec<Flat>) {
    // Children must end within the parent's contents.
    let parent_end = v
        .get("ce")
        .and_then(Value::as_u64)
        .or_else(|| v.get("end").and_then(Value::as_u64))
        .unwrap_or(u64::MAX);
    for c in v
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        flatten_node(c, path, parent_end, out);
    }
}

fn flatten_node(v: &Value, path: &str, parent_end: u64, out: &mut Vec<Flat>) {
    stacker::maybe_grow(64 * 1024, 4 * 1024 * 1024, || {
        flatten_node_inner(v, path, parent_end, out)
    })
}

fn flatten_node_inner(v: &Value, path: &str, parent_end: u64, out: &mut Vec<Flat>) {
    let ty = v["type"].as_str().unwrap_or("?").to_string();
    let here = format!("{path}/{ty}");
    out.push(Flat {
        ty,
        begin: v["begin"].as_u64().unwrap_or(0),
        end: v["end"].as_u64().unwrap_or(0),
        cb: v["cb"].as_u64(),
        ce: v["ce"].as_u64(),
        pa: v["pa"].as_u64(),
        pb: v["pb"].as_u64().unwrap_or(0),
        path: here.clone(),
        props: v
            .get("props")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
        overflow: v["end"].as_u64().unwrap_or(0) > parent_end,
    });
    flatten(v, &here, out);
    if let Some(sec) = v.get("secondary").and_then(Value::as_object) {
        for (k, arr) in sec {
            for c in arr.as_array().into_iter().flatten() {
                flatten_node(c, &format!("{here}:{k}"), u64::MAX, out);
            }
        }
    }
}

#[derive(Default)]
struct Stats {
    both: usize,
    emacs_only: usize,
    kalem_only: usize,
    field_diff: usize,
    prop_diff: usize,
    known: usize,
}

fn find_dump_el(explicit: &Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.clone());
    }
    let mut dir = std::env::current_dir().map_err(|e| e.to_string())?;
    loop {
        let candidate = dir.join("tests/emacs/dump.el");
        if candidate.exists() {
            return Ok(candidate);
        }
        if !dir.pop() {
            return Err("cannot find tests/emacs/dump.el; pass --dump-el".into());
        }
    }
}

fn emacs_dumps(files: &[PathBuf], opts: &DiffOptions) -> Result<PathBuf> {
    if let Some(dir) = &opts.emacs_dumps {
        return Ok(dir.clone());
    }
    let dump_el = find_dump_el(&opts.dump_el)?;
    let dir = std::env::temp_dir().join(format!("kalem-diff-emacs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let status = Command::new(&opts.emacs)
        .arg("-Q")
        .arg("--batch")
        .arg("-l")
        .arg(&dump_el)
        .arg("--batch-dir")
        .arg(&dir)
        .args(files)
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("cannot run {}: {e}", opts.emacs))?;
    if !status.success() {
        return Err(format!("{} exited with {status}", opts.emacs));
    }
    Ok(dir)
}

/// The dump of the `i`th file: numbered when Kalem ran Emacs (file names
/// repeat across a corpus), `NAME.json` in a directory given by the user.
pub(crate) fn dump_path(dir: &Path, f: &Path, i: usize, opts: &DiffOptions) -> PathBuf {
    if opts.emacs_dumps.is_some() {
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        dir.join(format!("{name}.json"))
    } else {
        dir.join(format!("{i:05}.json"))
    }
}

pub(crate) fn diff_emacs(files: &[PathBuf], opts: &DiffOptions) -> Result<ExitCode> {
    let dir = emacs_dumps(files, opts)?;
    let mut per_type: BTreeMap<String, Stats> = BTreeMap::new();
    let mut files_exact = 0;
    let mut report = Vec::new();
    for (i, f) in files.iter().enumerate() {
        let dump = dump_path(&dir, f, i, opts);
        let Ok(ej) = read(&dump) else {
            eprintln!("kalem: no Emacs dump for {}", f.display());
            continue;
        };
        let ev: Value = parse_json(&ej).map_err(|e| format!("{}: {e}", dump.display()))?;
        let text = read(f)?;
        let ctx =
            org_syntax::ParseContext::for_file(&text, f, &org_syntax::ParseContext::default());
        let kv: Value =
            parse_json(&org_syntax::debug::emacs_json(&text, &ctx)).map_err(|e| e.to_string())?;
        let (mut e, mut k) = (Vec::new(), Vec::new());
        flatten(&ev, "", &mut e);
        flatten(&kv, "", &mut k);
        let key = |n: &Flat| (n.ty.clone(), n.begin, n.path.clone());
        let km: HashMap<_, &Flat> = k.iter().map(|n| (key(n), n)).collect();
        let em: HashMap<_, &Flat> = e.iter().map(|n| (key(n), n)).collect();
        let mut diffs: Vec<String> = Vec::new();
        for n in &e {
            let st = per_type.entry(n.ty.clone()).or_default();
            match km.get(&key(n)) {
                None => {
                    st.emacs_only += 1;
                    diffs.push(format!(
                        "emacs only  {:>8} {} {}",
                        n.begin,
                        n.path,
                        excerpt(&text, n.begin)
                    ));
                }
                Some(_) if n.overflow => st.known += 1,
                Some(m) if (m.end, m.cb, m.ce, m.pa, m.pb) != (n.end, n.cb, n.ce, n.pa, n.pb) => {
                    st.field_diff += 1;
                    diffs.push(format!(
                        "fields      {:>8} {} emacs(end {} cb {:?} ce {:?} pa {:?} pb {}) kalem(end {} cb {:?} ce {:?} pa {:?} pb {})",
                        n.begin, n.path, n.end, n.cb, n.ce, n.pa, n.pb, m.end, m.cb, m.ce, m.pa, m.pb
                    ));
                }
                Some(m) => {
                    let mut bad = Vec::new();
                    for (k, ev) in &n.props {
                        if SKIPPED_PROPS.contains(&k.as_str()) || k.starts_with("attr_") {
                            continue;
                        }
                        let kv = m.props.get(k).unwrap_or(&Value::Null);
                        if kv != ev {
                            bad.push(format!("{k}: emacs {ev} kalem {kv}"));
                        }
                    }
                    if bad.is_empty() {
                        st.both += 1;
                    } else {
                        st.prop_diff += 1;
                        diffs.push(format!(
                            "props       {:>8} {} {}",
                            n.begin,
                            n.path,
                            bad.join("; ")
                        ));
                    }
                }
            }
        }
        for n in &k {
            if !em.contains_key(&key(n)) {
                per_type.entry(n.ty.clone()).or_default().kalem_only += 1;
                diffs.push(format!(
                    "kalem only  {:>8} {} {}",
                    n.begin,
                    n.path,
                    excerpt(&text, n.begin)
                ));
            }
        }
        diffs.sort_by_key(|d| d[12..20].trim().parse::<u64>().unwrap_or(0));
        if diffs.is_empty() {
            files_exact += 1;
        }
        if !opts.summary && !opts.json && !diffs.is_empty() {
            println!("== {} ({} differences)", f.display(), diffs.len());
            for d in diffs.iter().take(opts.show) {
                println!("  {d}");
            }
        }
        report.push(
            serde_json::json!({ "file": f.display().to_string(), "differences": diffs.len() }),
        );
    }
    let (mut tb, mut te, mut tk, mut tf) = (0, 0, 0, 0);
    let known: usize = per_type.values().map(|s| s.known).sum();
    for s in per_type.values() {
        tb += s.both;
        te += s.emacs_only;
        tk += s.kalem_only;
        tf += s.field_diff + s.prop_diff;
    }
    let agreement = |b: usize, e: usize, k: usize, f: usize| {
        let total = b + e + k + f;
        if total == 0 {
            100.0
        } else {
            100.0 * b as f64 / total as f64
        }
    };
    if opts.json {
        let types: serde_json::Map<String, Value> = per_type
            .iter()
            .map(|(t, s)| {
                (t.clone(), serde_json::json!({"both": s.both, "emacs_only": s.emacs_only, "kalem_only": s.kalem_only, "field_diff": s.field_diff, "prop_diff": s.prop_diff}))
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({"files": report, "exact_files": files_exact, "agreement": agreement(tb, te, tk, tf), "types": types})
        );
    } else {
        println!();
        println!(
            "{:<22} {:>8} {:>10} {:>10} {:>8} {:>8} {:>9}",
            "type", "equal", "emacs only", "kalem only", "bounds", "props", "agree"
        );
        for (t, s) in &per_type {
            println!(
                "{t:<22} {:>8} {:>10} {:>10} {:>8} {:>8} {:>8.2}%",
                s.both,
                s.emacs_only,
                s.kalem_only,
                s.field_diff,
                s.prop_diff,
                agreement(
                    s.both,
                    s.emacs_only,
                    s.kalem_only,
                    s.field_diff + s.prop_diff
                )
            );
        }
        println!(
            "{:<22} {tb:>8} {te:>10} {tk:>10} {tf:>17} {:>8.2}%",
            "TOTAL",
            agreement(tb, te, tk, tf)
        );
        println!("files identical to Emacs: {files_exact} of {}", files.len());
        if known > 0 {
            println!("known differences (book/part-2/org-known-differences.org): {known}");
        }
    }
    if opts.emacs_dumps.is_none() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    Ok(if te + tk + tf == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn excerpt(text: &str, at: u64) -> String {
    let at = at as usize;
    if at > text.len() || !text.is_char_boundary(at) {
        return String::new();
    }
    text[at..]
        .chars()
        .take(50)
        .collect::<String>()
        .replace('\n', "⏎")
}

#[allow(dead_code)]
fn _unused(_: &Path) {}
