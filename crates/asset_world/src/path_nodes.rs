//! T5 path nodes (`GameWorldSp`/`GameWorldMp` `PathData`), decoded into plain data.
//!
//! BO1 zombies walks these nodes: the actor runtime pathfinds over the links,
//! and `_zombiemode` finds traversal and zone nodes by their script strings.
//! Layout of one 128-byte `pathnode_t` (offsets checked against Kino, where the
//! origins span the playable map and every link lands on a real node):
//!
//! ```text
//! 0x00 u16 type        0x02 u16 spawnflags   0x04 u16 targetname
//! 0x06 u16 linkname    0x08 u16 noteworthy   0x0a u16 target
//! 0x0c u16 animscript  0x14 f32[3] origin    0x20 f32 angle
//! 0x24 f32[2] forward  0x2c f32 radius       0x30 f32 min_use_dist_sq
//! 0x34 i16[2] overlap  0x38 i16 chain id     0x3a i16 chain depth
//! 0x3c i16 chain parent  0x3e u16 link count 0x40 *pathlink_t
//! ```
//!
//! A `pathlink_t` is 12 bytes: `f32 dist, u16 node, u8 disconnect_count,
//! u8 negotiation, u8 bad_place_count[4]`. The `u16` string fields are indices
//! into the zone's script string table; `0` means unset.

use fastfile_t5::size as sz;
use fastfile_t5::{PathDataGeometry, ScriptStrings, ZonePtr, ZoneStream};

const OFF_TYPE: usize = 0x00;
const OFF_SPAWNFLAGS: usize = 0x02;
const OFF_TARGETNAME: usize = 0x04;
const OFF_LINKNAME: usize = 0x06;
const OFF_NOTEWORTHY: usize = 0x08;
const OFF_TARGET: usize = 0x0a;
const OFF_ANIMSCRIPT: usize = 0x0c;
const OFF_ORIGIN: usize = 0x14;
const OFF_ANGLE: usize = 0x20;
const OFF_FORWARD: usize = 0x24;
const OFF_RADIUS: usize = 0x2c;
const OFF_MIN_USE_DIST_SQ: usize = 0x30;
const OFF_OVERLAP: usize = 0x34;
const OFF_CHAIN_ID: usize = 0x38;
const OFF_CHAIN_DEPTH: usize = 0x3a;
const OFF_CHAIN_PARENT: usize = 0x3c;

const LINK_DIST: usize = 0;
const LINK_NODE: usize = 4;
const LINK_DISCONNECT: usize = 6;
const LINK_NEGOTIATION: usize = 7;

/// `type` values seen in BO1 zones. Kino has plain nodes plus 54 pairs of the
/// two traversal kinds; the rest of the engine's table is not needed yet.
pub const NODE_TYPE_PATH: u16 = 1;
pub const NODE_TYPE_NEGOTIATION_BEGIN: u16 = 17;
pub const NODE_TYPE_NEGOTIATION_END: u16 = 18;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathLink {
    pub dist: f32,
    pub to: u16,
    pub disconnect_count: u8,
    pub negotiation: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathNode {
    pub kind: u16,
    pub spawnflags: u16,
    pub targetname: Option<String>,
    pub linkname: Option<String>,
    pub noteworthy: Option<String>,
    pub target: Option<String>,
    pub animscript: Option<String>,
    pub origin: [f32; 3],
    pub angle: f32,
    pub forward: [f32; 2],
    pub radius: f32,
    pub min_use_dist_sq: f32,
    pub overlap: [i16; 2],
    pub chain_id: i16,
    pub chain_depth: i16,
    pub chain_parent: i16,
    pub links: Vec<PathLink>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathNodeGraph {
    pub nodes: Vec<PathNode>,
}

impl PathNodeGraph {
    pub fn link_count(&self) -> usize {
        self.nodes.iter().map(|n| n.links.len()).sum()
    }

    /// Links whose far end is not a node of this graph. A healthy decode has none.
    pub fn dangling_links(&self) -> usize {
        self.nodes
            .iter()
            .flat_map(|n| &n.links)
            .filter(|l| usize::from(l.to) >= self.nodes.len())
            .count()
    }

    pub fn count_kind(&self, kind: u16) -> usize {
        self.nodes.iter().filter(|n| n.kind == kind).count()
    }
}

/// Decode the retained path data. `None` when the zone had no path nodes or a
/// read left the zone memory, so a bad decode never yields a partial graph.
pub fn decode_t5_path_nodes(
    s: &ZoneStream<'_>,
    geometry: PathDataGeometry,
    strings: &ScriptStrings,
) -> Option<PathNodeGraph> {
    let base = geometry.nodes?;
    let string = |id: u16| -> Option<String> {
        if id == 0 {
            return None;
        }
        strings.get(s, id).map(str::to_owned)
    };
    let mut nodes = Vec::with_capacity(geometry.node_count);
    for i in 0..geometry.node_count {
        let n = base.at(i * sz::PATH_NODE);
        let link_count = usize::from(s.u16_at(n, sz::PATH_NODE_TOTAL_LINK_COUNT_OFF).ok()?);
        let mut links = Vec::with_capacity(link_count);
        if link_count > 0 {
            let ZonePtr::Offset(lp) = s.ptr_at(n, sz::PATH_NODE_LINKS_OFF).ok()? else {
                return None;
            };
            for j in 0..link_count {
                let l = lp.at(j * sz::PATH_LINK);
                links.push(PathLink {
                    dist: s.f32_at(l, LINK_DIST).ok()?,
                    to: s.u16_at(l, LINK_NODE).ok()?,
                    disconnect_count: s.u8_at(l, LINK_DISCONNECT).ok()?,
                    negotiation: s.u8_at(l, LINK_NEGOTIATION).ok()?,
                });
            }
        }
        nodes.push(PathNode {
            kind: s.u16_at(n, OFF_TYPE).ok()?,
            spawnflags: s.u16_at(n, OFF_SPAWNFLAGS).ok()?,
            targetname: string(s.u16_at(n, OFF_TARGETNAME).ok()?),
            linkname: string(s.u16_at(n, OFF_LINKNAME).ok()?),
            noteworthy: string(s.u16_at(n, OFF_NOTEWORTHY).ok()?),
            target: string(s.u16_at(n, OFF_TARGET).ok()?),
            animscript: string(s.u16_at(n, OFF_ANIMSCRIPT).ok()?),
            origin: [
                s.f32_at(n, OFF_ORIGIN).ok()?,
                s.f32_at(n, OFF_ORIGIN + 4).ok()?,
                s.f32_at(n, OFF_ORIGIN + 8).ok()?,
            ],
            angle: s.f32_at(n, OFF_ANGLE).ok()?,
            forward: [
                s.f32_at(n, OFF_FORWARD).ok()?,
                s.f32_at(n, OFF_FORWARD + 4).ok()?,
            ],
            radius: s.f32_at(n, OFF_RADIUS).ok()?,
            min_use_dist_sq: s.f32_at(n, OFF_MIN_USE_DIST_SQ).ok()?,
            overlap: [
                s.i16_at(n, OFF_OVERLAP).ok()?,
                s.i16_at(n, OFF_OVERLAP + 2).ok()?,
            ],
            chain_id: s.i16_at(n, OFF_CHAIN_ID).ok()?,
            chain_depth: s.i16_at(n, OFF_CHAIN_DEPTH).ok()?,
            chain_parent: s.i16_at(n, OFF_CHAIN_PARENT).ok()?,
            links,
        });
    }
    Some(PathNodeGraph { nodes })
}
