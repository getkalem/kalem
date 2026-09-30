//! The LaTeX mode against a corpus of real documents (T2.7h.30, T2.7h.31):
//! `cargo run --release -p kalem-core --example latex_corpus -- DIR...`.
//!
//! For every `.tex` file under the folders given (or the files given):
//!
//! - the parse gives back the text byte for byte;
//! - the document opens as the editors open it (its project found), and
//!   every line is drawn away from the cursor and with the cursor on it;
//!   the blocks, the outline, the diagnostics, the coverage and completion
//!   at random places are worked out;
//! - random edits (typing, deleting, pasting pieces of the document) are
//!   reparsed incrementally and compared with a full parse, and the lines
//!   around each edit are drawn again.
//!
//! A panic, a lost byte or an incremental parse that differs is reported
//! with the file and, for edits, the seed and the step, so it can be
//! replayed. `EDITS` (default 40) sets the edits per file, `SEED` the
//! first seed, `SLOW_MS` (default 2000) when a file is reported as slow.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A small deterministic generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

/// Pieces typed at random places: the characters that change LaTeX's
/// structure most, and some text.
const PIECES: &[&str] = &[
    "\\",
    "{",
    "}",
    "$",
    "$$",
    "[",
    "]",
    "%",
    "\n",
    "\n\n",
    "&",
    "\\\\",
    "#",
    "^",
    "_",
    "~",
    "é",
    "ğ",
    "日本",
    "\u{1f600}",
    "\\begin{itemize}\n\\item a\n",
    "\\end{itemize}",
    "\\end{",
    "\\begin{",
    "\\verb|x",
    "|",
    "\\section{T}",
    "\\[",
    "\\]",
    "\\(",
    "\\)",
    "\\iffalse",
    "\\fi",
    "\\makeatletter",
    "\\makeatother",
    "\\begin{verbatim}",
    "\\end{verbatim}",
    "\\begin{tabular}{ll}\na & b \\\\\n",
    "\\end{tabular}",
    "\\label{x}",
    "\\ref{x}",
    "\\cite{k}",
    "\\footnote{",
    "\\left(",
    "\\right)",
    "\\input{a}",
    "word ",
    "\r\n",
    "\t",
    "\\begin{equation}",
    "\\end{equation}",
    "\\newcommand{\\foo}[1]{#1}",
    "\\foo{a}",
];

fn char_floor(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// A random edit of `text`.
fn random_edit(rng: &mut Rng, text: &str) -> latex_syntax::TextEdit {
    let at = char_floor(text, rng.below(text.len() + 1));
    let kind = rng.below(10);
    let (end, insert) = match kind {
        // Deleting a few characters.
        0..=2 => (char_floor(text, at + 1 + rng.below(20)), String::new()),
        // Pasting a piece of the document itself.
        3 => {
            let a = char_floor(text, rng.below(text.len() + 1));
            let b = char_floor(text, a + rng.below(200));
            (at, text[a..b].to_string())
        }
        // Replacing with a piece.
        4 => (
            char_floor(text, at + rng.below(10)),
            PIECES[rng.below(PIECES.len())].to_string(),
        ),
        // Typing a piece.
        _ => (at, PIECES[rng.below(PIECES.len())].to_string()),
    };
    latex_syntax::TextEdit {
        range: at..end.max(at),
        insert,
    }
}

#[derive(Default)]
struct Report {
    files: usize,
    bytes: usize,
    not_utf8: usize,
    failures: Vec<String>,
    slow: Vec<(Duration, String)>,
    coverage: Vec<(f64, String)>,
    unrendered: std::collections::HashMap<String, usize>,
    edits: usize,
    incremental: usize,
    /// The slowest keystroke as the editors handle it (the edit applied
    /// to the document, the lines around it drawn), with its file.
    slowest_edit: (Duration, String),
    /// The slowest opening (the document read and its lines drawn).
    slowest_open: (Duration, String),
}

fn tex_files(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        let mut v: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        v.sort();
        for e in v {
            if e.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            tex_files(&e, out);
        }
    } else if p.extension().is_some_and(|e| e == "tex" || e == "ltx") {
        out.push(p.to_path_buf());
    }
}

/// The last panic's message and place.
static PANIC: Mutex<Option<String>> = Mutex::new(None);

fn lines_of(d: &kalem_core::DocumentState) -> Vec<std::ops::Range<usize>> {
    let t = d.text();
    (0..t.line_count())
        .map(|l| {
            let mut r = t.line_range(l);
            if t.as_str()[r.clone()].ends_with('\n') {
                r.end -= 1;
            }
            if t.as_str()[r.clone()].ends_with('\r') {
                r.end -= 1;
            }
            r
        })
        .collect()
}

