//! Actor natives: spawners, spawning, and the AI queries `_zombiemode` makes.
//! Behaviour lives in BO1's animscripts; this supplies the engine side.

use super::super::args::{float, optional, string, vector};
use crate::ai::Goal;
use super::super::arrays::new_array;
use super::super::entities::EntityKind;
use crate::script::{
    Namespace::{Function, Method},
    NativeRegistry, RoundScript, Value,
};
use bevy_ecs::prelude::World;

const SPAWNER_FLAG: i32 = 1;

pub(crate) fn register(registry: &mut NativeRegistry) {
    registry.register(Function, "getspawnerarray", |world, _, _| {
        let ids = spawner_ids(world);
        new_array(world, ids.into_iter().map(Value::Object).collect())
    });
    registry.register(Function, "getnode", |world, _, args| {
        let (name, key) = (string(args, 0)?, string(args, 1)?);
        Ok(find_nodes(world, &name, &key)?
            .into_iter()
            .next()
            .map_or(Value::Undefined, Value::Object))
    });
    registry.register(Function, "getnodearray", |world, _, args| {
        let (name, key) = (string(args, 0)?, string(args, 1)?);
        let ids = find_nodes(world, &name, &key)?;
        new_array(world, ids.into_iter().map(Value::Object).collect())
    });
    registry.register(Function, "getallnodes", |world, _, _| {
        let ids = node_objects(world)?;
        new_array(world, ids.into_iter().map(Value::Object).collect())
    });
    registry.register(Function, "isspawner", |world, _, args| {
        let yes = match args.first() {
            Some(Value::Object(id)) => is_spawner(world.resource::<RoundScript>(), *id),
            _ => false,
        };
        Ok(Value::Int(yes.into()))
    });
    registry.register(Function, "getaiarray", |world, _, args| {
        let team = optional(args, 0, string)?;
        let ids = live_actors(world, team.as_deref(), None);
        new_array(world, ids.into_iter().map(Value::Object).collect())
    });
    registry.register(Function, "getaispeciesarray", |world, _, args| {
        let team = optional(args, 0, string)?;
        let species = optional(args, 1, string)?;
        let ids = live_actors(world, team.as_deref(), species.as_deref());
        new_array(world, ids.into_iter().map(Value::Object).collect())
    });
    for name in ["dospawn", "stalingradspawn", "stalingradspawnsafe"] {
        registry.register(Method, name, |world, receiver, args| {
            let Value::Object(spawner) = receiver else {
                return Err("spawn receiver is not a spawner".into());
            };
            let name = optional(args, 0, string)?;
            spawn_from(world, *spawner, name)
        });
    }
    registry.register(Method, "setgoalpos", |world, receiver, args| {
        let goal = vector(args, 0)?;
        let yaw = optional(args, 1, vector)?.map(|angles| angles[1]);
        set_goal(world, receiver, Goal::Position(goal))?;
        if let Value::Object(id) = receiver
            && let Some(actor) = world
                .resource_mut::<RoundScript>()
                .engine
                .actors
                .by_object
                .get_mut(id)
        {
            actor.goal_yaw = yaw;
        }
        Ok(Value::Undefined)
    });
    registry.register(Method, "setgoalnode", |world, receiver, args| {
        let node = args.first().cloned().unwrap_or(Value::Undefined);
        let origin = match &node {
            Value::Object(id) => vec3(super::super::players::entity_field(world, *id, "origin")),
            _ => return Err("setgoalnode needs a node".into()),
        };
        set_goal(world, receiver, Goal::Position(origin))
    });
    registry.register(Method, "setgoalentity", |world, receiver, args| {
        match args.first() {
            Some(Value::Object(e)) => set_goal(world, receiver, Goal::Entity(*e)),
            _ => Err("setgoalentity needs an entity".into()),
        }
    });
    registry.register(Method, "forceteleport", |world, receiver, args| {
        let origin = vector(args, 0)?;
        let angles = optional(args, 1, vector)?;
        let Value::Object(id) = receiver else {
            return Err("forceteleport receiver is not an actor".into());
        };
        let mut runtime = world.resource_mut::<RoundScript>();
        runtime.set_object_field(*id, "origin", Value::Vector(origin));
        if let Some(angles) = angles {
            runtime.set_object_field(*id, "angles", Value::Vector(angles));
        }
        if let Some(actor) = runtime.engine.actors.by_object.get_mut(id) {
            actor.path.clear();
        }
        Ok(Value::Undefined)
    });
    registry.register(Method, "orientmode", |world, receiver, args| {
        let mode = string(args, 0)?.to_ascii_lowercase();
        let yaw = optional(args, 1, super::super::args::float)?;
        let Value::Object(id) = receiver else {
            return Err("orientmode receiver is not an actor".into());
        };
        if let Some(actor) = world
            .resource_mut::<RoundScript>()
            .engine
            .actors
            .by_object
            .get_mut(id)
        {
            if mode.starts_with("face angle")
                && let Some(yaw) = yaw
            {
                actor.face_yaw = Some(yaw);
            }
            actor.orient = mode;
            if let Some(yaw) = yaw {
                actor.orient_yaw = yaw;
            }
        }
        Ok(Value::Undefined)
    });
    registry.register(Method, "animmode", |world, receiver, args| {
        let mode = string(args, 0)?.to_ascii_lowercase();
        let Value::Object(id) = receiver else {
            return Err("animmode receiver is not an actor".into());
        };
        if let Some(actor) = world
            .resource_mut::<RoundScript>()
            .engine
            .actors
            .by_object
            .get_mut(id)
        {
            actor.anim_mode = mode;
        }
        Ok(Value::Undefined)
    });
    // The traverse animscript turns gravity back on once it has crossed.
    registry.register(Method, "traversemode", |world, receiver, args| {
        if string(args, 0)?.eq_ignore_ascii_case("gravity")
            && let Value::Object(id) = receiver
            && let Some(traverse) = world
                .resource_mut::<RoundScript>()
                .engine
                .actors
                .by_object
                .get_mut(id)
                .and_then(|actor| actor.traverse.as_mut())
        {
            traverse.finishing = true;
        }
        Ok(Value::Undefined)
    });
    registry.register(Method, "getnegotiationstartnode", |world, receiver, _| {
        negotiation_node(world, receiver, false)
    });
    registry.register(Method, "getnegotiationendnode", |world, receiver, _| {
        negotiation_node(world, receiver, true)
    });
    registry.register(Method, "teleport", |world, receiver, args| {
        let origin = vector(args, 0)?;
        let angles = optional(args, 1, vector)?;
        let Value::Object(id) = receiver else {
            return Err("teleport is not simulated for this receiver".into());
        };
        let mut runtime = world.resource_mut::<RoundScript>();
        if !runtime.engine.actors.by_object.contains_key(id) {
            return Err("teleport is not simulated for this receiver".into());
        }
        runtime.set_object_field(*id, "origin", Value::Vector(origin));
        if let Some(angles) = angles {
            runtime.set_object_field(*id, "angles", Value::Vector(angles));
        }
        Ok(Value::Undefined)
    });
    // `StalingradSpawn(no_enemy_info, targetname)` in _utility wraps this.
    registry.register(Method, "codespawnerforcespawn", |world, receiver, args| {
        let Value::Object(spawner) = receiver else {
            return Err("spawn receiver is not a spawner".into());
        };
        let name = optional(args, 1, string)?;
        spawn_from(world, *spawner, name)
    });
    registry.register(Function, "getmovedelta", |world, _, args| {
        let (t0, t1) = (float(args, 1)?, float(args, 2)?);
        Ok(Value::Vector(match actor_clip(world, args)? {
            Some(clip) => {
                let (a, b) = (clip.abs_delta_trans(t0), clip.abs_delta_trans(t1));
                std::array::from_fn(|i| b[i] - a[i])
            }
            None => [0.0; 3],
        }))
    });
    // The clips carry no root yaw track yet; a zero turn keeps scripts moving.
    registry.register(Function, "getangledelta", |_, _, _| Ok(Value::Float(0.0)));
    registry.register(Function, "getnotetracksindelta", |world, _, args| {
        let (t0, t1) = (float(args, 1)?, float(args, 2)?);
        let found: Vec<(String, f32)> = actor_clip(world, args)?
            .map(|clip| {
                clip.notifies
                    .iter()
                    .filter(|n| n.time >= t0 && n.time <= t1)
                    .map(|n| (n.name.clone(), n.time))
                    .collect()
            })
            .unwrap_or_default();
        let mut rows = Vec::with_capacity(found.len());
        for (name, time) in found {
            rows.push(new_array(
                world,
                vec![Value::string(&name), Value::Float(time)],
            )?);
        }
        new_array(world, rows)
    });
    // Zombie aitypes name weapons the registry never loaded; they have no class.
    registry.register(Function, "weaponclass", |world, _, args| {
        Ok(match super::engine::weapon_facts(world, args) {
            Ok(facts) => super::engine::enum_name(super::engine::WEAPON_CLASSES, facts.weap_class),
            Err(_) => Value::string("none"),
        })
    });
    registry.register(Method, "finishactordamage", |world, receiver, args| {
        let Value::Object(id) = receiver else {
            return Err("finishactordamage receiver is not an actor".into());
        };
        let damage = super::super::args::int(args, 2)?;
        finish_damage(world, *id, args, damage);
        Ok(Value::Undefined)
    });
    registry.register(Method, "disconnectpaths", |world, receiver, _| {
        let Value::Object(id) = receiver else {
            return Ok(Value::Undefined);
        };
        let (mins, maxs) = super::super::triggers::entity_bounds(world, *id);
        if mins == maxs {
            return Ok(Value::Undefined);
        }
        let origin = vec3(
            world
                .resource_mut::<RoundScript>()
                .object_field(*id, "origin"),
        );
        let grow = 4.0;
        let lo = std::array::from_fn(|i| origin[i] + mins[i] - grow);
        let hi = std::array::from_fn(|i| origin[i] + maxs[i] + grow);
        let mut runtime = world.resource_mut::<RoundScript>();
        if runtime.engine.path_holds.contains_key(id) {
            return Ok(Value::Undefined);
        }
        let closed = std::sync::Arc::make_mut(&mut runtime.engine.paths).close_through(lo, hi);
        runtime.engine.path_holds.insert(*id, closed);
        Ok(Value::Undefined)
    });
    registry.register(Method, "connectpaths", |world, receiver, _| {
        let Value::Object(id) = receiver else {
            return Ok(Value::Undefined);
        };
        let mut runtime = world.resource_mut::<RoundScript>();
        if let Some(closed) = runtime.engine.path_holds.remove(id) {
            std::sync::Arc::make_mut(&mut runtime.engine.paths).open_links(&closed);
        }
        Ok(Value::Undefined)
    });
    registry.register(Method, "isnotarget", |_, _, _| Ok(Value::Int(0)));
    registry.register(Method, "setspawnerteam", |world, receiver, args| {
        let Value::Object(id) = receiver else {
            return Err("setspawnerteam receiver is not a spawner".into());
        };
        let team = string(args, 0)?;
        world
            .resource_mut::<RoundScript>()
            .set_object_field(*id, "team", Value::string(&team));
        Ok(Value::Undefined)
    });
}

