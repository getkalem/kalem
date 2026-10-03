//! `kalem latex-coverage DIR...`: how much of a corpus of LaTeX sources
//! the rendered view covers (T2.7h.1). A folder holds fields, a field
//! papers, a paper its files (`arxiv/math/2401.00001/main.tex`): the
//! report gives, overall and by field, the share of the body that shows
//! as source and the share in formulas, and the most frequent commands
//! and environments with how many papers use them and whether the view
//! renders them. The files a document brings in before
//! `\begin{document}` (its macros) are preamble, left out, and so are
//! the files no document of the paper reads (drafts, leftovers).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::Result;

/// A command's or environment's uses, the papers using it, and whether
/// the view renders it.
type Use = (usize, HashSet<String>, bool);

/// Files larger than this are left out (generated data, not prose).
const MAX_FILE: u64 = 2 * 1024 * 1024;

#[derive(Default)]
struct Totals {
    papers: HashSet<String>,
    files: usize,
    body: usize,
    source: usize,
    math: usize,
    tex: usize,
}

impl Totals {
    fn add(&mut self, paper: &str, c: &kalem_core::latex_check::Coverage) {
        self.papers.insert(paper.to_string());
        self.files += 1;
        self.body += c.body;
        self.source += c.source;
        self.math += c.math;
        self.tex += c.tex;
    }

    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "papers": self.papers.len(),
            "files": self.files,
            "body_bytes": self.body,
            "source_bytes": self.source,
            "math_bytes": self.math,
            "tex_bytes": self.tex,
            "source_share": share(self.source, self.body),
            "math_share": share(self.math, self.body),
        })
    }
}

fn share(a: usize, b: usize) -> f64 {
    if b == 0 {
        0.
    } else {
        (a as f64 / b as f64 * 10_000.).round() / 10_000.
    }
}

/// The `.tex` files under `dir`, with their field and paper: the first
/// and first two parts of their path below `dir`.
fn files(dir: &Path) -> Vec<(PathBuf, String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("tex"))
                && e.metadata().is_ok_and(|m| m.len() <= MAX_FILE)
            {
                let parts: Vec<String> = p
                    .strip_prefix(dir)
                    .unwrap_or(&p)
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                let field = if parts.len() > 2 {
                    parts[0].clone()
                } else {
                    "-".to_string()
                };
                let paper = if parts.len() > 2 {
                    format!("{}/{}", parts[0], parts[1])
                } else {
                    parts.first().cloned().unwrap_or_default()
                };
                out.push((p, field, paper));
            }
        }
    }
    out.sort();
    out
}

/// The files a document brings in before `\\begin{document}` (its
/// macros, its preamble in parts): preamble, not body text.
fn preamble_files(files: &[(PathBuf, String, String)]) -> HashSet<PathBuf> {
    let mut out = HashSet::new();
    for (f, _, _) in files {
        let Ok(bytes) = std::fs::read(f) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let Some(begin) = text.find("\\begin{document}") else {
            continue;
        };
        let dir = f.parent().unwrap_or(Path::new(""));
        let mut rest = &text[..begin];
        while let Some(i) = rest.find(['\\']) {
            rest = &rest[i + 1..];
            let Some(name) = ["input", "include", "usepackage", "RequirePackage"]
                .iter()
                .find(|n| rest.starts_with(*n))
            else {
                continue;
            };
            let arg = rest[name.len()..].trim_start();
            let Some(inner) = arg.strip_prefix('{').and_then(|a| a.split_once('}')) else {
                continue;
            };
            for part in inner.0.split(',') {
                let p = dir.join(part.trim());
                for cand in [p.clone(), p.with_extension("tex"), p.with_extension("sty")] {
                    if cand.is_file() {
                        out.insert(cand);
                    }
                }
            }
        }
    }
    out
}

