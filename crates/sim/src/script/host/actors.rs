//! The actor think phase: follow the goal over the path graph, move with hull
//! traces, face the way the actor orients, and announce arrival to the script.

use super::natives::engine::trace;
use super::players::entity_field;
use crate::ai::Goal;
use crate::bullet_collision::{CONTENTS_BODY, MASK_PLAYER_SOLID};

/// Actors do not block each other (or themselves: each carries a body mover).
const ACTOR_SOLID: u32 = MASK_PLAYER_SOLID & !CONTENTS_BODY;
use crate::script::{RoundScript, Value};
use bevy_ecs::prelude::World;

const HULL_MINS: [f32; 3] = [-15.0, -15.0, 0.0];
const HULL_MAXS: [f32; 3] = [15.0, 15.0, 72.0];
const STEP_HEIGHT: f32 = 18.0;
const GRAVITY: f32 = 800.0;
const TURN_RATE: f32 = 540.0;
const REPATH_MS: i64 = 1000;
const MIN_GOAL_RADIUS: f32 = 12.0;
const WAYPOINT_RADIUS: f32 = 20.0;

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
fn walk(world: &mut World, from: [f32; 3], dx: f32, dy: f32, dt: f32) -> [f32; 3] {
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
    let now = i64::from(request.tick.0) * i64::from(crate::MATCH_TICK_MS);
    let runtime = world.resource::<RoundScript>();
    if runtime.fault.is_some() || runtime.program.is_none() {
        return;
    }
    let ids: Vec<u64> = runtime
        .engine
        .actors
        .by_object
        .iter()
        .filter(|(_, a)| !a.dead)
        .map(|(id, _)| *id)
        .collect();
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
    let mut announce: Option<&'static str> = None;
    let mut new_origin = origin;
    let mut events: Vec<(std::sync::Arc<str>, String)> = Vec::new();
    for playing in &mut actor.playing {
        for note in playing.advance(dt) {
            if let Some(flag) = &playing.flag {
                events.push((flag.clone(), note));
            }
        }
    }
    let mut carried = false;
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
                let route = world
                    .resource::<RoundScript>()
                    .engine
                    .paths
                    .path(origin, goal);
                match route {
                    Some(path) => {
                        actor.path = path;
                        // Do not backtrack to a node that lies behind the actor.
                        while path_behind(origin, &actor.path) {
                            actor.path.remove(0);
                        }
                        actor.path.reverse();
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
                actor.path.pop();
            }
            if let Some(&next) = actor.path.last() {
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
                    new_origin = walk(world, origin, dx, dy, dt);
                    actor.orient_yaw = dy.atan2(dx).to_degrees();
                }
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
    let reshow = display.is_some() && display != actor.shown;
    if reshow {
        actor.shown = display.clone();
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
        *slot = actor;
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
