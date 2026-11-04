use bevy::prelude::*;
use net::AuthorityWorld;

use crate::ConsoleCommand;

use super::echo::ConsoleEcho;

pub(crate) fn route_hitvol_commands(
    mut events: MessageReader<ConsoleCommand>,
    mut echo: ConsoleEcho,
    authority: Option<Res<AuthorityWorld>>,
    mut overlay: ResMut<hud::HitboxDebug>,
) {
    for cmd in events.read() {
        if matches!(cmd.name.as_str(), "r_hitboxes" | "hitbox_toggle") {
            let requested = match (cmd.name.as_str(), cmd.args.as_slice()) {
                ("hitbox_toggle", []) => Some(!overlay.enabled),
                ("r_hitboxes", []) => None,
                ("r_hitboxes", [arg]) => match arg.to_ascii_lowercase().as_str() {
                    "1" | "on" | "true" => Some(true),
                    "0" | "off" | "false" => Some(false),
                    "toggle" => Some(!overlay.enabled),
                    _ => {
                        echo.write("usage: r_hitboxes [0|1|toggle]");
                        continue;
                    }
                },
                _ => {
                    echo.write("usage: r_hitboxes [0|1|toggle]");
                    continue;
                }
            };
            if let Some(enabled) = requested {
                overlay.enabled = enabled;
            }
            echo.write(format!(
                "r_hitboxes = {}",
                u8::from(overlay.enabled)
            ));
            continue;
        }
        if cmd.name != "hitvol" {
            continue;
        }
        let Some(authority) = authority.as_deref() else {
            echo.write("hitvol: no authority world on this client");
            continue;
        };
        for line in hitvol_report(&authority.0) {
            echo.write(line);
        }
    }
}

pub(super) fn hitvol_report(world: &sim::SimWorld) -> Vec<String> {
    let census = world.collision_census();
    let w = &census.world;
    let p = &census.players;
    let e = &census.entities;
    let mut out = vec![
        format!(
            "hitvol world: brushes {} leaves {} leafbrushes {} meshtris {} cmodels {} smodels {} (with tris {}) pen_table {}",
            w.brushes,
            w.bsp_leaves,
            w.leafbrushes,
            w.mesh_tris,
            w.cmodels,
            w.static_models,
            w.static_models_with_tris,
            w.pen_table_loaded
        ),
        format!(
            "hitvol players: poses {} bones {} aabb-only {} bone-count {}..{} with-head-bone {}",
            p.poses, p.with_bones, p.aabb_only, p.min_bones, p.max_bones, p.with_head_bone
        ),
        format!(
            "hitvol entities: rows {} colltris {} boxes {} brush {} authored-no-collision {} not-bullet-solid {} no-dobj {} no-capability {} materialize-failed {} linked-brushes {}",
            e.rows,
            e.colltris,
            e.boxes_only,
            e.brush_only,
            e.no_collision_authored,
            e.not_bullet_solid,
            e.no_dobj,
            e.no_capability,
            e.materialize_failed,
            e.linked_brushes
        ),
    ];
    if !e.no_clip_sample.is_empty() {
        out.push(format!(
            "hitvol entities with no model clip: {}",
            e.no_clip_sample.join(", ")
        ));
    }
    if let Some(error) = &p.materialize_error {
        out.push(format!("hitvol players: materialize error {error}"));
    }
    for kit in &census.kits {
        out.push(format!(
            "hitvol kit `{}`: {} bones {} boxes {} collsurfs {} colltris {} lod {}",
            kit.key,
            kit.clip(),
            kit.bones,
            kit.bone_boxes,
            kit.coll_surfs,
            kit.coll_tris,
            kit.coll_lod
        ));
    }
    for row in world.hitvol_dump() {
        out.push(format!(
            "hitvol client {:?}: geom {} bones {} pose {} body `{}` head `{}` controller {}",
            row.client.map(|c| c.0),
            row.geom,
            row.bone_count,
            row.pose_kind,
            row.body_key,
            row.head_key,
            row.controller
        ));
    }
    out
}
