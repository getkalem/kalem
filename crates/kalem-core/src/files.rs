//! Files (design §2.3): reading with mode, byte order mark and line ending
//! detection, atomic saving with an optional `.bak`, and noticing changes
//! other programs make.
//!
//! Saving writes a temporary file next to the target and renames it over
//! the target, keeping its permissions. A symbolic link is followed and
//! stays a link. A file with several hard links, or owned by someone else,
//! is written in place instead, since a rename would detach the links or
//! change the owner.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::{DefaultHasher, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::document::{LineEnding, Metadata};
use crate::events::{Event, EventSender};
use crate::mode::DocumentMode;

/// Why a file cannot be opened.
#[derive(Debug)]
pub enum OpenError {
    /// Reading failed.
    Io(io::Error),
    /// The file is not text (§2.6).
    Binary,
    /// The file is not valid in the encoding asked for.
    NotUtf8 {
        /// The offset of the first invalid byte.
        at: usize,
    },
    /// No encoding of that name.
    UnknownEncoding(String),
    /// An encoding Kalem cannot read or write (UTF-32).
    UnsupportedEncoding(String),
    /// The viewer that opens the file failed (`crate::viewer`).
    Viewer(String),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Io(e) => write!(f, "{e}"),
            OpenError::Binary => f.write_str("The file is not a text file"),
            OpenError::NotUtf8 { at } => {
                write!(f, "The file is not UTF-8 (invalid byte at offset {at})")
            }
            OpenError::UnknownEncoding(name) => write!(f, "Unknown encoding: {name}"),
            OpenError::UnsupportedEncoding(name) => {
                write!(f, "The file is in {name}, which Kalem cannot read")
            }
            OpenError::Viewer(e) => {
                f.write_str(&crate::tr!("msg-viewer-cannot-open", error = e.as_str()))
            }
        }
    }
}

impl std::error::Error for OpenError {}

impl From<io::Error> for OpenError {
    fn from(e: io::Error) -> Self {
        OpenError::Io(e)
    }
}

/// What a file on disk was when it was last read or written: enough to
/// tell a change of content from a touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskState {
    /// The length in bytes.
    pub len: u64,
    /// The modification time.
    pub modified: Option<SystemTime>,
    /// A hash of the contents.
    pub hash: u64,
}

fn hash(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    h.write(bytes);
    h.finish()
}

impl DiskState {
    /// The state of `bytes` written at `modified`.
    pub fn of(bytes: &[u8], modified: Option<SystemTime>) -> DiskState {
        DiskState {
            len: bytes.len() as u64,
            modified,
            hash: hash(bytes),
        }
    }
}

/// How a file compares with a known [`DiskState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskChange {
    /// As known.
    Unchanged,
    /// Same contents, new modification time: the new state.
    Touched(DiskState),
    /// Different contents.
    Modified,
    /// The file is gone.
    Deleted,
}

/// Compares the file at `path` with `known`, reading it only when its
/// length and time differ.
pub fn check(path: &Path, known: &DiskState) -> io::Result<DiskChange> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(DiskChange::Deleted),
        Err(e) => return Err(e),
    };
    let modified = meta.modified().ok();
    if meta.len() == known.len && modified == known.modified {
        return Ok(DiskChange::Unchanged);
    }
    if meta.len() != known.len {
        return Ok(DiskChange::Modified);
    }
    let bytes = std::fs::read(path)?;
    let now = DiskState::of(&bytes, modified);
    Ok(if now.hash == known.hash {
        DiskChange::Touched(now)
    } else {
        DiskChange::Modified
    })
}

/// The line endings of `text`: CRLF when every line feed follows a
/// carriage return, as Emacs decides between `dos` and `unix`; CR when
/// there is no line feed but a carriage return (`mac`).
pub fn line_ending_of(text: &str) -> LineEnding {
    let b = text.as_bytes();
    if !b.contains(&b'\n') && b.contains(&b'\r') {
        return LineEnding::Cr;
    }
    let mut lf = b
        .iter()
        .enumerate()
        .filter(|(_, c)| **c == b'\n')
        .peekable();
    if lf.peek().is_none() {
        return LineEnding::Lf;
    }
    if lf.all(|(i, _)| i > 0 && b[i - 1] == b'\r') {
        LineEnding::CrLf
    } else {
        LineEnding::Lf
    }
}

/// Decodes a file's bytes: the mode, the byte order mark (removed from the
/// text) and the line endings (kept in the text; the parser reads them as
/// Emacs does after decoding). UTF-8 and UTF-16 with a byte order mark are
/// read as they are; other text that is not UTF-8 is read in the encoding
/// its bytes suggest (chardetng), as Emacs guesses a coding system.
pub fn decode(path: Option<&Path>, bytes: Vec<u8>) -> Result<(String, Metadata), OpenError> {
    decode_with(path, bytes, None)
}

