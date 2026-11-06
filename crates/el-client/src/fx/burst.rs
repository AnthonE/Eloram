//! Bursts (`shaders/fx_burst.wgsl`): a pool of meshes, each a burst's particles with their whole
//! lives in their vertices, written once when the burst starts and moved by the shader after
//! that. A cue becomes one or two bursts (what glows, added to the picture; what's smoke or dust,
//! laid over it) from a recipe for its part, in its family's style: ember sparks fall, frost
//! shatters into shards, tide splashes, wind streaks flat, dusk drifts up in motes, growth scatters
//! leaves, stone throws rubble and dust, bronze rings, steel sparks white.
//!
//! How big and bright a burst is follows the cue's rating for this viewer (Riot: attention as
//! importance); `?quality=low` halves the particles.
//!
//! A slot is drawn only while its burst lasts: each is its own mesh, so a pool drawn whole was a
//! draw call a slot every frame (52 of a frame's 200 on the low tier, 96 of 300 on the high,
//! `scripts/e2e.sh draws`), nearly all of them of particles long dead.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use el_client_core::fx::director::{Cue, Status, What};
use el_client_core::fx::look::{Family, Palette, Reach, Rgb};
use el_client_core::fx::sound::Part;
use el_client_core::fx::stage::Knobs;
use el_sim::units::Kind;

use super::{Catch, Cues, Want};
use crate::coords::{S, world};
use crate::ground::FogTex;
use crate::look::Look;
use crate::view::Drawn;

/// Particles a burst holds at most (what's past it is dropped, so what matters goes first).
const QUADS: usize = 48;

/// A burst's material: added (what glows) or laid over (smoke, dust, rubble).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct BurstMat {
    #[texture(0)]
    #[sampler(1)]
    pub fog: Handle<Image>,
    /// x: 1 for the added pool (light: born white-hot), 0 for the laid-over one.
    #[uniform(2)]
    pub pool: Vec4,
    /// The stage's knobs on the shader's numbers (`el_client_core::fx::stage::KNOBS`; all 1 in a
    /// match): x how white-hot light is born, y how brightly the night lights smoke, z a puff's
    /// alpha, w a flash's and a star's brightness.
    #[uniform(3)]
    pub knobs: Vec4,
    /// The flipbook sheet ([`sheet`], `web/assets/fx/`): light through the smoke from six sides,
    /// its coverage and its heat, a strip of frames a cell.
    #[texture(4)]
    #[sampler(5)]
    pub sheet_a: Handle<Image>,
    #[texture(6)]
    #[sampler(7)]
    pub sheet_b: Handle<Image>,
    pub additive: bool,
}

/// The flipbook sheet the shader plays (`el_client_core::fx::stage::sheet` is its cut, checked
/// against the baker's and the shader's).
pub mod sheet {
    pub const FILES: [&str; 2] = ["fx/cloud_a.png", "fx/cloud_b.png"];
}

/// The knobs the recipes are made with (`el_client_core::fx::stage::Knobs`): the stage's, or none.
#[derive(Resource, Default)]
pub struct KnobsRes(pub Knobs);

impl KnobsRes {
    /// The shader's four, as `BurstMat::knobs` takes them.
    fn shader(&self) -> Vec4 {
        Vec4::new(self.0.k("hot"), self.0.k("night"), self.0.k("puff"), self.0.k("flash"))
    }
}

