//! The actor think phase: follow the goal over the path graph, move with hull
//! traces, face the way the actor orients, and announce arrival to the script.

use super::natives::engine::trace;
use super::players::entity_field;
use crate::ai::{Goal, Traverse};
use crate::ai::path_graph::{NODE_TYPE_NEGOTIATION_BEGIN, NODE_TYPE_NEGOTIATION_END};
use crate::bullet_collision::{CONTENTS_BODY, MASK_PLAYER_SOLID};

/// Actors do not block each other (or themselves: each carries a body mover). They are stopped by world solids and monster clip;
/// the invisible player clip that fences the map in for players does not apply to them.
const CONTENTS_PLAYERCLIP: u32 = 0x0001_0000;
const CONTENTS_MONSTERCLIP: u32 = 0x0002_0000;
pub(crate) const ACTOR_SOLID: u32 =
    (MASK_PLAYER_SOLID & !CONTENTS_BODY & !CONTENTS_PLAYERCLIP) | CONTENTS_MONSTERCLIP;
use crate::script::{RoundScript, Value};
use bevy_ecs::prelude::World;

/// Centre-to-centre distance two bodies keep (two hull radii).
const BODY_GAP: f32 = 30.0;
const HULL_MINS: [f32; 3] = [-15.0, -15.0, 0.0];
const HULL_MAXS: [f32; 3] = [15.0, 15.0, 72.0];
const STEP_HEIGHT: f32 = 18.0;
const GRAVITY: f32 = 800.0;
const TURN_RATE: f32 = 540.0;
const REPATH_MS: i64 = 1000;
const MIN_GOAL_RADIUS: f32 = 12.0;
const WAYPOINT_RADIUS: f32 = 20.0;
const TRAVERSE_TIMEOUT_MS: i64 = 8000;

/// Units per second for each `zombie_move_speed` the scripts pick.
fn move_speed(name: &str) -> f32 {
    match name {
        "walk" => 40.0,
        "run" => 70.0,
        "sprint" | "super_sprint" => 105.0,
        "crawl" => 25.0,
        _ => 60.0,
    }
}

