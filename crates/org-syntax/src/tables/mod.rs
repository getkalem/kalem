//! Character classes and entity names as Org sees them.
//!
//! The tables are generated from Emacs by `tools/gen-tables.el`, so the
//! parser classifies characters exactly like `org-element.el` does in an
//! `org-mode` buffer.

mod case;
mod chars;
mod entities;
mod scripts;

use chars::CHAR_CLASSES;
pub(crate) use entities::ENTITIES;

pub(crate) const SYNTAX_WHITESPACE: u16 = 0;
pub(crate) const SYNTAX_WORD: u16 = 1;
pub(crate) const SYNTAX_PUNCT: u16 = 3;
pub(crate) const SYNTAX_OPEN: u16 = 4;
pub(crate) const SYNTAX_CLOSE: u16 = 5;
pub(crate) const SYNTAX_STRING: u16 = 6;
pub(crate) const ALNUM: u16 = 0x10;
pub(crate) const ALPHA: u16 = 0x20;
pub(crate) const PUNCT: u16 = 0x40;
pub(crate) const BLANK: u16 = 0x80;
pub(crate) const LINE_BREAKABLE: u16 = 0x100;

static ASCII: [u16; 128] = {
    let mut out = [0u16; 128];
    let mut i = 0;
    while i < CHAR_CLASSES.len() {
        let (first, last, bits) = CHAR_CLASSES[i];
        let mut c = first;
        while c <= last && c < 128 {
            out[c as usize] = bits;
            c += 1;
        }
        i += 1;
    }
    out
};

/// Returns the class bits of `c`.
#[inline]
pub(crate) fn bits(c: char) -> u16 {
    let c = c as u32;
    if c < 128 {
        return ASCII[c as usize];
    }
    match CHAR_CLASSES.binary_search_by(|&(first, last, _)| {
        if last < c {
            std::cmp::Ordering::Less
        } else if first > c {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    }) {
        Ok(i) => CHAR_CLASSES[i].2,
        Err(_) => 0,
    }
}

#[inline]
pub(crate) fn syntax(c: char) -> u16 {
    bits(c) & 0xF
}

/// `\s-`, `[[:space:]]`: whitespace syntax.
#[inline]
pub(crate) fn is_space(c: char) -> bool {
    syntax(c) == SYNTAX_WHITESPACE
}

/// `\w`, `[[:word:]]`: word syntax.
#[inline]
pub(crate) fn is_word(c: char) -> bool {
    syntax(c) == SYNTAX_WORD
}

/// `[[:alnum:]]`.
#[inline]
pub(crate) fn is_alnum(c: char) -> bool {
    bits(c) & ALNUM != 0
}

const CAT_COMBINING: u16 = 0x800;
const CAT_C: u16 = 0x1000;
const CAT_H: u16 = 0x2000;
const CAT_K: u16 = 0x4000;
const NO_CATEGORIES: u16 = 0x8000;

fn script(c: char) -> u8 {
    let c = c as u32;
    match scripts::SCRIPTS.binary_search_by(|&(first, last, _)| {
        if last < c {
            std::cmp::Ordering::Less
        } else if first > c {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    }) {
        Ok(i) => scripts::SCRIPTS[i].2,
        Err(_) => u8::MAX,
    }
}

/// Emacs's `word_boundary_p`: whether there is a word boundary between two
/// adjacent word-constituent characters. Characters of different scripts
/// are separated unless `word-combining-categories` joins them; characters
/// of the same script are joined unless `word-separating-categories`
/// separates them.
pub(crate) fn word_boundary(c1: char, c2: char) -> bool {
    // ASCII is one script without separating categories (checked against
    // the tables by a test).
    if c1.is_ascii() && c2.is_ascii() {
        return false;
    }
    word_boundary_tables(c1, c2)
}

fn word_boundary_tables(c1: char, c2: char) -> bool {
    let (b1, b2) = (bits(c1), bits(c2));
    let same = script(c1) == script(c2);
    let default = !same;
    if b1 & NO_CATEGORIES != 0 || b2 & NO_CATEGORIES != 0 {
        return default;
    }
    // (A . B) matches when (A is nil or (c1 in A and c2 not in A)) and
    // (B is nil or (c1 not in B and c2 in B)).
    let m = |a: Option<u16>, b: Option<u16>| {
        a.is_none_or(|a| b1 & a != 0 && b2 & a == 0) && b.is_none_or(|b| b1 & b == 0 && b2 & b != 0)
    };
    let rules: &[(Option<u16>, Option<u16>)] = if same {
        // word-separating-categories: ((?H . ?K))
        &[(Some(CAT_H), Some(CAT_K))]
    } else {
        // word-combining-categories: ((nil . ?^) (?^ . nil) (?C . ?H) (?C . ?K))
        &[
            (None, Some(CAT_COMBINING)),
            (Some(CAT_COMBINING), None),
            (Some(CAT_C), Some(CAT_H)),
            (Some(CAT_C), Some(CAT_K)),
        ]
    };
    if rules.iter().any(|&(a, b)| m(a, b)) {
        !default
    } else {
        default
    }
}

/// Builds the body of a regex character class (without brackets) that
/// matches every character whose bits satisfy `pred`.
pub(crate) fn class_body(pred: impl Fn(u16) -> bool) -> String {
    let mut out = String::new();
    let mut run: Option<(u32, u32)> = None;
    let push = |a: u32, b: u32, out: &mut String| {
        if a == b {
            out.push_str(&format!("\\x{{{a:X}}}"));
        } else {
            out.push_str(&format!("\\x{{{a:X}}}-\\x{{{b:X}}}"));
        }
    };
    for &(first, last, bits) in CHAR_CLASSES.iter() {
        if !pred(bits) {
            continue;
        }
        // Skip the surrogate gap, which cannot appear in Rust strings.
        let (first, last) =
            if (0xD800..=0xDFFF).contains(&first) && (0xD800..=0xDFFF).contains(&last) {
                continue;
            } else {
                (first, last)
            };
        match run {
            Some((a, b)) if b + 1 == first || (b == 0xD7FF && first == 0xE000) => {
                run = Some((a, last))
            }
            Some((a, b)) => {
                push(a, b, &mut out);
                run = Some((first, last));
            }
            None => run = Some((first, last)),
        }
    }
    if let Some((a, b)) = run {
        if a <= 0xD7FF && b >= 0xE000 {
            push(a, 0xD7FF, &mut out);
            push(0xE000, b, &mut out);
        } else {
            push(a, b, &mut out);
        }
    }
    out
}

fn map_case(s: &str, table: &[(char, &str)]) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match table.binary_search_by_key(&c, |e| e.0) {
            Ok(i) => out.push_str(table[i].1),
            Err(_) => out.push(c),
        }
    }
    out
}