impl Material for BurstMat {
    fn vertex_shader() -> ShaderRef {
        "embedded://el_client/shaders/fx_burst.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://el_client/shaders/fx_burst.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        if self.additive { AlphaMode::Add } else { AlphaMode::Premultiplied }
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &bevy::mesh::MeshVertexBufferLayoutRef,
        _: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Seen from both sides (a ring on the ground, a streak turned any way).
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// One particle, as the shader takes it.
#[derive(Clone, Copy, Debug, Default)]
struct P {
    at: Vec3,
    vel: Vec3,
    born: f32,
    life: f32,
    size: f32,
    gravity: f32,
    drag: f32,
    shape: f32,
    color: [f32; 4],
}

/// The shapes `fx_burst.wgsl` draws.
mod shape {
    pub const GLOW: f32 = 0.0;
    pub const STREAK: f32 = 1.0;
    pub const SHARD: f32 = 2.0;
    pub const PUFF: f32 = 3.0;
    pub const RING: f32 = 4.0;
    pub const COLUMN: f32 = 5.0;
    pub const FLASH: f32 = 6.0;
    pub const STAR: f32 = 7.0;
    pub const EMBER: f32 = 8.0;
    pub const SCORCH: f32 = 9.0;
    /// A fireball (the sheet): fire burning to soot in the laid-over pool, its heat alone (an
    /// energy burst) in the added one.
    pub const BOOM: f32 = 10.0;
    /// A flash of petals (the sheet), a hit's.
    pub const FLASH_PETALS: f32 = 11.0;
}

struct Slot {
    mesh: Handle<Mesh>,
    entity: Entity,
    /// When its burst is over (the shader's clock).
    ends: f32,
}

/// How far ahead (s) a burst's end may be and the burst still live: far longer than any lasts, so
/// a slot whose end is further off was written before the clock wrapped (an hour) and is over.
const LONGEST: f32 = 600.0;

#[derive(Resource)]
pub struct Bursts {
    glow: Vec<Slot>,
    smoke: Vec<Slot>,
    /// The two pools' materials, for the knobs.
    mats: [Handle<BurstMat>; 2],
    /// The flipbook sheet's two images.
    sheets: [Handle<Image>; 2],
    rng: u32,
    low: bool,
    /// The shader's clock as `spawn` last saw it.
    now_seen: f32,
}

fn empty_mesh() -> Mesh {
    let n = QUADS * 4;
    let mut idx = Vec::with_capacity(QUADS * 6);
    for q in 0..QUADS as u32 {
        let k = q * 4;
        idx.extend([k, k + 1, k + 2, k, k + 2, k + 3]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32; 3]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.0f32; 2]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[0.0f32; 4]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; n])
        .with_inserted_indices(Indices::U32(idx))
}

pub fn setup(
    mut commands: Commands,
    look: Res<Look>,
    fog: Res<FogTex>,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<BurstMat>>,
) {
    let (glows, smokes) = if look.high { (72, 24) } else { (40, 12) };
    // The sheet holds light and coverage, not colour: linear (`mips.rs` gives it its chain).
    let linear = |s: &mut bevy::image::ImageLoaderSettings| {
        s.is_srgb = false;
        s.sampler = crate::mips::sampler(false);
    };
    let sheet_a: Handle<Image> = server.load_builder().with_settings(linear).load(sheet::FILES[0]);
    let sheet_b: Handle<Image> = server.load_builder().with_settings(linear).load(sheet::FILES[1]);
    let mat = |pool: Vec4, additive: bool| BurstMat {
        fog: fog.0.clone(),
        pool,
        knobs: Vec4::ONE,
        sheet_a: sheet_a.clone(),
        sheet_b: sheet_b.clone(),
        additive,
    };
    let glow_mat = mats.add(mat(Vec4::X, true));
    let smoke_mat = mats.add(mat(Vec4::ZERO, false));
    let mut pool = |n: usize, mat: &Handle<BurstMat>| {
        (0..n)
            .map(|_| {
                let mesh = meshes.add(empty_mesh());
                let entity = commands
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(mat.clone()),
                        Transform::default(),
                        // Drawn only while it holds a burst (`spawn`).
                        Visibility::Hidden,
                        NoFrustumCulling,
                        bevy::light::NotShadowCaster,
                    ))
                    .id();
                Slot { mesh, entity, ends: 0.0 }
            })
            .collect::<Vec<_>>()
    };
    let glow = pool(glows, &glow_mat);
    let smoke = pool(smokes, &smoke_mat);
    commands.insert_resource(Bursts {
        glow,
        smoke,
        mats: [glow_mat, smoke_mat],
        sheets: [sheet_a, sheet_b],
        rng: 0x2545_F491,
        low: !look.high,
        now_seen: 0.0,
    });
}

impl Bursts {
    /// The generator started again from `seed` (the stage: a strip shot again is the same strip).
    pub fn reseed(&mut self, seed: u32) {
        self.rng = seed.wrapping_mul(0x9E37_79B9) ^ 0x2545_F491 | 1;
    }

    /// Every slot's burst over (the stage going round to its plate).
    pub fn clear(&mut self) {
        for s in self.glow.iter_mut().chain(&mut self.smoke) {
            s.ends = 0.0;
        }
    }

    /// The flipbook sheet is in (nothing of a burst that plays it is drawn before).
    pub fn sheets_loaded(&self, server: &AssetServer) -> bool {
        self.sheets.iter().all(|h| server.is_loaded_with_dependencies(h))
    }

    /// How many slots hold a live burst: of light, and of smoke.
    pub fn live(&self) -> (u32, u32) {
        let n = |v: &[Slot]| v.iter().filter(|s| s.ends > self.now_seen).count() as u32;
        (n(&self.glow), n(&self.smoke))
    }