/// The files of `list` no document of their paper reads: a paper's roots
/// (`\\documentclass` and `\\begin{document}`) and what each brings in
/// are the document; drafts and leftovers beside it are not typeset by
/// anyone. A paper without a root keeps all its files.
fn unread_files(list: &[(PathBuf, String, String)]) -> HashSet<PathBuf> {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let mut by_paper: BTreeMap<&str, Vec<&PathBuf>> = BTreeMap::new();
    for (f, _, paper) in list {
        by_paper.entry(paper.as_str()).or_default().push(f);
    }
    let mut out = HashSet::new();
    for files in by_paper.values() {
        let roots: Vec<&&PathBuf> = files
            .iter()
            .filter(|f| {
                std::fs::read(f).is_ok_and(|b| {
                    let t = String::from_utf8_lossy(&b);
                    t.contains("\\documentclass") && t.contains("\\begin{document}")
                })
            })
            .collect();
        if roots.is_empty() {
            continue;
        }
        let mut read: HashSet<PathBuf> = HashSet::new();
        for r in roots {
            let project =
                latex_model::project::ProjectCache::default().load(r, &latex_model::project::Disk);
            read.insert(canon(r));
            read.extend(project.model.files.iter().map(|f| canon(f)));
        }
        for f in files {
            if !read.contains(&canon(f)) {
                out.insert((*f).clone());
            }
        }
    }
    out
}

