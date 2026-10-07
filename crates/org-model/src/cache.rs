//! Reuse of model data across versions of a document.
//!
//! After an incremental reparse, every subtree away from the edit is the
//! same green node as before (rowan shares them). The cache is keyed by the
//! identity of those green nodes, so a new version of the document only
//! computes the headlines on the path to the edit (T1.1.8).
//!
//! A cached headline holds its child headlines' values instead of copies,
//! so a lookup that hits stops there. Each build of the outline or of the
//! keywords is a [`Pass`]: it marks what it looks up or adds, and when it
//! ends, what neither it nor the pass before used, and no cached value
//! holds, is dropped. The cache thus holds about one version of the
//! document, plus the path to the last edit in the version before.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use org_syntax::SyntaxNode;

use crate::info::Keywords;
use crate::outline::Subtree;

/// Data shared by the versions of one document. Create one per open
/// document and pass it to [`crate::Document::with_cache`].
#[derive(Debug, Default)]
pub struct ModelCache {
    pub(crate) subtrees: Store<Subtree>,
    pub(crate) keywords: Store<Keywords>,
}

impl ModelCache {
    /// An empty cache.
    pub fn new() -> Arc<ModelCache> {
        Arc::new(ModelCache::default())
    }
}

fn key(node: &SyntaxNode) -> usize {
    node.green() as *const rowan::GreenNodeData as usize
}

/// Hashes the addresses that key the cache: a multiply, its high bits
/// folded into the low ones the table indexes by (an address's low bits
/// are always zero). SipHash cost about a tenth of a build of thousands
/// of headlines.
#[derive(Debug, Default, Clone, Copy)]
struct AddressHasher(u64);

impl std::hash::Hasher for AddressHasher {
    fn finish(&self) -> u64 {
        self.0 ^ (self.0 >> 32)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }

    fn write_usize(&mut self, n: usize) {
        self.0 = (self.0 ^ n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

type Map<T> = HashMap<usize, Slot<T>, std::hash::BuildHasherDefault<AddressHasher>>;

/// A cached value, with the green node it was computed from and the last
/// pass that used it.
#[derive(Debug)]
struct Slot<T> {
    /// Kept alive with the value, so that its address cannot be reused by
    /// another node while cached.
    _green: rowan::GreenNode,
    value: Arc<T>,
    used: u64,
}

#[derive(Debug)]
struct Slots<T> {
    map: Map<T>,
    /// The number of the last pass begun.
    passes: u64,
    /// Values computed, for tests to tell what was reused.
    #[cfg(test)]
    added: usize,
}

/// Values keyed by green node.
#[derive(Debug)]
pub(crate) struct Store<T> {
    slots: Mutex<Slots<T>>,
}

impl<T> Default for Store<T> {
    fn default() -> Self {
        Store {
            slots: Mutex::new(Slots {
                map: Map::default(),
                passes: 0,
                #[cfg(test)]
                added: 0,
            }),
        }
    }
}

impl<T> Store<T> {
    /// Begins a build that reads and fills the store.
    pub(crate) fn pass(&self) -> Pass<'_, T> {
        let n = match self.slots.lock() {
            Ok(mut s) => {
                s.passes += 1;
                s.passes
            }
            Err(_) => 0,
        };
        Pass { store: self, n }
    }

    /// The number of cached values.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.slots.lock().map_or(0, |s| s.map.len())
    }

    /// The number of values computed so far.
    #[cfg(test)]
    pub(crate) fn added(&self) -> usize {
        self.slots.lock().map_or(0, |s| s.added)
    }
}

/// One build reading and filling a [`Store`]. Dropping it ends the build.
#[derive(Debug)]
pub(crate) struct Pass<'a, T> {
    store: &'a Store<T>,
    n: u64,
}

impl<T> Pass<'_, T> {
    /// The value cached for `node`, marked as used by this pass.
    pub(crate) fn get(&self, node: &SyntaxNode) -> Option<Arc<T>> {
        let mut s = self.store.slots.lock().ok()?;
        let slot = s.map.get_mut(&key(node))?;
        slot.used = slot.used.max(self.n);
        Some(slot.value.clone())
    }

    pub(crate) fn put(&self, node: &SyntaxNode, value: Arc<T>) {
        if let Ok(mut s) = self.store.slots.lock() {
            #[cfg(test)]
            {
                s.added += 1;
            }
            s.map.insert(
                key(node),
                Slot {
                    _green: node.green().to_owned(),
                    value,
                    used: self.n,
                },
            );
        }
    }
}

