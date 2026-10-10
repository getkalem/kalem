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

/// The most an archive may unpack to.
const MAX_UNPACKED: u64 = 256 << 20;

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
    /// The extensions it opens (`.xlsx`), for a viewer: what names it for
    /// a file Kalem cannot open yet (T3.7.9).
    pub opens: Vec<String>,
    /// The index it is listed in.
    pub index: String,
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
            ureq::Error::StatusCode(404) => crate::tr!("plugin-url-not-found", url = url),
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
                opens: strings(&p["opens"]),
                index: String::new(),
            })
        })
        .collect())
}

/// The released plugins of the index that open the file at `path`, by its
/// extension (`.xlsx`, `.tar.gz` too), in the index's order: what Kalem
/// offers to install for a file that it cannot open yet (T3.7.9).
pub fn opening<'a>(entries: &'a [IndexEntry], path: &std::path::Path) -> Vec<&'a IndexEntry> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    entries
        .iter()
        .filter(|e| !e.declarative && e.download.is_some())
        .filter(|e| {
            e.opens.iter().any(|x| {
                let x = x.trim().to_lowercase();
                let x = if x.starts_with('.') {
                    x
                } else {
                    format!(".{x}")
                };
                name.len() > x.len() && name.ends_with(&x)
            })
        })
        .collect()
}

/// The index at `url`, its entries' sources and downloads written
/// relative to it resolved (so that a fork of an index lists its own
/// plugins without rewriting their links).
pub fn fetch_index(url: &str) -> Result<Vec<IndexEntry>, String> {
    let bytes = fetch(url)?;
    let mut entries = parse_index(&String::from_utf8_lossy(&bytes))?;
    for e in &mut entries {
        e.source = resolve_source(url, &e.source);
        e.download = e.download.take().map(|d| resolve_file(url, &d));
        e.index = url.to_string();
    }
    Ok(entries)
}

/// The indexes plugins are listed in: the user's own (`plugins.sources`:
/// a fork of the official one, or one of their own), then the official
/// one (`plugins.index`). A plugin in an earlier one stands for the same
/// ID in a later one.
pub fn index_urls(config: &crate::Config) -> Vec<String> {
    let mut urls: Vec<String> = config
        .strings("plugins.sources")
        .into_iter()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let main = match config.str("plugins.index").trim() {
        "" => DEFAULT_INDEX.to_string(),
        s => s.to_string(),
    };
    if !urls.contains(&main) {
        urls.push(main);
    }
    urls
}

/// The plugins of the indexes at `urls` together, a plugin of an earlier
/// index standing for the same ID in a later one. An index that cannot be
/// read is left out, unless none can.
pub fn fetch_indexes(urls: &[String]) -> Result<Vec<IndexEntry>, String> {
    let mut out: Vec<IndexEntry> = Vec::new();
    let mut errors = Vec::new();
    for url in urls {
        match fetch_index(url) {
            Ok(entries) => {
                for e in entries {
                    if !out.iter().any(|o| o.id == e.id) {
                        out.push(e);
                    }
                }
            }
            Err(e) => errors.push(e),
        }
    }
    if out.is_empty()
        && let Some(e) = errors.into_iter().next()
    {
        return Err(e);
    }
    Ok(out)
}

/// The folder an index's entry names, `rel` resolved against the index's
/// address `base`: a link stays as it is; a relative path in an index on
/// GitHub is a folder of that repository at the same reference, else a
/// path or address beside the index.
pub fn resolve_source(base: &str, rel: &str) -> String {
    let rel = rel.trim();
    if rel.is_empty() || is_absolute(rel) {
        return rel.to_string();
    }
    if let Some((owner, repo, reference, dir)) = github_file(base) {
        let path = join(&dir, rel);
        return format!("https://github.com/{owner}/{repo}/tree/{reference}/{path}");
    }
    // An index on disk: a folder beside it.
    if let Some(path) = base.strip_prefix("file://") {
        let dir = Path::new(path).parent().unwrap_or(Path::new(""));
        return dir.join(rel).to_string_lossy().into_owned();
    }
    resolve_file(base, rel)
}