/// [`decode`] in the encoding `wanted` unless a byte order mark names
/// another; UTF-8 that is not valid is guessed as [`decode`] does.
pub fn decode_with(
    path: Option<&Path>,
    bytes: Vec<u8>,
    wanted: Option<&'static encoding_rs::Encoding>,
) -> Result<(String, Metadata), OpenError> {
    use encoding_rs::{UTF_8, UTF_16BE, UTF_16LE};
    if utf32_bom(&bytes) {
        return Err(OpenError::UnsupportedEncoding("UTF-32".into()));
    }
    let binary =
        |b: &[u8]| DocumentMode::detect(path, &b[..b.len().min(8192)]) == DocumentMode::Binary;
    let bom = encoding_rs::Encoding::for_bom(&bytes);
    let (text, encoding, has_bom, lossy) = match bom {
        Some((enc, len)) if enc == UTF_16LE || enc == UTF_16BE => {
            let (t, lossy) = enc.decode_without_bom_handling(&bytes[len..]);
            (t.into_owned(), enc, true, lossy)
        }
        Some((_, len)) => {
            if binary(&bytes[len..]) {
                return Err(OpenError::Binary);
            }
            let mut text = String::from_utf8(bytes).map_err(|e| OpenError::NotUtf8 {
                at: e.utf8_error().valid_up_to(),
            })?;
            text.drain(..len);
            (text, UTF_8, true, false)
        }
        None => {
            if binary(&bytes) {
                return Err(OpenError::Binary);
            }
            // Else the file's own `coding:` (a `-*-` line or a `Local
            // Variables:` block), as Emacs reads it.
            match wanted
                .or_else(|| file_coding(&bytes))
                .filter(|e| *e != UTF_8)
            {
                Some(enc) => {
                    let (t, lossy) = enc.decode_without_bom_handling(&bytes);
                    (t.into_owned(), enc, false, lossy)
                }
                None => match String::from_utf8(bytes) {
                    Ok(t) => (t, UTF_8, false, false),
                    Err(e) => {
                        let bytes = e.into_bytes();
                        let enc = guess(&bytes);
                        match enc.decode_without_bom_handling(&bytes) {
                            (t, false) => (t.into_owned(), enc, false, false),
                            // Bytes the guess cannot read: Windows-1252
                            // reads every byte, and writes each back.
                            _ => {
                                let enc = encoding_rs::WINDOWS_1252;
                                let (t, _) = enc.decode_without_bom_handling(&bytes);
                                (t.into_owned(), enc, false, false)
                            }
                        }
                    }
                },
            }
        }
    };
    let meta = Metadata {
        path: path.map(Path::to_path_buf),
        mode: DocumentMode::detect_text(path, &text),
        line_ending: line_ending_of(&text),
        bom: has_bom,
        encoding,
        lossy,
    };
    Ok((mac_to_lf(text, meta.line_ending), meta))
}

/// The text of a CR file with its carriage returns as line feeds.
fn mac_to_lf(text: String, ending: LineEnding) -> String {
    if ending == LineEnding::Cr {
        text.replace('\r', "\n")
    } else {
        text
    }
}

/// A file's bytes read in `encoding` whatever they are (Reopen with
/// Encoding): a byte order mark of that encoding is dropped, and bytes it
/// cannot read become U+FFFD.
pub fn decode_as(
    path: Option<&Path>,
    bytes: &[u8],
    encoding: &'static encoding_rs::Encoding,
) -> (String, Metadata) {
    let (bom, rest) = match encoding_rs::Encoding::for_bom(bytes) {
        Some((e, len)) if e == encoding => (true, &bytes[len..]),
        _ => (false, bytes),
    };
    let (text, lossy) = encoding.decode_without_bom_handling(rest);
    let text = text.into_owned();
    let meta = Metadata {
        path: path.map(Path::to_path_buf),
        mode: DocumentMode::detect_text(path, &text),
        line_ending: line_ending_of(&text),
        bom: bom || encoding == encoding_rs::UTF_16LE || encoding == encoding_rs::UTF_16BE,
        encoding,
        lossy,
    };
    (mac_to_lf(text, meta.line_ending), meta)
}

/// The encoding a status bar names: nothing for UTF-8, else its name
/// (`UTF-16LE`, `windows-1254`).
pub fn encoding_label(meta: &Metadata) -> Option<&'static str> {
    (meta.encoding != encoding_rs::UTF_8).then(|| meta.encoding.name())
}

