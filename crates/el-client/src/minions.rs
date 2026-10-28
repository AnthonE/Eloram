//! The minions drawn (`docs/CHAMPIONS.md`, "The minions"): a model a kind
//! (`web/assets/models/minions/`), fetched once a match is on, hung under each minion slot's unit
//! and posed by `el_client_core::pose` as the champions are: they walk, swing as their windup
//! ends, flinch, are thrown up and fall. The siege cart has no rig: it rocks as it rolls and kicks
//! back when it fires. Until a model is in, the slot's primitive stands in (`view.rs`).
//!
//! One model serves both teams. Each of its materials is made once a team and shared by every
//! minion of that kind and team: Dusk's with its own texture (the blues Meshy painted turned red,
//! `art/tools/champions.mjs`), each a little toward its team's light, and the band of what it holds
//! that is the team's (`team`: a shield's face, a staff's stone, a hammer's head) in that light.
//!
//! A minion seen to die falls where it was, lies a moment and sinks into the floor.

use std::collections::HashMap;

use bevy::gltf::{Gltf, GltfMaterialName};
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use el_client_core::client::FEED;
use el_client_core::pose::{Clip, MINIONS, Poser, Seen};
use el_proto::event::WireEvent;
use el_proto::unit::flags;
use el_sim::content::rules::minion_def;
use el_sim::units::{FIRST_MINION, Kind, MAX_UNITS};

use crate::coords::S;
use crate::dev_hooks::DevStatus;
use crate::look::Look;
use crate::net::now_s;
use crate::session::Session;
use crate::view::{Drawn, UnitView, ViewSet};

/// How tall each kind stands, for its radius (`MINIONS`' order): Riot's minions keep "the low
/// profile of their predecessors", a melee minion two thirds of a champion (a champion's radius of
/// 60 is 180 tall), a caster a little less; a super minion stands over the champions; the cart is
/// as tall as its wheels and bed.
const HEIGHT: [f32; MINIONS.len()] = [2.5 * S, 2.4 * S, 1.5 * S, 3.1 * S];
/// How long a body lies before it sinks, and how long it takes to go under (s).
const LIE: f64 = 1.4;
const SINK: f64 = 0.6;
/// How much a minion is lit by its own colour (a champion is a little more: `champions.rs`).
const LIFT: f32 = 0.14;
/// The cart's kick when it fires (of its height) and how long it takes to settle (s).
const KICK: f32 = 0.09;
const SETTLE: f64 = 0.35;

/// A model, once it's in: its scene, and its clips in a graph.
struct Art {
    scene: Handle<WorldAsset>,
    graph: Handle<AnimationGraph>,
    nodes: [Option<AnimationNodeIndex>; Clip::ALL.len()],
}

#[derive(Resource, Default)]
struct Models {
    asked: [Option<Handle<Gltf>>; MINIONS.len()],
    dusk: [Option<Handle<Image>>; MINIONS.len()],
    ready: [Option<Art>; MINIONS.len()],
    /// A model's material as each team's, by the material its scene came with.
    mats: HashMap<AssetId<StandardMaterial>, [Handle<StandardMaterial>; 2]>,
}

/// A minion slot's figure.
#[derive(Default)]
struct Slot {
    /// Its model (in `MINIONS`) and the figure's entity.
    figure: Option<(usize, Entity)>,
    poser: Option<Poser>,
    /// The life shown (the low byte of the slot's generation), where it was last drawn and which
    /// way it faced, and when.
    life: Option<u8>,
    last: Option<(Vec2, f32)>,
    drawn_at: f64,
    /// When it was seen to fall, while its body lies there.
    fell: Option<f64>,
    /// The cart: whether it was winding up, and when it last fired.
    winding: bool,
    fired: f64,
}

#[derive(Resource)]
struct Slots {
    slots: Vec<Slot>,
    /// The client's events read so far.
    feed_read: u64,
}

impl Default for Slots {
    fn default() -> Self {
        Self { slots: (FIRST_MINION..MAX_UNITS).map(|_| Slot::default()).collect(), feed_read: 0 }
    }
}

/// A figure's model, set up once its scene is in: what plays its clips, and each of its meshes
/// with the material it came with (to be given a team's).
#[derive(Component, Default)]
struct Figure {
    model: usize,
    player: Option<Entity>,
    meshes: Vec<(Entity, AssetId<StandardMaterial>)>,
    /// The team whose materials it wears.
    team: Option<u8>,
}

pub struct MinionsPlugin;

