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

fn index_url(c: &Config) -> String {
    match c.str("plugins.index") {
        "" => plugin_store::DEFAULT_INDEX.to_string(),
        s => s.to_string(),
    }
}

/// `kalem plugin browse`.
pub(crate) fn browse() -> Result<ExitCode> {
    let c = config();
    let installed = plugin_store::installed();
    for e in plugin_store::fetch_index(&index_url(&c))? {
        let state = match installed.iter().find(|i| i.id == e.id) {
            Some(i) => format!(" (installed {})", i.version),
            None if !e.declarative => {
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
    let p = plugin_store::prepare(source, &index_url(&c))?;
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

/// `kalem plugin list`.
pub(crate) fn list() -> Result<ExitCode> {
    config();
    let all = plugin_store::installed();
    if all.is_empty() {
        println!("No plugins installed.");
    }
    for p in all {
        let from = p.source.unwrap_or_else(|| "installed by hand".into());
        println!(
            "{} {} — {} ({from})\n  {}",
            p.id,
            p.version,
            p.name,
            p.dir.display()
        );
    }
    Ok(ExitCode::SUCCESS)
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
    let b = kalem_core::plugin_build::build(&dir)?;
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