/// Emacs's `upcase` on a string.
pub(crate) fn upcase(s: &str) -> String {
    if s.is_ascii() {
        s.to_ascii_uppercase()
    } else {
        map_case(s, &case::UPCASE)
    }
}

/// Emacs's `downcase` on a string.
pub(crate) fn downcase(s: &str) -> String {
    if s.is_ascii() {
        s.to_ascii_lowercase()
    } else {
        map_case(s, &case::DOWNCASE)
    }
}

/// Emacs's case-folding canon: two characters match under
/// `case-fold-search` when their canons are equal.
pub(crate) fn canon(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }
    match case::CANON.binary_search_by_key(&c, |e| e.0) {
        Ok(i) => case::CANON[i].1,
        Err(_) => c,
    }
}

/// Looks up an entity by name.
pub(crate) fn entity(
    name: &str,
) -> Option<&'static (
    &'static str,
    &'static str,
    bool,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
)> {
    let i = ENTITIES.partition_point(|e| e.0 < name);
    ENTITIES.get(i).filter(|e| e.0 == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_named_as_latex_operators_too() {
        // `\deg` and `\sup` are signs, as Org documents mean them.
        assert_eq!(entity("deg").map(|e| e.1), Some("\\textdegree{}"));
        assert_eq!(entity("sup").map(|e| e.1), Some("\\supset"));
        assert!(entity("nope").is_none());
    }

    #[test]
    fn ascii_classes() {
        assert!(is_space(' ') && is_space('\t') && is_space('\n'));
        assert!(is_word('a') && is_word('Z') && is_word('0'));
        assert!(is_word('\''), "text-mode gives the apostrophe word syntax");
        assert!(!is_word('-') && !is_word('_'));
        assert!(is_alnum('a') && !is_alnum('_'));
        assert!(bits('.') & PUNCT != 0 && bits('-') & PUNCT != 0);
    }

    #[test]
    fn unicode_classes() {
        for c in ['ç', 'ğ', 'ı', 'ş', 'ö', 'ü', 'İ', 'Ş', 'é', 'ß', 'α', 'ж'] {
            assert!(is_word(c), "{c}");
            assert!(is_alnum(c), "{c}");
            assert!(bits(c) & ALPHA != 0, "{c}");
        }
        assert!(bits('中') & LINE_BREAKABLE != 0);
    }

    #[test]
    fn emacs_case() {
        assert_eq!(upcase("title"), "TITLE");
        assert_eq!(upcase("tıtle"), "TıTLE", "Emacs leaves the dotless i alone");
        assert_eq!(upcase("straße"), "STRASSE");
        assert_eq!(downcase("MACRO"), "macro");
    }

    #[test]
    fn case_fold_canon() {
        assert_eq!(canon('Ç'), 'ç');
        assert_eq!(canon('ẞ'), canon('ß'));
        assert_eq!(
            canon('\u{212A}'),
            '\u{212A}',
            "Emacs does not fold the Kelvin sign"
        );
        assert_eq!(canon('ı'), 'ı', "Emacs keeps the dotless i apart from I");
        assert_eq!(canon('İ'), 'İ');
    }

    #[test]
    fn word_boundaries() {
        assert!(!word_boundary('a', 'b'));
        assert!(word_boundary('中', 's'), "Han and Latin are separate words");
        assert!(!word_boundary('ç', 'a'));
        // The ASCII shortcut agrees with the tables.
        for a in 0u8..128 {
            for b in 0u8..128 {
                assert!(!word_boundary_tables(a as char, b as char), "{a} {b}");
            }
        }
    }

    #[test]
    fn entities() {
        assert_eq!(entity("alpha").unwrap().6, "α");
        assert!(entity("nosuchentity").is_none());
    }
}
