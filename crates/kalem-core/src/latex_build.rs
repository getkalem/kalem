//! Building LaTeX documents (T2.7h.22, T2.7h.23): the root document of a
//! project compiled by `latexmk` or the engine it names (`% !TEX
//! program`, the `latex.engine` setting, or XeLaTeX for documents that
//! load `fontspec`), `biber` or `bibtex` between runs, into an output
//! folder if one is set; and the problems of the log with the file and
//! line each comes from (LaTeX's log opens a file with `(` and closes it
//! with `)`; `-file-line-error` names it on error lines).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::pdf::{self, Engine, Tool};

/// How bad a problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The build stopped or the output is wrong.
    Error,
    /// LaTeX warned (an undefined reference).
    Warning,
    /// An overfull or underfull box.
    BadBox,
}

/// A problem in a LaTeX log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The file, as the log names it (`./chapters/one.tex`).
    pub file: Option<String>,
    /// The line in it.
    pub line: Option<usize>,
    /// What LaTeX says, with the install hint for a missing package.
    pub message: String,
    /// How bad.
    pub severity: Severity,
}

/// A root document and the problems of its last build, each with the
/// file it is in.
type Recorded = (PathBuf, Vec<(PathBuf, Problem)>);

/// The problems of the last build of each root document, and a count of
/// the builds recorded.
static RECORDED: std::sync::Mutex<(u64, Vec<Recorded>)> = std::sync::Mutex::new((0, Vec::new()));

/// Keeps the problems of a build of `root`, for the editor to show in the
/// files they are in (a problem without a file is the root's).
pub fn record(root: &Path, problems: &[Problem]) {
    let dir = root.parent().map(Path::to_path_buf).unwrap_or_default();
    let canon = |p: &Path| dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let located: Vec<(PathBuf, Problem)> = problems
        .iter()
        .map(|p| {
            let file = match &p.file {
                Some(f) => canon(&dir.join(f)),
                None => canon(root),
            };
            (file, p.clone())
        })
        .collect();
    if let Ok(mut r) = RECORDED.lock() {
        let root = canon(root);
        r.1.retain(|(k, _)| *k != root);
        r.1.push((root, located));
        r.0 += 1;
    }
}

/// How many builds were recorded: the editor shows their problems again
/// when it changes.
pub fn recorded() -> u64 {
    RECORDED.lock().map_or(0, |r| r.0)
}

/// The problems the last builds found in `file`.
pub fn problems_in(file: &Path) -> Vec<Problem> {
    let file = dunce::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    RECORDED.lock().map_or(Vec::new(), |r| {
        r.1.iter()
            .flat_map(|(_, v)| v.iter())
            .filter(|(f, _)| *f == file)
            .map(|(_, p)| p.clone())
            .collect()
    })
}

/// `% !TEX program = NAME` among the first lines.
fn magic_program(text: &str) -> Option<String> {
    text.lines().take(20).find_map(|l| {
        // A byte order mark before the first line's `%` too.
        let l = l.trim_start_matches('\u{feff}').trim_start();
        let l = l.strip_prefix('%')?.trim_start();
        let l = l.strip_prefix('!').unwrap_or(l).trim_start();
        let (k, v) = l.split_once('=')?;
        let k: Vec<String> = k.split_whitespace().map(str::to_lowercase).collect();
        (k == ["tex", "program"] || k == ["tex", "ts-program"]).then(|| v.trim().to_lowercase())
    })
}

/// The engine for a root document with text `text`: `% !TEX program`,
/// else `setting` (`latex.engine`) unless `auto`, else XeLaTeX when the
/// document loads `fontspec`, `unicode-math` or `polyglossia`, else
/// pdfLaTeX.
pub fn engine(text: &str, model: &latex_model::Model, setting: &str) -> Engine {
    let named = magic_program(text)
        .or_else(|| (setting != "auto" && !setting.is_empty()).then(|| setting.to_lowercase()));
    if let Some(n) = named {
        return Engine::from_keyword(Some(&n));
    }
    let unicode = model
        .packages
        .iter()
        .any(|p| matches!(p.name.as_str(), "fontspec" | "unicode-math" | "polyglossia"));
    if unicode {
        Engine::XeLatex
    } else {
        Engine::PdfLatex
    }
}

/// The TeX distribution installed, for install hints.
fn distribution(path: &std::ffi::OsStr) -> Option<&'static str> {
    if pdf::find("tlmgr", path).is_some() {
        Some("texlive")
    } else if pdf::find("mpm", path).is_some() || pdf::find("miktex", path).is_some() {
        Some("miktex")
    } else {
        None
    }
}