/// What to tell when a file was opened in an encoding guessed from its
/// bytes (a legacy encoding without a byte order mark), or with bytes the
/// encoding could not read.
pub fn guessed_message(meta: &Metadata) -> Option<String> {
    if meta.lossy {
        return Some(crate::tr!(
            "msg-opened-lossy",
            encoding = meta.encoding.name()
        ));
    }
    let utf = [
        encoding_rs::UTF_8,
        encoding_rs::UTF_16LE,
        encoding_rs::UTF_16BE,
    ];
    (!utf.contains(&meta.encoding))
        .then(|| crate::tr!("msg-opened-as", encoding = meta.encoding.name()))
}

/// The legacy encoding bytes that are not UTF-8 are most likely in, the
/// system's region or the interface language taken as a hint (a short
/// Turkish text reads as Windows-1254 on a Turkish system, not as
/// Windows-1252).
pub fn guess(bytes: &[u8]) -> &'static encoding_rs::Encoding {
    guess_in(bytes, locale_hint().as_deref())
}

/// [`guess`] with a top-level domain as the hint (`tr`, `jp`, `ru`). A
/// hint of a region that writes in Windows-1252 (`us`, `de`) says only
/// what the text is when nothing else does: the bytes decide first, so a
/// Turkish file opened on an American system reads as Turkish.
pub fn guess_in(bytes: &[u8], tld: Option<&str>) -> &'static encoding_rs::Encoding {
    let mut d = chardetng::EncodingDetector::new();
    d.feed(bytes, true);
    let hinted = d.guess(tld.map(str::as_bytes), true);
    if hinted == encoding_rs::WINDOWS_1252 && tld.is_some() {
        return d.guess(None, true);
    }
    hinted
}

/// The top-level domain of the system locale's region (`tr-TR` gives
/// `tr`), else of the interface language.
fn locale_hint() -> Option<String> {
    let from_language = |l: &str| {
        Some(
            match l {
                "en" | "" => return None,
                "ja" => "jp",
                "el" => "gr",
                "zh" => "cn",
                "ko" => "kr",
                "cs" => "cz",
                "uk" => "ua",
                "he" => "il",
                l => l,
            }
            .to_string(),
        )
    };
    let locale = sys_locale::get_locale().unwrap_or_default();
    let mut parts = locale.split(['-', '_', '.']);
    let language = parts.next().unwrap_or("").to_ascii_lowercase();
    match parts.find(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic())) {
        Some(region) => Some(region.to_ascii_lowercase()),
        None => from_language(&language).or_else(|| from_language(&crate::l10n::language())),
    }
}

/// The encoding a file names in a `coding:` file variable: Emacs's names
/// (`latin-1`, `iso-latin-5`, `utf-8-unix`, `cp1254`) or any WHATWG label.
pub fn file_coding(bytes: &[u8]) -> Option<&'static encoding_rs::Encoding> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(8192)]);
    let tail = String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(3000)..]);
    let name = crate::mode::mode_line_variable(&head, "coding")
        .or_else(|| crate::mode::local_variable(&tail, "coding"))?;
    emacs_coding(&name)
}

/// The encoding of an Emacs coding system's name.
fn emacs_coding(name: &str) -> Option<&'static encoding_rs::Encoding> {
    let name = name.trim().to_ascii_lowercase();
    let name = ["-unix", "-dos", "-mac"]
        .iter()
        .find_map(|s| name.strip_suffix(s))
        .unwrap_or(&name);
    let name = match name {
        "utf-8-with-signature" | "utf-8-emacs" | "prefer-utf-8" | "mule-utf-8" => "utf-8",
        "turkish-iso-8bit" => "iso-8859-9",
        "undecided" | "raw-text" | "no-conversion" | "binary" => return None,
        n => n,
    };
    let label = match name
        .strip_prefix("iso-latin-")
        .or_else(|| name.strip_prefix("latin-"))
    {
        Some(n) => format!("latin{n}"),
        None => name.to_string(),
    };
    encoding_for(&label)
}

/// A UTF-32 byte order mark, which starts like UTF-16LE's.
fn utf32_bom(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xFE, 0, 0]) || bytes.starts_with(&[0, 0, 0xFE, 0xFF])
}

/// The encoding a name or label gives (`utf-8`, `latin1`, `windows-1254`,
/// `shift_jis`, `utf-16le`).
pub fn encoding_for(name: &str) -> Option<&'static encoding_rs::Encoding> {
    encoding_rs::Encoding::for_label(name.trim().as_bytes())
}

