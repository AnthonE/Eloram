//! What stands on and round the Causeway (`docs/ART.md`, steps 4 to 6): the models (structures,
//! rock, props, the ruins and ice in the chasm), the brush's frosted thickets, the braziers' fire
//! and light, and falling snow. Where things stand is `el_client_core::scenery`'s (tested there
//! against the navigation grid) and the map's own (rock, brush); here they're drawn.
//!
//! A model comes from `web/assets/models/<name>.glb` (Meshy's, `art/`), its one mesh and material
//! taken out and the material redone as [`FoggedMat`] so it sits in the fog of war. Until a model
//! is in, nothing is drawn for it, and a structure keeps its primitive (`view.rs`).

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::MeshAabb;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use el_client_core::scenery::{Prop, props};
use el_sim::map;
use el_sim::math::hash01;

use crate::coords::{S, world};
use crate::ground::{FogTex, FoggedMat, FoggedParams};
use crate::look::Look;

pub struct SceneryPlugin;

impl Plugin for SceneryPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/snow.wgsl");
        bevy::asset::embedded_asset!(app, "shaders/flame.wgsl");
        app.add_plugins((MaterialPlugin::<SnowMat>::default(), MaterialPlugin::<FlameMat>::default()))
            .add_systems(PreStartup, load)
            .add_systems(Startup, (place, brush, snow))
            .add_systems(Update, (ready, report));
    }
}

/// Every model the scene uses.
pub const MODELS: [&str; 12] = [
    "turret",
    "inhibitor",
    "nexus",
    "fountain",
    "brazier",
    "rock_a",
    "rock_b",
    "pillar",
    "statue",
    "spire",
    "arch",
    "banner",
];

/// A model's mesh and material as they come in, and once in, its bounds.
pub struct Model {
    pub mesh: Handle<Mesh>,
    pub mat: Handle<bevy::gltf::GltfMaterial>,
    /// The bounds' lowest corner and its size, once the mesh is in.
    pub bounds: Option<(Vec3, Vec3)>,
}

#[derive(Resource, Default)]
pub struct Models {
    pub by_name: HashMap<&'static str, Model>,
    /// The fogged materials made from each model's, by model and look.
    made: HashMap<(&'static str, Kind), Handle<FoggedMat>>,
}

/// How a model's material is redone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// As it is.
    Plain,
    /// Its crystal (between these percentages of its height) glowing in a team's colour.
    Glow(u8, u8, u8),
    /// Ice, lit faintly from within.
    Ice,
    /// Darkened, cold: a structure fallen.
    Ruin,
}

impl Models {
    /// The model's mesh, its scale for the size asked and its material, once it's in.
    pub fn get(
        &mut self,
        name: &'static str,
        size: Size,
        kind: Kind,
        fog: &FogTex,
        gltf_mats: &Assets<bevy::gltf::GltfMaterial>,
        mats: &mut Assets<FoggedMat>,
    ) -> Option<(Handle<Mesh>, f32, Handle<FoggedMat>)> {
        let m = self.by_name.get(name)?;
        let (_, extent) = m.bounds?;
        let scale = match size {
            Size::Height(h) => h / extent.y.max(1e-3),
            Size::Radius(r) => 2.0 * r / extent.x.max(extent.z).max(1e-3),
        };
        let mesh = m.mesh.clone();
        let mat = match self.made.get(&(name, kind)) {
            Some(h) => h.clone(),
            None => {
                let mut base = standard(gltf_mats.get(&m.mat)?);
                base.double_sided = true;
                base.cull_mode = None;
                let mut params = FoggedParams { look: Vec4::new(0.0, 0.0, 0.0, 0.0), tint: Vec4::ZERO };
                match kind {
                    Kind::Plain => {}
                    Kind::Glow(team, from, to) => {
                        let h = extent.y * scale / 100.0;
                        params.look.x = 4.0;
                        params.look.w = h * f32::from(from);
                        params.tint = LinearRgba::from(crate::view::TEAM[usize::from(team.min(1))]).to_vec4();
                        params.tint.w = h * f32::from(to) + 0.31;
                    }
                    Kind::Ice => {
                        params.look.x = 0.9;
                        params.look.w = -100.0;
                        params.tint.w = 100.0;
                        params.tint = Vec4::new(0.35, 0.65, 1.0, 1.0);
                    }
                    Kind::Ruin => {
                        base.base_color = Color::srgb(0.38, 0.40, 0.46);
                        params.look.y = 0.8;
                    }
                }
                let h = mats.add(fog.fogged(base, params));
                self.made.insert((name, kind), h.clone());
                h
            }
        };
        Some((mesh, scale, mat))
    }
}

