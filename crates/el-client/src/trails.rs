//! What a figure's weapon and hands draw (`docs/CHAMPIONS.md`, "The motion"):
//!
//! - **A swing's arc**: a ribbon over the outer part of the blade, from where it was a
//!   tenth of a second ago to where it is, brightest at the tip and fading as it goes, in its
//!   champion's colours. Riot's melee attacks read by their swipes; ours are the blade's own path,
//!   drawn from its tip (`<prop>.tip`, `figure::Sockets`) as the strike comes round
//!   (`Pose::swing`).
//! - **A light gathering in the casting hand** (or at the tip of what it casts with) as a cast
//!   winds up, growing to its release (`Pose::charge`).
//!
//! One of each a seat, made at the start; a ribbon's mesh is written only while it shows.
//! `champions.rs` (or the anim stage, `animstage.rs`) says what each seat's arms do this frame
//! ([`Arms`]), and it's let go once drawn. An arc ages by the picture's clock (Bevy's `Time`),
//! which the stages step.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use el_sim::units::SEATS;

use crate::figure::FiguresSet;
use crate::view::facing_camera;

/// How long an arc lasts behind the blade (s), and how many points of it are kept.
const LIFE: f64 = 0.08;
const POINTS: usize = 12;
/// How bright an arc is at its tip, and at its inner edge (of its colour).
const TIP: f32 = 0.6;
const INNER: f32 = 0.0;
/// Where an arc starts along the blade, from the grip (the outer part: a crescent, not a sheet).
pub const ARC_FROM: f32 = 0.6;
/// A gathering light's size (m) as it starts and as it's released, and its brightness.
const GLOW_FROM: f32 = 0.3;
const GLOW_TO: f32 = 1.0;
const GLOW: f32 = 2.2;

/// What each seat's arms do this frame (`champions.rs` writes it, the trails draw it).
#[derive(Resource, Default)]
pub struct Arms {
    /// The blade's inner and outer points while it swings.
    pub swing: [Option<(Vec3, Vec3)>; SEATS],
    /// The casting hand (or tip) and how far through its wind-up the cast is.
    pub glow: [Option<(Vec3, f32)>; SEATS],
    /// Its champion's colour (linear, as `fx::look` gives it).
    pub color: [[f32; 3]; SEATS],
}

/// An arc's or a glow's mesh (the anim stage keeps them in its picture).
#[derive(Component)]
pub struct TrailView;

struct Arc {
    entity: Entity,
    mesh: Handle<Mesh>,
    /// The points kept, oldest first: inner, outer, when.
    points: Vec<(Vec3, Vec3, f64)>,
    shown: bool,
}

struct Glow {
    entity: Entity,
    mat: Handle<StandardMaterial>,
    color: [f32; 3],
}

#[derive(Resource)]
struct Trails {
    arcs: Vec<Arc>,
    glows: Vec<Glow>,
    /// Arcs drawn, all told (`window.__el.swing_arcs`).
    drawn: u64,
}

pub struct TrailsPlugin;

impl Plugin for TrailsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Arms>().add_systems(Startup, setup).add_systems(Update, draw.after(FiguresSet));
    }
}

fn ribbon() -> Mesh {
    let n = POINTS * 2;
    let mut idx = Vec::with_capacity((POINTS - 1) * 6);
    for k in 0..(POINTS - 1) as u32 {
        let a = 2 * k;
        idx.extend([a, a + 1, a + 3, a, a + 3, a + 2]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; n])
        .with_inserted_indices(Indices::U32(idx))
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Light that adds, unlit, both sides: the vertex colours are the light.
    let additive = StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        fog_enabled: false,
        ..default()
    };
    let arc_mat = materials.add(additive.clone());
    let arcs = (0..SEATS)
        .map(|_| {
            let mesh = meshes.add(ribbon());
            let entity = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(arc_mat.clone()),
                    Transform::default(),
                    Visibility::Hidden,
                    NoFrustumCulling,
                    TrailView,
                ))
                .id();
            Arc { entity, mesh, points: Vec::with_capacity(POINTS), shown: false }
        })
        .collect();
    let tex = crate::figure::disc(&mut images);
    let quad = meshes.add(Rectangle::new(1.0, 1.0));
    let glows = (0..SEATS)
        .map(|_| {
            let mat =
                materials.add(StandardMaterial { base_color_texture: Some(tex.clone()), ..additive.clone() });
            let entity = commands
                .spawn((
                    Mesh3d(quad.clone()),
                    MeshMaterial3d(mat.clone()),
                    Transform::default(),
                    Visibility::Hidden,
                    TrailView,
                ))
                .id();
            Glow { entity, mat, color: [0.0; 3] }
        })
        .collect();
    commands.insert_resource(Trails { arcs, glows, drawn: 0 });
}