/// A file an index's entry names (a download), `rel` resolved against the
/// index's address `base`.
pub fn resolve_file(base: &str, rel: &str) -> String {
    let rel = rel.trim();
    if rel.is_empty() || is_absolute(rel) {
        return rel.to_string();
    }
    match base.rfind('/') {
        Some(i) if base.contains("://") => {
            let (scheme_host, _) = base.split_at(i);
            // The address's folder, `..` and `.` taken.
            let (root, dir) = match scheme_host.find("://").map(|k| k + 3) {
                Some(k) => match scheme_host[k..].find('/') {
                    Some(j) => (&scheme_host[..k + j], &scheme_host[k + j + 1..]),
                    None => (scheme_host, ""),
                },
                None => (scheme_host, ""),
            };
            format!("{root}/{}", join(dir, rel))
        }
        _ => {
            let dir = Path::new(base).parent().unwrap_or(Path::new(""));
            dir.join(rel).to_string_lossy().into_owned()
        }
    }
}

fn is_absolute(s: &str) -> bool {
    s.contains("://")
        || s.starts_with("github.com/")
        || s.starts_with('/')
        || s.starts_with('~')
        || Path::new(s).is_absolute()
}

/// The owner, repository, reference and folder of a file on GitHub
/// (`raw.githubusercontent.com/O/R/REF/dir/f` or
/// `github.com/O/R/blob/REF/dir/f`).
fn github_file(url: &str) -> Option<(String, String, String, String)> {
    let u = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let parts: Vec<&str> = u.split('/').collect();
    let (owner, repo, reference, rest) = match *parts.first()? {
        "raw.githubusercontent.com" if parts.len() >= 5 => {
            (parts[1], parts[2], parts[3], &parts[4..])
        }
        "github.com" if parts.len() >= 6 && matches!(parts[3], "blob" | "raw") => {
            (parts[1], parts[2], parts[4], &parts[5..])
        }
        _ => return None,
    };
    let dir = rest[..rest.len().saturating_sub(1)].join("/");
    Some((owner.into(), repo.into(), reference.into(), dir))
}

