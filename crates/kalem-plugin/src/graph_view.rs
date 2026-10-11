//! Documents drawn as graphs of nodes and edges (API 0.2.12): the plugin
//! gives the nodes and edges and a text, the editor lays them out and
//! draws them, and opening a node runs the graph's command with
//! `{"node": id}`.
//!
//! ```ignore
//! use kalem_plugin::graph_view::{self, Edge, Graph, Node};
//! let g = Graph {
//!     nodes: vec![graph_view::node("a", "A", "pages", 3), graph_view::node("b", "B", "pages", 1)],
//!     edges: vec![Edge { from: "a".into(), to: "b".into() }],
//!     focus: None,
//!     command: "notes.open".into(),
//! };
//! let spec = kalem_plugin::documents::Spec::new("notes.graph", "", "Graph", "notes-graph");
//! graph_view::open(&spec, &g, "A\nB\n")?;
//! ```

use crate::extension::kalem::plugin::graph_view as api;
pub use crate::extension::kalem::plugin::graph_view::{Edge, Graph, Node};

/// A node.
pub fn node(id: &str, label: &str, group: &str, weight: u32) -> Node {
    Node {
        id: id.to_string(),
        label: label.to_string(),
        group: group.to_string(),
        weight,
    }
}

/// Shows `g` as the plugin's document `spec`, `text` its source; its
/// number.
pub fn open(spec: &crate::documents::Spec, g: &Graph, text: &str) -> Result<u64, String> {
    api::open(spec.inner(), g, text)
}

/// Writes document `doc` anew, the nodes still there keeping their places.
pub fn set(doc: u64, g: &Graph, text: &str) -> Result<(), String> {
    api::set(doc, g, text)
}
