//! A plugin's document drawn as a graph of nodes and edges (plugin API
//! 0.2.12, the `graph-view` interface): a notes graph's pages and links, a
//! project's dependencies. The plugin gives the nodes and edges; the core
//! lays them out with forces (nodes push each other away, edges pull their
//! ends together, a weak pull to the middle), a few steps a frame until the
//! layout settles, keeps where each node is when the plugin writes the
//! graph again, and maps the layout to the window (pan and zoom). The
//! editors draw it and hand back the node opened.
//!
//! Pure: the layout is deterministic for a graph, so that a test can check
//! it and the same graph opens the same way.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A node of a graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The plugin's name for it, handed back when it is opened.
    pub id: String,
    /// What is written beside it.
    pub label: String,
    /// Its group: nodes of a group share a color (a namespace, a tag,
    /// journals).
    pub group: String,
    /// How large it is drawn, 1 the least (a page's links).
    pub weight: u32,
}

/// A graph as the plugin gave it, its edges by the nodes' places.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Graph {
    /// The nodes.
    pub nodes: Vec<Node>,
    /// The edges, each once, between two different nodes.
    pub edges: Vec<(usize, usize)>,
    /// The node marked and kept in the middle (a local graph's page).
    pub focus: Option<usize>,
    /// The plugin's command run with `{"node": id}` when a node is opened.
    pub command: String,
}

impl Graph {
    /// The graph of `nodes` and the edges between the nodes named, those
    /// naming no node, a node itself or one already given left out.
    pub fn new(
        nodes: Vec<Node>,
        edges: &[(String, String)],
        focus: Option<&str>,
        command: String,
    ) -> Graph {
        let mut at: HashMap<&str, usize> = HashMap::with_capacity(nodes.len());
        for (i, n) in nodes.iter().enumerate() {
            at.entry(n.id.as_str()).or_insert(i);
        }
        let mut seen = std::collections::HashSet::new();
        let edges: Vec<(usize, usize)> = edges
            .iter()
            .filter_map(|(a, b)| {
                let (a, b) = (*at.get(a.as_str())?, *at.get(b.as_str())?);
                let key = (a.min(b), a.max(b));
                (a != b && seen.insert(key)).then_some(key)
            })
            .collect();
        let focus = focus.and_then(|f| at.get(f).copied());
        Graph {
            nodes,
            edges,
            focus,
            command,
        }
    }
}

/// The ideal length of an edge, in the layout's units.
const SPRING: f32 = 40.0;
/// How strongly nodes push each other away.
const REPULSION: f32 = SPRING * SPRING;
/// The pull towards the middle, which keeps parts from drifting apart:
/// a graph of n nodes spreads to about `SPRING * sqrt(n / GRAVITY)`.
const GRAVITY: f32 = 0.6;
/// How far a node may move in a step at the start; it cools down.
const START_HEAT: f32 = SPRING * 2.0;
/// Below this a layout is settled.
const COLD: f32 = 0.05;
/// How quickly the layout cools.
const COOLING: f32 = 0.975;
/// Barnes–Hut's opening angle: a cell farther than its size over this is
/// one body.
const THETA: f32 = 0.9;

/// A graph laid out and seen through a window.
#[derive(Debug, Clone)]
pub struct GraphView {
    graph: Arc<Graph>,
    /// Each node's place.
    pos: Vec<[f32; 2]>,
    /// Each node's neighbors.
    neighbors: Vec<Vec<usize>>,
    /// Each node's group's rank, the most common first: its color.
    ranks: Vec<usize>,
    /// How far a node may still move in a step.
    heat: f32,
    /// The point of the layout in the middle of the window.
    pub center: [f32; 2],
    /// Pixels per unit of the layout.
    pub zoom: f32,
    /// Whether the user moved the view (panned, zoomed, dragged): until
    /// then it is fitted to the layout as the layout spreads.
    moved: bool,
    /// The weight a node needs for its label to be written at any zoom:
    /// the heaviest dozen's.

    /// The node under the mouse.
    pub hovered: Option<usize>,
    /// The node chosen (clicked, or stepped to with the keys).
    pub selected: Option<usize>,
    /// The node dragged, held where the mouse is.
    held: Option<usize>,
}