fn is_spawner(runtime: &RoundScript, id: u64) -> bool {
    let Some(entity) = runtime.entities.get(&id) else {
        return false;
    };
    entity.kind == EntityKind::Map
        && entity.classname.starts_with("actor_")
        && spawnflags(runtime, id) & SPAWNER_FLAG != 0
}

fn spawnflags(runtime: &RoundScript, id: u64) -> i32 {
    let Some(symbol) = runtime
        .program
        .as_ref()
        .and_then(|p| p.symbol_ids.get("spawnflags"))
    else {
        return 0;
    };
    match runtime.objects.get(&id).and_then(|f| f.get(symbol)) {
        Some(Value::Int(n)) => *n,
        _ => 0,
    }
}

/// Script objects for every path node, created the first time scripts look.
fn node_objects(world: &mut World) -> Result<Vec<u64>, String> {
    let mut runtime = world.resource_mut::<RoundScript>();
    let paths = runtime.engine.paths.clone();
    let have = &runtime.engine.node_objects;
    if have.len() == paths.len() && have.iter().all(|id| runtime.objects.contains_key(id)) {
        return Ok(have.clone());
    }
    let mut ids = Vec::with_capacity(paths.len());
    for node in paths.nodes() {
        let id = runtime.new_object()?;
        runtime.set_object_field(id, "origin", Value::Vector(node.origin));
        runtime.set_object_field(id, "angles", Value::Vector([0.0, node.angle, 0.0]));
        let kind = match node.kind {
            crate::ai::path_graph::NODE_TYPE_NEGOTIATION_BEGIN => "Begin",
            crate::ai::path_graph::NODE_TYPE_NEGOTIATION_END => "End",
            _ => "Path",
        };
        runtime.set_object_field(id, "type", Value::string(kind));
        for (field, value) in [
            ("targetname", &node.targetname),
            ("target", &node.target),
            ("script_noteworthy", &node.noteworthy),
            ("linkname", &node.linkname),
            ("animscript", &node.animscript),
        ] {
            if let Some(value) = value {
                runtime.set_object_field(id, field, Value::string(value));
            }
        }
        ids.push(id);
    }
    runtime.engine.node_objects = ids.clone();
    Ok(ids)
}

