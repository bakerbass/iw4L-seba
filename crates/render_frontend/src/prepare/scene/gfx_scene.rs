pub use render_scene::{
    GfxSceneAdd, GfxSceneClear, HostGfxScene, SceneEntSkinInput, SceneEntSkinInputs,
    SceneEntSkinModel, SceneEntSkinPending, SceneEntSkinPendingModel, SceneEntSurfaceCache,
    ScriptMoverBmodelClaim, SpotShadowEntityOriginTrack, SpotShadowSceneOccupancy,
    expand_scene_ent_pending, gfx_scene_spot_shadow_dobj_slots, gfx_scene_spot_shadow_model_slots,
    hide_part_bits_from_tags, occupy_add_bmodel, occupy_add_dobj, occupy_add_dobj_fx,
    occupy_script_brushes, scene_quat_from_angles, scene_quat_from_viewmodel_axes,
    store_scene_ent_pending,
};
pub(crate) use render_scene::{clear_host_gfx_scene, snapshot_spot_shadow_occupancy};

use bevy::prelude::*;
use render_scene::AddDObjPose;

pub(crate) fn apply_anim_dobj_scene_submissions(
    mut submissions: MessageReader<
        crate::adapters::anim::scene_submission::AnimDObjSceneSubmission,
    >,
    mut scene: ResMut<HostGfxScene>,
    mut skin_inputs: ResMut<SceneEntSkinInputs>,
) {
    for submission in submissions.read() {
        apply_anim_dobj_scene_submission(submission, &mut scene, &mut skin_inputs);
    }
}

fn apply_anim_dobj_scene_submission(
    submission: &crate::adapters::anim::scene_submission::AnimDObjSceneSubmission,
    scene: &mut HostGfxScene,
    skin_inputs: &mut SceneEntSkinInputs,
) {
    occupy_add_dobj_fx(
        &mut scene.scene,
        submission.occupy_model_n as usize,
        submission.has_tree,
        submission.render_fx_flags,
        AddDObjPose {
            origin: submission.origin,
            lighting_origin: submission.lighting_origin,
            radius: submission.radius,
            entnum: submission.entnum,
            quat: submission.quat,
        },
    );
    if submission.store_skin {
        store_scene_ent_pending(
            &mut scene.scene,
            skin_inputs,
            submission.entnum,
            submission
                .models
                .iter()
                .map(|model| SceneEntSkinPendingModel {
                    lod: model.lod,
                    bone_count: model.bone_count,
                    skel: std::sync::Arc::clone(&model.skel),
                })
                .collect(),
            submission.hide_part_bits,
        );
    }
}

pub(crate) fn occupy_script_brush_scene(
    world: Option<Res<crate::prepare::scene::world::WorldScene>>,
    presented: Option<Res<net::PresentedSnapshot>>,
    runtimes: Query<(&net::CEntity, &net::CEntityRuntime)>,
    mut gfx_scene: ResMut<HostGfxScene>,
) {
    let Some(world) = world else {
        return;
    };
    let models = world
        .cull
        .as_ref()
        .map(|cull| cull.brush_models.as_slice())
        .unwrap_or(&[]);
    let at_time = presented
        .as_ref()
        .and_then(|value| value.snapshot())
        .map(|snapshot| net::ServerTime::from_tick(snapshot.tick).ms())
        .unwrap_or(0);
    let mut live = Vec::new();
    for (cent, runtime) in &runtimes {
        let entnum = u32::from(cent.number());
        let dobj_present = gfx_scene.scene.scene_ent_live(entnum);
        let hidden = !dobj_present
            && runtime.next_state.e_type == entity_iw4::ET_SCRIPTMOVER
            && runtime.next_state.solid == entity_iw4::SCRIPT_MOVER_BMODEL_SOLID
            && runtime.next_state.e_flags & entity_iw4::CG_SCRIPT_MOVER_NODRAW != 0;
        if !hidden
            && !entity_iw4::script_mover_add_bmodel(
                runtime.next_state.e_type,
                runtime.next_state.e_flags,
                runtime.next_state.solid,
                dobj_present,
            )
        {
            continue;
        }
        let Ok(model_index) = u32::try_from(runtime.next_state.index) else {
            continue;
        };
        let origin = entity_iw4::evaluate_trajectory(&runtime.current.pos, at_time);
        let angles = entity_iw4::evaluate_trajectory(&runtime.current.apos, at_time);
        live.push(ScriptMoverBmodelClaim {
            model_index,
            origin,
            angles,
            entnum,
            hidden,
        });
    }
    occupy_script_brushes(
        &mut gfx_scene.scene,
        models,
        &world.script_brush_models,
        &live,
    );
}

