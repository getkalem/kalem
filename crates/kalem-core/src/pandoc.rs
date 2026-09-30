//! The bridge to pandoc (§10): Word, OpenDocument, EPUB and RTF written
//! from the document, and Word, OpenDocument, Markdown, HTML, EPUB and RTF
//! read into Org, with a clean-up pass over what pandoc writes (the
//! anchors and properties it adds that nothing refers to).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A format pandoc writes for Kalem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Word (`.docx`).
    Docx,
    /// OpenDocument text (`.odt`).
    Odt,
    /// EPUB 3.
    Epub,
    /// Rich Text Format.
    Rtf,
}

impl Format {
    /// Every format, for menus.
    pub const ALL: [Format; 4] = [Format::Docx, Format::Odt, Format::Epub, Format::Rtf];

    /// The format a name or extension gives (`docx`, `odt`, `epub`, `rtf`).
    pub fn from_name(name: &str) -> Option<Format> {
        match name.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "docx" | "word" => Some(Format::Docx),
            "odt" => Some(Format::Odt),
            "epub" => Some(Format::Epub),
            "rtf" => Some(Format::Rtf),
            _ => None,
        }
    }

    /// pandoc's name for it.
    pub fn pandoc_name(self) -> &'static str {
        match self {
            Format::Docx => "docx",
            Format::Odt => "odt",
            Format::Epub => "epub3",
            Format::Rtf => "rtf",
        }
    }

    /// The file extension, with its dot.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Docx => ".docx",
            Format::Odt => ".odt",
            Format::Epub => ".epub",
            Format::Rtf => ".rtf",
        }
    }
}

/// `pandoc` in the folders of `path` (the value of `PATH`).
pub fn find(path: &std::ffi::OsStr) -> Option<PathBuf> {
    crate::pdf::find("pandoc", path)
}

/// pandoc as `export.pandoc_path` gives it (a path, or a program name
/// looked for in `path`), else [`find`].
pub fn find_with(setting: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    let setting = setting.trim();
    if setting.is_empty() {
        return find(path);
    }
    let p = PathBuf::from(setting);
    if p.components().count() > 1 {
        return p.is_file().then_some(p);
    }
    crate::pdf::find(setting, path)
}

/// The document as pandoc should read it: `#+INCLUDE` and macros expanded
/// as Org's exporter expands them, and Kalem's formatting taken out, which
/// pandoc's Org reader does not know.
pub fn prepare(text: &str, file: Option<&Path>) -> Result<String, String> {
    let text = org_export::include::expand(text, file)?;
    let now = jiff::Zoned::now();
    let text = org_export::macros::expand(&text, &["TITLE", "DATE", "AUTHOR"], file, &now)?;
    Ok(for_pandoc(&crate::kinds::strip_markup(&text).0))
}

