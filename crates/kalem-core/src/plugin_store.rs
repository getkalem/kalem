//! Installing plugins from inside Kalem (design §11.8, T3.3.3): from the
//! index of `getkalem/plugins` by name, from a link, or from a folder or
//! archive on disk.
//!
//! Installing is two steps, so that nothing lands before the user has
//! seen what it is: [`prepare`] downloads and unpacks into a staging
//! folder and reads the manifest (on a background thread); the user is
//! shown the plugin's name, version, source and permissions; [`install`]
//! moves the folder into `CONFIG/plugins/ID` and records it in
//! `CONFIG/plugins.toml`, after which the plugin loads at once.
//!
//! Links Kalem understands:
//!
//! - a name or ID from the index (`elixir`, `org.kalem.elixir`);
//! - a GitHub folder, `https://github.com/OWNER/REPO/tree/REF/PATH`, or a
//!   repository whose root is the plugin, `https://github.com/OWNER/REPO`;
//! - an archive, `https://…/NAME.tar.gz` (or `.tgz`);
//! - a folder or a `.tar.gz` on disk.
//!
//! Declarative plugins (language plugins) install as they are. A plugin
//! with a WebAssembly component installs when it is a viewer (it says what
//! it `opens`) whose component is built (`kalem plugin build`): Kalem
//! loads component viewers today; other components wait for their part
//! of the API.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Where the index is read unless the setting `plugins.index` says
/// otherwise.
pub const DEFAULT_INDEX: &str =
    "https://raw.githubusercontent.com/getkalem/plugins/main/index.json";

/// The largest archive downloaded.
const MAX_DOWNLOAD: u64 = 64 << 20;

/// A plugin listed in the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    /// Its ID.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// What it does.
    pub description: String,
    /// The permissions it asks for.
    pub permissions: Vec<String>,
    /// Its source folder (a GitHub link).
    pub source: String,
    /// Its published archive or component, when released.
    pub download: Option<String>,
    /// The SHA-256 of `download`.
    pub sha256: Option<String>,
    /// `declarative` for a language plugin; a component otherwise.
    pub declarative: bool,
}

/// A plugin downloaded and unpacked, waiting for the user's yes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    /// Its ID.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// What it does.
    pub description: String,
    /// The permissions it asks for.
    pub permissions: Vec<String>,
    /// The languages it serves, by name.
    pub languages: Vec<String>,
    /// The language servers it may start, by name.
    pub servers: Vec<String>,
    /// The extensions it opens, for a viewer.
    pub opens: Vec<String>,
    /// It has a component (`main`): a viewer when it opens files, else an
    /// extension adding commands, keys and panels.
    pub component: bool,
    /// Where it came from, as the user gave it (for updates).
    pub source: String,
    /// The unpacked folder, holding `plugin.json`.
    pub staging: PathBuf,
    /// The version installed now, when this replaces it.
    pub replaces: Option<String>,
}

/// An installed plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// Its ID.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// Its folder.
    pub dir: PathBuf,
    /// Where it was installed from, when Kalem installed it.
    pub source: Option<String>,
}

/// Where Kalem installs plugins: `CONFIG/plugins`.
pub fn plugins_dir() -> Option<PathBuf> {
    crate::settings::config_dir().map(|d| d.join("plugins"))
}

fn record_path() -> Option<PathBuf> {
    crate::settings::config_dir().map(|d| d.join("plugins.toml"))
}

fn staging_root() -> PathBuf {
    crate::logging::state_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("plugin-staging")
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Downloads `url`, at most [`MAX_DOWNLOAD`] bytes.
pub fn fetch(url: &str) -> Result<Vec<u8>, String> {
    if let Some(rest) = url.strip_prefix("file://") {
        // A proper URI (`file:///C:/x`, percent-encoded), else the path
        // as written after the scheme (`file://C:\x`, `file:///tmp/x`).
        let path = kalem_lsp::uri::to_path(url)
            .filter(|p| p.exists())
            .unwrap_or_else(|| std::path::PathBuf::from(rest));
        return std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()));
    }
    let mut resp = ureq::get(url)
        .header("User-Agent", concat!("Kalem/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(404) => format!("{url}: not found"),
            e => format!("{url}: {e}"),
        })?;
    resp.body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD)
        .read_to_vec()
        .map_err(|e| format!("{url}: {e}"))
}