/// A stable hash of `s` (FNV-1a), for the first places and the colors.
fn hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// A first place for node `i` of `n`: on a spiral, by its id's hash, so
/// that a graph opens the same way each time.
fn first_place(i: usize, id: &str) -> [f32; 2] {
    let h = hash(id);
    let r = SPRING * (i as f32).sqrt() * 0.8 + SPRING;
    let a = (i as f32) * 2.399_963 + (h % 1000) as f32 * 0.001;
    let jitter = ((h >> 16) % 100) as f32 * 0.1;
    [(r + jitter) * a.cos(), (r + jitter) * a.sin()]
}

impl GraphView {
    /// A view of `graph`, laid out from scratch.
    pub fn new(graph: Arc<Graph>) -> GraphView {
        let pos = (0..graph.nodes.len())
            .map(|i| first_place(i, &graph.nodes[i].id))
            .collect();
        let mut v = GraphView {
            neighbors: neighbors(&graph),
            ranks: ranks(&graph),
            graph,
            pos,
            heat: START_HEAT,
            center: [0.0, 0.0],
            zoom: 1.0,
            moved: false,
            hovered: None,
            selected: None,
            held: None,
        };
        v.selected = v.graph.focus;
        v
    }

    /// The graph written again: the nodes still there keep their places,
    /// a new one starts beside its neighbors, and the layout warms up.
    pub fn update(&mut self, graph: Arc<Graph>) {
        if graph == self.graph {
            return;
        }
        let old: HashMap<&str, usize> = self
            .graph
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect();
        let neighbors = neighbors(&graph);
        let mut pos: Vec<Option<[f32; 2]>> = graph
            .nodes
            .iter()
            .map(|n| old.get(n.id.as_str()).map(|&i| self.pos[i]))
            .collect();
        for i in 0..pos.len() {
            if pos[i].is_some() {
                continue;
            }
            let placed: Vec<[f32; 2]> = neighbors[i].iter().filter_map(|&j| pos[j]).collect();
            pos[i] = Some(if placed.is_empty() {
                first_place(i, &graph.nodes[i].id)
            } else {
                let n = placed.len() as f32;
                let (x, y) = placed
                    .iter()
                    .fold((0.0, 0.0), |(x, y), p| (x + p[0], y + p[1]));
                let h = hash(&graph.nodes[i].id);
                let a = (h % 628) as f32 * 0.01;
                [
                    x / n + SPRING * 0.5 * a.cos(),
                    y / n + SPRING * 0.5 * a.sin(),
                ]
            });
        }
        let id_of = |g: &Graph, i: Option<usize>| i.map(|i| g.nodes[i].id.clone());
        let selected = id_of(&self.graph, self.selected);
        self.pos = pos.into_iter().map(Option::unwrap_or_default).collect();
        self.neighbors = neighbors;
        self.ranks = ranks(&graph);
        self.graph = graph;
        let find =
            |id: Option<String>| id.and_then(|id| self.graph.nodes.iter().position(|n| n.id == id));
        self.selected = find(selected).or(self.graph.focus);
        self.hovered = None;
        self.held = None;
        self.heat = self.heat.max(START_HEAT * 0.5);
    }

    /// The graph.
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// Node `i`'s place in the layout.
    pub fn place(&self, i: usize) -> [f32; 2] {
        self.pos[i]
    }

    /// Node `i`'s neighbors.
    pub fn neighbors(&self, i: usize) -> &[usize] {
        &self.neighbors[i]
    }

    /// Whether the layout still moves.
    pub fn moving(&self) -> bool {
        self.heat > COLD || self.held.is_some()
    }

    /// Steps the layout for at most `budget`; whether it still moves.
    pub fn step(&mut self, budget: Duration) -> bool {
        let start = Instant::now();
        while self.moving() {
            self.iterate();
            if start.elapsed() >= budget {
                break;
            }
        }
        self.moving()
    }

