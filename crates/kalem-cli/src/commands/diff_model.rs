//! `kalem diff-emacs --model`: compares `org-model` with what Org computes
//! (tags, properties, categories, TODO sets, matches), as dumped by
//! `tests/emacs/model.el`.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use org_model::{Document, EntryId, Inherit};
use serde_json::Value;

use super::{DiffOptions, Result, read};

fn find_model_el(explicit: &Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
        return Ok(dir.join("model.el"));
    }
    let mut dir = std::env::current_dir().map_err(|e| e.to_string())?;
    loop {
        let candidate = dir.join("tests/emacs/model.el");
        if candidate.exists() {
            return Ok(candidate);
        }
        if !dir.pop() {
            return Err("cannot find tests/emacs/model.el; pass --dump-el".into());
        }
    }
}

fn emacs_models(files: &[PathBuf], opts: &DiffOptions) -> Result<PathBuf> {
    if let Some(dir) = &opts.emacs_dumps {
        return Ok(dir.clone());
    }
    let model_el = find_model_el(&opts.dump_el)?;
    let dir = std::env::temp_dir().join(format!("kalem-diff-model-{}", std::process::id()));
    let status = Command::new(&opts.emacs)
        .arg("-Q")
        .arg("--batch")
        .arg("-l")
        .arg(&model_el)
        .arg("--batch-dir")
        .arg(&dir)
        .args(files)
        .status()
        .map_err(|e| format!("cannot run {}: {e}", opts.emacs))?;
    if !status.success() {
        return Err(format!("{} exited with {status}", opts.emacs));
    }
    Ok(dir)
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|x| x.as_str().unwrap_or("").to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn pairs(v: &Value) -> Vec<(String, String)> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|p| {
                    let s = |i: usize| p.get(i).and_then(|x| x.as_str()).unwrap_or("").to_string();
                    (s(0), s(1))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Default)]
struct Tally {
    checked: BTreeMap<&'static str, usize>,
    failed: BTreeMap<&'static str, usize>,
}

struct Ctx<'a> {
    file: &'a Path,
    shown: usize,
    show: usize,
    tally: &'a mut Tally,
    out: &'a mut Vec<String>,
}

impl Ctx<'_> {
    fn check<T: PartialEq + std::fmt::Debug>(
        &mut self,
        what: &'static str,
        at: &str,
        emacs: T,
        kalem: T,
    ) {
        *self.tally.checked.entry(what).or_default() += 1;
        if emacs != kalem {
            *self.tally.failed.entry(what).or_default() += 1;
            if self.shown < self.show {
                self.shown += 1;
                self.out.push(format!(
                    "{} {at} {what}:\n    emacs {emacs:?}\n    kalem {kalem:?}",
                    self.file.display()
                ));
            }
        }
    }
}

fn sorted(mut v: Vec<(String, String)>) -> Vec<(String, String)> {
    v.sort();
    v
}