/// A glTF's material as Bevy's standard one: what Meshy's models carry (colour, normal and
/// metal-roughness maps).
fn standard(g: &bevy::gltf::GltfMaterial) -> StandardMaterial {
    StandardMaterial {
        base_color: g.base_color,
        base_color_texture: g.base_color_texture.clone(),
        emissive: g.emissive,
        emissive_texture: g.emissive_texture.clone(),
        perceptual_roughness: g.perceptual_roughness,
        metallic: g.metallic,
        metallic_roughness_texture: g.metallic_roughness_texture.clone(),
        normal_map_texture: g.normal_map_texture.clone(),
        occlusion_texture: g.occlusion_texture.clone(),
        ..default()
    }
}

/// How big a model is drawn: as tall as, or as wide (its radius on the ground) as, so many metres.
#[derive(Clone, Copy, Debug)]
pub enum Size {
    Height(f32),
    Radius(f32),
}

/// A model wanted somewhere, drawn once it's in.
#[derive(Component)]
struct Wants {
    name: &'static str,
    size: Size,
    kind: Kind,
    /// A brazier gets its fire on top once it's in.
    fire: bool,
}

fn load(mut commands: Commands, look: Res<Look>, server: Res<AssetServer>) {
    let mut models = Models::default();
    if !look.art {
        commands.insert_resource(models);
        return;
    }
    for name in MODELS {
        let path = format!("models/{name}.glb");
        models.by_name.insert(
            name,
            Model {
                mesh: server
                    .load(GltfAssetLabel::Primitive { mesh: 0, primitive: 0 }.from_asset(path.clone())),
                mat: server
                    .load(GltfAssetLabel::Material { index: 0, is_scale_inverted: false }.from_asset(path)),
                bounds: None,
            },
        );
    }
    commands.insert_resource(models);
}

/// The props and the rock, wanted where they stand.
fn place(mut commands: Commands, look: Res<Look>) {
    if !look.art {
        return;
    }
    for p in props() {
        let size = match p.prop {
            Prop::Brazier => Size::Radius(p.radius * S * 1.05),
            _ => Size::Height(p.height * S),
        };
        let kind = if matches!(p.prop, Prop::Spire | Prop::IceSpikes) { Kind::Ice } else { Kind::Plain };
        commands.spawn((
            Transform::from_translation(world(p.at, -p.depth * S))
                .with_rotation(Quat::from_rotation_y(p.yaw)),
            Visibility::default(),
            Wants { name: p.prop.model(), size, kind, fire: p.prop == Prop::Brazier },
        ));
    }
    // Rock where the map has rock: a snowy outcrop for the big ones, ice for the small.
    for (k, d) in map::ROCKS.iter().flat_map(|d| [*d, d.mirrored()]).enumerate() {
        let (name, size) = if d.1 >= 250.0 {
            ("rock_a", Size::Radius(d.1 * S * 1.12))
        } else {
            ("rock_b", Size::Radius(d.1 * S * 1.25))
        };
        let yaw = hash01(k as u32, 11) * std::f32::consts::TAU;
        commands.spawn((
            Transform::from_translation(world(d.0, -0.05)).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
            Wants { name, size, kind: Kind::Plain, fire: false },
        ));
    }
}

