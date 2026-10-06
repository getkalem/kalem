//! Diagnostics of LaTeX documents (T2.7h.20) for `kalem check FILE.tex`
//! and the editor: what the parser closed or skipped, labels referred to
//! and not defined or defined twice (across the files of the project),
//! citation keys no bibliography has, files and pictures not found,
//! commands LaTeX 2ε deprecates, a few of chktex's rules; and the report
//! of what a document leaves as source (T2.7h.13).

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use latex_model::project::{Disk, ProjectCache};
use latex_syntax::SyntaxKind as K;

/// How much a diagnostic matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Probably wrong.
    Warning,
    /// Style.
    Info,
}

/// A diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Where, in bytes.
    pub range: Range<usize>,
    /// How much it matters.
    pub severity: Severity,
    /// A stable name for it.
    pub code: &'static str,
    /// What, for the user.
    pub message: String,
    /// The edit that fixes it, when one is obvious: the range replaced
    /// and the text put there.
    pub fix: Option<(Range<usize>, String)>,
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (dunce::canonicalize(a), dunce::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// The diagnostics of a LaTeX text that need no other file: what the
/// parser closed or skipped, deprecated commands and chktex's rules, with
/// their fixes. The editor shows these as the text changes.
pub fn text_diagnostics(parse: &latex_syntax::Parse) -> Vec<Diagnostic> {
    let mut out: Vec<Diagnostic> = parse
        .diagnostics()
        .iter()
        .map(|d| Diagnostic {
            range: d.range.clone(),
            severity: Severity::Warning,
            code: "latex-syntax",
            message: d.message.clone(),
            fix: None,
        })
        .collect();
    out.extend(style_diagnostics(&parse.syntax()));
    out.sort_by_key(|d| (d.range.start, d.range.end));
    out
}

/// The fix of the diagnostic at the cursor of `sel`, or else of the first
/// one on its line that has a fix (Quick Fix): the replacement, with the
/// cursor after it.
pub fn quick_fix(
    text: &str,
    sel: org_edit::Selection,
    root: &latex_syntax::SyntaxNode,
) -> Option<org_edit::Transaction> {
    let fixable: Vec<Diagnostic> = style_diagnostics(root)
        .into_iter()
        .filter(|d| d.fix.is_some())
        .collect();
    let pos = sel.head;
    let line_start = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
    let d = at(&fixable, pos).or_else(|| {
        fixable
            .iter()
            .find(|d| line_start <= d.range.start && d.range.start <= line_end)
    })?;
    let (range, insert) = d.fix.clone()?;
    let head = range.start + insert.len();
    let mut tx = org_edit::Transaction::new("Quick Fix");
    tx.replace(range, insert).ok()?;
    Some(tx.select(org_edit::Selection::caret(head)))
}

/// Deprecated commands and chktex's rules, with their fixes.
fn style_diagnostics(root_node: &latex_syntax::SyntaxNode) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    // Deprecated commands and chktex's rules, in text.
    // Math and verbatim around a token, counted in one walk (asking each
    // token's ancestors is quadratic on deep nesting).
    let is_math_node = |a: &latex_syntax::SyntaxNode| {
        matches!(a.kind(), K::INLINE_MATH | K::DISPLAY_MATH)
            || (a.kind() == K::ENVIRONMENT
                && latex_syntax::name(a).is_some_and(|n| {
                    latex_syntax::signatures::is_math(&n)
                        || latex_syntax::signatures::is_verbatim(&n)
                }))
    };
    let mut math_depth = 0usize;
    let mut shorthand: Option<bool> = None;
    for event in root_node.preorder_with_tokens() {
        let t = match event {
            latex_syntax::WalkEvent::Enter(e) => {
                if let Some(n) = e.as_node() {
                    if is_math_node(n) {
                        math_depth += 1;
                    }
                    continue;
                }
                match e.into_token() {
                    Some(t) => t,
                    None => continue,
                }
            }
            latex_syntax::WalkEvent::Leave(e) => {
                if let Some(n) = e.as_node()
                    && is_math_node(n)
                {
                    math_depth -= 1;
                }
                continue;
            }
        };
        let range = usize::from(t.text_range().start())..usize::from(t.text_range().end());
        let in_math = math_depth > 0;
        let info = |code: &'static str, key: &str| Diagnostic {
            range: range.clone(),
            severity: Severity::Info,
            code,
            message: crate::l10n::tr(key),
            fix: None,
        };
        match t.kind() {
            K::CONTROL_WORD => {
                let name = &t.text()[1..];
                if matches!(name, "bf" | "it" | "rm" | "sc" | "sf" | "tt" | "sl" | "cal") {
                    // The LaTeX 2ε declaration that does the same; in math
                    // the `\math…` command, which takes its argument in
                    // braces, so nothing replaces it word for word there
                    // (`$\bfseries x$` does not compile).
                    let modern = match name {
                        "bf" => Some("\\bfseries"),
                        "it" => Some("\\itshape"),
                        "rm" => Some("\\rmfamily"),
                        "sc" => Some("\\scshape"),
                        "sf" => Some("\\sffamily"),
                        "tt" => Some("\\ttfamily"),
                        "sl" => Some("\\slshape"),
                        _ => None,
                    };
                    let math = match name {
                        "bf" => Some("\\mathbf"),
                        "it" => Some("\\mathit"),
                        "rm" => Some("\\mathrm"),
                        "sf" => Some("\\mathsf"),
                        "tt" => Some("\\mathtt"),
                        "cal" => Some("\\mathcal"),
                        _ => None,
                    };
                    let (message, fix) = match (in_math, math) {
                        (true, Some(m)) => (
                            crate::tr!("latex-deprecated-font-math", command = name, math = m),
                            None,
                        ),
                        (true, None) => (crate::tr!("latex-deprecated-font", command = name), None),
                        (false, _) => (
                            crate::tr!("latex-deprecated-font", command = name),
                            modern.map(|m| (range.clone(), m.to_string())),
                        ),
                    };
                    out.push(Diagnostic {
                        range: range.clone(),
                        severity: Severity::Info,
                        code: "latex-deprecated",
                        message,
                        fix,
                    });
                }
                if matches!(
                    name,
                    "ref" | "eqref" | "cite" | "cref" | "autoref" | "pageref"
                ) && !in_math
                    && t.prev_token().is_some_and(|p| p.kind() == K::WHITESPACE)
                    && t.prev_token()
                        .and_then(|p| p.prev_token())
                        .is_some_and(|w| w.kind() == K::TEXT)
                {
                    let mut d = info("latex-tie", "latex-tie");
                    d.fix = t.prev_token().map(|w| {
                        (
                            usize::from(w.text_range().start())..usize::from(w.text_range().end()),
                            "~".to_string(),
                        )
                    });
                    out.push(d);
                }
            }
            K::DOUBLE_DOLLAR
                if t.parent()
                    .and_then(|p| p.children_with_tokens().next())
                    .is_some_and(|f| f.as_token() == Some(&t)) =>
            {
                let mut d = info("latex-deprecated", "latex-double-dollar");
                // `\[…\]` for `$$…$$`.
                d.fix = t.parent().and_then(|m| {
                    let r = usize::from(m.text_range().start())..usize::from(m.text_range().end());
                    let src = m.text().to_string();
                    let body = src.strip_prefix("$$")?.strip_suffix("$$")?;
                    Some((r, format!("\\[{body}\\]")))
                });
                out.push(d);
            }
            K::TEXT if !in_math => {
                let s = t.text();
                if let Some(i) = s.find("...") {
                    let mut d = info("latex-ellipsis", "latex-ellipsis");
                    let at = range.start + i;
                    d.range = at..at + 3;
                    // `\ldots{}`: the braces keep the space or letter after it.
                    d.fix = Some((d.range.clone(), "\\ldots{}".to_string()));
                    out.push(d);
                }
                // Not where babel makes `"` a shorthand (`Stra"se`).
                if s.contains('"')
                    && !*shorthand.get_or_insert_with(|| {
                        crate::latex_edit::quote_shorthand(&root_node.text().to_string())
                    })
                {
                    out.push(info("latex-quotes", "latex-quotes"));
                }
            }
            _ => {}
        }
    }
    out.sort_by_key(|d| (d.range.start, d.range.end));
    out
}

