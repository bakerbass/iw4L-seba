//! The actor runtime: BO1's AI actors (zombies, dogs) as script entities with
//! a goal, a path and a movement state. Scripts decide behaviour; this module
//! supplies what the engine did natively: spawning, pathing and motion.

use std::collections::BTreeMap;

pub mod path_graph;
pub use path_graph::PathGraph;

/// Where an actor was told to go.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Goal {
    #[default]
    None,
    Position([f32; 3]),
    Entity(u64),
}

/// Engine-side state of one spawned actor, keyed by its script object id.
#[derive(Clone, Debug, Default)]
pub(crate) struct Actor {
    /// Set when the actor's health reaches zero; dead actors leave `getaiarray`.
    pub dead: bool,
    pub goal: Goal,
    /// The goal was reached and announced; cleared when the goal changes.
    pub arrived: bool,
    /// Facing wanted once the goal is reached (`setgoalpos(pos, angles)`).
    pub goal_yaw: Option<f32>,
    /// A turn in progress; `orientdone` is raised when it completes.
    pub face_yaw: Option<f32>,
    pub path: Vec<[f32; 3]>,
    pub path_goal: [f32; 3],
    pub repath_at_ms: i64,
    /// `orientmode` name (`face motion`, `face angle`, `face enemy`, ...).
    pub orient: String,
    pub orient_yaw: f32,
    /// `animmode` name (`normal`, `gravity`, `nogravity`, `none`, ...).
    pub anim_mode: String,
    /// Animations running on the actor, newest last.
    pub playing: Vec<Playing>,
    /// An `animscripted` clip that drives the actor's position.
    pub scripted: Option<Scripted>,
    /// The animation last handed to the renderer.
    pub shown: Option<std::sync::Arc<str>>,
}

/// One running animation. `flag` names the notify raised for its notetracks.
#[derive(Clone, Debug)]
pub(crate) struct Playing {
    pub flag: Option<std::sync::Arc<str>>,
    pub name: std::sync::Arc<str>,
    pub clip: Option<std::sync::Arc<xmodel_runtime::AnimClip>>,
    /// 0..1 through the clip.
    pub t: f32,
    pub rate: f32,
    pub ended: bool,
}

impl Playing {
    pub fn looping(&self) -> bool {
        self.clip.as_ref().is_some_and(|c| c.looping)
    }

    pub fn seconds(&self) -> f32 {
        self.clip.as_ref().map_or(1.0, |c| c.duration()).max(0.05)
    }
}

/// An `animscripted` playback: the actor is carried by the clip's root motion
/// from `origin`/`angles` until it ends.
#[derive(Clone, Debug)]
pub(crate) struct Scripted {
    pub anim: Playing,
    pub origin: [f32; 3],
    pub angles: [f32; 3],
}

impl Playing {
    /// Advance by `dt` seconds; returns the notify names to raise.
    pub fn advance(&mut self, dt: f32) -> Vec<String> {
        let mut raised = Vec::new();
        if self.ended {
            return raised;
        }
        let before = self.t;
        self.t += dt * self.rate / self.seconds();
        let upto = self.t.min(1.0);
        if let Some(clip) = &self.clip {
            raised.extend(
                clip.notifies
                    .iter()
                    .filter(|n| n.time > before && n.time <= upto)
                    .map(|n| n.name.clone()),
            );
        }
        if self.t >= 1.0 {
            raised.push("end".to_owned());
            if self.looping() {
                self.t -= self.t.floor();
            } else {
                self.t = 1.0;
                self.ended = true;
            }
        }
        raised
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Actors {
    pub by_object: BTreeMap<u64, Actor>,
}

/// Looks up a decoded zombie animation by name (set at install; lazy decode).
#[derive(Clone, Default)]
pub struct ActorAnimSource(
    Option<std::sync::Arc<dyn Fn(&str) -> Option<std::sync::Arc<xmodel_runtime::AnimClip>> + Send + Sync>>,
);

impl ActorAnimSource {
    pub fn new(
        lookup: impl Fn(&str) -> Option<std::sync::Arc<xmodel_runtime::AnimClip>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self(Some(std::sync::Arc::new(lookup)))
    }

    pub(crate) fn clip(&self, name: &str) -> Option<std::sync::Arc<xmodel_runtime::AnimClip>> {
        self.0.as_ref().and_then(|lookup| lookup(name))
    }
}

impl std::fmt::Debug for ActorAnimSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ActorAnimSource")
    }
}