pub(crate) fn diff_model(files: &[PathBuf], opts: &DiffOptions) -> Result<ExitCode> {
    let dir = emacs_models(files, opts)?;
    let mut tally = Tally::default();
    let mut lines = Vec::new();
    let mut identical = 0;
    for (i, f) in files.iter().enumerate() {
        let json_path = super::diff_emacs::dump_path(&dir, f, i, opts);
        let Ok(json) = std::fs::read_to_string(&json_path) else {
            lines.push(format!("{}: no Emacs model", f.display()));
            continue;
        };
        let em: Value =
            serde_json::from_str(&json).map_err(|e| format!("{}: {e}", json_path.display()))?;
        let text = read(f)?;
        let doc = Document::new(org_syntax::parse_file(&text, f));
        let before = tally.failed.values().sum::<usize>();
        let mut c = Ctx {
            file: f,
            shown: 0,
            show: opts.show,
            tally: &mut tally,
            out: &mut lines,
        };
        compare(&mut c, &em, &doc);
        if c.tally.failed.values().sum::<usize>() == before {
            identical += 1;
        }
    }
    if !opts.emacs_dumps.is_some() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    let mut out = std::io::stdout().lock();
    if !opts.summary {
        for l in &lines {
            writeln!(out, "{l}").map_err(|e| e.to_string())?;
        }
    }
    writeln!(
        out,
        "{:<22} {:>9} {:>9} {:>8}",
        "field", "checked", "differ", "agree"
    )
    .map_err(|e| e.to_string())?;
    let (mut tc, mut tf) = (0, 0);
    for (k, n) in &tally.checked {
        let f = tally.failed.get(k).copied().unwrap_or(0);
        tc += n;
        tf += f;
        writeln!(
            out,
            "{k:<22} {n:>9} {f:>9} {:>7.2}%",
            100.0 * (n - f) as f64 / *n as f64
        )
        .map_err(|e| e.to_string())?;
    }
    writeln!(
        out,
        "{:<22} {tc:>9} {tf:>9} {:>7.2}%",
        "TOTAL",
        100.0 * (tc - tf) as f64 / tc.max(1) as f64
    )
    .map_err(|e| e.to_string())?;
    writeln!(
        out,
        "files identical to Emacs: {identical} of {}",
        files.len()
    )
    .map_err(|e| e.to_string())?;
    Ok(if tf == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn compare(c: &mut Ctx<'_>, em: &Value, doc: &Document) {
    let ctx = doc.parse().context();
    let sets: Vec<Vec<String>> = ctx
        .todo_sequences
        .iter()
        .map(|s| s.keywords.iter().map(|k| k.name.clone()).collect())
        .collect();
    let em_sets: Vec<Vec<String>> = em["todo-sets"]
        .as_array()
        .map(|a| a.iter().map(strings).collect())
        .unwrap_or_default();
    c.check("todo-sets", "document", em_sets, sets);
    let all: Vec<String> = ctx
        .todo_sequences
        .iter()
        .flat_map(|s| s.keywords.iter().map(|k| k.name.clone()))
        .collect();
    c.check(
        "todo-keywords",
        "document",
        strings(&em["todo-keywords"]),
        all,
    );
    c.check(
        "done-keywords",
        "document",
        strings(&em["done-keywords"]),
        ctx.done_keywords.clone(),
    );
    let info = doc.info();
    c.check(
        "file-tags",
        "document",
        strings(&em["file-tags"]),
        info.file_tags.clone(),
    );
    c.check(
        "keyword-properties",
        "document",
        pairs(&em["keyword-properties"]),
        info.keyword_properties.clone(),
    );
    let p = &em["priorities"];
    let ep = (
        p[0].as_u64().unwrap_or(0) as u32,
        p[1].as_u64().unwrap_or(0) as u32,
        p[2].as_u64().unwrap_or(0) as u32,
    );
    c.check(
        "priorities",
        "document",
        ep,
        (
            info.priorities.highest,
            info.priorities.lowest,
            info.priorities.default,
        ),
    );

    let outline = doc.outline();
    let doc_names: BTreeSet<String> = {
        use org_syntax::ast::{AstNode, Keyword, NodeProperty};
        let root = doc.parse().syntax();
        let mut n = BTreeSet::new();
        for d in root.descendants() {
            if let Some(p) = NodeProperty::cast(d.clone()) {
                n.insert(p.key().to_uppercase().trim_end_matches('+').to_string());
            } else if let Some(k) = Keyword::cast(d)
                && k.key() == "PROPERTY"
                && let Some(w) = k.value().split_whitespace().next()
            {
                n.insert(w.to_uppercase().trim_end_matches('+').to_string());
            }
        }
        n
    };
    let by_begin: BTreeMap<u64, EntryId> = outline
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| (u64::from(u32::from(e.range.start())), EntryId(i)))
        .collect();
    let empty = Vec::new();
    let clock_sums = doc.clock_sums();
    for h in em["headlines"].as_array().unwrap_or(&empty) {
        let begin = h["begin"].as_u64().unwrap_or(0);
        let at = format!("@{begin}");
        let Some(&id) = by_begin.get(&begin) else {
            c.check("headline", &at, "present", "missing");
            continue;
        };
        let e = doc.entry(id);
        c.check(
            "level",
            &at,
            h["level"].as_u64().unwrap_or(0) as usize,
            e.level,
        );
        c.check(
            "todo",
            &at,
            h["todo"].as_str().map(str::to_string),
            e.todo.clone(),
        );
        c.check(
            "local-tags",
            &at,
            strings(&h["local-tags"]),
            e.local_tags.clone(),
        );
        c.check("tags", &at, strings(&h["tags"]), doc.tags(id));
        c.check(
            "clock",
            &at,
            h["clock"].as_i64(),
            clock_sums.get(&(begin as usize)).copied(),
        );
        c.check(
            "category",
            &at,
            h["category"].as_str().unwrap_or("").to_string(),
            doc.category(Some(id)),
        );
        c.check(
            "standard",
            &at,
            sorted(pairs(&h["standard"])),
            sorted(doc.standard_properties(Some(id))),
        );
        c.check(
            "special",
            &at,
            sorted(pairs(&h["special"])),
            sorted(doc.special_properties(Some(id))),
        );
        // Property lookups, for every name Emacs reported plus the drawer
        // keys Kalem sees.
        let mut names: BTreeSet<String> = BTreeSet::new();
        for k in ["get", "selective", "inherit"] {
            names.extend(pairs(&h[k]).into_iter().map(|(n, _)| n));
        }
        for n in [
            "CATEGORY",
            "ARCHIVE",
            "COLUMNS",
            "LOGGING",
            "ID",
            "CUSTOM_ID",
            "EFFORT",
        ] {
            names.insert(n.to_string());
        }
        // Like model.el: names used in the document itself (drawers and
        // `#+PROPERTY` lines, not setup files).
        names.extend(doc_names.iter().cloned());
        // Allowed values, for TODO, PRIORITY and every property name.
        let em_allowed: BTreeMap<String, (Vec<String>, bool)> = h["allowed"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|x| {
                        (
                            x[0].as_str().unwrap_or("").to_string(),
                            (strings(&x[1]), x[2].as_bool().unwrap_or(false)),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        for n in std::iter::once("TODO".to_string())
            .chain(std::iter::once("PRIORITY".to_string()))
            .chain(names.iter().cloned())
        {
            let a = doc.allowed_values(Some(id), &n);
            let kalem = (!a.values.is_empty()).then(|| (a.values.clone(), a.unrestricted));
            c.check(
                "allowed-values",
                &format!("{at} {n}"),
                em_allowed.get(&n).cloned(),
                kalem,
            );
        }
        for (field, inherit) in [
            ("get", Inherit::No),
            ("selective", Inherit::Selective),
            ("inherit", Inherit::Yes),
        ] {
            let emap: BTreeMap<String, String> = pairs(&h[field]).into_iter().collect();
            for n in &names {
                c.check(
                    match field {
                        "get" => "entry-get",
                        "selective" => "entry-get selective",
                        _ => "entry-get inherit",
                    },
                    &format!("{at} {n}"),
                    emap.get(n).cloned(),
                    doc.entry_get(Some(id), n, inherit, false),
                );
            }
        }
    }
    // Match strings from `#+KALEM_MATCH:` keywords.
    let now = jiff::Zoned::now().datetime();
    for m in em["matches"].as_array().unwrap_or(&empty) {
        let input = m["match"].as_str().unwrap_or("");
        let begins: Vec<Value> = m["begins"].as_array().cloned().unwrap_or_default();
        if begins.iter().any(|b| b.is_string()) {
            // Emacs signaled an error for this match string.
            continue;
        }
        let emacs: Vec<u64> = begins.iter().filter_map(Value::as_u64).collect();
        let kalem: Vec<u64> = doc
            .matching(input, now)
            .into_iter()
            .map(|id| u64::from(u32::from(doc.entry(id).range.start())))
            .collect();
        c.check("match", &format!("{input:?}"), emacs, kalem);
    }
    // Internal links and their targets.
    let kalem_links: BTreeMap<u64, (String, Option<u64>)> = doc
        .internal_links()
        .into_iter()
        .map(|l| (l.begin as u64, (l.link_type, l.target.map(|t| t as u64))))
        .collect();
    for l in em["links"].as_array().unwrap_or(&empty) {
        let begin = l[0].as_u64().unwrap_or(0);
        let emacs = (l[1].as_str().unwrap_or("").to_string(), l[3].as_u64());
        let kalem = kalem_links.get(&begin).cloned();
        c.check(
            "link-target",
            &format!("@{begin} {:?}", l[2].as_str().unwrap_or("")),
            Some(emacs),
            kalem,
        );
    }
    // Footnote references and their definitions.
    let kalem_fn: BTreeMap<u64, Option<u64>> = doc
        .footnote_references()
        .into_iter()
        .filter(|r| r.label.is_some())
        .map(|r| (r.begin as u64, r.definition.map(|d| d as u64)))
        .collect();
    for f in em["footnotes"].as_array().unwrap_or(&empty) {
        let begin = f[0].as_u64().unwrap_or(0);
        c.check(
            "footnote",
            &format!("@{begin} {}", f[1].as_str().unwrap_or("")),
            Some(f[2].as_u64()),
            kalem_fn.get(&begin).copied(),
        );
    }
    // Statistics cookies after `org-update-statistics-cookies`.
    let kalem_cookies: BTreeMap<u64, String> = doc
        .statistics_cookies()
        .into_iter()
        .map(|(b, t)| (b as u64, t))
        .collect();
    for ck in em["cookies"].as_array().unwrap_or(&empty) {
        let begin = ck[0].as_u64().unwrap_or(0);
        let emacs = ck[2].as_str().map(str::to_string);
        c.check(
            "statistics-cookie",
            &format!("@{begin} {}", ck[1].as_str().unwrap_or("")),
            emacs,
            kalem_cookies.get(&begin).cloned(),
        );
    }
    for begin in by_begin.keys() {
        let present = em["headlines"]
            .as_array()
            .is_some_and(|a| a.iter().any(|h| h["begin"].as_u64() == Some(*begin)));
        if !present {
            c.check("headline", &format!("@{begin}"), "missing", "present");
        }
    }
}