/// Opens `path` as the editors do and works out everything the view does.
fn view_all(path: &Path, rng: &mut Rng) -> Result<(), String> {
    let base = kalem_core::settings::Config::default().parse_base();
    let mut d =
        kalem_core::DocumentState::open(path, Arc::new(org_model::Settings::default()), &base)
            .map_err(|e| format!("open: {e:?}"))?;
    if d.latex().is_none() {
        return Ok(());
    }
    d.wait_for_latex_project();
    d.update_latex_diagnostics();
    d.poll();
    let lines = lines_of(&d);
    for r in &lines {
        if r.len() > kalem_core::view::LONG_LINE {
            continue;
        }
        std::hint::black_box(kalem_core::latex_view::line_view(&d, r.clone(), None));
        std::hint::black_box(kalem_core::latex_view::line_view(
            &d,
            r.clone(),
            Some(r.start),
        ));
    }
    std::hint::black_box(kalem_core::latex_view::blocks(&d));
    std::hint::black_box(kalem_core::latex_view::outline_items(&d));
    for r in lines
        .iter()
        .filter(|r| d.text().as_str()[(*r).clone()].contains("\\tableofcontents"))
    {
        std::hint::black_box(kalem_core::toc::latex_toc(&d, r.clone()));
    }
    let reg = kalem_core::completers::Registry::with_builtins();
    let len = d.text().len();
    for _ in 0..3 {
        let at = char_floor(d.text().as_str(), rng.below(len + 1));
        d.selection = org_edit::Selection::caret(at);
        std::hint::black_box(reg.complete(&mut d, true, Duration::from_millis(50)));
        std::hint::black_box(kalem_core::latex_view::note_at(&d, at));
        std::hint::black_box(kalem_core::latex_view::link_at(&d, at));
        std::hint::black_box(kalem_core::latex_view::formula_at(&d, at));
    }
    Ok(())
}

/// Random edits of `text`, each reparsed incrementally and compared with
/// a full parse; the document edited as the editors edit it, and the
/// lines around the edit drawn.
fn edit_all(
    path: &Path,
    text: &str,
    seed: u64,
    edits: usize,
    report: &mut Report,
) -> Result<(), String> {
    let mut rng = Rng(seed | 1);
    let mut text = text.to_string();
    let mut parse = latex_syntax::parse(&text);
    let meta = kalem_core::Metadata {
        path: Some(path.to_path_buf()),
        mode: kalem_core::DocumentMode::Latex,
        line_ending: kalem_core::LineEnding::Lf,
        bom: false,
        encoding: kalem_core::encoding_rs::UTF_8,
        lossy: false,
    };
    let mut d = kalem_core::DocumentState::new(
        text.clone(),
        meta,
        Arc::new(org_model::Settings::default()),
    );
    for step in 0..edits {
        let edit = random_edit(&mut rng, &text);
        let new_text = edit.apply(&text);
        let incremental = parse.reparse_incremental(&new_text, &edit);
        if incremental.is_some() {
            report.incremental += 1;
        }
        let full = latex_syntax::parse(&new_text);
        if let Some(inc) = &incremental
            && inc.syntax().to_string() != new_text
        {
            return Err(format!(
                "seed {seed} step {step}: incremental parse lost text"
            ));
        }
        if let Some(inc) = &incremental
            && format!("{:#?}", inc.syntax()) != format!("{:#?}", full.syntax())
        {
            return Err(format!(
                "seed {seed} step {step}: incremental parse differs from a full parse (edit {:?} at {:?})",
                edit.insert, edit.range
            ));
        }
        if full.syntax().to_string() != new_text {
            return Err(format!("seed {seed} step {step}: full parse lost text"));
        }
        // The document edited as a command edits it: timed.
        let t_edit = Instant::now();
        let mut tx = org_edit::Transaction::new("edit");
        let _ = tx.replace(edit.range.clone(), edit.insert.clone());
        d.apply(&tx, org_edit::ChangeKind::Command, Instant::now());
        if d.text().as_str() != new_text {
            return Err(format!(
                "seed {seed} step {step}: the document's text differs"
            ));
        }
        let t = d.text();
        let l = t.line_of(edit.range.start.min(new_text.len()));
        for line in l.saturating_sub(2)..(l + 3).min(t.line_count()) {
            let mut r = t.line_range(line);
            if new_text[r.clone()].ends_with('\n') {
                r.end -= 1;
            }
            if r.len() <= kalem_core::view::LONG_LINE {
                std::hint::black_box(kalem_core::latex_view::line_view(&d, r.clone(), None));
                std::hint::black_box(kalem_core::latex_view::line_view(
                    &d,
                    r.clone(),
                    Some(edit.range.start.min(new_text.len())),
                ));
            }
        }
        let took = t_edit.elapsed();
        if std::env::var("EDIT_TIMES").is_ok() {
            println!(
                "edit {step}: {:.2} ms, incremental {}, {:?} at {:?}",
                took.as_secs_f64() * 1000.0,
                incremental.is_some(),
                edit.insert.chars().take(20).collect::<String>(),
                edit.range
            );
        }
        if took > report.slowest_edit.0 {
            report.slowest_edit = (took, format!("{} step {step}", path.display()));
        }
        if step % 10 == 9 {
            std::hint::black_box(kalem_core::latex_view::blocks(&d));
            std::hint::black_box(kalem_core::latex_check::text_diagnostics(&full));
        }
        report.edits += 1;
        text = new_text;
        parse = full;
    }
    Ok(())
}

