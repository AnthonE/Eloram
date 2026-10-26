//! The Causeway's edges (`docs/ART.md`, step 3): along the floor's outline (traced from the map's
//! own shapes, `el_client_core::scenery::outline`), a parapet of dressed stone with merlons, and
//! below it a cliff of jagged rock falling into the chasm's haze; mist lying over the chasm. The
//! parapet stands outward of the edge, so the wall is where a champion stops (tested there).

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use el_client_core::scenery::{floor_distance, outline, outward};
use el_sim::math::hash01;

use crate::coords::{S, world};
use crate::ground::{FogTex, GroundMat, GroundParams, GroundTex};

pub struct EdgesPlugin;

impl Plugin for EdgesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, edges);
    }
}

/// The parapet's height and how far it stands out (game units), the merlons' size and spacing.
const WALL_H: f32 = 70.0;
const WALL_T: f32 = 60.0;
const MERLON_W: f32 = 70.0;
const MERLON_H: f32 = 38.0;
const MERLON_EVERY: usize = 6;
/// The outline's spacing (game units).
const STEP: f32 = 50.0;
/// How deep the cliff goes (scene metres), and its rings' depths.
const DEPTHS: [f32; 7] = [0.0, -0.6, -1.8, -3.4, -5.4, -8.0, -12.0];

#[allow(clippy::too_many_arguments)]
fn edges(
    mut commands: Commands,
    look: Res<crate::look::Look>,
    tex: Res<GroundTex>,
    fog: Res<FogTex>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<GroundMat>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    if !look.art {
        return;
    }
    let edge = |mode: f32, tile: f32| GroundParams { mode: Vec4::new(mode, tile, 0.0, -14.0), ..default() };
    let rock = mats.add(tex.material(&fog, edge(1.0, 3.0)));
    let slabs = mats.add(tex.material(&fog, edge(2.0, 1.6)));
    for l in outline(STEP) {
        let rim = rim(&l);
        commands.spawn((Mesh3d(meshes.add(parapet(&l, &rim))), MeshMaterial3d(slabs.clone())));
        commands.spawn((
            Mesh3d(meshes.add(cliff(&l, &rim))),
            MeshMaterial3d(rock.clone()),
            bevy::light::NotShadowCaster,
        ));
    }
    // The rock under the props that stand off the edge.
    for d in el_client_core::scenery::plinths() {
        commands.spawn((
            Mesh3d(meshes.add(plinth(d.1 * S))),
            MeshMaterial3d(rock.clone()),
            Transform::from_translation(world(d.0, 0.0)),
        ));
    }
    // Mist over the chasm: long soft sheets beside the lane, a little below the floor.
    let mist = std_mats.add(StandardMaterial {
        base_color_texture: Some(images.add(mist_image())),
        base_color: Color::srgba(0.35, 0.45, 0.65, 0.16),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        fog_enabled: false,
        ..default()
    });
    let len = el_sim::MAP_W * S * 2.5;
    for (z, y, w) in [(-14.0, -2.2, 10.0), (-20.0, -4.5, 14.0), (13.0, -2.6, 9.0), (18.0, -5.0, 12.0)] {
        commands.spawn((
            Mesh3d(meshes.add(Plane3d::default().mesh().size(len, w))),
            MeshMaterial3d(mist.clone()),
            Transform::from_xyz(0.0, y, z),
            bevy::light::NotShadowCaster,
        ));
    }
}

/// Each outline point's outward direction and the parapet's thickness there (thinner where two
/// parts of the floor nearly meet, so the wall never covers floor).
fn rim(l: &[Vec2]) -> Vec<(Vec2, f32)> {
    l.iter()
        .map(|&p| {
            let out = outward(p);
            let mut t = WALL_T;
            while t > 6.0 && floor_distance(p + out * t) < t * 0.8 {
                t *= 0.6;
            }
            (out, t)
        })
        .collect()
}

/// A mesh from rings of points round the loop: each ring a profile point per outline point,
/// stitched to the next ring into quads. Flat between rings, so each face of the profile keeps its
/// own normal.
struct Strips {
    pos: Vec<[f32; 3]>,
    idx: Vec<u32>,
}

impl Strips {
    fn new() -> Self {
        Self { pos: Vec::new(), idx: Vec::new() }
    }

    /// A band between two rings (each a point per outline point, in order round the loop).
    fn band(&mut self, a: &[Vec3], b: &[Vec3]) {
        let n = a.len();
        let base = self.pos.len() as u32;
        self.pos.extend(a.iter().map(|v| v.to_array()));
        self.pos.extend(b.iter().map(|v| v.to_array()));
        for i in 0..n as u32 {
            let j = (i + 1) % n as u32;
            let (a0, a1, b0, b1) = (base + i, base + j, base + n as u32 + i, base + n as u32 + j);
            self.idx.extend([a0, b0, a1, a1, b0, b1]);
        }
    }

    fn mesh(self) -> Mesh {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_indices(Indices::U32(self.idx));
        m.compute_smooth_normals();
        let n = m.count_vertices();
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; n]);
        m
    }
}