#[allow(clippy::too_many_arguments)]
fn draw(
    time: Res<Time>,
    mut arms: ResMut<Arms>,
    mut trails: ResMut<Trails>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shown: Query<(&mut Visibility, &mut Transform)>,
    mut dev: ResMut<crate::dev_hooks::DevStatus>,
) {
    let now = time.elapsed_secs_f64();
    let trails = &mut *trails;
    for (i, arc) in trails.arcs.iter_mut().enumerate() {
        if let Some((inner, outer)) = arms.swing[i] {
            if arc.points.is_empty() {
                trails.drawn += 1;
            }
            if arc.points.len() == POINTS {
                arc.points.remove(0);
            }
            arc.points.push((inner, outer, now));
        }
        arc.points.retain(|p| now - p.2 < LIFE);
        let show = arc.points.len() >= 2;
        if let Ok((mut v, _)) = shown.get_mut(arc.entity) {
            v.set_if_neq(if show { Visibility::Visible } else { Visibility::Hidden });
        }
        if !show {
            if arc.shown {
                arc.points.clear();
            }
            arc.shown = false;
            continue;
        }
        arc.shown = true;
        let Some(mut mesh) = meshes.get_mut(&arc.mesh) else { continue };
        let [r, g, b] = arms.color[i];
        let last = arc.points.len() - 1;
        let point = |k: usize| arc.points[k.min(last)];
        for (attr, values) in mesh.attributes_mut() {
            match values {
                VertexAttributeValues::Float32x3(v) if attr.id == Mesh::ATTRIBUTE_POSITION.id => {
                    for (j, x) in v.iter_mut().enumerate() {
                        let (a, o, _) = point(j / 2);
                        *x = if j % 2 == 0 { a } else { o }.to_array();
                    }
                }
                VertexAttributeValues::Float32x4(v) if attr.id == Mesh::ATTRIBUTE_COLOR.id => {
                    for (j, x) in v.iter_mut().enumerate() {
                        let k = j / 2;
                        // Past the points kept: nothing (they're folded onto the last).
                        let fade = if k > last {
                            0.0
                        } else {
                            (1.0 - (now - point(k).2) / LIFE).clamp(0.0, 1.0) as f32
                        };
                        // Its newest end brightest; its oldest gone.
                        let along = fade * fade * if j % 2 == 0 { INNER } else { TIP };
                        *x = [r * along, g * along, b * along, along.min(1.0)];
                    }
                }
                _ => {}
            }
        }
    }
    let face = facing_camera();
    for (i, glow) in trails.glows.iter_mut().enumerate() {
        let Ok((mut v, mut tf)) = shown.get_mut(glow.entity) else { continue };
        let Some((at, charge)) = arms.glow[i] else {
            v.set_if_neq(Visibility::Hidden);
            continue;
        };
        v.set_if_neq(Visibility::Visible);
        let size = GLOW_FROM + (GLOW_TO - GLOW_FROM) * charge * charge;
        tf.set_if_neq(Transform::from_translation(at).with_rotation(face).with_scale(Vec3::splat(size)));
        // Brighter as it gathers, in a few steps (a material changed is a material prepared
        // again).
        let k = GLOW * (0.4 + 0.6 * (charge * 4.0).round() / 4.0);
        let c = arms.color[i].map(|v| v * k);
        if c != glow.color
            && let Some(mut m) = materials.get_mut(&glow.mat)
        {
            glow.color = c;
            m.base_color = LinearRgba::rgb(c[0], c[1], c[2]).into();
        }
    }
    dev.set("swing_arcs", trails.drawn as f64);
    dev.set("hands_glowing", arms.glow.iter().flatten().count() as u32);
    *arms = Arms::default();
}
