//! BO1's single-player script layer for zombies.
//!
//! `_zombiemode` needs the scripts of several zones, not just the map's:
//! `code_post_gfx` (utility, anim), `common`, `common_zombie`, `patch`, then
//! the map. Later zones replace earlier ones. Everything is stored under the
//! `t5sp/` prefix so IW4 modules of the same name stay untouched.

use std::path::Path;

use asset_transport::T5ZoneMemory;
use fastfile_t5::{AssetLinkSink, AssetSink, AssetType, Ptr, Result, ScriptStrings, ZoneStream};

use crate::ScriptSources;

/// Zones whose scripts the single-player build loads, in load order.
const COMMON_ZONES: [&str; 5] = [
    "code_pre_gfx",
    "code_post_gfx",
    "common",
    "common_zombie",
    "patch",
];

#[derive(Default)]
struct ScriptOnlySink {
    scripts: ScriptSources,
}

impl AssetSink for ScriptOnlySink {
    fn set_script_strings(&mut self, _strings: ScriptStrings) {}

    fn load_asset(
        &mut self,
        s: &mut ZoneStream<'_>,
        _index: usize,
        ty: AssetType,
        slot: Ptr,
    ) -> Result<()> {
        fastfile_t5::load_asset_at_observed(s, ty, slot, self)?;
        Ok(())
    }
}

impl AssetLinkSink for ScriptOnlySink {
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

    fn capture_raw_file(&mut self, name: &str, data: &[u8], zlib_compressed: bool) -> Result<()> {
        self.scripts.capture_sp(name, data, zlib_compressed);
        Ok(())
    }
}

fn walk_zone(path: &Path, scripts: &mut ScriptSources) -> std::result::Result<String, String> {
    let image = asset_transport::open_zone(path).map_err(|e| format!("open: {e}"))?;
    let header = image.t5_header().map_err(|e| format!("header: {e}"))?;
    let mut memory = T5ZoneMemory::for_header(&header);
    let mut stream = memory
        .stream(&image.bytes)
        .map_err(|e| format!("arenas: {e}"))?;
    let mut sink = ScriptOnlySink::default();
    let walked = fastfile_t5::load_zone(&mut stream, &mut sink);
    let count = sink.scripts.sp_len();
    scripts.overlay(sink.scripts);
    match walked {
        Ok(_) => Ok(format!("{count} scripts")),
        Err(e) => Err(format!("walk stopped after {count} scripts: {e}")),
    }
}

/// Build the layer: the common zones next to the map zone, then the map's own
/// raw scripts. Returns the layer and report lines.
pub(super) fn build_sp_layer(
    zone_dir: &Path,
    map_scripts: &ScriptSources,
) -> (ScriptSources, Vec<String>) {
    let mut layer = ScriptSources::default();
    let mut report = Vec::new();
    for zone in COMMON_ZONES {
        let path = zone_dir.join(format!("{zone}.ff"));
        if !path.is_file() {
            report.push(format!("t5 sp scripts: {zone}.ff not found in {}", zone_dir.display()));
            continue;
        }
        match walk_zone(&path, &mut layer) {
            Ok(summary) => report.push(format!("t5 sp scripts: {zone}: {summary}")),
            Err(gap) => report.push(format!("t5 sp scripts gap: {zone}: {gap}")),
        }
    }
    layer.absorb_sp(map_scripts);
    report.push(format!(
        "t5 sp scripts: {} modules under {}/ ({} from the map zone)",
        layer.sp_len(),
        crate::script_sources::SP_LAYER,
        map_scripts.len()
    ));
    if let Some(entities) = map_scripts.entities() {
        layer.set_sp_entities(entities.to_owned());
    }
    (layer, report)
}
