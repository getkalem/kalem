//! `kalem book build` and `kalem book check`: the Book (design_doc2.md §10)
//! as a static site, built by Kalem's own HTML exporter.
//!
//! `book/index.org` is the table of contents: its title, an introduction,
//! then one headline for each part with a list of links to the part's
//! chapters (`[[file:part-1/installing.org][Installing]]`). A link to
//! `generated:NAME` is a chapter written at build time from the code, so
//! that it cannot drift from it: `generated:commands` (every command, its
//! scope and keys), `generated:settings` (every setting) and
//! `generated:cli` (the command line's help). Each chapter is exported
//! with the HTML back-end, formulas as SVG, into the page template
//! `book/theme/page.html`; the theme's other files are copied beside the
//! pages, and `search-index.js` holds the text of every page for the
//! search box.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::Result;

/// A chapter of the Book.
#[derive(Debug, Clone)]
struct Chapter {
    /// Its title in the table of contents.
    title: String,
    /// Its Org source relative to the Book's folder, or `generated:NAME`.
    source: String,
}

impl Chapter {
    /// The page's path relative to the site's root.
    fn page(&self) -> String {
        match self.source.strip_prefix("generated:") {
            Some(name) => format!("appendices/{name}.html"),
            None => format!("{}.html", self.source.trim_end_matches(".org")),
        }
    }
}

/// A part: its title and chapters.
#[derive(Debug, Clone)]
struct Part {
    title: String,
    chapters: Vec<Chapter>,
}

/// The table of contents read from `index.org`.
#[derive(Debug)]
struct Contents {
    title: String,
    /// The Org text before the first part.
    intro: String,
    parts: Vec<Part>,
}

/// Reads the table of contents.
fn contents(index: &str) -> Contents {
    let mut title = String::from("The Kalem Book");
    let mut intro = String::new();
    let mut parts: Vec<Part> = Vec::new();
    for line in index.lines() {
        if let Some(t) = line.strip_prefix("#+TITLE:") {
            title = t.trim().to_string();
            continue;
        }
        if let Some(h) = line.strip_prefix("* ") {
            parts.push(Part {
                title: h.trim().to_string(),
                chapters: Vec::new(),
            });
            continue;
        }
        match parts.last_mut() {
            None => {
                intro.push_str(line);
                intro.push('\n');
            }
            Some(p) => {
                let item = line.trim_start();
                if let Some(rest) = item.strip_prefix("- [[")
                    && let Some((target, rest)) = rest.split_once("][")
                    && let Some((name, _)) = rest.split_once("]]")
                {
                    let source = target.strip_prefix("file:").unwrap_or(target).to_string();
                    p.chapters.push(Chapter {
                        title: name.to_string(),
                        source,
                    });
                }
            }
        }
    }
    Contents {
        title,
        intro,
        parts,
    }
}

/// Escapes text for HTML.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The text of an HTML fragment, tags taken out, blanks collapsed.
fn plain_text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut skip = false;
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    let b = html.as_bytes();
    while i < b.len() {
        if !in_tag && lower[i..].starts_with("<svg") {
            skip = true;
        }
        if skip && lower[i..].starts_with("</svg>") {
            skip = false;
            i += "</svg>".len();
            continue;
        }
        let c = b[i];
        if c == b'<' {
            in_tag = true;
        } else if c == b'>' && in_tag {
            in_tag = false;
            out.push(' ');
        } else if !in_tag && !skip {
            // Whole characters: step over the continuation bytes.
            let ch_len = html[i..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&html[i..i + ch_len]);
            i += ch_len;
            continue;
        }
        i += 1;
    }
    let out = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A string as a JavaScript string literal.
