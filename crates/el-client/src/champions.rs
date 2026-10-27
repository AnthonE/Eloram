//! The champions drawn (`docs/CHAMPIONS.md`): each champion's model, fetched once the match (or
//! champion select, for the team's own) deals it, skinned and animated where its unit is drawn;
//! `el_client_core::pose` says what it plays, from what the client knows of it each frame. Until a
//! model is in, the unit's figure of primitives stands in (`models.rs`).
//!
//! A champion seen to die stays where it fell, playing its death, until it lives again (or for a
//! while, if it's an enemy no one sees come back). A champion hit flashes.

use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use el_client_core::client::{FEED, OWN_FEED, OwnEvent};
use el_client_core::pose::{Clip, Layer, Poser, Seen};
use el_proto::event::WireEvent;
use el_sim::content::champions::CHAMPIONS;
use el_sim::units::SEATS;

use crate::coords::S;
use crate::dev_hooks::DevStatus;
use crate::look::Look;
use crate::net::now_s;
use crate::session::Session;
use crate::view::{Drawn, UnitView, ViewSet};

/// How long an enemy's body lies where it fell, if no one sees it come back (s).
const CORPSE: f64 = 12.0;
/// How long a hit's flash lasts (s), and in how many steps it fades (a material changed is a
/// material prepared again).
const FLASH: f64 = 0.16;
const FLASH_STEPS: f32 = 4.0;
/// How tall a champion stands for its radius (a radius of 60 is 1.8 m).
const HEIGHT: f32 = 3.0 * S;

/// A champion's model, once it's in: its scene, and its clips in a graph.
struct Art {
    scene: Handle<WorldAsset>,
    graph: Handle<AnimationGraph>,
    nodes: [Option<AnimationNodeIndex>; Clip::ALL.len()],
}

#[derive(Resource, Default)]
struct Models {
    asked: [Option<Handle<Gltf>>; CHAMPIONS.len()],
    ready: [Option<Art>; CHAMPIONS.len()],
}

/// A seat's figure.
#[derive(Default)]
struct Seat {
    /// The champion its figure is, and the figure's entity.
    figure: Option<(usize, Entity)>,
    poser: Option<Poser>,
    /// Where it was last drawn, and which way it faced.
    last: Option<(Vec2, f32)>,
    /// When it was seen to fall, while its body lies there.
    fell: Option<f64>,
    flash_at: f64,
}

#[derive(Resource, Default)]
struct Seats {
    seats: [Seat; SEATS],
    /// The client's events read so far, and its own.
    feed_read: u64,
    own_read: u64,
}

/// A figure's model, set up once its scene is in: what plays its clips, and its own materials
/// (to flash), with each one's glow.
#[derive(Component, Default)]
struct Figure {
    player: Option<Entity>,
    mats: Vec<(Handle<StandardMaterial>, LinearRgba)>,
    flash_step: u8,
}

pub struct ChampionsPlugin;

impl Plugin for ChampionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Models>()
            .init_resource::<Seats>()
            .add_systems(Update, (load, (spawn, animate).chain().after(ViewSet)));
    }
}