impl Plugin for MinionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Models>()
            .init_resource::<Slots>()
            .add_systems(Update, (load, animate.after(ViewSet)));
    }
}

/// A minion of `kind`'s model, in `MINIONS`.
fn model_of(kind: Kind) -> Option<usize> {
    MINIONS.iter().position(|m| m.0 == kind)
}

/// The models, once there's a match (or champion select before one) to draw them in.
fn load(
    session: Res<Session>,
    look: Res<Look>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut models: ResMut<Models>,
) {
    if !look.art || (session.client.is_none() && session.watch.is_none() && session.select.is_none()) {
        return;
    }
    for (m, &(_, name, _)) in MINIONS.iter().enumerate() {
        if models.asked[m].is_none() {
            models.asked[m] = Some(server.load(format!("models/minions/{name}.glb")));
            models.dusk[m] = Some(server.load(format!("models/minions/{name}.dusk.jpg")));
        }
        if models.ready[m].is_some() {
            continue;
        }
        let Some(gltf) = models.asked[m].as_ref().and_then(|h| gltfs.get(h)) else { continue };
        let Some(scene) = gltf.default_scene.clone().or_else(|| gltf.scenes.first().cloned()) else {
            continue;
        };
        let mut graph = AnimationGraph::new();
        let root = graph.root;
        let nodes = Clip::ALL
            .map(|c| gltf.named_animations.get(c.name()).map(|clip| graph.add_clip(clip.clone(), 1.0, root)));
        models.ready[m] = Some(Art { scene, graph: graphs.add(graph), nodes });
    }
}

/// A figure's scene is in: its clips' graph on its player, and its meshes noted (their team's
/// materials are put on as it's drawn).
fn ready(
    event: On<WorldInstanceReady>,
    mut commands: Commands,
    models: Res<Models>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    meshes: Query<&MeshMaterial3d<StandardMaterial>>,
    mut figures: Query<&mut Figure>,
) {
    let e = event.event_target();
    let Ok(mut figure) = figures.get_mut(e) else { return };
    let Some(art) = &models.ready[figure.model] else { return };
    for c in children.iter_descendants(e) {
        if players.contains(c) {
            commands.entity(c).insert(AnimationGraphHandle(art.graph.clone()));
            figure.player = Some(c);
        }
        if let Ok(m) = meshes.get(c) {
            figure.meshes.push((c, m.0.id()));
        }
    }
    // Not dressed yet.
    figure.team = None;
}

/// A model's material as each team's: Dusk's texture its own, each a little toward its team's
/// light; the team's band in that light.
fn team_mats(
    original: &StandardMaterial,
    team_band: bool,
    dusk: Option<&Handle<Image>>,
    materials: &mut Assets<StandardMaterial>,
) -> [Handle<StandardMaterial>; 2] {
    use el_client_core::fx::look::world::TEAM as LIGHT;
    std::array::from_fn(|t| {
        let mut m = original.clone();
        m.double_sided = true;
        m.cull_mode = None;
        let [r, g, b] = LIGHT[t].edge;
        if team_band {
            // White in the model; its glow (a staff's stone, a hammer's head) the team's light.
            let glows = original.emissive.red > 0.5;
            m.base_color = crate::view::TEAM[t];
            m.emissive = if glows { LinearRgba::rgb(r, g, b) * 1.2 } else { LinearRgba::rgb(r, g, b) * 0.12 };
            return materials.add(m);
        }
        if t == 1
            && m.base_color_texture.is_some()
            && let Some(d) = dusk
        {
            m.base_color_texture = Some(d.clone());
        }
        // A little toward the team's light, as League's minions wear their side's colours.
        let [tr, tg, tb] = [[0.90, 0.96, 1.08], [1.08, 0.94, 0.90]][t];
        let c = m.base_color.to_linear();
        m.base_color = LinearRgba::new(c.red * tr, c.green * tg, c.blue * tb, c.alpha).into();
        if m.base_color_texture.is_some() && m.emissive_texture.is_none() {
            m.emissive_texture = m.base_color_texture.clone();
            m.emissive = LinearRgba::rgb(LIFT, LIFT, LIFT);
        }
        materials.add(m)
    })
}