impl<T> Drop for Pass<'_, T> {
    /// Drops what neither this pass nor the one before used (a frontend
    /// that builds two versions in turn finds both), unless another value
    /// still holds it: a headline found in the cache was not looked into.
    /// Dropping a headline can free its children, hence the loop.
    fn drop(&mut self) {
        if let Ok(mut s) = self.store.slots.lock() {
            let n = self.n;
            loop {
                let before = s.map.len();
                s.map
                    .retain(|_, slot| slot.used + 1 >= n || Arc::strong_count(&slot.value) > 1);
                if s.map.len() == before {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use org_syntax::{Parse, TextEdit, TextRange, TextSize};

    use crate::{Document, ModelCache, Settings};

    /// Inserts `x` at `at` and builds the outline and the keywords of the
    /// new version.
    fn type_x(text: &mut String, parse: &mut Parse, at: usize, cache: &Arc<ModelCache>) {
        let edit = TextEdit {
            range: TextRange::empty(TextSize::from(at as u32)),
            insert: "x".into(),
        };
        let new_text = edit.apply(text);
        *parse = parse.reparse(&new_text, &edit);
        *text = new_text;
        build(parse, cache);
    }

    fn build(parse: &Parse, cache: &Arc<ModelCache>) {
        let settings = Arc::new(Settings::default());
        let d = Document::with_cache(parse.clone(), settings, None, cache.clone());
        let _ = (d.outline(), d.info());
    }

    /// Typing into one of many headlines keeps the cache to about one
    /// version of the document, and computes only the path to the edit.
    #[test]
    fn cache_stays_bounded_across_versions() {
        const HEADLINES: usize = 2000;
        let mut text = String::from("* Root\n");
        for i in 0..HEADLINES {
            text.push_str(&format!("** Headline {i} :tag:\nText {i}.\n#+KEY: {i}\n"));
        }
        let cache = ModelCache::new();
        let mut parse = org_syntax::parse(&text);
        let start = text.find("Text 1000.").unwrap_or(0);
        for at in (start..).take(100) {
            let added = (cache.subtrees.added(), cache.keywords.added());
            type_x(&mut text, &mut parse, at, &cache);
            // This version's root and children, the last version's root
            // and edited child.
            assert!(cache.subtrees.len() <= HEADLINES + 3);
            assert!(cache.keywords.len() <= HEADLINES + 3);
            if added != (0, 0) {
                assert_eq!(cache.subtrees.added() - added.0, 2);
                assert_eq!(cache.keywords.added() - added.1, 2);
            }
        }
        let d = Document::with_cache(parse.clone(), Arc::default(), None, cache.clone());
        assert_eq!(
            d.outline(),
            &crate::Outline::new(&parse.syntax(), parse.context())
        );
        assert_eq!(d.info().keywords.len(), HEADLINES);
        // A full parse shares no node with the last version: both are kept
        // until the next pass.
        let fresh = org_syntax::parse(&text);
        for _ in 0..2 {
            build(&fresh, &cache);
            assert!(cache.subtrees.len() <= 2 * (HEADLINES + 1));
            assert!(cache.keywords.len() <= 2 * (HEADLINES + 1));
        }
        assert_eq!(cache.subtrees.len(), HEADLINES + 1);
        assert_eq!(cache.keywords.len(), HEADLINES + 1);
    }

    /// Two versions built in turn both stay cached.
    #[test]
    fn two_versions_in_turn() {
        let a = org_syntax::parse("* A\n** B\n* C\n");
        let b = org_syntax::parse("* A\n** B\n* C\n** D\n");
        let cache = ModelCache::new();
        build(&a, &cache);
        build(&b, &cache);
        assert_eq!(cache.subtrees.added(), 7);
        for p in [&a, &b, &a, &b, &a] {
            build(p, &cache);
        }
        assert_eq!(cache.subtrees.added(), 7);
        assert_eq!(cache.subtrees.len(), 7);
    }

    /// Headlines below one found in the cache stay cached, though no pass
    /// looked them up: typing moves from one branch to another.
    #[test]
    fn edit_moving_to_another_branch() {
        let mut text = String::new();
        for top in ["One", "Two"] {
            text.push_str(&format!("* {top}\n"));
            for i in 0..100 {
                text.push_str(&format!("** {top} {i}\n#+KEY: {i}\n"));
            }
        }
        let cache = ModelCache::new();
        let mut parse = org_syntax::parse(&text);
        build(&parse, &cache);
        let start = text.find("** Two 50").unwrap_or(0) + 3;
        for at in (start..).take(5) {
            type_x(&mut text, &mut parse, at, &cache);
        }
        let added = (cache.subtrees.added(), cache.keywords.added());
        let at = text.find("** One 50").unwrap_or(0) + 3;
        type_x(&mut text, &mut parse, at, &cache);
        assert_eq!(cache.subtrees.added() - added.0, 2);
        assert_eq!(cache.keywords.added() - added.1, 2);
        assert!(cache.subtrees.len() <= 202 + 2);
    }
}