    fn rand(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    /// A direction on the unit sphere's upper part (`up`: 0 flat round, 1 straight up).
    fn dir(&mut self, up: f32) -> Vec3 {
        let a = self.rand() * std::f32::consts::TAU;
        let y = up * (0.3 + 0.7 * self.rand());
        let r = (1.0 - y * y).max(0.0).sqrt();
        Vec3::new(a.cos() * r, y, a.sin() * r)
    }

    /// Writes `ps` into the least busy slot of its pool.
    fn put(&mut self, ps: &[P], smoke: bool, now: f32, meshes: &mut Assets<Mesh>) {
        if ps.is_empty() {
            return;
        }
        let pool = if smoke { &mut self.smoke } else { &mut self.glow };
        let Some(slot) = pool.iter_mut().min_by(|a, b| a.ends.total_cmp(&b.ends)) else { return };
        let Some(mut mesh) = meshes.get_mut(&slot.mesh) else { return };
        let n = QUADS * 4;
        let (mut pos, mut vel, mut uv, mut life, mut tan, mut col) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );
        let mut ends = now;
        for k in 0..QUADS {
            let p = ps.get(k).copied().unwrap_or_default();
            ends = ends.max(p.born + p.life);
            for c in [[0.0f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] {
                pos.push(p.at.to_array());
                vel.push(p.vel.to_array());
                uv.push(c);
                life.push([p.born, p.life]);
                tan.push([p.size, p.gravity, p.drag, p.shape]);
                col.push(p.color);
            }
        }
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vel);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, life);
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tan);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
        slot.ends = ends;
    }
}

fn rgba(c: Rgb, k: f32, a: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, a]
}

/// A burst being made: what glows and what's smoke.
struct Make<'a> {
    b: &'a mut Bursts,
    now: f32,
    glow: Vec<P>,
    smoke: Vec<P>,
    /// The share of particles kept (the low tier keeps half).
    keep: f32,
    /// The stage's knobs (none in a match: every one 1).
    knobs: &'a Knobs,
}