/// What each minion slot's unit is doing, and its figure drawn doing it.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn animate(
    mut commands: Commands,
    session: Res<Session>,
    mut models: ResMut<Models>,
    drawn: Res<Drawn>,
    mut slots: ResMut<Slots>,
    mut views: Query<(Entity, &UnitView, &mut Transform, &mut Visibility), Without<Figure>>,
    mut parts: Query<&mut Visibility, (Without<UnitView>, Without<Figure>)>,
    mut figures: Query<(&mut Figure, &mut Transform, &mut Visibility), Without<UnitView>>,
    names: Query<&GltfMaterialName>,
    mut players: Query<&mut AnimationPlayer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut dev: ResMut<DevStatus>,
) {
    let now = now_s();
    let watch = session.watch.as_deref();
    let client = session.client.as_deref().filter(|_| watch.is_none());
    if watch.is_none() && client.is_none() {
        return;
    }
    let slots = &mut *slots;
    let models = &mut *models;
    // Deaths told, each when the units are drawn there.
    if let Some(c) = client {
        let view = c.clock.view_tick(now);
        let from = slots.feed_read.max(c.feed_n.saturating_sub(FEED as u64));
        let mut n = from;
        while n < c.feed_n {
            let (tick, e) = c.feed[(n % FEED as u64) as usize];
            if f64::from(tick) > view && n + 32 >= c.feed_n {
                break;
            }
            // Only one drawn a moment ago falls where it was: a death told after its unit was
            // gone from sight a while has nowhere to lie.
            if let WireEvent::Death { unit, .. } = e
                && let Some(s) =
                    usize::from(unit).checked_sub(FIRST_MINION).and_then(|k| slots.slots.get_mut(k))
                && s.last.is_some()
                && now - s.drawn_at < 0.5
            {
                s.fell = Some(now);
            }
            n += 1;
        }
        slots.feed_read = n;
    }
    let mut shown = 0;
    let mut clips: HashMap<&'static str, u32> = HashMap::new();
    for (root, view, mut tf, mut vis) in &mut views {
        let i = view.slot;
        let Some(slot) = i.checked_sub(FIRST_MINION).and_then(|k| slots.slots.get_mut(k)) else { continue };
        // What's there, as the client knows it: its kind, team, life and what it's doing.
        let seen = match (watch, client) {
            (Some(w), _) => {
                let u = &w.referee.m.units[i];
                (u.alive && u.kind.is_minion()).then(|| {
                    let q = w.referee.m.quantize(i, u.team);
                    (u.kind, u.team, q.generation, Seen::of(&q))
                })
            }
            (None, Some(c)) => c
                .unit(i, now)
                .map(|s| (Kind::from_u8(s.q.kind), s.q.team, s.q.generation, Seen::of(&s.q)))
                .filter(|s| s.0.is_minion()),
            (None, None) => None,
        };
        let here = drawn.units[i].filter(|_| seen.is_some());
        if let (Some(d), Some((_, _, life, _))) = (here, seen) {
            // A new life in the slot cuts short whatever lay there.
            if slot.life != Some(life) || slot.fell.is_some_and(|t| now - t > 1.0) {
                slot.fell = None;
                slot.poser = None;
            }
            slot.life = Some(life);
            slot.last = Some((d.pos, d.facing));
            slot.drawn_at = now;
        } else if let Some(w) = watch
            && slot.life.is_some()
            && slot.fell.is_none()
            && !w.referee.m.units[i].alive
        {
            // Watching, every death is seen.
            slot.fell = Some(now);
        }
        let lying = here.is_none() && slot.fell.is_some_and(|t| now - t < LIE + SINK) && slot.last.is_some();
        if here.is_none() && !lying {
            // Gone, or out of sight: nothing's drawn of it until it's seen again.
            slot.life = None;
            slot.fell = None;
            slot.poser = None;
            if let Some((_, f)) = slot.figure
                && let Ok((_, _, mut fvis)) = figures.get_mut(f)
            {
                fvis.set_if_neq(Visibility::Hidden);
            }
            continue;
        }
        // Its kind and team: as it's seen, or as it was when it fell.
        let (kind, team) = match seen {
            Some((k, t, _, _)) => (k, t),
            None => match slot.figure.and_then(|(_, f)| figures.get(f).ok()) {
                Some((fig, _, _)) => (MINIONS[fig.model].0, fig.team.unwrap_or(0)),
                None => continue,
            },
        };
        let Some(m) = model_of(kind) else { continue };
        let Some(art) = &models.ready[m] else { continue };
        if slot.figure.is_none_or(|(had, _)| had != m) {
            if let Some((_, old)) = slot.figure.take() {
                commands.entity(old).despawn();
            }
            let figure = commands
                .spawn((
                    WorldAssetRoot(art.scene.clone()),
                    Transform::default(),
                    Visibility::Hidden,
                    Figure { model: m, ..default() },
                ))
                .observe(ready)
                .id();
            commands.entity(root).add_child(figure);
            slot.figure = Some((m, figure));
            slot.poser = None;
        }
        let Some((_, entity)) = slot.figure else { continue };
        let Ok((mut figure, mut ftf, mut fvis)) = figures.get_mut(entity) else { continue };
        let rigged = MINIONS[m].2.is_some();
        if (rigged && figure.player.is_none()) || figure.meshes.is_empty() {
            // Its scene isn't in yet: the primitive stands in.
            fvis.set_if_neq(Visibility::Hidden);
            continue;
        }
        // Its team's materials, made once a model and team.
        let team = team.min(1);
        if figure.team != Some(team) {
            for &(e, orig) in &figure.meshes {
                let mats = match models.mats.get(&orig) {
                    Some(h) => h.clone(),
                    None => {
                        let Some(o) = materials.get(orig).cloned() else { continue };
                        let band = names.get(e).is_ok_and(|n| &**n == "team");
                        let h = team_mats(&o, band, models.dusk[m].as_ref(), &mut materials);
                        models.mats.insert(orig, h.clone());
                        h
                    }
                };
                commands.entity(e).insert(MeshMaterial3d(mats[usize::from(team)].clone()));
            }
            figure.team = Some(team);
        }
        if let Ok(mut v) = parts.get_mut(view.body) {
            v.set_if_neq(Visibility::Hidden);
        }
        let shown_parts = if lying { Visibility::Hidden } else { Visibility::Inherited };
        for e in [view.ring, view.bar, view.fill] {
            if let Ok(mut v) = parts.get_mut(e) {
                v.set_if_neq(shown_parts);
            }
        }
        let Some((pos, facing)) = (if lying { slot.last } else { here.map(|d| (d.pos, d.facing)) }) else {
            continue;
        };
        if lying {
            // Its unit is gone: its body is drawn where it fell, nothing else.
            vis.set_if_neq(Visibility::Visible);
            tf.set_if_neq(Transform { translation: crate::coords::world(pos, 0.0), ..*tf });
        }
        fvis.set_if_neq(Visibility::Inherited);
        let height = minion_def(kind).radius * HEIGHT[m];
        let s = seen.map_or(Seen::default(), |s| s.3);
        let mut place = Transform {
            rotation: Quat::from_rotation_y(facing + std::f32::consts::FRAC_PI_2),
            scale: Vec3::splat(height),
            ..default()
        };
        if lying {
            // Under the floor once it has lain a while.
            let t = ((now - slot.fell.unwrap_or(now) - LIE) / SINK).clamp(0.0, 1.0) as f32;
            place.translation.y = -t * t * height * 0.8;
        }
        if rigged {
            let poser = slot.poser.get_or_insert_with(|| Poser::minion(kind));
            let pose = poser.step(now, &Seen { alive: !lying, ..s });
            if let Some(p) = figure.player
                && let Ok(mut player) = players.get_mut(p)
            {
                crate::champions::play(&mut player, &art.nodes, pose.main, pose.fading);
            }
            *clips.entry(pose.main.clip.name()).or_default() += 1;
        } else {
            // The cart: it fires as its windup ends (its flag falls), kicking back and settling;
            // rolling, it rocks.
            let winding = s.flags & flags::ATTACKING != 0;
            if slot.winding && !winding && s.speed < el_client_core::pose::MOVING {
                slot.fired = now;
            }
            slot.winding = winding;
            let k = ((now - slot.fired) / SETTLE) as f32;
            let kick = if (0.0..1.0).contains(&k) { (1.0 - k) * (1.0 - k) } else { 0.0 };
            let roll =
                if s.speed > el_client_core::pose::MOVING { (now * 9.0).sin() as f32 * 0.035 } else { 0.0 };
            place.translation += place.rotation * Vec3::new(0.0, 0.0, -kick * KICK * height);
            place.rotation *= Quat::from_rotation_x(-kick * 0.12 + roll);
            let state = if lying {
                "wreck"
            } else if kick > 0.0 {
                "fire"
            } else {
                "roll"
            };
            *clips.entry(state).or_default() += 1;
        }
        ftf.set_if_neq(place);
        shown += 1;
    }
    dev.set("minion_models", models.ready.iter().filter(|a| a.is_some()).count());
    dev.set("minions_drawn", shown);
    let mut by: Vec<_> = clips.into_iter().collect();
    by.sort();
    dev.set("minion_clips", by.iter().map(|(c, n)| format!("{c}:{n}")).collect::<Vec<_>>().join(","));
}
