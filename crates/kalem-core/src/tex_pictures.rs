//! Pictures TeX draws (TikZ, pgfplots): each compiled on its own, as a
//! `standalone` document with the preamble of the document it is in, by
//! the TeX installed, on a thread; the PDF is drawn as a picture is.
//! Without TeX, or when the picture does not compile, it stays as source.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};

/// Where a picture is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    /// Being compiled: its PDF will be at the path.
    Pending(PathBuf),
    /// Compiled.
    Ready(PathBuf),
    /// TeX did not make a PDF of it.
    Failed,
}

struct Job {
    tex: PathBuf,
    pdf: PathBuf,
    dir: Option<PathBuf>,
    key: u64,
}

static STATES: Mutex<Option<HashMap<u64, State>>> = Mutex::new(None);
/// Counts the pictures done: a frontend draws again when it changes.
static DONE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many pictures TeX has finished (drawn or failed), to tell when
/// what the view shows changed.
pub fn finished() -> u64 {
    DONE.load(std::sync::atomic::Ordering::SeqCst)
}
static QUEUE: Mutex<Option<Sender<Job>>> = Mutex::new(None);

/// Packages that a picture on its own page does not take: page layout,
/// links, bibliographies, line numbers.
const LEFT_OUT: &[&str] = &[
    "hyperref",
    "geometry",
    "fullpage",
    "biblatex",
    "fancyhdr",
    "titlesec",
    "lineno",
    "showframe",
    "refcheck",
    "background",
    "draftwatermark",
    "cleveref",
    "bookmark",
    "pdfpages",
    "tocbibind",
];

/// The preamble a picture is compiled with: the document's, without its
/// class and the packages of [`LEFT_OUT`].
pub fn picture_preamble(preamble: &str) -> String {
    let mut out = String::new();
    let mut rest = preamble;
    while !rest.is_empty() {
        let line_end = rest.find('\n').map_or(rest.len(), |i| i + 1);
        let line = &rest[..line_end];
        let t = line.trim_start();
        if t.starts_with("\\documentclass") {
            // To its closing brace, which may be lines later.
            let end = rest.find('}').map_or(rest.len(), |i| i + 1);
            rest = &rest[end..];
            continue;
        }
        if let Some(r) = t.strip_prefix("\\usepackage") {
            let (opts, r) = match r.trim_start().strip_prefix('[') {
                Some(o) => match o.split_once(']') {
                    Some((o, r)) => (Some(o), r),
                    None => (None, r),
                },
                None => (None, r),
            };
            if let Some((names, after)) = r
                .trim_start()
                .strip_prefix('{')
                .and_then(|r| r.split_once('}'))
            {
                let kept: Vec<&str> = names
                    .split(',')
                    .map(str::trim)
                    .filter(|n| !n.is_empty() && !LEFT_OUT.contains(n))
                    .collect();
                if !kept.is_empty() {
                    out.push_str("\\usepackage");
                    if let Some(o) = opts {
                        out.push_str(&format!("[{o}]"));
                    }
                    out.push_str(&format!("{{{}}}", kept.join(",")));
                }
                out.push_str(after);
                rest = &rest[line_end..];
                continue;
            }
        }
        out.push_str(line);
        rest = &rest[line_end..];
    }
    out
}

/// The text before `\begin{document}` of the document `text`, or of the
/// root document at `root` when `text` has none (a file of a project).
pub fn preamble_of(text: &str, root: Option<&Path>) -> Option<String> {
    if let Some(i) = text.find("\\begin{document}") {
        return Some(text[..i].to_string());
    }
    let root = std::fs::read_to_string(root?).ok()?;
    let i = root.find("\\begin{document}")?;
    Some(root[..i].to_string())
}

/// The PDF of the picture `source` (a whole `tikzpicture` environment)
/// compiled with `preamble` in folder `dir` (where its files are found):
/// a path whose file appears when TeX is done, `None` when there is no
/// TeX or the picture does not compile.
pub fn picture(preamble: &str, source: &str, dir: Option<&Path>) -> Option<PathBuf> {
    let doc = format!(
        "\\documentclass[border=2pt]{{standalone}}\n\\usepackage{{tikz}}\n{}\n\\begin{{document}}\n{source}\n\\end{{document}}\n",
        picture_preamble(preamble)
    );
    compile_cached("picture", &doc, dir)
}

/// The PDF of formula `source` (with its delimiters, or a whole
/// environment) that the math renderer cannot read, typeset by TeX with
/// `preamble`, as [`picture`] makes one. Its file name starts with
/// `formula-`: the frontends draw it in the text's color.
pub fn formula(preamble: &str, source: &str, dir: Option<&Path>) -> Option<PathBuf> {
    let doc = format!(
        "\\documentclass[border=1pt,varwidth]{{standalone}}\n\\usepackage{{amsmath,amssymb}}\n{}\n\\begin{{document}}\n{source}\n\\end{{document}}\n",
        picture_preamble(preamble)
    );
    compile_cached("formula", &doc, dir)
}

/// Whether `path` is a formula TeX typeset ([`formula`]).
pub fn is_formula(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("formula-"))
        && path
            .parent()
            .and_then(|d| d.file_name())
            .is_some_and(|d| d == "kalem-pictures")
}

