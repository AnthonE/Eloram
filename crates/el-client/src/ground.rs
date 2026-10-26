//! The Causeway's ground and the materials everything is drawn with (`docs/ART.md`, step 2).
//!
//! [`Ground`] is the stone: the floor (paving, snow and gravel blended by a splat baked here from
//! the map's own shapes, the runner down the lane, the rings round the bases) and the cliffs and
//! parapets at its edges (`edges.rs`). [`Fogged`] is everything that stands on it: models, units,
//! brush. Both lay the fog of war over what they draw, read from one texture ([`FogTex`]) that
//! `view.rs` keeps up to date, so a wall or a statue sits in the dark with the floor round it.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageArrayLayout, ImageLoaderSettings};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use el_sim::map::{self, VIS_H, VIS_W};
use el_sim::{MAP_H, MAP_W};

use crate::coords::{S, world};

pub type GroundMat = ExtendedMaterial<StandardMaterial, Ground>;
pub type FoggedMat = ExtendedMaterial<StandardMaterial, Fogged>;

/// The ground's stone (`shaders/ground.wgsl`).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct Ground {
    #[uniform(100)]
    pub params: GroundParams,
    #[texture(101, dimension = "2d_array")]
    #[sampler(102)]
    pub albedo: Handle<Image>,
    #[texture(103, dimension = "2d_array")]
    #[sampler(104)]
    pub normal: Handle<Image>,
    #[texture(105, dimension = "2d_array")]
    #[sampler(106)]
    pub rha: Handle<Image>,
    #[texture(107)]
    #[sampler(108)]
    pub splat: Handle<Image>,
    #[texture(109)]
    #[sampler(110)]
    pub runner: Handle<Image>,
    #[texture(111)]
    #[sampler(112)]
    pub fog: Handle<Image>,
}

#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct GroundParams {
    /// x: the mode (0 floor, 1 cliff rock, 2 dressed slabs); y: the layer's tile (m); z, w: the
    /// heights the depth's darkening runs between.
    pub mode: Vec4,
    /// x: 1 on the low tier (a few textures read, not a score).
    pub tier: Vec4,
    /// The runner: from x0 to x1 (map units), its half width, its tile's length.
    pub runner: Vec4,
    /// The rings: centre, inner and outer radius (map units).
    pub rings: [Vec4; 4],
    /// The rings' glow, Dawn's and Dusk's (linear).
    pub team: [Vec4; 2],
}

impl MaterialExtension for Ground {
    fn fragment_shader() -> ShaderRef {
        "embedded://el_client/shaders/ground.wgsl".into()
    }
}

/// Whatever stands on the ground (`shaders/fogged.wgsl`).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct Fogged {
    #[uniform(100)]
    pub params: FoggedParams,
    #[texture(101)]
    #[sampler(102)]
    pub fog: Handle<Image>,
}

#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct FoggedParams {
    /// x: the crystal's glow; y: snow on top; z: foliage (1) or not; w: the height (m) the crystal
    /// starts at.
    pub look: Vec4,
    /// The glow's colour (linear); w: the height (m) the crystal ends at.
    pub tint: Vec4,
}

impl MaterialExtension for Fogged {
    fn fragment_shader() -> ShaderRef {
        "embedded://el_client/shaders/fogged.wgsl".into()
    }
}

/// The fog of war's texture: a texel a vision cell, its darkness in red (0 seen, up to 0.58
/// unseen), the map's far side the top row.
#[derive(Resource, Clone)]
pub struct FogTex(pub Handle<Image>);

impl FogTex {
    /// A material for something standing in the fog.
    pub fn fogged(&self, base: StandardMaterial, params: FoggedParams) -> FoggedMat {
        ExtendedMaterial { base, extension: Fogged { params, fog: self.0.clone() } }
    }

    /// The plain kind: no glow, no snow.
    pub fn plain(&self, base: StandardMaterial) -> FoggedMat {
        self.fogged(base, FoggedParams::default())
    }
}

/// The ground's textures, loaded once.
#[derive(Resource, Clone)]
pub struct GroundTex {
    pub albedo: Handle<Image>,
    pub normal: Handle<Image>,
    pub rha: Handle<Image>,
    pub runner: Handle<Image>,
    pub splat: Handle<Image>,
    /// The low tier's cheaper shading.
    pub low: bool,
}

