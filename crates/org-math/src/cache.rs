//! Rendered formulas by their request, least recently used ones dropped.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::{Image, MathEngine, MathError, Request};

type Key = (String, bool, u32, u32, [u8; 4]);
type Entry = Arc<Result<Image, MathError>>;

/// A cache of rendered formulas in front of an engine; shared between
/// threads.
pub struct Cache {
    engine: Box<dyn MathEngine>,
    inner: Mutex<Inner>,
    capacity: usize,
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cache")
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct Inner {
    map: HashMap<Key, (Entry, u64)>,
    clock: u64,
}

fn key(r: &Request) -> Key {
    (
        r.latex.clone(),
        r.display,
        r.size.to_bits(),
        r.scale.to_bits(),
        r.color,
    )
}

impl Cache {
    /// A cache of at most `capacity` formulas in front of `engine`.
    pub fn new(engine: Box<dyn MathEngine>, capacity: usize) -> Cache {
        Cache {
            engine,
            inner: Mutex::default(),
            capacity: capacity.max(1),
        }
    }

    /// The rendered formula, from the cache or rendered now.
    pub fn get(&self, request: &Request) -> Entry {
        let k = key(request);
        if let Ok(mut inner) = self.inner.lock() {
            inner.clock += 1;
            let now = inner.clock;
            if let Some((e, used)) = inner.map.get_mut(&k) {
                *used = now;
                return e.clone();
            }
        }
        let e = Arc::new(self.engine.render(request));
        if let Ok(mut inner) = self.inner.lock() {
            if inner.map.len() >= self.capacity {
                // Drop the least recently used eighth.
                let mut ages: Vec<u64> = inner.map.values().map(|(_, u)| *u).collect();
                ages.sort_unstable();
                let cut = ages[(ages.len() / 8).min(ages.len() - 1)];
                inner.map.retain(|_, (_, u)| *u > cut);
            }
            let now = inner.clock;
            inner.map.insert(k, (e.clone(), now));
        }
        e
    }

    /// The rendered formula if it is in the cache.
    pub fn peek(&self, request: &Request) -> Option<Entry> {
        let inner = self.inner.lock().ok()?;
        inner.map.get(&key(request)).map(|(e, _)| e.clone())
    }

    /// How many formulas are cached.
    pub fn len(&self) -> usize {
        self.inner.lock().map_or(0, |i| i.map.len())
    }

    /// Whether nothing is cached.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