/// The install hint for a missing `.sty` or `.cls` file (or another file
/// of a package); none for a picture or a document's own file.
fn install_hint(file: &str, path: &std::ffi::OsStr) -> Option<String> {
    let (package, ext) = file.rsplit_once('.')?;
    let of_a_package = matches!(
        ext,
        "sty" | "cls" | "clo" | "def" | "fd" | "cfg" | "ldf" | "bst" | "bbx" | "cbx" | "lbx"
    );
    if !of_a_package || package.contains('/') {
        return None;
    }
    Some(match distribution(path)? {
        "texlive" => crate::tr!("latex-install-texlive", package = package),
        _ => crate::tr!("latex-install-miktex", package = package),
    })
}

/// The problems of a LaTeX log, with their files.
pub fn problems(log: &str) -> Vec<Problem> {
    let search = std::env::var_os("PATH").unwrap_or_default();
    // LaTeX wraps log lines at 79 characters: pdfTeX counts bytes (a
    // Turkish letter is two), XeTeX and LuaTeX characters.
    let mut joined = String::new();
    for l in log.lines() {
        joined.push_str(l);
        if l.len() != 79 && l.chars().count() != 79 {
            joined.push('\n');
        }
    }
    let lines: Vec<&str> = joined.lines().collect();
    let mut files: Vec<String> = Vec::new();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        // `./chapters/one.tex:12: Undefined control sequence.`, or from a
        // package: `/…/babel.sty:1234: Package babel Error: …`.
        if let Some((file, n, msg)) = file_line_error(l) {
            let mut message = msg.trim().to_string();
            if let Some(f) = missing_file(&message)
                && let Some(h) = install_hint(&f, &search)
            {
                message = format!("{message} {h}");
            }
            out.push(Problem {
                file: Some(file.to_string()),
                line: Some(n),
                message,
                severity: Severity::Error,
            });
            i += 1;
            continue;
        }
        if let Some(msg) = l.strip_prefix("! ") {
            let n = lines[i + 1..]
                .iter()
                .take(12)
                .find_map(|x| x.strip_prefix("l.")?.split(' ').next()?.parse().ok());
            let mut message = msg.trim().to_string();
            if let Some(f) = missing_file(&message)
                && let Some(h) = install_hint(&f, &search)
            {
                message = format!("{message} {h}");
            }
            out.push(Problem {
                file: files.last().cloned(),
                line: n,
                message,
                severity: Severity::Error,
            });
            i += 1;
            continue;
        }
        let warning = l.contains("Warning:");
        let bad = l.starts_with("Overfull ") || l.starts_with("Underfull ");
        if warning || bad {
            let mut text = l.to_string();
            let mut j = i + 1;
            while warning && j < lines.len() && lines[j].starts_with("   ") && !text.ends_with('.')
            {
                text.push(' ');
                text.push_str(lines[j].trim());
                j += 1;
            }
            out.push(Problem {
                file: files.last().cloned(),
                line: line_number_in(&text),
                message: text.trim().to_string(),
                severity: if bad {
                    Severity::BadBox
                } else {
                    Severity::Warning
                },
            });
            track(l, &mut files);
            i = j;
            continue;
        }
        track(l, &mut files);
        i += 1;
    }
    out
}

/// `FILE:LINE: MESSAGE`, as `-file-line-error` writes errors: the file a
/// name with an extension, without blanks unless it is a path (`./my
/// paper.tex`, as TeX writes the document's own files).
fn file_line_error(l: &str) -> Option<(&str, usize, &str)> {
    let mut from = 0;
    while let Some(i) = l[from..].find(':') {
        let colon = from + i;
        let rest = &l[colon + 1..];
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0 && rest[digits..].starts_with(": ") {
            let file = &l[..colon];
            let ext = file.rsplit_once('.').map(|(_, e)| e);
            let path = file.starts_with("./") || file.starts_with("../") || file.starts_with('/');
            if (path || !file.contains(' '))
                && ext.is_some_and(|e| {
                    !e.is_empty() && e.len() <= 4 && e.chars().all(|c| c.is_ascii_alphanumeric())
                })
            {
                let n = rest[..digits].parse().ok()?;
                return Some((file, n, rest[digits + 2..].trim()));
            }
        }
        from = colon + 1;
    }
    None
}

/// The file names opened and closed on a log line.
fn track(line: &str, files: &mut Vec<String>) {
    let b = line.as_bytes();
    let mut k = 0;
    while k < b.len() {
        match b[k] {
            b'(' => {
                let rest = &line[k + 1..];
                let mut end = rest
                    .find(|c: char| c.is_whitespace() || c == ')' || c == '(')
                    .unwrap_or(rest.len());
                // `(./my paper.tex`: a path cut at a blank before its
                // extension runs on to the extension.
                if (rest.starts_with("./") || rest.starts_with('/'))
                    && !rest[..end].rsplit('/').next().unwrap_or("").contains('.')
                    && let Some(more) = path_with_blanks(rest)
                {
                    end = more;
                }
                let name = &rest[..end];
                let looks = name.starts_with("./") || name.starts_with('/') || name.contains('.');
                files.push(if looks {
                    name.to_string()
                } else {
                    String::new()
                });
                k += 1 + end;
                continue;
            }
            b')' => {
                files.pop();
            }
            _ => {}
        }
        k += 1;
    }
    // Empty entries stand for parentheses that were not files.
    while files.last().is_some_and(String::is_empty) && files.len() > 64 {
        files.pop();
    }
}

