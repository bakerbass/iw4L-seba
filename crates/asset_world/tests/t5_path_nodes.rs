//! Decodes the path nodes of a real BO1 zone. Ignored by default because the
//! game files are not in the repository:
//!
//! ```text
//! T5_ZONE_FF="C:/Program Files (x86)/Activision/Call of Duty - Black Ops/zone/Common/zombie_theater.ff" \
//!   cargo test -p asset_world --test t5_path_nodes -- --ignored
//! ```

use asset_transport::zone::parse_zone_image;
use asset_world::{
    NODE_TYPE_NEGOTIATION_BEGIN, NODE_TYPE_NEGOTIATION_END, PathNodeGraph, T5ZoneMemory,
    decode_t5_path_nodes,
};
use fastfile_t5::{
    AssetLinkSink, AssetSink, AssetType, Ptr, Result, ScriptStrings, ZoneStream,
    load_asset_at_observed, load_zone, parse_zone_header,
};

#[derive(Default)]
struct Sink {
    strings: ScriptStrings,
}

impl AssetSink for Sink {
    fn set_script_strings(&mut self, strings: ScriptStrings) {
        self.strings = strings;
    }

    fn load_asset(
        &mut self,
        s: &mut ZoneStream<'_>,
        _index: usize,
        ty: AssetType,
        slot: Ptr,
    ) -> Result<()> {
        load_asset_at_observed(s, ty, slot, self)?;
        Ok(())
    }
}

impl AssetLinkSink for Sink {
    fn loaded(
        &mut self,
        _s: &ZoneStream<'_>,
        _ty: AssetType,
        _slot: Ptr,
        _insert_slot: Option<Ptr>,
    ) -> Result<()> {
        Ok(())
    }

    fn alias(&mut self, _ty: AssetType, _slot: Ptr, _target: Ptr) -> Result<()> {
        Ok(())
    }

    fn capture_raw_file(&mut self, _name: &str, _data: &[u8], _zlib: bool) -> Result<()> {
        Ok(())
    }
}

fn decode(path: &str) -> PathNodeGraph {
    let bytes = std::fs::read(path).expect("read zone");
    let image = parse_zone_image(&bytes).expect("zone image");
    let header = parse_zone_header(&image.bytes).expect("xfile header");
    let mut memory = T5ZoneMemory::for_header(&header);
    let mut stream = memory.stream(&image.bytes).expect("zone arenas");
    let mut sink = Sink::default();
    load_zone(&mut stream, &mut sink).expect("zone walk");
    let geometry = stream.path_data().expect("zone has path data");
    decode_t5_path_nodes(&stream, geometry, &sink.strings).expect("path nodes decode")
}

#[test]
#[ignore = "needs T5_ZONE_FF pointing at a BO1 map zone"]
fn kino_path_nodes_decode_to_a_consistent_graph() {
    let Ok(path) = std::env::var("T5_ZONE_FF") else {
        panic!("set T5_ZONE_FF to a BO1 zone such as zombie_theater.ff");
    };
    // The decode walks the whole zone on a deep stack, like the census probe.
    let graph = std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || decode(&path))
        .unwrap()
        .join()
        .unwrap();

    assert!(graph.nodes.len() > 1000, "nodes: {}", graph.nodes.len());
    assert_eq!(graph.dangling_links(), 0, "every link must land on a node");
    assert_eq!(
        graph.count_kind(NODE_TYPE_NEGOTIATION_BEGIN),
        graph.count_kind(NODE_TYPE_NEGOTIATION_END),
        "traversals come in begin/end pairs"
    );
    for node in &graph.nodes {
        assert!(
            node.origin.iter().all(|v| v.abs() < 20_000.0),
            "origin out of range: {:?}",
            node.origin
        );
        for link in &node.links {
            assert!(link.dist.is_finite() && link.dist > 0.0, "link {link:?}");
        }
    }
    assert!(
        graph.nodes.iter().any(|n| n.linkname.is_some()),
        "script strings should resolve to names"
    );
}