impl GroundTex {
    pub fn material(&self, fog: &FogTex, mut params: GroundParams) -> GroundMat {
        params.tier.x = if self.low { 1.0 } else { 0.0 };
        ExtendedMaterial {
            // Both sides: the edges' faces are stitched from rings, and the shader faces each
            // normal to the camera.
            base: StandardMaterial {
                perceptual_roughness: 0.9,
                double_sided: true,
                cull_mode: None,
                ..default()
            },
            extension: Ground {
                params,
                albedo: self.albedo.clone(),
                normal: self.normal.clone(),
                rha: self.rha.clone(),
                splat: self.splat.clone(),
                runner: self.runner.clone(),
                fog: fog.0.clone(),
            },
        }
    }
}

/// The runner: where it runs, its half width and its tile (map units).
const RUNNER: Vec4 = Vec4::new(3_500.0, MAP_W - 3_500.0, 250.0, 500.0);

pub struct GroundPlugin;

impl Plugin for GroundPlugin {
    fn build(&self, app: &mut App) {
        bevy::shader::load_shader_library!(app, "shaders/fog.wgsl");
        bevy::asset::embedded_asset!(app, "shaders/ground.wgsl");
        bevy::asset::embedded_asset!(app, "shaders/fogged.wgsl");
        app.add_plugins((MaterialPlugin::<GroundMat>::default(), MaterialPlugin::<FoggedMat>::default()));
        let fog = {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            let mut img = Image::new(
                Extent3d { width: VIS_W as u32, height: VIS_H as u32, depth_or_array_layers: 1 },
                TextureDimension::D2,
                [153, 0, 0, 255].repeat(VIS_W * VIS_H),
                TextureFormat::Rgba8Unorm,
                RenderAssetUsages::default(),
            );
            img.sampler = crate::mips::sampler(false);
            images.add(img)
        };
        app.insert_resource(FogTex(fog)).add_systems(PreStartup, load).add_systems(Startup, floor);
    }
}

fn load(
    mut commands: Commands,
    look: Res<crate::look::Look>,
    server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
) {
    if !look.art {
        commands.insert_resource(GroundTex {
            albedo: default(),
            normal: default(),
            rha: default(),
            runner: default(),
            splat: default(),
            low: true,
        });
        return;
    }
    let array = |srgb: bool| {
        move |s: &mut ImageLoaderSettings| {
            s.array_layout = Some(ImageArrayLayout::RowCount { rows: 5 });
            s.is_srgb = srgb;
            s.sampler = crate::mips::sampler(true);
        }
    };
    commands.insert_resource(GroundTex {
        albedo: server.load_builder().with_settings(array(true)).load("ground/albedo.jpg"),
        normal: server.load_builder().with_settings(array(false)).load("ground/normal.jpg"),
        rha: server.load_builder().with_settings(array(false)).load("ground/rha.jpg"),
        runner: server
            .load_builder()
            .with_settings(|s: &mut ImageLoaderSettings| {
                s.is_srgb = false;
                s.sampler = crate::mips::sampler(true);
            })
            .load("ground/runner.png"),
        splat: images.add(splat()),
        low: !look.high,
    });
}

/// The rings round the bases: the platform's and the nexus's, Dawn's then Dusk's.
fn rings() -> [Vec4; 4] {
    let ring = |c: Vec2, r: f32| Vec4::new(c.x, c.y, r, r + 102.5);
    let p = map::PLATFORM.0;
    let n = map::NEXUS.0;
    [ring(p, 450.0), ring(n, 330.0), ring(map::mirror(p), 450.0), ring(map::mirror(n), 330.0)]
}

fn floor(
    mut commands: Commands,
    look: Res<crate::look::Look>,
    tex: Res<GroundTex>,
    fog: Res<FogTex>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<GroundMat>>,
) {
    if !look.art {
        return;
    }
    let glow = |c: Color| LinearRgba::from(c).to_vec4() * 1.6;
    let params = GroundParams {
        mode: Vec4::new(0.0, 7.0, 0.0, 0.0),
        runner: RUNNER,
        rings: rings(),
        team: [glow(crate::view::TEAM[0]), glow(crate::view::TEAM[1])],
        ..default()
    };
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(MAP_W * S, MAP_H * S))),
        MeshMaterial3d(mats.add(tex.material(&fog, params))),
        Transform::from_translation(world(Vec2::new(MAP_W, MAP_H) * 0.5, 0.0)),
        bevy::light::NotShadowCaster,
    ));
}