impl Make<'_> {
    /// A knob's multiplier (`stage::KNOBS`).
    fn k(&self, name: &str) -> f32 {
        self.knobs.k(name)
    }

    fn n(&self, n: f32) -> usize {
        (n * self.keep * self.k("count")).round().max(1.0) as usize
    }

    /// A particle as the knobs have it: its size, life and speed scaled, its light brightened or
    /// its smoke thinned.
    fn knobbed(&self, mut p: P, smoke: bool) -> P {
        p.size *= self.k("size");
        p.life *= self.k("life");
        p.vel *= self.k("speed");
        if smoke {
            p.color[3] *= self.k("smoke");
        } else {
            let b = self.k("bright");
            p.color[0] *= b;
            p.color[1] *= b;
            p.color[2] *= b;
        }
        p
    }

    #[allow(clippy::too_many_arguments)]
    fn spray(
        &mut self,
        at: Vec3,
        n: f32,
        shape: f32,
        speed: f32,
        up: f32,
        life: f32,
        size: f32,
        gravity: f32,
        drag: f32,
        color: [f32; 4],
        smoke: bool,
    ) {
        for _ in 0..self.n(n) {
            let d = self.b.dir(up);
            let s = speed * (0.5 + 0.7 * self.b.rand());
            let p = P {
                at: at + d * 0.05,
                vel: d * s,
                born: self.now + self.b.rand() * 0.04,
                life: life * (0.7 + 0.5 * self.b.rand()),
                size: size * (0.7 + 0.6 * self.b.rand()),
                gravity,
                drag,
                shape,
                color,
            };
            self.push(p, smoke);
        }
    }

    fn one(&mut self, at: Vec3, shape: f32, size: f32, life: f32, color: [f32; 4], smoke: bool) {
        let p = P { at, born: self.now, life, size, shape, color, ..P::default() };
        self.push(p, smoke);
    }

    fn push(&mut self, p: P, smoke: bool) {
        let p = self.knobbed(p, smoke);
        if smoke { self.smoke.push(p) } else { self.glow.push(p) }
    }

    /// A family's sparks: what flies off a hit of its kind.
    fn sparks(&mut self, fam: Option<Family>, pal: &Palette, at: Vec3, k: f32) {
        let core = rgba(pal.core, 1.0, 1.0);
        let edge = rgba(pal.edge, 1.0, 1.0);
        let dust = rgba(pal.smoke, 1.0, 0.7);
        match fam {
            Some(Family::Ember) => {
                self.spray(at, 14.0 * k, shape::STREAK, 6.0, 0.7, 0.45, 0.05, 7.0, 2.0, core, false);
                self.spray(at, 3.0 * k, shape::PUFF, 0.8, 0.9, 0.8, 0.24, -0.6, 2.0, dust, true);
                // Embers that linger, drifting up off the hit as it cools.
                self.spray(at, 5.0 * k, shape::EMBER, 0.9, 0.95, 1.3, 0.06, -0.9, 1.6, edge, false);
            }
            Some(Family::Frost) => {
                self.spray(at, 12.0 * k, shape::SHARD, 4.0, 0.6, 0.55, 0.09, 6.0, 2.5, core, false);
                self.spray(at, 6.0 * k, shape::GLOW, 1.5, 0.5, 0.5, 0.12, 0.0, 3.0, edge, false);
            }
            Some(Family::Tide) => {
                self.spray(at, 16.0 * k, shape::STREAK, 5.0, 0.8, 0.5, 0.05, 9.0, 1.5, core, false);
                self.spray(at, 3.0 * k, shape::PUFF, 1.0, 0.6, 0.7, 0.22, 0.0, 2.0, dust, true);
            }
            Some(Family::Wind) => {
                self.spray(at, 12.0 * k, shape::STREAK, 7.0, 0.1, 0.35, 0.05, 0.0, 4.0, core, false);
            }
            Some(Family::Dusk) => {
                self.spray(at, 12.0 * k, shape::GLOW, 2.5, 0.8, 0.8, 0.07, -1.5, 2.0, core, false);
                self.spray(at, 5.0 * k, shape::SHARD, 3.0, 0.6, 0.5, 0.06, 0.0, 3.0, edge, false);
            }
            Some(Family::Growth) => {
                self.spray(at, 10.0 * k, shape::SHARD, 3.0, 0.7, 0.9, 0.08, 3.0, 2.0, core, false);
                self.spray(at, 6.0 * k, shape::GLOW, 1.5, 0.9, 0.8, 0.06, -1.0, 2.0, edge, false);
            }
            Some(Family::Stone) => {
                self.spray(
                    at,
                    10.0 * k,
                    shape::SHARD,
                    4.0,
                    0.7,
                    0.6,
                    0.08,
                    11.0,
                    1.0,
                    rgba(pal.smoke, 1.6, 1.0),
                    true,
                );
                // Dust off a hit, no more: the puffs outgrew and outlasted the hit itself (the
                // strip of Brakka's Q peaked at 0.6 s on its dust, not at the strike).
                self.spray(at, 3.0 * k, shape::PUFF, 1.0, 0.5, 0.7, 0.22, -0.3, 2.0, dust, true);
                self.spray(at, 5.0 * k, shape::STREAK, 4.0, 0.6, 0.3, 0.04, 6.0, 2.0, core, false);
            }
            Some(Family::Bronze) => {
                self.spray(at, 10.0 * k, shape::GLOW, 3.0, 0.5, 0.5, 0.08, 0.0, 3.0, core, false);
                self.one(at, shape::RING, 0.6 * k, 0.4, edge, false);
            }
            Some(Family::Steel) | None => {
                self.spray(at, 14.0 * k, shape::STREAK, 8.0, 0.5, 0.3, 0.04, 8.0, 3.0, core, false);
            }
        }
    }
}

/// The bursts of this frame's cues; each slot drawn while its burst lasts, and only then.
///
/// A catch armed for a cue's part holds the picture on the first such cue, its bursts born its
/// age ago; while the picture is held nothing new is born in it.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    time: Res<Time>,
    cues: Res<Cues>,
    drawn: Res<Drawn>,
    knobs: Res<KnobsRes>,
    mut catch: ResMut<Catch>,
    mut clock: ResMut<Time<Virtual>>,
    mut bursts: ResMut<Bursts>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<BurstMat>>,
    mut shown: Query<&mut Visibility>,
) {
    let now = time.elapsed_secs_wrapped();
    bursts.now_seen = now;
    if knobs.is_changed() {
        for h in bursts.mats.clone() {
            if let Some(mut m) = mats.get_mut(&h) {
                m.knobs = knobs.shader();
            }
        }
    }
    let keep = if bursts.low { 0.5 } else { 1.0 };
    for (i, q) in cues.list.iter().enumerate() {
        if catch.held {
            break;
        }
        let born = match catch.want {
            Some(w @ Want::Cue { age, .. }) if w.takes(q) => {
                catch.hold(
                    &mut clock,
                    format!("{:?} {:?} on {:?} at {:.0},{:.0}", q.what, q.source, q.unit, q.at.x, q.at.y),
                );
                catch.cue = Some((i, age));
                now - age
            }
            _ => now,
        };
        let mut m =
            Make { b: &mut bursts, now: born, glow: Vec::new(), smoke: Vec::new(), keep, knobs: &knobs.0 };
        recipe(&mut m, q, &drawn);
        let (glow, smoke) = (std::mem::take(&mut m.glow), std::mem::take(&mut m.smoke));
        bursts.put(&glow, false, born, &mut meshes);
        bursts.put(&smoke, true, born, &mut meshes);
    }
    for slot in bursts.glow.iter().chain(&bursts.smoke) {
        let live = slot.ends > now && slot.ends - now < LONGEST;
        if let Ok(mut v) = shown.get_mut(slot.entity) {
            v.set_if_neq(if live { Visibility::Inherited } else { Visibility::Hidden });
        }
    }
}

