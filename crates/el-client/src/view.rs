//! The match drawn: the Causeway (its floor from the map's own shapes, rock, brush), every unit
//! slot an entity kept from the start (shown while the team sees its unit), missiles at the own
//! champion's clock (`docs/NETCODE.md`, "Closed forms go once"), area flashes, relics, and the fog
//! over what the team can't see.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use el_client_core::fog;
use el_proto::event::WireEvent;
use el_proto::quant::pos_from_code;
use el_sim::map::{self, VIS_H, VIS_W};
use el_sim::missile::MAX_MISSILES;
use el_sim::units::{Kind, MAX_UNITS, SEATS};
use el_sim::{MAP_H, MAP_W};

use crate::coords::{S, world};
use crate::input::{Armed, Controls};
use crate::net::now_s;
use crate::session::Session;

/// A unit as it was drawn this frame: what the input picks from.
#[derive(Clone, Copy, Debug)]
pub struct DrawnUnit {
    pub pos: Vec2,
    pub radius: f32,
    pub team: u8,
    pub generation: u8,
}

#[derive(Resource)]
pub struct Drawn {
    pub units: Vec<Option<DrawnUnit>>,
}

impl Default for Drawn {
    fn default() -> Self {
        Self { units: vec![None; MAX_UNITS] }
    }
}

/// The floor's texture (also the minimap's).
#[derive(Resource, Clone)]
pub struct FloorImage(pub Handle<Image>);

/// Each team's colour, and each champion's.
pub const TEAM: [Color; 2] = [Color::srgb(0.24, 0.55, 0.98), Color::srgb(0.95, 0.32, 0.26)];
pub const CHAMPION: [Color; 20] = [
    Color::srgb(0.92, 0.76, 0.32),
    Color::srgb(0.62, 0.42, 0.92),
    Color::srgb(0.46, 0.62, 0.40),
    Color::srgb(0.70, 0.18, 0.28),
    Color::srgb(0.30, 0.80, 0.76),
    Color::srgb(0.77, 0.46, 0.33),
    Color::srgb(0.16, 0.16, 0.68),
    Color::srgb(0.43, 0.83, 0.27),
    Color::srgb(0.62, 0.22, 0.45),
    Color::srgb(0.27, 0.75, 0.83),
    Color::srgb(0.68, 0.60, 0.16),
    Color::srgb(0.57, 0.33, 0.77),
    Color::srgb(0.16, 0.68, 0.29),
    Color::srgb(0.83, 0.27, 0.29),
    Color::srgb(0.22, 0.35, 0.62),
    Color::srgb(0.62, 0.83, 0.27),
    Color::srgb(0.68, 0.16, 0.63),
    Color::srgb(0.33, 0.77, 0.68),
    Color::srgb(0.68, 0.42, 0.16),
    Color::srgb(0.39, 0.27, 0.83),
];

const _: () = assert!(CHAMPION.len() == el_sim::content::champions::CHAMPIONS.len());

#[derive(Resource)]
struct Kit {
    bodies: [Handle<Mesh>; Kind::COUNT],
    team: [[Handle<StandardMaterial>; 2]; Kind::COUNT],
    champion: [Handle<StandardMaterial>; 20],
    ring: [Handle<StandardMaterial>; 2],
    fill: [Handle<StandardMaterial>; 3],
    missile: [Handle<StandardMaterial>; 2],
}

#[derive(Component)]
struct UnitView {
    slot: usize,
    body: Entity,
    ring: Entity,
    bar: Entity,
    fill: Entity,
    shown: Option<(Kind, u8, u8)>,
}

#[derive(Component)]
struct MissileView(usize);

#[derive(Component)]
struct Flash {
    born: f64,
    life: f64,
    radius: f32,
}

#[derive(Component)]
struct RelicView(usize);

#[derive(Resource)]
struct FogImage {
    handle: Handle<Image>,
    /// Each cell's darkness now (eased toward what the team sees).
    dark: Vec<f32>,
    last: f64,
}

/// A health bar's width (scene units) and height above the ground, by kind.
fn bar_of(kind: Kind) -> (f32, f32) {
    match kind {
        Kind::Champion => (1.5, 2.4),
        k if k.is_minion() => (0.8, 1.1),
        Kind::Turret => (2.2, 3.9),
        Kind::Nexus => (3.0, 3.6),
        _ => (2.4, 2.6),
    }
}