fn js_string(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

/// Text from the code (descriptions written with Markdown's backticks)
/// as the text of an Org table cell: `code` as Org code, `|` as a
/// vertical bar Org does not read as a column.
fn cell(s: &str) -> String {
    let mut out = String::new();
    for (i, part) in s.split('`').enumerate() {
        if i % 2 == 1 && !part.is_empty() && !part.contains('|') && !part.contains('~') {
            out.push('~');
            out.push_str(part);
            out.push('~');
        } else {
            out.push_str(&part.replace('|', "\\vert{}"));
        }
    }
    out
}

/// A value as code in an Org table cell, unless it holds a `|`, which a
/// cell cannot hold as code.
fn code(s: &str) -> String {
    if s.contains('|') || s.contains('~') {
        s.replace('|', "\\vert{}")
    } else {
        format!("~{s}~")
    }
}

/// The Org source of a generated chapter.
fn generated(name: &str) -> Option<String> {
    Some(match name {
        "commands" => {
            let reg = kalem_core::CommandRegistry::with_builtins();
            let mut cmds: Vec<_> = reg.commands().collect();
            cmds.sort_by(|a, b| a.id.cmp(&b.id));
            let mut s = String::from(
                "#+OPTIONS: ^:{}\nEvery command of Kalem, generated from the command registry when the Book is built. The scope says which types of text a command serves (§11.2 of the design); the keys are the default ones, before a keymap profile or your own keymap changes them.\n\n| Command | Title | Scope | Keys |\n|-\n",
            );
            for c in cmds {
                let keys: Vec<String> = c
                    .default_keys
                    .iter()
                    .map(|k| code(&k.to_string()))
                    .collect();
                let scope = c.scope.as_ref().map_or_else(String::new, |s| s.describe());
                s.push_str(&format!(
                    "| {} | {} | {} | {} |\n",
                    code(&c.id),
                    cell(&c.display_title()),
                    cell(&scope),
                    keys.join(" ")
                ));
            }
            s
        }
        "settings" => {
            let mut s = String::from(
                "#+OPTIONS: ^:{}\nEvery setting of Kalem, generated from the settings' definitions when the Book is built. Settings are written in =settings.toml= (your own) or =.kalem/settings.toml= (a workspace's), as the chapter on settings describes.\n\n| Setting | Type | Default | What it does |\n|-\n",
            );
            for spec in kalem_core::settings::SPECS {
                use kalem_core::settings::Kind;
                let kind = match spec.kind {
                    Kind::Bool => "true or false".to_string(),
                    Kind::Int(a, b) => format!("a number from {a} to {b}"),
                    Kind::Str => "text".to_string(),
                    Kind::Enum(v) => format!("one of {}", v.join(", ")),
                    Kind::List(None) => "a list of text".to_string(),
                    Kind::List(Some(v)) => format!("a list of {}", v.join(", ")),
                    Kind::Modes(v) => format!("a table of paths to {} or a language", v.join(", ")),
                };
                s.push_str(&format!(
                    "| {} | {} | {} | {} |\n",
                    code(spec.key),
                    cell(&kind),
                    code(spec.default),
                    cell(spec.description)
                ));
            }
            s
        }
        "cli" => {
            use clap::CommandFactory;
            let mut cmd = crate::Cli::command();
            // Subcommands' usage lines with `kalem` in front.
            cmd.build();
            let mut s = String::from(
                "The command line's own help, generated when the Book is built: =kalem --help=, then each subcommand's.\n\n",
            );
            s.push_str("#+begin_example\n");
            s.push_str(&cmd.render_long_help().to_string());
            s.push_str("#+end_example\n");
            let subs: Vec<clap::Command> = cmd.get_subcommands().cloned().collect();
            for mut sub in subs {
                if sub.get_name() == "help" {
                    continue;
                }
                s.push_str(&format!(
                    "\n* =kalem {}=\n\n#+begin_example\n",
                    sub.get_name()
                ));
                s.push_str(&sub.render_long_help().to_string());
                s.push_str("#+end_example\n");
                let inner: Vec<clap::Command> = sub.get_subcommands().cloned().collect();
                for mut i in inner {
                    if i.get_name() == "help" {
                        continue;
                    }
                    s.push_str(&format!(
                        "\n** =kalem {} {}=\n\n#+begin_example\n",
                        sub.get_name(),
                        i.get_name()
                    ));
                    s.push_str(&i.render_long_help().to_string());
                    s.push_str("#+end_example\n");
                }
            }
            s
        }
        _ => return None,
    })
}

/// The HTML body of Org `text` from `file`. Underscores stay underscores
/// (`design_doc2.md`), as `^:{}` asks, unless the chapter says otherwise.
fn export_body(text: &str, file: &Path) -> std::result::Result<String, String> {
    let text = &format!("#+OPTIONS: ^:{{}}\n{text}");
    let settings = org_export::Settings {
        body_only: true,
        input_file: Some(std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf())),
        now: None,
        subtree: None,
        math: Some(kalem_core::math::export_renderer()),
        options: None,
    };
    org_export::export(text, &org_export::Html, &settings)
}

/// A page's path to the site's root (`../` for `part-1/x.html`).
fn root_of(page: &str) -> String {
    "../".repeat(page.matches('/').count())
}

/// The table of contents as HTML, `current` marked.
fn toc_html(c: &Contents, current: Option<&str>, root: &str) -> String {
    let mut s = String::from("<nav class=\"toc\"><ol class=\"parts\">");
    for p in &c.parts {
        s.push_str(&format!(
            "<li class=\"part\"><span class=\"part-title\">{}</span><ol>",
            escape(&p.title)
        ));
        for ch in &p.chapters {
            let page = ch.page();
            let here = if current == Some(page.as_str()) {
                " class=\"current\" aria-current=\"page\""
            } else {
                ""
            };
            s.push_str(&format!(
                "<li><a href=\"{root}{page}\"{here}>{}</a></li>",
                escape(&ch.title)
            ));
        }
        s.push_str("</ol></li>");
    }
    s.push_str("</ol></nav>");
    s
}

