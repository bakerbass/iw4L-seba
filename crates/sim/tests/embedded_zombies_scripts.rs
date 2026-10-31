//! The Zombies gametype scripts are embedded text that is otherwise only compiled
//! when a zombie map loads, so a typo surfaces as a map-load error. Compile them
//! here against the IW4 catalog so it fails in `cargo test` instead.

use std::collections::BTreeMap;

use sim::script::{Catalog, Program};

const ZOMBIES: &str = include_str!("../../assets/src/zom.gsc.txt");
const ZOMBIE_ECONOMY: &str = include_str!("../../assets/src/zom_economy.gsc.txt");

#[test]
fn zombies_scripts_compile() {
    let mut sources = BTreeMap::new();
    sources.insert("maps/mp/gametypes/zom".to_owned(), ZOMBIES.to_owned());
    sources.insert("iw4l_zom/economy".to_owned(), ZOMBIE_ECONOMY.to_owned());
    sources.insert(
        "maps/mp/gametypes/dm".to_owned(),
        "main() {}\nonStartGameType() {}\n".to_owned(),
    );
    let roots = ["maps/mp/gametypes/zom"];
    if let Err(fault) = Program::load(&sources, &roots, &Catalog::iw4()) {
        panic!("Zombies scripts failed to compile: {fault:?}");
    }
}
