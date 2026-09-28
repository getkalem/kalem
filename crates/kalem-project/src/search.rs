//! Text search through a project's files (`projectile-ripgrep`): a string
//! or a regular expression, with case and whole-word switches, in the
//! background, with results as they come.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::{BinaryDetection, SearcherBuilder, sinks};

/// At most this many matching lines are reported.
pub const MAX_HITS: usize = 10_000;

/// Lines are cut to this many bytes for display.
const MAX_LINE: usize = 400;

/// What to search for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// The text, or a regular expression.
    pub text: String,
    /// `text` is a regular expression.
    pub regex: bool,
    /// Upper and lower case differ.
    pub case_sensitive: bool,
    /// Only whole words match.
    pub whole_word: bool,
}

/// A matching line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The file.
    pub path: PathBuf,
    /// The line number, from 1.
    pub line: u64,
    /// The match's first byte in the line.
    pub column: usize,
    /// The match's length in bytes.
    pub len: usize,
    /// The line, without its line break, cut when very long.
    pub text: String,
}

/// Searches the files of `root` (with the project's ignore rules) for
/// `query`, calling `sink` with each matching line until it returns
/// `false` or `cancel` is set. Errors if the query is not a valid regular
/// expression.
pub fn search(
    root: &Path,
    ignore: &[String],
    query: &Query,
    cancel: &AtomicBool,
    mut sink: impl FnMut(Hit) -> bool,
) -> Result<(), String> {
    if query.text.is_empty() {
        return Ok(());
    }
    let matcher = RegexMatcherBuilder::new()
        .case_insensitive(!query.case_sensitive)
        .word(query.whole_word)
        .fixed_strings(!query.regex)
        .build(&query.text)
        .map_err(|e| e.to_string())?;
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .line_number(true)
        .build();
    let mut stop = false;
    for entry in crate::files::walker(root, ignore).build() {
        if stop || cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(e) = entry else { continue };
        if !e.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = e.path().to_path_buf();
        let result = searcher.search_path(
            &matcher,
            &path,
            sinks::Lossy(|line, text| {
                if cancel.load(Ordering::Relaxed) {
                    stop = true;
                    return Ok(false);
                }
                let text = text.trim_end_matches(['\n', '\r']);
                let (column, len) = matcher
                    .find(text.as_bytes())
                    .ok()
                    .flatten()
                    .map_or((0, 0), |m| (m.start(), m.end() - m.start()));
                let mut cut = text.len().min(MAX_LINE.max(column + len));
                while !text.is_char_boundary(cut) {
                    cut -= 1;
                }
                let go = sink(Hit {
                    path: path.clone(),
                    line,
                    column,
                    len,
                    text: text[..cut].to_string(),
                });
                if !go {
                    stop = true;
                }
                Ok(go)
            }),
        );
        if let Err(e) = result {
            tracing::debug!("search {}: {e}", path.display());
        }
    }
    Ok(())
}

#[derive(Debug, Default)]
struct Found {
    hits: Vec<Hit>,
    done: bool,
    error: Option<String>,
}

/// A search running in the background.
#[derive(Debug)]
pub struct Search {
    /// What is searched for.
    pub query: Query,
    found: Arc<Mutex<Found>>,
    cancel: Arc<AtomicBool>,
}

impl Search {
    /// Starts searching `root` for `query`.
    pub fn start(root: &Path, ignore: &[String], query: Query) -> Search {
        let found = Arc::new(Mutex::new(Found::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let (root, ignore, q, f, c) = (
            root.to_path_buf(),
            ignore.to_vec(),
            query.clone(),
            found.clone(),
            cancel.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("kalem-project-search".into())
            .spawn(move || {
                let mut count = 0;
                let result = search(&root, &ignore, &q, &c, |hit| {
                    count += 1;
                    f.lock().expect("the results").hits.push(hit);
                    count < MAX_HITS
                });
                let mut f = f.lock().expect("the results");
                f.done = true;
                f.error = result.err();
            });
        if let Err(e) = spawned {
            let mut f = found.lock().expect("the results");
            f.done = true;
            f.error = Some(e.to_string());
        }
        Search {
            query,
            found,
            cancel,
        }
    }

    /// The hits from `from` on, whether the search is done, and its error.
    pub fn hits(&self, from: usize) -> (Vec<Hit>, bool, Option<String>) {
        let f = self.found.lock().expect("the results");
        (
            f.hits.get(from..).map(<[Hit]>::to_vec).unwrap_or_default(),
            f.done,
            f.error.clone(),
        )
    }

    /// Stops the search.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Waits until the search is done (for tests and the command line).
    pub fn wait(&self) -> (Vec<Hit>, Option<String>) {
        loop {
            let (hits, done, error) = self.hits(0);
            if done {
                return (hits, error);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

impl Drop for Search {
    fn drop(&mut self) {
        self.cancel();
    }
}