fn vec3(value: Value) -> [f32; 3] {
    match value {
        Value::Vector(v) => v,
        _ => [0.0; 3],
    }
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

fn flat_dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Move `from` by `(dx, dy)`, stepping over low ledges and sliding along walls,
/// then settle onto the floor (or fall).
fn walk(
    world: &mut World,
    from: [f32; 3],
    dx: f32,
    dy: f32,
    dt: f32,
    others: &[[f32; 3]],
) -> [f32; 3] {
    let mut best = from;
    let mut best_travel = -1.0;
    // Try stepping up over a ledge first, then moving level.
    for lift in [STEP_HEIGHT, 0.0] {
        let up = [from[0], from[1], from[2] + lift];
        for (mx, my) in [(dx, dy), (dx, 0.0), (0.0, dy)] {
            if mx == 0.0 && my == 0.0 {
                continue;
            }
            let target = [up[0] + mx, up[1] + my, up[2]];
            let t = trace(world, up, target, HULL_MINS, HULL_MAXS, ACTOR_SOLID);
            if t.allsolid != 0 || t.startsolid != 0 {
                continue;
            }
            let end = lerp(up, target, t.fraction);
            // Players and other actors are solid: a step that closes in on one is
            // refused, which also slides the actor along it.
            if others.iter().any(|o| {
                (o[2] - end[2]).abs() < HULL_MAXS[2]
                    && flat_dist(end, *o) < BODY_GAP
                    && flat_dist(end, *o) < flat_dist(from, *o)
            }) {
                continue;
            }
            let travel = t.fraction * (mx * mx + my * my).sqrt();
            if travel > best_travel {
                best_travel = travel;
                best = lerp(up, target, t.fraction);
            }
            if t.fraction >= 1.0 {
                break;
            }
        }
        if best_travel > 0.5 {
            break;
        }
    }
    if best_travel < 0.0 {
        return from;
    }
    let reach = STEP_HEIGHT + 24.0;
    let below = [best[0], best[1], best[2] - reach];
    let ground = trace(world, best, below, HULL_MINS, HULL_MAXS, ACTOR_SOLID);
    if ground.allsolid != 0 {
        return from;
    }
    if ground.fraction < 1.0 {
        lerp(best, below, ground.fraction)
    } else {
        [best[0], best[1], from[2] - GRAVITY * dt * dt]
    }
}

pub(crate) fn think(world: &mut World) {
    let request = world.resource::<crate::step::StepRequest>();
    if !request.reason.advances_authority_world() {
        return;
    }
    let tick_no = request.tick.0;
    let now = i64::from(request.tick.0) * i64::from(crate::MATCH_TICK_MS);
    let runtime = world.resource::<RoundScript>();
    if runtime.fault.is_some() || runtime.program.is_none() {
        return;
    }
    // Corpses keep thinking so their death clip plays and raises its notetracks.
    let ids: Vec<u64> = runtime.engine.actors.by_object.keys().copied().collect();
    let living: Vec<u64> = runtime
        .engine
        .actors
        .by_object
        .iter()
        .filter(|(_, a)| !a.dead)
        .map(|(id, _)| *id)
        .collect();
    // The engine wakes three actors per frame, in turn, for the animscripts'
    // slow checks (`waittill("do_slow_things")`).
    if !living.is_empty() {
        let turn = tick_no as usize * 3;
        for k in 0..3.min(living.len()) {
            let id = living[(turn + k) % living.len()];
            crate::script::runtime::raise(world, Value::Object(id), "do_slow_things", Vec::new());
        }
    }
    let dt = crate::MATCH_TICK_MS as f32 / 1000.0;
    for id in ids {
        think_one(world, id, now, dt);
    }
}

fn think_one(world: &mut World, id: u64, now: i64, dt: f32) {
    let mut actor = match world.resource::<RoundScript>().engine.actors.by_object.get(&id) {
        Some(actor) => actor.clone(),
        None => return,
    };
    let origin = vec3(entity_field(world, id, "origin"));
    let mut angles = vec3(entity_field(world, id, "angles"));
    let goal_pos = match actor.goal {
        Goal::None => None,
        Goal::Position(p) => Some(p),
        Goal::Entity(e) => Some(vec3(entity_field(world, e, "origin"))),
    };
    // The engine makes an actor's favorite enemy its current enemy; the combat
    // and melee animscripts act on `self.enemy`.
    {
        let favorite = entity_field(world, id, "favoriteenemy");
        let enemy = entity_field(world, id, "enemy");
        if favorite != enemy {
            world
                .resource_mut::<RoundScript>()
                .set_object_field(id, "enemy", favorite);
        }
    }
    let mut announce: Option<&'static str> = None;
    let mut new_origin = origin;
    let mut events: Vec<(std::sync::Arc<str>, String)> = Vec::new();
    let crossing = actor.traverse.is_some();
    let mut traverse_move = [0.0f32; 3];
    for playing in &mut actor.playing {
        let before = playing.t;
        for note in playing.advance(dt) {
            if let Some(flag) = &playing.flag {
                events.push((flag.clone(), note));
            }
        }
        // The traverse animation carries the actor by its root motion.
        if crossing
            && playing.flag.as_deref() == Some("traverseAnim")
            && let Some(clip) = &playing.clip
        {
            let (a, b) = (
                clip.abs_delta_trans(before.min(1.0)),
                clip.abs_delta_trans(playing.t.min(1.0)),
            );
            let (sin, cos) = angles[1].to_radians().sin_cos();
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            traverse_move[0] += dx * cos - dy * sin;
            traverse_move[1] += dx * sin + dy * cos;
            traverse_move[2] += b[2] - a[2];
        }
    }
    let mut carried = false;
    let mut launch: Option<(u16, u16)> = None;
    if let Some(tv) = actor.traverse.as_mut() {
        carried = true;
        if tv.launch {
            tv.launch = false;
            launch = Some((tv.start, tv.end));
        }
        new_origin = [
            origin[0] + traverse_move[0],
            origin[1] + traverse_move[1],
            origin[2] + traverse_move[2],
        ];
        if now - tv.since_ms > TRAVERSE_TIMEOUT_MS {
            tv.finishing = true;
        }
        if tv.finishing {
            let end = tv.end;
            actor.traverse = None;
            actor.arrived = false;
            actor.path.clear();
            actor.path_nodes.clear();
            actor.playing.retain(|p| p.flag.as_deref() != Some("traverseAnim"));
            new_origin = settle_at_node(world, end, new_origin);
        }
    }
    if let Some(scripted) = actor.scripted.as_mut() {
        carried = true;
        for note in scripted.anim.advance(dt) {
            // A scripted clip plays once, even a looping one.
            if note == "end" {
                scripted.anim.ended = true;
            }
            if let Some(flag) = &scripted.anim.flag {
                events.push((flag.clone(), note));
            }
        }
        if let Some(clip) = &scripted.anim.clip {
            let delta = clip.abs_delta_trans(scripted.anim.t);
            let (sin, cos) = scripted.angles[1].to_radians().sin_cos();
            new_origin = [
                scripted.origin[0] + delta[0] * cos - delta[1] * sin,
                scripted.origin[1] + delta[0] * sin + delta[1] * cos,
                scripted.origin[2] + delta[2],
            ];
            angles = scripted.angles;
        }
        if scripted.anim.ended {
            actor.scripted = None;
        }
    }
    if let Some(goal) = goal_pos.filter(|_| !actor.arrived && !carried) {
        let radius = match entity_field(world, id, "goalradius") {
            Value::Float(r) => r,
            Value::Int(r) => r as f32,
            _ => 0.0,
        }
        .max(MIN_GOAL_RADIUS);
        if flat_dist(origin, goal) <= radius && (origin[2] - goal[2]).abs() < 96.0 {
            actor.arrived = true;
            actor.path.clear();
            announce = Some("goal");
            actor.face_yaw = actor.goal_yaw;
        } else {
            let stale = actor.path.is_empty()
                || flat_dist(actor.path_goal, goal) > 48.0
                || now >= actor.repath_at_ms;
            if stale {
                actor.repath_at_ms = now + REPATH_MS;
                actor.path_goal = goal;
                let graph = world.resource::<RoundScript>().engine.paths.clone();
                let route = {
                    let cell = std::cell::RefCell::new(&mut *world);
                    graph.path_with_nodes(origin, goal, &|node| {
                        let there = [node[0], node[1], origin[2] + STEP_HEIGHT];
                        let from = [origin[0], origin[1], origin[2] + STEP_HEIGHT];
                        let mut world = cell.borrow_mut();
                        let t = trace(&mut **world, from, there, HULL_MINS, HULL_MAXS, ACTOR_SOLID);
                        t.allsolid == 0 && t.startsolid == 0 && t.fraction >= 1.0
                    })
                };
                match route {
                    Some(path) => {
                        actor.path = path.iter().map(|(_, point)| *point).collect();
                        actor.path_nodes = path.iter().map(|(node, _)| *node).collect();
                        // Do not backtrack to a node that lies behind the actor.
                        while path_behind(origin, &actor.path) {
                            // A traversal's start is never behind: the actor walks
                            // back to it and crosses.
                            if let (Some(Some(a)), Some(Some(b))) =
                                (actor.path_nodes.first(), actor.path_nodes.get(1))
                                && negotiation_pair(world, *a, *b)
                            {
                                break;
                            }
                            actor.path.remove(0);
                            actor.path_nodes.remove(0);
                        }
                        actor.path.reverse();
                        actor.path_nodes.reverse();
                    }
                    None => {
                        actor.path.clear();
                        actor.arrived = false;
                        announce = Some("bad_path");
                        diag::info!(Sim, "actor: no path for {id} from {origin:?} to {goal:?}");
                        actor.goal = Goal::None;
                    }
                }
            }
            while actor
                .path
                .last()
                .is_some_and(|p| flat_dist(origin, *p) < WAYPOINT_RADIUS && actor.path.len() > 1)
            {
                // Reaching the start of a negotiation link hands the actor to its
                // traverse animscript.
                let here = actor.path_nodes.last().copied().flatten();
                let there = actor
                    .path_nodes
                    .len()
                    .checked_sub(2)
                    .and_then(|i| actor.path_nodes.get(i))
                    .copied()
                    .flatten();
                if let (Some(a), Some(b)) = (here, there)
                    && negotiation_pair(world, a, b)
                {
                    actor.traverse = Some(Traverse {
                        start: a,
                        end: b,
                        since_ms: now,
                        launch: true,
                        finishing: false,
                    });
                    actor.path.clear();
                    actor.path_nodes.clear();
                    break;
                }
                actor.path.pop();
                actor.path_nodes.pop();
            }
            if let (Some(&next), None) = (actor.path.last(), actor.traverse.as_ref()) {
                let speed_name = match entity_field(world, id, "zombie_move_speed") {
                    Value::String(s) => s.to_string(),
                    _ => String::new(),
                };
                let (mut dx, mut dy) = (next[0] - origin[0], next[1] - origin[1]);
                let len = (dx * dx + dy * dy).sqrt();
                if len > 0.001 {
                    let animated = actor
                        .playing
                        .iter()
                        .rev()
                        .filter_map(|p| p.clip.as_ref().map(|c| c.move_speed() * p.rate))
                        .find(|speed| *speed > 1.0);
                    let speed = animated.unwrap_or_else(|| move_speed(&speed_name));
                    let step = (speed * dt).min(len);
                    dx *= step / len;
                    dy *= step / len;
                    let own = world
                        .resource::<RoundScript>()
                        .entities
                        .get(&id)
                        .map_or(-1, |e| e.number);
                    let others: Vec<[f32; 3]> = {
                        let frame = crate::frame::FrameWorld::from_world(world);
                        crate::step::alive_body_clips(&frame)
                            .into_iter()
                            .filter(|body| i32::from(body.entnum) != own)
                            .map(|body| body.origin)
                            .collect()
                    };
                    new_origin = walk(world, origin, dx, dy, dt, &others);
                    actor.orient_yaw = dy.atan2(dx).to_degrees();
                }
            }
        }
    }
    // The engine runs the move animscript while an actor walks and the combat
    // (stand) animscript when it stops.
    let mut switch_script: Option<bool> = None;
    if !actor.dead && actor.traverse.is_none() && actor.scripted.is_none() {
        if new_origin != origin {
            actor.moving_ticks += 1;
            actor.still_ticks = 0;
            if !actor.moving_script && actor.moving_ticks >= 2 {
                actor.moving_script = true;
                switch_script = Some(true);
            }
        } else {
            actor.still_ticks += 1;
            actor.moving_ticks = 0;
            if actor.moving_script && actor.still_ticks >= 6 {
                actor.moving_script = false;
                switch_script = Some(false);
            }
        }
    }
    let wants_motion_facing = actor.orient.is_empty() || actor.orient.starts_with("face motion");
    if wants_motion_facing && new_origin != origin {
        let delta = math_iw4::angle_subtract(actor.orient_yaw, angles[1]);
        let turn = delta.clamp(-TURN_RATE * dt, TURN_RATE * dt);
        angles[1] += turn;
    }
    if let Some(yaw) = actor.face_yaw {
        let delta = math_iw4::angle_subtract(yaw, angles[1]);
        if delta.abs() <= TURN_RATE * dt {
            angles[1] = yaw;
            actor.face_yaw = None;
            events.push(("orientdone".into(), String::new()));
        } else {
            angles[1] += delta.signum() * TURN_RATE * dt;
        }
    }
    let display = actor
        .scripted
        .as_ref()
        .map(|s| &s.anim)
        .or_else(|| actor.playing.iter().rev().find(|p| p.clip.is_some()))
        .map(|p| p.name.clone());
    let reshow =
        display.is_some() && (display != actor.shown || actor.anim_serial != actor.shown_serial);
    if reshow {
        actor.shown = display.clone();
        actor.shown_serial = actor.anim_serial;
    }
    let mut runtime = world.resource_mut::<RoundScript>();
    if reshow && let Some(entity) = runtime.entities.get_mut(&id) {
        entity.anim_op = Some(display);
    }
    if new_origin != origin {
        runtime.set_object_field(id, "origin", Value::Vector(new_origin));
    }
    runtime.set_object_field(id, "angles", Value::Vector(angles));
    if let Some(slot) = runtime.engine.actors.by_object.get_mut(&id) {
        actor.origin = Some(new_origin);
        *slot = actor;
    }
    if let Some(moving) = switch_script {
        let ai_type = match entity_field(world, id, "type") {
            Value::String(t) => t.to_string(),
            _ => "zombie".to_owned(),
        };
        let function = format!(
            "animscripts/{ai_type}_{}::main",
            if moving { "move" } else { "combat" }
        );
        let known = world
            .resource::<RoundScript>()
            .program
            .as_ref()
            .is_some_and(|p| p.names.contains_key(&function.to_ascii_lowercase()));
        if known {
            crate::script::runtime::raise(world, Value::Object(id), "killanimscript", Vec::new());
            if let Err(fault) =
                crate::script::runtime::start(world, &function, Value::Object(id), Vec::new())
            {
                diag::warn!(Sim, "actor: {function}: {fault:?}");
            }
        }
    }
    if let Some((start, _)) = launch {
        let animscript = traverse_animscript(world, start);
        if let Some(animscript) = animscript {
            let function = format!("animscripts/traverse/{animscript}::main");
            let known = world
                .resource::<RoundScript>()
                .program
                .as_ref()
                .is_some_and(|p| p.names.contains_key(&function.to_ascii_lowercase()));
            if known {
                if let Err(fault) = crate::script::runtime::start(
                    world,
                    &function,
                    Value::Object(id),
                    Vec::new(),
                ) {
                    diag::warn!(Sim, "actor: traverse {function}: {fault:?}");
                }
            } else {
                diag::warn!(Sim, "actor: traverse script {function} is not loaded");
            }
        }
    }
    if let Some(name) = announce {
        crate::script::runtime::raise(world, Value::Object(id), name, Vec::new());
    }
    for (flag, note) in events {
        let args = if note.is_empty() { Vec::new() } else { vec![Value::string(&note)] };
        crate::script::runtime::raise(world, Value::Object(id), &flag, args);
    }
}

fn path_behind(origin: [f32; 3], path: &[[f32; 3]]) -> bool {
    match path {
        [first, second, ..] => {
            let d = |a: [f32; 3], b: [f32; 3]| {
                ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
            };
            d(origin, *second) < d(*first, *second)
        }
        _ => false,
    }
}

/// Whether the link `a` -> `b` is a traversal: a negotiation begin node into
/// its end node.
fn negotiation_pair(world: &World, a: u16, b: u16) -> bool {
    let paths = &world.resource::<RoundScript>().engine.paths;
    matches!(
        (paths.node(usize::from(a)), paths.node(usize::from(b))),
        (Some(from), Some(to))
            if from.kind == NODE_TYPE_NEGOTIATION_BEGIN
                && to.kind == NODE_TYPE_NEGOTIATION_END
    )
}

/// Put the actor on the floor under the traversal's end node.
fn settle_at_node(world: &mut World, node: u16, fallback: [f32; 3]) -> [f32; 3] {
    let Some(end) = world
        .resource::<RoundScript>()
        .engine
        .paths
        .node(usize::from(node))
        .map(|n| n.origin)
    else {
        return fallback;
    };
    let from = [end[0], end[1], end[2] + 8.0];
    let to = [end[0], end[1], end[2] - 96.0];
    let ground = trace(world, from, to, HULL_MINS, HULL_MAXS, ACTOR_SOLID);
    if ground.allsolid != 0 || ground.startsolid != 0 {
        return [end[0], end[1], fallback[2]];
    }
    lerp(from, to, ground.fraction)
}

/// The animscript a negotiation start node names. The decoded path data does not
/// carry it reliably, so it is read from the map's own node entity at the same
/// place.
fn traverse_animscript(world: &mut World, start: u16) -> Option<String> {
    let at = world
        .resource::<RoundScript>()
        .engine
        .paths
        .node(usize::from(start))?
        .origin;
    let mut runtime = world.resource_mut::<RoundScript>();
    let ids = runtime.engine.map_nodes.clone();
    ids.into_iter().find_map(|node| {
        let Value::Vector(o) = runtime.object_field(node, "origin") else {
            return None;
        };
        if (o[0] - at[0]).abs() > 1.0 || (o[1] - at[1]).abs() > 1.0 || (o[2] - at[2]).abs() > 1.0 {
            return None;
        }
        match runtime.object_field(node, "animscript") {
            Value::String(name) => Some(name.to_string()),
            _ => None,
        }
    })
}
