//! The match drawn: the Causeway (its floor from the map's own shapes, rock, brush), every unit
//! slot an entity kept from the start (shown while the team sees its unit), pings, relics, and the
//! fog over what the team can't see. Missiles, blasts and every other effect are `fx.rs`'s.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use el_client_core::fog;
use el_proto::event::WireEvent;
use el_proto::quant::pos_from_code;
use el_sim::map::{self, VIS_H, VIS_W};
use el_sim::units::{Kind, MAX_UNITS, SEATS};
use el_sim::{MAP_H, MAP_W};

use crate::coords::{S, world};
use crate::ground::{FogTex, FoggedMat};
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
    /// The champions' figures, a role each (`models`), in `Role`'s order.
    figures: [Handle<Mesh>; 6],
    team: [[Handle<FoggedMat>; 2]; Kind::COUNT],
    champion: [Handle<FoggedMat>; 20],
    /// A body's height off the floor and its scale, by kind (a structure's model stands on its
    /// foot at its own size; its primitive until then, lifted).
    lift: [f32; Kind::COUNT],
    scale: [f32; Kind::COUNT],
    /// What a fallen structure leaves, by kind (its model, cold and dark), once the models are in.
    ruin: [Option<Handle<FoggedMat>>; Kind::COUNT],
    models_in: bool,
    ring: [Handle<StandardMaterial>; 2],
    fill: [Handle<StandardMaterial>; 3],
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
struct Flash {
    born: f64,
    life: f64,
    radius: f32,
    /// The fade step its material shows (a material changed is a material prepared again, so it
    /// changes in steps, not every frame).
    step: u8,
}

/// The steps a flash fades out in.
const FADE_STEPS: f32 = 16.0;

#[derive(Component)]
struct RelicView(usize);

#[derive(Resource)]
struct FogImage {
    handle: Handle<Image>,
    /// Each cell's darkness now (eased toward what the team sees).
    dark: Vec<f32>,
    /// Each pixel's alpha as last uploaded (image rows): the image is only touched, and sent to
    /// the GPU again, when one of them changes.
    alpha: Vec<u8>,
    last: f64,
}

