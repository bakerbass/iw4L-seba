//! Debug overlay: the oriented collision boxes bullets are traced against on script entities
//! (zombies and other AI), drawn as wire boxes. Toggled by `r_hitboxes`.
use crate::draw2d::{Draw2dProvenance, Draw2dQuad};
use crate::gpu_list::{HudTessPass, TessJob};
use bevy::prelude::*;
use net::AuthorityWorld;

/// Boxes farther than this from the camera are skipped.
const MAX_DISTANCE: f32 = 2500.0;
/// Edges are clipped to this distance in front of the camera before projection.
const NEAR_CLIP: f32 = 4.0;
const LINE_WIDTH: f32 = 2.0;

/// The 12 edges of a box, as pairs of corner indices; corner `i` has x/y/z signs from bits 0/1/2.
const EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HitboxDebug {
    pub enabled: bool,
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<HitboxDebug>().add_systems(
        Update,
        draw_hitboxes
            .in_set(frame::InMatch)
            .after(crate::overhead_names::OverheadNamesBuilt)
            .before(crate::plugin::flush_overhead_names_tess),
    );
}

fn draw_hitboxes(
    debug: Res<HitboxDebug>,
    authority: Option<Res<AuthorityWorld>>,
    surface: Res<crate::surface::Hud2dSurface>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut pass: ResMut<HudTessPass>,
) {
    if !debug.enabled || !surface.is_ready() {
        return;
    }
    let (Some(authority), Some((camera, transform))) = (
        authority,
        cameras.iter().find(|(camera, _)| camera.is_active),
    ) else {
        return;
    };
    let eye = transform.translation();
    let forward = *transform.forward();
    let mut quads = Vec::new();
    for entity in authority.0.entity_collision_capabilities() {
        if entity.hidden || !entity.solid {
            continue;
        }
        let Some(collision) = entity
            .dobj
            .as_ref()
            .and_then(|state| state.current_collision.as_ref())
        else {
            continue;
        };
        for bone in &collision.bones {
            let center = Vec3::from_array(bone.center);
            if center.distance_squared(eye) > MAX_DISTANCE * MAX_DISTANCE {
                continue;
            }
            let axes = bone.axes.map(Vec3::from_array);
            let half = Vec3::from_array(bone.half_size);
            let corners: [Vec3; 8] = std::array::from_fn(|i| {
                let sign = |bit: usize| if i & bit == 0 { -1.0 } else { 1.0 };
                center
                    + axes[0] * half.x * sign(1)
                    + axes[1] * half.y * sign(2)
                    + axes[2] * half.z * sign(4)
            });
            let color = if hud_iw4::obituary_is_headshot(bone.part_classification) {
                [1.0, 0.15, 0.15, 1.0]
            } else {
                [0.2, 1.0, 0.3, 1.0]
            };
            for (a, b) in EDGES {
                let Some((a, b)) = clip_to_view(eye, forward, corners[a], corners[b]) else {
                    continue;
                };
                let (Ok(a), Ok(b)) = (
                    camera.world_to_viewport(transform, a),
                    camera.world_to_viewport(transform, b),
                ) else {
                    continue;
                };
                if let Some(quad) = line_quad(a, b, color) {
                    quads.push(quad);
                }
            }
        }
    }
    if quads.is_empty() {
        return;
    }
    // The overhead-name pass owns this raster; append to whatever it produced this frame.
    match &mut pass.overhead_names {
        TessJob::Quads(existing) => existing.extend(quads),
        job => *job = TessJob::Quads(quads),
    }
}

/// Keeps the part of the segment in front of the camera, or `None` when none of it is.
fn clip_to_view(eye: Vec3, forward: Vec3, a: Vec3, b: Vec3) -> Option<(Vec3, Vec3)> {
    let da = (a - eye).dot(forward) - NEAR_CLIP;
    let db = (b - eye).dot(forward) - NEAR_CLIP;
    match (da >= 0.0, db >= 0.0) {
        (true, true) => Some((a, b)),
        (false, false) => None,
        (true, false) => Some((a, a.lerp(b, da / (da - db)))),
        (false, true) => Some((a.lerp(b, da / (da - db)), b)),
    }
}

fn line_quad(a: Vec2, b: Vec2, color: [f32; 4]) -> Option<Draw2dQuad> {
    if !a.is_finite() || !b.is_finite() {
        return None;
    }
    let along = (b - a).try_normalize()?;
    let side = Vec2::new(-along.y, along.x) * (LINE_WIDTH * 0.5);
    Some(Draw2dQuad {
        xy: [
            (a - side).to_array(),
            (a + side).to_array(),
            (b + side).to_array(),
            (b - side).to_array(),
        ],
        st: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        color,
        material: "white".into(),
        material_namespace: crate::images::HUD_CHROME_NAMESPACE,
        provenance: Draw2dProvenance::CgDraw {
            site: "hitbox_debug",
        },
        layer: 1,
        clip: None,
    })
}