/// The diagnostics of a LaTeX document as it is edited (T2.7h.20):
/// worked out on a thread after a pause in typing, as `kalem check` does
/// (the project's other files from the disk, this one as edited), and
/// shown while the text is the one they were worked out for.
#[derive(Debug, Default)]
pub struct Live {
    /// The diagnostics, the version they are for, and a count of the
    /// times they were installed.
    shown: Option<(u64, Arc<Vec<Diagnostic>>)>,
    installed: u64,
    /// The builds recorded when the shown ones were worked out.
    builds: u64,
    /// The shown ones were carried through edits, not worked out for the
    /// text as it is: a fresh pass is due.
    mapped: bool,
    pending: Option<(u64, Receiver<Vec<Diagnostic>>)>,
    /// The version last seen changing, and when.
    changed: Option<(u64, Instant)>,
}

/// The pause in typing after which diagnostics are worked out again.
const PAUSE: Duration = Duration::from_millis(400);

impl Live {
    /// The diagnostics of `version` of the text, when they are known.
    pub fn current(&self, version: u64) -> Option<&Arc<Vec<Diagnostic>>> {
        self.shown
            .as_ref()
            .filter(|(v, _)| *v == version)
            .map(|(_, d)| d)
    }

    /// Whether a pass for `version` of the text is still to come.
    pub fn due(&self, version: u64) -> bool {
        self.mapped || self.current(version).is_none()
    }

    /// How many times diagnostics were installed: caches of what they
    /// show start again when it changes.
    pub fn generation(&self) -> u64 {
        self.installed
    }

    /// Collects diagnostics that finished, and starts working them out for
    /// `version` of `text` after a pause (at once the first time); `true`
    /// when new ones are to be shown.
    pub(crate) fn poll(&mut self, version: u64, path: Option<&Path>, text: &str) -> bool {
        let mut shown = false;
        if let Some((v, rx)) = &self.pending {
            match rx.try_recv() {
                Ok(d) => {
                    // For an older text: the ones carried through the
                    // edits stay until a pass for this one.
                    if *v == version {
                        shown = true;
                        self.shown = Some((*v, Arc::new(d)));
                        self.installed += 1;
                        self.mapped = false;
                    }
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.pending = None,
            }
        }
        // A build's problems: again at once.
        let builds = crate::latex_build::recorded();
        if builds != self.builds && self.pending.is_none() {
            self.builds = builds;
            self.shown = None;
        }
        let known = !self.mapped && self.shown.as_ref().is_some_and(|(v, _)| *v == version);
        if known || self.pending.is_some() {
            return shown;
        }
        let due = match self.changed {
            _ if self.shown.is_none() => true,
            Some((v, at)) if v == version => at.elapsed() >= PAUSE,
            _ => {
                self.changed = Some((version, Instant::now()));
                false
            }
        };
        if due {
            let (tx, rx) = std::sync::mpsc::channel();
            let (path, text) = (path.map(Path::to_path_buf), text.to_string());
            std::thread::spawn(move || {
                let _ = tx.send(diagnose(path.as_deref(), &text));
            });
            self.pending = Some((version, rx));
        }
        shown
    }

    /// Carries the shown diagnostics through edit `tx`, which made
    /// `version` of the text, so that they stay in place while typing; the
    /// ones the edit touches go. A fresh pass follows after the pause.
    pub(crate) fn map(&mut self, tx: &org_edit::Transaction, version: u64) {
        use org_edit::Assoc;
        let Some((_, d)) = &self.shown else { return };
        let touched = |r: &Range<usize>| {
            tx.edits
                .iter()
                .any(|e| e.range.start <= r.end && r.start <= e.range.end)
        };
        let carry = |r: &Range<usize>| tx.map(r.start, Assoc::After)..tx.map(r.end, Assoc::Before);
        let kept: Vec<Diagnostic> = d
            .iter()
            .filter(|x| !touched(&x.range))
            .map(|x| {
                let mut y = x.clone();
                y.range = carry(&x.range);
                y.fix = x
                    .fix
                    .as_ref()
                    .filter(|(r, _)| !touched(r))
                    .map(|(r, t)| (carry(r), t.clone()));
                y
            })
            .collect();
        self.shown = Some((version, Arc::new(kept)));
        self.mapped = true;
    }

    /// Works the diagnostics out now (tests, and batch use).
    pub fn update_now(&mut self, version: u64, path: Option<&Path>, text: &str) {
        self.pending = None;
        self.mapped = false;
        self.builds = crate::latex_build::recorded();
        self.shown = Some((version, Arc::new(diagnose(path, text))));
        self.installed += 1;
    }
}

/// The diagnostics of `text`: of the file at `path` with its project, or
/// of the text alone.
fn diagnose(path: Option<&Path>, text: &str) -> Vec<Diagnostic> {
    match path {
        Some(p) => {
            let mut out = check(p, text);
            out.extend(build_diagnostics(p, text));
            out.sort_by_key(|d| (d.range.start, d.range.end));
            out
        }
        None => text_diagnostics(&latex_syntax::parse(text)),
    }
}

/// The problems the last build found in the file at `path` (T2.7h.23), on
/// their lines of `text`.
fn build_diagnostics(path: &Path, text: &str) -> Vec<Diagnostic> {
    use crate::latex_build::Severity as S;
    crate::latex_build::problems_in(path)
        .into_iter()
        .filter_map(|p| {
            let n = p.line?.checked_sub(1)?;
            let start = if n == 0 {
                0
            } else {
                text.match_indices('\n').nth(n - 1)?.0 + 1
            };
            let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
            let line = &text[start..end];
            let lead = line.len() - line.trim_start().len();
            let range = (start + lead)..end.max(start + lead);
            Some(Diagnostic {
                range,
                severity: if p.severity == S::BadBox {
                    Severity::Info
                } else {
                    Severity::Warning
                },
                code: "latex-build",
                message: crate::tr!("latex-build-problem", message = p.message.as_str()),
                fix: None,
            })
        })
        .collect()
}

/// The start of the diagnostic after `pos` (before it, when `back`),
/// round to the first (last) at the end.
pub fn next(diags: &[Diagnostic], pos: usize, back: bool) -> Option<usize> {
    let starts = diags.iter().map(|d| d.range.start);
    if back {
        starts
            .clone()
            .filter(|&s| s < pos)
            .max()
            .or_else(|| starts.max())
    } else {
        starts
            .clone()
            .filter(|&s| s > pos)
            .min()
            .or_else(|| starts.min())
    }
}

/// The diagnostic at `pos` of `diags` (in order): the narrowest holding it,
/// warnings before style.
pub fn at(diags: &[Diagnostic], pos: usize) -> Option<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.range.start <= pos && pos <= d.range.end)
        .min_by_key(|d| (d.severity != Severity::Warning, d.range.len()))
}

