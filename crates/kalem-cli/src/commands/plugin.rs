//! `kalem plugin`: browse the index, install, list and remove plugins,
//! as the editor's Plugins menu does (T3.3.3); start and build one
//! (T3.1.2).

#![allow(clippy::print_stdout)]

use std::io::{BufRead, Write};
use std::process::ExitCode;

use kalem_core::plugin_store;
use kalem_core::settings::{self, Config};

use super::Result;

fn config() -> Config {
    let user = settings::config_dir().map(|d| d.join("settings.toml"));
    let c = Config::load(user.as_deref(), None);
    c.apply_process_settings();
    c
}

/// `kalem plugin browse`.
pub(crate) fn browse() -> Result<ExitCode> {
    let c = config();
    let installed = plugin_store::installed();
    for e in plugin_store::fetch_indexes(&plugin_store::index_urls(&c))? {
        let state = match installed.iter().find(|i| i.id == e.id) {
            Some(i) => format!(" (installed {})", i.version),
            None if !e.declarative && e.download.is_none() => {
                " (no release yet: build and install it from its source)".into()
            }
            None => String::new(),
        };
        println!(
            "{} {}{state}\n  {} — {}",
            e.id, e.version, e.name, e.description
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// `kalem plugin install SOURCE [--yes]`.
pub(crate) fn install(source: &str, yes: bool) -> Result<ExitCode> {
    let c = config();
    let p = plugin_store::prepare(source, &plugin_store::index_urls(&c))?;
    for (i, line) in plugin_store::summary(&p).iter().enumerate() {
        println!("{}{line}", if i == 0 { "" } else { "  " });
    }
    if !yes {
        print!("Install? [y/N] ");
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        let _ = std::io::stdin().lock().read_line(&mut answer);
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            plugin_store::discard(&p.staging);
            println!("Not installed.");
            return Ok(ExitCode::from(1));
        }
    }
    let dir = plugin_store::install(&p)?;
    println!("Installed {} {} in {}", p.name, p.version, dir.display());
    Ok(ExitCode::SUCCESS)
}

/// `kalem plugin check`: each installed component viewer compiled and
/// bound to the plugin API, as opening a file would; 1 when one cannot
/// run with this Kalem.
pub(crate) fn check() -> Result<ExitCode> {
    config();
    #[cfg(feature = "plugins")]
    {
        let mut failed = false;
        // The ones built in (wasm_todo W5).
        for v in crate::embedded_viewers() {
            match v.check() {
                Ok(()) => println!("{} (built in): runs", v.label()),
                Err(e) => {
                    failed = true;
                    println!("{} (built in): cannot run: {}", v.label(), e.0);
                }
            }
        }
        let viewers = crate::installed_viewers();
        if viewers.is_empty() {
            println!("No component viewers installed.");
        }
        for (p, v) in viewers {
            let runs = match v {
                Ok(v) => v.check().map_err(|e| e.0),
                Err(why) => Err(why),
            };
            match runs {
                Ok(()) => println!("{} {}: runs", p.id, p.version),
                Err(e) => {
                    failed = true;
                    println!("{} {}: cannot run: {e}", p.id, p.version);
                }
            }
        }
        Ok(if failed {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        })
    }
    #[cfg(not(feature = "plugins"))]
    {
        println!("This build of Kalem has no plugin host (the `plugins` feature).");
        Ok(ExitCode::SUCCESS)
    }
}

/// `kalem plugin list`.
pub(crate) fn list() -> Result<ExitCode> {
    config();
    // The plugins built into this Kalem as components (wasm_todo W5).
    #[cfg(feature = "plugins")]
    for m in crate::embedded_components() {
        let (id, version) = (
            m["id"].as_str().unwrap_or_default(),
            m["version"].as_str().unwrap_or_default(),
        );
        println!(
            "{id} {version} — {} (built in)",
            m["name"].as_str().unwrap_or_default()
        );
        if let Some(note) = turned_off_note(id, version) {
            println!("  {note}");
        }
    }
    let all = plugin_store::installed();
    if all.is_empty() {
        println!("No plugins installed.");
    }
    for p in all {
        #[cfg(feature = "plugins")]
        let unused = crate::embedded_is_newer(&p);
        #[cfg(not(feature = "plugins"))]
        let unused: Option<String> = None;
        let from = p
            .source
            .clone()
            .unwrap_or_else(|| "installed by hand".into());
        println!(
            "{} {} — {} ({from})\n  {}",
            p.id,
            p.version,
            p.name,
            p.dir.display()
        );
        if let Some(why) = unused {
            println!("  {why}");
        }
        if let Some(note) = turned_off_note(&p.id, &p.version) {
            println!("  {note}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `kalem plugin enable ID`: a plugin Kalem turned off after it stopped
/// three times, turned on again (its stops forgotten).
pub(crate) fn enable(id: &str) -> Result<ExitCode> {
    config();
    if plugin_store::clear_stops(id) {
        println!("{}", kalem_core::tr!("plugin-enabled", id = id));
    } else {
        println!("{}", kalem_core::tr!("plugin-not-turned-off", id = id));
    }
    Ok(ExitCode::SUCCESS)
}

/// The note `kalem plugin list` gives plugin `id` at `version` when Kalem
/// turned it off.
fn turned_off_note(id: &str, version: &str) -> Option<String> {
    plugin_store::turned_off(id, version).then(|| {
        kalem_core::tr!(
            "plugin-turned-off-short",
            count = plugin_store::stops(id, version),
            id = id
        )
    })
}

/// `kalem plugin remove ID`.
pub(crate) fn remove(id: &str) -> Result<ExitCode> {
    config();
    let name = plugin_store::remove(id)?;
    println!("Removed {name}");
    Ok(ExitCode::SUCCESS)
}

/// `kalem plugin new NAME`.
pub(crate) fn new(name: &str) -> Result<ExitCode> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let dir = kalem_core::plugin_build::new(name, &cwd)?;
    println!("Started {name} in {}", dir.display());
    println!("Build it with `kalem plugin build {}`.", dir.display());
    Ok(ExitCode::SUCCESS)
}

/// `kalem plugin build [DIR]`.
pub(crate) fn build(dir: Option<&std::path::Path>) -> Result<ExitCode> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let dir = dir.map_or(cwd.clone(), |d| cwd.join(d));
    let b = kalem_core::plugin_build::build(&dir, false)?;
    println!("Built {} ({} kB)", b.path.display(), b.size.div_ceil(1024));
    let list = |v: &[String]| {
        if v.is_empty() {
            "nothing".to_string()
        } else {
            v.join(", ")
        }
    };
    println!("  imports {}", list(&b.imports));
    println!("  exports {}", list(&b.exports));
    Ok(ExitCode::SUCCESS)
}

/// `kalem plugin dev [DIR]`: the plugin built with its functions' names
/// kept and installed, then built and installed again whenever its
/// sources change, until stopped (wasm_todo W10). A Kalem running reads
/// the new build for the files it opens from then on; one started after
/// the first install registers the plugin.
pub(crate) fn dev(dir: Option<&std::path::Path>) -> Result<ExitCode> {
    let c = config();
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let dir = dir.map_or(cwd.clone(), |d| cwd.join(d));
    let source = dir.display().to_string();
    let mut seen = None;
    loop {
        let now = sources_changed(&dir);
        if seen != Some(now) {
            seen = Some(now);
            match kalem_core::plugin_build::build(&dir, true).and_then(|b| {
                let p = plugin_store::prepare(&source, &plugin_store::index_urls(&c))?;
                plugin_store::install(&p).map(|at| (b, p, at))
            }) {
                Ok((b, p, at)) => println!(
                    "Built {} ({} kB) and installed {} {} in {}",
                    b.path.display(),
                    b.size.div_ceil(1024),
                    p.name,
                    p.version,
                    at.display()
                ),
                Err(e) => eprintln!("{e}"),
            }
            println!("Watching {} (Ctrl+C stops)", dir.display());
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// When the plugin's sources in `dir` last changed: its manifest, Cargo
/// files and build script, and what is under `src` and `wit`.
fn sources_changed(dir: &std::path::Path) -> std::time::SystemTime {
    fn newest(path: &std::path::Path, out: &mut std::time::SystemTime) {
        let Ok(m) = std::fs::metadata(path) else {
            return;
        };
        if let Ok(t) = m.modified() {
            *out = (*out).max(t);
        }
        if m.is_dir()
            && let Ok(entries) = std::fs::read_dir(path)
        {
            for e in entries.flatten() {
                newest(&e.path(), out);
            }
        }
    }
    let mut t = std::time::UNIX_EPOCH;
    for p in ["plugin.json", "Cargo.toml", "build.rs", "src", "wit"] {
        newest(&dir.join(p), &mut t);
    }
    t
}