/// What pandoc's Org reader does not know, written as it knows it: math
/// environments as displayed formulas (`\[…\]`), and links without a
/// description to a named figure, table, equation or listing, a
/// `CUSTOM_ID` or a heading given the label the Org exporters print
/// (`Figure 1`, `Table 2`, `(3)`, the heading's title).
fn for_pandoc(text: &str) -> String {
    use org_syntax::SyntaxKind::*;
    use org_syntax::ast::{self, AstNode};
    let root = org_syntax::parse(text).syntax();
    let mut labels: std::collections::HashMap<String, String> = Default::default();
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let (mut figures, mut tables, mut equations, mut listings) = (0, 0, 0, 0);
    for n in root.descendants() {
        let name = crate::affiliated::value(&n, "NAME");
        let caption = crate::affiliated::value(&n, "CAPTION").is_some();
        let label = match n.kind() {
            PARAGRAPH if caption && n.descendants().any(|d| d.kind() == LINK) => {
                figures += 1;
                Some(format!("Figure {figures}"))
            }
            TABLE if caption => {
                tables += 1;
                Some(format!("Table {tables}"))
            }
            SRC_BLOCK if caption => {
                listings += 1;
                Some(format!("Listing {listings}"))
            }
            LATEX_ENVIRONMENT => {
                let value = ast::LatexEnvironment::cast(n.clone())
                    .map(|l| l.value())
                    .unwrap_or_default();
                let start = usize::from(n.text_range().start())
                    + n.text().to_string().find(value.trim_start()).unwrap_or(0);
                let body = value.trim();
                let env = body
                    .strip_prefix("\\begin{")
                    .and_then(|r| r.split_once('}'))
                    .map(|(e, _)| e.to_string())
                    .unwrap_or_default();
                let inner = body
                    .strip_prefix(&format!("\\begin{{{env}}}"))
                    .and_then(|r| r.strip_suffix(&format!("\\end{{{env}}}")))
                    .map(str::trim);
                let base = env.trim_end_matches('*');
                let display = match (base, inner) {
                    ("equation" | "displaymath" | "math" | "multline", Some(i)) => {
                        Some(i.to_string())
                    }
                    ("align" | "flalign" | "alignat" | "eqnarray", Some(i)) => {
                        Some(format!("\\begin{{aligned}}\n{i}\n\\end{{aligned}}"))
                    }
                    ("gather", Some(i)) => {
                        Some(format!("\\begin{{gathered}}\n{i}\n\\end{{gathered}}"))
                    }
                    _ => None,
                };
                match display {
                    Some(d) => {
                        edits.push((start..start + body.len(), format!("\\[\n{d}\n\\]")));
                        (!env.ends_with('*')).then(|| {
                            equations += 1;
                            format!("({equations})")
                        })
                    }
                    None => None,
                }
            }
            _ => None,
        };
        if let (Some(name), Some(label)) = (name, label) {
            labels.entry(name).or_insert(label);
        }
    }
    let doc = org_model::Document::new(org_syntax::parse(text));
    for e in doc.outline().entries.iter().filter(|e| !e.inlinetask) {
        let title = e.raw_title.trim().to_string();
        if let Some((_, id)) = e
            .drawer
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("CUSTOM_ID"))
        {
            labels.entry(format!("#{id}")).or_insert(title.clone());
        }
        labels.entry(format!("*{title}")).or_insert(title);
    }
    for n in root.descendants().filter(|n| n.kind() == LINK) {
        let t = n.text().to_string();
        let t = t.trim_end();
        let Some(target) = t.strip_prefix("[[").and_then(|r| r.strip_suffix("]]")) else {
            continue;
        };
        if target.contains("][") {
            continue;
        }
        if let Some(label) = labels.get(target) {
            let start = usize::from(n.text_range().start());
            edits.push((start..start + t.len(), format!("[[{target}][{label}]]")));
        }
    }
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    let mut out = text.to_string();
    for (r, new) in edits {
        out.replace_range(r, &new);
    }
    out
}

/// Writes `text` (from the Org file `file`) as `format` to `out` with the
/// pandoc at `pandoc`. Relative links and images resolve from the file's
/// folder.
pub fn export(
    pandoc: &Path,
    text: &str,
    file: &Path,
    format: Format,
    out: &Path,
) -> Result<(), String> {
    let prepared = prepare(text, Some(file))?;
    let dir = file.parent().filter(|d| !d.as_os_str().is_empty());
    let mut cmd = Command::new(pandoc);
    cmd.args([
        "--from",
        "org",
        "--to",
        format.pandoc_name(),
        "--standalone",
        "--output",
    ])
    .arg(out);
    if let Some(d) = dir {
        cmd.current_dir(d).arg("--resource-path").arg(d);
    }
    // Citations, from the file `#+BIBLIOGRAPHY` names.
    let lower = prepared.to_lowercase();
    if lower.contains("[cite") || lower.contains("#+print_bibliography") {
        cmd.arg("--citeproc");
    }
    run(cmd, Some(prepared.as_bytes())).map(|_| ())
}

/// Writes the LaTeX file `file` as `to` (`html5`, `markdown`, `docx`)
/// to `out` through pandoc, for co-authors on Word and the web (T2.7h.26):
/// the pictures found beside the file, citations resolved when the
/// document names a bibliography, formulas as MathML in HTML.
pub fn export_latex(pandoc: &Path, file: &Path, to: &str, out: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(pandoc);
    cmd.args(["--from", "latex", "--to", to, "--standalone", "--output"])
        .arg(out);
    if to.starts_with("html") {
        cmd.arg("--mathml");
    }
    if text.contains("\\bibliography{") || text.contains("\\addbibresource{") {
        cmd.arg("--citeproc");
    }
    if let Some(d) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
        cmd.current_dir(d).arg("--resource-path").arg(d);
    }
    cmd.arg(file.file_name().map(PathBuf::from).unwrap_or_default());
    run(cmd, None).map(|_| ())
}