fn run_file(path: &Path, seed: u64, edits: usize, report: &mut Report) {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return,
    };
    report.files += 1;
    report.bytes += bytes.len();
    let name = path.display().to_string();
    let t0 = Instant::now();
    // Round trip of the parse, and the checks of `kalem check`.
    let text = match String::from_utf8(bytes) {
        Ok(t) => Some(t),
        Err(_) => {
            report.not_utf8 += 1;
            None
        }
    };
    let mut rng = Rng(seed.wrapping_mul(31) | 1);
    let t_open = Instant::now();
    let r = std::panic::catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
        if let Some(text) = &text {
            let p = latex_syntax::parse(text);
            if p.syntax().to_string() != *text {
                return Err("the parse does not give back the text".into());
            }
            std::hint::black_box(kalem_core::latex_check::check(path, text));
        }
        view_all(path, &mut rng)
    }));
    let opened = t_open.elapsed();
    if opened > report.slowest_open.0 {
        report.slowest_open = (opened, name.clone());
    }
    match r {
        Ok(Ok(())) => {}
        Ok(Err(e)) => report.failures.push(format!("{name}: {e}")),
        Err(_) => report.failures.push(format!(
            "{name}: panic while viewing: {}",
            PANIC
                .lock()
                .map(|p| p.clone().unwrap_or_default())
                .unwrap_or_default()
        )),
    }
    if let Some(text) = &text {
        let c = std::panic::catch_unwind(|| kalem_core::latex_check::coverage_in(text, Some(path)))
            .unwrap_or(-1.0);
        report.coverage.push((c, name.clone()));
        if let Ok(u) =
            std::panic::catch_unwind(|| kalem_core::latex_check::unrendered_in(text, Some(path)))
        {
            for (k, n) in u {
                *report.unrendered.entry(k).or_default() += n;
            }
        }
        let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let mut sub = Report::default();
            let res = edit_all(path, text, seed, edits, &mut sub);
            (res, sub.edits, sub.incremental, sub.slowest_edit)
        }));
        match r {
            Ok((res, e, i, slowest)) => {
                report.edits += e;
                report.incremental += i;
                if slowest.0 > report.slowest_edit.0 {
                    report.slowest_edit = slowest;
                }
                if let Err(e) = res {
                    report.failures.push(format!("{name}: {e}"));
                }
            }
            Err(_) => report.failures.push(format!(
                "{name}: panic while editing (seed {seed}): {}",
                PANIC
                    .lock()
                    .map(|p| p.clone().unwrap_or_default())
                    .unwrap_or_default()
            )),
        }
    }
    let took = t0.elapsed();
    let slow_ms: u64 = std::env::var("SLOW_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    if took > Duration::from_millis(slow_ms) {
        report.slow.push((took, name));
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let at = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        if let Ok(mut p) = PANIC.lock() {
            *p = Some(format!("{msg} at {at}"));
        }
    }));
    let edits: usize = std::env::var("EDITS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);
    let seed: u64 = std::env::var("SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let mut files = Vec::new();
    for a in std::env::args().skip(1) {
        tex_files(Path::new(&a), &mut files);
    }
    let mut report = Report::default();
    let start = Instant::now();
    for (i, f) in files.iter().enumerate() {
        run_file(f, seed.wrapping_add(i as u64), edits, &mut report);
    }
    println!(
        "{} files, {:.1} MB ({} not UTF-8), {} edits ({} incremental), {:.1} s",
        report.files,
        report.bytes as f64 / 1e6,
        report.not_utf8,
        report.edits,
        report.incremental,
        start.elapsed().as_secs_f64()
    );
    println!(
        "slowest keystroke {:.1} ms ({}); slowest open with every line drawn twice {:.2} s ({})",
        report.slowest_edit.0.as_secs_f64() * 1000.0,
        report.slowest_edit.1,
        report.slowest_open.0.as_secs_f64(),
        report.slowest_open.1
    );
    println!("{} failures", report.failures.len());
    for f in &report.failures {
        println!("  FAIL {f}");
    }
    report.slow.sort_by_key(|s| std::cmp::Reverse(s.0));
    for (t, f) in report.slow.iter().take(15) {
        println!("  SLOW {:.1} s {f}", t.as_secs_f64());
    }
    let valid: Vec<f64> = report
        .coverage
        .iter()
        .map(|c| c.0)
        .filter(|c| *c >= 0.0)
        .collect();
    if !valid.is_empty() {
        let mut v = valid.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        println!(
            "coverage: median {:.1}%, mean {:.1}%, lowest decile {:.1}%",
            v[v.len() / 2] * 100.0,
            v.iter().sum::<f64>() / v.len() as f64 * 100.0,
            v[v.len() / 10] * 100.0
        );
    }
    let mut u: Vec<_> = report.unrendered.iter().collect();
    u.sort_by(|a, b| b.1.cmp(a.1));
    let top: Vec<String> = u.iter().take(60).map(|(k, n)| format!("{k} {n}")).collect();
    println!("most frequent unrendered: {}", top.join(", "));
    if !report.failures.is_empty() {
        std::process::exit(1);
    }
}
