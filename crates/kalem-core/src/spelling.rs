//! Spelling with Hunspell dictionaries (`LANG.aff` and `LANG.dic`): the
//! user's own in Kalem's configuration folder (`dictionaries/`), the
//! system's (`/usr/share/hunspell` and the like), or LibreOffice's. Words
//! added are kept in `dictionaries/words.txt`, for every language.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

static FOLDER: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

/// The user's dictionaries kept in `folder` instead of the configuration's
/// (tests, which must not touch the user's).
pub fn use_folder(folder: &Path) {
    *FOLDER.write().unwrap_or_else(|e| e.into_inner()) = Some(folder.to_path_buf());
}

/// The folders searched for dictionaries, the user's first.
fn folders() -> Vec<PathBuf> {
    let mut out = vec![dictionary_folder()];
    if let Some(h) = std::env::var_os("HOME").map(PathBuf::from) {
        out.push(h.join("Library/Spelling"));
        out.push(h.join(".local/share/hunspell"));
    }
    for d in [
        "/Library/Spelling",
        "/usr/share/hunspell",
        "/usr/share/myspell",
        "/usr/share/myspell/dicts",
        "/usr/local/share/hunspell",
        "/opt/homebrew/share/hunspell",
    ] {
        out.push(PathBuf::from(d));
    }
    // LibreOffice's, each language in a folder of its own.
    for app in [
        "/Applications/LibreOffice.app/Contents/Resources/extensions",
        "/usr/lib/libreoffice/share/extensions",
        "/opt/libreoffice/share/extensions",
    ] {
        if let Ok(entries) = std::fs::read_dir(app) {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with("dict-") {
                    out.push(e.path());
                }
            }
        }
    }
    out
}

/// The dictionaries found: language (`en_US`), `.aff` and `.dic`.
pub fn dictionaries() -> Vec<(String, PathBuf, PathBuf)> {
    let mut out: Vec<(String, PathBuf, PathBuf)> = Vec::new();
    for dir in folders() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut found: Vec<(String, PathBuf, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                (p.extension()? == "aff").then_some(())?;
                let lang = p.file_stem()?.to_string_lossy().into_owned();
                let dic = p.with_extension("dic");
                dic.exists().then_some((lang, p, dic))
            })
            .collect();
        found.sort();
        for f in found {
            if !out.iter().any(|o| o.0 == f.0) {
                out.push(f);
            }
        }
    }
    out
}

/// The language spelling uses unless told: the user's (from `LANG`), else
/// American English, else the first found.
pub fn default_language() -> Option<String> {
    let all = dictionaries();
    let want = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
        .map(|v| v.split('.').next().unwrap_or_default().replace('-', "_"));
    if let Some(w) = want {
        let lang = w.split('_').next().unwrap_or_default().to_owned();
        if let Some(d) = all.iter().find(|d| d.0 == w).or_else(|| {
            all.iter()
                .find(|d| d.0 == lang || d.0.starts_with(&format!("{lang}_")))
        }) {
            return Some(d.0.clone());
        }
    }
    all.iter()
        .find(|d| d.0 == "en_US")
        .or_else(|| all.first())
        .map(|d| d.0.clone())
}

/// A loaded dictionary.
pub type Speller = Arc<Mutex<spellbook::Dictionary>>;

/// The dictionary of `lang`, read once, with the user's words.
pub fn speller(lang: &str) -> Result<Speller, String> {
    static LOADED: OnceLock<Mutex<HashMap<String, Speller>>> = OnceLock::new();
    let loaded = LOADED.get_or_init(Default::default);
    let mut map = loaded.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(d) = map.get(lang) {
        return Ok(d.clone());
    }
    let Some((_, aff, dic)) = dictionaries().into_iter().find(|d| d.0 == lang) else {
        return Err(format!(
            "No {lang} dictionary: put {lang}.aff and {lang}.dic (Hunspell's) into {}",
            dictionary_folder().display()
        ));
    };
    let read = |p: &Path| std::fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned());
    let (aff, dic) = (
        read(&aff).map_err(|e| e.to_string())?,
        read(&dic).map_err(|e| e.to_string())?,
    );
    let mut d = spellbook::Dictionary::new(&aff, &dic).map_err(|e| e.to_string())?;
    for w in user_words() {
        let _ = d.add(&w);
    }
    let d = Arc::new(Mutex::new(d));
    map.insert(lang.to_owned(), d.clone());
    Ok(d)
}

/// The user's own dictionaries folder.
pub fn dictionary_folder() -> PathBuf {
    if let Some(f) = FOLDER.read().unwrap_or_else(|e| e.into_inner()).clone() {
        return f;
    }
    crate::settings::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dictionaries")
}

fn user_words() -> Vec<String> {
    std::fs::read_to_string(dictionary_folder().join("words.txt"))
        .map(|t| {
            t.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// A word added to the user's dictionary (and to `speller`).
pub fn add_word(speller: &Speller, word: &str) -> Result<(), String> {
    let dir = dictionary_folder();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut words = user_words();
    if !words.iter().any(|w| w == word) {
        words.push(word.to_owned());
        std::fs::write(dir.join("words.txt"), words.join("\n") + "\n")
            .map_err(|e| e.to_string())?;
    }
    let mut d = speller.lock().unwrap_or_else(|e| e.into_inner());
    d.add(word).map_err(|e| e.to_string())
}

/// The words of a text worth checking (letters, no digits), with their
/// places.
pub fn words(text: &str) -> Vec<(usize, &str)> {
    use unicode_segmentation::UnicodeSegmentation;
    text.split_word_bound_indices()
        .filter(|(_, w)| {
            w.chars().any(char::is_alphabetic) && !w.chars().any(|c| c.is_ascii_digit())
        })
        .collect()
}

/// Whether `speller` knows `word`.
pub fn check(speller: &Speller, word: &str) -> bool {
    speller
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .check(word)
}

/// What `speller` offers for `word`, the likeliest first.
pub fn suggest(speller: &Speller, word: &str) -> Vec<String> {
    let mut out = Vec::new();
    speller
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .suggest(word, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_of_a_cell() {
        let w: Vec<&str> = words("Kira 2026: ödendi, x2 değil")
            .into_iter()
            .map(|x| x.1)
            .collect();
        assert_eq!(w, ["Kira", "ödendi", "değil"]);
    }
}