/// The champions to fetch: the match's ten, or in champion select the team's own (the other
/// team's come with the match).
fn load(
    session: Res<Session>,
    look: Res<Look>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut models: ResMut<Models>,
) {
    if !look.art {
        return;
    }
    let dealt: Vec<usize> =
        match (session.watch.as_deref(), session.client.as_deref(), session.select.as_ref()) {
            (Some(w), _, _) => w.referee.m.champs[..SEATS].iter().map(|c| usize::from(c.def)).collect(),
            (None, Some(c), _) => c.world.champs[..SEATS].iter().map(|c| usize::from(c.def)).collect(),
            (None, None, Some(s)) => s.picks.iter().map(|&p| usize::from(p)).collect(),
            (None, None, None) => Vec::new(),
        };
    for d in dealt.into_iter().filter(|&d| d < CHAMPIONS.len()) {
        if models.asked[d].is_none() {
            let path = format!("models/champions/{}.glb", CHAMPIONS[d].name.to_lowercase());
            models.asked[d] = Some(server.load(path));
        }
    }
    for d in 0..CHAMPIONS.len() {
        if models.ready[d].is_some() {
            continue;
        }
        let Some(gltf) = models.asked[d].as_ref().and_then(|h| gltfs.get(h)) else { continue };
        let Some(scene) = gltf.default_scene.clone().or_else(|| gltf.scenes.first().cloned()) else {
            continue;
        };
        let mut graph = AnimationGraph::new();
        let root = graph.root;
        let nodes = Clip::ALL
            .map(|c| gltf.named_animations.get(c.name()).map(|clip| graph.add_clip(clip.clone(), 1.0, root)));
        models.ready[d] = Some(Art { scene, graph: graphs.add(graph), nodes });
    }
}

/// Each seat's figure, once its champion's model is in: its scene under the unit, the primitive
/// figure hidden.
fn spawn(
    mut commands: Commands,
    session: Res<Session>,
    models: Res<Models>,
    mut seats: ResMut<Seats>,
    views: Query<(Entity, &UnitView)>,
) {
    let champs = match (session.watch.as_deref(), session.client.as_deref()) {
        (Some(w), _) => &w.referee.m.champs,
        (None, Some(c)) => &c.world.champs,
        (None, None) => return,
    };
    for (root, view) in &views {
        let i = view.slot;
        if i >= SEATS {
            continue;
        }
        let d = usize::from(champs[i].def) % CHAMPIONS.len();
        let seat = &mut seats.seats[i];
        if seat.figure.is_some_and(|(had, _)| had == d) {
            continue;
        }
        let Some(art) = &models.ready[d] else { continue };
        if let Some((_, old)) = seat.figure.take() {
            commands.entity(old).despawn();
        }
        let figure = commands
            .spawn((
                WorldAssetRoot(art.scene.clone()),
                Transform::default(),
                Visibility::Hidden,
                Figure::default(),
            ))
            .observe(ready)
            .id();
        commands.entity(root).add_child(figure);
        seat.figure = Some((d, figure));
        seat.poser = Some(Poser::new(d));
    }
}

/// A figure's scene is in: the clips' graph on its player, and materials of its own.
#[allow(clippy::too_many_arguments)]
fn ready(
    event: On<WorldInstanceReady>,
    mut commands: Commands,
    models: Res<Models>,
    seats: Res<Seats>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    meshes: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut figures: Query<&mut Figure>,
) {
    let e = event.event_target();
    let Some(d) = seats.seats.iter().find_map(|s| s.figure.filter(|f| f.1 == e).map(|f| f.0)) else { return };
    let Some(art) = &models.ready[d] else { return };
    let Ok(mut figure) = figures.get_mut(e) else { return };
    for c in children.iter_descendants(e) {
        if players.contains(c) {
            commands.entity(c).insert(AnimationGraphHandle(art.graph.clone()));
            figure.player = Some(c);
        }
        if let Ok(m) = meshes.get(c)
            && let Some(mut own) = materials.get(&m.0).cloned()
        {
            // Lit as the rest of the Causeway is; a hit flashes this one alone.
            own.double_sided = true;
            own.cull_mode = None;
            let glow = own.emissive;
            let h = materials.add(own);
            commands.entity(c).insert(MeshMaterial3d(h.clone()));
            figure.mats.push((h, glow));
        }
    }
}

