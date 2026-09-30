//! PDF through LaTeX (§9.3): the document exported with `%% org:LINE`
//! comments, compiled by `latexmk`, the TeX engine (run twice, for the
//! references) or `tectonic`, whichever is installed, and the errors of
//! LaTeX's log given at the lines of the Org file they come from.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A TeX engine (`#+LATEX_COMPILER`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// pdfLaTeX, Org's default.
    PdfLatex,
    /// XeLaTeX.
    XeLatex,
    /// LuaLaTeX.
    LuaLatex,
    /// Tectonic, which runs XeTeX and fetches the packages it needs.
    Tectonic,
}

impl Engine {
    /// The engine a `#+LATEX_COMPILER` value names; pdfLaTeX otherwise.
    pub fn from_keyword(value: Option<&str>) -> Engine {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("xelatex") => Engine::XeLatex,
            Some("lualatex") => Engine::LuaLatex,
            Some("tectonic") => Engine::Tectonic,
            _ => Engine::PdfLatex,
        }
    }

    fn program(self) -> &'static str {
        match self {
            Engine::PdfLatex => "pdflatex",
            Engine::XeLatex => "xelatex",
            Engine::LuaLatex => "lualatex",
            Engine::Tectonic => "tectonic",
        }
    }
}

/// How the PDF is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tool {
    /// `latexmk`, which runs the engine as often as needed.
    Latexmk(PathBuf),
    /// The engine itself, run twice.
    Engine(PathBuf),
    /// `tectonic`, which fetches the packages it needs.
    Tectonic(PathBuf),
}

/// `program` in the folders of `path` (the value of `PATH`).
pub fn find(program: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    let names: Vec<String> = if cfg!(windows) {
        vec![
            format!("{program}.exe"),
            format!("{program}.cmd"),
            program.to_string(),
        ]
    } else {
        vec![program.to_string()]
    };
    std::env::split_paths(path)
        .find_map(|dir| names.iter().map(|n| dir.join(n)).find(|p| p.is_file()))
}

/// The tool for `engine`, looked for in `path`: `latexmk` with the
/// engine, else the engine, else `tectonic`.
pub fn detect(engine: Engine, path: &std::ffi::OsStr) -> Option<Tool> {
    // Tectonic asked for: it alone.
    if engine == Engine::Tectonic {
        return find("tectonic", path).map(Tool::Tectonic);
    }
    let program = find(engine.program(), path);
    if let (Some(mk), Some(_)) = (find("latexmk", path), &program) {
        return Some(Tool::Latexmk(mk));
    }
    if let Some(p) = program {
        return Some(Tool::Engine(p));
    }
    find("tectonic", path).map(Tool::Tectonic)
}

/// [`detect`], with the tool `export.pdf_engine` names (`latexmk` or
/// `tectonic`) tried first; `auto` is [`detect`].
pub fn detect_preferring(engine: Engine, path: &std::ffi::OsStr, preference: &str) -> Option<Tool> {
    let preferred = match preference {
        "latexmk" if engine != Engine::Tectonic => find("latexmk", path)
            .filter(|_| find(engine.program(), path).is_some())
            .map(Tool::Latexmk),
        "tectonic" => find("tectonic", path).map(Tool::Tectonic),
        _ => None,
    };
    preferred.or_else(|| detect(engine, path))
}

/// A problem LaTeX reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The line of the `.tex` file, if the log gives one.
    pub tex_line: Option<usize>,
    /// The line of the Org file it comes from, if known.
    pub org_line: Option<usize>,
    /// What LaTeX says.
    pub message: String,
    /// An error rather than a warning.
    pub error: bool,
}