/// The encodings offered by Reopen with Encoding and Save with Encoding.
pub const COMMON_ENCODINGS: &[&str] = &[
    "UTF-8",
    "UTF-16LE",
    "UTF-16BE",
    "windows-1252",
    "ISO-8859-1",
    "windows-1254",
    "ISO-8859-9",
    "ISO-8859-15",
    "windows-1250",
    "windows-1251",
    "KOI8-R",
    "Shift_JIS",
    "EUC-JP",
    "GBK",
    "gb18030",
    "Big5",
    "EUC-KR",
];

/// The state of a file on disk, read for its hash (a file a viewer
/// opened, which is not read as text).
pub fn stat(path: &Path) -> io::Result<DiskState> {
    let bytes = std::fs::read(path)?;
    Ok(DiskState::of(
        &bytes,
        std::fs::metadata(path)?.modified().ok(),
    ))
}

/// Reads a file.
pub fn read(path: &Path) -> Result<(String, Metadata, DiskState), OpenError> {
    read_with(path, None)
}

/// Reads a file in the encoding `wanted` (see [`decode_with`]).
pub fn read_with(
    path: &Path,
    wanted: Option<&'static encoding_rs::Encoding>,
) -> Result<(String, Metadata, DiskState), OpenError> {
    let bytes = std::fs::read(path)?;
    let modified = std::fs::metadata(path)?.modified().ok();
    let disk = DiskState::of(&bytes, modified);
    let (text, meta) = decode_with(Some(path), bytes, wanted)?;
    Ok((text, meta, disk))
}

/// The first character of `text` that `encoding` cannot write.
pub fn unencodable(text: &str, encoding: &'static encoding_rs::Encoding) -> Option<char> {
    use encoding_rs::{UTF_8, UTF_16BE, UTF_16LE};
    if encoding == UTF_8 || encoding == UTF_16LE || encoding == UTF_16BE {
        return None;
    }
    let (_, _, errors) = encoding.encode(text);
    if !errors {
        return None;
    }
    let mut buf = [0u8; 4];
    text.chars()
        .find(|c| encoding.encode(c.encode_utf8(&mut buf)).2)
}

/// The bytes to save: the text in the file's encoding, the byte order mark
/// if the file had one, and in CRLF files a carriage return before line
/// feeds that lack one (commands insert bare line feeds). Saving checks
/// first that the encoding can write every character ([`unencodable`])
/// and refuses otherwise; should one slip through here, `encoding_rs`
/// writes it as `&#N;`.
pub fn encode(text: &str, meta: &Metadata) -> Vec<u8> {
    use encoding_rs::{UTF_8, UTF_16BE, UTF_16LE};
    let mut lines = Vec::with_capacity(text.len() + 3);
    match meta.line_ending {
        LineEnding::Lf => lines.extend_from_slice(text.as_bytes()),
        LineEnding::Cr => lines.extend(text.bytes().map(|c| if c == b'\n' { b'\r' } else { c })),
        LineEnding::CrLf => {
            let b = text.as_bytes();
            for (i, c) in b.iter().enumerate() {
                if *c == b'\n' && (i == 0 || b[i - 1] != b'\r') {
                    lines.push(b'\r');
                }
                lines.push(*c);
            }
        }
    }
    let enc = meta.encoding;
    if enc == UTF_8 {
        let mut out = Vec::with_capacity(lines.len() + 3);
        if meta.bom {
            out.extend_from_slice(b"\xEF\xBB\xBF");
        }
        out.extend_from_slice(&lines);
        return out;
    }
    // The text's bytes with carriage returns added: UTF-8 still.
    let text = String::from_utf8(lines)
        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
    if enc == UTF_16LE || enc == UTF_16BE {
        let le = enc == UTF_16LE;
        let mut out = Vec::with_capacity(text.len() * 2 + 2);
        if meta.bom {
            out.extend_from_slice(if le { &[0xFF, 0xFE] } else { &[0xFE, 0xFF] });
        }
        for u in text.encode_utf16() {
            out.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() });
        }
        return out;
    }
    enc.encode(&text).0.into_owned()
}

/// Options for saving.
#[derive(Debug, Clone, Copy, Default)]
pub struct SaveOptions {
    /// Copy the old file to `NAME.bak` first.
    pub backup: bool,
}

