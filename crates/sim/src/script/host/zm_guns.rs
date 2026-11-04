//! Optional MW2 or MW3 guns for BO1 zombies (`zm_gun_source mw2|mw3|mix`).
//!
//! BO1's scripts keep their own weapon names, prices and hints; the engine holds the MW2 or MW3 gun.
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

/// BO1 `_zm` weapon and the MW3 gun that stands in for it, one to one like `GUNS`.
const MW3_GUNS: &[(&str, &str)] = &[
    // pistols
    ("m1911_zm", "iw5_usp45_mp"),
    ("python_zm", "iw5_44magnum_mp"),
    ("cz75_zm", "iw5_fnfiveseven_mp"),
    // SMGs
    ("mp5k_zm", "iw5_mp5_mp"),
    ("mp40_zm", "iw5_ump45_mp"),
    ("mpl_zm", "iw5_pp90m1_mp"),
    ("pm63_zm", "iw5_mp7_mp"),
    ("spectre_zm", "iw5_p90_mp"),
    ("ak74u_zm", "iw5_m9_mp"),
    // shotguns
    ("ithaca_zm", "iw5_ksg_mp"),
    ("spas_zm", "iw5_spas12_mp"),
    ("rottweil72_zm", "iw5_1887_mp"),
    ("hs10_zm", "iw5_striker_mp"),
    ("stakeout_zm", "iw5_usas12_mp"),
    // rifles
    ("enfield_zm", "iw5_cm901_mp"),
    ("m14_zm", "iw5_mk14_mp"),
    ("m16_zm", "iw5_m16_mp"),
    ("g11_lps_zm", "iw5_type95_mp"),
    ("famas_zm", "iw5_fad_mp"),
    ("fnfal_zm", "iw5_scar_mp"),
    ("galil_zm", "iw5_g36c_mp"),
    ("commando_zm", "iw5_acr_mp"),
    ("aug_acog_zm", "iw5_m4_mp"),
    ("ak47_zm", "iw5_ak47_mp"),
    // snipers
    ("dragunov_zm", "iw5_dragunov_mp"),
    ("l96a1_zm", "iw5_l96a1_mp"),
    // machine guns
    ("rpk_zm", "iw5_pecheneg_mp"),
    ("hk21_zm", "iw5_mk46_mp"),
    // launchers
    ("m72_law_zm", "iw5_smaw_mp"),
    ("china_lake_zm", "iw5_xm25_mp"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Bo1,
    Mw2,
    Mw3,
    /// Each gun is BO1, MW2 or MW3, fixed by its name.
    Mix,
}

/// `zm_gun_source`: `bo1` (default), `mw2`, `mw3` or `mix`. `zm_mw2_guns 1` is `mw2`.
fn source(world: &World) -> Source {
    let dvars = &world.resource::<MatchScript>().dvars;
    match dvars.get("zm_gun_source").map(String::as_str) {
        Some("mw2") => Source::Mw2,
        Some("mw3") => Source::Mw3,
        Some("mix") => Source::Mix,
        _ if dvars.get("zm_mw2_guns").is_some_and(|value| !value.is_empty() && value != "0") => Source::Mw2,
        _ => Source::Bo1,
    }
}

pub(crate) fn active(world: &World) -> bool {
    source(world) != Source::Bo1
}

/// The table a BO1 gun swaps from; in a mixed game about a third of the guns stay BO1.
fn table_for(source: Source, zm: &str) -> Option<&'static [(&'static str, &'static str)]> {
    match source {
        Source::Bo1 => None,
        Source::Mw2 => Some(GUNS),
        Source::Mw3 => Some(MW3_GUNS),
        Source::Mix => match zm.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b))) % 3 {
            0 => None,
            1 => Some(GUNS),
            _ => Some(MW3_GUNS),
        },
    }
}

fn native_name(table: &[(&str, &'static str)], zm: &str) -> Option<&'static str> {
    table.iter().find(|(from, _)| *from == zm).map(|(_, to)| *to)
}

/// MW2 and MW3 names never collide, so the reverse lookup searches both tables.
fn script_name(native: &str) -> Option<&'static str> {
    GUNS.iter()
        .chain(MW3_GUNS)
        .find(|(_, to)| *to == native)
        .map(|(from, _)| *from)
}

/// The native weapon standing in for a BO1 script weapon, when the option is on.
pub(crate) fn native_for(world: &mut World, script: u32) -> Option<u32> {
    if !active(world) {
        return None;
    }
    let source = source(world);
    let frame = FrameWorld::from_world(world);
    static REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !REPORTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        for (zm, native) in GUNS.iter().chain(MW3_GUNS) {
            diag::info!(
                Sim,
                "zm_guns: {zm} (script {:?}) -> {native} (native {:?})",
                frame.weapon_index_by_script_name(zm),
                frame.weapon_index_by_script_name(native)
            );
        }
    }
    // T5 weapons carry an `_mp` suffix in their script names (`m1911_zm_mp`).
    let named = frame.weapon_script_name(script);
    let from = named.strip_suffix("_zm_mp").map_or_else(|| named.to_owned(), |base| format!("{base}_zm"));
    let to = native_name(table_for(source, &from)?, &from)?;
    let found = frame.weapon_index_by_script_name(to).filter(|&w| w != script);
    if found.is_none() {
        static WARNED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        if let Ok(mut warned) = WARNED.lock()
            && !warned.contains(&from)
        {
            warned.push(from.clone());
            diag::warn!(Sim, "zm_guns: no weapon `{to}` for `{from}`, it stays BO1");
        }
    }
    found
}

/// The native BO1 weapon a bridged MW2 or MW3 gun stands in for, read from the frame.
/// Bridged guns borrow its damage numbers, so kill speed stays BO1's.
pub(crate) fn bo1_counterpart(frame: &FrameWorld, native: u32) -> Option<u32> {
    if !active(frame.ecs_ref()) {
        return None;
    }
    let from = script_name(frame.weapon_script_name(native))?;
    frame.weapon_index_by_script_name(from).filter(|&w| w != native)
}

/// The BO1 script weapon a native MW2 or MW3 weapon stands in for.
pub(crate) fn script_for(world: &mut World, native: u32) -> Option<u32> {
    if !active(world) {
        return None;
    }
    let frame = FrameWorld::from_world(world);
    let from = script_name(frame.weapon_script_name(native))?;
    frame.weapon_index_by_script_name(from).filter(|&w| w != native)
}