/// The problems in a LaTeX log: errors (`file.tex:12: …` with
/// `-file-line-error`, or `! …` followed by `l.12`), and the warnings
/// about undefined references and citations and overfull boxes.
pub fn parse_log(log: &str) -> Vec<Problem> {
    let lines: Vec<&str> = log.lines().collect();
    let mut out: Vec<Problem> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        // `./notes.tex:12: Undefined control sequence.`
        if let Some((file, rest)) = l.split_once(".tex:")
            && !file.contains(' ')
            && let Some((n, msg)) = rest.split_once(": ")
            && let Ok(n) = n.parse::<usize>()
        {
            out.push(Problem {
                tex_line: Some(n),
                org_line: None,
                message: msg.trim().to_string(),
                error: true,
            });
            i += 1;
            continue;
        }
        if let Some(msg) = l.strip_prefix("! ") {
            // The line follows as `l.12 …`.
            let n = lines[i + 1..]
                .iter()
                .take(12)
                .find_map(|x| x.strip_prefix("l.")?.split(' ').next()?.parse().ok());
            out.push(Problem {
                tex_line: n,
                org_line: None,
                message: msg.trim().to_string(),
                error: true,
            });
            i += 1;
            continue;
        }
        let warning = l.contains("Warning:")
            && (l.contains("undefined") || l.contains("Citation") || l.contains("Reference"));
        if warning || l.starts_with("Overfull ") || l.starts_with("Underfull ") {
            // A warning may go on over the next lines.
            let mut text = l.to_string();
            let mut j = i + 1;
            while warning && j < lines.len() && lines[j].starts_with("   ") && !text.ends_with('.')
            {
                text.push(' ');
                text.push_str(lines[j].trim());
                j += 1;
            }
            let n = line_number_in(&text);
            out.push(Problem {
                tex_line: n,
                org_line: None,
                message: text.trim().to_string(),
                error: false,
            });
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

/// `on input line 12` or `at lines 10--12` in a warning.
fn line_number_in(s: &str) -> Option<usize> {
    for key in ["input line ", "at lines ", "at line "] {
        if let Some(i) = s.find(key) {
            let rest = &s[i + key.len()..];
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(n) = digits.parse() {
                return Some(n);
            }
        }
    }
    None
}

/// The Org line a line of the `.tex` file comes from: the `%% org:N`
/// comment at or before it.
pub fn org_line(tex: &str, tex_line: usize) -> Option<usize> {
    tex.lines()
        .take(tex_line)
        .filter_map(|l| l.strip_prefix("%% org:")?.trim().parse().ok())
        .last()
}

/// What a compilation gave.
#[derive(Debug, Clone)]
pub struct Compiled {
    /// The PDF, if one was written.
    pub pdf: Option<PathBuf>,
    /// The problems, at their Org lines.
    pub problems: Vec<Problem>,
}

/// The command line that compiles `tex` (in its folder).
pub fn command(tool: &Tool, engine: Engine, tex: &Path) -> Command {
    let name = tex.file_name().map(PathBuf::from).unwrap_or_default();
    let (mut cmd, args): (Command, Vec<String>) = match tool {
        Tool::Latexmk(p) => (
            Command::new(p),
            vec![
                "-f".into(),
                "-pdf".into(),
                format!("-{}", engine.program()),
                "-interaction=nonstopmode".into(),
                "-file-line-error".into(),
            ],
        ),
        Tool::Engine(p) => (
            Command::new(p),
            vec!["-interaction=nonstopmode".into(), "-file-line-error".into()],
        ),
        Tool::Tectonic(p) => (Command::new(p), vec!["--keep-logs".into()]),
    };
    cmd.args(args).arg(name);
    if let Some(dir) = tex.parent().filter(|d| !d.as_os_str().is_empty()) {
        cmd.current_dir(dir);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd
}

/// The files LaTeX leaves beside the PDF (`org-latex-logfiles-extensions`).
const LOG_FILES: &[&str] = &[
    "aux",
    "bcf",
    "blg",
    "fdb_latexmk",
    "fls",
    "figlist",
    "idx",
    "log",
    "nav",
    "out",
    "ptc",
    "run.xml",
    "snm",
    "toc",
    "vrb",
    "xdv",
];

/// Compiles `tex` with `tool`. When the engine runs alone, it runs twice
/// (for references), and when the document has a `natbib` or `biblatex`
/// bibliography, `bibtex` or `biber` runs after the first time and the
/// engine twice more, as `latexmk` would. The log files go when there was
/// no error.
pub fn compile(tool: &Tool, engine: Engine, tex: &Path) -> Result<Compiled, String> {
    let run = || {
        command(tool, engine, tex)
            .status()
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    run()?;
    if let Tool::Engine(program) = tool {
        let again = match bibliography_tool(program, tex) {
            Some(mut bib) => {
                bib.status().map_err(|e| e.to_string())?;
                2
            }
            None => 1,
        };
        for _ in 0..again {
            run()?;
        }
    }
    let log_path = tex.with_extension("log");
    let log = std::fs::read(&log_path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let source = std::fs::read_to_string(tex).unwrap_or_default();
    let mut problems = parse_log(&log);
    for p in &mut problems {
        p.org_line = p.tex_line.and_then(|n| org_line(&source, n));
    }
    let pdf = tex.with_extension("pdf");
    let pdf = pdf.is_file().then_some(pdf);
    if pdf.is_some() && !problems.iter().any(|p| p.error) {
        for ext in LOG_FILES {
            let _ = std::fs::remove_file(tex.with_extension(ext));
        }
    }
    Ok(Compiled { pdf, problems })
}

/// `biber` when the first run left a `.bcf` file (`biblatex`), `bibtex`
/// when the `.aux` file names a bibliography (`natbib`, `\\bibliography`);
/// looked for beside the engine, then in `PATH`.
fn bibliography_tool(engine: &Path, tex: &Path) -> Option<Command> {
    let name = if tex.with_extension("bcf").is_file() {
        "biber"
    } else if std::fs::read(tex.with_extension("aux"))
        .is_ok_and(|a| String::from_utf8_lossy(&a).contains("\\bibdata{"))
    {
        "bibtex"
    } else {
        return None;
    };
    let mut search: Vec<PathBuf> = engine.parent().map(Path::to_path_buf).into_iter().collect();
    search.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let joined = std::env::join_paths(search).ok()?;
    let program = find(name, &joined)?;
    let mut cmd = Command::new(program);
    cmd.arg(tex.file_stem()?);
    if let Some(dir) = tex.parent().filter(|d| !d.as_os_str().is_empty()) {
        cmd.current_dir(dir);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    Some(cmd)
}

/// The problems as a compiler prints them: `notes.org:12: error: …`.
pub fn report(org: &Path, problems: &[Problem]) -> String {
    let name = org.display().to_string();
    problems
        .iter()
        .map(|p| {
            let kind = if p.error { "error" } else { "warning" };
            match p.org_line {
                Some(n) => format!("{name}:{n}: {kind}: {}", p.message),
                None => format!("{name}: {kind}: {}", p.message),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "This is pdfTeX, Version 3.141592653\n\
(./notes.tex\n\
./notes.tex:14: Undefined control sequence.\n\
l.14 \\foo\n\
          bar\n\
! Missing $ inserted.\n\
<inserted text> \n\
                $\n\
l.20 a_b\n\
\n\
LaTeX Warning: Reference `sec:x' on page 1 undefined on input line 23.\n\
\n\
Overfull \\hbox (12.3pt too wide) in paragraph at lines 25--27\n\
LaTeX Warning: There were undefined references.\n";

    #[test]
    fn logs() {
        let p = parse_log(LOG);
        assert_eq!(p.len(), 5, "{p:#?}");
        assert_eq!((p[0].tex_line, p[0].error), (Some(14), true));
        assert_eq!(p[0].message, "Undefined control sequence.");
        assert_eq!(
            (p[1].tex_line, p[1].message.as_str()),
            (Some(20), "Missing $ inserted.")
        );
        assert_eq!((p[2].tex_line, p[2].error), (Some(23), false));
        assert_eq!(p[3].tex_line, Some(25));
        assert_eq!(p[4].tex_line, None);
        let tex = "\\documentclass{article}\n%% org:3\nText.\n%% org:7\nMore\nand more.\n";
        assert_eq!(org_line(tex, 1), None);
        assert_eq!(org_line(tex, 3), Some(3));
        assert_eq!(org_line(tex, 6), Some(7));
    }

    #[cfg(unix)]
    #[test]
    fn compiling_with_a_fake_engine() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("kalem-pdf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        // A `pdflatex` that writes a PDF and a log with one error.
        let script = "#!/bin/sh\nfor a; do f=$a; done\nb=${f%.tex}\nprintf '%%PDF-1.4\\n' > $b.pdf\nprintf './%s.tex:3: Undefined control sequence.\\n' $b > $b.log\n";
        let engine = bin.join("pdflatex");
        std::fs::write(&engine, script).unwrap();
        std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::ffi::OsString::from(bin.as_os_str());
        let tool = detect(Engine::PdfLatex, &path).unwrap();
        assert_eq!(tool, Tool::Engine(engine.clone()));
        assert_eq!(detect(Engine::XeLatex, &path), None);
        // `export.pdf_engine`: tectonic first when installed.
        let tectonic = bin.join("tectonic");
        std::fs::write(&tectonic, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&tectonic, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            detect_preferring(Engine::PdfLatex, &path, "tectonic"),
            Some(Tool::Tectonic(tectonic.clone()))
        );
        assert_eq!(
            detect_preferring(Engine::PdfLatex, &path, "latexmk"),
            Some(Tool::Engine(engine.clone()))
        );
        std::fs::remove_file(&tectonic).unwrap();
        let tex = dir.join("doc.tex");
        std::fs::write(&tex, "\\begin{document}\n%% org:5\n\\foo\n").unwrap();
        let out = compile(&tool, Engine::PdfLatex, &tex).unwrap();
        assert_eq!(out.pdf, Some(dir.join("doc.pdf")));
        assert_eq!(out.problems.len(), 1);
        assert_eq!(out.problems[0].org_line, Some(5));
        assert_eq!(
            report(Path::new("doc.org"), &out.problems),
            "doc.org:5: error: Undefined control sequence."
        );
        // An error keeps the log.
        assert!(dir.join("doc.log").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn bibliographies_run_bibtex_or_biber() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("kalem-pdf-bib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        // An engine that counts its runs and leaves an `.aux` naming a
        // bibliography, and a `bibtex` that writes the `.bbl`.
        let engine = bin.join("pdflatex");
        std::fs::write(
            &engine,
            "#!/bin/sh\nfor a; do f=$a; done\nb=${f%.tex}\nprintf x >> $b.runs\nprintf '\\\\bibdata{refs}\\n' > $b.aux\nprintf '%%PDF-1.4\\n' > $b.pdf\n: > $b.log\n",
        )
        .unwrap();
        let bibtex = bin.join("bibtex");
        std::fs::write(&bibtex, "#!/bin/sh\n: > $1.bbl\n").unwrap();
        for p in [&engine, &bibtex] {
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let tex = dir.join("paper.tex");
        std::fs::write(&tex, "\\begin{document}\n").unwrap();
        let out = compile(&Tool::Engine(engine), Engine::PdfLatex, &tex).unwrap();
        assert_eq!(out.pdf, Some(dir.join("paper.pdf")));
        assert!(dir.join("paper.bbl").is_file());
        assert_eq!(
            std::fs::read_to_string(dir.join("paper.runs")).unwrap(),
            "xxx"
        );
    }
}
