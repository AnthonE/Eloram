//! What lasts (`shaders/fx_live.wgsl`): missiles' heads and trails (in place of the spheres they
//! were), telegraphs and fields from the zones the team was told of, and the statuses on units
//! from their flags. Pools of quads kept from the start; each frame they're put where their thing
//! is, and only what moved is written (`ROADMAP.md`: a component written is a component changed).
//!
//! Riot's rules hold here: a telegraph is drawn at the hitbox's own radius, from the wire; a
//! missile's trail shows which way it flies; an enemy's telegraph has a hot rim, an ally's a cool
//! one and fainter (it tells allies less); a status looks the same whoever caused it.

use std::collections::HashMap;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use el_client_core::fx::look::{self, Palette, Rgb, world as hue};
use el_proto::unit::flags;
use el_sim::missile::MAX_MISSILES;
use el_sim::units::{Kind, MAX_UNITS, SEATS};
use el_sim::zone::MAX_ZONES;

use crate::coords::{S, world};
use crate::ground::FogTex;
use crate::net::now_s;
use crate::session::Session;
use crate::view::{Drawn, facing_camera};

#[derive(ShaderType, Debug, Clone, Copy, Default, PartialEq)]
pub struct LiveParams {
    pub core: Vec4,
    pub edge: Vec4,
    pub p: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct LiveMat {
    #[uniform(0)]
    pub live: LiveParams,
    #[texture(1)]
    #[sampler(2)]
    pub fog: Handle<Image>,
}

impl Material for LiveMat {
    fn fragment_shader() -> ShaderRef {
        "embedded://el_client/shaders/fx_live.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
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
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// The shapes `fx_live.wgsl` draws.
mod shape {
    pub const HEAD: f32 = 0.0;
    pub const TRAIL: f32 = 1.0;
    pub const TELEGRAPH: f32 = 2.0;
    pub const FIELD: f32 = 3.0;
    pub const ROOT: f32 = 4.0;
    pub const COLUMN: f32 = 5.0;
    pub const STARS: f32 = 6.0;
    pub const SHIELD: f32 = 7.0;
    pub const MARK: f32 = 8.0;
    pub const GLOW: f32 = 9.0;
}

fn v4(c: Rgb, a: f32) -> Vec4 {
    Vec4::new(c[0], c[1], c[2], a)
}

/// A status as drawn: its mesh, its material, how it stands (on the ground, over the head, as tall
/// as the unit...).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mark {
    Stars,
    RootRing,
    RootColumn,
    Silence,
    Reveal,
    Slow,
    Lift,
    Shield,
    Immune,
    Empowered,
    Hasted,
}

impl Mark {
    const ALL: [Mark; 11] = [
        Mark::Stars,
        Mark::RootRing,
        Mark::RootColumn,
        Mark::Silence,
        Mark::Reveal,
        Mark::Slow,
        Mark::Lift,
        Mark::Shield,
        Mark::Immune,
        Mark::Empowered,
        Mark::Hasted,
    ];

    /// The marks a unit's flags show.
    fn of(f: u32, out: &mut Vec<Mark>) {
        let on = |b: u32| f & b != 0;
        if on(flags::STUNNED) {
            out.push(Mark::Stars);
        }
        if on(flags::ROOTED) {
            out.extend([Mark::RootRing, Mark::RootColumn]);
        }
        if on(flags::SILENCED) {
            out.push(Mark::Silence);
        }
        if on(flags::REVEALED) {
            out.push(Mark::Reveal);
        }
        if on(flags::SLOWED) {
            out.push(Mark::Slow);
        }
        if on(flags::AIRBORNE) {
            out.push(Mark::Lift);
        }
        if on(flags::SHIELDED) {
            out.push(Mark::Shield);
        }
        if on(flags::IMMUNE) {
            out.push(Mark::Immune);
        }
        if on(flags::EMPOWERED) {
            out.push(Mark::Empowered);
        }
        if on(flags::HASTED) {
            out.push(Mark::Hasted);
        }
    }