/// Each frame until every model is in: their bounds read, and what wanted them drawn.
#[allow(clippy::too_many_arguments)]
fn ready(
    mut commands: Commands,
    mut models: ResMut<Models>,
    mut meshes: ResMut<Assets<Mesh>>,
    gltf_mats: Res<Assets<bevy::gltf::GltfMaterial>>,
    mut mats: ResMut<Assets<FoggedMat>>,
    mut flames: ResMut<Assets<FlameMat>>,
    mut quad: Local<Option<Handle<Mesh>>>,
    fog: Res<FogTex>,
    look: Res<Look>,
    mut wanting: Query<(Entity, &Wants, &mut Transform)>,
) {
    for m in models.by_name.values_mut() {
        if m.bounds.is_none()
            && let Some(mut mesh) = meshes.get_mut(&m.mesh)
            && let Some(aabb) = mesh.compute_aabb()
        {
            // Stood on its foot, centred: the file's own placement is on its node, which a
            // primitive loaded alone doesn't carry.
            let (lo, hi) = (Vec3::from(aabb.min()), Vec3::from(aabb.max()));
            let size = hi - lo;
            mesh.translate_by(-Vec3::new(lo.x + size.x * 0.5, lo.y, lo.z + size.z * 0.5));
            m.bounds = Some((Vec3::new(-size.x * 0.5, 0.0, -size.z * 0.5), size));
        }
    }
    for (e, w, mut tf) in &mut wanting {
        let Some((mesh, scale, mat)) = models.get(w.name, w.size, w.kind, &fog, &gltf_mats, &mut mats) else {
            continue;
        };
        tf.scale = Vec3::splat(scale);
        let mut ec = commands.entity(e);
        ec.remove::<Wants>().insert((Mesh3d(mesh), MeshMaterial3d(mat)));
        if w.fire {
            let top = models.by_name[w.name].bounds.map_or(1.0, |(lo, size)| (lo.y + size.y) * scale);
            let quad = quad.get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0))).clone();
            let phase =
                hash01((tf.translation.x * 10.0) as i32 as u32, (tf.translation.z * 10.0) as i32 as u32);
            let flame = flames.add(FlameMat { color: Vec4::new(9.0, 5.2, 2.2, phase) });
            // The fire's quad faces the camera, which never turns: undo the brazier's own turn.
            let face = tf.rotation.inverse() * crate::view::facing_camera();
            let s = 1.0 / scale;
            ec.with_children(|c| {
                c.spawn((
                    Mesh3d(quad),
                    MeshMaterial3d(flame),
                    Transform::from_xyz(0.0, top * s + 0.45 * s, 0.0)
                        .with_rotation(face)
                        .with_scale(Vec3::new(0.9 * s, 1.3 * s, 1.0)),
                    bevy::light::NotShadowCaster,
                ));
                // The fire's light on what's round it: the high tier's (the low draws the fire alone).
                if !look.high {
                    return;
                }
                c.spawn((
                    PointLight {
                        color: Color::srgb(1.0, 0.62, 0.32),
                        intensity: 60_000.0,
                        range: 8.0,
                        radius: 0.2,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_xyz(0.0, top * s + 0.6 * s, 0.0),
                ));
            });
        }
    }
}

/// `window.__el.art`: every texture and model in (a test waits for it before looking).
fn report(
    models: Res<Models>,
    waiting: Query<(), With<Wants>>,
    ground: Res<crate::ground::GroundTex>,
    server: Res<AssetServer>,
    mut status: ResMut<crate::dev_hooks::DevStatus>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let tex = [&ground.albedo, &ground.normal, &ground.rha, &ground.runner];
    *done = waiting.is_empty()
        && models.by_name.values().all(|m| m.bounds.is_some())
        && tex.iter().all(|h| server.is_loaded_with_dependencies(h.id()));
    status.set("art", *done);
}

