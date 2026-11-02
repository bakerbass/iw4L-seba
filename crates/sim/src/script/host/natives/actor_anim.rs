//! Animation natives for actors. BO1's animscripts start flagged animations and
//! wait on their notetracks (`waittillmatch("runanim", "end")`), so each actor
//! keeps a small list of running clips that raise those notifies as time passes.

use super::super::args::{string, vector};
use crate::ai::{Playing, Scripted};
use crate::script::{
    Arc,
    Namespace::{Function, Method},
    NativeRegistry, RoundScript, Value,
};
use bevy_ecs::prelude::World;

const FLAGGED: &[&str] = &[
    "setflaggedanim",
    "setflaggedanimrestart",
    "setflaggedanimlimited",
    "setflaggedanimlimitedrestart",
    "setflaggedanimknob",
    "setflaggedanimknobrestart",
    "setflaggedanimknoblimited",
    "setflaggedanimknoblimitedrestart",
    "setflaggedanimknoball",
    "setflaggedanimknoballrestart",
    "setflaggedanimknoballlimited",
    "setflaggedanimknoballlimitedrestart",
];

const PLAIN: &[&str] = &[
    "setanim",
    "setanimrestart",
    "setanimlimited",
    "setanimlimitedrestart",
    "setanimknob",
    "setanimknobrestart",
    "setanimknoblimited",
    "setanimknoblimitedrestart",
    "setanimknoball",
    "setanimknoballrestart",
    "setanimknoballlimited",
    "setanimknoballlimitedrestart",
];

pub(crate) fn register(registry: &mut NativeRegistry) {
    for name in FLAGGED {
        registry.register(Method, name, |world, receiver, args| {
            let flag = string(args, 0)?;
            play(world, receiver, Some(flag.as_str()), &args[1..])
        });
    }
    for name in PLAIN {
        registry.register(Method, name, |world, receiver, args| {
            play(world, receiver, None, args)
        });
    }
    for name in ["clearanim", "clearanimlimited"] {
        registry.register(Method, name, |world, receiver, args| {
            let Value::Object(id) = receiver else {
                return Err("clearanim receiver is not an actor".into());
            };
            let target = match args.first() {
                Some(Value::Animation { name, .. }) => name.clone(),
                _ => return Ok(Value::Undefined),
            };
            if let Some(actor) = world
                .resource_mut::<RoundScript>()
                .engine
                .actors
                .by_object
                .get_mut(id)
            {
                // Tree nodes such as %root or %body group several clips; only
                // leaves are tracked, so the grouping names clear everything.
                if is_group(&target) {
                    actor.playing.clear();
                } else {
                    actor.playing.retain(|p| *p.name != *target);
                }
            }
            Ok(Value::Undefined)
        });
    }
    registry.register(Method, "useanimtree", |_, _, _| Ok(Value::Undefined));
    registry.register(Method, "animscripted", |world, receiver, args| {
        let Value::Object(id) = receiver else {
            return Err("animscripted receiver is not an actor".into());
        };
        let flag = string(args, 0)?;
        let origin = vector(args, 1)?;
        let angles = vector(args, 2)?;
        let rate = match args.get(6) {
            Some(Value::Float(r)) => *r,
            Some(Value::Int(r)) => *r as f32,
            _ => 1.0,
        };
        let name = match args.get(3) {
            Some(Value::Animation { name, .. }) => name.clone(),
            _ => return Err("animscripted needs an animation".into()),
        };
        let clip = crate::frame::FrameWorld::from_world(world).actor_anim_clip(&name);
        let mut runtime = world.resource_mut::<RoundScript>();
        if let Some(actor) = runtime.engine.actors.by_object.get_mut(id) {
            actor.scripted = Some(Scripted {
                anim: Playing {
                    flag: Some(flag.as_str().into()),
                    name,
                    clip,
                    t: 0.0,
                    rate,
                    ended: false,
                },
                origin,
                angles,
            });
            actor.path.clear();
        }
        runtime.set_object_field(*id, "origin", Value::Vector(origin));
        runtime.set_object_field(*id, "angles", Value::Vector(angles));
        Ok(Value::Undefined)
    });
    for name in ["stopanimscripted", "animscriptedskiprestart"] {
        registry.register(Method, name, |world, receiver, _| {
            if let Value::Object(id) = receiver
                && let Some(actor) = world
                    .resource_mut::<RoundScript>()
                    .engine
                    .actors
                    .by_object
                    .get_mut(id)
            {
                actor.scripted = None;
            }
            Ok(Value::Undefined)
        });
    }
    registry.register(Function, "getanimlength", |world, _, args| {
        let name = anim_name(args, 0)?;
        let frame = crate::frame::FrameWorld::from_world(world);
        let clip = frame
            .actor_anim_clip(&name)
            .or_else(|| frame.player_anim_clip_named(&name))
            .ok_or_else(|| format!("animation '{name}' is not loaded in the simulation"))?;
        Ok(Value::Float(clip.duration()))
    });
    registry.register(Function, "animhasnotetrack", |world, _, args| {
        let name = anim_name(args, 0)?;
        let note = string(args, 1)?;
        let frame = crate::frame::FrameWorld::from_world(world);
        let clip = frame
            .actor_anim_clip(&name)
            .or_else(|| frame.player_anim_clip_named(&name));
        Ok(Value::Int(
            clip.is_some_and(|c| c.notifies.iter().any(|n| n.name.eq_ignore_ascii_case(&note)))
                .into(),
        ))
    });
    registry.register(Function, "getnotetracktimes", |world, _, args| {
        let name = anim_name(args, 0)?;
        let note = string(args, 1)?;
        let frame = crate::frame::FrameWorld::from_world(world);
        let clip = frame
            .actor_anim_clip(&name)
            .or_else(|| frame.player_anim_clip_named(&name));
        let times: Vec<Value> = clip
            .map(|c| {
                c.notifies
                    .iter()
                    .filter(|n| n.name.eq_ignore_ascii_case(&note))
                    .map(|n| Value::Float(n.time))
                    .collect()
            })
            .unwrap_or_default();
        super::super::arrays::new_array(world, times)
    });
}