fn radius_of(kind: Kind) -> f32 {
    match kind {
        Kind::Champion => 60.0,
        Kind::Melee | Kind::Caster => 48.0,
        Kind::Siege | Kind::Super => 65.0,
        Kind::Turret => 110.0,
        Kind::Inhibitor => 150.0,
        Kind::Nexus => 260.0,
        Kind::Fountain => 80.0,
    }
}

/// Faces a flat thing lying in XY to the camera (which only ever moves, never turns).
fn facing_camera() -> Quat {
    let o = crate::camera::offset(1.0).normalize();
    Quat::from_rotation_x(-o.y.atan2(o.z))
}

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Drawn>()
            .insert_resource(GlobalAmbientLight { brightness: 260.0, ..default() })
            .add_systems(Startup, (setup_map, setup_pools))
            .add_systems(
                Update,
                (draw_units, draw_missiles, draw_flashes, draw_relics, draw_fog, draw_gizmos)
                    .after(crate::net::NetSet::Send),
            );
    }
}

fn floor_image() -> Image {
    const W: usize = 960;
    const H: usize = 240;
    let mut data = vec![0u8; W * H * 4];
    let brushes = map::brushes();
    for py in 0..H {
        for px in 0..W {
            let p =
                Vec2::new((px as f32 + 0.5) * MAP_W / W as f32, MAP_H - (py as f32 + 0.5) * MAP_H / H as f32);
            let n = (el_sim::math::hash01(px as u32, py as u32) - 0.5) * 0.05;
            let (r, g, b) = if !map::on_floor(p, 0.0) {
                (0.03, 0.04, 0.07)
            } else if brushes.iter().any(|r| r.contains(p, 0.0)) {
                (0.16 + n, 0.30 + n, 0.14 + n)
            } else {
                // Each side's stone a little its colour, the middle neutral.
                let side = ((p.x / MAP_W) - 0.5) * 2.0;
                let (tr, tb) = (side.max(0.0) * 0.05, (-side).max(0.0) * 0.05);
                (0.36 + n + tr, 0.35 + n, 0.33 + n + tb)
            };
            let k = (py * W + px) * 4;
            data[k] = (r.clamp(0.0, 1.0) * 255.0) as u8;
            data[k + 1] = (g.clamp(0.0, 1.0) * 255.0) as u8;
            data[k + 2] = (b.clamp(0.0, 1.0) * 255.0) as u8;
            data[k + 3] = 255;
        }
    }
    Image::new(
        Extent3d { width: W as u32, height: H as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

fn setup_map(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let floor = images.add(floor_image());
    commands.insert_resource(FloorImage(floor.clone()));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(MAP_W * S, MAP_H * S))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(floor),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::default(),
    ));
    let rock = materials.add(StandardMaterial {
        base_color: Color::srgb(0.30, 0.27, 0.25),
        perceptual_roughness: 0.9,
        ..default()
    });
    for d in map::ROCKS.iter().flat_map(|d| [*d, d.mirrored()]) {
        commands.spawn((
            Mesh3d(meshes.add(Cylinder::new(d.1 * S, 1.6))),
            MeshMaterial3d(rock.clone()),
            Transform::from_translation(world(d.0, 0.8)),
        ));
    }
    let grass = materials.add(StandardMaterial {
        base_color: Color::srgba(0.20, 0.42, 0.18, 0.88),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 1.0,
        ..default()
    });
    for r in map::brushes() {
        let size = (r.1 - r.0) * S;
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(size.x, 0.45, size.y))),
            MeshMaterial3d(grass.clone()),
            Transform::from_translation(world((r.0 + r.1) * 0.5, 0.22)),
        ));
    }
    // The fog, just over the floor.
    let fog_img = Image::new(
        Extent3d { width: VIS_W as u32, height: VIS_H as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0u8; VIS_W * VIS_H * 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    let handle = images.add(fog_img);
    commands.insert_resource(FogImage { handle: handle.clone(), dark: vec![0.6; VIS_W * VIS_H], last: 0.0 });
    commands.spawn((
        Mesh3d(
            meshes.add(
                Plane3d::default()
                    .mesh()
                    .size(VIS_W as f32 * map::VIS_CELL * S, VIS_H as f32 * map::VIS_CELL * S),
            ),
        ),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(handle),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        })),
        Transform::from_translation(world(
            Vec2::new(VIS_W as f32 * map::VIS_CELL, VIS_H as f32 * map::VIS_CELL) * 0.5,
            0.05,
        )),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 9_000.0, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(-20.0, 30.0, 18.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn setup_pools(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut mat = |c: Color, glow: f32| {
        materials.add(StandardMaterial {
            base_color: c,
            emissive: LinearRgba::from(c) * glow,
            perceptual_roughness: 0.6,
            ..default()
        })
    };
    let bodies: [Handle<Mesh>; Kind::COUNT] = [
        meshes.add(Capsule3d::new(0.5, 0.9)),
        meshes.add(Cuboid::new(0.55, 0.55, 0.55)),
        meshes.add(Sphere::new(0.32)),
        meshes.add(Cuboid::new(0.95, 0.6, 0.75)),
        meshes.add(Cuboid::new(1.1, 1.1, 1.1)),
        meshes.add(Cylinder::new(1.0, 3.2)),
        meshes.add(Sphere::new(1.4)),
        meshes.add(Sphere::new(2.4)),
        meshes.add(Cylinder::new(0.8, 2.0)),
    ];
    let team = std::array::from_fn(|k| {
        let glow = if matches!(Kind::from_u8(k as u8), Kind::Nexus | Kind::Inhibitor) { 0.5 } else { 0.08 };
        [mat(TEAM[0], glow), mat(TEAM[1], glow)]
    });
    let champion = CHAMPION.map(|c| mat(c, 0.05));
    let unlit = |materials: &mut Assets<StandardMaterial>, c: Color| {
        materials.add(StandardMaterial {
            base_color: c,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })
    };
    let ring =
        [unlit(&mut materials, TEAM[0].with_alpha(0.7)), unlit(&mut materials, TEAM[1].with_alpha(0.7))];
    let fill = [
        unlit(&mut materials, Color::srgb(0.35, 0.92, 0.35)),
        unlit(&mut materials, Color::srgb(0.30, 0.66, 1.0)),
        unlit(&mut materials, Color::srgb(0.95, 0.28, 0.24)),
    ];
    let bar_bg = unlit(&mut materials, Color::srgba(0.0, 0.0, 0.0, 0.75));
    let missile = [
        materials.add(StandardMaterial {
            base_color: TEAM[0],
            emissive: LinearRgba::from(TEAM[0]) * 3.0,
            unlit: true,
            ..default()
        }),
        materials.add(StandardMaterial {
            base_color: TEAM[1],
            emissive: LinearRgba::from(TEAM[1]) * 3.0,
            unlit: true,
            ..default()
        }),
    ];
    let ring_mesh = meshes.add(Annulus::new(0.85, 1.0));
    let bar_mesh = meshes.add(Rectangle::new(1.0, 0.13));
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let face = facing_camera();
    for slot in 0..MAX_UNITS {
        let body = commands
            .spawn((Mesh3d(bodies[0].clone()), MeshMaterial3d(team[0][0].clone()), Transform::default()))
            .id();
        let ring_e = commands
            .spawn((
                Mesh3d(ring_mesh.clone()),
                MeshMaterial3d(ring[0].clone()),
                Transform::from_xyz(0.0, 0.03, 0.0).with_rotation(flat),
            ))
            .id();
        let bar = commands
            .spawn((
                Mesh3d(bar_mesh.clone()),
                MeshMaterial3d(bar_bg.clone()),
                Transform::from_xyz(0.0, 2.4, 0.0).with_rotation(face),
            ))
            .id();
        let fill_e = commands
            .spawn((
                Mesh3d(bar_mesh.clone()),
                MeshMaterial3d(fill[1].clone()),
                Transform::from_xyz(0.0, 2.4, 0.01).with_rotation(face),
            ))
            .id();
        commands
            .spawn((
                Transform::default(),
                Visibility::Hidden,
                UnitView { slot, body, ring: ring_e, bar, fill: fill_e, shown: None },
            ))
            .add_children(&[body, ring_e, bar, fill_e]);
    }
    let missile_mesh = meshes.add(Sphere::new(1.0));
    for k in 0..MAX_MISSILES / 2 {
        commands.spawn((
            Mesh3d(missile_mesh.clone()),
            MeshMaterial3d(missile[0].clone()),
            Transform::default(),
            Visibility::Hidden,
            MissileView(k),
        ));
    }
    let relic = materials.add(StandardMaterial {
        base_color: Color::srgb(0.4, 1.0, 0.5),
        emissive: LinearRgba::rgb(0.6, 2.0, 0.8),
        ..default()
    });
    let relic_mesh = meshes.add(Sphere::new(0.35));
    for k in 0..4 {
        commands.spawn((
            Mesh3d(relic_mesh.clone()),
            MeshMaterial3d(relic.clone()),
            Transform::default(),
            Visibility::Hidden,
            RelicView(k),
        ));
    }
    commands.insert_resource(Kit { bodies, team, champion, ring, fill, missile });
}

#[allow(clippy::type_complexity)]
fn draw_units(
    session: Res<Session>,
    kit: Res<Kit>,
    mut drawn: ResMut<Drawn>,
    mut roots: Query<(&mut UnitView, &mut Transform, &mut Visibility)>,
    mut parts: Query<(&mut Transform, &mut Mesh3d, &mut MeshMaterial3d<StandardMaterial>), Without<UnitView>>,
) {
    let now = now_s();
    // What's drawn: the match being played, as the team sees it, or the one being watched, all
    // of it.
    let watch = session.watch.as_deref();
    let client = session.client.as_deref().filter(|_| watch.is_none());
    if watch.is_none() && client.is_none() {
        for (_, _, mut vis) in &mut roots {
            *vis = Visibility::Hidden;
        }
        return;
    }
    let (seat, my_team) = client.map_or((usize::MAX, u8::MAX), |c| (c.seat, c.team));
    for (mut uv, mut tf, mut vis) in &mut roots {
        let i = uv.slot;
        let unit = match (watch, client) {
            (Some(w), _) => {
                let u = &w.referee.m.units[i];
                u.alive.then(|| {
                    let frac = u.hp / u.stats.max_hp.max(1.0);
                    (w.pos(i), u.facing, u.kind, u.team, u.generation as u8, frac)
                })
            }
            (None, Some(c)) if i == seat => {
                let u = &c.pred.units[seat];
                u.alive.then(|| {
                    let frac = u.hp / u.stats.max_hp.max(1.0);
                    (c.own_pos(now), u.facing, Kind::Champion, u.team, u.generation as u8, frac)
                })
            }
            (None, Some(c)) => match c.unit(i, now) {
                Some(s) => Some((
                    s.pos,
                    s.facing,
                    Kind::from_u8(s.q.kind),
                    s.q.team,
                    s.q.generation,
                    s.q.hp as f32 / 1_023.0,
                )),
                None => {
                    // A structure no record has come for yet: where the map puts it.
                    let u = &c.world.units[i];
                    (u.alive && u.kind.is_structure() && c.tracks[i].newest().is_none()).then_some((
                        u.pos,
                        u.facing,
                        u.kind,
                        u.team,
                        u.generation as u8,
                        1.0,
                    ))
                }
            },
            (None, None) => None,
        };
        let Some((pos, facing, kind, team, generation, hp)) = unit else {
            *vis = Visibility::Hidden;
            drawn.units[i] = None;
            continue;
        };
        *vis = Visibility::Visible;
        tf.translation = world(pos, 0.0);
        drawn.units[i] = Some(DrawnUnit { pos, radius: radius_of(kind), team, generation });
        let champs = watch.map_or_else(|| client.map(|c| &c.world.champs), |w| Some(&w.referee.m.champs));
        let def = if i < SEATS { champs.map_or(0, |ch| ch[i].def) } else { 0 };
        let (w, h) = bar_of(kind);
        if uv.shown != Some((kind, team, def)) {
            uv.shown = Some((kind, team, def));
            if let Ok((_, mut mesh, mut m)) = parts.get_mut(uv.body) {
                mesh.0 = kit.bodies[kind as usize].clone();
                m.0 = if kind == Kind::Champion {
                    kit.champion[usize::from(def) % kit.champion.len()].clone()
                } else {
                    kit.team[kind as usize][usize::from(team.min(1))].clone()
                };
            }
            if let Ok((mut t, _, mut m)) = parts.get_mut(uv.ring) {
                m.0 = kit.ring[usize::from(team.min(1))].clone();
                t.scale = Vec3::splat(radius_of(kind) * S * 1.25);
            }
            if let Ok((mut t, _, _)) = parts.get_mut(uv.bar) {
                t.translation.y = h;
                t.scale = Vec3::new(w, 1.0, 1.0);
            }
            if let Ok((_, _, mut m)) = parts.get_mut(uv.fill) {
                // Yours, your team's, theirs; watching, each side its own colour.
                let k = if i == seat {
                    0
                } else if team == my_team || (watch.is_some() && team == 0) {
                    1
                } else {
                    2
                };
                m.0 = kit.fill[k].clone();
            }
        }
        if let Ok((mut t, _, _)) = parts.get_mut(uv.body) {
            let lift = match kind {
                Kind::Champion => 0.95,
                Kind::Melee => 0.28,
                Kind::Caster => 0.34,
                Kind::Siege => 0.32,
                Kind::Super => 0.55,
                Kind::Turret => 1.6,
                Kind::Fountain => 1.0,
                _ => 0.6,
            };
            t.translation.y = lift;
            t.rotation = Quat::from_rotation_y(facing);
        }
        if let Ok((mut t, _, _)) = parts.get_mut(uv.fill) {
            let f = hp.clamp(0.0, 1.0);
            t.scale = Vec3::new(w * f, 1.0, 1.0);
            t.translation = Vec3::new(-(1.0 - f) * w * 0.5, h, 0.01);
        }
    }
}

fn draw_missiles(
    session: Res<Session>,
    kit: Res<Kit>,
    drawn: Res<Drawn>,
    mut q: Query<(&MissileView, &mut Transform, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    // Each live missile: where it's drawn, how big, whose.
    let mut shots: Vec<(Vec2, f32, u8)> = Vec::new();
    if let Some(w) = session.watch.as_deref() {
        let m = &w.referee.m;
        for ms in m.missiles.iter().filter(|ms| ms.alive) {
            let r = if ms.homing { 0.14 } else { (ms.width * S * 0.5).clamp(0.16, 0.6) };
            shots.push((ms.pos, r, ms.team));
        }
    } else if let Some(c) = session.client.as_ref() {
        let at = c.missile_tick(now_s());
        for (_, s) in c.missiles.live() {
            let target = drawn.units.get(usize::from(s.target)).copied().flatten().map(|d| d.pos);
            let r = if s.homing { 0.14 } else { (s.width * S * 0.5).clamp(0.16, 0.6) };
            shots.push((s.pos(at, target), r, s.team));
        }
    }
    let mut views: Vec<_> = q.iter_mut().collect();
    views.sort_by_key(|v| v.0.0);
    let mut shots = shots.into_iter();
    for (_, mut tf, mut vis, mut mat) in views {
        let Some((p, r, team)) = shots.next() else {
            *vis = Visibility::Hidden;
            continue;
        };
        *vis = Visibility::Visible;
        *tf = Transform::from_translation(world(p, 1.1)).with_scale(Vec3::splat(r));
        mat.0 = kit.missile[usize::from(team.min(1))].clone();
    }
}

/// Spawns a flash for an area that landed, and fades every flash out.
fn draw_flashes(
    mut commands: Commands,
    mut session: ResMut<Session>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut flashes: Query<(Entity, &Flash, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut ring: Local<Option<Handle<Mesh>>>,
) {
    let now = now_s();
    let mesh = ring.get_or_insert_with(|| meshes.add(Annulus::new(0.9, 1.0))).clone();
    let mut new = Vec::new();
    session.read_events(now, |e| {
        if let WireEvent::Area { at, radius, team } = *e {
            new.push((pos_from_code(at), f32::from(radius), team));
        }
    });
    for (at, radius, team) in new {
        let m = materials.add(StandardMaterial {
            base_color: TEAM[usize::from(team.min(1))].with_alpha(0.9),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(m),
            Transform::from_translation(world(at, 0.08))
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
            Flash { born: now, life: 0.6, radius: radius * S },
        ));
    }
    for (e, f, mut tf, m) in &mut flashes {
        let t = ((now - f.born) / f.life) as f32;
        if t >= 1.0 {
            materials.remove(&m.0);
            commands.entity(e).despawn();
            continue;
        }
        tf.scale = Vec3::splat(f.radius * (0.6 + 0.4 * t));
        if let Some(mut mat) = materials.get_mut(&m.0) {
            mat.base_color.set_alpha(0.9 * (1.0 - t));
        }
    }
}

fn draw_relics(session: Res<Session>, mut q: Query<(&RelicView, &mut Transform, &mut Visibility)>) {
    let relics = match (session.watch.as_deref(), session.client.as_deref()) {
        (Some(w), _) => &w.referee.m.relics,
        (None, Some(c)) => &c.world.relics,
        (None, None) => return,
    };
    let now = now_s();
    for (r, mut tf, mut vis) in &mut q {
        let relic = &relics[r.0];
        *vis = if relic.up { Visibility::Visible } else { Visibility::Hidden };
        let bob = (now * 2.0 + r.0 as f64).sin() as f32 * 0.12;
        tf.translation = world(relic.pos, 0.7 + bob);
    }
}

fn draw_fog(session: Res<Session>, mut fog_img: ResMut<FogImage>, mut images: ResMut<Assets<Image>>) {
    let now = now_s();
    let dt = (now - fog_img.last).clamp(0.0, 0.25) as f32;
    fog_img.last = now;
    // Watching, everything is seen.
    let grid = match (session.watch.as_deref(), session.client.as_deref()) {
        (Some(_), _) => [u64::MAX; el_sim::map::VIS_WORDS * VIS_H],
        (None, Some(c)) => fog::team_grid(&c.pred, c.team),
        (None, None) => return,
    };
    let ease = 1.0 - (-dt * 8.0).exp();
    let handle = fog_img.handle.clone();
    let Some(mut image) = images.get_mut(&handle) else { return };
    let Some(data) = image.data.as_mut() else { return };
    for y in 0..VIS_H {
        for x in 0..VIS_W {
            let want = if fog::lit(&grid, x, y) { 0.0 } else { 0.58 };
            let k = y * VIS_W + x;
            fog_img.dark[k] += (want - fog_img.dark[k]) * ease;
            // The image's top row is the map's far edge.
            let px = ((VIS_H - 1 - y) * VIS_W + x) * 4;
            data[px] = 4;
            data[px + 1] = 6;
            data[px + 2] = 14;
            data[px + 3] = (fog_img.dark[k] * 255.0) as u8;
        }
    }
}

fn draw_gizmos(mut gizmos: Gizmos, session: Res<Session>, controls: Res<Controls>, drawn: Res<Drawn>) {
    let flat = Quat::from_rotation_arc(Vec3::Z, Vec3::Y);
    let now = now_s();
    if let Some((at, when, fight)) = controls.marker {
        let t = (now - when) as f32;
        if t < 0.5 {
            let color = if fight { Color::srgb(1.0, 0.4, 0.3) } else { Color::srgb(0.4, 1.0, 0.5) };
            gizmos.circle(Isometry3d::new(world(at, 0.06), flat), 0.5 * (1.0 - t), color);
        }
    }
    if let Some(d) = controls.hovered.and_then(|i| drawn.units[i]) {
        gizmos.circle(
            Isometry3d::new(world(d.pos, 0.07), flat),
            d.radius * S * 1.4,
            Color::srgb(1.0, 0.85, 0.3),
        );
    }
    if let Some(c) = session.client.as_ref()
        && controls.armed == Some(Armed::AttackMove)
    {
        let u = &c.pred.units[c.seat];
        gizmos.circle(
            Isometry3d::new(world(c.own_pos(now), 0.07), flat),
            (u.stats.range + u.radius) * S,
            Color::srgba(1.0, 0.35, 0.3, 0.8),
        );
    }
}
