//! The font families installed, which the settings panel steps through
//! for `editor.font_family` and `editor.code_font_family` so that a font
//! is chosen rather than typed.

use std::sync::OnceLock;

/// The families of the fonts installed, sorted; only monospace ones with
/// `monospace`. Read once (a few hundred milliseconds with many fonts):
/// [`prefetch`] reads them on another thread before they are needed.
pub fn families(monospace: bool) -> &'static [String] {
    static ALL: OnceLock<(Vec<String>, Vec<String>)> = OnceLock::new();
    let (all, mono) = ALL.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut all = Vec::new();
        let mut mono = Vec::new();
        for face in db.faces() {
            let Some((name, _)) = face.families.first() else {
                continue;
            };
            // Fonts the system keeps for itself.
            if name.starts_with('.') {
                continue;
            }
            all.push(name.clone());
            if face.monospaced {
                mono.push(name.clone());
            }
        }
        for list in [&mut all, &mut mono] {
            list.sort_by_key(|n| n.to_lowercase());
            list.dedup();
        }
        (all, mono)
    });
    if monospace { mono } else { all }
}

/// Reads the font families on another thread, so that the settings
/// panel has them when a font is stepped through.
pub fn prefetch() {
    let _ = std::thread::Builder::new()
        .name("kalem-fonts".into())
        .spawn(|| {
            let _ = families(false);
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn families_sorted_monospace_among_them() {
        // Whatever fonts the machine has (none on some servers).
        let all = super::families(false);
        let mono = super::families(true);
        let key = |n: &String| n.to_lowercase();
        assert!(all.windows(2).all(|w| key(&w[0]) <= key(&w[1])));
        assert!(mono.iter().all(|m| all.contains(m)));
        assert!(all.iter().all(|n| !n.starts_with('.')));
    }
}