/// The diagnostics of the LaTeX file `path` with text `text`, in order.
pub fn check(path: &Path, text: &str) -> Vec<Diagnostic> {
    let parse = latex_syntax::parse(text);
    let mut out: Vec<Diagnostic> = Vec::new();
    // The project it belongs to, for labels and citations in other files.
    let root = crate::latex_view::find_root(path, text);
    let project = ProjectCache::default().load(&root, &Disk);
    let (model, this) = match project.model.files.iter().position(|f| same_file(f, path)) {
        Some(i) if !same_file(&root, path) || i == 0 => (project.model.clone(), i),
        _ => (std::sync::Arc::new(latex_model::Model::new(&parse)), 0),
    };
    let root_dir = root.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for l in &model.labels {
        *counts.entry(l.name.as_str()).or_insert(0) += 1;
    }
    for l in model.labels.iter().filter(|l| l.file == this) {
        if counts.get(l.name.as_str()).copied().unwrap_or(0) > 1 {
            out.push(Diagnostic {
                range: l.range.clone(),
                severity: Severity::Warning,
                code: "latex-duplicate-label",
                message: crate::tr!("latex-duplicate-label", key = l.name.as_str()),
                fix: None,
            });
        }
    }
    // What amsmath stops at, and labels LaTeX never writes.
    for (list, code, id) in [
        (
            &model.label_clashes,
            "latex-label-clash",
            "latex-label-clash",
        ),
        (
            &model.unwritten_labels,
            "latex-label-unwritten",
            "latex-label-unwritten",
        ),
        (
            &model.labels_before_caption,
            "latex-label-before-caption",
            "latex-label-before-caption",
        ),
    ] {
        for &i in list {
            if let Some(l) = model.labels.get(i).filter(|l| l.file == this) {
                out.push(Diagnostic {
                    range: l.range.clone(),
                    severity: Severity::Warning,
                    code,
                    message: crate::tr!(id, key = l.name.as_str()),
                    fix: None,
                });
            }
        }
    }
    for r in model.references.iter().filter(|r| r.file == this) {
        for k in &r.keys {
            if !counts.contains_key(k.as_str()) {
                out.push(Diagnostic {
                    range: r.range.clone(),
                    severity: Severity::Warning,
                    code: "latex-undefined-reference",
                    message: crate::tr!("latex-unknown-label", key = k.as_str()),
                    fix: None,
                });
            }
        }
    }
    // Citations of the document's own bibliography (`thebibliography`)
    // when it names no BibTeX file: a key it does not have.
    if model.bibliography.is_empty() && !model.bib_items.is_empty() {
        for c in model.citations.iter().filter(|c| c.file == this) {
            for k in c.keys.iter().filter(|k| *k != "*") {
                if !model.bib_items.iter().any(|i| &i.key == k) {
                    out.push(Diagnostic {
                        range: c.range.clone(),
                        severity: Severity::Warning,
                        code: "cite-unknown-key",
                        message: crate::tr!("latex-cite-unknown-key", key = k.as_str()),
                        fix: None,
                    });
                }
            }
        }
    }
    // Citations, when the document names its bibliography.
    let files: Vec<PathBuf> = model
        .bibliography
        .iter()
        .flat_map(|b| b.files.iter())
        .map(|f| crate::latex_view::find_bib(&root_dir, f))
        .collect();
    if !files.is_empty() {
        let (bib, errors) = org_cite::Bibliography::load(&files);
        // Malformed entries left out, the rest read, as BibTeX goes on.
        if let Some(b) = model.bibliography.iter().find(|b| b.file == this) {
            for (f, e) in bib.skipped() {
                out.push(Diagnostic {
                    range: b.range.clone(),
                    severity: Severity::Warning,
                    code: "bibliography-entry-skipped",
                    message: crate::tr!(
                        "cite-entry-skipped",
                        file = f.display().to_string(),
                        error = e.clone()
                    ),
                    fix: None,
                });
            }
        }
        for (f, e) in &errors {
            if let Some(b) = model.bibliography.iter().find(|b| b.file == this) {
                out.push(Diagnostic {
                    range: b.range.clone(),
                    severity: Severity::Warning,
                    code: "bibliography-unreadable",
                    message: crate::tr!(
                        "cite-bibliography-unreadable",
                        file = f.display().to_string(),
                        error = e.clone()
                    ),
                    fix: None,
                });
            }
        }
        // Entries nothing cites, reported with the root document (unless
        // `\nocite{*}` takes them all).
        let all = model
            .citations
            .iter()
            .any(|c| c.keys.iter().any(|k| k == "*"));
        if errors.is_empty() && this == 0 && same_file(&root, path) && !all {
            let cited: std::collections::HashSet<&str> = model
                .citations
                .iter()
                .flat_map(|c| c.keys.iter().map(String::as_str))
                .collect();
            if let Some(b) = model.bibliography.iter().find(|b| b.file == 0) {
                let unused: Vec<&str> = bib
                    .entries()
                    .iter()
                    .map(|e| e.key.as_str())
                    .filter(|k| !cited.contains(k))
                    .collect();
                // A shared library cited in part: one note, not one for
                // each of its hundreds of entries.
                let notes: Vec<String> = if unused.len() > 10 {
                    vec![crate::tr!(
                        "latex-cite-unused-entries",
                        count = unused.len(),
                        total = bib.entries().len()
                    )]
                } else {
                    unused
                        .iter()
                        .map(|k| crate::tr!("latex-cite-unused-entry", key = *k))
                        .collect()
                };
                for message in notes {
                    out.push(Diagnostic {
                        range: b.range.clone(),
                        severity: Severity::Info,
                        code: "cite-unused-entry",
                        message,
                        fix: None,
                    });
                }
            }
        }
        if errors.len() < files.len() {
            for c in model.citations.iter().filter(|c| c.file == this) {
                // `\nocite{*}` takes every entry; it names none.
                for k in c.keys.iter().filter(|k| *k != "*") {
                    if bib.get(k).is_none() && !model.bib_items.iter().any(|i| &i.key == k) {
                        out.push(Diagnostic {
                            range: c.range.clone(),
                            severity: Severity::Warning,
                            code: "cite-unknown-key",
                            message: crate::tr!("latex-cite-unknown-key", key = k.as_str()),
                            fix: None,
                        });
                    }
                }
            }
        }
    }
    for i in model
        .includes
        .iter()
        .filter(|i| i.file == this && i.resolved.is_none())
    {
        out.push(Diagnostic {
            range: i.range.clone(),
            severity: Severity::Warning,
            code: "latex-missing-file",
            message: crate::tr!("latex-missing-file", file = i.target.as_str()),
            fix: None,
        });
    }
    let root_node = parse.syntax();
    let base = path.parent();
    // `\item` outside a list, and lists more than four deep: LaTeX
    // stops at both.
    let body = model.body.clone().unwrap_or(0..text.len());
    // Four of one kind (`enumerate`, `itemize`), six in all.
    let mut open: Vec<String> = Vec::new();
    for event in root_node.preorder() {
        match event {
            latex_syntax::WalkEvent::Enter(n) => {
                let list = (n.kind() == K::ENVIRONMENT)
                    .then(|| latex_syntax::name(&n))
                    .flatten()
                    .filter(|x| crate::latex_view::is_list(x));
                let depth = open.len();
                if let Some(kind) = list {
                    open.push(kind.clone());
                    let same = open.iter().filter(|k| **k == kind).count();
                    if (kind != "description" && same == 5) || open.len() == 7 {
                        let s = usize::from(n.text_range().start());
                        out.push(Diagnostic {
                            range: s..s + text[s..].find('}').map_or(0, |i| i + 1),
                            severity: Severity::Warning,
                            code: "latex-too-deep",
                            message: crate::l10n::tr("latex-too-deep"),
                            fix: None,
                        });
                    }
                }
                if n.kind() == K::COMMAND
                    && depth == 0
                    && open.is_empty()
                    && body.contains(&usize::from(n.text_range().start()))
                    && latex_syntax::name(&n).as_deref() == Some("item")
                    && !n.ancestors().any(|a| {
                        a.kind() == K::ENVIRONMENT
                            && latex_syntax::name(&a).is_some_and(|x| {
                                matches!(
                                    x.as_str(),
                                    "thebibliography" | "itemize" | "enumerate" | "description"
                                ) || x.contains("list")
                            })
                    })
                {
                    let s = usize::from(n.text_range().start());
                    out.push(Diagnostic {
                        range: s..s + 5,
                        severity: Severity::Warning,
                        code: "latex-item-outside-list",
                        message: crate::l10n::tr("latex-item-outside-list"),
                        fix: None,
                    });
                }
            }
            latex_syntax::WalkEvent::Leave(n) => {
                if n.kind() == K::ENVIRONMENT
                    && latex_syntax::name(&n).is_some_and(|x| crate::latex_view::is_list(&x))
                {
                    open.pop();
                }
            }
        }
    }
    for n in root_node.descendants().filter(|n| n.kind() == K::COMMAND) {
        let range = usize::from(n.text_range().start())..usize::from(n.text_range().end());
        if latex_syntax::name(&n).as_deref() == Some("includegraphics")
            && let Some(g) = n.children().find(|c| c.kind() == K::GROUP)
        {
            let name = g.text().to_string();
            let name = name
                .trim_start_matches('{')
                .trim_end_matches('}')
                .trim()
                .to_string();
            // From the root document's folder, where LaTeX runs: a picture
            // found only beside a chapter in a subfolder is not found by
            // the build.
            let _ = base;
            let found =
                crate::latex_view::find_picture(Some(&root_dir), &model.graphics_paths, &name);
            if found.is_none() {
                out.push(Diagnostic {
                    range,
                    severity: Severity::Warning,
                    code: "latex-missing-picture",
                    message: crate::tr!("latex-missing-picture", file = name.as_str()),
                    fix: None,
                });
            }
        }
    }
    out.extend(text_diagnostics(&parse));
    out.sort_by_key(|d| (d.range.start, d.range.end));
    out
}