/// The links in `html` to pages of the site that do not exist (`href`s
/// ending in `.html`, relative, from page `page`), and to `.org` files.
fn broken_links(html: &str, page: &str, pages: &[String]) -> Vec<String> {
    let dir = Path::new(page).parent().unwrap_or(Path::new(""));
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("href=\"") {
        rest = &rest[i + 6..];
        let Some(end) = rest.find('"') else { break };
        let href = &rest[..end];
        rest = &rest[end..];
        if href.contains("://") || href.starts_with('#') || href.starts_with("mailto:") {
            continue;
        }
        let target = href.split('#').next().unwrap_or("");
        if target.ends_with(".org") {
            out.push(href.to_string());
            continue;
        }
        if !target.ends_with(".html") {
            continue;
        }
        // Resolve `..` against the page's folder.
        let mut parts: Vec<String> = dir
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        for seg in target.split('/') {
            match seg {
                "." | "" => {}
                ".." => {
                    parts.pop();
                }
                s => parts.push(s.to_string()),
            }
        }
        let resolved = parts.join("/");
        if !pages.contains(&resolved) {
            out.push(href.to_string());
        }
    }
    out
}

/// A page of the site: its path and HTML body.
struct Page {
    path: String,
    title: String,
    body: String,
}

/// Exports every chapter; the pages and the problems found.
fn pages(dir: &Path, c: &Contents) -> (Vec<Page>, Vec<String>) {
    let mut problems = Vec::new();
    let mut out = Vec::new();
    let intro = export_body(&c.intro, &dir.join("index.org")).unwrap_or_else(|e| {
        problems.push(format!("index.org: {e}"));
        String::new()
    });
    out.push(Page {
        path: "index.html".into(),
        title: c.title.clone(),
        body: intro,
    });
    for p in &c.parts {
        for ch in &p.chapters {
            let (text, file) = match ch.source.strip_prefix("generated:") {
                Some(name) => match generated(name) {
                    Some(t) => (t, dir.join("appendices").join(format!("{name}.org"))),
                    None => {
                        problems.push(format!("index.org: no generated chapter {name:?}"));
                        continue;
                    }
                },
                None => {
                    let file = dir.join(&ch.source);
                    match std::fs::read_to_string(&file) {
                        Ok(t) => (t, file),
                        Err(e) => {
                            problems.push(format!("{}: {e}", file.display()));
                            continue;
                        }
                    }
                }
            };
            match export_body(&text, &file) {
                Ok(body) => out.push(Page {
                    path: ch.page(),
                    title: ch.title.clone(),
                    body,
                }),
                Err(e) => problems.push(format!("{}: {e}", ch.source)),
            }
        }
    }
    let paths: Vec<String> = out.iter().map(|p| p.path.clone()).collect();
    for p in &out {
        for l in broken_links(&p.body, &p.path, &paths) {
            problems.push(format!("{}: broken link {l}", p.path));
        }
    }
    (out, problems)
}