pub(crate) fn latex_coverage(dirs: &[PathBuf], json: bool, top: usize) -> Result<ExitCode> {
    let mut all = Totals::default();
    let mut fields: BTreeMap<String, Totals> = BTreeMap::new();
    // Per name: uses, the papers using it, rendered.
    let mut names: HashMap<String, Use> = HashMap::new();
    // Per name: the bytes it shows as source.
    let mut source_bytes: HashMap<String, usize> = HashMap::new();
    // Per name: the papers it shows source in, and an example.
    let mut source_papers: HashMap<String, HashSet<String>> = HashMap::new();
    let mut examples: HashMap<String, String> = HashMap::new();
    // Per renderer message: the bytes of the formulas TeX typesets, and
    // their papers.
    let mut tex_bytes: HashMap<String, usize> = HashMap::new();
    let mut tex_papers: HashMap<String, HashSet<String>> = HashMap::new();
    let base = kalem_core::settings::Config::default().parse_base();
    let mut unread_count = 0;
    // `KALEM_COVERAGE_TRACE="vu E begin{lemma}"`: why those macros are
    // undefined, and why those environments show as source.
    let trace: Vec<String> = std::env::var("KALEM_COVERAGE_TRACE")
        .unwrap_or_default()
        .split_whitespace()
        .map(|t| format!("\\{}", t.trim_start_matches('\\')))
        .collect();
    let mut traced: HashSet<(PathBuf, String)> = HashSet::new();
    // `KALEM_COVERAGE_WHERE="alpha begin{prof}"`: the files showing those
    // names as source, and how much.
    let wanted: Vec<String> = std::env::var("KALEM_COVERAGE_WHERE")
        .unwrap_or_default()
        .split_whitespace()
        .map(|t| format!("\\{}", t.trim_start_matches('\\')))
        .collect();
    // `KALEM_COVERAGE_SHOW="Mismatch|got '_'"`: the source, as the view gives
    // it to the renderer, of the first formulas whose error holds one of
    // these, three each.
    let show: Vec<String> = std::env::var("KALEM_COVERAGE_SHOW")
        .unwrap_or_default()
        .split('|')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    let mut shown: HashMap<String, usize> = HashMap::new();
    for dir in dirs {
        let list = files(dir);
        let preamble = preamble_files(&list);
        let unread = unread_files(&list);
        unread_count += unread.len();
        for (f, field, paper) in list {
            if preamble.contains(&f) || unread.contains(&f) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&f) else {
                continue;
            };
            let text = String::from_utf8_lossy(&bytes);
            let mut c = kalem_core::latex_check::coverage_report(&text, Some(&f));
            for name in &wanted {
                if let Some(n) = c.source_by_name.get(name) {
                    eprintln!("where {name}: {} ({n} bytes)", f.display());
                }
            }
            // Formulas the renderer cannot read are shown as source.
            if let Ok(mut doc) = kalem_core::DocumentState::open(
                &f,
                std::sync::Arc::new(org_model::Settings::default()),
                &base,
            ) {
                // The definitions of the files the document reads.
                doc.wait_for_latex_project();
                // A traced macro shown as source in the text: its
                // definition as the model read it.
                for name in &trace {
                    if c.source_by_name.contains_key(name)
                        && traced.insert((f.clone(), format!("text {name}")))
                    {
                        // `begin{name}`: an environment.
                        let why = match name
                            .strip_prefix("\\begin{")
                            .and_then(|n| n.strip_suffix('}'))
                        {
                            Some(env) => kalem_core::latex_view::explain_environment(&doc, env, &f),
                            None => kalem_core::latex_view::explain_macro(&doc, name),
                        };
                        eprintln!("trace {} {name} [text]: {why}", f.display());
                    }
                }
                for (r, kind) in kalem_core::latex_view::formula_failures(&doc) {
                    // Typeset by TeX in the view (when it is installed):
                    // counted apart, not as source.
                    let n = r.len();
                    c.tex += n;
                    if let Some(name) = kind.strip_prefix("Undefined control sequence: ")
                        && trace.iter().any(|t| t == name)
                        && traced.insert((f.clone(), name.to_string()))
                    {
                        eprintln!(
                            "trace {} {name}: {}",
                            f.display(),
                            kalem_core::latex_view::explain_macro(&doc, name)
                        );
                    }
                    for pat in show.iter().filter(|p| kind.contains(p.as_str())) {
                        let n = shown.entry(pat.clone()).or_insert(0);
                        if *n < 3 {
                            *n += 1;
                            let src = kalem_core::latex_view::math_source(&doc, r.clone())
                                .unwrap_or_default();
                            eprintln!(
                                "show {} [{kind}]: {}",
                                f.display(),
                                src.chars().take(700).collect::<String>()
                            );
                        }
                    }
                    // A traced macro in a formula that fails otherwise.
                    let text = doc.text().as_str();
                    for name in &trace {
                        let src = &text[r.clone()];
                        let used = src.match_indices(name.as_str()).any(|(k, _)| {
                            !src[k + name.len()..].starts_with(|c: char| c.is_ascii_alphabetic())
                        });
                        if !kind.contains(name.as_str())
                            && used
                            && traced.insert((f.clone(), name.clone()))
                        {
                            eprintln!(
                                "trace {} {name} [{kind}]: {}",
                                f.display(),
                                kalem_core::latex_view::explain_macro(&doc, name)
                            );
                        }
                    }
                    let key = format!("formula: {kind}");
                    *tex_bytes.entry(key.clone()).or_insert(0) += n;
                    tex_papers
                        .entry(key.clone())
                        .or_default()
                        .insert(paper.clone());
                    examples.entry(key).or_insert_with(|| {
                        kalem_core::latex_check::example(&doc.text().as_str()[r.clone()])
                    });
                }
            }
            all.add(&paper, &c);
            fields.entry(field).or_default().add(&paper, &c);
            for (name, b) in &c.source_by_name {
                *source_bytes.entry(name.clone()).or_insert(0) += b;
                source_papers
                    .entry(name.clone())
                    .or_default()
                    .insert(paper.clone());
            }
            for (name, e) in c.examples.drain() {
                examples.entry(name).or_insert(e);
            }
            for (name, (n, rendered)) in c.names {
                let e = names
                    .entry(name)
                    .or_insert_with(|| (0, HashSet::new(), rendered));
                e.0 += n;
                e.1.insert(paper.clone());
            }
        }
    }
    if all.files == 0 {
        return Err("No .tex files found".into());
    }
    let mut ranked: Vec<(&String, &Use)> = names.iter().collect();
    // By how many papers use it, then how often: one long document's own
    // macros do not crowd out what most papers use.
    ranked.sort_by(|a, b| {
        b.1.1
            .len()
            .cmp(&a.1.1.len())
            .then(b.1.0.cmp(&a.1.0))
            .then(a.0.cmp(b.0))
    });
    let row = |(name, (n, papers, rendered)): &(&String, &Use)| {
        (name.to_string(), *n, papers.len(), *rendered)
    };
    let most: Vec<_> = ranked.iter().take(top).map(row).collect();
    let unrendered: Vec<_> = ranked
        .iter()
        .filter(|(_, (_, _, r))| !r)
        .take(top)
        .map(row)
        .collect();
    let mut heaviest: Vec<(&String, &usize)> = source_bytes.iter().collect();
    heaviest.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    heaviest.truncate(top);
    let mut out = std::io::stdout().lock();
    if json {
        let list = |v: &[(String, usize, usize, bool)]| -> Vec<serde_json::Value> {
            v.iter()
                .map(|(name, n, p, r)| {
                    serde_json::json!({"name": name, "uses": n, "papers": p, "rendered": r})
                })
                .collect()
        };
        let by_field: serde_json::Map<String, serde_json::Value> =
            fields.iter().map(|(k, t)| (k.clone(), t.json())).collect();
        let v = serde_json::json!({
            "total": all.json(),
            "fields": by_field,
            "most_frequent": list(&most),
            "most_frequent_unrendered": list(&unrendered),
            "most_source": heaviest
                .iter()
                .map(|(name, b)| serde_json::json!({"name": name, "source_bytes": b, "share": share(**b, all.source)}))
                .collect::<Vec<_>>(),
        });
        writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
        )
        .map_err(|e| e.to_string())?;
        return Ok(ExitCode::SUCCESS);
    }
    let pct = |a: usize, b: usize| format!("{:.2}%", share(a, b) * 100.);
    let mut s = String::new();
    s.push_str(&format!(
        "{} papers, {} files, {} KB of body text: {} shows as source, {} is formulas, {} formulas TeX typesets (the math renderer cannot read them; without TeX they show as source) ({} files no document reads left out)\n\n",
        all.papers.len(),
        all.files,
        all.body / 1024,
        pct(all.source, all.body),
        pct(all.math, all.body),
        pct(all.tex, all.body),
        unread_count
    ));
    s.push_str(
        "| field | papers | files | body KB | source | formulas | by TeX |\n|---|---:|---:|---:|---:|---:|---:|\n",
    );
    for (k, t) in &fields {
        s.push_str(&format!(
            "| {k} | {} | {} | {} | {} | {} | {} |\n",
            t.papers.len(),
            t.files,
            t.body / 1024,
            pct(t.source, t.body),
            pct(t.math, t.body),
            pct(t.tex, t.body)
        ));
    }
    let table = |title: &str, v: &[(String, usize, usize, bool)]| {
        let mut s =
            format!("\n{title}\n\n| name | uses | papers | rendered |\n|---|---:|---:|---|\n");
        for (name, n, p, r) in v {
            s.push_str(&format!(
                "| `{name}` | {n} | {p} | {} |\n",
                if *r { "yes" } else { "no" }
            ));
        }
        s
    };
    s.push_str(&table(
        &format!(
            "The {} most frequent commands and environments (outside formulas)",
            most.len()
        ),
        &most,
    ));
    s.push_str(&table(
        &format!("The {} most frequent shown as source", unrendered.len()),
        &unrendered,
    ));
    s.push_str(&format!(
        "\nThe {} names showing the most source\n\n| name | source KB | share of source | papers | example |\n|---|---:|---:|---:|---|\n",
        heaviest.len()
    ));
    for (name, b) in &heaviest {
        let papers = source_papers.get(*name).map_or(0, HashSet::len);
        let ex = examples
            .get(*name)
            .map(|e| e.replace('|', "\\|").replace('`', "'"))
            .unwrap_or_default();
        s.push_str(&format!(
            "| `{name}` | {:.1} | {} | {papers} | {ex} |\n",
            **b as f64 / 1024.,
            pct(**b, all.source)
        ));
    }
    let mut by_tex: Vec<(&String, &usize)> = tex_bytes.iter().collect();
    by_tex.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    by_tex.truncate(top);
    s.push_str(&format!(
        "\nThe {} renderer messages of the formulas TeX typesets\n\n| message | KB | papers | example |\n|---|---:|---:|---|\n",
        by_tex.len()
    ));
    for (name, b) in &by_tex {
        let papers = tex_papers.get(*name).map_or(0, HashSet::len);
        let ex = examples
            .get(*name)
            .map(|e| e.replace('|', "\\|").replace('`', "'"))
            .unwrap_or_default();
        s.push_str(&format!(
            "| `{name}` | {:.1} | {papers} | {ex} |\n",
            **b as f64 / 1024.
        ));
    }
    write!(out, "{s}").map_err(|e| e.to_string())?;
    Ok(ExitCode::SUCCESS)
}