/// The backup file of `path`: `notes.org.bak`.
pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn write_in_place(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

/// Writes `bytes` to `path` atomically (see the module documentation) and
/// returns the new state of the file.
pub fn write(path: &Path, bytes: &[u8], options: SaveOptions) -> io::Result<DiskState> {
    // Follow links, so that a link stays a link.
    let target = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let existing = std::fs::metadata(&target).ok();
    // A write-protected file is not replaced: renaming over it needs only
    // its folder's permission, and would have saved it all the same.
    if existing
        .as_ref()
        .is_some_and(|m| m.permissions().readonly())
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            crate::l10n::tr("msg-file-read-only"),
        ));
    }
    if options.backup && existing.is_some() {
        std::fs::copy(&target, backup_path(&target))?;
        tracing::debug!(path = %target.display(), "backup written");
    }
    let state = |target: &Path| -> io::Result<DiskState> {
        Ok(DiskState::of(
            bytes,
            std::fs::metadata(target)?.modified().ok(),
        ))
    };
    #[cfg(unix)]
    if let Some(m) = &existing {
        use std::os::unix::fs::MetadataExt;
        if m.nlink() > 1 {
            tracing::debug!(path = %target.display(), "hard-linked file saved in place");
            write_in_place(&target, bytes)?;
            return state(&target);
        }
    }
    let dir = target
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.kalem-{}-{n}.tmp", std::process::id()));
    let result = (|| -> io::Result<bool> {
        let mut f = std::fs::File::create_new(&tmp)?;
        if let Some(m) = &existing {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                // Renaming would give the file to us.
                if f.metadata()?.uid() != m.uid() {
                    return Ok(false);
                }
            }
            std::fs::set_permissions(&tmp, m.permissions())?;
        }
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, &target)?;
        Ok(true)
    })();
    match result {
        Ok(true) =>
        {
            #[cfg(unix)]
            if let Ok(d) = std::fs::File::open(dir) {
                let _ = d.sync_all();
            }
        }
        Ok(false) => {
            tracing::debug!(path = %target.display(), "file of another owner saved in place");
            let _ = std::fs::remove_file(&tmp);
            write_in_place(&target, bytes)?;
        }
        // A folder Kalem may not write in, holding a file it may: the
        // file written in place, as a hard-linked one is.
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied && existing.is_some() => {
            let _ = std::fs::remove_file(&tmp);
            tracing::debug!(path = %target.display(), "file in a read-only folder saved in place");
            write_in_place(&target, bytes)?;
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    }
    state(&target)
}

type Watched = HashMap<PathBuf, HashSet<std::ffi::OsString>>;

/// Watches files for changes by other programs and sends
/// `workspace:file-changed` to the event bus. It watches the files'
/// directories, so files replaced by renaming (as editors save) are seen
/// too. Changes Kalem makes itself also arrive; [`check`] against the
/// document's [`DiskState`] tells them apart.
pub struct FileWatcher {
    watcher: notify::RecommendedWatcher,
    /// Watched files by directory (canonical), with the original paths.
    watched: Arc<Mutex<Watched>>,
    originals: Arc<Mutex<HashMap<PathBuf, PathBuf>>>,
    /// Folders watched as a whole (file manager listings), canonical, with
    /// the original paths.
    dirs: Arc<Mutex<HashMap<PathBuf, PathBuf>>>,
}

impl fmt::Debug for FileWatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileWatcher").finish_non_exhaustive()
    }
}

fn split(path: &Path) -> io::Result<(PathBuf, std::ffi::OsString)> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a file path"))?;
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok((dunce::canonicalize(dir)?, name.to_os_string()))
}

