use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Namespace {
    Function,
    Method,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Owner {
    Script,
    Player,
    Entity,
    HudElem,
    ScriptMover,
    PlayerCommand,
    Helicopter,
    Vehicle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Builtin {
    pub namespace: Namespace,
    pub name: &'static str,
    pub owner: Owner,
    pub developer: bool,
}
impl Builtin {
    pub const fn new(
        namespace: Namespace,
        name: &'static str,
        owner: Owner,
        developer: bool,
    ) -> Self {
        Self {
            namespace,
            name,
            owner,
            developer,
        }
    }
}

/// Which engine flavour the scripts were written for. The multiplayer build
/// drives matches through `maps/mp/gametypes/_callbacksetup`; the
/// single-player build (BO1 zombies) through `maps/_callbacksetup`, with
/// actors, spawners and no classes or teams menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Profile {
    Mp,
    Sp,
}

#[derive(Clone, Debug)]
pub struct Catalog {
    realm: crate::script::Realm,
    profile: Profile,
    names: BTreeMap<Namespace, BTreeMap<&'static str, Builtin>>,
}
impl Catalog {
    pub fn iw4() -> Self {
        Self::from_list(crate::script::Realm::Iw4, super::iw4_catalog::IW4)
    }
    pub fn t5() -> Self {
        Self::from_list(crate::script::Realm::T5, super::t5_catalog::T5)
    }
    pub fn profile(&self) -> Profile {
        self.profile
    }
    /// The single-player build's builtins: the multiplayer-derived T5 list plus
    /// everything BlackOps.exe registers that it lacks (AI, actor animation,
    /// spawners). BO1 zombies runs on this build, not the multiplayer one.
    pub fn t5_sp() -> Self {
        let mut catalog = Self::t5();
        catalog.profile = Profile::Sp;
        for builtin in super::t5_sp_catalog::T5_SP_EXTRA {
            catalog.insert(builtin.clone());
        }
        catalog
    }
    fn from_list(realm: crate::script::Realm, list: &[Builtin]) -> Self {
        let mut catalog = Self {
            realm,
            profile: Profile::Mp,
            names: BTreeMap::new(),
        };
        for builtin in list {
            catalog.insert(builtin.clone());
        }
        catalog
    }
    pub fn realm(&self) -> crate::script::Realm {
        self.realm
    }
    fn insert(&mut self, builtin: Builtin) {
        self.names
            .entry(builtin.namespace)
            .or_default()
            .insert(builtin.name, builtin);
    }
    pub fn get(&self, namespace: Namespace, name: &str) -> Option<&Builtin> {
        self.names.get(&namespace)?.get(name)
    }
    pub fn iter(&self) -> impl Iterator<Item = &Builtin> {
        self.names.values().flat_map(BTreeMap::values)
    }
}