    fn params(self) -> LiveParams {
        let p = |pal: Palette, shape: f32, w: f32, a: f32| LiveParams {
            core: v4(pal.core, a),
            edge: v4(pal.edge, a),
            p: Vec4::new(shape, 0.0, 0.0, w),
        };
        match self {
            Mark::Stars => p(hue::STUN, shape::STARS, 0.0, 1.0),
            Mark::RootRing => p(hue::ROOT, shape::ROOT, 0.0, 1.0),
            Mark::RootColumn => p(hue::ROOT, shape::COLUMN, 0.0, 0.7),
            Mark::Silence => p(hue::SILENCE, shape::MARK, 0.0, 1.0),
            Mark::Reveal => p(hue::SILENCE, shape::MARK, 1.0, 0.8),
            Mark::Slow => p(hue::SLOW, shape::GLOW, 1.0, 0.9),
            Mark::Lift => p(hue::STUN, shape::COLUMN, 0.0, 0.6),
            Mark::Shield => p(hue::SHIELD, shape::SHIELD, 0.0, 0.8),
            Mark::Immune => p(hue::IMMUNE, shape::GLOW, 3.0, 1.0),
            Mark::Empowered => p(hue::EMPOWER, shape::GLOW, 0.0, 0.6),
            Mark::Hasted => p(hue::HASTE, shape::GLOW, 6.0, 0.5),
        }
    }
}

#[derive(Resource)]
pub struct Live {
    heads: Vec<Entity>,
    trails: Vec<Entity>,
    zones: Vec<Entity>,
    /// Each zone's material, and which zone (its generation, its shape) it was last set for.
    zone_mats: Vec<Handle<LiveMat>>,
    zone_for: Vec<Option<(u8, u8)>>,
    marks: Vec<Entity>,
    mark_mats: HashMap<Mark, Handle<LiveMat>>,
    /// A missile's look's material, by its shape and colour.
    shot_mats: HashMap<(u8, [u16; 6]), Handle<LiveMat>>,
    billboard: Handle<Mesh>,
    flat: Handle<Mesh>,
    column: Handle<Mesh>,
    sphere: Handle<Mesh>,
    fog: Handle<Image>,
}

#[derive(Component)]
pub struct LiveView;

pub fn setup(
    mut commands: Commands,
    fog: Res<FogTex>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<LiveMat>>,
) {
    let billboard = meshes.add(Rectangle::new(1.0, 1.0));
    let flat = meshes.add(Rectangle::new(2.0, 2.0));
    let column = meshes.add(Cylinder::new(1.0, 1.0));
    let sphere = meshes.add(Sphere::new(1.0));
    let blank = mats.add(LiveMat { live: LiveParams::default(), fog: fog.0.clone() });
    let mut pool = |n: usize, mesh: &Handle<Mesh>| -> Vec<Entity> {
        (0..n)
            .map(|_| {
                commands
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(blank.clone()),
                        Transform::default(),
                        Visibility::Hidden,
                        NoFrustumCulling,
                        bevy::light::NotShadowCaster,
                        LiveView,
                    ))
                    .id()
            })
            .collect()
    };
    let heads = pool(MAX_MISSILES / 4, &billboard);
    let trails = pool(MAX_MISSILES / 4, &flat);
    let zones = pool(MAX_ZONES, &flat);
    let marks = pool(96, &flat);
    let zone_mats = (0..MAX_ZONES)
        .map(|_| mats.add(LiveMat { live: LiveParams::default(), fog: fog.0.clone() }))
        .collect();
    let mark_mats =
        Mark::ALL.iter().map(|&m| (m, mats.add(LiveMat { live: m.params(), fog: fog.0.clone() }))).collect();
    commands.insert_resource(Live {
        heads,
        trails,
        zones,
        zone_mats,
        zone_for: vec![None; MAX_ZONES],
        marks,
        mark_mats,
        shot_mats: HashMap::new(),
        billboard,
        flat,
        column,
        sphere,
        fog: fog.0.clone(),
    });
}

fn key(c: Rgb, e: Rgb) -> [u16; 6] {
    let q = |v: f32| (v * 64.0).clamp(0.0, 65_535.0) as u16;
    [q(c[0]), q(c[1]), q(c[2]), q(e[0]), q(e[1]), q(e[2])]
}

type Parts<'a> = (&'a mut Transform, &'a mut Visibility, &'a mut Mesh3d, &'a mut MeshMaterial3d<LiveMat>);

/// Shows entity `e` at `tf`, with `mesh` and `mat`, writing only what changed.
fn show(
    q: &mut Query<Parts<'_>, With<LiveView>>,
    e: Entity,
    tf: Transform,
    mesh: &Handle<Mesh>,
    mat: &Handle<LiveMat>,
) {
    let Ok((mut t, mut v, mut m, mut mm)) = q.get_mut(e) else { return };
    v.set_if_neq(Visibility::Visible);
    t.set_if_neq(tf);
    if m.0 != *mesh {
        m.0 = mesh.clone();
    }
    if mm.0 != *mat {
        mm.0 = mat.clone();
    }
}