fn find_nodes(world: &mut World, name: &str, key: &str) -> Result<Vec<u64>, String> {
    let mapped: Vec<u64> = {
        let runtime = world.resource_mut::<RoundScript>();
        let ids = runtime.engine.map_nodes.clone();
        let mut runtime = runtime;
        ids.into_iter()
            .filter(|id| {
                matches!(runtime.object_field(*id, key), Value::String(v) if v.eq_ignore_ascii_case(name))
            })
            .collect()
    };
    if !mapped.is_empty() {
        return Ok(mapped);
    }
    let ids = node_objects(world)?;
    let paths = world.resource::<RoundScript>().engine.paths.clone();
    Ok(paths
        .nodes()
        .iter()
        .zip(ids)
        .filter(|(node, _)| {
            let field = match key {
                "targetname" => &node.targetname,
                "target" => &node.target,
                "script_noteworthy" => &node.noteworthy,
                "linkname" => &node.linkname,
                "animscript" => &node.animscript,
                _ => &None,
            };
            field.as_deref().is_some_and(|v| v.eq_ignore_ascii_case(name))
        })
        .map(|(_, id)| id)
        .collect())
}

fn negotiation_node(world: &mut World, receiver: &Value, end: bool) -> Result<Value, String> {
    let Value::Object(id) = receiver else {
        return Ok(Value::Undefined);
    };
    let node = world
        .resource::<RoundScript>()
        .engine
        .actors
        .by_object
        .get(id)
        .and_then(|actor| actor.traverse.as_ref())
        .map(|t| usize::from(if end { t.end } else { t.start }));
    let Some(node) = node else {
        return Ok(Value::Undefined);
    };
    Ok(node_objects(world)?
        .get(node)
        .map_or(Value::Undefined, |id| Value::Object(*id)))
}