/// Reads an index.
pub fn parse_index(text: &str) -> Result<Vec<IndexEntry>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("the plugin index: {e}"))?;
    Ok(v["plugins"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|p| {
            Some(IndexEntry {
                id: p["id"].as_str()?.to_string(),
                name: p["name"].as_str().unwrap_or_default().to_string(),
                version: p["version"].as_str().unwrap_or_default().to_string(),
                description: p["description"].as_str().unwrap_or_default().to_string(),
                permissions: strings(&p["permissions"]),
                source: p["source"].as_str().unwrap_or_default().to_string(),
                download: p["download"].as_str().map(str::to_string),
                sha256: p["sha256"].as_str().map(str::to_string),
                declarative: p["kind"].as_str() == Some("declarative"),
            })
        })
        .collect())
}

/// The index at `url`.
pub fn fetch_index(url: &str) -> Result<Vec<IndexEntry>, String> {
    let bytes = fetch(url)?;
    parse_index(&String::from_utf8_lossy(&bytes))
}

/// The entry of the index a name stands for: its ID, its name, or the
/// last part of its ID (`elixir` for `org.kalem.elixir`), ignoring case.
pub fn find_entry<'a>(index: &'a [IndexEntry], name: &str) -> Option<&'a IndexEntry> {
    let n = name.trim().to_lowercase();
    index.iter().find(|e| {
        e.id.to_lowercase() == n
            || e.name.to_lowercase() == n
            || e.id
                .rsplit('.')
                .next()
                .is_some_and(|last| last.to_lowercase() == n)
    })
}

/// What a link points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A name or ID to look up in the index.
    Index(String),
    /// A folder of a GitHub repository at a reference (`HEAD` for the
    /// default branch).
    GitHub {
        /// The owner.
        owner: String,
        /// The repository.
        repo: String,
        /// The branch, tag or commit.
        reference: String,
        /// The folder in it (empty for the root).
        path: String,
    },
    /// An archive to download.
    Archive(String),
    /// A folder on disk.
    Dir(PathBuf),
    /// An archive on disk.
    File(PathBuf),
}

/// Reads what the user typed.
pub fn parse_source(s: &str) -> Source {
    let s = s.trim();
    let home = crate::settings::expand_home(s);
    let local = Path::new(&home);
    if local.is_dir() {
        return Source::Dir(local.to_path_buf());
    }
    if local.is_file() {
        return Source::File(local.to_path_buf());
    }
    let url = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    if let Some(rest) = url.strip_prefix("github.com/") {
        let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
        if parts.len() >= 2 && !(s.ends_with(".tar.gz") || s.ends_with(".tgz")) {
            let repo = parts[1].trim_end_matches(".git").to_string();
            let (reference, path) = match parts.get(2) {
                Some(&"tree") | Some(&"blob") if parts.len() >= 4 => {
                    (parts[3].to_string(), parts[4..].join("/"))
                }
                _ => ("HEAD".to_string(), String::new()),
            };
            return Source::GitHub {
                owner: parts[0].to_string(),
                repo,
                reference,
                path,
            };
        }
    }
    if s.contains("://") {
        return Source::Archive(s.to_string());
    }
    Source::Index(s.to_string())
}

/// Unpacks a gzipped tar into `into`: the entries under `under` (a
/// folder inside the archive, after its single top folder when it has
/// one), without links, absolute paths or `..`.
pub fn unpack(archive: &[u8], under: &str, into: &Path) -> Result<usize, String> {
    let bad = |e: std::io::Error| format!("the archive: {e}");
    // The top folder, when every entry is in the same one.
    let mut tops = std::collections::BTreeSet::new();
    let mut a = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for e in a.entries().map_err(bad)? {
        let e = e.map_err(bad)?;
        // GitHub's archives start with a pax header, not a file.
        if is_meta(&e) {
            continue;
        }
        if let Some(std::path::Component::Normal(c)) = e.path().map_err(bad)?.components().next() {
            tops.insert(c.to_os_string());
        }
    }
    let mut prefix = if tops.len() == 1 {
        tops.into_iter()
            .next()
            .map(PathBuf::from)
            .unwrap_or_default()
    } else {
        PathBuf::new()
    };
    if !under.is_empty() {
        prefix = prefix.join(under);
    }
    std::fs::create_dir_all(into).map_err(|e| e.to_string())?;
    let mut count = 0;
    let mut a = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for e in a.entries().map_err(bad)? {
        let mut e = e.map_err(bad)?;
        if is_meta(&e) {
            continue;
        }
        let path = e.path().map_err(bad)?.into_owned();
        let Ok(rel) = path.strip_prefix(&prefix) else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        if rel
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(format!(
                "the archive holds an unsafe path: {}",
                path.display()
            ));
        }
        let to = into.join(rel);
        match e.header().entry_type() {
            tar::EntryType::Directory => std::fs::create_dir_all(&to).map_err(|e| e.to_string())?,
            tar::EntryType::Regular | tar::EntryType::Continuous => {
                if let Some(d) = to.parent() {
                    std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                }
                let mut bytes = Vec::new();
                e.read_to_end(&mut bytes).map_err(bad)?;
                std::fs::write(&to, bytes).map_err(|e| format!("{}: {e}", to.display()))?;
                count += 1;
            }
            // Links and the rest are skipped: a plugin is plain files.
            _ => {}
        }
    }
    Ok(count)
}