/// What the body of a LaTeX document leaves as source (T2.7h.13): the
/// commands and environments the view does not render, with how often,
/// most frequent first (`\foo`, `\begin{bar}`). Formulas are the math
/// renderer's and do not count.
pub fn unrendered(text: &str) -> Vec<(String, usize)> {
    unrendered_in(text, None)
}

/// The model the view renders `text` with: its project's, when the file
/// is known (theorems and environments declared in an `\input` preamble
/// count), else the text's own.
/// The parse of `text`, the file `file` of a project, with the
/// definitions of the project's files that change how it parses
/// ([`latex_model::project::alias_source`]), as the view parses it.
fn parse_in(text: &str, file: Option<&std::path::Path>) -> latex_syntax::Parse {
    match file {
        Some(f) => {
            let root = crate::latex_view::find_root(f, text);
            let extra = latex_model::project::alias_source(&root, &latex_model::project::Disk);
            latex_syntax::parse_with(text, &extra)
        }
        None => latex_syntax::parse(text),
    }
}

fn view_model(parse: &latex_syntax::Parse, file: Option<&std::path::Path>) -> latex_model::Model {
    match file {
        Some(f) => {
            let disk = latex_model::project::Disk;
            // The project from its root, as the view reads it: a file a
            // root document `\input`s has the root's definitions.
            let text = parse.syntax().text().to_string();
            let root = crate::latex_view::find_root(f, &text);
            let mut cache = latex_model::project::ProjectCache::default();
            cache.set_extra(parse.extra());
            let project = cache.load(&root, &disk);
            let mut m = (*project.model).clone();
            // The body is this file's.
            m.body = latex_model::Model::new(parse).body;
            m
        }
        None => latex_model::Model::new(parse),
    }
}