fn hide(q: &mut Query<Parts<'_>, With<LiveView>>, e: Entity) {
    if let Ok((_, mut v, _, _)) = q.get_mut(e) {
        v.set_if_neq(Visibility::Hidden);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    session: Res<Session>,
    drawn: Res<Drawn>,
    time: Res<Time>,
    mut live: ResMut<Live>,
    mut mats: ResMut<Assets<LiveMat>>,
    mut q: Query<Parts<'_>, With<LiveView>>,
    mut marks: Local<Vec<(Mark, Transform)>>,
    mut last: Local<Vec<(usize, Vec2)>>,
) {
    let live = &mut *live;
    let Some(c) = session.client.as_deref().filter(|_| session.watch.is_none()) else {
        for e in live.heads.iter().chain(&live.trails).chain(&live.zones).chain(&live.marks) {
            hide(&mut q, *e);
        }
        return;
    };
    let now = now_s();
    let clock = time.elapsed_secs_wrapped();
    let face = facing_camera();
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    // Missiles: a head and a trail each.
    let at = c.missile_tick(now);
    let mut k = 0;
    let mut seen_pos = std::mem::take(&mut *last);
    let mut next_pos = Vec::with_capacity(seen_pos.len());
    for (id, s) in c.missiles.live() {
        if k >= live.heads.len() {
            break;
        }
        let target = drawn.units.get(usize::from(s.target)).copied().flatten().map(|d| d.pos);
        let pos = s.pos(at, target);
        let dir = if s.homing {
            let before = seen_pos.iter().find(|(i, _)| *i == id).map(|(_, p)| *p);
            before.map_or(target.map_or(s.dir, |t| (t - pos).normalize_or(s.dir)), |b| {
                (pos - b).normalize_or(s.dir)
            })
        } else {
            s.dir
        };
        next_pos.push((id, pos));
        let owner = usize::from(s.owner);
        let champ = (owner < SEATS).then(|| c.world.champs[owner].def);
        let l = look::look(s.source, champ, s.team);
        let pal = l.palette;
        let ck = key(pal.core, pal.edge);
        let fog = live.fog.clone();
        let mat = |shape: f32, live: &mut Live, mats: &mut Assets<LiveMat>| {
            live.shot_mats
                .entry((shape as u8, ck))
                .or_insert_with(|| {
                    mats.add(LiveMat {
                        live: LiveParams {
                            core: v4(pal.core, 1.0),
                            edge: v4(pal.edge, 1.0),
                            p: Vec4::new(shape, 0.0, 0.0, 0.0),
                        },
                        fog: fog.clone(),
                    })
                })
                .clone()
        };
        let head_m = mat(shape::HEAD, live, &mut mats);
        let trail_m = mat(shape::TRAIL, live, &mut mats);
        let r = if s.homing { 0.16 } else { (s.width * S * 0.5).clamp(0.18, 0.7) };
        let p3 = world(pos, 1.1);
        show(
            &mut q,
            live.heads[k],
            Transform::from_translation(p3).with_rotation(face).with_scale(Vec3::splat(r * 2.6)),
            &live.billboard.clone(),
            &head_m,
        );
        // The trail: behind the head, as long as it's fast (and no longer than it has flown), as
        // wide as its lane.
        let flown = s.flown(at) * S;
        let len = (s.speed * 0.06 * S).clamp(0.3, 2.2).min(flown.max(0.05));
        let width = if s.homing { 0.14 } else { (s.width * S).max(0.14) };
        let d3 = Vec3::new(dir.x, 0.0, -dir.y);
        let mid = p3 - d3 * len * 0.5;
        let turn = Quat::from_rotation_y(dir.y.atan2(dir.x)) * flat;
        // The flat quad is 2 across: half the length and width.
        let tf = Transform::from_translation(mid).with_rotation(turn).with_scale(Vec3::new(
            len * 0.5,
            width * 0.5,
            1.0,
        ));
        show(&mut q, live.trails[k], tf, &live.flat.clone(), &trail_m);
        k += 1;
    }
    *last = next_pos;
    for i in k..live.heads.len() {
        hide(&mut q, live.heads[i]);
        hide(&mut q, live.trails[i]);
    }
    seen_pos.clear();
    // Zones: a telegraph filling toward its landing, or a field's churn and pulses.
    let own_tick = c.clock.own_tick(now);
    let secs = |tick: f64| clock + ((tick - own_tick) / f64::from(el_proto::TICK_HZ)) as f32;
    for i in 0..MAX_ZONES {
        let f = c.zones.fields[i];
        let lands = f.period == 0;
        if !f.alive || (lands && f64::from(f.next) + 2.0 < own_tick) {
            hide(&mut q, live.zones[i]);
            live.zone_for[i] = None;
            continue;
        }
        let owner = usize::from(f.owner);
        let centre = if f.follows {
            if owner == c.seat {
                c.own_pos(now)
            } else {
                drawn.units.get(owner).copied().flatten().map_or(f.at, |d| d.pos)
            }
        } else {
            f.at
        };
        let shape_now = if lands { shape::TELEGRAPH } else { shape::FIELD };
        let id = (f.generation, shape_now as u8);
        if live.zone_for[i] != Some(id) {
            live.zone_for[i] = Some(id);
            let champ = (owner < SEATS).then(|| c.world.champs[owner].def);
            let l = look::look(f.source, champ, f.team);
            let ours = f.team == c.team;
            // An enemy's rim is hot and full; an ally's cool and fainter (Riot: it tells them less).
            let edge = if ours { v4(l.palette.edge, 0.55) } else { Vec4::new(3.4, 0.9, 0.25, 1.0) };
            let core = v4(l.palette.core, if ours { 0.25 } else { 0.45 });
            let p = if lands {
                Vec4::new(shape::TELEGRAPH, secs(f64::from(c.zones.since[i])), secs(f64::from(f.next)), 0.0)
            } else {
                Vec4::new(
                    shape::FIELD,
                    secs(f64::from(f.next)),
                    f.period as f32 / el_proto::TICK_HZ as f32,
                    0.0,
                )
            };
            if let Some(mut m) = mats.get_mut(&live.zone_mats[i]) {
                m.live = LiveParams { core, edge, p };
            }
        }
        let tf = Transform::from_translation(world(centre, 0.06))
            .with_rotation(flat)
            .with_scale(Vec3::splat(f.radius * S));
        show(&mut q, live.zones[i], tf, &live.flat.clone(), &live.zone_mats[i].clone());
    }
    // Statuses: the marks each drawn unit's flags show.
    marks.clear();
    let mut list = Vec::with_capacity(4);
    for (i, d) in drawn.units.iter().enumerate().take(MAX_UNITS) {
        let Some(d) = d else { continue };
        let f = if i == c.seat {
            c.pred.quantize_teams(i)[usize::from(c.team)].flags
        } else {
            match c.unit(i, now) {
                Some(s) => s.q.flags,
                None => continue,
            }
        };
        if f == 0 {
            continue;
        }
        let kind = if i < SEATS {
            Kind::Champion
        } else {
            c.tracks[i].newest().map_or(Kind::Melee, |(_, q)| Kind::from_u8(q.kind))
        };
        if kind.is_structure() {
            continue;
        }
        let r = d.radius * S;
        let head = if kind == Kind::Champion { 2.6 } else { 1.4 };
        list.clear();
        Mark::of(f, &mut list);
        let mut over = 0.0;
        for &m in &list {
            let tf =
                match m {
                    Mark::Stars | Mark::Silence | Mark::Reveal => {
                        let t = Transform::from_translation(world(d.pos, head + over))
                            .with_rotation(face)
                            .with_scale(Vec3::new(1.1, 0.55, 1.0) * if m == Mark::Stars { 1.0 } else { 0.5 });
                        over += 0.45;
                        t
                    }
                    Mark::RootColumn => Transform::from_translation(world(d.pos, 0.45))
                        .with_scale(Vec3::new(r * 1.2, 0.9, r * 1.2)),
                    Mark::Lift => Transform::from_translation(world(d.pos, 0.6)).with_scale(Vec3::new(
                        r * 0.9,
                        1.2,
                        r * 0.9,
                    )),
                    Mark::Shield => Transform::from_translation(world(d.pos, head * 0.45))
                        .with_scale(Vec3::new(r * 1.6, head * 0.55, r * 1.6)),
                    Mark::RootRing | Mark::Slow | Mark::Immune | Mark::Empowered | Mark::Hasted => {
                        Transform::from_translation(world(d.pos, 0.05))
                            .with_rotation(flat)
                            .with_scale(Vec3::splat(r * 1.5))
                    }
                };
            marks.push((m, tf));
        }
    }
    for (k, e) in live.marks.clone().into_iter().enumerate() {
        let Some(&(m, tf)) = marks.get(k) else {
            hide(&mut q, e);
            continue;
        };
        let mesh = match m {
            Mark::Stars | Mark::Silence | Mark::Reveal => live.billboard.clone(),
            Mark::RootColumn | Mark::Lift => live.column.clone(),
            Mark::Shield => live.sphere.clone(),
            Mark::RootRing | Mark::Slow | Mark::Immune | Mark::Empowered | Mark::Hasted => live.flat.clone(),
        };
        let mat = live.mark_mats[&m].clone();
        show(&mut q, e, tf, &mesh, &mat);
    }
}
