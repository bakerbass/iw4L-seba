//! Optional MW2 guns for BO1 zombies (`zm_mw2_guns 1`).
//!
//! BO1's scripts keep their own weapon names, prices and hints; the engine holds the MW2 gun.
//! The mapping is applied where script weapons meet native ones (`players::bridged_weapon` and
//! `players::script_weapon`), so `hasweapon`, ammo and the weapon notifies stay on BO1 names.
//! Wonder weapons, equipment and Pack-a-Punch variants have no entry and stay BO1.

use crate::frame::FrameWorld;
use crate::script::MatchScript;
use bevy_ecs::prelude::World;

/// BO1 `_zm` weapon and the MW2 `_mp` weapon that stands in for it. One to one, so a native gun
/// reads back as exactly one BO1 name. Not listed, so left as BO1: the ray gun, tesla, thundergun,
/// crossbow, ballistic knives, freeze gun, monkey bomb, claymore, frags, dual-wield CZ75 and every
/// Pack-a-Punch variant.
const GUNS: &[(&str, &str)] = &[
    // pistols
    ("m1911_zm", "usp_mp"),
    ("python_zm", "coltanaconda_mp"),
    ("cz75_zm", "glock_mp"),
    // SMGs
    ("mp5k_zm", "mp5k_mp"),
    ("mp40_zm", "ump45_mp"),
    ("mpl_zm", "pp2000_mp"),
    ("pm63_zm", "uzi_mp"),
    ("spectre_zm", "p90_mp"),
    ("ak74u_zm", "kriss_mp"),
    // shotguns
    ("ithaca_zm", "spas12_mp"),
    ("spas_zm", "aa12_mp"),
    ("rottweil72_zm", "model1887_mp"),
    ("hs10_zm", "ranger_mp"),
    ("stakeout_zm", "striker_mp"),
    ("enfield_zm", "m1014_mp"),
    // rifles
    ("m14_zm", "m21_mp"),
    ("m16_zm", "m16_mp"),
    ("g11_lps_zm", "tavor_mp"),
    ("famas_zm", "famas_mp"),
    ("fnfal_zm", "fal_mp"),
    ("galil_zm", "scar_mp"),
    ("commando_zm", "masada_mp"),
    ("aug_acog_zm", "fn2000_mp"),
    ("ak47_zm", "ak47_mp"),
    // snipers
    ("dragunov_zm", "wa2000_mp"),
    ("l96a1_zm", "cheytac_mp"),
    // machine guns
    ("rpk_zm", "mg4_mp"),
    ("hk21_zm", "rpd_mp"),
    // launchers
    ("m72_law_zm", "at4_mp"),
    ("china_lake_zm", "m79_mp"),
];

pub(crate) fn active(world: &World) -> bool {
    world
        .resource::<MatchScript>()
        .dvars
        .get("zm_mw2_guns")
        .is_some_and(|value| !value.is_empty() && value != "0")
}

fn native_name(zm: &str) -> Option<&'static str> {
    GUNS.iter().find(|(from, _)| *from == zm).map(|(_, to)| *to)
}

fn script_name(native: &str) -> Option<&'static str> {
    GUNS.iter().find(|(_, to)| *to == native).map(|(from, _)| *from)
}

/// The native weapon standing in for a BO1 script weapon, when the option is on.
pub(crate) fn native_for(world: &mut World, script: u32) -> Option<u32> {
    if !active(world) {
        return None;
    }
    let frame = FrameWorld::from_world(world);
    static REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !REPORTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        for (zm, mw2) in GUNS {
            diag::info!(
                Sim,
                "zm_mw2_guns: {zm} (script {:?}) -> {mw2} (native {:?})",
                frame.weapon_index_by_script_name(zm),
                frame.weapon_index_by_script_name(mw2)
            );
        }
    }
    // T5 weapons carry an `_mp` suffix in their script names (`m1911_zm_mp`).
    let named = frame.weapon_script_name(script);
    let from = named.strip_suffix("_zm_mp").map_or_else(|| named.to_owned(), |base| format!("{base}_zm"));
    let to = native_name(&from)?;
    let found = frame.weapon_index_by_script_name(to).filter(|&w| w != script);
    if found.is_none() {
        static WARNED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        if let Ok(mut warned) = WARNED.lock()
            && !warned.contains(&from)
        {
            warned.push(from.clone());
            diag::warn!(Sim, "zm_mw2_guns: no MW2 weapon `{to}` for `{from}`, it stays BO1");
        }
    }
    found
}

/// The BO1 script weapon a native MW2 weapon stands in for.
pub(crate) fn script_for(world: &mut World, native: u32) -> Option<u32> {
    if !active(world) {
        return None;
    }
    let frame = FrameWorld::from_world(world);
    let from = script_name(frame.weapon_script_name(native))?;
    frame.weapon_index_by_script_name(from).filter(|&w| w != native)
}
