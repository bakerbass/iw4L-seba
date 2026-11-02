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
    /// Doors and barricades close links while they stand; nonzero is closed.
    pub closed: u16,
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

    /// Close every open link whose segment passes through the box. Returns the
    /// links closed, for `open_links`.
    pub fn close_through(&mut self, mins: [f32; 3], maxs: [f32; 3]) -> Vec<(u16, u16)> {
        let mut closed = Vec::new();
        for from in 0..self.nodes.len() {
            let a = self.nodes[from].origin;
            for index in 0..self.nodes[from].links.len() {
                let to = usize::from(self.nodes[from].links[index].to);
                let Some(b) = self.nodes.get(to).map(|n| n.origin) else {
                    continue;
                };
                if segment_hits_box(a, b, mins, maxs) {
                    self.nodes[from].links[index].closed += 1;
                    closed.push((from as u16, index as u16));
                }
            }
        }
        closed
    }

    pub fn open_links(&mut self, links: &[(u16, u16)]) {
        for &(node, index) in links {
            if let Some(link) = self
                .nodes
                .get_mut(usize::from(node))
                .and_then(|n| n.links.get_mut(usize::from(index)))
            {
                link.closed = link.closed.saturating_sub(1);
            }
        }
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
                if link.closed > 0 || to >= self.nodes.len() || self.blocked[to] {
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

    /// The `count` open nodes closest to `point`, nearest first.
    pub fn nearest_many(&self, point: [f32; 3], count: usize) -> Vec<usize> {
        let mut found: Vec<(usize, f32)> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| !self.blocked[*i])
            .map(|(i, n)| (i, dist(n.origin, point)))
            .collect();
        found.sort_by(|a, b| a.1.total_cmp(&b.1));
        found.into_iter().take(count).map(|(i, _)| i).collect()
    }

    /// Waypoints from `from` to `to` over the graph (node origins, then `to`).
    /// A node beside a point can sit on a disconnected island (a traversal end
    /// node, say), so several nearby nodes are tried on each side.
    pub fn path(&self, from: [f32; 3], to: [f32; 3]) -> Option<Vec<[f32; 3]>> {
        let starts = self.nearest_many(from, 6);
        let goals = self.nearest_many(to, 6);
        let mut best: Option<(f32, Vec<usize>)> = None;
        for &start in &starts {
            for &goal in &goals {
                let Some(route) = self.route(start, goal) else {
                    continue;
                };
                let length: f32 = route
                    .windows(2)
                    .map(|w| dist(self.nodes[w[0]].origin, self.nodes[w[1]].origin))
                    .sum::<f32>()
                    + dist(from, self.nodes[start].origin)
                    + dist(to, self.nodes[goal].origin);
                if best.as_ref().is_none_or(|(b, _)| length < *b) {
                    best = Some((length, route));
                }
            }
        }
        let (_, route) = best?;
        let mut points: Vec<[f32; 3]> = route.iter().map(|i| self.nodes[*i].origin).collect();
        points.push(to);
        Some(points)
    }
}

fn segment_hits_box(a: [f32; 3], b: [f32; 3], mins: [f32; 3], maxs: [f32; 3]) -> bool {
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for axis in 0..3 {
        let d = b[axis] - a[axis];
        if d.abs() < 1e-6 {
            if a[axis] < mins[axis] || a[axis] > maxs[axis] {
                return false;
            }
        } else {
            let (mut near, mut far) = ((mins[axis] - a[axis]) / d, (maxs[axis] - a[axis]) / d);
            if near > far {
                std::mem::swap(&mut near, &mut far);
            }
            t0 = t0.max(near);
            t1 = t1.min(far);
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}