/// How much of a LaTeX document the rendered view covers (T2.7h.1): its
/// body's bytes, those the view leaves as source (the outermost commands
/// and environments it does not render, outside formulas), and every
/// command and environment of the body with how often it comes and
/// whether the view renders it. Formulas count as rendered: they are the
/// math renderer's.
#[derive(Debug, Clone, Default)]
pub struct Coverage {
    /// The bytes of the body (`\begin{document}` to `\end{document}`, or
    /// the whole file without them).
    pub body: usize,
    /// The bytes of the body that show as source.
    pub source: usize,
    /// The bytes of the body in formulas.
    pub math: usize,
    /// The bytes of the formulas the math renderer cannot read, which the
    /// view has TeX typeset when it is installed.
    pub tex: usize,
    /// Commands (`\foo`) and environments (`\begin{bar}`): how often, and
    /// whether rendered.
    pub names: HashMap<String, (usize, bool)>,
    /// The bytes shown as source, by the outermost command or
    /// environment shown so.
    pub source_by_name: HashMap<String, usize>,
    /// An example of each, its first one in the body.
    pub examples: HashMap<String, String>,
}

/// A piece of source as an example: one line, at most 100 characters.
pub fn example(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    one.chars().take(100).collect()
}

/// [`Coverage`] of `text`, with the project of `file` when given.
pub fn coverage_report(text: &str, file: Option<&std::path::Path>) -> Coverage {
    let parse = parse_in(text, file);
    let model = view_model(&parse, file);
    let body = model.body.clone().unwrap_or(0..text.len());
    let mut c = Coverage {
        body: body.len(),
        ..Coverage::default()
    };
    // The end of the last span counted, so that only the outermost of
    // nested spans counts.
    let mut source_to = 0;
    let mut math_to = 0;
    // The end of a rendered command whose arguments the view draws or
    // hides (a picture's options, a label's key): what is inside is not
    // text shown as source.
    let mut drawn_to = 0;
    for n in parse.syntax().descendants() {
        let r = n.text_range();
        let (start, end) = (usize::from(r.start()), usize::from(r.end()));
        if !body.contains(&start) || start < drawn_to {
            continue;
        }
        let end = end.min(body.end);
        // A simple table: the view's grid.
        if n.kind() == K::ENVIRONMENT && crate::latex_table::simple(text, &n).is_some() {
            let name = latex_syntax::name(&n).unwrap_or_default();
            let e = c
                .names
                .entry(format!("\\begin{{{name}}}"))
                .or_insert((0, true));
            e.0 += 1;
            drawn_to = end;
            continue;
        }
        let is_math_env = n.kind() == K::ENVIRONMENT
            && latex_syntax::name(&n).is_some_and(|x| latex_syntax::signatures::is_math(&x));
        if matches!(n.kind(), K::INLINE_MATH | K::DISPLAY_MATH) || is_math_env {
            if start >= math_to && start >= source_to {
                c.math += end - start;
                math_to = end;
            }
            continue;
        }
        if start < math_to {
            continue;
        }
        let (key, rendered) = match n.kind() {
            K::COMMAND => match latex_syntax::name(&n) {
                Some(name) if name.chars().all(|c| c.is_ascii_alphabetic() || c == '@') => {
                    let r = crate::latex_view::renders_command(&name)
                        || crate::latex_view::glossary_drawn(&model, &n)
                        || crate::latex_view::own_wrapper(&model, &name)
                        || {
                            // A document's own text macro, shown as its text;
                            // a note to self's argument, hidden.
                            match crate::latex_view::own_macro(&model, &name) {
                                Some(own) => {
                                    if own.args > 0 {
                                        let spec = "m".repeat(own.args);
                                        if let Some(e) =
                                            crate::latex_view::args_end(text, end, body.end, &spec)
                                        {
                                            drawn_to = drawn_to.max(e);
                                        }
                                    }
                                    true
                                }
                                None => match crate::latex_view::own_macro_use(
                                    &model, &name, text, end, body.end,
                                )
                                .map(|(_, _, e)| e)
                                .or_else(|| {
                                    // Pictures and files: drawn and named.
                                    crate::latex_view::own_block(&model, &name, text, end, body.end)
                                        .map(|(_, e)| e)
                                }) {
                                    Some(e) => {
                                        drawn_to = drawn_to.max(e);
                                        true
                                    }
                                    // A formula, drawn when the renderer
                                    // reads it (the document's definitions
                                    // aside).
                                    None => match crate::latex_view::own_math(
                                        &model, &name, text, end, body.end,
                                    ) {
                                        Some((src, e))
                                            if org_math::check(&org_math::source::prepare(
                                                &src, "",
                                            ))
                                            .is_ok() =>
                                        {
                                            drawn_to = drawn_to.max(e);
                                            true
                                        }
                                        _ => false,
                                    },
                                },
                            }
                        };
                    (format!("\\{name}"), r)
                }
                _ => continue,
            },
            K::ENVIRONMENT => match latex_syntax::name(&n) {
                Some(name) => {
                    let r = crate::latex_view::renders_environment(&name, &model);
                    // A picture TeX draws: all of it.
                    if crate::latex_view::tex_picture(&name) {
                        drawn_to = end;
                    }
                    // A rendered environment's `\\begin` line is markup, its
                    // arguments with it (`{0.48\\textwidth}`).
                    if r && let Some(b) = n.children().find(|c| c.kind() == K::BEGIN) {
                        drawn_to = drawn_to.max(usize::from(b.text_range().end()));
                    }
                    (format!("\\begin{{{name}}}"), r)
                }
                None => continue,
            },
            _ => continue,
        };
        let e = c.names.entry(key.clone()).or_insert((0, rendered));
        e.0 += 1;
        if !rendered && start >= source_to {
            // An environment the view does not know shows its `\begin`
            // and `\end` as source and its body as text, each thing in it
            // as the view shows it.
            let spans: Vec<(usize, usize)> = if n.kind() == K::ENVIRONMENT {
                n.children()
                    .filter(|x| matches!(x.kind(), K::BEGIN | K::END))
                    .map(|x| {
                        let r = x.text_range();
                        (usize::from(r.start()), usize::from(r.end()).min(body.end))
                    })
                    .collect()
            } else {
                vec![(start, end)]
            };
            for (a, b) in spans {
                if a >= source_to && b > a {
                    c.source += b - a;
                    *c.source_by_name.entry(key.clone()).or_insert(0) += b - a;
                    c.examples
                        .entry(key.clone())
                        .or_insert_with(|| example(&text[start..end]));
                    source_to = b;
                }
            }
            if n.kind() != K::ENVIRONMENT {
                source_to = end;
            }
        }
        if rendered
            && n.kind() == K::COMMAND
            && latex_syntax::name(&n).is_some_and(|x| !crate::latex_view::shows_arguments(&x))
        {
            drawn_to = end;
        }
        // A box's size and placement (`\resizebox{\textwidth}{!}{…}`): hidden,
        // its contents shown; a setting's arguments, read past the node
        // when the parser does not know them (`\let\a\b`), hidden.
        if rendered
            && n.kind() == K::COMMAND
            && let Some(spec) = latex_syntax::name(&n).and_then(|x| {
                crate::latex_view::box_args(&x).or_else(|| crate::latex_view::silent(&x))
            })
            && let Some(word) = n.first_token()
            && let Some(e) = crate::latex_view::args_end(
                text,
                usize::from(word.text_range().end()),
                body.end,
                spec,
            )
        {
            drawn_to = drawn_to.max(e);
        }
    }
    c
}