impl FileWatcher {
    /// A watcher that sends to `sender`.
    pub fn new(sender: EventSender) -> notify::Result<FileWatcher> {
        let watched: Arc<Mutex<Watched>> = Arc::default();
        let originals: Arc<Mutex<HashMap<PathBuf, PathBuf>>> = Arc::default();
        let dirs: Arc<Mutex<HashMap<PathBuf, PathBuf>>> = Arc::default();
        let (w, o, ds) = (watched.clone(), originals.clone(), dirs.clone());
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            if matches!(event.kind, notify::EventKind::Access(_)) {
                return;
            }
            let watched = w.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let originals = o.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let whole = ds.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut sent = HashSet::new();
            for p in &event.paths {
                let (Some(dir), Some(name)) = (p.parent(), p.file_name()) else {
                    continue;
                };
                let dir = dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
                if let Some(original) = whole.get(&dir).or_else(|| whole.get(p))
                    && sent.insert(original.clone())
                {
                    sender.send(Event::WorkspaceFileChanged {
                        path: original.clone(),
                    });
                }
                if watched.get(&dir).is_some_and(|names| names.contains(name)) {
                    let key = dir.join(name);
                    let path = originals.get(&key).cloned().unwrap_or(key);
                    if sent.insert(path.clone()) {
                        sender.send(Event::WorkspaceFileChanged { path });
                    }
                }
            }
        })?;
        Ok(FileWatcher {
            watcher,
            watched,
            originals,
            dirs,
        })
    }

    /// Starts watching folder `dir` as a whole: any change in it sends
    /// `workspace:file-changed` with `dir` as the path.
    pub fn watch_dir(&mut self, dir: &Path) -> notify::Result<()> {
        use notify::Watcher;
        let canonical = dunce::canonicalize(dir)?;
        let new_dir = !self
            .watched
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(&canonical)
            && !self
                .dirs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains_key(&canonical);
        if new_dir {
            self.watcher
                .watch(&canonical, notify::RecursiveMode::NonRecursive)?;
        }
        self.dirs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(canonical, dir.to_path_buf());
        Ok(())
    }

    /// Stops watching folder `dir` as a whole.
    pub fn unwatch_dir(&mut self, dir: &Path) -> notify::Result<()> {
        use notify::Watcher;
        let canonical = dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let removed = self
            .dirs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&canonical)
            .is_some();
        if removed
            && !self
                .watched
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains_key(&canonical)
        {
            self.watcher.unwatch(&canonical)?;
        }
        Ok(())
    }

    /// Starts watching `path`.
    pub fn watch(&mut self, path: &Path) -> notify::Result<()> {
        use notify::Watcher;
        let (dir, name) = split(path)?;
        // The watch list is not held while notify starts watching: its
        // thread may be waiting for it to report an event.
        let new_dir = !self
            .watched
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(&dir)
            && !self
                .dirs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains_key(&dir);
        if new_dir {
            self.watcher
                .watch(&dir, notify::RecursiveMode::NonRecursive)?;
        }
        self.watched
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(dir.clone())
            .or_default()
            .insert(name.clone());
        self.originals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(dir.join(name), path.to_path_buf());
        Ok(())
    }

    /// Stops watching `path`.
    pub fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        use notify::Watcher;
        let (dir, name) = split(path)?;
        let now_empty = {
            let mut watched = self
                .watched
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(names) = watched.get_mut(&dir) else {
                return Ok(());
            };
            names.remove(&name);
            let empty = names.is_empty();
            if empty {
                watched.remove(&dir);
            }
            empty
        };
        self.originals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&dir.join(&name));
        if now_empty
            && !self
                .dirs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains_key(&dir)
        {
            self.watcher.unwatch(&dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    /// A write-protected file is refused, not replaced; a writable file
    /// in a folder Kalem may not write in is written in place.
    #[cfg(unix)]
    #[test]
    fn permissions_on_save() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("kalem-perm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("locked.txt");
        std::fs::write(&p, "old\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o444)).unwrap();
        let e = super::write(&p, b"new\n", super::SaveOptions::default()).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "old\n");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        // The folder read-only, the file writable (a root shell's tests
        // write anywhere: nothing to check then).
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let tmp_allowed = std::fs::File::create(dir.join("probe")).is_ok();
        if !tmp_allowed {
            super::write(&p, b"new\n", super::SaveOptions::default()).unwrap();
            assert_eq!(std::fs::read_to_string(&p).unwrap(), "new\n");
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kalem-files-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn decoding() {
        let (t, m) = decode(
            Some(Path::new("a.org")),
            b"\xEF\xBB\xBF* A\r\nb\r\n".to_vec(),
        )
        .unwrap();
        assert_eq!(t, "* A\r\nb\r\n");
        assert!(m.bom && m.line_ending == LineEnding::CrLf && m.mode == DocumentMode::Org);
        assert_eq!(m.encoding, encoding_rs::UTF_8);
        assert_eq!(encode(&t, &m), b"\xEF\xBB\xBF* A\r\nb\r\n");
        // Line feeds from commands get their carriage return.
        assert_eq!(encode("* A\r\nnew\n", &m), b"\xEF\xBB\xBF* A\r\nnew\r\n");
        assert_eq!(line_ending_of("a\r\nb\n"), LineEnding::Lf);
        assert_eq!(line_ending_of("a"), LineEnding::Lf);
        assert_eq!(line_ending_of("a\rb\r"), LineEnding::Cr);
        // The file's `coding:` names its encoding, as Emacs reads it.
        let mut bytes = b"# -*- coding: iso-latin-5 -*-\n".to_vec();
        bytes.extend_from_slice(b"\xfd\xfe\n");
        let (t, m) = decode(Some(Path::new("t.txt")), bytes).unwrap();
        assert_eq!(
            (m.encoding, t.lines().nth(1)),
            (encoding_rs::WINDOWS_1254, Some("ış"))
        );
        let mut bytes = vec![b'x'; 9000];
        bytes.extend_from_slice(b"\nLocal Variables:\ncoding: cp1251\nmode: org\nEnd:\n\xe0");
        let (_, m) = decode(Some(Path::new("t.txt")), bytes).unwrap();
        assert_eq!(
            (m.encoding, &m.mode),
            (encoding_rs::WINDOWS_1251, &DocumentMode::Org)
        );
        // Short Turkish text reads as Windows-1254 with a Turkish hint.
        let (b, _, _) = encoding_rs::WINDOWS_1254.encode("ağaç");
        assert_eq!(guess_in(&b, Some("tr")), encoding_rs::WINDOWS_1254);
        // A Western region's hint does not outweigh the bytes.
        let (b, _, _) = encoding_rs::WINDOWS_1254
            .encode("Ağaçların gölgesinde çalışan işçiler, güneşin doğuşunu şarkılarla karşıladı.");
        assert_eq!(guess_in(&b, Some("us")), encoding_rs::WINDOWS_1254);
        let (b, _, _) = encoding_rs::WINDOWS_1252.encode("Größe und Café, déjà vu à Noël.");
        assert_eq!(guess_in(&b, Some("us")), encoding_rs::WINDOWS_1252);
        // UTF-32 is refused, not read as UTF-16.
        let e = decode(None, b"\xFF\xFE\0\0a\0\0\0".to_vec()).unwrap_err();
        assert!(matches!(e, OpenError::UnsupportedEncoding(_)), "{e}");
        // A byte order mark does not make binary data text.
        let e = decode(None, b"\xEF\xBB\xBFab\0\0\0cd".to_vec()).unwrap_err();
        assert!(matches!(e, OpenError::Binary), "{e}");
        // UTF-16 with a lone surrogate: read with U+FFFD, and said so.
        let (_, m) = decode(None, b"\xFF\xFEa\0\x00\xD8b\0".to_vec()).unwrap();
        assert!(m.lossy && guessed_message(&m).is_some());
        // Bytes Shift_JIS cannot read are read as Windows-1252, which
        // writes them back unchanged.
        let bytes = b"caf\xE9 d\x81j\xFF vu, na\xEFve\n".to_vec();
        let (t, m) = decode(None, bytes.clone()).unwrap();
        assert!(!m.lossy);
        assert_eq!(encode(&t, &m), bytes);
        // A classic Mac file: read with line feeds, written back as it was.
        let (text, m) = decode(Some(Path::new("t.csv")), b"a,b\rc,d\r".to_vec()).unwrap();
        assert_eq!(
            (text.as_str(), m.line_ending),
            ("a,b\nc,d\n", LineEnding::Cr)
        );
        assert_eq!(encode(&text, &m), b"a,b\rc,d\r");
        assert!(matches!(
            decode(None, b"a\0b".to_vec()),
            Err(OpenError::Binary)
        ));
        // Text that is not UTF-8 is read in the encoding it looks like.
        let long = [b"x".repeat(9000), vec![0xFF]].concat();
        let (t, m) = decode(None, long).unwrap();
        assert!(
            t.ends_with('ÿ') && m.encoding != encoding_rs::UTF_8,
            "{:?}",
            m.encoding
        );
    }

    #[test]
    fn encodings() {
        use encoding_rs::{UTF_16BE, UTF_16LE, WINDOWS_1254};
        // UTF-16 with a byte order mark, both ways round.
        for (enc, bom) in [(UTF_16LE, [0xFF, 0xFE]), (UTF_16BE, [0xFE, 0xFF])] {
            let mut bytes = bom.to_vec();
            for u in "* Başlık\r\nmetin\r\n".encode_utf16() {
                bytes.extend(if enc == UTF_16LE {
                    u.to_le_bytes()
                } else {
                    u.to_be_bytes()
                });
            }
            let (t, m) = decode(Some(Path::new("a.org")), bytes.clone()).unwrap();
            assert_eq!(t, "* Başlık\r\nmetin\r\n");
            assert!(m.bom && m.encoding == enc && m.mode == DocumentMode::Org);
            assert_eq!(m.line_ending, LineEnding::CrLf);
            assert_eq!(encode(&t, &m), bytes);
            assert_eq!(encoding_label(&m), Some(enc.name()));
            assert_eq!(guessed_message(&m), None);
        }
        // Turkish in Windows-1254, guessed from its bytes.
        let turkish = "Ağaçların gölgesinde çalışan işçiler, güneşin doğuşunu şarkılarla karşıladı. Öğretmen İstanbul'dan geldi ve ılık bir çay içti.\n";
        let (bytes, _, _) = WINDOWS_1254.encode(turkish);
        let (t, m) = decode(Some(Path::new("t.txt")), bytes.to_vec()).unwrap();
        assert_eq!(m.encoding, WINDOWS_1254);
        assert_eq!(t, turkish);
        assert!(guessed_message(&m).is_some());
        assert_eq!(encode(&t, &m), bytes.to_vec());
        // Characters the encoding has no place for.
        assert_eq!(unencodable("çok güzel ł", WINDOWS_1254), Some('ł'));
        assert_eq!(unencodable("çok güzel", WINDOWS_1254), None);
        assert_eq!(unencodable("ł", UTF_16LE), None);
        // Reopened in an encoding chosen by hand.
        let (t, m) = decode_as(None, &[0x63, 0x61, 0x66, 0xE9], encoding_rs::WINDOWS_1252);
        assert_eq!(
            (t.as_str(), m.encoding),
            ("café", encoding_rs::WINDOWS_1252)
        );
        let (t, m) = decode_as(None, "é".as_bytes(), encoding_rs::UTF_8);
        assert_eq!((t.as_str(), m.bom), ("é", false));
        // The encoding the document had, when it is read again.
        let (t, m) = decode_with(None, vec![0xFD], Some(WINDOWS_1254)).unwrap();
        assert_eq!((t.as_str(), m.encoding), ("ı", WINDOWS_1254));
        assert_eq!(encoding_for("latin1"), Some(encoding_rs::WINDOWS_1252));
        assert_eq!(encoding_for("nope"), None);
    }

    #[test]
    fn saving() {
        let d = temp_dir("save");
        let p = d.join("n.org");
        let s1 = write(&p, b"one\n", SaveOptions::default()).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"one\n");
        assert_eq!(check(&p, &s1).unwrap(), DiskChange::Unchanged);
        let s2 = write(&p, b"two\n", SaveOptions { backup: true }).unwrap();
        assert_eq!(std::fs::read(backup_path(&p)).unwrap(), b"one\n");
        assert_eq!(check(&p, &s1).unwrap(), DiskChange::Modified);
        assert_eq!(check(&p, &s2).unwrap(), DiskChange::Unchanged);
        // A touch with the same contents.
        let older = DiskState {
            modified: None,
            ..s2
        };
        assert!(matches!(check(&p, &older).unwrap(), DiskChange::Touched(_)));
        std::fs::remove_file(&p).unwrap();
        assert_eq!(check(&p, &s2).unwrap(), DiskChange::Deleted);
        // No temporary files are left.
        let names: Vec<String> = std::fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["n.org.bak"]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn links_and_permissions() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
        let d = temp_dir("links");
        let real = d.join("real.org");
        std::fs::write(&real, "a\n").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
        let link = d.join("link.org");
        symlink(&real, &link).unwrap();
        write(&link, b"b\n", SaveOptions::default()).unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(&real).unwrap(), b"b\n");
        assert_eq!(
            std::fs::metadata(&real).unwrap().permissions().mode() & 0o777,
            0o640
        );
        let hard = d.join("hard.org");
        std::fs::hard_link(&real, &hard).unwrap();
        let ino = std::fs::metadata(&real).unwrap().ino();
        write(&hard, b"c\n", SaveOptions::default()).unwrap();
        assert_eq!(std::fs::metadata(&real).unwrap().ino(), ino);
        assert_eq!(std::fs::read(&real).unwrap(), b"c\n");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn watching() {
        use crate::events::{EventBus, Reply};
        use std::cell::RefCell;
        use std::rc::Rc;
        let d = temp_dir("watch");
        let p = d.join("w.org");
        std::fs::write(&p, "a\n").unwrap();
        let mut bus = EventBus::new();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        bus.subscribe(None, move |e| {
            if let Event::WorkspaceFileChanged { path } = e {
                s.borrow_mut().push(path.clone());
            }
            Reply::Continue
        });
        let mut w = FileWatcher::new(bus.sender()).unwrap();
        w.watch(&p).unwrap();
        // Let the watcher start (FSEvents reports from when it started).
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::fs::write(d.join("other.org"), "x\n").unwrap();
        write(&p, b"b\n", SaveOptions::default()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while seen.borrow().is_empty() && std::time::Instant::now() < deadline {
            bus.dispatch_queued();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!seen.borrow().is_empty(), "no event");
        assert!(seen.borrow().iter().all(|x| x == &p), "{:?}", seen.borrow());
        w.unwatch(&p).unwrap();
        std::fs::remove_dir_all(&d).unwrap();
    }
}