fn is_group(name: &str) -> bool {
    matches!(name, "root" | "body" | "combatrun" | "walk_and_run_loops")
}

fn anim_name(args: &[Value], index: usize) -> Result<Arc<str>, String> {
    match args.get(index) {
        Some(Value::Animation { name, .. }) => Ok(name.clone()),
        _ => Err(format!("parameter {} is not an animation", index + 1)),
    }
}

/// `weight, blend, rate` follow the animation (and a root animation for the
/// `knoball` forms); only the animation and the rate matter here.
fn play(
    world: &mut World,
    receiver: &Value,
    flag: Option<&str>,
    args: &[Value],
) -> Result<Value, String> {
    let Value::Object(id) = receiver else {
        return Err("animation receiver is not an actor".into());
    };
    let Some(Value::Animation { name, .. }) = args.first() else {
        return Ok(Value::Undefined);
    };
    let name = name.clone();
    let numbers = args
        .iter()
        .skip(1)
        .skip_while(|v| matches!(v, Value::Animation { .. }))
        .collect::<Vec<_>>();
    let number = |i: usize, default: f32| match numbers.get(i) {
        Some(Value::Float(n)) => *n,
        Some(Value::Int(n)) => *n as f32,
        _ => default,
    };
    let (weight, rate) = (number(0, 1.0), number(2, 1.0));
    let clip = crate::frame::FrameWorld::from_world(world).actor_anim_clip(&name);
    let mut runtime = world.resource_mut::<RoundScript>();
    let Some(actor) = runtime.engine.actors.by_object.get_mut(id) else {
        return Ok(Value::Undefined);
    };
    if weight <= 0.0 {
        actor.playing.retain(|p| *p.name != *name);
        return Ok(Value::Undefined);
    }
    if let Some(running) = actor.playing.iter_mut().find(|p| *p.name == *name) {
        running.rate = rate;
        if flag.is_some() {
            running.flag = flag.map(Into::into);
        }
        return Ok(Value::Undefined);
    }
    if flag.is_some() {
        // A flag follows one animation at a time.
        actor.playing.retain(|p| p.flag.as_deref() != flag);
    }
    actor.playing.push(Playing {
        flag: flag.map(Into::into),
        name,
        clip,
        t: 0.0,
        rate,
        ended: false,
    });
    Ok(Value::Undefined)
}