/// The splat: a texel every 20 map units over the whole map. Red: snow (thick toward the floor's
/// edge and round rock, in drifts elsewhere); green: gravel (at the rocks' feet, under the brush);
/// blue: how open to the sky (darker against walls, rock, structures and in the brush); alpha: the
/// floor itself (it ends at the edge, under the parapet; beyond is the chasm).
fn splat() -> Image {
    const K: f32 = 20.0;
    let (w, h) = ((MAP_W / K) as usize, (MAP_H / K) as usize);
    let rocks: Vec<map::Disc> = map::ROCKS.iter().flat_map(|d| [*d, d.mirrored()]).collect();
    let dawn_structures = [
        map::OUTER_TURRET,
        map::INHIB_TURRET,
        map::INHIBITOR,
        map::NEXUS,
        map::NEXUS_TURRETS[0],
        map::NEXUS_TURRETS[1],
    ];
    let structures: Vec<map::Disc> = dawn_structures.iter().flat_map(|d| [*d, d.mirrored()]).collect();
    let brushes = map::brushes();
    let mut data = vec![0u8; w * h * 4];
    for py in 0..h {
        for px in 0..w {
            // The top row is the far side.
            let p = Vec2::new((px as f32 + 0.5) * K, MAP_H - (py as f32 + 0.5) * K);
            let edge = -el_client_core::scenery::floor_distance(p);
            let rock = rocks.iter().map(|d| (p - d.0).length() - d.1).fold(f32::INFINITY, f32::min);
            let tower = structures.iter().map(|d| (p - d.0).length() - d.1).fold(f32::INFINITY, f32::min);
            let in_brush = brushes.iter().any(|r| r.contains(p, -30.0));
            let n = fbm(p / 900.0);
            let fine = fbm(p / 160.0 + 31.0);
            let mut snow = smooth(220.0, 20.0, edge) * 0.85 + smooth(160.0, 0.0, rock) * 0.5;
            snow += ((n - 0.66) * 2.4).max(0.0) + ((fine - 0.7) * 1.6).max(0.0) * 0.5;
            // The bases are swept: less snow on the platforms and round the nexus.
            let swept = smooth(900.0, 500.0, (p - map::for_team(p_team(p), map::NEXUS.0)).length());
            snow *= 1.0 - 0.6 * swept;
            let mut gravel = smooth(220.0, 0.0, rock) * 0.9 + if in_brush { 0.55 } else { 0.0 };
            gravel += ((fine - 0.66) * 2.0).max(0.0) * 0.5;
            let mut open = 0.55 + 0.45 * smooth(0.0, 260.0, edge);
            open *= 0.6 + 0.4 * smooth(0.0, 180.0, rock);
            open *= 0.7 + 0.3 * smooth(0.0, 140.0, tower);
            if in_brush {
                open *= 0.72;
            }
            let k = (py * w + px) * 4;
            data[k] = (snow.clamp(0.0, 1.0) * 255.0) as u8;
            data[k + 1] = (gravel.clamp(0.0, 1.0) * 255.0) as u8;
            data[k + 2] = (open.clamp(0.0, 1.0) * 255.0) as u8;
            data[k + 3] = if edge > -15.0 { 255 } else { 0 };
        }
    }
    let mut img = Image::new(
        Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    img.sampler = crate::mips::sampler(false);
    img
}

/// Whose half a point is on.
fn p_team(p: Vec2) -> u8 {
    if p.x < MAP_W * 0.5 { map::DAWN } else { map::DUSK }
}

/// 1 at `a`, 0 at `b`, smooth between (either way round).
fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Value noise in octaves, from the sim's hash (only for the picture: nothing here is the match's).
fn fbm(p: Vec2) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut q = p;
    for o in 0..4u32 {
        sum += amp * value(q, o);
        q *= 2.03;
        amp *= 0.5;
    }
    sum / 0.9375
}

fn value(p: Vec2, seed: u32) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let h = |x: f32, y: f32| {
        el_sim::math::hash01((x as i32 as u32).wrapping_add(seed.wrapping_mul(7_919)), y as i32 as u32)
    };
    let a = h(i.x, i.y);
    let b = h(i.x + 1.0, i.y);
    let c = h(i.x, i.y + 1.0);
    let d = h(i.x + 1.0, i.y + 1.0);
    a + (b - a) * u.x + (c - a) * u.y + (a - b - c + d) * u.x * u.y
}