/// A health bar's width (scene units) and height above the ground, by kind.
fn bar_of(kind: Kind) -> (f32, f32) {
    match kind {
        Kind::Champion => (1.5, 2.4),
        k if k.is_minion() => (0.8, 1.1),
        Kind::Turret => (2.2, 5.6),
        Kind::Nexus => (3.0, 6.6),
        Kind::Inhibitor => (2.4, 3.6),
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
pub fn facing_camera() -> Quat {
    let o = crate::camera::offset(1.0).normalize();
    Quat::from_rotation_x(-o.y.atan2(o.z))
}

pub struct ViewPlugin;

/// The view's systems: what's drawn of units is set by their end (`Drawn`).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewSet;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Drawn>().add_systems(Startup, (setup_map, setup_pools)).add_systems(
            Update,
            (structure_models, draw_units, draw_flashes, draw_relics, draw_fog, draw_gizmos)
                .chain()
                .in_set(ViewSet)
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

fn setup_map(mut commands: Commands, fog: Res<FogTex>, mut images: ResMut<Assets<Image>>) {
    // The floor is `ground.rs`'s, the edges `edges.rs`'s, rock and brush `scenery.rs`'s; this
    // picture of the map is the minimap's.
    commands.insert_resource(FloorImage(images.add(floor_image())));
    commands.insert_resource(FogImage {
        handle: fog.0.clone(),
        dark: vec![0.6; VIS_W * VIS_H],
        alpha: vec![u8::MAX; VIS_W * VIS_H],
        last: 0.0,
    });
}

fn setup_pools(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut fogged: ResMut<Assets<FoggedMat>>,
    fog: Res<FogTex>,
) {
    let mut mat = |c: Color, glow: f32| {
        fogged.add(fog.plain(StandardMaterial {
            base_color: c,
            emissive: LinearRgba::from(c) * glow,
            perceptual_roughness: 0.6,
            ..default()
        }))
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
    use el_sim::content::Role;
    let figures = [Role::Marksman, Role::Mage, Role::Tank, Role::Fighter, Role::Assassin, Role::Enchanter]
        .map(|r| meshes.add(crate::models::figure(r)));
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
    // Opaque: drawn batched in the opaque pass, not sorted with the see-through things.
    let opaque = |materials: &mut Assets<StandardMaterial>, c: Color| {
        materials.add(StandardMaterial { base_color: c, unlit: true, ..default() })
    };
    let fill = [
        opaque(&mut materials, Color::srgb(0.35, 0.92, 0.35)),
        opaque(&mut materials, Color::srgb(0.30, 0.66, 1.0)),
        opaque(&mut materials, Color::srgb(0.95, 0.28, 0.24)),
    ];
    let bar_bg = unlit(&mut materials, Color::srgba(0.0, 0.0, 0.0, 0.75));
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
    let lift = std::array::from_fn(|k| match Kind::from_u8(k as u8) {
        Kind::Melee => 0.28,
        Kind::Caster => 0.34,
        Kind::Siege => 0.32,
        Kind::Super => 0.55,
        Kind::Turret => 1.6,
        Kind::Fountain => 1.0,
        Kind::Inhibitor | Kind::Nexus => 0.6,
        Kind::Champion => 0.0,
    });
    commands.insert_resource(Kit {
        bodies,
        figures,
        team,
        champion,
        lift,
        scale: [1.0; Kind::COUNT],
        ruin: std::array::from_fn(|_| None),
        models_in: false,
        ring,
        fill,
    });
}

/// The structures' models (`scenery.rs`), how big each stands (a turret and the nexus by their
/// height, the fountain's dais by its width) and the band of its height its crystal fills (in
/// percent: it glows in the team's colour).
const STRUCTURES: [(Kind, &str, crate::scenery::Size, (u8, u8)); 4] = [
    (Kind::Turret, "turret", crate::scenery::Size::Height(5.0), (57, 70)),
    (Kind::Inhibitor, "inhibitor", crate::scenery::Size::Height(3.6), (50, 100)),
    (Kind::Nexus, "nexus", crate::scenery::Size::Height(6.2), (48, 100)),
    (Kind::Fountain, "fountain", crate::scenery::Size::Radius(1.7), (60, 100)),
];

/// Once the structures' models are in, they take the primitives' place, and every unit is drawn
/// afresh.
fn structure_models(
    mut kit: ResMut<Kit>,
    mut models: ResMut<crate::scenery::Models>,
    fog: Res<FogTex>,
    gltf_mats: Res<Assets<bevy::gltf::GltfMaterial>>,
    mut mats: ResMut<Assets<FoggedMat>>,
    mut views: Query<&mut UnitView>,
) {
    use crate::scenery::Kind as Look;
    if kit.models_in {
        return;
    }
    let mut got = Vec::new();
    for (kind, name, size, crystal) in STRUCTURES {
        let glow = |t: u8, models: &mut crate::scenery::Models, mats: &mut Assets<FoggedMat>| {
            models.get(name, size, Look::Glow(t, crystal.0, crystal.1), &fog, &gltf_mats, mats)
        };
        let (Some(dawn), Some(dusk), Some(ruin)) = (
            glow(0, &mut models, &mut mats),
            glow(1, &mut models, &mut mats),
            models.get(name, size, Look::Ruin, &fog, &gltf_mats, &mut mats),
        ) else {
            return;
        };
        got.push((kind, dawn, dusk, ruin));
    }
    for (kind, (mesh, scale, dawn), (_, _, dusk), (_, _, ruin)) in got {
        let k = kind as usize;
        kit.bodies[k] = mesh;
        kit.team[k] = [dawn, dusk];
        kit.ruin[k] = Some(ruin);
        kit.lift[k] = 0.0;
        kit.scale[k] = scale;
    }
    kit.models_in = true;
    for mut v in &mut views {
        v.shown = None;
    }
}

#[allow(clippy::type_complexity)]
fn draw_units(
    session: Res<Session>,
    kit: Res<Kit>,
    mut drawn: ResMut<Drawn>,
    mut roots: Query<(&mut UnitView, &mut Transform, &mut Visibility)>,
    mut bodies: Query<
        (&mut Transform, &mut Mesh3d, &mut MeshMaterial3d<FoggedMat>, &mut Visibility),
        Without<UnitView>,
    >,
    mut parts: Query<
        (&mut Transform, &mut MeshMaterial3d<StandardMaterial>, &mut Visibility),
        (Without<UnitView>, Without<MeshMaterial3d<FoggedMat>>),
    >,
) {
    let now = now_s();
    // What's drawn: the match being played, as the team sees it, or the one being watched, all
    // of it.
    let watch = session.watch.as_deref();
    let client = session.client.as_deref().filter(|_| watch.is_none());
    if watch.is_none() && client.is_none() {
        for (_, _, mut vis) in &mut roots {
            vis.set_if_neq(Visibility::Hidden);
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
        // Only what changed is written: a component written is a component changed, and changed
        // ones are walked, propagated and extracted again.
        let Some((pos, facing, kind, team, generation, hp)) = unit else {
            drawn.units[i] = None;
            // A structure that fell leaves its ruin (only when the match says it fell: never for
            // what the fog hides).
            let fell = match (watch, client) {
                (Some(w), _) => !w.referee.m.units[i].alive,
                (None, Some(c)) => !c.world.units[i].alive,
                (None, None) => false,
            };
            let ruin = uv
                .shown
                .and_then(|(k, _, _)| kit.ruin[k as usize].clone().filter(|_| k.is_structure() && fell));
            let Some(ruin) = ruin else {
                vis.set_if_neq(Visibility::Hidden);
                continue;
            };
            vis.set_if_neq(Visibility::Visible);
            if let Ok((mut t, _, mut m, _)) = bodies.get_mut(uv.body)
                && m.0 != ruin
            {
                m.0 = ruin;
                // Broken off low.
                t.scale.y *= 0.4;
                uv.shown = uv.shown.map(|(k, tm, d)| (k, tm, d.wrapping_add(128)));
            }
            for e in [uv.ring, uv.bar, uv.fill] {
                if let Ok((_, _, mut v)) = parts.get_mut(e) {
                    v.set_if_neq(Visibility::Hidden);
                }
            }
            continue;
        };
        vis.set_if_neq(Visibility::Visible);
        tf.set_if_neq(Transform { translation: world(pos, 0.0), ..*tf });
        drawn.units[i] = Some(DrawnUnit { pos, radius: radius_of(kind), team, generation });
        let champs = watch.map_or_else(|| client.map(|c| &c.world.champs), |w| Some(&w.referee.m.champs));
        let def = if i < SEATS { champs.map_or(0, |ch| ch[i].def) } else { 0 };
        let (w, h) = bar_of(kind);
        if uv.shown != Some((kind, team, def)) {
            uv.shown = Some((kind, team, def));
            for e in [uv.ring, uv.bar, uv.fill] {
                if let Ok((_, _, mut v)) = parts.get_mut(e) {
                    v.set_if_neq(Visibility::Inherited);
                }
            }
            if let Ok((mut t, mut mesh, mut m, _)) = bodies.get_mut(uv.body) {
                if kind == Kind::Champion {
                    let d = &el_sim::content::champions::CHAMPIONS[usize::from(def) % kit.champion.len()];
                    mesh.0 = kit.figures[d.role as usize].clone();
                    m.0 = kit.champion[usize::from(def) % kit.champion.len()].clone();
                    // The figures are made at a radius of 65: a bigger champion stands bigger.
                    t.scale = Vec3::splat(d.radius / 65.0);
                } else {
                    mesh.0 = kit.bodies[kind as usize].clone();
                    m.0 = kit.team[kind as usize][usize::from(team.min(1))].clone();
                    t.scale = Vec3::splat(kit.scale[kind as usize]);
                }
            }
            if let Ok((mut t, mut m, _)) = parts.get_mut(uv.ring) {
                m.0 = kit.ring[usize::from(team.min(1))].clone();
                t.scale = Vec3::splat(radius_of(kind) * S * 1.25);
            }
            if let Ok((mut t, _, _)) = parts.get_mut(uv.bar) {
                t.translation.y = h;
                t.scale = Vec3::new(w, 1.0, 1.0);
            }
            if let Ok((_, mut m, _)) = parts.get_mut(uv.fill) {
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
        if let Ok((mut t, _, _, _)) = bodies.get_mut(uv.body) {
            let mut n = *t;
            n.translation.y = kit.lift[kind as usize];
            n.rotation = Quat::from_rotation_y(facing);
            t.set_if_neq(n);
        }
        if let Ok((mut t, _, _)) = parts.get_mut(uv.fill) {
            let f = hp.clamp(0.0, 1.0);
            let n = Transform {
                scale: Vec3::new(w * f, 1.0, 1.0),
                translation: Vec3::new(-(1.0 - f) * w * 0.5, h, 0.01),
                ..*t
            };
            t.set_if_neq(n);
        }
    }
}

/// A ping's colour: warnings red, a call to fight orange, the rest the team's own light.
fn ping_color(kind: u8) -> Color {
    use el_proto::order::ping;
    match kind {
        ping::DANGER | ping::BACK => Color::srgb(0.95, 0.30, 0.25),
        ping::ATTACK => Color::srgb(0.98, 0.60, 0.20),
        ping::GONE => Color::srgb(0.98, 0.86, 0.30),
        _ => Color::srgb(0.55, 0.85, 1.0),
    }
}

/// Spawns a ring for a teammate's ping, and fades every ring out.
fn draw_flashes(
    mut commands: Commands,
    mut session: ResMut<Session>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut flashes: Query<(Entity, &mut Flash, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut ring: Local<Option<Handle<Mesh>>>,
) {
    let now = now_s();
    let mesh = ring.get_or_insert_with(|| meshes.add(Annulus::new(0.9, 1.0))).clone();
    let mut new = Vec::new();
    session.read_events(now, |e| {
        // A teammate's ping: a ring that stays a while, coloured by what it says.
        if let WireEvent::Ping { at, kind, .. } = *e {
            new.push((pos_from_code(at), 150.0, ping_color(kind), 2.5));
        }
    });
    for (at, radius, color, life) in new {
        let m = materials.add(StandardMaterial {
            base_color: color.with_alpha(0.9),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(m),
            Transform::from_translation(world(at, 0.08))
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
            Flash { born: now, life, radius: radius * S, step: 0 },
        ));
    }
    for (e, mut f, mut tf, m) in &mut flashes {
        let t = ((now - f.born) / f.life) as f32;
        if t >= 1.0 {
            materials.remove(&m.0);
            commands.entity(e).despawn();
            continue;
        }
        tf.scale = Vec3::splat(f.radius * (0.6 + 0.4 * t));
        let step = (t * FADE_STEPS) as u8;
        if step != f.step
            && let Some(mut mat) = materials.get_mut(&m.0)
        {
            f.step = step;
            mat.base_color.set_alpha(0.9 * (1.0 - f32::from(step) / FADE_STEPS));
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
        vis.set_if_neq(if relic.up { Visibility::Visible } else { Visibility::Hidden });
        if relic.up {
            let bob = (now * 2.0 + r.0 as f64).sin() as f32 * 0.12;
            tf.translation = world(relic.pos, 0.7 + bob);
        }
    }
}

fn draw_fog(
    session: Res<Session>,
    look: Res<crate::look::Look>,
    mut fog_img: ResMut<FogImage>,
    mut images: ResMut<Assets<Image>>,
) {
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
    let fog_img = &mut *fog_img;
    let mut changed = false;
    for y in 0..VIS_H {
        for x in 0..VIS_W {
            let want = if !look.fog || fog::lit(&grid, x, y) { 0.0 } else { 0.58 };
            let k = y * VIS_W + x;
            let d = &mut fog_img.dark[k];
            *d += (want - *d) * ease;
            // Settled is settled: no endless creep in the last bit.
            if (want - *d).abs() < 0.5 / 255.0 {
                *d = want;
            }
            // The image's top row is the map's far edge.
            let a = (*d * 255.0) as u8;
            let px = (VIS_H - 1 - y) * VIS_W + x;
            changed |= fog_img.alpha[px] != a;
            fog_img.alpha[px] = a;
        }
    }
    // Nothing moved: the texture on the GPU is still right (most frames, once the fog settles).
    if !changed {
        return;
    }
    let Some(mut image) = images.get_mut(&fog_img.handle) else { return };
    let Some(data) = image.data.as_mut() else { return };
    for (px, &a) in data.as_chunks_mut::<4>().0.iter_mut().zip(fog_img.alpha.iter()) {
        *px = [a, 0, 0, 255];
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
    if let (Some(c), Some(s)) = (session.client.as_ref(), controls.aiming) {
        indicator(&mut gizmos, c, s, controls.cursor, now);
    }
}

/// Ability `s`'s reach while its key is held: its range around the champion, and its shape where
/// it would land (a skillshot's lane, an area's circle at the cursor kept in range, a dash's path).
fn indicator(gizmos: &mut Gizmos, c: &el_client_core::Client, s: u8, cursor: Option<Vec2>, now: f64) {
    use el_sim::content::{Effect, Target};
    let reach = Color::srgba(0.55, 0.8, 1.0, 0.75);
    let shape = Color::srgba(1.0, 0.85, 0.35, 0.95);
    let me = c.own_pos(now);
    let own = c.pred.units[c.seat].radius;
    let def = el_sim::content::champions::ability(c.pred.champs[c.seat].def, s);
    if def.range > 0.0 {
        ring(gizmos, me, def.range, reach);
    }
    let aim = cursor.unwrap_or(me);
    let dir = (aim - me).normalize_or(Vec2::X);
    let within = if aim.distance(me) > def.range { me + dir * def.range } else { aim };
    match def.effect {
        Effect::Line { width, .. } => {
            let end = me + dir * def.range;
            let side = dir.perp() * (width * 0.5);
            for (a, b) in [(me + side, end + side), (me - side, end - side), (end + side, end - side)] {
                gizmos.line(world(a, 0.08), world(b, 0.08), shape);
            }
        }
        Effect::Area { radius, .. } | Effect::Zone { radius, .. } => {
            ring(gizmos, if def.target == Target::Caster { me } else { within }, radius, shape);
        }
        Effect::Dash { radius, .. } => {
            gizmos.line(world(me, 0.08), world(within, 0.08), shape);
            ring(gizmos, within, radius.max(own), shape);
        }
        Effect::Blink { radius, .. } => ring(gizmos, within, radius.max(own), shape),
        Effect::OnSelf { .. } => ring(gizmos, me, own * 1.6, shape),
        // A spell at a unit: the ring on what's under the cursor says which.
        Effect::Homing { .. } | Effect::Strike { .. } => {}
    }
}

/// A circle on the floor, `r` game units about `at`.
fn ring(gizmos: &mut Gizmos, at: Vec2, r: f32, color: Color) {
    let flat = Quat::from_rotation_arc(Vec3::Z, Vec3::Y);
    gizmos.circle(Isometry3d::new(world(at, 0.08), flat), r * S, color);
}