    /// One step of the forces.
    fn iterate(&mut self) {
        let n = self.pos.len();
        if n == 0 {
            self.heat = 0.0;
            return;
        }
        let mut force = vec![[0.0f32; 2]; n];
        // Every node pushes every other away, far cells as one body.
        let tree = QuadTree::build(&self.pos);
        for (i, f) in force.iter_mut().enumerate() {
            let [fx, fy] = tree.push(&self.pos, i);
            f[0] += fx;
            f[1] += fy;
        }
        // Edges pull their ends together.
        for &(a, b) in &self.graph.edges {
            let (pa, pb) = (self.pos[a], self.pos[b]);
            let (dx, dy) = (pb[0] - pa[0], pb[1] - pa[1]);
            let d = (dx * dx + dy * dy).sqrt().max(0.01);
            let pull = d * d / SPRING / d;
            force[a][0] += dx * pull;
            force[a][1] += dy * pull;
            force[b][0] -= dx * pull;
            force[b][1] -= dy * pull;
        }
        // The middle pulls a little; the focus stays there.
        for (i, f) in force.iter_mut().enumerate() {
            f[0] -= self.pos[i][0] * GRAVITY;
            f[1] -= self.pos[i][1] * GRAVITY;
        }
        for (i, (p, [fx, fy])) in self.pos.iter_mut().zip(force).enumerate() {
            if Some(i) == self.held {
                continue;
            }
            if Some(i) == self.graph.focus {
                *p = [0.0, 0.0];
                continue;
            }
            let len = (fx * fx + fy * fy).sqrt();
            if len > 0.0 {
                let m = len.min(self.heat) / len;
                p[0] += fx * m;
                p[1] += fy * m;
            }
        }
        self.heat *= COOLING;
    }

    /// Node `i`'s radius in the layout's units.
    pub fn radius(&self, i: usize) -> f32 {
        3.0 + 1.6 * (self.graph.nodes[i].weight.max(1) as f32).sqrt()
    }

    /// Where a point of the layout is in a window of `size` pixels.
    pub fn to_screen(&self, p: [f32; 2], size: [f32; 2]) -> [f32; 2] {
        [
            (p[0] - self.center[0]) * self.zoom + size[0] / 2.0,
            (p[1] - self.center[1]) * self.zoom + size[1] / 2.0,
        ]
    }

    /// Which point of the layout a pixel of a window of `size` shows.
    pub fn to_layout(&self, s: [f32; 2], size: [f32; 2]) -> [f32; 2] {
        [
            (s[0] - size[0] / 2.0) / self.zoom + self.center[0],
            (s[1] - size[1] / 2.0) / self.zoom + self.center[1],
        ]
    }

    /// The node at pixel `s` of a window of `size`, the nearest of those
    /// under it (with a few pixels' grace for small ones).
    pub fn node_at(&self, s: [f32; 2], size: [f32; 2]) -> Option<usize> {
        let p = self.to_layout(s, size);
        let grace = 4.0 / self.zoom;
        (0..self.pos.len())
            .filter_map(|i| {
                let (dx, dy) = (self.pos[i][0] - p[0], self.pos[i][1] - p[1]);
                let d = (dx * dx + dy * dy).sqrt();
                (d <= self.radius(i) + grace).then_some((d, i))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, i)| i)
    }

    /// Fits the layout into a window of `size` while it spreads, until
    /// the user moves the view; true when it did.
    pub fn follow(&mut self, size: [f32; 2]) -> bool {
        if self.moved || size[0] < 1.0 || size[1] < 1.0 {
            return false;
        }
        self.fit(size);
        true
    }