fn spawner_ids(world: &World) -> Vec<u64> {
    let runtime = world.resource::<RoundScript>();
    runtime
        .entities
        .keys()
        .copied()
        .filter(|id| is_spawner(runtime, *id))
        .collect()
}

fn text_field(world: &mut World, id: u64, name: &str) -> Option<String> {
    match world.resource_mut::<RoundScript>().object_field(id, name) {
        Value::String(s) => Some(s.to_string()),
        _ => None,
    }
}

fn live_actors(world: &mut World, team: Option<&str>, species: Option<&str>) -> Vec<u64> {
    let ids: Vec<u64> = world
        .resource::<RoundScript>()
        .engine
        .actors
        .by_object
        .iter()
        .filter(|(_, actor)| !actor.dead)
        .map(|(id, _)| *id)
        .collect();
    ids.into_iter()
        .filter(|id| {
            team.is_none_or(|t| t == "all" || text_field(world, *id, "team").as_deref() == Some(t))
                && species.is_none_or(|s| {
                    s == "all" || text_field(world, *id, "type").as_deref() == Some(s)
                })
        })
        .collect()
}

/// `spawner dospawn()`: make an actor from a spawner entity and run its aitype
/// and animscript setup, as the engine does.
fn spawn_from(world: &mut World, spawner: u64, name: Option<String>) -> Result<Value, String> {
    if !is_spawner(world.resource::<RoundScript>(), spawner) {
        return Err("spawn receiver is not a spawner".into());
    }
    let (classname, fields, origin, angles) = {
        let mut runtime = world.resource_mut::<RoundScript>();
        if let Value::Int(count) = runtime.object_field(spawner, "count") {
            if count <= 0 {
                return Ok(Value::Undefined);
            }
            runtime.set_object_field(spawner, "count", Value::Int(count - 1));
        }
        let origin = vec3(runtime.object_field(spawner, "origin"));
        let angles = vec3(runtime.object_field(spawner, "angles"));
        let classname = runtime.entities[&spawner].classname.clone();
        (classname, runtime.objects[&spawner].clone(), origin, angles)
    };
    let presence = super::super::presence::spawn_presence(world, origin)?;
    let number = crate::frame::FrameWorld::from_world(world).gentity_number(presence);
    let actor = {
        let mut runtime = world.resource_mut::<RoundScript>();
        let id = runtime.create_entity(EntityKind::Spawned, &classname)?;
        runtime.objects.insert(id, fields);
        runtime.set_object_field(id, "spawnflags", Value::Int(0));
        runtime.set_object_field(id, "count", Value::Undefined);
        runtime.set_object_field(id, "origin", Value::Vector(origin));
        runtime.set_object_field(id, "angles", Value::Vector(angles));
        if let Some(name) = name {
            runtime.set_object_field(id, "targetname", Value::string(&name));
        }
        let entity = runtime.entities.get_mut(&id).unwrap();
        entity.presence = Some(presence);
        entity.can_damage = true;
        if let Some(number) = number {
            entity.number = number;
        }
        runtime
            .engine
            .actors
            .by_object
            .insert(id, Default::default());
        id
    };
    let actor_value = Value::Object(actor);
    let now = i64::from(crate::level_time_ms(
        world.resource::<crate::step::StepRequest>().tick,
    ));
    let kind = classname
        .strip_prefix("actor_")
        .unwrap_or(&classname)
        .to_ascii_lowercase();
    run_if_present(world, &format!("aitype/{kind}::main"), &actor_value, now)?;
    let ai_type = text_field(world, actor, "type").unwrap_or_else(|| "zombie".into());
    // Engine-defined AI fields the animscripts read before anything sets them.
    {
        let mut runtime = world.resource_mut::<RoundScript>();
        for (field, value) in [
            ("isdog", Value::Int((ai_type == "zombie_dog").into())),
            ("delayeddeath", Value::Int(0)),
        ] {
            if matches!(runtime.object_field(actor, field), Value::Undefined) {
                runtime.set_object_field(actor, field, value);
            }
        }
    }
    run_if_present(
        world,
        &format!("animscripts/{ai_type}_init::main"),
        &actor_value,
        now,
    )?;
    let combat = format!("animscripts/{ai_type}_combat::main");
    if has_function(world, &combat) {
        crate::script::runtime::start(world, &combat, actor_value.clone(), Vec::new())
            .map_err(|fault| format!("{fault:?}"))?;
    }
    diag::info!(Sim, "actor: spawned {classname} as object {actor}");
    crate::script::runtime::raise(
        world,
        Value::Object(spawner),
        "spawned",
        vec![actor_value.clone()],
    );
    Ok(actor_value)
}

