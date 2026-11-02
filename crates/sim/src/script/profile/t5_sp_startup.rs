use crate::script::source::SourceResolver;

/// Script startup for BO1's single-player build (zombies).
///
/// There is no gametype script: the engine runs the level script's `main`,
/// which sets up callbacks and calls `maps/_zombiemode::main`. Nothing under
/// `maps/mp/` is loaded.
///
/// Install runs the struct initializer itself, before the map's structs exist.
pub struct T5SpStartup {
    pub roots: Vec<String>,
    pub entries: Vec<String>,
}

/// The single-player code-callback module the engine calls into.
pub const CALLBACKS: &str = "maps/_callbacksetup";

impl T5SpStartup {
    /// `map` is the zone name (`zombie_theater`); its script is `maps/<map>`.
    pub fn new(resolver: &impl SourceResolver, map: &str) -> Result<Self, String> {
        let level = format!("maps/{map}");
        for required in [CALLBACKS, level.as_str()] {
            if resolver.read_bytes(required).is_err() {
                return Err(format!("single-player startup needs {required}"));
            }
        }
        let roots = vec![
            "codescripts/delete".to_owned(),
            "codescripts/struct".to_owned(),
            "common_scripts/utility".to_owned(),
            CALLBACKS.to_owned(),
            level.clone(),
        ];
        let entries = vec![
            format!("{level}::main"),
            format!("{CALLBACKS}::codecallback_startgametype"),
        ];
        Ok(Self { roots, entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn sources(modules: &[&str]) -> BTreeMap<String, String> {
        modules
            .iter()
            .map(|m| ((*m).to_owned(), String::new()))
            .collect()
    }

    #[test]
    fn level_main_runs_before_the_start_callback() {
        let startup =
            T5SpStartup::new(&sources(&["maps/_callbacksetup", "maps/zombie_theater"]), "zombie_theater")
                .unwrap();
        assert_eq!(
            startup.entries,
            [
                "maps/zombie_theater::main",
                "maps/_callbacksetup::codecallback_startgametype"
            ]
        );
        assert!(startup.roots.iter().all(|r| !r.starts_with("maps/mp/")));
        assert!(startup.roots.contains(&"maps/zombie_theater".to_owned()));
    }

    #[test]
    fn a_missing_level_script_is_refused_by_name() {
        let err = T5SpStartup::new(&sources(&["maps/_callbacksetup"]), "zombie_theater")
            .err()
            .unwrap();
        assert!(err.contains("maps/zombie_theater"), "{err}");
    }
}
