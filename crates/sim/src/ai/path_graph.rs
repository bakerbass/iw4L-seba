//! Path nodes and A* over their links. The graph is the zone's decoded
//! `PathData`; doors and barricades switch nodes off with `set_blocked`.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

pub const NODE_TYPE_NEGOTIATION_BEGIN: u16 = 17;
pub const NODE_TYPE_NEGOTIATION_END: u16 = 18;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Link {
    pub to: u16,
    pub dist: f32,
    pub negotiation: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    pub kind: u16,
    pub spawnflags: u16,
    pub origin: [f32; 3],
    pub angle: f32,
    pub targetname: Option<String>,
    pub linkname: Option<String>,
    pub noteworthy: Option<String>,
    pub target: Option<String>,
    pub animscript: Option<String>,
    pub links: Vec<Link>,
}

#[derive(Clone, Debug, Default)]
pub struct PathGraph {
    nodes: Vec<Node>,
    blocked: Vec<bool>,
}

#[derive(PartialEq)]
struct Open {
    f: f32,
    node: u16,
}
impl Eq for Open {}
impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other.f.total_cmp(&self.f).then(other.node.cmp(&self.node))
    }
}
impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

impl PathGraph {
    pub fn new(nodes: Vec<Node>) -> Self {
        let blocked = vec![false; nodes.len()];
        Self { nodes, blocked }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn node(&self, index: usize) -> Option<&Node> {
        self.nodes.get(index)
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn set_blocked(&mut self, index: usize, blocked: bool) {
        if let Some(slot) = self.blocked.get_mut(index) {
            *slot = blocked;
        }
    }

    /// The open node closest to `point`, optionally within `max` units.
    pub fn nearest(&self, point: [f32; 3], max: f32) -> Option<usize> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| !self.blocked[*i])
            .map(|(i, n)| (i, dist(n.origin, point)))
            .filter(|(_, d)| *d <= max)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Node indices from `start` to `goal`, inclusive, or `None` when no open
    /// route links them.
    pub fn route(&self, start: usize, goal: usize) -> Option<Vec<usize>> {
        if start >= self.nodes.len() || goal >= self.nodes.len() || self.blocked[goal] {
            return None;
        }
        let mut best = vec![f32::INFINITY; self.nodes.len()];
        let mut came: Vec<Option<u16>> = vec![None; self.nodes.len()];
        let mut open = BinaryHeap::new();
        best[start] = 0.0;
        open.push(Open {
            f: dist(self.nodes[start].origin, self.nodes[goal].origin),
            node: start as u16,
        });
        while let Some(Open { node, .. }) = open.pop() {
            let at = usize::from(node);
            if at == goal {
                let mut route = vec![goal];
                let mut cursor = goal;
                while let Some(previous) = came[cursor] {
                    cursor = usize::from(previous);
                    route.push(cursor);
                }
                route.reverse();
                return Some(route);
            }
            for link in &self.nodes[at].links {
                let to = usize::from(link.to);
                if to >= self.nodes.len() || self.blocked[to] {
                    continue;
                }
                let cost = best[at] + link.dist.max(0.01);
                if cost < best[to] {
                    best[to] = cost;
                    came[to] = Some(node);
                    open.push(Open {
                        f: cost + dist(self.nodes[to].origin, self.nodes[goal].origin),
                        node: link.to,
                    });
                }
            }
        }
        None
    }

    /// Waypoints from `from` to `to` over the graph (node origins, then `to`).
    pub fn path(&self, from: [f32; 3], to: [f32; 3]) -> Option<Vec<[f32; 3]>> {
        let start = self.nearest(from, f32::INFINITY)?;
        let goal = self.nearest(to, f32::INFINITY)?;
        let route = self.route(start, goal)?;
        let mut points: Vec<[f32; 3]> = route.iter().map(|i| self.nodes[*i].origin).collect();
        points.push(to);
        Some(points)
    }
}