    /// Fits the whole layout into a window of `size`.
    pub fn fit(&mut self, size: [f32; 2]) {
        if self.pos.is_empty() {
            return;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in &self.pos {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
        let (w, h) = ((x1 - x0).max(SPRING), (y1 - y0).max(SPRING));
        self.center = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
        self.zoom = ((size[0] * 0.9) / w)
            .min((size[1] * 0.9) / h)
            .clamp(0.02, 4.0);
    }

    /// Zooms by `factor` keeping pixel `s` over the same point.
    pub fn zoom_at(&mut self, factor: f32, s: [f32; 2], size: [f32; 2]) {
        self.moved = true;
        let before = self.to_layout(s, size);
        self.zoom = (self.zoom * factor).clamp(0.02, 8.0);
        let after = self.to_layout(s, size);
        self.center[0] += before[0] - after[0];
        self.center[1] += before[1] - after[1];
    }

    /// Moves the view by `d` pixels, the layout following the mouse.
    pub fn pan_by(&mut self, d: [f32; 2]) {
        self.moved = true;
        self.center[0] -= d[0] / self.zoom;
        self.center[1] -= d[1] / self.zoom;
    }

    /// Holds node `i` at pixel `s` (dragged), the rest moving around it.
    pub fn drag(&mut self, i: usize, s: [f32; 2], size: [f32; 2]) {
        if i >= self.pos.len() {
            return;
        }
        self.held = Some(i);
        self.moved = true;
        self.pos[i] = self.to_layout(s, size);
        self.heat = self.heat.max(START_HEAT * 0.2);
    }

    /// Lets the node dragged go.
    pub fn release(&mut self) {
        self.held = None;
    }

    /// The node dragged.
    pub fn held(&self) -> Option<usize> {
        self.held
    }

    /// The nodes whose names are written, of those `on` screen at
    /// `places`, each name `width(i)` wide and `height` high under its
    /// node: the heaviest first, as many as fit, a name left out where it
    /// would cover one already written (zooming in makes room); the node
    /// chosen, the one under the mouse and the one in focus always, then
    /// the neighbors of the one under the mouse. Of a large graph, the
    /// [`NAMED`] heaviest on the screen are tried.
    pub fn labels(
        &self,
        places: &[[f32; 2]],
        on: impl Fn(usize) -> bool,
        width: impl Fn(usize) -> f32,
        height: f32,
    ) -> Vec<usize> {
        let n = self.graph.nodes.len().min(places.len());
        let strong = |i: usize| {
            Some(i) == self.hovered || Some(i) == self.selected || Some(i) == self.graph.focus
        };
        let near = |i: usize| self.hovered.is_some_and(|h| self.neighbors[h].contains(&i));
        let mut order: Vec<usize> = (0..n).filter(|&i| on(i)).collect();
        order.sort_by_key(|&i| {
            (
                !strong(i),
                !near(i),
                std::cmp::Reverse(self.graph.nodes[i].weight),
                i,
            )
        });
        order.truncate(NAMED.max(order.iter().take_while(|&&i| strong(i)).count()));
        let mut taken: Vec<[f32; 4]> = Vec::new();
        let mut out = Vec::new();
        for i in order {
            let (p, w) = (places[i], width(i));
            let top = p[1] + self.radius(i) * self.zoom + 2.0;
            let r = [p[0] - w / 2.0, top, p[0] + w / 2.0, top + height];
            let covers = |t: &[f32; 4]| r[0] < t[2] && t[0] < r[2] && r[1] < t[3] && t[1] < r[3];
            if strong(i) || !taken.iter().any(covers) {
                taken.push(r);
                out.push(i);
            }
        }
        out
    }

    /// Whether node `i` is drawn faint: the mouse is over another that is
    /// not its neighbor.
    pub fn faint(&self, i: usize) -> bool {
        self.hovered
            .is_some_and(|h| h != i && !self.neighbors[h].contains(&i))
    }

    /// The color of node `i`: none for the most common group, drawn in a
    /// neutral color as Logseq and Obsidian draw their pages grey; for the
    /// others a place in a palette of `n` by their group, the next most
    /// common the first color.
    pub fn color(&self, i: usize, n: usize) -> Option<usize> {
        self.ranks[i].checked_sub(1).map(|r| r % n.max(1))
    }

    /// Steps the choice to the heaviest neighbor of the node chosen, or
    /// back to the heaviest node: the keys' way about the graph.
    pub fn select_next(&mut self, back: bool) {
        let n = self.graph.nodes.len();
        if n == 0 {
            return;
        }
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|&i| (std::cmp::Reverse(self.graph.nodes[i].weight), i));
        let at = self
            .selected
            .and_then(|s| order.iter().position(|&i| i == s));
        let next = match (at, back) {
            (None, _) => 0,
            (Some(a), false) => (a + 1) % n,
            (Some(a), true) => (a + n - 1) % n,
        };
        self.selected = Some(order[next]);
    }

    /// Centers the view on the node chosen.
    pub fn center_on_selected(&mut self) {
        if let Some(i) = self.selected {
            self.center = self.pos[i];
        }
    }
}

/// The names of a large graph tried for a frame: its heaviest nodes on
/// the screen.
pub const NAMED: usize = 300;

/// Each node's group's rank among the groups, the most common first (and
/// of two as common, the first named in alphabetical order).
fn ranks(graph: &Graph) -> Vec<usize> {
    let mut count: HashMap<&str, usize> = HashMap::new();
    for n in &graph.nodes {
        *count.entry(n.group.as_str()).or_default() += 1;
    }
    let mut groups: Vec<(&str, usize)> = count.into_iter().collect();
    groups.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let rank: HashMap<&str, usize> = groups.iter().enumerate().map(|(i, g)| (g.0, i)).collect();
    graph.nodes.iter().map(|n| rank[n.group.as_str()]).collect()
}

/// Each node's neighbors in `graph`.
fn neighbors(graph: &Graph) -> Vec<Vec<usize>> {
    let mut v = vec![Vec::new(); graph.nodes.len()];
    for &(a, b) in &graph.edges {
        v[a].push(b);
        v[b].push(a);
    }
    v
}

/// A Barnes–Hut quadtree over the nodes' places: each cell's mass and
/// center of mass, so that a far cell pushes as one body.
struct QuadTree {
    cells: Vec<Cell>,
}

struct Cell {
    /// The cell's middle and half its side.
    mid: [f32; 2],
    half: f32,
    /// How many nodes, and their center.
    mass: f32,
    com: [f32; 2],
    /// A leaf's node.
    body: Option<usize>,
    /// The four quarters, when split.
    kids: Option<[usize; 4]>,
}

impl QuadTree {
    fn build(pos: &[[f32; 2]]) -> QuadTree {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in pos {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
        let half = ((x1 - x0).max(y1 - y0) / 2.0).max(1.0) + 1.0;
        let mut t = QuadTree {
            cells: vec![Cell {
                mid: [(x0 + x1) / 2.0, (y0 + y1) / 2.0],
                half,
                mass: 0.0,
                com: [0.0, 0.0],
                body: None,
                kids: None,
            }],
        };
        for i in 0..pos.len() {
            t.insert(0, i, pos, 0);
        }
        t
    }

    fn insert(&mut self, c: usize, i: usize, pos: &[[f32; 2]], depth: u32) {
        let p = pos[i];
        {
            let cell = &mut self.cells[c];
            let m = cell.mass;
            cell.com = [
                (cell.com[0] * m + p[0]) / (m + 1.0),
                (cell.com[1] * m + p[1]) / (m + 1.0),
            ];
            cell.mass += 1.0;
        }
        if self.cells[c].kids.is_none() {
            match self.cells[c].body {
                None if self.cells[c].mass <= 1.0 => {
                    self.cells[c].body = Some(i);
                    return;
                }
                // Two nodes at one place, or deep enough: kept together.
                _ if depth > 24 => return,
                _ => self.split(c),
            }
            if let Some(b) = self.cells[c].body.take() {
                let k = self.quarter(c, pos[b]);
                self.place_in(k, b, pos, depth + 1);
            }
        }
        let k = self.quarter(c, p);
        self.place_in(k, i, pos, depth + 1);
    }

    /// Puts node `i` in quarter `k` without counting it again above.
    fn place_in(&mut self, k: usize, i: usize, pos: &[[f32; 2]], depth: u32) {
        self.insert(k, i, pos, depth);
    }

    fn split(&mut self, c: usize) {
        let (mid, half) = (self.cells[c].mid, self.cells[c].half / 2.0);
        let mut kids = [0; 4];
        for (q, k) in kids.iter_mut().enumerate() {
            let dx = if q & 1 == 0 { -half } else { half };
            let dy = if q & 2 == 0 { -half } else { half };
            *k = self.cells.len();
            self.cells.push(Cell {
                mid: [mid[0] + dx, mid[1] + dy],
                half,
                mass: 0.0,
                com: [0.0, 0.0],
                body: None,
                kids: None,
            });
        }
        self.cells[c].kids = Some(kids);
    }

    fn quarter(&self, c: usize, p: [f32; 2]) -> usize {
        let cell = &self.cells[c];
        let kids = cell.kids.unwrap_or([c; 4]);
        let q = usize::from(p[0] >= cell.mid[0]) | (usize::from(p[1] >= cell.mid[1]) << 1);
        kids[q]
    }

    /// The push on node `i` from all the others.
    fn push(&self, pos: &[[f32; 2]], i: usize) -> [f32; 2] {
        let mut f = [0.0, 0.0];
        let mut stack = vec![0usize];
        let p = pos[i];
        while let Some(c) = stack.pop() {
            let cell = &self.cells[c];
            if cell.mass == 0.0 || cell.body == Some(i) {
                continue;
            }
            let (dx, dy) = (p[0] - cell.com[0], p[1] - cell.com[1]);
            let d2 = dx * dx + dy * dy;
            let far =
                cell.kids.is_none() || (cell.half * 2.0) * (cell.half * 2.0) < THETA * THETA * d2;
            if far {
                // Nodes at one place pushed apart in a direction of their own.
                let (dx, dy, d2) = if d2 < 0.01 {
                    let a = (i as f32) * 2.399_963;
                    (a.cos(), a.sin(), 1.0)
                } else {
                    (dx, dy, d2)
                };
                // Fruchterman and Reingold's k²/d along the line.
                let push = REPULSION * cell.mass / d2;
                f[0] += dx * push;
                f[1] += dy * push;
            } else if let Some(kids) = cell.kids {
                stack.extend(kids);
            }
        }
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, weight: u32) -> Node {
        Node {
            id: id.into(),
            label: id.to_uppercase(),
            group: if id.starts_with('j') {
                "journal"
            } else {
                "page"
            }
            .into(),
            weight,
        }
    }

    fn graph(n: usize, edges: &[(usize, usize)]) -> Arc<Graph> {
        let nodes = (0..n).map(|i| node(&format!("n{i}"), 1)).collect();
        let edges: Vec<(String, String)> = edges
            .iter()
            .map(|(a, b)| (format!("n{a}"), format!("n{b}")))
            .collect();
        Arc::new(Graph::new(nodes, &edges, None, "p.open".into()))
    }

    #[test]
    fn names_written_heaviest_first_none_over_another() {
        // Three nodes side by side, the middle one the heaviest: its name
        // is written, its neighbors' would cover it; far apart, all.
        let g = Graph::new(
            vec![node("a", 1), node("b", 5), node("c", 1)],
            &[],
            None,
            "p.open".into(),
        );
        let mut v = GraphView::new(Arc::new(g));
        let near = [[100.0, 100.0], [130.0, 100.0], [160.0, 100.0]];
        let all = |_| true;
        assert_eq!(v.labels(&near, all, |_| 50.0, 14.0), [1]);
        let far = [[100.0, 100.0], [300.0, 100.0], [500.0, 100.0]];
        assert_eq!(v.labels(&far, all, |_| 50.0, 14.0), [1, 0, 2]);
        // The node chosen is named whatever it covers: then the heaviest's
        // name would cover its, and the first's no longer covers any.
        v.selected = Some(2);
        assert_eq!(v.labels(&near, all, |_| 50.0, 14.0), [2, 0]);
        // Nodes off the screen are not named.
        assert_eq!(v.labels(&far, |i| i != 0, |_| 50.0, 14.0), [2, 1]);
        // The most common group drawn neutral, the others colored.
        assert_eq!(v.color(0, 8), None);
    }

    fn dist(v: &GraphView, a: usize, b: usize) -> f32 {
        let (p, q) = (v.place(a), v.place(b));
        ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()
    }

    #[test]
    fn edges_by_the_nodes_named() {
        let g = Graph::new(
            vec![node("a", 1), node("b", 1), node("c", 1)],
            &[
                ("a".into(), "b".into()),
                ("b".into(), "a".into()),
                ("a".into(), "a".into()),
                ("a".into(), "nowhere".into()),
                ("c".into(), "b".into()),
            ],
            Some("b"),
            "p.open".into(),
        );
        assert_eq!(g.edges, [(0, 1), (1, 2)]);
        assert_eq!(g.focus, Some(1));
    }

    #[test]
    fn a_layout_settles_linked_nodes_closer() {
        // Two clusters of four, joined by one edge.
        let g = graph(
            8,
            &[
                (0, 1),
                (1, 2),
                (2, 3),
                (3, 0),
                (0, 2),
                (4, 5),
                (5, 6),
                (6, 7),
                (7, 4),
                (4, 6),
                (3, 4),
            ],
        );
        let mut v = GraphView::new(g);
        let mut steps = 0;
        while v.moving() && steps < 2000 {
            v.iterate();
            steps += 1;
        }
        assert!(!v.moving(), "settled in {steps} steps");
        // Inside a cluster nearer than across.
        assert!(
            dist(&v, 0, 2) < dist(&v, 0, 6),
            "{} {}",
            dist(&v, 0, 2),
            dist(&v, 0, 6)
        );
        // Nobody on top of anybody.
        for a in 0..8 {
            for b in a + 1..8 {
                assert!(dist(&v, a, b) > SPRING * 0.2, "{a} {b}");
            }
        }
        // The same graph, the same layout.
        let mut w = GraphView::new(graph(
            8,
            &[
                (0, 1),
                (1, 2),
                (2, 3),
                (3, 0),
                (0, 2),
                (4, 5),
                (5, 6),
                (6, 7),
                (7, 4),
                (4, 6),
                (3, 4),
            ],
        ));
        while w.moving() {
            w.iterate();
        }
        assert_eq!(v.place(5), w.place(5));
    }

    #[test]
    fn many_nodes_lay_out_within_a_budget() {
        let n = 2000;
        let edges: Vec<(usize, usize)> = (1..n).map(|i| (i, (i * 7) % i)).collect();
        let mut v = GraphView::new(graph(n, &edges));
        let t = Instant::now();
        v.step(Duration::from_millis(50));
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn written_again_the_nodes_keep_their_places() {
        let mut v = GraphView::new(graph(3, &[(0, 1), (1, 2)]));
        while v.moving() {
            v.iterate();
        }
        let before = v.place(1);
        v.selected = Some(1);
        // A node added beside node 2; node 0 gone.
        let nodes = vec![node("n1", 1), node("n2", 1), node("n9", 1)];
        let g = Graph::new(
            nodes,
            &[("n1".into(), "n2".into()), ("n2".into(), "n9".into())],
            None,
            "p.open".into(),
        );
        v.update(Arc::new(g));
        assert_eq!(v.place(0), before);
        assert_eq!(v.selected, Some(0), "the choice follows its node");
        assert!(
            dist(&v, 1, 2) < SPRING * 1.5,
            "the new node beside its neighbor"
        );
        assert!(v.moving());
    }

    #[test]
    fn pixels_and_the_layout() {
        let mut v = GraphView::new(graph(4, &[(0, 1), (1, 2), (2, 3)]));
        while v.moving() {
            v.iterate();
        }
        let size = [800.0, 600.0];
        assert!(v.follow(size));
        // Every node on the screen.
        for i in 0..4 {
            let s = v.to_screen(v.place(i), size);
            assert!((0.0..=800.0).contains(&s[0]) && (0.0..=600.0).contains(&s[1]));
            assert_eq!(v.node_at(s, size), Some(i));
        }
        // Zooming keeps the point under the mouse; the view is the
        // user's then.
        let s = [100.0, 120.0];
        let p = v.to_layout(s, size);
        v.zoom_at(2.0, s, size);
        assert!(!v.follow(size));
        let q = v.to_layout(s, size);
        assert!((p[0] - q[0]).abs() < 0.01 && (p[1] - q[1]).abs() < 0.01);
        // Panning moves the layout with the mouse.
        let before = v.to_screen(v.place(0), size);
        v.pan_by([10.0, -5.0]);
        let after = v.to_screen(v.place(0), size);
        assert!((after[0] - before[0] - 10.0).abs() < 0.01);
        assert!((after[1] - before[1] + 5.0).abs() < 0.01);
        // A node dragged is held at the mouse.
        v.drag(2, [400.0, 300.0], size);
        assert_eq!(v.node_at([400.0, 300.0], size), Some(2));
        assert!(v.moving());
        v.release();
        // The neighbors of the node under the mouse stand out.
        v.hovered = Some(1);
        assert!(!v.faint(0) && !v.faint(2) && v.faint(3));
    }

    #[test]
    fn the_focus_stays_in_the_middle() {
        let nodes = (0..5).map(|i| node(&format!("n{i}"), 1)).collect();
        let edges: Vec<(String, String)> = (1..5).map(|i| ("n0".into(), format!("n{i}"))).collect();
        let mut v = GraphView::new(Arc::new(Graph::new(nodes, &edges, Some("n0"), "p".into())));
        while v.moving() {
            v.iterate();
        }
        assert_eq!(v.place(0), [0.0, 0.0]);
        assert_eq!(v.selected, Some(0));
        v.select_next(false);
        assert_eq!(v.selected, Some(1));
    }
}