fn vec3(value: Value) -> [f32; 3] {
    match value {
        Value::Vector(v) => v,
        _ => [0.0; 3],
    }
}

fn has_function(world: &World, name: &str) -> bool {
    world
        .resource::<RoundScript>()
        .program
        .as_ref()
        .is_some_and(|p| p.names.contains_key(&name.to_ascii_lowercase()))
}

fn run_if_present(world: &mut World, name: &str, receiver: &Value, now: i64) -> Result<(), String> {
    if !has_function(world, name) {
        return Ok(());
    }
    crate::script::runtime::run_now(world, name, receiver.clone(), Vec::new(), now)
        .map_err(|fault| format!("{fault:?}"))
}

fn set_goal(world: &mut World, receiver: &Value, goal: Goal) -> Result<Value, String> {
    let Value::Object(id) = receiver else {
        return Err("goal receiver is not an actor".into());
    };
    let mut runtime = world.resource_mut::<RoundScript>();
    let actor = runtime
        .engine
        .actors
        .by_object
        .get_mut(id)
        .ok_or("goal receiver is not an actor")?;
    if actor.goal != goal {
        actor.arrived = false;
        actor.path.clear();
    }
    actor.goal = goal;
    Ok(Value::Undefined)
}

/// The zombie xanim named by an animation literal argument, if it is loaded.
fn actor_clip(
    world: &mut World,
    args: &[Value],
) -> Result<Option<std::sync::Arc<xmodel_runtime::AnimClip>>, String> {
    let name = match args.first() {
        Some(Value::Animation { name, .. }) => name.clone(),
        Some(Value::Undefined) | None => return Ok(None),
        Some(_) => return Err("parameter 1 is not an animation".into()),
    };
    Ok(crate::frame::FrameWorld::from_world(world).actor_anim_clip(&name))
}