/// [`unrendered`] with the project of `file`.
pub fn unrendered_in(text: &str, file: Option<&std::path::Path>) -> Vec<(String, usize)> {
    let parse = parse_in(text, file);
    let model = view_model(&parse, file);
    let body = model.body.clone().unwrap_or(0..text.len());
    let mut counts: HashMap<String, usize> = HashMap::new();
    for n in parse.syntax().descendants() {
        let start = usize::from(n.text_range().start());
        if !body.contains(&start) {
            continue;
        }
        // In a formula, or in a picture TeX draws.
        let in_math = n.ancestors().skip(1).any(|a| {
            matches!(a.kind(), K::INLINE_MATH | K::DISPLAY_MATH)
                || (a.kind() == K::ENVIRONMENT
                    && latex_syntax::name(&a).is_some_and(|x| crate::latex_view::tex_picture(&x)))
                || (a.kind() == K::ENVIRONMENT
                    && latex_syntax::name(&a)
                        .is_some_and(|x| latex_syntax::signatures::is_math(&x)))
        });
        if in_math {
            continue;
        }
        match n.kind() {
            K::COMMAND => {
                if let Some(name) = latex_syntax::name(&n)
                    && !crate::latex_view::renders_command(&name)
                    && name.chars().all(|c| c.is_ascii_alphabetic() || c == '@')
                {
                    *counts.entry(format!("\\{name}")).or_insert(0) += 1;
                }
            }
            K::ENVIRONMENT => {
                if let Some(name) = latex_syntax::name(&n)
                    && !crate::latex_view::renders_environment(&name, &model)
                {
                    *counts.entry(format!("\\begin{{{name}}}")).or_insert(0) += 1;
                }
            }
            _ => {}
        }
    }
    let mut v: Vec<(String, usize)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

/// The share of the body's text (its characters that are not blanks)
/// the view renders rather than shows as source (T2.7h.36): the
/// commands and environments it does not render count as source, whole.
pub fn coverage(text: &str) -> f64 {
    coverage_in(text, None)
}

/// [`coverage`] with the project of `file`.
pub fn coverage_in(text: &str, file: Option<&std::path::Path>) -> f64 {
    let parse = parse_in(text, file);
    let model = view_model(&parse, file);
    let body = model.body.clone().unwrap_or(0..text.len());
    let solid = |r: std::ops::Range<usize>| text[r].chars().filter(|c| !c.is_whitespace()).count();
    let total = solid(body.clone());
    if total == 0 {
        return 1.0;
    }
    let mut source = 0;
    let mut until = 0;
    for n in parse.syntax().descendants() {
        let r = usize::from(n.text_range().start())..usize::from(n.text_range().end());
        if !body.contains(&r.start) || r.start < until {
            continue;
        }
        let in_math = n.ancestors().skip(1).any(|a| {
            matches!(a.kind(), K::INLINE_MATH | K::DISPLAY_MATH)
                || (a.kind() == K::ENVIRONMENT
                    && latex_syntax::name(&a)
                        .is_some_and(|x| latex_syntax::signatures::is_math(&x)))
        });
        let unrendered = !in_math
            && match n.kind() {
                K::COMMAND => latex_syntax::name(&n).is_some_and(|name| {
                    !crate::latex_view::renders_command(&name)
                        && name.chars().all(|c| c.is_ascii_alphabetic() || c == '@')
                }),
                K::ENVIRONMENT => latex_syntax::name(&n)
                    .is_some_and(|name| !crate::latex_view::renders_environment(&name, &model)),
                _ => false,
            };
        if unrendered {
            source += solid(r.start..r.end.min(body.end));
            until = r.end;
        }
    }
    1.0 - source as f64 / total as f64
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_file_the_root_inputs_has_its_definitions() {
        // `\model` defined in the root, used in a file it `\input`s: drawn
        // (the count read the file alone and found it undefined).
        let dir = std::env::temp_dir().join(format!("kalem-cov-root-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("main.tex"),
            "\\documentclass{article}\n\\newcommand{\\model}{xTrimo}\n\\newcommand{\\showedits}[1]{{#1}}\n\\begin{document}\n\\input{intro}\n\\end{document}\n",
        )
        .unwrap();
        let intro = "We train \\model{} on \\showedits{data}.\n";
        std::fs::write(dir.join("intro.tex"), intro).unwrap();
        let c = super::coverage_report(intro, Some(&dir.join("intro.tex")));
        assert_eq!(c.source, 0, "{:?}", c.source_by_name);
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;

    #[test]
    fn equation_macros_of_the_project_s_package() {
        // `\def\ba{\begin{eqnarray}}` in the document's own package: the
        // formula between `\ba` and `\ea` drawn, in the count and in the
        // view, and numbered.
        let dir = std::env::temp_dir().join(format!("kalem-cov-alias-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("macros.sty"),
            "\\def \\ba  {\\begin{eqnarray}}\n\\def \\ea  {\\end{eqnarray}}\n",
        )
        .unwrap();
        let main = "\\documentclass{article}\n\\usepackage{macros}\n\\begin{document}\nSee\n\\ba\nx &=& \\frac{1}{2}\\label{e}\n\\ea\nby (\\ref{e}).\n\\end{document}\n";
        let path = dir.join("main.tex");
        std::fs::write(&path, main).unwrap();
        let c = super::coverage_report(main, Some(&path));
        assert_eq!(c.source, 0, "{:?}", c.source_by_name);
        let base = crate::settings::Config::default().parse_base();
        let mut doc = crate::DocumentState::open(
            &path,
            std::sync::Arc::new(org_model::Settings::default()),
            &base,
        )
        .unwrap();
        doc.wait_for_latex_project();
        let state = doc.latex().unwrap();
        assert!(
            state
                .parse()
                .syntax()
                .descendants()
                .any(|n| n.kind() == K::DISPLAY_MATH)
        );
        assert_eq!(
            state
                .model()
                .label("e")
                .and_then(|l| l.number.clone())
                .as_deref(),
            Some("1")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lonely_items_and_deep_lists() {
        let codes = |t: &str| -> Vec<&'static str> {
            let p = std::env::temp_dir().join(format!("kalem-lists-{}.tex", std::process::id()));
            check(&p, t)
                .into_iter()
                .map(|d| d.code)
                .filter(|c| c.starts_with("latex-item") || *c == "latex-too-deep")
                .collect()
        };
        assert_eq!(
            codes("\\begin{document}\n\\item x\n\\end{document}\n"),
            ["latex-item-outside-list"]
        );
        let nest = |kinds: &[&str]| {
            let mut t = String::from("\\begin{document}\n");
            for k in kinds {
                t.push_str(&format!("\\begin{{{k}}}\\item x\n"));
            }
            for k in kinds.iter().rev() {
                t.push_str(&format!("\\end{{{k}}}\n"));
            }
            t.push_str("\\end{document}\n");
            t
        };
        assert!(codes(&nest(&["enumerate"; 4])).is_empty());
        assert_eq!(codes(&nest(&["enumerate"; 5])), ["latex-too-deep"]);
        // Mixed kinds: six in all.
        assert!(
            codes(&nest(&[
                "itemize",
                "itemize",
                "itemize",
                "enumerate",
                "enumerate",
                "enumerate"
            ]))
            .is_empty()
        );
    }

    #[test]
    fn quotes_are_shorthands_in_german() {
        let plain = latex_syntax::parse("\\begin{document}\nSay \"hi\".\n\\end{document}\n");
        assert!(
            text_diagnostics(&plain)
                .iter()
                .any(|d| d.code == "latex-quotes")
        );
        let german = latex_syntax::parse(
            "\\usepackage[ngerman]{babel}\n\\begin{document}\nStra\"se\n\\end{document}\n",
        );
        assert!(
            !text_diagnostics(&german)
                .iter()
                .any(|d| d.code == "latex-quotes")
        );
    }

    #[test]
    fn coverage_uses_the_project() {
        // A theorem declared in an `\input` preamble renders.
        let dir = std::env::temp_dir().join(format!("kalem-latex-cover-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // (A name of its own: `lemma` and the usual names render without
        // their declaration.)
        std::fs::write(dir.join("defs.tex"), "\\newtheorem{keyfact}{Key fact}\n").unwrap();
        let text = "\\documentclass{article}\n\\input{defs}\n\\begin{document}\n\\begin{keyfact}\nTrue.\n\\end{keyfact}\n\\end{document}\n";
        let path = dir.join("main.tex");
        std::fs::write(&path, text).unwrap();
        assert!(
            unrendered(text)
                .iter()
                .any(|(n, _)| n == "\\begin{keyfact}")
        );
        assert!(
            unrendered_in(text, Some(&path)).is_empty(),
            "{:?}",
            unrendered_in(text, Some(&path))
        );
        assert_eq!(coverage_in(text, Some(&path)), 1.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diagnostics() {
        crate::l10n::set_language("en");
        let dir = std::env::temp_dir().join(format!("kalem-latex-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("refs.bib"),
            "@book{knuth, title = {T}, year = 1984}\n@book{extra, title = {U}}\n",
        )
        .unwrap();
        let text = "\\documentclass{article}\n\\begin{document}\n\\section{A}\\label{a}\\label{a}\nSee \\ref{b} and\\ref{a}, {\\bf x} $$y$$ \"q\" wait... \\cite{knuth,nope}\n\\input{missing}\n\\includegraphics{nofig}\n\\begin{itemize}\n\\bibliography{refs}\n\\end{document}\n";
        let path = dir.join("p.tex");
        std::fs::write(&path, text).unwrap();
        let codes: Vec<&str> = check(&path, text).iter().map(|d| d.code).collect();
        assert_eq!(
            codes,
            [
                "latex-duplicate-label",
                "latex-duplicate-label",
                "latex-tie",
                "latex-undefined-reference",
                "latex-deprecated",
                "latex-deprecated",
                "latex-quotes",
                "latex-ellipsis",
                "latex-tie",
                "cite-unknown-key",
                "latex-missing-file",
                "latex-missing-picture",
                "latex-syntax",
                "cite-unused-entry",
            ]
        );
        // The keys as LaTeX writes them, not Org's `@key`.
        let unknown = check(&path, text)
            .into_iter()
            .find(|d| d.code == "cite-unknown-key")
            .unwrap();
        assert_eq!(unknown.message, "No bibliography has the key nope");
        // `\nocite{*}` cites every entry and names no key.
        let all = "\\documentclass{article}\n\\begin{document}\n\\nocite{*}\n\\bibliography{refs}\n\\end{document}\n";
        let all_path = dir.join("all.tex");
        std::fs::write(&all_path, all).unwrap();
        let cite: Vec<_> = check(&all_path, all)
            .into_iter()
            .filter(|d| d.code.starts_with("cite-"))
            .collect();
        assert!(cite.is_empty(), "{cite:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_shared_library_cited_in_part_is_one_note() {
        crate::l10n::set_language("en");
        let dir = std::env::temp_dir().join(format!("kalem-shared-bib-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let entries: String = (0..12)
            .map(|i| format!("@book{{k{i}, title = {{T{i}}}}}\n"))
            .collect();
        std::fs::write(dir.join("lib.bib"), entries).unwrap();
        let text = "\\documentclass{article}\n\\begin{document}\nSee~\\cite{k0}.\n\\bibliography{lib}\n\\end{document}\n";
        let path = dir.join("p.tex");
        std::fs::write(&path, text).unwrap();
        let unused: Vec<String> = check(&path, text)
            .into_iter()
            .filter(|d| d.code == "cite-unused-entry")
            .map(|d| d.message)
            .collect();
        assert_eq!(
            unused,
            ["Nothing cites 11 of the 12 entries of the bibliography"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_old_font_command_in_math_has_no_text_fix() {
        // `$\bf x$` → `$\bfseries x$` does not compile (publish_todo 3.3):
        // in math the message names `\mathbf{…}`, and nothing is replaced.
        let dir = std::env::temp_dir().join(format!("kalem-bf-math-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let text = "\\documentclass{article}\n\\begin{document}\n{\\bf a} $\\bf x$ \\[ {\\cal C} \\]\n\\end{document}\n";
        let path = dir.join("m.tex");
        std::fs::write(&path, text).unwrap();
        let found: Vec<_> = check(&path, text)
            .into_iter()
            .filter(|d| d.code == "latex-deprecated")
            .collect();
        assert_eq!(found.len(), 3, "{found:?}");
        assert_eq!(
            found[0].fix.as_ref().map(|f| f.1.as_str()),
            Some("\\bfseries")
        );
        assert!(
            found[1].fix.is_none() && found[1].message.contains("\\mathbf"),
            "{found:?}"
        );
        assert!(found[2].fix.is_none() && found[2].message.contains("\\mathcal"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_problems_on_their_lines() {
        use crate::latex_build::{Problem, Severity as S};
        let dir = std::env::temp_dir().join(format!("kalem-build-problems-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let text = "\\documentclass{article}\n\\begin{document}\n  \\foo here\n\\end{document}\n";
        let path = dir.join("main.tex");
        std::fs::write(&path, text).unwrap();
        let before = crate::latex_build::recorded();
        crate::latex_build::record(
            &path,
            &[Problem {
                file: Some("./main.tex".into()),
                line: Some(3),
                message: "Undefined control sequence.".into(),
                severity: S::Error,
            }],
        );
        assert!(crate::latex_build::recorded() > before);
        let diags = diagnose(Some(&path), text);
        let d = diags.iter().find(|d| d.code == "latex-build").unwrap();
        assert_eq!(&text[d.range.clone()], "\\foo here");
        assert!(d.message.contains("Undefined control sequence."));
        // Next and previous, round the ends.
        let starts: Vec<usize> = diags.iter().map(|d| d.range.start).collect();
        assert_eq!(next(&diags, 0, false), Some(starts[0]));
        assert_eq!(next(&diags, text.len(), false), Some(starts[0]));
        assert_eq!(next(&diags, 0, true), starts.last().copied());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn label_before_caption() {
        crate::l10n::set_language("en");
        let text = "\\documentclass{article}\n\\begin{document}\n\\begin{figure}\\label{f}\\caption{C}\\label{g}\\end{figure}\n\\end{document}\n";
        let dir = std::env::temp_dir().join(format!("kalem-latex-lbc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.tex");
        std::fs::write(&path, text).unwrap();
        let d = check(&path, text);
        let _ = std::fs::remove_dir_all(&dir);
        let found: Vec<(&str, &str)> = d.iter().map(|d| (d.code, &text[d.range.clone()])).collect();
        assert_eq!(found, [("latex-label-before-caption", "\\label{f}")]);
        assert!(d[0].message.contains("after \\caption"), "{}", d[0].message);
    }

    #[test]
    fn diagnostics_follow_edits() {
        let text = "a {\\bf x} b {\\it y}\n";
        let mut live = Live::default();
        live.update_now(0, None, text);
        let at = |live: &Live, v: u64| {
            live.current(v)
                .unwrap()
                .iter()
                .map(|d| d.range.start)
                .collect::<Vec<_>>()
        };
        assert_eq!(at(&live, 0), [3, 13]);
        // Typing before both: they move.
        let mut tx = org_edit::Transaction::new("Typing");
        tx.replace(0..0, "zz").unwrap();
        live.map(&tx, 1);
        assert_eq!(at(&live, 1), [5, 15]);
        // An edit inside the first: it goes, the second stays.
        let mut tx = org_edit::Transaction::new("Typing");
        tx.replace(7..7, "x").unwrap();
        live.map(&tx, 2);
        assert_eq!(at(&live, 2), [16]);
        // Still due for a fresh pass.
        assert!(live.mapped);
    }

    #[test]
    fn quick_fix_at_the_cursor() {
        let text = "Some {\\bf x} here...\n";
        let p = latex_syntax::parse(text);
        let fix = |pos: usize| {
            let tx = quick_fix(text, org_edit::Selection::caret(pos), &p.syntax())?;
            let mut t = text.to_string();
            for e in tx.edits.iter().rev() {
                t.replace_range(e.range.clone(), &e.insert);
            }
            Some(t)
        };
        // On `\bf`, and on the line elsewhere (the first fix on it).
        assert_eq!(fix(7).as_deref(), Some("Some {\\bfseries x} here...\n"));
        assert_eq!(fix(0).as_deref(), Some("Some {\\bfseries x} here...\n"));
        assert_eq!(fix(19).as_deref(), Some("Some {\\bf x} here\\ldots{}\n"));
    }

    #[test]
    fn fixes() {
        let text = "{\\bf x} see \\ref{a} and so... on $$y$$\n";
        let p = latex_syntax::parse(text);
        let mut fixed = text.to_string();
        let mut fixes: Vec<(Range<usize>, String)> = text_diagnostics(&p)
            .into_iter()
            .filter_map(|d| d.fix)
            .collect();
        fixes.sort_by_key(|f| std::cmp::Reverse(f.0.start));
        for (r, t) in fixes {
            fixed.replace_range(r, &t);
        }
        assert_eq!(
            fixed,
            "{\\bfseries x} see~\\ref{a} and so\\ldots{} on \\[y\\]\n"
        );
    }

    #[test]
    fn coverage_share() {
        let text = "\\begin{document}\nabcd \\foo{xy} \\emph{ok}\n\\end{document}\n";
        // `\\foo` (4 of 21 characters) stays as source; its argument is a
        // group of text.
        let c = coverage(text);
        assert!((c - (1.0 - 4.0 / 21.0)).abs() < 1e-9, "{c}");
        assert_eq!(coverage("\\begin{document}\n\\end{document}"), 1.0);
    }

    #[test]
    fn unrendered_report() {
        let text = "\\documentclass{article}\\usepackage{tikz}\n\\begin{document}\n\\foo{x} \\foo \\emph{y} $\\alpha$\n\\begin{tikzpicture}\\draw;\\end{tikzpicture}\n\\end{document}\n";
        assert_eq!(unrendered(text), [("\\foo".to_string(), 2),]);
    }
}