/// `rel` under the folder `dir` (both with `/`), `.` and `..` taken.
fn join(dir: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for p in rel.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
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
        .or_else(|| {
            s.strip_prefix("git@github.com:")
                .map(|_| &s["git@".len()..])
        })
        .unwrap_or(s);
    // `git@github.com:owner/repo.git`, as GitHub's Clone button gives it.
    let url = url.replacen("github.com:", "github.com/", 1);
    // `owner/repo`: an index's names have no slash.
    let short = format!("github.com/{url}");
    let url = if is_owner_repo(&url) { &short } else { &url };
    if let Some(rest) = url.strip_prefix("github.com/") {
        let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
        if parts.len() >= 2 && !(s.ends_with(".tar.gz") || s.ends_with(".tgz")) {
            let repo = parts[1].trim_end_matches(".git").to_string();
            let (reference, path) = match parts.get(2) {
                Some(&"tree") | Some(&"blob") if parts.len() >= 4 => {
                    // A link to its manifest names its folder.
                    let path = parts[4..].join("/");
                    let path = path
                        .strip_suffix("plugin.json")
                        .map_or(path.clone(), |p| p.trim_end_matches('/').to_string());
                    (parts[3].to_string(), path)
                }
                // A release's page: the repository at its tag.
                Some(&"releases") if parts.get(3) == Some(&"tag") && parts.len() >= 5 => {
                    (parts[4].to_string(), String::new())
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
    // A small archive can unpack to a great deal: the total is capped.
    let mut total: u64 = 0;
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
                total = total.saturating_add(e.header().size().unwrap_or(0));
                if total > MAX_UNPACKED {
                    return Err(format!(
                        "the archive unpacks to more than {MAX_UNPACKED} bytes"
                    ));
                }
                let mut bytes = Vec::new();
                (&mut e)
                    .take(MAX_UNPACKED)
                    .read_to_end(&mut bytes)
                    .map_err(bad)?;
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
    // Plugins never confirmed (Kalem quit while asking): gone after a day.
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.filter_map(Result::ok) {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > std::time::Duration::from_secs(24 * 60 * 60));
            if old {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
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
pub fn prepare(source: &str, index_urls: &[String]) -> Result<Prepared, String> {
    let staging = new_staging()?;
    let r = fill(source, index_urls, &staging).and_then(|()| read_prepared(source, &staging));
    if r.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    r
}

fn fill(source: &str, index_urls: &[String], staging: &Path) -> Result<(), String> {
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
                return Err(crate::tr!(
                    "plugin-no-folder",
                    repo = format!("{owner}/{repo}"),
                    path = path.as_str(),
                    reference = reference.as_str()
                ));
            }
            // A component plugin's source: its build from a release.
            released_asset(&owner, &repo, &reference, &path, staging)
        }
        Source::Index(name) => {
            let index = fetch_indexes(index_urls)?;
            let e = find_entry(&index, &name).ok_or_else(|| {
                crate::tr!("plugin-no-such", name = name.as_str(), count = index.len())
            })?;
            match (&e.download, &e.sha256) {
                (Some(url), Some(sha)) => {
                    let bytes = fetch(url)?;
                    let got = format!("{:x}", Sha256::digest(&bytes));
                    if !got.eq_ignore_ascii_case(sha) {
                        return Err(crate::tr!(
                            "plugin-bad-sha",
                            url = url.as_str(),
                            got = got.as_str(),
                            want = sha.as_str()
                        ));
                    }
                    if !e.declarative {
                        return released_component(e, &bytes, staging);
                    }
                    unpack(&bytes, "", staging).map(drop)
                }
                // Not released yet: its source folder.
                _ if !e.declarative => Err(component_not_released(&e.name)),
                _ => fill(&e.source, index_urls, staging),
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
    if Path::new(main).has_root() || Path::new(main).is_absolute() || main.contains("..") {
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
        .map_err(|_| crate::tr!("plugin-no-manifest", source = source))?;
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
        return Err(crate::tr!("plugin-bad-id", id = id.as_str()));
    }
    let name = m["name"].as_str().unwrap_or(&id).to_string();
    let opens = strings(&m["opens"]);
    let component = m.get("main").is_some();
    if let Some(main) = m.get("main").and_then(Value::as_str) {
        let file = staging.join(main);
        if Path::new(main).has_root()
            || Path::new(main).is_absolute()
            || main.contains("..")
            || !file.is_file()
        {
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
        return Err(crate::tr!("plugin-nothing-to-load", id = id.as_str()));
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
        return Err(crate::tr!("plugin-not-waiting"));
    }
    read_prepared(source, staging)
}

/// What the user is asked before installing: a line per fact.
pub fn summary(p: &Prepared) -> Vec<String> {
    let mut out = vec![match &p.replaces {
        Some(v) => crate::tr!(
            "plugin-replaces",
            name = p.name.as_str(),
            version = p.version.as_str(),
            old = v.as_str()
        ),
        None => format!("{} {}", p.name, p.version),
    }];
    if !p.description.is_empty() {
        out.push(p.description.clone());
    }
    if !p.languages.is_empty() {
        out.push(crate::tr!(
            "plugin-languages",
            list = p.languages.join(", ")
        ));
    }
    if !p.opens.is_empty() {
        out.push(crate::tr!("plugin-opens", list = p.opens.join(", ")));
    } else if p.component {
        out.push(crate::tr!("plugin-adds"));
    }
    // `subprocess` runs a language plugin's servers; `subprocess:NAME`, the
    // program NAME through the `process` interface (API 0.2.4).
    let named: Vec<&str> = p
        .permissions
        .iter()
        .filter_map(|x| x.strip_prefix("subprocess:"))
        .collect();
    let servers = p.permissions.iter().any(|x| x == "subprocess") || !p.servers.is_empty();
    if servers || !named.is_empty() {
        let mut programs: Vec<String> = named.iter().map(|n| n.to_string()).collect();
        if servers {
            programs.push(if p.servers.is_empty() {
                crate::tr!("plugin-its-servers")
            } else {
                p.servers.join(", ")
            });
        }
        out.push(crate::tr!("plugin-runs", list = programs.join(", ")));
    }
    let other: Vec<&String> = p
        .permissions
        .iter()
        .filter(|x| *x != "subprocess" && !x.starts_with("subprocess:"))
        .collect();
    if !other.is_empty() {
        let list = other
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        out.push(crate::tr!("plugin-permissions", list = list));
    }
    out.push(crate::tr!("plugin-from", source = p.source.as_str()));
    out
}

/// Installs a prepared plugin: its folder moved to `CONFIG/plugins/ID`
/// (replacing an older one), recorded in `plugins.toml`, and the
/// language plugins loaded again.
pub fn install(p: &Prepared) -> Result<PathBuf, String> {
    let root = plugins_dir().ok_or_else(|| crate::tr!("plugin-no-config"))?;
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

/// How many times a plugin of one version may stop (a trap, its time or
/// its memory spent) before Kalem turns it off until it is updated
/// (wasm_todo W8, T3.1.13).
pub const STOPS_TO_TURN_OFF: u32 = 3;

/// Where the plugins' stops are counted: `STATE/plugin-failures.json`.
fn failures_path() -> Option<PathBuf> {
    crate::logging::state_dir().map(|d| d.join("plugin-failures.json"))
}

fn load_failures(path: &Path) -> serde_json::Map<String, Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn stops_in(path: &Path, id: &str, version: &str) -> u32 {
    let all = load_failures(path);
    let Some(e) = all.get(id) else { return 0 };
    if e["version"].as_str() != Some(version) {
        return 0;
    }
    e["count"].as_u64().map_or(0, |n| n as u32)
}

fn record_stop_in(path: &Path, id: &str, version: &str, why: &str) -> u32 {
    let mut all = load_failures(path);
    let count = stops_in(path, id, version) + 1;
    all.insert(
        id.to_string(),
        serde_json::json!({ "version": version, "count": count, "last": why }),
    );
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, Value::Object(all).to_string());
    count
}

fn clear_stops_in(path: &Path, id: &str) -> bool {
    let mut all = load_failures(path);
    let had = all.remove(id).is_some();
    if had {
        let _ = std::fs::write(path, Value::Object(all).to_string());
    }
    had
}

/// How many times plugin `id` at `version` stopped (another version's
/// stops do not count).
pub fn stops(id: &str, version: &str) -> u32 {
    failures_path().map_or(0, |p| stops_in(&p, id, version))
}

/// Counts a stop of plugin `id` at `version`, `why` kept for the user:
/// how many it has now.
pub fn record_stop(id: &str, version: &str, why: &str) -> u32 {
    failures_path().map_or(0, |p| record_stop_in(&p, id, version, why))
}

/// Whether Kalem turned plugin `id` at `version` off: it stopped
/// [`STOPS_TO_TURN_OFF`] times. An update turns it on again.
pub fn turned_off(id: &str, version: &str) -> bool {
    stops(id, version) >= STOPS_TO_TURN_OFF
}

/// Forgets plugin `id`'s stops, turning it on again (`kalem plugin
/// enable`): whether there were any.
pub fn clear_stops(id: &str) -> bool {
    failures_path().is_some_and(|p| clear_stops_in(&p, id))
}

/// What changes when a plugin is installed, updated or removed, or turned
/// off or on again, by this Kalem or by another (`kalem plugin install`
/// in a terminal): the times and sizes of `plugins.toml`, of the plugins'
/// folder and of the stops' file. A running Kalem compares it to choose
/// its viewers again without a restart.
pub fn stamp() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for path in [record_path(), plugins_dir(), failures_path()] {
        let meta = path.and_then(|p| std::fs::metadata(p).ok());
        meta.as_ref().and_then(|m| m.modified().ok()).hash(&mut h);
        meta.map(|m| m.len()).hash(&mut h);
    }
    h.finish()
}

fn load_record() -> toml_edit::DocumentMut {
    record_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| t.parse().ok())
        .unwrap_or_default()
}

/// Writes `plugins.toml` through a temporary file; a file there that is
/// not TOML (which [`load_record`] read as empty) is kept aside as
/// `plugins.toml.broken` first, and one that cannot be read is not
/// written over.
fn save_record(doc: &toml_edit::DocumentMut) -> Result<(), String> {
    let path = record_path().ok_or("No settings folder")?;
    save_record_at(&path, doc)
}

fn save_record_at(path: &Path, doc: &toml_edit::DocumentMut) -> Result<(), String> {
    let e = |err: std::io::Error| format!("{}: {err}", path.display());
    match std::fs::read_to_string(path) {
        Ok(text) if text.parse::<toml_edit::DocumentMut>().is_err() => {
            let aside = path.with_extension("toml.broken");
            if !aside.exists() {
                std::fs::write(&aside, &text).map_err(e)?;
            }
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(e(err)),
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(e)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, doc.to_string()).map_err(e)?;
    std::fs::rename(&tmp, path).map_err(e)
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

/// Whether `s` is `owner/repo`, GitHub's short name of a repository (an
/// owner's name has letters, digits and hyphens only).
fn is_owner_repo(s: &str) -> bool {
    let owner = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let repo = |p: &str| {
        !p.is_empty()
            && !p.starts_with('.')
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    matches!(s.split_once('/'), Some((o, r)) if owner(o) && repo(r.trim_end_matches(".git")))
}

/// The GitHub link `s` as Kalem installs from it
/// (`https://github.com/OWNER/REPO[/tree/REF/PATH]`), when it is one: a
/// repository's page or address, `owner/repo`, a folder or a manifest in
/// it, or a release's page. `None` for anything else.
pub fn github_link(s: &str) -> Option<String> {
    let s = s.trim();
    if s.contains(char::is_whitespace) {
        return None;
    }
    match parse_source(s) {
        Source::GitHub {
            owner,
            repo,
            reference,
            path,
        } => Some(if reference == "HEAD" && path.is_empty() {
            format!("https://github.com/{owner}/{repo}")
        } else {
            format!("https://github.com/{owner}/{repo}/tree/{reference}/{path}")
                .trim_end_matches('/')
                .to_string()
        }),
        _ => None,
    }
}

/// The component of a plugin whose GitHub folder has its manifest but
/// not the built component (`dist/` is not committed): the asset named
/// as `main`'s file in one of the repository's releases, written at
/// `main` in `staging`. The release tagged `reference` first, then one
/// tagged with the manifest's version (`v1.2.0`, `1.2.0` or
/// `FOLDER-v1.2.0`), then the newest.
fn released_asset(
    owner: &str,
    repo: &str,
    reference: &str,
    path: &str,
    staging: &Path,
) -> Result<(), String> {
    let Ok(text) = std::fs::read_to_string(staging.join("plugin.json")) else {
        return Ok(());
    };
    let Ok(m) = serde_json::from_str::<Value>(&text) else {
        return Ok(());
    };
    let Some(main) = m["main"].as_str() else {
        return Ok(());
    };
    if staging.join(main).is_file() || main.contains("..") || Path::new(main).has_root() {
        return Ok(());
    }
    let file = main.rsplit('/').next().unwrap_or(main);
    let name = m["name"].as_str().unwrap_or(file);
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases?per_page=50");
    let releases: Value = serde_json::from_slice(&fetch(&url)?)
        .map_err(|e| format!("{owner}/{repo}'s releases: {e}"))?;
    let folder = path.rsplit('/').next().unwrap_or(path);
    let version = m["version"].as_str().unwrap_or_default();
    let Some(asset) = pick_release_asset(&releases, file, reference, version, folder) else {
        return Err(crate::tr!(
            "plugin-no-release-asset",
            name = name,
            file = file,
            repo = format!("{owner}/{repo}")
        ));
    };
    let bytes = fetch(&asset)?;
    let at = staging.join(main);
    if let Some(d) = at.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&at, bytes).map_err(|e| format!("{}: {e}", at.display()))
}

/// The download link of asset `file` in the releases GitHub listed (its
/// API's JSON), as [`released_asset`] prefers them.
fn pick_release_asset(
    releases: &Value,
    file: &str,
    reference: &str,
    version: &str,
    folder: &str,
) -> Option<String> {
    let with_asset: Vec<(&str, bool, &str)> = releases
        .as_array()?
        .iter()
        .filter(|r| r["draft"] != true)
        .filter_map(|r| {
            let url = r["assets"]
                .as_array()?
                .iter()
                .find(|a| a["name"].as_str() == Some(file))?["browser_download_url"]
                .as_str()?;
            Some((
                r["tag_name"].as_str().unwrap_or_default(),
                r["prerelease"] == true,
                url,
            ))
        })
        .collect();
    let tagged = |tags: &[String]| {
        with_asset
            .iter()
            .find(|(t, _, _)| tags.iter().any(|x| x == t))
            .map(|r| r.2.to_string())
    };
    tagged(&[reference.to_string()])
        .or_else(|| {
            (!version.is_empty())
                .then(|| {
                    tagged(&[
                        format!("v{version}"),
                        version.to_string(),
                        format!("{folder}-v{version}"),
                    ])
                })
                .flatten()
        })
        .or_else(|| {
            with_asset
                .iter()
                .find(|r| !r.1)
                .or(with_asset.first())
                .map(|r| r.2.to_string())
        })
}

/// The plugins in `CONFIG/plugins`, with what `plugins.toml` says of
/// those Kalem installed.
pub fn installed() -> Vec<Installed> {
    crate::settings::config_dir().map_or_else(Vec::new, |d| installed_in(&d))
}

/// [`installed`] in the settings folder `config` (a test's, a frontend's
/// own).
pub fn installed_in(config: &std::path::Path) -> Vec<Installed> {
    let root = config.join("plugins");
    let rec: toml_edit::DocumentMut = std::fs::read_to_string(config.join("plugins.toml"))
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or_default();
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
        .ok_or_else(|| crate::tr!("plugin-not-installed", id = id))?;
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
        // Missing parts are zeros: `1.0` is `1.0.0`.
        (Some(mut x), Some(mut y)) => {
            x.resize(3, 0);
            y.resize(3, 0);
            x > y
        }
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
    Some(crate::tr!("plugin-updates", list = list.join(", ")))
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
    let indexes = index_urls(config);
    std::thread::spawn(move || {
        let Ok(entries) = fetch_indexes(&indexes) else {
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

    #[test]
    fn the_plugins_that_open_a_file() {
        let index = parse_index(
            r#"{"schema":1,"plugins":[
              {"id":"a.sheets","name":"Sheets","version":"1.0.0","download":"https://x/s.wasm",
               "opens":[".xlsx", "XLSM"]},
              {"id":"a.draft","name":"Draft","version":"0.1.0","download":null,"opens":[".xlsx"]},
              {"id":"a.lang","name":"Lang","version":"1.0.0","kind":"declarative",
               "download":"https://x/l.tar.gz","opens":[".xlsx"]},
              {"id":"a.tar","name":"Tar","version":"1.0.0","download":"https://x/t.wasm",
               "opens":[".tar.gz"]}]}"#,
        )
        .unwrap();
        assert_eq!(index[0].opens, [".xlsx", "XLSM"]);
        let ids = |name: &str| -> Vec<String> {
            opening(&index, std::path::Path::new(name))
                .iter()
                .map(|e| e.id.clone())
                .collect()
        };
        // Released components only, by extension in any case.
        assert_eq!(ids("/books/Budget.XLSX"), ["a.sheets"]);
        assert_eq!(ids("b.xlsm"), ["a.sheets"]);
        assert_eq!(ids("backup.tar.gz"), ["a.tar"]);
        assert!(ids("notes.org").is_empty());
        // A name that is only the extension opens nothing.
        assert!(ids(".xlsx").is_empty());
    }
    #[test]
    fn a_broken_record_is_kept_aside() {
        let dir = std::env::temp_dir().join(format!("kalem-record-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plugins.toml");
        std::fs::write(&path, "[plugins\n").unwrap();
        let mut doc = toml_edit::DocumentMut::new();
        doc["x"] = toml_edit::value(1);
        save_record_at(&path, &doc).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("plugins.toml.broken")).unwrap(),
            "[plugins\n"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "x = 1\n");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn stops_counted_by_version_and_forgotten() {
        let dir = std::env::temp_dir().join(format!("kalem-stops-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("plugin-failures.json");
        assert_eq!(super::stops_in(&path, "org.x", "1.0.0"), 0);
        assert_eq!(super::record_stop_in(&path, "org.x", "1.0.0", "trap"), 1);
        assert_eq!(super::record_stop_in(&path, "org.x", "1.0.0", "trap"), 2);
        assert_eq!(super::record_stop_in(&path, "org.y", "0.1.0", "time"), 1);
        assert_eq!(super::stops_in(&path, "org.x", "1.0.0"), 2);
        // An update starts again.
        assert_eq!(super::stops_in(&path, "org.x", "1.0.1"), 0);
        assert_eq!(super::record_stop_in(&path, "org.x", "1.0.1", "trap"), 1);
        assert!(super::clear_stops_in(&path, "org.x"));
        assert!(!super::clear_stops_in(&path, "org.x"));
        assert_eq!(super::stops_in(&path, "org.x", "1.0.1"), 0);
        assert_eq!(super::stops_in(&path, "org.y", "0.1.0"), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;

    #[test]
    fn sources_relative_to_their_index() {
        // A fork's index lists its plugins by folder: they resolve in the
        // fork's repository, at the index's reference.
        let raw = "https://raw.githubusercontent.com/ada/plugins/main/index.json";
        assert_eq!(
            resolve_source(raw, "wordcount"),
            "https://github.com/ada/plugins/tree/main/wordcount"
        );
        assert_eq!(
            resolve_source(
                "https://github.com/ada/plugins/blob/dev/list/index.json",
                "../x"
            ),
            "https://github.com/ada/plugins/tree/dev/x"
        );
        // Links stay as they are.
        let link = "https://github.com/getkalem/plugins/tree/main/elixir";
        assert_eq!(resolve_source(raw, link), link);
        // Downloads beside the index; an index on disk, its folder.
        assert_eq!(
            resolve_file("https://example.com/kalem/index.json", "w-1.0.wasm"),
            "https://example.com/kalem/w-1.0.wasm"
        );
        // A path on disk, joined with the system's separator.
        assert_eq!(
            resolve_source("file:///home/ada/index.json", "./mine"),
            Path::new("/home/ada").join("./mine").to_string_lossy()
        );
    }

    #[test]
    fn indexes_read_in_order() {
        // The user's sources first, then the official index; a plugin of an
        // earlier one stands for the same ID in a later one.
        let dir = std::env::temp_dir().join(format!("kalem-indexes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fork = dir.join("fork.json");
        let main = dir.join("main.json");
        std::fs::write(
            &fork,
            r#"{"plugins":[{"id":"org.kalem.wc","name":"wc","version":"9","source":"wc","kind":"declarative"}]}"#,
        )
        .unwrap();
        std::fs::write(
            &main,
            r#"{"plugins":[{"id":"org.kalem.wc","name":"wc","version":"1","source":"https://github.com/getkalem/plugins/tree/main/wc","kind":"declarative"},{"id":"org.kalem.x","name":"x","version":"1","source":"x","kind":"declarative"}]}"#,
        )
        .unwrap();
        let urls = vec![
            format!("file://{}", fork.display()),
            format!("file://{}", main.display()),
        ];
        let entries = fetch_indexes(&urls).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].version, "9");
        assert_eq!(entries[0].source, dir.join("wc").to_string_lossy());
        let config = crate::Config::from_layers(&[(
            crate::settings::Layer::User,
            None,
            "plugins.sources = [\"https://example.com/a.json\"]\n",
        )]);
        assert_eq!(
            index_urls(&config),
            vec![
                "https://example.com/a.json".to_string(),
                DEFAULT_INDEX.to_string()
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn versions() {
        assert!(newer("0.10.0", "0.9.1"));
        assert!(newer("1.0.0", "0.99"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0", "0.2.0"));
        assert!(newer("v2.0.0", "1.9.9"));
        assert!(!newer("1.0.0", "1.0"));
        assert!(newer("1.0.1", "1.0"));
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
        let gh = |owner: &str, repo: &str, reference: &str, path: &str| Source::GitHub {
            owner: owner.into(),
            repo: repo.into(),
            reference: reference.into(),
            path: path.into(),
        };
        // GitHub's other ways of naming a repository.
        assert_eq!(
            parse_source("someone/kalem-zig"),
            gh("someone", "kalem-zig", "HEAD", "")
        );
        assert_eq!(
            parse_source("git@github.com:someone/kalem-zig.git"),
            gh("someone", "kalem-zig", "HEAD", "")
        );
        assert_eq!(
            parse_source("https://github.com/someone/kalem-zig/releases/tag/v1.2.0"),
            gh("someone", "kalem-zig", "v1.2.0", "")
        );
        assert_eq!(
            parse_source("https://github.com/someone/plugins/blob/main/plugins/zig/plugin.json"),
            gh("someone", "plugins", "main", "plugins/zig")
        );
    }

    #[test]
    fn github_links() {
        use super::github_link;
        assert_eq!(
            github_link(" github.com/someone/kalem-zig/ ").as_deref(),
            Some("https://github.com/someone/kalem-zig")
        );
        assert_eq!(
            github_link("someone/kalem-zig").as_deref(),
            Some("https://github.com/someone/kalem-zig")
        );
        assert_eq!(
            github_link("https://github.com/someone/plugins/tree/main/plugins/zig").as_deref(),
            Some("https://github.com/someone/plugins/tree/main/plugins/zig")
        );
        assert_eq!(
            github_link("https://github.com/someone/kalem-zig/releases/tag/v1.2.0").as_deref(),
            Some("https://github.com/someone/kalem-zig/tree/v1.2.0")
        );
        for not in [
            "elixir",
            "https://example.org/zig.tar.gz",
            "some one/zig",
            "",
            "github.com/someone",
        ] {
            assert_eq!(github_link(not), None, "{not}");
        }
    }

    #[test]
    fn a_release_asset_chosen() {
        use super::pick_release_asset;
        let releases = serde_json::json!([
            {"tag_name": "zig-v0.3.0", "draft": true, "prerelease": false,
             "assets": [{"name": "zig.wasm", "browser_download_url": "https://x/draft"}]},
            {"tag_name": "v0.2.1", "draft": false, "prerelease": true,
             "assets": [{"name": "zig.wasm", "browser_download_url": "https://x/pre"}]},
            {"tag_name": "zig-v0.2.0", "draft": false, "prerelease": false,
             "assets": [{"name": "zig.wasm", "browser_download_url": "https://x/0.2.0"}]},
            {"tag_name": "v0.1.0", "draft": false, "prerelease": false,
             "assets": [{"name": "zig.wasm", "browser_download_url": "https://x/0.1.0"},
                        {"name": "other.wasm", "browser_download_url": "https://x/other"}]}
        ]);
        let pick = |reference: &str, version: &str| {
            pick_release_asset(&releases, "zig.wasm", reference, version, "zig")
        };
        // The tag the link named; else the manifest's version; else the
        // newest release that is not a prerelease (never a draft).
        assert_eq!(pick("v0.1.0", "0.2.0").as_deref(), Some("https://x/0.1.0"));
        assert_eq!(pick("HEAD", "0.2.0").as_deref(), Some("https://x/0.2.0"));
        assert_eq!(pick("HEAD", "0.2.1").as_deref(), Some("https://x/pre"));
        assert_eq!(pick("main", "9.9.9").as_deref(), Some("https://x/0.2.0"));
        assert_eq!(
            pick_release_asset(&releases, "none.wasm", "HEAD", "", "zig"),
            None
        );
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
        let p = prepare("counter", std::slice::from_ref(&url)).unwrap();
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
        // A program named by its permission is listed as one it runs, and
        // not again among the other permissions.
        let git = Prepared {
            permissions: vec!["subprocess:git".into(), "net:fetch:github.com".into()],
            ..p.clone()
        };
        let lines = summary(&git);
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("Runs programs on this computer: git ")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l == "Permissions: net:fetch:github.com"),
            "{lines:?}"
        );

        // The manifest at the release must be the entry's version.
        std::fs::write(&at, index("0.1.0", &sha)).unwrap();
        assert!(
            prepare("counter", std::slice::from_ref(&url))
                .unwrap_err()
                .contains("not org.x.counter 0.1.0")
        );
        // And the asset the hash the index gives.
        std::fs::write(&at, index("0.2.0", &"0".repeat(64))).unwrap();
        assert!(
            prepare("counter", std::slice::from_ref(&url))
                .unwrap_err()
                .contains("SHA-256")
        );
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