/// `kalem book check DIR`: every chapter exports, every link inside the
/// Book leads to a page of it.
pub(crate) fn check(dir: &Path) -> Result<ExitCode> {
    let index = std::fs::read_to_string(dir.join("index.org"))
        .map_err(|e| format!("{}: {e}", dir.join("index.org").display()))?;
    let c = contents(&index);
    let (pages, problems) = pages(dir, &c);
    for p in &problems {
        eprintln!("{p}");
    }
    println!("{} pages, {} problems", pages.len(), problems.len());
    Ok(if problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// `kalem book build DIR --out OUT`: the site.
pub(crate) fn build(dir: &Path, out: &Path) -> Result<ExitCode> {
    let index = std::fs::read_to_string(dir.join("index.org"))
        .map_err(|e| format!("{}: {e}", dir.join("index.org").display()))?;
    let c = contents(&index);
    let template = std::fs::read_to_string(dir.join("theme/page.html"))
        .map_err(|e| format!("{}: {e}", dir.join("theme/page.html").display()))?;
    let (pages, problems) = pages(dir, &c);
    for p in &problems {
        eprintln!("{p}");
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    // The order of reading, for the previous and next links.
    let order: Vec<(String, String)> = pages
        .iter()
        .map(|p| (p.path.clone(), p.title.clone()))
        .collect();
    let mut search = String::from("window.BOOK_INDEX = [\n");
    for (i, p) in pages.iter().enumerate() {
        let root = root_of(&p.path);
        let nav = |j: Option<usize>, class: &str, label: &str| {
            j.and_then(|j| order.get(j))
                .map_or_else(String::new, |(path, title)| {
                    format!(
                        "<a class=\"{class}\" href=\"{root}{path}\"><span>{label}</span> {}</a>",
                        escape(title)
                    )
                })
        };
        let html = template
            .replace("{{book_title}}", &escape(&c.title))
            .replace("{{title}}", &escape(&p.title))
            .replace("{{root}}", &root)
            .replace("{{toc}}", &toc_html(&c, Some(&p.path), &root))
            .replace("{{prev}}", &nav(i.checked_sub(1), "prev", "←"))
            .replace("{{next}}", &nav(Some(i + 1), "next", "→"))
            .replace("{{content}}", &p.body);
        let target = out.join(&p.path);
        if let Some(d) = target.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        std::fs::write(&target, html).map_err(|e| format!("{}: {e}", target.display()))?;
        let text: String = plain_text(&p.body).chars().take(40_000).collect();
        search.push_str(&format!(
            "{{t:{},u:{},x:{}}},\n",
            js_string(&p.title),
            js_string(&p.path),
            js_string(&text)
        ));
    }
    search.push_str("];\n");
    std::fs::write(out.join("search-index.js"), search)
        .map_err(|e| format!("{}: {e}", out.display()))?;
    // The theme's files beside the pages.
    let theme = dir.join("theme");
    let out_theme = out.join("theme");
    std::fs::create_dir_all(&out_theme).map_err(|e| format!("{}: {e}", out_theme.display()))?;
    for e in std::fs::read_dir(&theme).map_err(|e| format!("{}: {e}", theme.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let name = e.file_name();
        if name == "page.html" || !e.path().is_file() {
            continue;
        }
        std::fs::copy(e.path(), out_theme.join(&name))
            .map_err(|err| format!("{}: {err}", e.path().display()))?;
    }
    // Pictures the chapters use, kept at their paths.
    copy_assets(dir, out, dir)?;
    // GitHub Pages serves the folder as it is.
    std::fs::write(out.join(".nojekyll"), "").map_err(|e| e.to_string())?;
    println!("{} ({} pages)", out.display(), pages.len());
    Ok(if problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// Copies the pictures under `from` to the same place under `out`.
fn copy_assets(base: &Path, out: &Path, from: &Path) -> Result<()> {
    let Ok(rd) = std::fs::read_dir(from) else {
        return Ok(());
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "theme") {
                continue;
            }
            copy_assets(base, out, &p)?;
            continue;
        }
        let ext = p
            .extension()
            .map(|x| x.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !matches!(
            ext.as_str(),
            "png" | "jpg" | "jpeg" | "svg" | "gif" | "webp"
        ) {
            continue;
        }
        let rel: PathBuf = p.strip_prefix(base).map_err(|e| e.to_string())?.into();
        let target = out.join(rel);
        if let Some(d) = target.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::copy(&p, &target).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_contents() {
        let c = contents(
            "#+TITLE: B\nHello.\n* Part I. Kalem\n- [[file:part-1/a.org][A]]\n- [[generated:commands][Commands]]\n* Part II\n",
        );
        assert_eq!(c.title, "B");
        assert_eq!(c.intro, "Hello.\n");
        assert_eq!(c.parts.len(), 2);
        assert_eq!(c.parts[0].chapters[0].page(), "part-1/a.html");
        assert_eq!(c.parts[0].chapters[1].page(), "appendices/commands.html");
    }

    #[test]
    fn finds_broken_links() {
        let pages = vec!["index.html".to_string(), "part-1/a.html".to_string()];
        let html = "<a href=\"../index.html\">x</a><a href=\"b.html#s\">y</a><a href=\"https://x.org/a.html\">z</a><a href=\"c.org\">w</a>";
        assert_eq!(
            broken_links(html, "part-1/a.html", &pages),
            vec!["b.html#s".to_string(), "c.org".to_string()]
        );
    }

    #[test]
    fn plain_text_of_html() {
        assert_eq!(
            plain_text("<p>A &amp; <b>B</b></p><svg><text>x</text></svg>ç"),
            "A & B ç"
        );
    }

    #[test]
    fn generated_chapters() {
        for name in ["commands", "settings", "cli"] {
            let org = generated(name).unwrap();
            let html = export_body(&org, Path::new("appendices/x.org")).unwrap();
            assert!(html.len() > 1000, "{name}");
        }
        assert!(generated("commands").unwrap().contains("~bib.sortView~"));
    }
}