/// Script brush models (Kino's doors) only get surfaces in the scene; give their
/// surfaces world draw items too, and follow the live entity's pose as it moves.
pub(crate) fn append_script_brush_draw_items(
    world: Option<ResMut<crate::prepare::scene::world::WorldScene>>,
    presented: Option<Res<net::PresentedSnapshot>>,
    runtimes: Query<(&net::CEntity, &net::CEntityRuntime)>,
    mut seeded_for: Local<usize>,
) {
    let Some(mut world) = world else {
        return;
    };
    let world = &mut *world;
    let identity = std::sync::Arc::as_ptr(&world.runtime_material_catalog) as usize;
    let seed = *seeded_for != identity;
    let baked_material_keys: Vec<Option<u64>> = if seed {
        world
            .runtime_material_catalog
            .parts()
            .materials
            .iter()
            .map(|material| material.baked_draw_surf)
            .collect()
    } else {
        Vec::new()
    };
    let batch_primary_lights: Vec<u8> = if seed {
        world
            .batches
            .iter()
            .map(|batch| batch.primary_light_index)
            .collect()
    } else {
        Vec::new()
    };
    let Some(cull) = world.cull.as_mut() else {
        return;
    };
    let mut changed = false;
    if seed {
        *seeded_for = identity;
        let mut already = bevy::platform::collections::HashSet::new();
        for item in &cull.draw_items {
            for offset in 0..item.run {
                already.insert(item.surf.saturating_add(offset));
            }
        }
        for brush in &world.script_brush_models {
            let Some(model) = cull.brush_models.get(brush.cmodel_handle as usize).copied() else {
                continue;
            };
            if brush.cmodel_handle == 0 {
                continue;
            }
            let (added, _) = crate::prepare::scene::cull::append_bmodel_colour_span(
                cull,
                model.start_surf,
                model.surface_count,
                &mut already,
                &baked_material_keys,
                &batch_primary_lights,
            );
            changed |= added > 0;
        }
    }
    let at_time = presented
        .as_ref()
        .and_then(|value| value.snapshot())
        .map(|snapshot| net::ServerTime::from_tick(snapshot.tick).ms())
        .unwrap_or(0);
    for (_, runtime) in &runtimes {
        let state = &runtime.next_state;
        if state.e_type != entity_iw4::ET_SCRIPTMOVER
            || state.solid != entity_iw4::SCRIPT_MOVER_BMODEL_SOLID
            || state.e_flags & entity_iw4::CG_SCRIPT_MOVER_NODRAW != 0
        {
            continue;
        }
        let Ok(handle) = usize::try_from(state.index) else {
            continue;
        };
        if handle == 0 || handle >= cull.bmodel_world_from_local.len() {
            continue;
        }
        let origin = entity_iw4::evaluate_trajectory(&runtime.current.pos, at_time);
        let angles = entity_iw4::evaluate_trajectory(&runtime.current.apos, at_time);
        let pose = crate::prepare::scene::world::mat4_from_origin_axis(
            origin,
            crate::prepare::scene::world::bmodel_axis_from_angles(angles),
        );
        if cull.bmodel_world_from_local[handle] != pose {
            cull.bmodel_world_from_local[handle] = pose;
            changed = true;
        }
    }
    if changed {
        crate::prepare::scene::cull::refresh_draw_items_id(cull);
        for pose in &cull.bmodel_world_from_local {
            for value in pose.to_cols_array() {
                crate::assemble::drawsurf::list::mix_content_id(
                    &mut cull.draw_items_id,
                    u64::from(value.to_bits()),
                );
            }
        }
    }
}