/// Where a cue on a unit is drawn: at its chest, on its body's near side. In its middle, a hit
/// was drawn inside the model, and its depth hid all of it but what flew out (a picture held on a
/// skillshot's hit showed nothing of it at all).
fn on_body(q: &Cue, drawn: &Drawn) -> Vec3 {
    let unit = q.unit.and_then(|u| drawn.units.get(usize::from(u)).copied().flatten());
    let r = unit.map(|d| d.radius * S);
    let kind = match (q.unit, r) {
        (_, Some(r)) if r > 1.2 => Kind::Turret,
        (Some(u), _) if usize::from(u) < el_sim::units::SEATS => Kind::Champion,
        _ => Kind::Melee,
    };
    let out = r.map_or(body(kind), |r| (r * 0.8).clamp(0.3, 1.8));
    world(q.at, chest(kind)) + toward_camera() * out
}

/// From the floor toward the camera, a metre.
fn toward_camera() -> Vec3 {
    crate::camera::VIEW.offset(1.0).normalize()
}

/// How far a unit's body stands out from its middle toward the camera (m), by its kind, where
/// its radius isn't drawn.
fn body(kind: Kind) -> f32 {
    match kind {
        Kind::Champion => 0.55,
        k if k.is_minion() => 0.4,
        k if k.is_structure() => 1.6,
        _ => 0.5,
    }
}

/// How high off the floor a unit is hit (m), by its kind.
fn chest(kind: Kind) -> f32 {
    match kind {
        Kind::Champion => 1.0,
        k if k.is_minion() => 0.5,
        k if k.is_structure() => 2.0,
        _ => 1.0,
    }
}

