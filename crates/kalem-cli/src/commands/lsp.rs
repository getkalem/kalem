//! `kalem lsp`: the language plugins and their servers from the command
//! line (D57, T3.8.1): what serves a file, and a server's diagnostics,
//! documentation and definitions without an editor (headless, as the
//! tests and scripts use them).

#![allow(clippy::print_stdout)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kalem_core::DocumentState;
use kalem_core::languages::{self, Resolved};
use kalem_core::lsp::{self, Kind, Outcome};
use kalem_core::settings::{self, Config};

use super::Result;

fn load_settings(file: Option<&Path>) {
    let user = settings::config_dir().map(|d| d.join("settings.toml"));
    let workspace = file
        .and_then(|p| std::path::absolute(p).ok())
        .and_then(|p| p.parent().and_then(settings::find_workspace_settings));
    Config::load(user.as_deref(), workspace.as_deref()).apply_process_settings();
}

fn open(file: &Path) -> Result<DocumentState> {
    let path = std::path::absolute(file).map_err(|e| format!("{}: {e}", file.display()))?;
    DocumentState::open(
        &path,
        Arc::new(org_model::Settings::default()),
        &org_syntax::ParseContext::default(),
    )
    .map_err(|e| format!("{}: {e}", file.display()))
}

/// `kalem lsp status [FILE]`.
pub(crate) fn status(file: Option<&Path>) -> Result<ExitCode> {
    load_settings(file);
    println!("Plugin folders:");
    for d in languages::plugin_dirs() {
        println!("  {}", d.display());
    }
    // Each plugin's syntaxes too.
    languages::wait();
    let plugins = languages::plugins();
    if plugins.is_empty() {
        println!("No language plugins.");
    }
    for p in &plugins {
        println!("{} {} ({}) in {}", p.id, p.version, p.name, p.dir.display());
        for l in &p.languages {
            println!(
                "  {}: .{} → {}",
                l.name,
                l.extensions.join(", ."),
                l.servers.join(", ")
            );
        }
        if !p.syntaxes.is_empty() {
            println!("  syntaxes: {}", p.syntaxes.join(", "));
        }
    }
    for e in languages::problems() {
        println!("problem: {e}");
    }
    let Some(file) = file else {
        return Ok(ExitCode::SUCCESS);
    };
    let abs = std::path::absolute(file).map_err(|e| e.to_string())?;
    let first = std::fs::read_to_string(&abs).ok();
    let Some((plugin, lang)) =
        languages::for_path(&abs, first.as_deref().and_then(|t| t.lines().next()))
    else {
        println!("{}: no language plugin serves it", file.display());
        return Ok(ExitCode::from(1));
    };
    let spec = lang.servers.iter().find_map(|k| plugin.server(k));
    let root = spec
        .and_then(|s| kalem_lsp::find_root(&abs, &s.root_markers, s.root_outermost))
        .or_else(|| abs.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    println!(
        "{}: {} ({}), root {}",
        file.display(),
        lang.name,
        plugin.id,
        root.display()
    );
    match languages::resolve_server(&plugin, &lang, Some(&root)) {
        Resolved::Found(s, program, args) => {
            println!(
                "  server: {} — {} {}",
                s.name,
                program.display(),
                args.join(" ")
            );
            Ok(ExitCode::SUCCESS)
        }
        Resolved::Off => {
            println!("  server: off (settings)");
            Ok(ExitCode::SUCCESS)
        }
        Resolved::Missing(m) => {
            println!("  server: {m}");
            Ok(ExitCode::from(1))
        }
    }
}

/// Waits until the servers of the open documents are ready and have been
/// quiet for `quiet`, at most `limit`.
fn settle(docs: &[DocumentState], quiet: Duration, limit: Duration) -> Result<()> {
    let start = Instant::now();
    let mut last_change = Instant::now();
    let mut last: Vec<usize> = Vec::new();
    loop {
        if lsp::tick() {
            last_change = Instant::now();
        }
        // What servers say on their own: their errors to standard error.
        for (text, error) in kalem_core::jobs::take_notices() {
            if error {
                eprintln!("{text}");
            }
        }
        // Diagnostics and log lines are activity: servers that report no
        // progress (Expert) still log their builds.
        let now: Vec<usize> = docs
            .iter()
            .flat_map(|d| {
                let p = d.meta.path.as_deref().unwrap_or(Path::new(""));
                [lsp::diagnostics(p).len(), lsp::log(p).len()]
            })
            .collect();
        if now != last {
            last = now;
            last_change = Instant::now();
        }
        let ready = docs
            .iter()
            .all(|d| lsp::can(d, Kind::Hover) || !lsp::serves(d));
        let busy = docs
            .iter()
            .any(|d| d.meta.path.as_deref().is_some_and(lsp::working));
        if ready && !busy && last_change.elapsed() >= quiet {
            return Ok(());
        }
        if start.elapsed() > limit {
            return Err(format!(
                "the language server did not settle in {}s",
                limit.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `kalem lsp check FILE...`: the diagnostics of the files' servers.
pub(crate) fn check(files: &[PathBuf], json: bool, wait: u64, log: bool) -> Result<ExitCode> {
    load_settings(files.first().map(PathBuf::as_path));
    let docs: Vec<DocumentState> = files.iter().map(|f| open(f)).collect::<Result<_>>()?;
    for d in &docs {
        lsp::sync(d);
        if !lsp::serves(d) {
            let why = lsp::describe(d).unwrap_or_else(|| "no language plugin serves it".into());
            return Err(format!(
                "{}: {why}",
                d.meta.path.as_deref().unwrap_or(Path::new("")).display()
            ));
        }
    }
    settle(&docs, Duration::from_secs(3), Duration::from_secs(wait))?;
    let mut errors = 0;
    let mut out = Vec::new();
    for (f, d) in files.iter().zip(&docs) {
        let text = d.text().as_str();
        for diag in lsp::diagnostics(d.meta.path.as_deref().unwrap_or(Path::new(""))) {
            let line = text[..diag.range.start].matches('\n').count() + 1;
            let col =
                diag.range.start - text[..diag.range.start].rfind('\n').map_or(0, |i| i + 1) + 1;
            let sev = format!("{:?}", diag.severity).to_lowercase();
            if sev == "error" {
                errors += 1;
            }
            if json {
                out.push(serde_json::json!({
                    "file": f, "line": line, "column": col, "severity": sev,
                    "message": diag.message, "source": diag.source,
                }));
            } else {
                let src = diag.source.map(|s| format!(" [{s}]")).unwrap_or_default();
                println!(
                    "{}:{line}:{col}: {sev}: {}{src}",
                    f.display(),
                    diag.message.lines().next().unwrap_or("")
                );
            }
        }
    }
    if json {
        println!("{}", serde_json::Value::Array(out));
    }
    if log && let Some(p) = docs.first().and_then(|d| d.meta.path.as_deref()) {
        for line in lsp::log(p) {
            eprintln!("{line}");
        }
    }
    lsp::shutdown_all();
    Ok(if errors > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// `kalem lsp hover|definition|references FILE LINE:COL`.
pub(crate) fn at(kind: &str, file: &Path, place: &str, wait: u64) -> Result<ExitCode> {
    load_settings(Some(file));
    let completion = kind == "completion";
    let kind = match kind {
        "completion" => Kind::Hover,
        "hover" => Kind::Hover,
        "definition" => Kind::Definition,
        "references" => Kind::References,
        "symbols" => Kind::Symbols,
        "signature" => Kind::Signature,
        "format" => Kind::Format,
        k => return Err(format!("unknown request {k}")),
    };
    let mut doc = open(file)?;
    let (line, col) = place
        .split_once(':')
        .and_then(|(l, c)| Some((l.parse::<usize>().ok()?, c.parse::<usize>().ok()?)))
        .ok_or_else(|| format!("{place}: expected LINE:COLUMN"))?;
    let text = doc.text();
    let l = line.max(1) - 1;
    if l >= text.line_count() {
        return Err(format!("{place}: past the end"));
    }
    let start = text.line_range(l).start;
    let at = (start + col.max(1) - 1).min(text.line_range(l).end);
    doc.selection = org_edit::Selection::caret(at);
    lsp::sync(&doc);
    settle(
        std::slice::from_ref(&doc),
        Duration::from_secs(3),
        Duration::from_secs(wait),
    )?;
    if completion {
        // As typing does: not asked for, the trigger decides.
        let reg = kalem_core::completers::Registry::with_builtins();
        let t = Instant::now();
        let items = reg.complete(&mut doc, false, Duration::from_secs(wait.min(10)));
        eprintln!("{} items in {} ms", items.len(), t.elapsed().as_millis());
        for i in &items {
            let doc = i
                .documentation
                .as_deref()
                .and_then(|d| d.lines().next())
                .unwrap_or("");
            println!("{}\t{}\t{}\t{doc}", i.label, i.insert, i.source);
        }
        lsp::shutdown_all();
        return Ok(if items.iter().any(|i| i.source == "lsp") {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        });
    }
    lsp::request(&doc, kind)?;
    let path = doc.meta.path.clone();
    let start = Instant::now();
    let outcome = loop {
        lsp::tick();
        if let Some(o) = path
            .as_deref()
            .and_then(|p| lsp::take_outcomes(p, doc.version()).into_iter().next())
        {
            break o;
        }
        if start.elapsed() > Duration::from_secs(wait) {
            return Err("no answer in time".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let code = match outcome {
        Outcome::Message { text, error } => {
            println!("{text}");
            if error {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Outcome::Choose(items) => {
            for i in items {
                println!("{}\t{}", i.title, i.category);
            }
            ExitCode::SUCCESS
        }
        Outcome::Signature { text, .. } => {
            println!("{}", text.unwrap_or_default());
            ExitCode::SUCCESS
        }
        Outcome::Hover { text, .. } => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Outcome::Jump(p) => {
            println!(
                "{}:{}:{}: {}",
                p.path.display(),
                p.line,
                p.column + 1,
                p.preview
            );
            ExitCode::SUCCESS
        }
        Outcome::Places { places, .. } => {
            for p in places {
                println!(
                    "{}:{}:{}: {}",
                    p.path.display(),
                    p.line,
                    p.column + 1,
                    p.preview
                );
            }
            ExitCode::SUCCESS
        }
        Outcome::Edits { edits, .. } => {
            print!(
                "{}",
                kalem_lsp::features::apply(doc.text().as_str(), &edits)
            );
            ExitCode::SUCCESS
        }
    };
    lsp::shutdown_all();
    Ok(code)
}
