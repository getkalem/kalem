//! Localization (§7.4): the user interface in English and Turkish with
//! Fluent, shared by both frontends.
//!
//! The strings live in `locales/<language>/kalem.ftl` and are built in.
//! [`set_language`] picks the language (`auto` follows the system); a
//! string missing from a translation falls back to English, and a missing
//! English string to its ID. Frontends call [`tr`] or the [`tr!`] macro.
//!
//! Only Kalem's own interface is translated: the messages of Org commands
//! (`org-edit`) stay those of Emacs, and plugins bring their own strings.
//! Nothing is translated until a frontend sets the language, so tests see
//! English.

use std::sync::{LazyLock, RwLock};

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

/// The languages with a translation, as (code, name in that language).
pub const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("tr", "Türkçe")];

fn source(lang: &str) -> Option<&'static str> {
    Some(match lang {
        "en" => include_str!("../locales/en/kalem.ftl"),
        "tr" => include_str!("../locales/tr/kalem.ftl"),
        _ => return None,
    })
}

fn bundle(lang: &str) -> Option<FluentBundle<FluentResource>> {
    let text = source(lang)?;
    let id: LanguageIdentifier = lang.parse().ok()?;
    let res = FluentResource::try_new(text.to_string()).unwrap_or_else(|(r, e)| {
        tracing::warn!(lang, errors = ?e, "errors in a translation");
        r
    });
    let mut b = FluentBundle::new_concurrent(vec![id]);
    // Plain text: no bidi isolation marks around arguments.
    b.set_use_isolating(false);
    let _ = b.add_resource(res);
    Some(b)
}

struct Strings {
    lang: String,
    bundle: Option<FluentBundle<FluentResource>>,
}

static ENGLISH: LazyLock<FluentBundle<FluentResource>> =
    LazyLock::new(|| bundle("en").expect("the English strings"));

static CURRENT: LazyLock<RwLock<Strings>> = LazyLock::new(|| {
    RwLock::new(Strings {
        lang: "en".into(),
        bundle: None,
    })
});