/// The length of a path with blanks at the start of `s` (`./my paper.tex`):
/// up to the end of the first `.ext` (one to four letters or digits)
/// followed by a blank, a parenthesis or the end, with no parenthesis
/// before it.
fn path_with_blanks(s: &str) -> Option<usize> {
    let stop = s.find(['(', ')']).unwrap_or(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while let Some(dot) = s[i..stop].find('.').map(|d| i + d) {
        let ext = b[dot + 1..stop]
            .iter()
            .take_while(|c| c.is_ascii_alphanumeric())
            .count();
        let after = dot + 1 + ext;
        let ends = after == s.len()
            || b.get(after)
                .is_some_and(|c| c.is_ascii_whitespace() || *c == b'(' || *c == b')');
        if (1..=4).contains(&ext) && ends && dot > 1 {
            return Some(after);
        }
        i = dot + 1;
    }
    None
}

/// `File `foo.sty' not found.`
fn missing_file(message: &str) -> Option<String> {
    let rest = message.split("File `").nth(1)?;
    let (name, after) = rest.split_once('\'')?;
    after.contains("not found").then(|| name.to_string())
}

fn line_number_in(s: &str) -> Option<usize> {
    for key in ["input line ", "at lines ", "at line "] {
        if let Some(i) = s.find(key) {
            let digits: String = s[i + key.len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if let Ok(n) = digits.parse() {
                return Some(n);
            }
        }
    }
    None
}

/// What a build gave.
#[derive(Debug, Clone)]
pub struct Built {
    /// The PDF, if one was written.
    pub pdf: Option<PathBuf>,
    /// The problems, in the order of the log.
    pub problems: Vec<Problem>,
}

/// What LaTeX and its tools write beside a document, which a repository
/// shared with co-authors or Overleaf leaves out.
pub const BUILD_OUTPUTS: &[&str] = &[
    "*.aux",
    "*.log",
    "*.out",
    "*.toc",
    "*.lof",
    "*.lot",
    "*.fls",
    "*.fdb_latexmk",
    "*.synctex.gz",
    "*.synctex(busy)",
    "*.bbl",
    "*.blg",
    "*.bcf",
    "*.run.xml",
    "*.nav",
    "*.snm",
    "*.vrb",
    "*.xdv",
    "*.dvi",
];

/// The repository holding `root` (a `.git` in its folder or one above),
/// its `.gitignore`, and the patterns of [`BUILD_OUTPUTS`], the root's
/// PDF and the output folder it lacks; `None` outside a repository or
/// when it lacks none.
pub fn ignore_missing(root: &Path, out_dir: Option<&Path>) -> Option<(PathBuf, Vec<String>)> {
    let dir = root.parent()?;
    let repo = dir.ancestors().find(|d| d.join(".git").exists())?;
    let file = repo.join(".gitignore");
    let have: std::collections::HashSet<String> = std::fs::read_to_string(&file)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .collect();
    // Paths from the repository's top, as `.gitignore` reads them.
    let rel = |p: &Path| {
        let p = p.strip_prefix(repo).unwrap_or(p);
        format!("/{}", p.to_string_lossy().replace('\\', "/"))
    };
    let mut want: Vec<String> = BUILD_OUTPUTS.iter().map(|s| s.to_string()).collect();
    want.push(rel(&root.with_extension("pdf")));
    if let Some(o) = out_dir {
        want.push(format!("{}/", rel(&dir.join(o))));
    }
    let missing: Vec<String> = want.into_iter().filter(|w| !have.contains(w)).collect();
    (!missing.is_empty()).then_some((file, missing))
}

/// Adds what [`ignore_missing`] finds to the repository's `.gitignore`
/// (made if there is none): how many patterns, and the file.
pub fn ignore_build_outputs(
    root: &Path,
    out_dir: Option<&Path>,
) -> Result<(usize, PathBuf), String> {
    let Some((file, missing)) = ignore_missing(root, out_dir) else {
        return Ok((0, root.to_path_buf()));
    };
    let mut text = std::fs::read_to_string(&file).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str("# LaTeX build outputs\n");
    for m in &missing {
        text.push_str(m);
        text.push('\n');
    }
    std::fs::write(&file, text).map_err(|e| e.to_string())?;
    Ok((missing.len(), file))
}

/// The command that builds `root` with `tool`, the output in `out_dir`;
/// what it prints goes to `output`.
fn build_command(
    tool: &Tool,
    engine: Engine,
    root: &Path,
    out_dir: Option<&Path>,
    output: Option<&std::fs::File>,
    search: &std::ffi::OsStr,
) -> Command {
    let name = root.file_name().map(PathBuf::from).unwrap_or_default();
    let mut cmd = match tool {
        Tool::Latexmk(p) => {
            let mut c = Command::new(p);
            c.args([
                "-pdf",
                "-interaction=nonstopmode",
                "-file-line-error",
                "-halt-on-error",
                // Where each line is typeset, for Show in PDF and back.
                "-synctex=1",
            ]);
            c.arg(match engine {
                Engine::PdfLatex => "-pdflatex",
                Engine::XeLatex => "-xelatex",
                Engine::LuaLatex => "-lualatex",
                // Not reached: tectonic is run on its own.
                Engine::Tectonic => "-xelatex",
            });
            if let Some(d) = out_dir {
                c.arg(format!("-outdir={}", d.display()));
            }
            c
        }
        Tool::Engine(p) => {
            let mut c = Command::new(p);
            c.args(["-interaction=nonstopmode", "-file-line-error", "-synctex=1"]);
            if let Some(d) = out_dir {
                c.arg(format!("-output-directory={}", d.display()));
            }
            c
        }
        Tool::Tectonic(p) => {
            let mut c = Command::new(p);
            c.args(["--keep-logs", "--synctex"]);
            if let Some(d) = out_dir {
                c.arg(format!("--outdir={}", d.display()));
            }
            c
        }
    };
    cmd.arg(name);
    if let Some(dir) = root.parent().filter(|d| !d.as_os_str().is_empty()) {
        cmd.current_dir(dir);
    }
    let to = |f: Option<&std::fs::File>| {
        f.and_then(|f| f.try_clone().ok())
            .map_or_else(std::process::Stdio::null, std::process::Stdio::from)
    };
    // The TeX folders found beyond `PATH`, for the programs latexmk runs.
    cmd.env("PATH", search);
    cmd.stdin(std::process::Stdio::null())
        .stdout(to(output))
        .stderr(to(output));
    cmd
}

/// The last lines a build tool printed that say something (its errors
/// when it stopped before writing a log).
fn tail(output: &str) -> String {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("Latexmk: ") || l.contains("rror"))
        .collect();
    lines[lines.len().saturating_sub(3)..].join(" ")
}

/// The program a tool runs, for messages.
fn tool_name(tool: &Tool) -> String {
    let (Tool::Latexmk(p) | Tool::Engine(p) | Tool::Tectonic(p)) = tool;
    p.file_stem().map_or_else(
        || p.display().to_string(),
        |s| s.to_string_lossy().into_owned(),
    )
}

/// The flag of the build that runs, which [`cancel`] raises.
static CURRENT: std::sync::Mutex<Option<Arc<AtomicBool>>> = std::sync::Mutex::new(None);

/// A build asked for while one runs: the root, engine and output folder
/// of the last one asked, built when the running one ends (one build at
/// a time, on the same `.aux` and `.pdf`).
pub type Queued = (PathBuf, Engine, Option<PathBuf>);

static AGAIN: std::sync::Mutex<Option<Queued>> = std::sync::Mutex::new(None);

/// Whether a build runs.
pub fn running() -> bool {
    CURRENT.lock().is_ok_and(|c| c.is_some())
}

/// Asks for `build` once the running one ends; the last asked wins.
pub fn queue(build: Queued) {
    if let Ok(mut a) = AGAIN.lock() {
        *a = Some(build);
    }
}

/// The build asked for while one ran, if any, taken.
pub fn take_queued() -> Option<Queued> {
    AGAIN.lock().ok().and_then(|mut a| a.take())
}

/// Stops the build that runs, if one does (Cancel Build), and forgets the
/// one asked for after it; `false` when none runs.
pub fn cancel() -> bool {
    let _ = take_queued();
    match CURRENT.lock().ok().and_then(|c| c.clone()) {
        Some(flag) => {
            flag.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

/// Runs `cmd` to its end, or kills it when `cancelled` is raised; whether
/// it succeeded.
fn run_to_end(cmd: &mut Command, cancelled: &AtomicBool) -> Result<bool, String> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let mut child = cmd.spawn().map_err(|e| format!("{program}: {e}"))?;
    loop {
        if cancelled.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(crate::l10n::tr("msg-build-cancelled"));
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.success()),
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Builds the root document `root` with `engine`, the output in
/// `out_dir` (relative to the root's folder) when given. Without
/// `latexmk` the engine runs, then `biber` or `bibtex` if the document
/// has a bibliography, then the engine twice more. [`cancel`] stops it.
pub fn build(root: &Path, engine: Engine, out_dir: Option<&Path>) -> Result<Built, String> {
    let flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut c) = CURRENT.lock() {
        *c = Some(flag.clone());
    }
    let out = build_inner(root, engine, out_dir, &flag, &pdf::tex_search_path());
    if let Ok(mut c) = CURRENT.lock()
        && c.as_ref().is_some_and(|f| Arc::ptr_eq(f, &flag))
    {
        *c = None;
    }
    out
}

fn build_inner(
    root: &Path,
    engine: Engine,
    out_dir: Option<&Path>,
    cancelled: &AtomicBool,
    search: &std::ffi::OsStr,
) -> Result<Built, String> {
    let search = search.to_os_string();
    let mut tool = pdf::detect(engine, &search).ok_or_else(|| pdf::missing(engine, &search))?;
    let dir = root.parent().map(Path::to_path_buf).unwrap_or_default();
    if let Some(d) = out_dir {
        std::fs::create_dir_all(dir.join(d)).map_err(|e| e.to_string())?;
    }
    let out = out_dir.map_or(dir.clone(), |d| dir.join(d));
    let stem = root.file_stem().map(PathBuf::from).unwrap_or_default();
    let log_path = out.join(&stem).with_extension("log");
    let pdf_path = out.join(&stem).with_extension("pdf");
    // What the tools print, kept for when they stop without a log.
    static BUILDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = BUILDS.fetch_add(1, Ordering::SeqCst);
    let printed = std::env::temp_dir().join(format!("kalem-build-{}-{n}.out", std::process::id()));
    let output = std::fs::File::create(&printed).ok();
    let started = std::time::SystemTime::now();
    let fresh = |p: &Path| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .is_ok_and(|t| t >= started - std::time::Duration::from_secs(2))
    };
    let run = |tool: &Tool| {
        run_to_end(
            &mut build_command(tool, engine, root, out_dir, output.as_ref(), &search),
            cancelled,
        )
    };
    // `\include{chapters/intro}` writes `chapters/intro.aux` under the
    // output folder, which TeX does not make.
    if let Some(d) = out_dir {
        let text = std::fs::read(root)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        for part in text.split("\\include{").skip(1) {
            let Some((name, _)) = part.split_once('}') else {
                continue;
            };
            if let Some(sub) = Path::new(name.trim())
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
            {
                let _ = std::fs::create_dir_all(dir.join(d).join(sub));
            }
        }
    }
    let ok = run(&tool)?;
    // latexmk with nothing to do (the PDF up to date) leaves the log of
    // the build that made it: that build's problems and PDF.
    if ok && matches!(tool, Tool::Latexmk(_)) && !fresh(&log_path) && log_path.is_file() {
        let _ = std::fs::remove_file(&printed);
        let log = std::fs::read(&log_path)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        return Ok(Built {
            pdf: pdf_path.is_file().then_some(pdf_path),
            problems: problems(&log),
        });
    }
    // latexmk that could not run (no Perl, as MiKTeX's needs) writes no
    // log: the engine instead.
    if !ok
        && !fresh(&log_path)
        && matches!(tool, Tool::Latexmk(_))
        && let Some(p) = pdf::find(engine.program(), &search)
    {
        tool = Tool::Engine(p);
        run(&tool)?;
    }
    let mut missing_tool = None;
    let mut bib_problems: Vec<Problem> = Vec::new();
    if let Tool::Engine(program) = &tool {
        let aux = out.join(&stem).with_extension("aux");
        let bcf = out.join(&stem).with_extension("bcf");
        // biblatex's `.bcf` of this run (one left by an older build of a
        // document that has moved to BibTeX is not), else `\bibdata` in
        // the `.aux` or one it inputs (a bibliography in an `\include`d
        // chapter).
        let bib = if bcf.is_file() && fresh(&bcf) {
            Some("biber")
        } else if aux_has_bibdata(&out, &aux) {
            Some("bibtex")
        } else {
            None
        };
        let mut again = 1;
        if let Some(b) = bib {
            let mut paths: Vec<PathBuf> = program
                .parent()
                .map(Path::to_path_buf)
                .into_iter()
                .collect();
            paths.extend(std::env::split_paths(&search));
            if let Some(p) = std::env::join_paths(paths)
                .ok()
                .and_then(|j| pdf::find(b, &j))
            {
                // Run in the output folder, where the `.aux` is, the
                // bibliographies found beside the document as well.
                let mut c = Command::new(p);
                if b == "biber" {
                    c.arg("--input-directory").arg(&dir);
                } else {
                    let sep = if cfg!(windows) { ";" } else { ":" };
                    // A trailing separator keeps TeX's own places too.
                    c.env("BIBINPUTS", format!("{}{sep}", dir.display()));
                }
                run_to_end(
                    c.arg(&stem)
                        .current_dir(&out)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null()),
                    cancelled,
                )?;
                // BibTeX's errors (no style, a database not found, an
                // entry it could not read), from its `.blg`.
                if b == "bibtex" {
                    let blg = out.join(&stem).with_extension("blg");
                    if fresh(&blg) {
                        bib_problems = std::fs::read(&blg)
                            .map(|t| bibtex_problems(&String::from_utf8_lossy(&t)))
                            .unwrap_or_default();
                    }
                }
                again = 2;
            } else {
                missing_tool = Some(b);
            }
        }
        for _ in 0..again {
            run(&tool)?;
        }
        // Again while LaTeX asks for it (references that moved), as
        // latexmk does, up to five runs in all.
        let mut runs = 1 + again;
        while runs < 5
            && std::fs::read(&log_path).is_ok_and(|l| asks_rerun(&String::from_utf8_lossy(&l)))
        {
            run(&tool)?;
            runs += 1;
        }
    }
    let printed_text = std::fs::read(&printed)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let _ = std::fs::remove_file(&printed);
    // A log from an earlier build is not this one's.
    if !fresh(&log_path) {
        let said = tail(&printed_text);
        return Err(if said.is_empty() {
            crate::tr!("msg-build-no-log", program = tool_name(&tool))
        } else {
            format!("{}: {said}", tool_name(&tool))
        });
    }
    let log = std::fs::read(&log_path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let pdf = pdf_path;
    let mut problems = problems(&log);
    // The bibliography's tool not installed: said, not left as undefined
    // citations.
    if let Some(b) = missing_tool {
        problems.push(Problem {
            file: None,
            line: None,
            message: crate::tr!("latex-no-bib-tool", program = b),
            severity: Severity::Warning,
        });
    }
    problems.extend(bib_problems);
    Ok(Built {
        pdf: (pdf.is_file() && fresh(&pdf)).then_some(pdf),
        problems,
    })
}

/// The problems as a compiler prints them: `file:line: error: …`.
pub fn report(root: &Path, problems: &[Problem]) -> String {
    problems
        .iter()
        .map(|p| {
            let kind = match p.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::BadBox => "info",
            };
            let file = p.file.clone().unwrap_or_else(|| root.display().to_string());
            match p.line {
                Some(n) => format!("{file}:{n}: {kind}: {}", p.message),
                None => format!("{file}: {kind}: {}", p.message),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether the `.aux` at `aux`, or one it inputs (`\@input{chapter.aux}`
/// of an `\include`, in the output folder `out`), names a bibliography.
fn aux_has_bibdata(out: &Path, aux: &Path) -> bool {
    let mut pending = vec![aux.to_path_buf()];
    let mut seen = std::collections::HashSet::new();
    while let Some(a) = pending.pop() {
        if !seen.insert(a.clone()) || seen.len() > 1000 {
            continue;
        }
        let Ok(bytes) = std::fs::read(&a) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        if text.contains("\\bibdata{") {
            return true;
        }
        for line in text.lines() {
            if let Some(name) = line
                .trim()
                .strip_prefix("\\@input{")
                .and_then(|r| r.strip_suffix('}'))
            {
                pending.push(out.join(name));
            }
        }
    }
    false
}

/// Whether a LaTeX log asks for another run: references, citations or
/// labels that changed.
fn asks_rerun(log: &str) -> bool {
    log.contains("Rerun to get")
        || log.contains("Label(s) may have changed. Rerun")
        || log.contains("Rerun LaTeX")
        || log.contains("Please rerun LaTeX")
}

/// BibTeX's errors in its `.blg`: each message with the line of the
/// file it names (`---line 5 of file refs.bib`).
fn bibtex_problems(blg: &str) -> Vec<Problem> {
    let mut out = Vec::new();
    let lines: Vec<&str> = blg.lines().collect();
    for (i, l) in lines.iter().enumerate() {
        let l = l.trim_end();
        let (message, place) = match l.split_once("---") {
            Some((m, p)) if !m.trim().is_empty() => (m.trim().to_string(), Some(p)),
            // The place on the next line (`I couldn't open database file
            // x.bib` then `---line 3 of file doc.aux`).
            _ if l.starts_with("I couldn't open") || l.starts_with("I found no") => (
                l.to_string(),
                lines.get(i + 1).and_then(|n| n.strip_prefix("---")),
            ),
            _ => continue,
        };
        if message.starts_with("Warning") {
            continue;
        }
        let (mut file, mut line) = (None, None);
        if let Some(p) = place
            && let Some(rest) = p.strip_prefix("line ")
            && let Some((n, f)) = rest.split_once(" of file ")
        {
            line = n.trim().parse().ok();
            file = Some(f.trim().to_string()).filter(|f| !f.ends_with(".aux"));
            if file.is_none() {
                line = None;
            }
        }
        out.push(Problem {
            file,
            line,
            message: format!("BibTeX: {message}"),
            severity: Severity::Error,
        });
    }
    out
}

#[cfg(test)]
mod tests {

    /// A document whose name has a blank: its errors keep their file and
    /// line, and the file stack names it whole.
    #[test]
    fn names_with_blanks() {
        assert_eq!(
            file_line_error("./my paper.tex:4: Undefined control sequence."),
            Some(("./my paper.tex", 4, "Undefined control sequence."))
        );
        // Not a path: a blank still ends the name, as in a message.
        assert_eq!(file_line_error("See the paper.tex:4: x"), None);
        let mut files = Vec::new();
        track("(./my paper.tex [1]", &mut files);
        assert_eq!(files, ["./my paper.tex"]);
    }

    /// BibTeX's errors from its `.blg`; its warnings are LaTeX's too.
    #[test]
    fn bibtex_errors() {
        let blg = "This is BibTeX, Version 0.99d\nI found no \\bibstyle command---while reading file p.aux\nI couldn't open database file missing.bib\n---line 3 of file p.aux\n : \\bibdata{missing\nI was expecting a `,' or a `}'---line 5 of file refs.bib\nWarning--I didn't find a database entry for \"x\"\n(There were 3 error messages)\n";
        let p = bibtex_problems(blg);
        let got: Vec<(Option<&str>, Option<usize>, &str)> = p
            .iter()
            .map(|p| (p.file.as_deref(), p.line, p.message.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (None, None, "BibTeX: I found no \\bibstyle command"),
                (
                    None,
                    None,
                    "BibTeX: I couldn't open database file missing.bib"
                ),
                (
                    Some("refs.bib"),
                    Some(5),
                    "BibTeX: I was expecting a `,' or a `}'"
                ),
            ]
        );
        assert!(asks_rerun(
            "LaTeX Warning: Label(s) may have changed. Rerun to get cross-references right."
        ));
        assert!(!asks_rerun("Output written on p.pdf (1 page)."));
    }

    /// A bibliography in an `\include`d chapter: its `.aux` names it.
    #[test]
    fn bibdata_in_an_included_aux() {
        let dir = std::env::temp_dir().join(format!("kalem-aux-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("chapters")).unwrap();
        std::fs::write(
            dir.join("main.aux"),
            "\\relax\n\\@input{chapters/biblio.aux}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("chapters/biblio.aux"),
            "\\bibstyle{plain}\n\\bibdata{refs}\n",
        )
        .unwrap();
        assert!(aux_has_bibdata(&dir, &dir.join("main.aux")));
        std::fs::write(dir.join("chapters/biblio.aux"), "\\relax\n").unwrap();
        assert!(!aux_has_bibdata(&dir, &dir.join("main.aux")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Install hints only for a package's files.
    #[test]
    fn install_hints_for_packages_only() {
        let path = std::ffi::OsString::new();
        assert_eq!(install_hint("example-image", &path), None);
        assert_eq!(install_hint("../ch/pic.png", &path), None);
        assert_eq!(install_hint("two", &path), None);
    }

    /// pdfTeX wraps a log line at 79 bytes: a warning with Turkish letters
    /// keeps its line number.
    #[test]
    fn wrapped_lines_by_bytes() {
        let first = "LaTeX Warning: Reference `şekil:ölçüm-sonuçları' on page 1 undefined on i";
        assert_eq!(first.len(), 79);
        let log = format!("{first}\nnput line 12.\n");
        let p = problems(&log);
        assert!(p.iter().any(|p| p.line == Some(12)), "{p:?}");
    }
    #[test]
    fn build_outputs_kept_out_of_git() {
        let dir = std::env::temp_dir().join(format!("kalem-ignore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("paper")).unwrap();
        let root = dir.join("paper/main.tex");
        std::fs::write(&root, "\\documentclass{article}\n").unwrap();
        // Outside a repository: nothing to offer.
        assert!(ignore_missing(&root, None).is_none());
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(".gitignore"), "*.log\nnode_modules/").unwrap();
        let (file, missing) = ignore_missing(&root, Some(Path::new("build"))).unwrap();
        assert_eq!(file, dir.join(".gitignore"));
        assert!(!missing.contains(&"*.log".to_string()));
        assert!(missing.contains(&"/paper/main.pdf".to_string()));
        assert!(missing.contains(&"/paper/build/".to_string()));
        let (n, _) = ignore_build_outputs(&root, Some(Path::new("build"))).unwrap();
        assert_eq!(n, missing.len());
        let text = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert!(
            text.starts_with("*.log\nnode_modules/\n\n# LaTeX build outputs\n*.aux\n"),
            "{text}"
        );
        // Done once: nothing left to add.
        assert!(ignore_missing(&root, Some(Path::new("build"))).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    use super::*;

    #[test]
    fn a_build_asked_for_during_one_waits_for_it() {
        let flag = Arc::new(AtomicBool::new(false));
        *CURRENT.lock().unwrap() = Some(flag.clone());
        assert!(running());
        let a = (PathBuf::from("a.tex"), Engine::PdfLatex, None);
        let b = (PathBuf::from("b.tex"), Engine::XeLatex, None);
        queue(a);
        queue(b.clone());
        // The last asked is the one built next, once.
        assert_eq!(take_queued(), Some(b.clone()));
        assert_eq!(take_queued(), None);
        // Cancel stops the build and forgets the one asked for.
        queue(b);
        assert!(cancel());
        assert!(flag.load(Ordering::SeqCst));
        assert_eq!(take_queued(), None);
        *CURRENT.lock().unwrap() = None;
        assert!(!running());
    }

    #[cfg(unix)]
    #[test]
    fn cancelling_stops_the_program() {
        let flag = Arc::new(AtomicBool::new(false));
        let raise = flag.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            raise.store(true, Ordering::SeqCst);
        });
        let started = std::time::Instant::now();
        let r = run_to_end(Command::new("sleep").arg("10"), &flag);
        t.join().unwrap();
        assert_eq!(r, Err(crate::l10n::tr("msg-build-cancelled")));
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn engines() {
        let model = |t: &str| latex_model::Model::new(&latex_syntax::parse(t));
        let t = "% !TEX program = lualatex\n\\documentclass{article}";
        assert_eq!(engine(t, &model(t), "auto"), Engine::LuaLatex);
        // After a byte order mark, as a file read from the disk has it.
        let t = "\u{feff}% !TEX program = xelatex\n\\documentclass{article}";
        assert_eq!(engine(t, &model(t), "auto"), Engine::XeLatex);
        let t = "\\usepackage{fontspec}";
        assert_eq!(engine(t, &model(t), "auto"), Engine::XeLatex);
        assert_eq!(engine(t, &model(t), "pdflatex"), Engine::PdfLatex);
        assert_eq!(engine("x", &model("x"), "auto"), Engine::PdfLatex);
    }

    #[test]
    fn log_problems_with_files() {
        let log = "This is pdfTeX\n(./main.tex (/usr/share/texlive/article.cls\n) (./chapters/one.tex\n\nLaTeX Warning: Reference `x' on page 1 undefined on input line 3.\n\n) [1]\nOverfull \\hbox (12.0pt too wide) in paragraph at lines 10--12\n./chapters/two.tex:7: Undefined control sequence.\nl.7 \\foo\n\n! LaTeX Error: File `nosuch.sty' not found.\n";
        let p = problems(log);
        assert_eq!(p.len(), 4, "{p:#?}");
        assert_eq!(
            (p[0].file.as_deref(), p[0].line, p[0].severity),
            (Some("./chapters/one.tex"), Some(3), Severity::Warning)
        );
        assert_eq!(
            (p[1].file.as_deref(), p[1].line, p[1].severity),
            (Some("./main.tex"), Some(10), Severity::BadBox)
        );
        assert_eq!(
            (p[2].file.as_deref(), p[2].line),
            (Some("./chapters/two.tex"), Some(7))
        );
        assert!(
            p[3].message
                .starts_with("LaTeX Error: File `nosuch.sty' not found.")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_tool_that_stops_without_a_log() {
        use std::os::unix::fs::PermissionsExt;
        let Some(real) = pdf::find("pdflatex", &std::env::var_os("PATH").unwrap_or_default())
        else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("kalem-latex-nolog-{}", std::process::id()));
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let script = |name: &str, body: &str| {
            let p = bin.join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        std::fs::write(
            dir.join("main.tex"),
            "\\documentclass{article}\\begin{document}Hi\\end{document}\n",
        )
        .unwrap();
        // latexmk that cannot run (MiKTeX's without Perl): the engine
        // makes the PDF instead.
        script("latexmk", "echo 'perl: not found' >&2; exit 1");
        script("pdflatex", &format!("exec {} \"$@\"", real.display()));
        let flag = AtomicBool::new(false);
        let built = build_inner(
            &dir.join("main.tex"),
            Engine::PdfLatex,
            None,
            &flag,
            bin.as_os_str(),
        )
        .unwrap();
        assert!(built.pdf.is_some(), "{built:?}");
        // An engine that stops before a log: what it said, not "no PDF".
        let _ = std::fs::remove_file(dir.join("main.log"));
        let _ = std::fs::remove_file(dir.join("main.pdf"));
        std::fs::remove_file(bin.join("latexmk")).unwrap();
        script(
            "pdflatex",
            "echo 'fatal: cannot find the format file pdflatex.fmt' >&2; exit 1",
        );
        let err = build_inner(
            &dir.join("main.tex"),
            Engine::PdfLatex,
            None,
            &flag,
            bin.as_os_str(),
        )
        .unwrap_err();
        assert!(err.contains("pdflatex.fmt"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn builds_a_project() {
        let search = std::env::var_os("PATH").unwrap_or_default();
        if pdf::detect(Engine::PdfLatex, &search).is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("kalem-latex-build-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("ch")).unwrap();
        std::fs::write(
            dir.join("main.tex"),
            "\\documentclass{article}\\begin{document}\\input{ch/one}\\end{document}\n",
        )
        .unwrap();
        std::fs::write(dir.join("ch/one.tex"), "Hello \\ref{nope}.\n").unwrap();
        let built = build(
            &dir.join("main.tex"),
            Engine::PdfLatex,
            Some(Path::new("build")),
        )
        .unwrap();
        assert!(
            built
                .pdf
                .as_ref()
                .is_some_and(|p| p.ends_with("build/main.pdf")),
            "{built:?}"
        );
        assert!(
            built
                .problems
                .iter()
                .any(|p| p.message.contains("undefined")
                    && p.file.as_deref() == Some("./ch/one.tex")),
            "{:#?}",
            built.problems
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