/// The brush: frosted thickets, a few dozen clumps a brush, each two crossed cards from the
/// sprite atlas (`web/assets/brush.png`, three plants), merged into one mesh a brush.
fn brush(
    mut commands: Commands,
    look: Res<Look>,
    server: Res<AssetServer>,
    fog: Res<FogTex>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<FoggedMat>>,
) {
    if !look.art {
        return;
    }
    let tex: Handle<Image> = server
        .load_builder()
        .with_settings(|s: &mut bevy::image::ImageLoaderSettings| s.sampler = crate::mips::sampler(false))
        .load("brush.png");
    let mat = mats.add(fog.fogged(
        StandardMaterial {
            base_color_texture: Some(tex),
            base_color: Color::srgb(0.86, 0.92, 1.0),
            alpha_mode: AlphaMode::Mask(0.4),
            double_sided: true,
            cull_mode: None,
            perceptual_roughness: 0.8,
            ..default()
        },
        FoggedParams { look: Vec4::new(0.0, 0.0, 1.0, 0.0), tint: Vec4::ZERO },
    ));
    for (b, r) in map::brushes().iter().enumerate() {
        let (lo, hi) = (r.0, r.1);
        let mut pos: Vec<[f32; 3]> = Vec::new();
        let mut uv: Vec<[f32; 2]> = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        // A jittered grid of clumps, denser than anything else that grows on the map.
        let step = 70.0;
        let (nx, ny) = (((hi.x - lo.x) / step) as u32, ((hi.y - lo.y) / step) as u32);
        for j in 0..=ny {
            for i in 0..=nx {
                let h = |k: u32| hash01(b as u32 * 1_000 + j * 97 + i, k);
                let p = Vec2::new(
                    lo.x + (i as f32 + 0.15 + 0.7 * h(1)) * (hi.x - lo.x) / (nx + 1) as f32,
                    lo.y + (j as f32 + 0.15 + 0.7 * h(2)) * (hi.y - lo.y) / (ny + 1) as f32,
                );
                let cell = (h(3) * 3.0) as u32 % 3;
                let tall = match cell {
                    1 => 0.7,
                    2 => 1.6,
                    _ => 1.2,
                } * (0.8 + 0.45 * h(4));
                let base = world(p, 0.0);
                let yaw = h(5) * std::f32::consts::PI;
                for turn in [yaw, yaw + std::f32::consts::FRAC_PI_2] {
                    let side = Vec3::new(turn.cos(), 0.0, turn.sin()) * tall * 0.55;
                    let k = pos.len() as u32;
                    let (u0, u1) = (cell as f32 / 3.0, (cell + 1) as f32 / 3.0);
                    for (c, t) in [(-1.0, 0.0), (1.0, 0.0), (1.0, 1.0), (-1.0, 1.0)] {
                        pos.push((base + side * c + Vec3::Y * tall * t).to_array());
                        uv.push([if c < 0.0 { u0 } else { u1 }, 1.0 - t]);
                    }
                    idx.extend([k, k + 1, k + 2, k, k + 2, k + 3]);
                }
            }
        }
        let n = pos.len();
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_indices(Indices::U32(idx));
        commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat.clone()), Transform::default()));
    }
}

/// Falling snow (`shaders/snow.wgsl`).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct SnowMat {}

impl Material for SnowMat {
    fn vertex_shader() -> ShaderRef {
        "embedded://el_client/shaders/snow.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://el_client/shaders/snow.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
}

/// A brazier's fire (`shaders/flame.wgsl`).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct FlameMat {
    /// The fire's colour (linear, bright), and its flicker's phase.
    #[uniform(0)]
    pub color: Vec4,
}

impl Material for FlameMat {
    fn fragment_shader() -> ShaderRef {
        "embedded://el_client/shaders/flame.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
}

fn snow(
    mut commands: Commands,
    look: Res<Look>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<SnowMat>>,
) {
    if !look.art {
        return;
    }
    let flakes = if look.high { 3_000u32 } else { 800 };
    let mut pos = Vec::with_capacity(flakes as usize * 4);
    let mut uv = Vec::with_capacity(flakes as usize * 4);
    let mut idx = Vec::with_capacity(flakes as usize * 6);
    for f in 0..flakes {
        let p = [hash01(f, 1) * 48.0, hash01(f, 2) * 18.0, hash01(f, 3) * 36.0];
        let k = f * 4;
        for c in [[0.0f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] {
            pos.push(p);
            uv.push(c);
        }
        idx.extend([k, k + 1, k + 2, k, k + 2, k + 3]);
    }
    let n = pos.len();
    let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx));
    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(mats.add(SnowMat {})),
        Transform::default(),
        // The flakes are moved in the shader, far from the mesh's own bounds.
        NoFrustumCulling,
        bevy::light::NotShadowCaster,
    ));
}