const ACTOR_DAMAGE: &str = "maps/_callbacksetup::codecallback_actordamage";
const ACTOR_KILLED: &str = "maps/_callbacksetup::codecallback_actorkilled";

/// A hit on an actor goes to the script's damage callback, which decides the
/// amount and calls `finishactordamage` to apply it.
pub(crate) fn actor_damaged(
    world: &mut World,
    actor: u64,
    attacker: Value,
    damage: i32,
    flags: i32,
    means: &str,
    weapon: &str,
    point: [f32; 3],
    dir: [f32; 3],
    part: &str,
) {
    let part = part.to_ascii_lowercase();
    let hitloc = if part.contains("head") {
        "head"
    } else if part.contains("neck") {
        "neck"
    } else if part.contains("leg") || part.contains("knee") || part.contains("ankle") {
        "left_leg_upper"
    } else {
        "torso_upper"
    };
    let args = vec![
        attacker.clone(),
        attacker,
        Value::Int(damage),
        Value::Int(flags),
        Value::string(means),
        Value::string(weapon),
        Value::Vector(point),
        Value::Vector(dir),
        Value::string(hitloc),
        Value::Int(0),
        Value::Int(0),
    ];
    if has_function(world, ACTOR_DAMAGE) {
        let _ = crate::script::runtime::start(world, ACTOR_DAMAGE, Value::Object(actor), args);
    }
}

fn finish_damage(world: &mut World, id: u64, args: &[Value], damage: i32) {
    let mut runtime = world.resource_mut::<RoundScript>();
    let before = match runtime.object_field(id, "health") {
        Value::Int(n) => n,
        Value::Float(n) => n as i32,
        _ => 0,
    };
    let after = before.saturating_sub(damage);
    runtime.set_object_field(id, "health", Value::Int(after));
    // The engine records the last hit on the actor for the death scripts.
    for (field, index) in [
        ("damagemod", 4),
        ("damageweapon", 5),
        ("damagedir", 7),
        ("damagelocation", 8),
        ("attacker", 1),
    ] {
        runtime.set_object_field(id, field, args.get(index).cloned().unwrap_or(Value::Undefined));
    }
    let alive = runtime
        .engine
        .actors
        .by_object
        .get(&id)
        .is_some_and(|a| !a.dead);
    if !(alive && before > 0 && after <= 0) {
        return;
    }
    if let Some(actor) = runtime.engine.actors.by_object.get_mut(&id) {
        actor.dead = true;
        actor.goal = Goal::None;
        actor.path.clear();
    }
    let world_ent = world
        .resource::<RoundScript>()
        .engine
        .world
        .map_or(Value::Undefined, Value::Object);
    // The engine hands the killed callback the world when nothing hit the actor.
    diag::info!(Sim, "actor: {id} killed by {:?} damage={damage}", args.get(4));
    let at = |i: usize| match args.get(i) {
        Some(Value::Undefined) | None if i <= 1 => world_ent.clone(),
        Some(v) => v.clone(),
        None => Value::Undefined,
    };
    let receiver = Value::Object(id);
    let ai_type = text_field(world, id, "type").unwrap_or_else(|| "zombie".into());
    if has_function(world, ACTOR_KILLED) {
        let _ = crate::script::runtime::start(
            world,
            ACTOR_KILLED,
            receiver.clone(),
            vec![at(0), at(1), Value::Int(damage), at(4), at(5), at(7), at(8), Value::Int(0)],
        );
    }
    crate::script::runtime::raise(world, receiver.clone(), "death", vec![at(1), at(4), at(5)]);
    crate::script::runtime::raise(world, receiver.clone(), "killanimscript", Vec::new());
    let death = format!("animscripts/{ai_type}_death::main");
    if has_function(world, &death) {
        let _ = crate::script::runtime::start(world, &death, receiver, Vec::new());
    }
}