/// The parapet: up from the floor at the edge, across the top, and down its outer face to where the
/// cliff begins; merlons on top, a gap now and then.
fn parapet(l: &[Vec2], rim: &[(Vec2, f32)]) -> Mesh {
    let ring = |out: f32, h: f32| -> Vec<Vec3> {
        l.iter().zip(rim).map(|(&p, &(o, t))| world(p + o * out.min(t), h * S)).collect()
    };
    let mut s = Strips::new();
    // The loop runs counter-clockwise with the floor on its left: bands go inner to outer, bottom
    // to top, so their faces look out of the wall.
    let (inner_lo, inner_hi) = (ring(0.0, -2.0), ring(0.0, WALL_H));
    let (outer_hi, outer_lo) = (ring(WALL_T, WALL_H), ring(WALL_T, -40.0));
    s.band(&inner_hi, &inner_lo);
    s.band(&outer_hi, &inner_hi);
    s.band(&outer_lo, &outer_hi);
    let mut m = s.mesh();
    // The merlons: blocks on the top, every few points, not where the wall is thin.
    let block = Cuboid::new(MERLON_W * S, MERLON_H * S, WALL_T * S * 0.9).mesh().build();
    for (k, (&p, &(o, t))) in l.iter().zip(rim).enumerate() {
        if k % MERLON_EVERY != 0 || t < WALL_T * 0.9 || hash01(k as u32, 77) < 0.18 {
            continue;
        }
        let at = world(p + o * (t * 0.5), (WALL_H + MERLON_H * 0.5) * S);
        // The block's depth (its z) along the outward direction, which is (x, -y) in the scene.
        let yaw = o.x.atan2(-o.y);
        let tilt = (hash01(k as u32, 5) - 0.5) * 0.12;
        let tf = Transform::from_translation(at)
            .with_rotation(Quat::from_rotation_y(yaw) * Quat::from_rotation_z(tilt));
        let mut b = block.clone().transformed_by(tf);
        if b.contains_attribute(Mesh::ATTRIBUTE_TANGENT) {
            b.remove_attribute(Mesh::ATTRIBUTE_TANGENT);
        }
        let _ = m.merge(&b);
    }
    m
}

/// The cliff: from the parapet's foot down into the dark, stepping out and in by a little noise so
/// the rock is jagged, smooth along the loop so it doesn't saw.
fn cliff(l: &[Vec2], rim: &[(Vec2, f32)]) -> Mesh {
    let n = l.len();
    let jag = |i: usize, k: usize| -> f32 {
        // Noise along the loop, eased between every fourth point.
        let a = i / 4;
        let f = (i % 4) as f32 / 4.0;
        let f = f * f * (3.0 - 2.0 * f);
        let h = |a: usize| hash01((a % n.div_ceil(4)) as u32, k as u32 * 31 + 7);
        h(a) * (1.0 - f) + h(a + 1) * f
    };
    let rings: Vec<Vec<Vec3>> = DEPTHS
        .iter()
        .enumerate()
        .map(|(k, &d)| {
            l.iter()
                .zip(rim)
                .enumerate()
                .map(|(i, (&p, &(o, t)))| {
                    let out = t + if k == 0 { 0.0 } else { (jag(i, k) - 0.35) * 90.0 + k as f32 * 18.0 };
                    let mut v = world(p + o * out, 0.0);
                    v.y = d - if k == 0 { 0.4 } else { (jag(i, k + 9) - 0.5) * 0.8 };
                    v
                })
                .collect()
        })
        .collect();
    let mut s = Strips::new();
    for w in rings.windows(2) {
        s.band(&w[0], &w[1]);
    }
    s.mesh()
}

/// A rough column of rock from the chasm up to the floor, `r` metres across the top.
fn plinth(r: f32) -> Mesh {
    let segs = 9;
    let mut s = Strips::new();
    let ring = |y: f32, grow: f32, k: u32| -> Vec<Vec3> {
        (0..segs)
            .map(|i| {
                let a = -(i as f32) / segs as f32 * std::f32::consts::TAU;
                let rr = r * (grow + (hash01(i as u32, k) - 0.5) * 0.25);
                Vec3::new(a.cos() * rr, y, a.sin() * rr)
            })
            .collect()
    };
    let rs = [ring(0.02, 1.0, 1), ring(-1.5, 1.08, 2), ring(-5.0, 1.2, 3), ring(-12.0, 1.35, 4)];
    for w in rs.windows(2) {
        s.band(&w[0], &w[1]);
    }
    // The top.
    let top = ring(0.02, 1.0, 1);
    let base = s.pos.len() as u32;
    s.pos.push([0.0, 0.05, 0.0]);
    s.pos.extend(top.iter().map(|v| v.to_array()));
    for i in 0..segs as u32 {
        s.idx.extend([base, base + 1 + (i + 1) % segs as u32, base + 1 + i]);
    }
    s.mesh()
}

/// Mist: soft across, in drifts along.
fn mist_image() -> Image {
    let (w, h) = (256usize, 64usize);
    let mut data = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let v = (y as f32 + 0.5) / h as f32;
            let across = (v * std::f32::consts::PI).sin().powf(1.6);
            let drift = 0.55
                + 0.45
                    * (hash01((x / 16) as u32, 3) * (1.0 - (x % 16) as f32 / 16.0)
                        + hash01((x / 16 + 1) as u32 % (w as u32 / 16), 3) * ((x % 16) as f32 / 16.0));
            let a = (across * drift * 255.0) as u8;
            data[(y * w + x) * 4..][..4].copy_from_slice(&[255, 255, 255, a]);
        }
    }
    let mut img = Image::new(
        Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = crate::mips::sampler(true);
    img
}