/// The language of `setting` (`auto`, `en`, `tr`, or a locale such as
/// `tr-TR`): a language with a translation, English otherwise.
pub fn resolve(setting: &str) -> &'static str {
    let wanted = if setting.is_empty() || setting == "auto" {
        sys_locale::get_locale().unwrap_or_default()
    } else {
        setting.to_string()
    };
    let primary = wanted
        .split(['-', '_', '.'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    LANGUAGES
        .iter()
        .map(|(code, _)| *code)
        .find(|c| *c == primary)
        .unwrap_or("en")
}

/// Sets the interface language from the `ui.language` setting.
pub fn set_language(setting: &str) {
    let lang = resolve(setting);
    let mut s = CURRENT.write().unwrap_or_else(|e| e.into_inner());
    if s.lang != lang || (lang != "en" && s.bundle.is_none()) {
        s.bundle = (lang != "en").then(|| bundle(lang)).flatten();
        s.lang = lang.to_string();
    }
}

/// The interface language's code.
pub fn language() -> String {
    CURRENT
        .read()
        .map(|s| s.lang.clone())
        .unwrap_or_else(|_| "en".into())
}

/// An argument of a message.
#[derive(Debug, Clone)]
pub enum Arg {
    /// Text.
    Str(String),
    /// A number, which plural rules look at.
    Num(f64),
}

impl From<&str> for Arg {
    fn from(s: &str) -> Arg {
        Arg::Str(s.to_string())
    }
}

impl From<String> for Arg {
    fn from(s: String) -> Arg {
        Arg::Str(s)
    }
}

impl From<&String> for Arg {
    fn from(s: &String) -> Arg {
        Arg::Str(s.clone())
    }
}

impl From<usize> for Arg {
    fn from(n: usize) -> Arg {
        Arg::Num(n as f64)
    }
}

impl From<u32> for Arg {
    fn from(n: u32) -> Arg {
        Arg::Num(f64::from(n))
    }
}

impl From<i32> for Arg {
    fn from(n: i32) -> Arg {
        Arg::Num(f64::from(n))
    }
}

impl From<i64> for Arg {
    fn from(n: i64) -> Arg {
        Arg::Num(n as f64)
    }
}

fn format(b: &FluentBundle<FluentResource>, id: &str, args: &[(&str, Arg)]) -> Option<String> {
    let pattern = b.get_message(id)?.value()?;
    let mut fa = FluentArgs::new();
    for (k, v) in args {
        match v {
            Arg::Str(s) => fa.set(*k, FluentValue::from(s.as_str())),
            Arg::Num(n) => fa.set(*k, FluentValue::from(*n)),
        }
    }
    let mut errors = Vec::new();
    let out = b.format_pattern(pattern, Some(&fa), &mut errors);
    Some(out.into_owned())
}

/// Message `id` with `args` in language `lang` (a code of [`LANGUAGES`]),
/// falling back to English and then to the ID.
pub fn tr_in(lang: &str, id: &str, args: &[(&str, Arg)]) -> String {
    let own = (lang != "en")
        .then(|| bundle(lang))
        .flatten()
        .and_then(|b| format(&b, id, args));
    own.or_else(|| format(&ENGLISH, id, args))
        .unwrap_or_else(|| id.to_string())
}

/// Message `id` with `args` in the interface language.
pub fn tr_args(id: &str, args: &[(&str, Arg)]) -> String {
    let s = CURRENT.read().unwrap_or_else(|e| e.into_inner());
    s.bundle
        .as_ref()
        .and_then(|b| format(b, id, args))
        .or_else(|| format(&ENGLISH, id, args))
        .unwrap_or_else(|| id.to_string())
}

/// Message `id` in the interface language.
pub fn tr(id: &str) -> String {
    tr_args(id, &[])
}

/// A message in the interface language: `tr!("msg-saved")`, or with
/// arguments `tr!("msg-saved-as", path = p.display().to_string())`.
#[macro_export]
macro_rules! tr {
    ($id:expr) => {
        $crate::l10n::tr($id)
    };
    ($id:expr, $($k:ident = $v:expr),+ $(,)?) => {
        $crate::l10n::tr_args($id, &[$((stringify!($k), $crate::l10n::Arg::from($v))),+])
    };
}

/// The message ID of a built-in command's title.
pub fn command_key(id: &str) -> String {
    format!("cmd-{}", id.replace('.', "-"))
}

/// A number with digit groups: `87,879` in English, `87.879` in Turkish.
pub fn number(n: usize) -> String {
    let sep = tr("number-group-separator");
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push_str(&sep);
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(lang: &str) -> Vec<String> {
        source(lang)
            .unwrap()
            .lines()
            .filter(|l| !l.starts_with(['#', ' ']) && l.contains(" = "))
            .map(|l| l.split(" = ").next().unwrap().to_string())
            .collect()
    }

    #[test]
    fn translations_are_complete() {
        let en = keys("en");
        let tr = keys("tr");
        let missing: Vec<&String> = en.iter().filter(|k| !tr.contains(k)).collect();
        let extra: Vec<&String> = tr.iter().filter(|k| !en.contains(k)).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "{missing:?} {extra:?}"
        );
        // Every built-in command has a title.
        let reg = crate::CommandRegistry::with_builtins();
        for c in reg.commands() {
            let k = command_key(&c.id);
            assert!(en.contains(&k), "{k}");
            assert_eq!(tr_in("en", &k, &[]), c.title, "{k}");
        }
        // No message is defined twice (Fluent keeps the first).
        for lang in ["en", "tr"] {
            let k = keys(lang);
            let mut seen = std::collections::HashSet::new();
            let twice: Vec<&String> = k.iter().filter(|x| !seen.insert(*x)).collect();
            assert!(twice.is_empty(), "{lang}: {twice:?}");
        }
        // Both files parse without errors.
        for (lang, _) in LANGUAGES {
            assert!(
                FluentResource::try_new(source(lang).unwrap().to_string()).is_ok(),
                "{lang}"
            );
        }
    }

    #[test]
    fn messages() {
        assert_eq!(tr_in("tr", "cmd-app-save", &[]), "Kaydet");
        assert_eq!(
            tr_in("en", "msg-saved-as", &[("path", "a.org".into())]),
            "Saved a.org"
        );
        assert_eq!(
            tr_in(
                "en",
                "status-words",
                &[("count", 1usize.into()), ("shown", "1".into())]
            ),
            "1 word"
        );
        assert_eq!(
            tr_in(
                "tr",
                "status-words",
                &[("count", 2usize.into()), ("shown", "2".into())]
            ),
            "2 kelime"
        );
        // Missing: English, then the ID.
        assert_eq!(tr_in("xx", "msg-copied", &[]), "Copied");
        assert_eq!(tr_in("tr", "no-such-message", &[]), "no-such-message");
        assert_eq!(resolve("tr_TR.UTF-8"), "tr");
        assert_eq!(resolve("de-DE"), "en");
        // Nothing set: English.
        assert_eq!(tr("msg-copied"), "Copied");
        assert_eq!(number(1234567), "1,234,567");
    }
}
