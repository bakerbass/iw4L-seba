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

/// Script brush models (Kino's doors) have local-space surfaces that the world cull emits as
/// plain BSP runs at the identity pose. Pull those surfaces out of the runs and draw them as
/// posed bmodel items for live, drawable script movers only, following the entity's pose.
/// `apply_dpvs_cull` rebuilds `draw_items` each frame, so this runs after it every frame.
pub(crate) fn append_script_brush_draw_items(
    world: Option<ResMut<crate::prepare::scene::world::WorldScene>>,
    stats: Option<ResMut<crate::prepare::scene::cull::DpvsFrameStats>>,
    presented: Option<Res<net::PresentedSnapshot>>,
    runtimes: Query<(&net::CEntity, &net::CEntityRuntime)>,
    mut keys: Local<(usize, Vec<Option<u64>>, Vec<u8>, Vec<bool>)>,
    mut logged: Local<(u32, u32, u32)>,
) {
    use crate::prepare::scene::world::{WorldDrawItem, WorldDrawItemKind};
    let Some(mut world) = world else {
        return;
    };
    let world = &mut *world;
    let identity = std::sync::Arc::as_ptr(&world.runtime_material_catalog) as usize;
    if keys.0 != identity {
        keys.0 = identity;
        keys.1 = world
            .runtime_material_catalog
            .parts()
            .materials
            .iter()
            .map(|material| material.baked_draw_surf)
            .collect();
        keys.2 = world
            .batches
            .iter()
            .map(|batch| batch.primary_light_index)
            .collect();
        keys.3.clear();
        if let Some(cull) = world.cull.as_ref() {
            let surf_n = cull.surface_batch_ranges.len();
            keys.3.resize(surf_n, false);
            for brush in &world.script_brush_models {
                let handle = brush.cmodel_handle as usize;
                if handle == 0 {
                    continue;
                }
                let Some(model) = cull.brush_models.get(handle) else {
                    continue;
                };
                let start = usize::from(model.start_surf);
                for surf in start..(start + usize::from(model.surface_count)).min(surf_n) {
                    keys.3[surf] = true;
                }
            }
        }
    }
    let Some(cull) = world.cull.as_mut() else {
        return;
    };
    let mask = &keys.3;
    let is_script_bmodel = |surf: u16| mask.get(usize::from(surf)).copied().unwrap_or(false);
    let mut stripped = 0u32;
    let mut kept: Vec<WorldDrawItem> = Vec::with_capacity(cull.draw_items.len());
    for item in &cull.draw_items {
        let WorldDrawItemKind::Bsp(_) = item.kind else {
            kept.push(*item);
            continue;
        };
        if !(0..item.run).any(|offset| is_script_bmodel(item.surf.saturating_add(offset))) {
            kept.push(*item);
            continue;
        }
        let mut first = true;
        let mut run_start: Option<u16> = None;
        for offset in 0..=item.run {
            let surf = item.surf.saturating_add(offset);
            let keep = offset < item.run && !is_script_bmodel(surf);
            match (keep, run_start) {
                (true, None) => run_start = Some(surf),
                (false, Some(start)) => {
                    kept.push(WorldDrawItem {
                        surf: start,
                        run: surf - start,
                        setup_key_changed: item.setup_key_changed && first,
                        ..*item
                    });
                    first = false;
                    run_start = None;
                }
                _ => {}
            }
            if offset < item.run && !keep {
                stripped += 1;
            }
        }
    }
    cull.draw_items = kept;
    let at_time = presented
        .as_ref()
        .and_then(|value| value.snapshot())
        .map(|snapshot| net::ServerTime::from_tick(snapshot.tick).ms())
        .unwrap_or(0);
    let mut already = bevy::platform::collections::HashSet::new();
    for item in &cull.draw_items {
        for offset in 0..item.run {
            already.insert(item.surf.saturating_add(offset));
        }
    }
    let mut added = 0u32;
    let mut refused = 0u32;
    let mut drawn = 0u32;
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
        let Some(model) = cull.brush_models.get(handle).copied() else {
            continue;
        };
        drawn += 1;
        let origin = entity_iw4::evaluate_trajectory(&runtime.current.pos, at_time);
        let angles = entity_iw4::evaluate_trajectory(&runtime.current.apos, at_time);
        cull.bmodel_world_from_local[handle] = crate::prepare::scene::world::mat4_from_origin_axis(
            origin,
            crate::prepare::scene::world::bmodel_axis_from_angles(angles),
        );
        let (span_added, span_refused) = crate::prepare::scene::cull::append_bmodel_colour_span(
            cull,
            model.start_surf,
            model.surface_count,
            &mut already,
            &keys.1,
            &keys.2,
        );
        added += span_added;
        refused += span_refused;
    }
    crate::prepare::scene::cull::refresh_draw_items_id(cull);
    for pose in &cull.bmodel_world_from_local {
        for value in pose.to_cols_array() {
            crate::assemble::drawsurf::list::mix_content_id(
                &mut cull.draw_items_id,
                u64::from(value.to_bits()),
            );
        }
    }
    if let Some(mut stats) = stats {
        stats.keys = cull.draw_items.len() as u32;
        stats.rebinds = crate::prepare::scene::cull::draw_item_rebinds(&cull.draw_items);
    }
    if *logged != (drawn, added, stripped) {
        *logged = (drawn, added, stripped);
        diag::info!(
            World,
            "script brush draw items: drawn_movers={drawn} added_surfs={added} refused={refused} stripped_from_bsp_runs={stripped} draw_items={}",
            cull.draw_items.len()
        );
    }
}