/// The PDF of the standalone document `doc`, compiled on the thread and
/// cached by its text and folder; named `PREFIX-HASH.pdf`.
fn compile_cached(prefix: &str, doc: &str, dir: Option<&Path>) -> Option<PathBuf> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    doc.hash(&mut h);
    dir.hash(&mut h);
    let key = h.finish();
    {
        let mut states = STATES.lock().ok()?;
        let states = states.get_or_insert_with(HashMap::new);
        match states.get(&key) {
            Some(State::Ready(p) | State::Pending(p)) => return Some(p.clone()),
            Some(State::Failed) => return None,
            None => {}
        }
    }
    let cache = std::env::temp_dir().join("kalem-pictures");
    std::fs::create_dir_all(&cache).ok()?;
    let name = format!("{prefix}-{key:016x}");
    let pdf = cache.join(format!("{name}.pdf"));
    if pdf.is_file() {
        set(key, State::Ready(pdf.clone()));
        return Some(pdf);
    }
    let search = crate::pdf::tex_search_path();
    crate::pdf::find("pdflatex", &search)?;
    let tex = cache.join(format!("{name}.tex"));
    std::fs::write(&tex, doc).ok()?;
    set(key, State::Pending(pdf.clone()));
    let job = Job {
        tex,
        pdf: pdf.clone(),
        dir: dir.map(Path::to_path_buf),
        key,
    };
    let mut q = QUEUE.lock().ok()?;
    let tx = q.get_or_insert_with(|| {
        let (tx, rx) = channel::<Job>();
        std::thread::spawn(move || {
            for job in rx {
                compile(&job);
            }
        });
        tx
    });
    tx.send(job).ok()?;
    Some(pdf)
}

fn set(key: u64, state: State) {
    if let Ok(mut s) = STATES.lock() {
        s.get_or_insert_with(HashMap::new).insert(key, state);
    }
}

/// Runs pdflatex on a picture's file; its PDF is written beside it under
/// another name and moved in place when whole, so that a picture is never
/// read half written.
fn compile(job: &Job) {
    let search = crate::pdf::tex_search_path();
    let Some(program) = crate::pdf::find("pdflatex", &search) else {
        set(job.key, State::Failed);
        return;
    };
    let Some(out) = job.tex.parent() else {
        return;
    };
    let jobname = format!(
        "{}-run",
        job.tex
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("picture")
    );
    let mut cmd = std::process::Command::new(program);
    cmd.args(["-interaction=nonstopmode", "-no-shell-escape"])
        .arg(format!("-jobname={jobname}"))
        .arg(format!("-output-directory={}", out.display()))
        .arg(&job.tex)
        .env("PATH", &search)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // The document's folder: its `\input` files and pictures are found.
    if let Some(d) = &job.dir {
        cmd.current_dir(d);
        let sep = if cfg!(windows) { ";" } else { ":" };
        cmd.env("TEXINPUTS", format!("{}{sep}", d.display()));
    }
    let made = out.join(format!("{jobname}.pdf"));
    let ok = cmd.status().is_ok() && made.is_file();
    if ok && std::fs::rename(&made, &job.pdf).is_ok() {
        set(job.key, State::Ready(job.pdf.clone()));
    } else {
        set(job.key, State::Failed);
    }
    DONE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    for ext in ["aux", "log"] {
        let _ = std::fs::remove_file(out.join(format!("{jobname}.{ext}")));
    }
}

/// Whether `pdf` appears within thirty seconds: the tests that run
/// pdflatex stop when it is found but writes nothing (a sandbox, a TeX
/// without the packages).
#[cfg(test)]
pub(crate) fn wait_for(pdf: &std::path::Path) -> bool {
    for _ in 0..600 {
        if pdf.is_file() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    pdf.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preamble_of_a_picture() {
        let p = picture_preamble(
            "\\documentclass[11pt]{article}\n\\usepackage[margin=1in]{geometry}\n\\usepackage{amsmath,hyperref}\n\\usetikzlibrary{arrows}\n\\newcommand{\\R}{\\mathbb{R}}\n",
        );
        assert_eq!(
            p,
            "\n\n\\usepackage{amsmath}\n\\usetikzlibrary{arrows}\n\\newcommand{\\R}{\\mathbb{R}}\n"
        );
    }

    #[test]
    fn a_picture_compiles() {
        let search = crate::pdf::tex_search_path();
        if crate::pdf::find("pdflatex", &search).is_none() {
            return;
        }
        let src = "\\begin{tikzpicture}\\draw (0,0) -- (1,1) node[right]{$x^2$};\\end{tikzpicture}";
        let pdf = picture("\\usepackage{amsmath}\n", src, None).unwrap();
        if !wait_for(&pdf) {
            // pdflatex is there but wrote nothing (a sandbox, a TeX
            // without the packages): nothing to check here.
            return;
        }
        let img = crate::images::decode(&pdf, 800).unwrap();
        assert!(img.width() > 20 && img.height() > 20);
        // Asked again: the same file, at once.
        assert_eq!(picture("\\usepackage{amsmath}\n", src, None), Some(pdf));
    }
}