/// The Org text pandoc makes of `input` (Word, OpenDocument, Markdown,
/// HTML, EPUB or RTF, by its extension), cleaned up; the pictures in it go
/// to `media`, which the links name relative to `input`'s folder.
pub fn import(pandoc: &Path, input: &Path, media: Option<&Path>) -> Result<String, String> {
    let from = match input
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        // Markdown as Kalem's Markdown mode reads it: CommonMark with
        // GitHub's extensions, and a YAML metadata block for the title.
        Some("md" | "markdown" | "mdown" | "mkd" | "gfm") => Some("gfm+yaml_metadata_block"),
        Some("htm" | "html" | "xhtml") => Some("html"),
        Some("docx") => Some("docx"),
        Some("odt") => Some("odt"),
        Some("epub") => Some("epub"),
        Some("rtf") => Some("rtf"),
        Some("tex") => Some("latex"),
        _ => None,
    };
    let mut cmd = Command::new(pandoc);
    if let Some(f) = from {
        cmd.args(["--from", f]);
    }
    cmd.args(["--to", "org", "--standalone", "--wrap=none"]);
    if let Some(dir) = input.parent().filter(|d| !d.as_os_str().is_empty()) {
        cmd.current_dir(dir);
    }
    if let Some(m) = media {
        cmd.arg(format!("--extract-media={}", m.display()));
    }
    cmd.arg(input.file_name().map(PathBuf::from).unwrap_or_default());
    let out = run(cmd, None)?;
    Ok(cleanup(&out))
}

fn run(mut cmd: Command, stdin: Option<&[u8]>) -> Result<String, String> {
    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(input).map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("pandoc: {}", out.status)
        } else {
            err
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The clean-up pass ("Org cleanup") over pandoc's Org: property drawers
/// holding only a `CUSTOM_ID` nothing links to go, and so do `<<anchors>>`
/// nothing links to (Word's bookmarks); a link whose description repeats
/// its target loses the description; tables and blank lines are
/// formatted as `kalem fmt` formats them.
pub fn cleanup(org: &str) -> String {
    // What the document links to.
    let mut targets: Vec<String> = Vec::new();
    let mut rest = org;
    while let Some(i) = rest.find("[[") {
        let after = &rest[i + 2..];
        let end = after.find(']').unwrap_or(after.len());
        targets.push(after[..end].to_string());
        rest = &after[end..];
    }
    let linked = |id: &str| targets.iter().any(|t| t == &format!("#{id}") || t == id);
    let mut out: Vec<String> = Vec::new();
    let lines: Vec<&str> = org.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        // `:PROPERTIES:` / `:CUSTOM_ID: x` / `:END:` right after a heading.
        if l.trim().eq_ignore_ascii_case(":PROPERTIES:")
            && i + 2 < lines.len()
            && lines[i + 2].trim().eq_ignore_ascii_case(":END:")
            && out.last().is_some_and(|p| p.starts_with('*'))
        {
            let prop = lines[i + 1].trim();
            if let Some(id) = prop
                .strip_prefix(":CUSTOM_ID:")
                .or_else(|| prop.strip_prefix(":custom_id:"))
                && !linked(id.trim())
            {
                i += 3;
                continue;
            }
        }
        out.push(clean_line(l, &linked));
        i += 1;
    }
    let mut text = out.join("\n");
    if org.ends_with('\n') {
        text.push('\n');
    }
    let doc = org_model::Document::new(org_syntax::parse(&text));
    org_edit::format::format(&doc)
}

