//! Reuse of model data across versions of a document.
//!
//! After an incremental reparse, every subtree away from the edit is the
//! same green node as before (rowan shares them). The cache is keyed by the
//! identity of those green nodes, so a new version of the document only
//! computes the headlines on the path to the edit (T1.1.8).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use org_syntax::SyntaxNode;

use crate::outline::Entry;

type Keywords = Arc<Vec<(String, String)>>;
type Subtrees = HashMap<usize, (rowan::GreenNode, Arc<Vec<Entry>>)>;

/// Entries past this many make the cache start over.
const LIMIT: usize = 200_000;

/// Data shared by the versions of one document. Create one per open
/// document and pass it to [`crate::Document::with_cache`].
#[derive(Debug, Default)]
pub struct ModelCache {
    subtrees: Mutex<Subtrees>,
    keywords: Mutex<HashMap<usize, (rowan::GreenNode, Keywords)>>,
}

fn key(node: &SyntaxNode) -> usize {
    node.green() as *const rowan::GreenNodeData as usize
}

impl ModelCache {
    /// An empty cache.
    pub fn new() -> Arc<ModelCache> {
        Arc::new(ModelCache::default())
    }

    pub(crate) fn subtree(&self, node: &SyntaxNode) -> Option<Arc<Vec<Entry>>> {
        let map = self.subtrees.lock().ok()?;
        map.get(&key(node)).map(|(_, v)| v.clone())
    }

    pub(crate) fn put_subtree(&self, node: &SyntaxNode, v: Arc<Vec<Entry>>) {
        if let Ok(mut map) = self.subtrees.lock() {
            if map.len() > LIMIT {
                map.clear();
            }
            // The green node is kept alive with the entry, so its address
            // cannot be reused by another node while cached.
            map.insert(key(node), (node.green().to_owned(), v));
        }
    }

    pub(crate) fn keywords(&self, node: &SyntaxNode) -> Option<Keywords> {
        let map = self.keywords.lock().ok()?;
        map.get(&key(node)).map(|(_, v)| v.clone())
    }

    pub(crate) fn put_keywords(&self, node: &SyntaxNode, v: Keywords) {
        if let Ok(mut map) = self.keywords.lock() {
            if map.len() > LIMIT {
                map.clear();
            }
            map.insert(key(node), (node.green().to_owned(), v));
        }
    }
}