/// A pax or GNU header entry, which describes others.
fn is_meta<R: Read>(e: &tar::Entry<'_, R>) -> bool {
    matches!(
        e.header().entry_type(),
        tar::EntryType::XGlobalHeader
            | tar::EntryType::XHeader
            | tar::EntryType::GNULongName
            | tar::EntryType::GNULongLink
    )
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let ty = e.file_type().map_err(|e| e.to_string())?;
        let name = e.file_name();
        // Build outputs and editor folders are not part of a plugin.
        if name == ".git" || name == "target" || name == "_build" || name == ".elixir_ls" {
            continue;
        }
        if ty.is_dir() {
            copy_dir(&e.path(), &to.join(&name))?;
        } else if ty.is_file() {
            std::fs::copy(e.path(), to.join(&name)).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn new_staging() -> Result<PathBuf, String> {
    let root = staging_root();
    std::fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = root.join(format!("{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

/// Downloads and unpacks the plugin `source` names into a staging
/// folder and reads its manifest. Blocking: run it off the editor's
/// thread.
pub fn prepare(source: &str, index_url: &str) -> Result<Prepared, String> {
    let staging = new_staging()?;
    let r = fill(source, index_url, &staging).and_then(|()| read_prepared(source, &staging));
    if r.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    r
}

fn fill(source: &str, index_url: &str, staging: &Path) -> Result<(), String> {
    match parse_source(source) {
        Source::Dir(d) => copy_dir(&d, staging),
        Source::File(f) => {
            let bytes = std::fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?;
            unpack(&bytes, "", staging).map(drop)
        }
        Source::Archive(url) => unpack(&fetch(&url)?, "", staging).map(drop),
        Source::GitHub {
            owner,
            repo,
            reference,
            path,
        } => {
            let url = format!("https://codeload.github.com/{owner}/{repo}/tar.gz/{reference}");
            let n = unpack(&fetch(&url)?, &path, staging)?;
            if n == 0 {
                return Err(format!(
                    "{owner}/{repo} has no folder {path} at {reference}"
                ));
            }
            Ok(())
        }
        Source::Index(name) => {
            let index = fetch_index(index_url)?;
            let e = find_entry(&index, &name).ok_or_else(|| {
                format!(
                    "No plugin named {name} in the index ({} plugins listed)",
                    index.len()
                )
            })?;
            match (&e.download, &e.sha256) {
                (Some(url), Some(sha)) => {
                    let bytes = fetch(url)?;
                    let got = format!("{:x}", Sha256::digest(&bytes));
                    if !got.eq_ignore_ascii_case(sha) {
                        return Err(format!("{url}: its SHA-256 is {got}, the index says {sha}"));
                    }
                    if !e.declarative {
                        return released_component(e, &bytes, staging);
                    }
                    unpack(&bytes, "", staging).map(drop)
                }
                // Not released yet: its source folder.
                _ if !e.declarative => Err(component_not_released(&e.name)),
                _ => fill(&e.source, index_url, staging),
            }
        }
    }
}

/// Puts the released component `bytes` of entry `e` (its hash checked)
/// in `staging` with its manifest: the source folder's `plugin.json` at
/// the release's tag, which must name the entry's plugin and version.
fn released_component(e: &IndexEntry, bytes: &[u8], staging: &Path) -> Result<(), String> {
    let text = released_manifest(e)?;
    let m: Value =
        serde_json::from_slice(&text).map_err(|err| format!("{}'s plugin.json: {err}", e.name))?;
    if m["id"].as_str() != Some(e.id.as_str()) || m["version"].as_str() != Some(e.version.as_str())
    {
        return Err(format!(
            "{}'s plugin.json at its release is not {} {}",
            e.name, e.id, e.version
        ));
    }
    let main = m["main"]
        .as_str()
        .ok_or_else(|| format!("{}'s plugin.json names no component", e.name))?;
    if main.starts_with('/') || main.contains("..") {
        return Err(format!(
            "{}'s component {main} is not in its folder",
            e.name
        ));
    }
    let file = staging.join(main);
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d).map_err(|err| err.to_string())?;
    }
    std::fs::write(&file, bytes).map_err(|err| format!("{}: {err}", file.display()))?;
    std::fs::write(staging.join("plugin.json"), &text).map_err(|err| err.to_string())
}

/// The manifest of entry `e`'s release: its source folder's `plugin.json`
/// at the tag the release workflow makes (`NAME-vVERSION`, the folder's
/// name), or in the folder on disk.
fn released_manifest(e: &IndexEntry) -> Result<Vec<u8>, String> {
    if let Some(url) = released_manifest_url(&e.source, &e.version) {
        return fetch(&url);
    }
    match parse_source(&e.source) {
        Source::Dir(d) => {
            std::fs::read(d.join("plugin.json")).map_err(|err| format!("{}: {err}", d.display()))
        }
        _ => Err(format!(
            "{}: its release has no manifest Kalem finds",
            e.name
        )),
    }
}

/// Where the manifest of version `version` of a plugin whose source is a
/// GitHub folder is: the folder at the release's tag.
fn released_manifest_url(source: &str, version: &str) -> Option<String> {
    let Source::GitHub {
        owner, repo, path, ..
    } = parse_source(source)
    else {
        return None;
    };
    let folder = path.rsplit('/').next().unwrap_or(&path);
    Some(format!(
        "https://raw.githubusercontent.com/{owner}/{repo}/{folder}-v{version}/{path}/plugin.json"
    ))
}

fn component_not_released(name: &str) -> String {
    format!(
        "{name} is a WebAssembly component with no release yet: build it from its source with `kalem plugin build` and install that folder"
    )
}

fn read_prepared(source: &str, staging: &Path) -> Result<Prepared, String> {
    let text = std::fs::read_to_string(staging.join("plugin.json"))
        .map_err(|_| format!("{source} is not a plugin: it has no plugin.json"))?;
    let m: Value = serde_json::from_str(&text).map_err(|e| format!("plugin.json: {e}"))?;
    let id = m["id"]
        .as_str()
        .ok_or("plugin.json has no `id`")?
        .to_string();
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        || id.starts_with('.')
    {
        return Err(format!("plugin.json: `{id}` is not a plugin ID"));
    }
    let name = m["name"].as_str().unwrap_or(&id).to_string();
    let opens = strings(&m["opens"]);
    let component = m.get("main").is_some();
    if let Some(main) = m.get("main").and_then(Value::as_str) {
        let file = staging.join(main);
        if main.starts_with('/') || main.contains("..") || !file.is_file() {
            return Err(format!(
                "{name}'s component {main} is not built: run `kalem plugin build` in its folder first"
            ));
        }
        let head = std::fs::read(&file)
            .map(|b| b.get(..8).map(<[u8]>::to_vec).unwrap_or_default())
            .unwrap_or_default();
        if !crate::plugin_build::is_component(&head) {
            return Err(format!(
                "{name}'s {main} is not a component: build it with `kalem plugin build`"
            ));
        }
    } else if m.get("languages").is_none() {
        return Err(format!(
            "{id} declares no languages: nothing Kalem can load today"
        ));
    }
    let replaces = installed()
        .into_iter()
        .find(|i| i.id == id)
        .map(|i| i.version);
    Ok(Prepared {
        component,
        name,
        version: m["version"].as_str().unwrap_or("").to_string(),
        description: m["description"].as_str().unwrap_or("").to_string(),
        permissions: strings(&m["permissions"]),
        languages: m["languages"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|l| {
                l["name"]
                    .as_str()
                    .or_else(|| l["id"].as_str())
                    .map(str::to_string)
            })
            .collect(),
        servers: m["servers"]
            .as_object()
            .map(|o| {
                o.iter()
                    .map(|(k, v)| v["name"].as_str().unwrap_or(k).to_string())
                    .collect()
            })
            .unwrap_or_default(),
        opens,
        source: source.trim().to_string(),
        staging: staging.to_path_buf(),
        replaces,
        id,
    })
}

/// The plugin unpacked in `staging` (by [`prepare`], from `source`).
pub fn prepared_at(staging: &Path, source: &str) -> Result<Prepared, String> {
    if !staging.starts_with(staging_root()) || !staging.is_dir() {
        return Err("That plugin is no longer waiting to be installed".into());
    }
    read_prepared(source, staging)
}

/// What the user is asked before installing: a line per fact.
pub fn summary(p: &Prepared) -> Vec<String> {
    let mut out = vec![match &p.replaces {
        Some(v) => format!("{} {} (replaces {v})", p.name, p.version),
        None => format!("{} {}", p.name, p.version),
    }];
    if !p.description.is_empty() {
        out.push(p.description.clone());
    }
    if !p.languages.is_empty() {
        out.push(format!("Languages: {}", p.languages.join(", ")));
    }
    if !p.opens.is_empty() {
        out.push(format!(
            "Opens: {} (a component, run in Kalem's sandbox)",
            p.opens.join(", ")
        ));
    } else if p.component {
        out.push("Adds commands, keys and panels (a component, run in Kalem's sandbox)".into());
    }
    if p.permissions.iter().any(|x| x == "subprocess") || !p.servers.is_empty() {
        out.push(format!(
            "Runs programs on this computer: {} (when installed; Kalem never installs them)",
            if p.servers.is_empty() {
                "its language servers".into()
            } else {
                p.servers.join(", ")
            }
        ));
    }
    let other: Vec<&String> = p
        .permissions
        .iter()
        .filter(|x| *x != "subprocess")
        .collect();
    if !other.is_empty() {
        out.push(format!(
            "Permissions: {}",
            other
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out.push(format!("From {}", p.source));
    out
}

/// Installs a prepared plugin: its folder moved to `CONFIG/plugins/ID`
/// (replacing an older one), recorded in `plugins.toml`, and the
/// language plugins loaded again.
pub fn install(p: &Prepared) -> Result<PathBuf, String> {
    let root = plugins_dir().ok_or("No settings folder to install into")?;
    std::fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    let dest = root.join(&p.id);
    let old = root.join(format!(".{}.old", p.id));
    let _ = std::fs::remove_dir_all(&old);
    if dest.exists() {
        std::fs::rename(&dest, &old).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    let moved = std::fs::rename(&p.staging, &dest).or_else(|_| {
        // Another file system: copy, then drop the staging folder.
        copy_dir(&p.staging, &dest)
            .map(|()| {
                let _ = std::fs::remove_dir_all(&p.staging);
            })
            .map_err(std::io::Error::other)
    });
    if let Err(e) = moved {
        if old.exists() {
            let _ = std::fs::rename(&old, &dest);
        }
        return Err(format!("{}: {e}", dest.display()));
    }
    let _ = std::fs::remove_dir_all(&old);
    record(p)?;
    crate::languages::reload();
    // Its servers start again with the plugin as it is now.
    crate::lsp::plugin_changed(&p.id);
    Ok(dest)
}

/// Throws a prepared plugin away.
pub fn discard(staging: &Path) {
    if staging.starts_with(staging_root()) {
        let _ = std::fs::remove_dir_all(staging);
    }
}

fn load_record() -> toml_edit::DocumentMut {
    record_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| t.parse().ok())
        .unwrap_or_default()
}

fn save_record(doc: &toml_edit::DocumentMut) -> Result<(), String> {
    let path = record_path().ok_or("No settings folder")?;
    std::fs::write(&path, doc.to_string()).map_err(|e| format!("{}: {e}", path.display()))
}

fn record(p: &Prepared) -> Result<(), String> {
    let mut doc = load_record();
    if !doc.contains_key("plugins") {
        let mut t = toml_edit::Table::new();
        t.set_implicit(true);
        doc["plugins"] = toml_edit::Item::Table(t);
    }
    let mut t = toml_edit::Table::new();
    t["version"] = toml_edit::value(p.version.as_str());
    t["source"] = toml_edit::value(p.source.as_str());
    let now = jiff::Timestamp::now().to_string();
    t["installed"] = toml_edit::value(&now[..10.min(now.len())]);
    let mut perms = toml_edit::Array::new();
    for x in &p.permissions {
        perms.push(x.as_str());
    }
    t["permissions"] = toml_edit::value(perms);
    doc["plugins"][p.id.as_str()] = toml_edit::Item::Table(t);
    save_record(&doc)
}

/// The plugins in `CONFIG/plugins`, with what `plugins.toml` says of
/// those Kalem installed.
pub fn installed() -> Vec<Installed> {
    let Some(root) = plugins_dir() else {
        return Vec::new();
    };
    let rec = load_record();
    let Ok(rd) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out: Vec<Installed> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|d| {
            !d.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        })
        .filter_map(|dir| {
            let text = std::fs::read_to_string(dir.join("plugin.json")).ok()?;
            let m: Value = serde_json::from_str(&text).ok()?;
            let id = m["id"].as_str()?.to_string();
            Some(Installed {
                name: m["name"].as_str().unwrap_or(&id).to_string(),
                version: m["version"].as_str().unwrap_or("").to_string(),
                source: rec
                    .get("plugins")
                    .and_then(|p| p.get(&id))
                    .and_then(|t| t.get("source"))
                    .and_then(|s| s.as_str())
                    .map(str::to_string),
                dir,
                id,
            })
        })
        .collect();
    out.sort_by_key(|p| p.name.to_lowercase());
    out
}

/// Removes an installed plugin: its folder and its record. A plugin
/// folder Kalem did not install (a link made by hand) is unlinked, and
/// what it points at is left alone.
pub fn remove(id: &str) -> Result<String, String> {
    let p = installed()
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("{id} is not installed"))?;
    let meta = std::fs::symlink_metadata(&p.dir).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() {
        std::fs::remove_file(&p.dir)
            .or_else(|_| std::fs::remove_dir(&p.dir))
            .map_err(|e| format!("{}: {e}", p.dir.display()))?;
    } else {
        std::fs::remove_dir_all(&p.dir).map_err(|e| format!("{}: {e}", p.dir.display()))?;
    }
    let mut doc = load_record();
    if let Some(t) = doc.get_mut("plugins").and_then(|t| t.as_table_mut()) {
        t.remove(id);
        let _ = save_record(&doc);
    }
    crate::languages::reload();
    crate::lsp::plugin_changed(id);
    Ok(p.name)
}

/// The versions the index listed when it was last read, by plugin ID.
static LATEST: std::sync::Mutex<Option<std::collections::HashMap<String, String>>> =
    std::sync::Mutex::new(None);

/// Whether version `a` is newer than `b`: dotted numbers compared as
/// numbers (`0.10.0` after `0.9.1`), anything else as text.
pub fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Option<Vec<u64>> {
        v.trim_start_matches('v')
            .split(['.', '-', '+'])
            .take(3)
            .map(|p| p.parse().ok())
            .collect()
    };
    match (parts(a), parts(b)) {
        (Some(x), Some(y)) => x > y,
        _ => a != b && a > b,
    }
}

/// The installed plugins the index has a newer version of: the plugin
/// and that version.
pub fn updates(index: &[IndexEntry]) -> Vec<(Installed, String)> {
    let mut latest = std::collections::HashMap::new();
    for e in index {
        latest.insert(e.id.clone(), e.version.clone());
    }
    if let Ok(mut l) = LATEST.lock() {
        *l = Some(latest);
    }
    installed()
        .into_iter()
        .filter_map(|i| {
            let e = index.iter().find(|e| e.id == i.id && e.declarative)?;
            newer(&e.version, &i.version).then(|| (i, e.version.clone()))
        })
        .collect()
}

/// The newer version of plugin `id` the index listed when last read.
pub fn available(id: &str, installed: &str) -> Option<String> {
    let l = LATEST.lock().ok()?;
    let v = l.as_ref()?.get(id)?;
    newer(v, installed).then(|| v.clone())
}

/// What the status bar says about `updates`.
pub fn updates_notice(updates: &[(Installed, String)]) -> Option<String> {
    if updates.is_empty() {
        return None;
    }
    let list: Vec<String> = updates
        .iter()
        .map(|(i, v)| format!("{} {v}", i.name))
        .collect();
    Some(format!(
        "Plugin updates: {} (Kalem menu, Installed Plugins)",
        list.join(", ")
    ))
}

/// How often the editor looks for updates.
const CHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Looks for updates of the installed plugins in the background, at most
/// once a day (the time kept in the state folder), unless
/// `plugins.check_updates` is off; a newer version is said in the status
/// bar ([`crate::jobs::notice`]). Called when an editor starts.
pub fn check_updates(config: &crate::Config) {
    if !config.bool("plugins.check_updates") || installed().is_empty() {
        return;
    }
    let stamp = crate::logging::state_dir().map(|d| d.join("plugin-update-check"));
    let due = stamp
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .is_none_or(|age| age >= CHECK_EVERY);
    if !due {
        return;
    }
    let index = match config.str("plugins.index") {
        "" => DEFAULT_INDEX.to_string(),
        s => s.to_string(),
    };
    std::thread::spawn(move || {
        let Ok(entries) = fetch_index(&index) else {
            // Offline: tried again at the next start.
            return;
        };
        if let Some(p) = &stamp {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, "");
        }
        if let Some(n) = updates_notice(&updates(&entries)) {
            crate::jobs::notice(n, false);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(newer("0.10.0", "0.9.1"));
        assert!(newer("1.0.0", "0.99"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0", "0.2.0"));
        assert!(newer("v2.0.0", "1.9.9"));
    }

    #[test]
    fn sources() {
        assert_eq!(
            parse_source("https://github.com/getkalem/plugins/tree/main/plugins/elixir"),
            Source::GitHub {
                owner: "getkalem".into(),
                repo: "plugins".into(),
                reference: "main".into(),
                path: "plugins/elixir".into()
            }
        );
        assert_eq!(
            parse_source("github.com/someone/kalem-zig.git"),
            Source::GitHub {
                owner: "someone".into(),
                repo: "kalem-zig".into(),
                reference: "HEAD".into(),
                path: String::new()
            }
        );
        assert_eq!(
            parse_source("https://example.org/zig-v1.tar.gz"),
            Source::Archive("https://example.org/zig-v1.tar.gz".into())
        );
        assert_eq!(parse_source("elixir"), Source::Index("elixir".into()));
    }

    #[test]
    fn index_names() {
        let index = parse_index(
            r#"{"schema":1,"plugins":[{"id":"org.kalem.elixir","name":"Elixir","version":"0.1.0","api":"^0.1",
               "source":"https://github.com/getkalem/plugins/tree/main/plugins/elixir","download":null,"sha256":null,"kind":"declarative"},
               {"id":"org.kalem.xlsx","name":"Excel workbooks","version":"0.0.1","api":"^0.1","source":"x","download":null,"sha256":null}]}"#,
        )
        .unwrap();
        assert!(index[0].declarative && !index[1].declarative);
        for n in ["elixir", "Elixir", "org.kalem.elixir"] {
            assert_eq!(find_entry(&index, n).unwrap().id, "org.kalem.elixir");
        }
        assert!(find_entry(&index, "python").is_none());
    }

    /// A gzipped tar of `files` under `top/`.
    fn tarball(top: &str, files: &[(&str, &str)]) -> Vec<u8> {
        let mut b = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        // As GitHub's archives: a pax header first, beside the top folder.
        let comment = "52 comment=0123456789012345678901234567890123456789\n";
        let mut h = tar::Header::new_ustar();
        h.set_entry_type(tar::EntryType::XGlobalHeader);
        h.set_size(comment.len() as u64);
        h.set_cksum();
        b.append_data(&mut h, "pax_global_header", comment.as_bytes())
            .unwrap();
        for (name, text) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(text.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, format!("{top}/{name}"), text.as_bytes())
                .unwrap();
        }
        b.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn unpacked_from_a_folder_of_an_archive() {
        let bytes = tarball(
            "plugins-main",
            &[
                ("plugins/elixir/plugin.json", "{}"),
                ("plugins/elixir/syntaxes/a.sublime-syntax", "x"),
                ("plugins/other/plugin.json", "{}"),
            ],
        );
        let dir = std::env::temp_dir().join(format!("kalem-unpack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(unpack(&bytes, "plugins/elixir", &dir).unwrap(), 2);
        assert!(dir.join("syntaxes/a.sublime-syntax").is_file());
        assert!(!dir.join("plugins").exists());
        assert_eq!(unpack(&bytes, "plugins/none", &dir.join("n")).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn components_and_manifests_read() {
        let dir = std::env::temp_dir().join(format!("kalem-prep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dist")).unwrap();
        // An extension whose component is not built.
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"id":"org.x.wasm","name":"W","main":"dist/w.wasm"}"#,
        )
        .unwrap();
        assert!(
            read_prepared("x", &dir)
                .unwrap_err()
                .contains("kalem plugin build")
        );
        // A viewer whose component is not built.
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"id":"org.x.view","name":"V","main":"dist/v.wasm","opens":[".v"]}"#,
        )
        .unwrap();
        assert!(
            read_prepared("x", &dir)
                .unwrap_err()
                .contains("kalem plugin build")
        );
        // Built, but a core module rather than a component.
        std::fs::write(dir.join("dist/v.wasm"), b"\0asm\x01\0\0\0").unwrap();
        assert!(
            read_prepared("x", &dir)
                .unwrap_err()
                .contains("not a component")
        );
        // A component: installed, saying what it opens.
        std::fs::write(dir.join("dist/v.wasm"), b"\0asm\x0d\0\x01\0").unwrap();
        let p = read_prepared("x", &dir).unwrap();
        assert_eq!(p.opens, [".v"]);
        assert!(summary(&p).iter().any(|l| l.starts_with("Opens: .v")));
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"id":"org.x.lang","name":"L","version":"1","permissions":["subprocess"],
                "languages":[{"id":"l","name":"Lang"}],"servers":{"s":{"name":"LangLS"}}}"#,
        )
        .unwrap();
        let p = read_prepared("x", &dir).unwrap();
        assert_eq!(
            (p.languages.clone(), p.servers.clone()),
            (vec!["Lang".into()], vec!["LangLS".into()])
        );
        assert!(
            summary(&p)
                .iter()
                .any(|l| l.contains("Runs programs") && l.contains("LangLS"))
        );
        std::fs::write(dir.join("plugin.json"), r#"{"id":"../x","languages":[]}"#).unwrap();
        assert!(read_prepared("x", &dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_released_component_installs_from_the_index() {
        let dir = std::env::temp_dir().join(format!("kalem-release-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let source = dir.join("plugins/counter");
        std::fs::create_dir_all(&source).unwrap();
        let manifest = |version: &str| {
            format!(
                r#"{{"id":"org.x.counter","name":"Counter","version":"{version}","main":"dist/counter.wasm"}}"#
            )
        };
        std::fs::write(source.join("plugin.json"), manifest("0.2.0")).unwrap();
        // The release's asset: a component (its header), alone.
        let component = b"\0asm\x0d\x00\x01\x00rest".to_vec();
        let asset = dir.join("counter.wasm");
        std::fs::write(&asset, &component).unwrap();
        let sha = format!("{:x}", Sha256::digest(&component));
        let index = |version: &str, sha: &str| {
            serde_json::json!({ "schema": 1, "plugins": [{
                "id": "org.x.counter", "name": "Counter", "version": version,
                "api": "^0.1", "permissions": [],
                "source": source.to_string_lossy(),
                "download": format!("file://{}", asset.display()),
                "sha256": sha,
            }]})
            .to_string()
        };
        let at = dir.join("index.json");
        let url = format!("file://{}", at.display());

        std::fs::write(&at, index("0.2.0", &sha)).unwrap();
        let p = prepare("counter", &url).unwrap();
        assert!(p.component && p.opens.is_empty());
        assert_eq!(
            (p.id.as_str(), p.version.as_str()),
            ("org.x.counter", "0.2.0")
        );
        assert_eq!(
            std::fs::read(p.staging.join("dist/counter.wasm")).unwrap(),
            component
        );
        assert!(summary(&p).iter().any(|l| l.starts_with("Adds commands")));
        let _ = std::fs::remove_dir_all(&p.staging);

        // The manifest at the release must be the entry's version.
        std::fs::write(&at, index("0.1.0", &sha)).unwrap();
        assert!(
            prepare("counter", &url)
                .unwrap_err()
                .contains("not org.x.counter 0.1.0")
        );
        // And the asset the hash the index gives.
        std::fs::write(&at, index("0.2.0", &"0".repeat(64))).unwrap();
        assert!(prepare("counter", &url).unwrap_err().contains("SHA-256"));
        let _ = std::fs::remove_dir_all(&dir);
        // getkalem/plugins: the folder at the tag its release workflow makes.
        assert_eq!(
            released_manifest_url(
                "https://github.com/getkalem/plugins/tree/main/plugins/pdf-viewer",
                "0.1.0"
            )
            .as_deref(),
            Some(
                "https://raw.githubusercontent.com/getkalem/plugins/pdf-viewer-v0.1.0/plugins/pdf-viewer/plugin.json"
            )
        );
    }
}