/// A line without anchors nothing links to, and with links that repeat
/// their target as description shortened.
fn clean_line(l: &str, linked: &dyn Fn(&str) -> bool) -> String {
    let mut s = l.to_string();
    // `<<anchor>>`, not a radio target.
    let mut from = 0;
    while let Some(i) = s[from..].find("<<") {
        let at = from + i;
        if s[at..].starts_with("<<<") {
            from = at + 3;
            continue;
        }
        let Some(j) = s[at + 2..].find(">>") else {
            break;
        };
        let id = s[at + 2..at + 2 + j].to_string();
        if !id.is_empty() && !id.contains(['<', '>', '\n']) && !linked(&id) {
            s.replace_range(at..at + 4 + j, "");
            from = at;
        } else {
            from = at + 2;
        }
    }
    // `[[x][x]]` and `[[file:x][file:x]]`.
    let mut from = 0;
    while let Some(i) = s[from..].find("[[") {
        let at = from + i;
        let Some(mid) = s[at..].find("][").map(|m| at + m) else {
            break;
        };
        let Some(end) = s[mid..].find("]]").map(|e| mid + e) else {
            break;
        };
        let target = &s[at + 2..mid];
        let desc = &s[mid + 2..end];
        if desc == target
            || format!("file:{desc}") == target
            || desc.strip_prefix("file:") == Some(target)
        {
            let t = target.to_string();
            s.replace_range(at..end + 2, &format!("[[{t}]]"));
            from = at + t.len() + 4;
        } else {
            from = end + 2;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pandoc_path_setting() {
        let empty = std::ffi::OsString::new();
        assert_eq!(find_with("", &empty), None);
        assert_eq!(find_with("/no/such/pandoc", &empty), None);
        let me = std::env::current_exe().unwrap();
        assert_eq!(find_with(me.to_str().unwrap(), &empty), Some(me.clone()));
        let dir = me.parent().unwrap().as_os_str().to_owned();
        let name = me.file_name().unwrap().to_str().unwrap();
        assert_eq!(find_with(name, &dir), Some(me.clone()));
    }

    #[test]
    fn cleaning_up() {
        let org = "#+title: My page\n\n* Introduction\n:PROPERTIES:\n:CUSTOM_ID: intro\n:END:\nSome *bold* and [[#usage][a link]] and [[https://x.y][https://x.y]].\n\n** Usage\n:PROPERTIES:\n:CUSTOM_ID: usage\n:END:\n| a | bb |\n|-+-|\n| 1 | 2 |\n\n<<anchor1>>Target <<kept>> and [[kept]], <<<radio>>>.\n";
        assert_eq!(
            cleanup(org),
            "#+title: My page\n\n* Introduction\nSome *bold* and [[#usage][a link]] and [[https://x.y]].\n\n** Usage\n:PROPERTIES:\n:CUSTOM_ID: usage\n:END:\n| a | bb |\n|---+----|\n| 1 |  2 |\n\nTarget <<kept>> and [[kept]], <<<radio>>>.\n"
        );
        assert_eq!(Format::from_name(".DOCX"), Some(Format::Docx));
        assert_eq!(Format::Epub.pandoc_name(), "epub3");
    }

    #[test]
    fn preparing_for_pandoc() {
        let text = "See [[fig:a]], [[tab:t]], [[eq:e]], [[#intro]] and [[*Intro]].\n\n* Intro\n:PROPERTIES:\n:CUSTOM_ID: intro\n:END:\n#+CAPTION: A picture.\n#+NAME: fig:a\n[[file:a.png]]\n\n#+CAPTION: Numbers.\n#+NAME: tab:t\n| 1 |\n\n#+NAME: eq:e\n\\begin{equation}\nE = mc^2\n\\end{equation}\n\n\\begin{align*}\na &= b\n\\end{align*}\n";
        let out = for_pandoc(text);
        assert!(
            out.starts_with("See [[fig:a][Figure 1]], [[tab:t][Table 1]], [[eq:e][(1)]], [[#intro][Intro]] and [[*Intro][Intro]]."),
            "{out}"
        );
        assert!(out.contains("#+NAME: eq:e\n\\[\nE = mc^2\n\\]\n"), "{out}");
        assert!(
            out.contains("\\[\n\\begin{aligned}\na &= b\n\\end{aligned}\n\\]"),
            "{out}"
        );
    }

    #[test]
    fn preparing() {
        let text = "#+MACRO: who World\nHello {{{who}}}, @@kalem:color=red@@red@@kalem:end@@.\n";
        assert_eq!(
            prepare(text, None).unwrap(),
            "#+MACRO: who World\nHello World, red.\n"
        );
    }

    /// With pandoc installed: a round trip through Word keeps the text.
    #[test]
    fn round_trip_through_word() {
        let search = std::env::var_os("PATH").unwrap_or_default();
        let Some(pandoc) = find(&search) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("kalem-pandoc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let org = dir.join("doc.org");
        let text = "#+TITLE: A document\n\n* First\nSome *bold* text[fn:1].\n\n| a | b |\n|---+---|\n| 1 | 2 |\n\n[fn:1] A note.\n";
        std::fs::write(&org, text).unwrap();
        let docx = dir.join("doc.docx");
        export(&pandoc, text, &org, Format::Docx, &docx).unwrap();
        assert!(docx.is_file());
        let back = import(&pandoc, &docx, None).unwrap();
        assert!(back.contains("#+title: A document"), "{back}");
        assert!(back.contains("* First\nSome *bold* text[fn:1]."), "{back}");
        assert!(back.contains("| a | b |\n|---+---|\n| 1 | 2 |"), "{back}");
        assert!(!back.contains("CUSTOM_ID"), "{back}");
    }
}
