//! Single-player builtins that BO1 zombies calls during setup and play, and
//! that the multiplayer build never had. Anything not here is bound to a
//! counted, logged no-op by `t5_sp_stubs`.

use super::super::args::{arg, float, vector};
use super::super::arrays::new_array;
use crate::script::{Namespace::Function, NativeRegistry, RoundScript, Value};

pub(crate) fn register(registry: &mut NativeRegistry) {
    // `setsaveddvar` is `setdvar` that persists across saves; there are no saves.
    registry.register(Function, "setsaveddvar", |world, _, args| {
        let name = super::iw4::dvar_name(args)?;
        let value = super::iw4::dvar_value(args)?;
        super::iw4::set_dvar(world, &name, &value);
        Ok(Value::Undefined)
    });
    // The zombie cap is enforced by the actor runtime when it exists.
    registry.register(Function, "setailimit", |_, _, _| Ok(Value::Undefined));

    registry.register(Function, "getplayers", |world, _, _| {
        let players = player_objects(world);
        new_array(world, players)
    });
    registry.register(Function, "getnumconnectedplayers", |world, _, _| {
        Ok(Value::Int(player_objects(world).len() as i32))
    });
    registry.register(Function, "getnumexpectedplayers", |world, _, _| {
        Ok(Value::Int(player_objects(world).len() as i32))
    });

    registry.register(Function, "ismp", |_, _, _| Ok(Value::Int(0)));
    // No network clients and no saves: scripts branch on these as booleans.
    registry.register(Function, "numremoteclients", |_, _, _| Ok(Value::Int(0)));
    registry.register(Function, "issaverecentlyloaded", |_, _, _| Ok(Value::Int(0)));
    registry.register(Function, "issavesuccessful", |_, _, _| Ok(Value::Int(0)));
    registry.register(Function, "getvisionsetnaked", |_, _, _| {
        Ok(Value::string("default"))
    });
    registry.register(Function, "getdifficulty", |_, _, _| Ok(Value::string("medium")));
    registry.register(Function, "isvec", |_, _, args| {
        Ok(Value::Int(matches!(arg(args, 0)?, Value::Vector(_)).into()))
    });
    registry.register(Function, "absangleclamp180", |_, _, args| {
        Ok(Value::Float(angle_clamp_180(float(args, 0)?).abs()))
    });
    registry.register(Function, "distance2dsquared", |_, _, args| {
        let (a, b) = (vector(args, 0)?, vector(args, 1)?);
        let (dx, dy) = (a[0] - b[0], a[1] - b[1]);
        Ok(Value::Float(dx * dx + dy * dy))
    });
    registry.register(Function, "lerpfloat", |_, _, args| {
        let (a, b, t) = (float(args, 0)?, float(args, 1)?, float(args, 2)?);
        Ok(Value::Float(a + (b - a) * t))
    });
    registry.register(Function, "getsnapshotindexarray", |world, _, _| {
        new_array(world, Vec::new())
    });
}

fn player_objects(world: &bevy_ecs::prelude::World) -> Vec<Value> {
    world
        .resource::<RoundScript>()
        .players
        .values()
        .map(|slot| Value::Object(slot.object))
        .collect()
}

fn angle_clamp_180(angle: f32) -> f32 {
    let wrapped = angle.rem_euclid(360.0);
    if wrapped > 180.0 { wrapped - 360.0 } else { wrapped }
}