/// What each seat's champion is doing, and its figure drawn doing it.
#[allow(clippy::too_many_arguments)]
fn animate(
    session: Res<Session>,
    models: Res<Models>,
    drawn: Res<Drawn>,
    mut seats: ResMut<Seats>,
    mut views: Query<(&UnitView, &mut Transform, &mut Visibility), Without<Figure>>,
    mut parts: Query<&mut Visibility, (Without<UnitView>, Without<Figure>)>,
    mut figures: Query<(&mut Figure, &mut Transform, &mut Visibility), Without<UnitView>>,
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
    let seats = &mut *seats;
    // What was told: casts begun, falls, hits; each when the units are drawn there.
    if let Some(c) = client {
        let view = c.clock.view_tick(now);
        let from = seats.feed_read.max(c.feed_n.saturating_sub(FEED as u64));
        let mut n = from;
        while n < c.feed_n {
            let (tick, e) = c.feed[(n % FEED as u64) as usize];
            if f64::from(tick) > view && n + 32 < c.feed_n {
                // Far behind: let it go now rather than hold the rest.
            } else if f64::from(tick) > view {
                break;
            }
            match e {
                WireEvent::Cast { unit, slot, .. }
                    if usize::from(unit) < SEATS && usize::from(unit) != c.seat =>
                {
                    if let Some(p) = seats.seats[usize::from(unit)].poser.as_mut() {
                        p.cast(slot, now);
                    }
                }
                WireEvent::Death { unit, .. } if usize::from(unit) < SEATS => {
                    seats.seats[usize::from(unit)].fell = Some(now);
                }
                WireEvent::Damage { target, .. } if usize::from(target) < SEATS => {
                    seats.seats[usize::from(target)].flash_at = now;
                }
                _ => {}
            }
            n += 1;
        }
        seats.feed_read = n;
        // The own champion's casts, as predicted.
        let from = seats.own_read.max(c.own_feed_n.saturating_sub(OWN_FEED as u64));
        for n in from..c.own_feed_n {
            if let (_, OwnEvent::Cast { slot, .. }) = c.own_feed[(n % OWN_FEED as u64) as usize]
                && let Some(p) = seats.seats[c.seat].poser.as_mut()
            {
                p.cast(slot, now);
            }
        }
        seats.own_read = c.own_feed_n;
    }
    let outcome = match (watch, client) {
        (Some(w), _) => w.referee.m.outcome,
        (None, Some(c)) => c.world.outcome,
        (None, None) => None,
    };
    let mut shown = Vec::new();
    for (view, mut tf, mut vis) in &mut views {
        let i = view.slot;
        if i >= SEATS {
            continue;
        }
        let seat = &mut seats.seats[i];
        let Some((d, entity)) = seat.figure else { continue };
        let Ok((mut figure, mut ftf, mut fvis)) = figures.get_mut(entity) else { continue };
        let Some(art) = &models.ready[d] else { continue };
        // What it's doing, as the client knows it.
        let seen = match (watch, client) {
            (Some(w), _) => {
                let u = &w.referee.m.units[i];
                Some(Seen { alive: u.alive, ..Seen::of(&w.referee.m.quantize(i, u.team)) })
            }
            (None, Some(c)) if i == c.seat => {
                let u = &c.pred.units[i];
                Some(Seen {
                    alive: u.alive,
                    windup_left: Some(f32::from(u.windup) / el_sim::TICK_HZ as f32),
                    ..Seen::of(&c.pred.quantize(i, c.team))
                })
            }
            (None, Some(c)) => c.unit(i, now).map(|s| Seen::of(&s.q)),
            (None, None) => None,
        };
        let here = drawn.units[i];
        let alive = here.is_some() && seen.is_some_and(|s| s.alive);
        if alive {
            seat.last = here.map(|d| (d.pos, d.facing));
            // Living again (a death told just before its unit was gone is no life).
            if seat.fell.is_some_and(|t| now - t > 1.0) {
                seat.fell = None;
            }
        } else if seen.is_some_and(|s| !s.alive) && seat.fell.is_none() && seat.last.is_some() {
            // Its own death, or one watched: it fell where it was last drawn.
            seat.fell = Some(now);
        }
        let lying = !alive && seat.fell.is_some_and(|t| now - t < CORPSE) && seat.last.is_some();
        let parts_shown = if lying { Visibility::Hidden } else { Visibility::Inherited };
        for e in [view.ring, view.bar, view.fill] {
            if let Ok(mut v) = parts.get_mut(e) {
                v.set_if_neq(parts_shown);
            }
        }
        let Some(poser) = seat.poser.as_mut() else { continue };
        let ready = figure.player.is_some();
        if !(alive || lying) || !ready {
            fvis.set_if_neq(Visibility::Hidden);
            if let Ok(mut v) = parts.get_mut(view.body) {
                v.set_if_neq(Visibility::Inherited);
            }
            if !(alive || lying) {
                // Back to idle when it's next seen.
                *poser = Poser::new(d);
            }
            continue;
        }
        let Some((pos, facing)) = (if alive { here.map(|d| (d.pos, d.facing)) } else { seat.last }) else {
            continue;
        };
        if lying {
            // Its unit is hidden with nothing drawn of it: its body is shown, nothing else.
            vis.set_if_neq(Visibility::Visible);
            tf.set_if_neq(Transform { translation: crate::coords::world(pos, 0.0), ..*tf });
        }
        if let Ok(mut v) = parts.get_mut(view.body) {
            v.set_if_neq(Visibility::Hidden);
        }
        fvis.set_if_neq(Visibility::Inherited);
        let height = CHAMPIONS[d].radius * HEIGHT;
        // Its model faces +Z; a unit's facing turns +X.
        ftf.set_if_neq(Transform {
            rotation: Quat::from_rotation_y(facing + std::f32::consts::FRAC_PI_2),
            scale: Vec3::splat(height),
            ..default()
        });
        let team = match (watch, client) {
            (Some(w), _) => w.referee.m.units[i].team,
            (None, Some(c)) => c.world.units[i].team,
            (None, None) => 0,
        };
        let mut s = seen.unwrap_or_default();
        s.alive = alive;
        s.won = outcome == Some(team);
        let pose = poser.step(now, &s);
        if let Some(p) = figure.player
            && let Ok(mut player) = players.get_mut(p)
        {
            play(&mut player, art, pose.main, pose.fading);
        }
        // A hit: a white flash, fading in steps.
        let t = ((now - seat.flash_at) / FLASH) as f32;
        let step = if (0.0..1.0).contains(&t) { FLASH_STEPS as u8 - (t * FLASH_STEPS) as u8 } else { 0 };
        if step != figure.flash_step {
            figure.flash_step = step;
            let k = f32::from(step) / FLASH_STEPS * 0.55;
            for (h, glow) in &figure.mats {
                if let Some(mut m) = materials.get_mut(h) {
                    m.emissive = *glow + LinearRgba::rgb(k, k, k);
                }
            }
        }
        shown.push(format!("{i}:{}", pose.main.clip.name()));
    }
    dev.set("champion_models", models.ready.iter().filter(|a| a.is_some()).count());
    dev.set("champion_poses", shown.join(","));
}

/// Plays `main` (and what's fading) on the player at the times the pose says: the clock is the
/// pose's, so each plays paused at its time.
fn play(player: &mut AnimationPlayer, art: &Art, main: Layer, fading: Option<Layer>) {
    let node = |c: Clip| art.nodes[Clip::ALL.iter().position(|&k| k == c).unwrap_or(0)];
    let want: Vec<(AnimationNodeIndex, Layer)> =
        [Some(main), fading].into_iter().flatten().filter_map(|l| node(l.clip).map(|n| (n, l))).collect();
    let stale: Vec<AnimationNodeIndex> =
        player.playing_animations().map(|(n, _)| *n).filter(|n| want.iter().all(|(w, _)| w != n)).collect();
    for n in stale {
        player.stop(n);
    }
    for (n, l) in want {
        player.play(n).set_weight(l.weight).seek_to(l.time).pause();
    }
}