fn recipe(m: &mut Make<'_>, q: &Cue, drawn: &Drawn) {
    // Bigger and brighter as it matters more to this viewer: 0.7 for a 1 to 1.4 for a 10.
    let k = 0.62 + 0.08 * f32::from(q.rating.clamp(1, 10));
    let pal = q.look.palette;
    let fam = q.look.family;
    let core = rgba(pal.core, 1.0, 1.0);
    let edge = rgba(pal.edge, 1.0, 1.0);
    let dust = rgba(pal.smoke, 1.0, 0.7);
    let ground = |h: f32| world(q.at, h);
    match q.what {
        What::Part(Part::Cast) => {
            let glow = m.k("cast.glow");
            m.spray(ground(0.2), 8.0 * k * glow, shape::GLOW, 1.2, 0.95, 0.55, 0.07, -2.0, 1.5, core, false);
            // A quick flash at the feet; the motes carry the cast (a wide one read as pink smoke).
            m.one(ground(0.04), shape::FLASH, 0.42 * k * m.k("cast.flash"), 0.25, edge, false);
        }
        What::Part(Part::Ult) => {
            m.one(ground(0.0), shape::COLUMN, 0.45 * k, 0.9, core, false);
            m.one(ground(0.05), shape::RING, 2.4 * k, 0.7, edge, false);
            m.spray(ground(0.3), 16.0 * k, shape::GLOW, 2.0, 0.9, 0.9, 0.08, -1.5, 1.5, core, false);
        }
        What::Part(Part::Launch) => {
            // The muzzle on the caster's near side, as a hit is (a turret's is its own height).
            let at = if matches!(q.look.reach, Reach::Turret) { ground(1.1) } else { on_body(q, drawn) };
            m.one(at, shape::GLOW, 0.25 * k, 0.16, core, false);
            if matches!(q.look.reach, Reach::Line { .. } | Reach::Homing | Reach::Turret) {
                // A muzzle's flash, and what it throws forward.
                m.one(at, shape::STAR, 0.3 * k, 0.1, core, false);
                m.spray(at, 4.0 * k, shape::GLOW, 1.5, 0.4, 0.3, 0.05, 0.0, 3.0, edge, false);
            }
        }
        What::Part(Part::Impact) => {
            let at = on_body(q, drawn);
            // A hit's size by its damage, a little.
            let hurt = (q.size / 150.0).clamp(0.6, 1.6);
            let auto = matches!(q.look.reach, Reach::Auto { .. });
            let kk = if auto { k * 0.5 * hurt } else { k * hurt };
            // Where it struck: a hot glow and a flash of petals, there and gone, the sharpest thing
            // in the hit (first: a burst past its particles' room drops the last).
            m.one(at, shape::GLOW, 0.32 * kk.max(0.6) * m.k("impact.glow"), 0.14, core, false);
            m.one(at, shape::FLASH_PETALS, 0.65 * kk.max(0.5) * m.k("impact.star"), 0.16, core, false);
            m.sparks(fam, &pal, at, kk * m.k("impact.sparks"));
            if q.look.ult || q.rating >= 8 {
                m.one(ground(0.05), shape::RING, 0.9 * k, 0.45, edge, false);
            }
        }
        What::Part(Part::Blast) => {
            let r = (q.size * S).max(0.3);
            m.one(ground(0.05), shape::RING, r * m.k("blast.ring"), 0.55, edge, false);
            m.one(ground(0.04), shape::FLASH, r * 0.9 * m.k("blast.flash"), 0.35, core, false);
            // What boils up from its middle: a fireball burning to soot where fire strikes, the
            // heat alone (an energy burst) for the rest; stone throws dust and rubble, not fire.
            if !matches!(fam, Some(Family::Stone)) {
                let fire = matches!(fam, Some(Family::Ember));
                let size = (r * 0.45).clamp(0.3, 1.4) * m.k("blast.flash");
                m.one(ground(0.3), shape::BOOM, size, 0.55, core, fire);
            }
            let n = (r * 6.0).clamp(8.0, 30.0) * k;
            for _ in 0..m.n(n) {
                let a = m.b.rand() * std::f32::consts::TAU;
                let d = m.b.rand().sqrt() * r * 0.9;
                let at = ground(0.1) + Vec3::new(a.cos() * d, 0.0, a.sin() * d);
                let p = P {
                    at,
                    vel: Vec3::new(a.cos() * 0.6, 2.0 + 2.5 * m.b.rand(), a.sin() * 0.6),
                    born: m.now + m.b.rand() * 0.08,
                    life: 0.6 + 0.4 * m.b.rand(),
                    size: 0.08,
                    gravity: if matches!(fam, Some(Family::Dusk | Family::Growth | Family::Wind)) {
                        -1.0
                    } else {
                        5.0
                    },
                    drag: 2.0,
                    shape: if matches!(fam, Some(Family::Frost | Family::Stone | Family::Growth)) {
                        shape::SHARD
                    } else {
                        shape::GLOW
                    },
                    color: core,
                };
                m.push(p, false);
            }
            if matches!(fam, Some(Family::Stone | Family::Ember | Family::Tide | Family::Wind)) {
                let puff = m.k("blast.puff");
                for _ in 0..m.n((r * 2.5).clamp(4.0, 10.0) * puff) {
                    let a = m.b.rand() * std::f32::consts::TAU;
                    let at = ground(0.2) + Vec3::new(a.cos() * r * 0.7, 0.0, a.sin() * r * 0.7);
                    let p = P {
                        at,
                        vel: Vec3::new(a.cos() * 0.8, 0.4, a.sin() * 0.8),
                        born: m.now,
                        life: 1.2,
                        size: 0.42 * r.min(1.6) * puff,
                        drag: 1.5,
                        shape: shape::PUFF,
                        color: dust,
                        ..P::default()
                    };
                    m.push(p, true);
                }
            }
            // What burns or breaks the ground leaves a mark on it for a while: soot where fire
            // or stone struck; an ultimate of any other family leaves its light on the ground
            // instead, fading slowly (a soot scorch under a healer's sanctuary read as a hole).
            if matches!(fam, Some(Family::Ember | Family::Stone)) {
                m.one(ground(0.03), shape::SCORCH, r * 0.5 * m.k("blast.scorch"), 3.0, SCORCH, true);
            } else if q.look.ult {
                m.one(
                    ground(0.03),
                    shape::FLASH,
                    r * 0.6 * m.k("blast.scorch"),
                    2.0,
                    rgba(pal.edge, 0.5, 0.6),
                    false,
                );
            }
            if matches!(fam, Some(Family::Ember)) {
                m.spray(
                    ground(0.3),
                    (r * 3.0).clamp(4.0, 10.0) * k,
                    shape::EMBER,
                    1.2,
                    0.9,
                    1.6,
                    0.06,
                    -0.8,
                    1.4,
                    edge,
                    false,
                );
            }
            if q.look.ult {
                m.one(ground(0.0), shape::COLUMN, r * 0.35, 0.8, core, false);
                m.one(ground(0.6), shape::STAR, r * 0.5, 0.18, core, false);
            }
        }
        What::Part(Part::Windup) => {
            // Motes drawn in toward where it will land.
            let r = (q.size * S).max(0.3);
            for _ in 0..m.n(14.0 * k) {
                let a = m.b.rand() * std::f32::consts::TAU;
                let from = Vec3::new(a.cos() * r, 0.15 + 0.4 * m.b.rand(), a.sin() * r);
                let p = P {
                    at: ground(0.0) + from,
                    vel: -from * 2.2,
                    born: m.now + m.b.rand() * 0.3,
                    life: 0.8,
                    size: 0.07,
                    drag: 2.0,
                    shape: shape::GLOW,
                    color: edge,
                    ..P::default()
                };
                m.push(p, false);
            }
        }
        What::Part(Part::Dash) => {
            let from = q.from.unwrap_or(q.at);
            let steps = m.n(12.0 * k);
            for i in 0..steps {
                let t = i as f32 / steps.max(1) as f32;
                let at = world(from.lerp(q.at, t), 0.4 + 0.3 * m.b.rand());
                let smoke = matches!(fam, Some(Family::Stone | Family::Tide));
                let p = P {
                    at,
                    vel: Vec3::Y * 0.4,
                    born: m.now + t * 0.12,
                    life: 0.5,
                    size: if smoke { 0.3 } else { 0.12 },
                    drag: 1.0,
                    shape: if smoke { shape::PUFF } else { shape::GLOW },
                    color: if smoke { dust } else { core },
                    ..P::default()
                };
                m.push(p, smoke);
            }
            m.one(ground(0.05), shape::FLASH, 0.7 * k * m.k("dash.flash"), 0.3, edge, false);
            if q.look.family.is_none() {
                // Blink: there and here at once.
                m.one(world(from, 0.05), shape::FLASH, 0.7, 0.3, edge, false);
                m.spray(world(from, 0.9), 10.0, shape::GLOW, 2.0, 0.5, 0.4, 0.06, 0.0, 3.0, core, false);
                m.spray(ground(0.9), 10.0, shape::GLOW, 2.0, 0.5, 0.4, 0.06, 0.0, 3.0, core, false);
            }
        }
        What::Part(Part::Buff) | What::Shield | What::Heal => {
            let glow = m.k("buff.glow");
            for i in 0..m.n(12.0 * k * glow) {
                let a = i as f32 * 0.9 + m.b.rand();
                let at = ground(0.1) + Vec3::new(a.cos() * 0.45, 0.0, a.sin() * 0.45);
                let p = P {
                    at,
                    vel: Vec3::new(-a.sin() * 0.6, 1.6 + m.b.rand(), a.cos() * 0.6),
                    born: m.now + m.b.rand() * 0.15,
                    life: 0.8,
                    size: 0.07 * glow,
                    drag: 1.0,
                    shape: shape::GLOW,
                    color: core,
                    ..P::default()
                };
                m.push(p, false);
            }
            m.one(ground(0.05), shape::RING, 0.85 * m.k("buff.ring"), 0.5, edge, false);
            // A soft bloom at the chest as it lands, so the buff shows in its first tenth of a
            // second (the ring and the motes take a moment to rise).
            m.one(ground(1.0), shape::GLOW, 0.5 * k * m.k("buff.glow"), 0.3, rgba(pal.core, 0.6, 1.0), false);
        }
        What::Fizzle => {
            m.spray(ground(1.1), 6.0, shape::GLOW, 1.0, 0.5, 0.35, 0.06, 0.0, 3.0, edge, false);
        }
        What::Status(s) => {
            let c = rgba(s.palette().core, 1.0, 1.0);
            let e = rgba(s.palette().edge, 1.0, 1.0);
            match s {
                Status::Stun => {
                    m.spray(ground(2.2), 10.0, shape::GLOW, 1.6, 0.3, 0.5, 0.07, 0.0, 3.0, c, false)
                }
                Status::Root => {
                    m.one(ground(0.05), shape::RING, 0.8, 0.5, e, false);
                    m.spray(ground(0.2), 8.0, shape::SHARD, 1.2, 0.9, 0.6, 0.07, 2.0, 2.0, c, false);
                }
                Status::Airborne => {
                    m.spray(
                        ground(0.1),
                        6.0,
                        shape::PUFF,
                        1.5,
                        0.2,
                        0.8,
                        0.35,
                        0.0,
                        2.0,
                        rgba(world_dust(), 1.0, 0.6),
                        true,
                    );
                    m.spray(ground(0.2), 8.0, shape::STREAK, 5.0, 1.0, 0.4, 0.04, 0.0, 2.0, c, false);
                }
                Status::Shove => {
                    m.spray(
                        ground(0.15),
                        6.0,
                        shape::PUFF,
                        1.2,
                        0.2,
                        0.7,
                        0.3,
                        0.0,
                        2.0,
                        rgba(world_dust(), 1.0, 0.6),
                        true,
                    );
                }
                Status::Slow => {
                    m.spray(ground(0.15), 8.0, shape::GLOW, 1.0, 0.05, 0.6, 0.07, 0.0, 2.0, c, false)
                }
                Status::Silence | Status::Reveal => m.one(ground(2.3), shape::GLOW, 0.35, 0.3, c, false),
                Status::Immune => {
                    m.one(ground(0.0), shape::COLUMN, 0.4, 0.6, c, false);
                    m.one(ground(0.05), shape::RING, 1.0, 0.5, e, false);
                }
                Status::Empowered | Status::Hasted => m.one(ground(0.05), shape::RING, 0.7, 0.4, e, false),
            }
        }
        What::Death(kind) => match kind {
            Kind::Champion => {
                m.spray(
                    ground(0.6),
                    8.0,
                    shape::PUFF,
                    1.0,
                    0.6,
                    1.3,
                    0.45,
                    -0.4,
                    1.5,
                    [0.05, 0.05, 0.07, 0.75],
                    true,
                );
                m.one(ground(0.05), shape::RING, 1.4, 0.7, edge, false);
                m.spray(ground(0.8), 14.0, shape::GLOW, 2.0, 0.7, 0.9, 0.07, -1.0, 2.0, core, false);
            }
            Kind::Turret | Kind::Inhibitor | Kind::Nexus | Kind::Fountain => {
                let big = if kind == Kind::Nexus { 2.0 } else { 1.0 };
                m.spray(
                    ground(1.0),
                    14.0 * big,
                    shape::PUFF,
                    2.5,
                    0.5,
                    2.2,
                    0.9 * big,
                    -0.3,
                    1.2,
                    rgba(world_dust(), 1.0, 0.8),
                    true,
                );
                m.spray(
                    ground(2.0),
                    18.0 * big,
                    shape::SHARD,
                    6.0,
                    0.8,
                    1.2,
                    0.15,
                    9.0,
                    1.0,
                    rgba(world_dust(), 1.3, 1.0),
                    true,
                );
                m.spray(ground(2.0), 16.0 * big, shape::SHARD, 5.0, 0.8, 1.0, 0.1, 7.0, 1.0, core, false);
                m.one(ground(0.05), shape::RING, 3.0 * big, 1.0, edge, false);
                m.one(ground(0.03), shape::SCORCH, 1.8 * big, 6.0, SCORCH, true);
                m.spray(ground(1.5), 10.0 * big, shape::EMBER, 1.5, 0.9, 2.5, 0.08, -0.6, 1.2, core, false);
            }
            _ => m.spray(
                ground(0.3),
                3.0,
                shape::PUFF,
                0.6,
                0.6,
                0.6,
                0.18,
                -0.3,
                2.0,
                rgba(world_dust(), 1.0, 0.6),
                true,
            ),
        },
        What::Respawn | What::LevelUp => {
            m.one(ground(0.0), shape::COLUMN, 0.5, 0.9, core, false);
            m.one(ground(0.05), shape::RING, 1.2, 0.6, edge, false);
            m.spray(ground(0.2), 14.0, shape::GLOW, 1.0, 0.95, 0.9, 0.07, -2.5, 1.0, core, false);
        }
        What::Relic { beam } => {
            if beam {
                m.one(ground(0.0), shape::COLUMN, 0.6, 1.2, core, false);
            } else {
                m.spray(ground(0.6), 16.0, shape::GLOW, 2.0, 0.8, 0.8, 0.07, -1.0, 2.0, core, false);
                m.one(ground(0.05), shape::RING, 1.0, 0.5, edge, false);
            }
        }
        // Heard, not drawn (a ping's ring is `view.rs`'s).
        What::Ping(_) | What::TurretWarn | What::Ui => {}
    }
}

/// A scorch's colour: soot, laid over the stone (its shape sets how much of the stone shows).
const SCORCH: [f32; 4] = [0.02, 0.018, 0.02, 1.0];

/// The Causeway's dust: stone and snow.
fn world_dust() -> Rgb {
    [0.62, 0.64, 0.70]
}
