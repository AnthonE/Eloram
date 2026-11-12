//! The champions drawn (`docs/CHAMPIONS.md`): each champion's model, fetched once the match (or
//! champion select, for the team's own) deals it, skinned and animated where its unit is drawn;
//! `el_client_core::pose` says what it plays, from what the client knows of it each frame, and
//! `el_client_core::motion` how it turns, leans, recoils and squashes on top (`figure.rs` draws
//! both). Until a model is in, the unit's figure of primitives stands in (`models.rs`).
//!
//! A champion seen to die stays where it fell, playing its death, until it lives again (or for a
//! while, if it's an enemy no one sees come back). A champion hit flashes, flinches and is pushed
//! back from whoever hit it.
//!
//! Its sockets (`figure::Sockets`) are where its effects come from: the dust the player's own
//! champion's running foot kicks up (and the step heard), the arc its weapon leaves as it swings and the
//! light gathering in its hand as it casts (`trails.rs`), where its shots leave (`Muzzles`), a
//! landing's ring and a dash's puff.
//!
//! Each is drawn with a rim of its team's light where it turns from the camera ([`Rim`],
//! `shaders/rim.wgsl`), so its silhouette reads against the stone: Riot inks its champions'
//! outlines; here the outline is lit.

use bevy::animation::AnimationTargetId;
use bevy::camera::visibility::RenderLayers;
use bevy::gltf::Gltf;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::world_serialization::WorldInstanceReady;
use el_client_core::client::{FEED, OWN_FEED, OwnEvent};
use el_client_core::fx::look::Family;
use el_client_core::fx::sound::Sfx;
use el_client_core::motion::{Feel, Motion};
use el_client_core::pose::{Beats, Clip, Poser, Seen};
use el_proto::event::WireEvent;
use el_sim::content::champions::CHAMPIONS;
use el_sim::units::{SEATS, team_of_seat};

use crate::coords::S;
use crate::dev_hooks::DevStatus;
use crate::figure::{self, Kick, Kicks, Nodes, Sockets};
use crate::look::Look;
use crate::net::now_s;
use crate::portrait::PortraitSubject;
use crate::session::Session;
use crate::trails::{ARC_FROM, Arms};
use crate::view::{Drawn, UnitView, ViewSet};

/// How long an enemy's body lies where it fell, if no one sees it come back (s).
const CORPSE: f64 = 12.0;
/// How long a hit's flash lasts (s), and in how many steps it fades (a material changed is a
/// material prepared again).
const FLASH: f64 = 0.16;
const FLASH_STEPS: f32 = 4.0;
/// How much a champion is lit by its own colour: warm, against the night's cold light (the subject
/// warm, the world cool: the oldest way to lift a figure off its ground).
const LIFT: [f32; 3] = [0.34, 0.28, 0.21];
/// How much richer a champion's own colours are than as they were made.
const SATURATION: f32 = 1.35;
/// How tall a champion stands for its radius (a radius of 60 is 1.8 m).
const HEIGHT: f32 = 3.0 * S;
/// The rim: how bright at the very edge, how tight to it, and how much is kept underneath.
const RIM: f32 = 0.55;
const RIM_POWER: f32 = 3.5;
const RIM_BELOW: f32 = 0.35;

pub type RimMat = ExtendedMaterial<StandardMaterial, Rim>;

/// A champion's rim of its team's light (`shaders/rim.wgsl`).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct Rim {
    #[uniform(100)]
    pub params: RimParams,
}

#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct RimParams {
    /// The colour (linear); w: its brightness at the edge.
    pub color: Vec4,
    /// x: the fresnel's power; y: how much is kept on what faces down.
    pub shape: Vec4,
    /// x: its own colours' saturation.
    pub look: Vec4,
}

impl MaterialExtension for Rim {
    fn fragment_shader() -> ShaderRef {
        "embedded://el_client/shaders/rim.wgsl".into()
    }
}

/// Team `team`'s rim.
fn rim(team: u8) -> Rim {
    let [r, g, b] = el_client_core::fx::look::world::TEAM[usize::from(team.min(1))].edge;
    Rim {
        params: RimParams {
            color: Vec4::new(r, g, b, RIM),
            shape: Vec4::new(RIM_POWER, RIM_BELOW, 0.0, 0.0),
            look: Vec4::new(SATURATION, 0.0, 0.0, 0.0),
        },
    }
}

/// How the figures move (`el_client_core::motion::Feel`): the game's, or the anim stage's knobs.
#[derive(Resource, Default, Clone, Copy)]
pub struct FeelRes(pub Feel);

/// Where each seat's shots leave from this frame (its weapon's tip, or its hand), for the
/// launch's flash and a missile's first moments (`fx/burst.rs`, `fx/live.rs`).
#[derive(Resource, Default)]
pub struct Muzzles(pub [Option<Vec3>; SEATS]);

/// The sounds the figures make (the player's own steps), for `audio.rs` to play and let go.
#[derive(Resource, Default)]
pub struct FigureSounds {
    pub list: Vec<(Sfx, Vec2)>,
}

/// A champion's blob of shadow on the low tier, which has no shadows: a soft dark disc under it,
/// so it stands on the stone rather than over it. Made once.
#[derive(Resource)]
struct Blob {
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
}

/// How wide a blob is, of a champion's radius, and how dark at its middle.
const BLOB: f32 = 2.3;
const BLOB_DARK: f32 = 0.55;

/// A champion's model, once it's in: its scene, and its clips in a graph.
pub(crate) struct Art {
    pub(crate) scene: Handle<WorldAsset>,
    graph: Handle<AnimationGraph>,
    nodes: Nodes,
    /// Its bones are in their mask groups (done with the first figure of it).
    masked: bool,
}

#[derive(Resource, Default)]
pub(crate) struct Models {
    asked: [Option<Handle<Gltf>>; CHAMPIONS.len()],
    pub(crate) ready: [Option<Art>; CHAMPIONS.len()],
}

/// A seat's figure.
#[derive(Default)]
struct Seat {
    /// The champion its figure is, and the figure's entity.
    figure: Option<(usize, Entity)>,
    poser: Option<Poser>,
    motion: Motion,
    /// Where it was last drawn, and which way it faced.
    last: Option<(Vec2, f32)>,
    /// When it was seen to fall, while its body lies there.
    fell: Option<f64>,
    flash_at: f64,
    /// Its toes, frame to frame: when one comes down.
    feet: figure::Feet,
    /// Its blob of shadow (the low tier).
    blob: Option<Entity>,
}

#[derive(Resource, Default)]
struct Seats {
    seats: [Seat; SEATS],
    /// The client's events read so far, and its own.
    feed_read: u64,
    own_read: u64,
    /// The world those counts are of (`Session::world`).
    world: u32,
    /// Casts begun on the figures, all told; hits taken; steps; strikes.
    casts: u64,
    hits: u64,
    steps: u64,
    strikes: u64,
}

/// A figure's model, set up once its scene is in: what plays its clips, its own materials (to
/// flash), with each one's glow, and its sockets.
#[derive(Component, Default)]
struct Figure {
    player: Option<Entity>,
    mats: Vec<(Handle<RimMat>, LinearRgba)>,
    flash_step: u8,
    sockets: Sockets,
}

pub struct ChampionsPlugin;

impl Plugin for ChampionsPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/rim.wgsl");
        app.add_plugins(MaterialPlugin::<RimMat>::default())
            .init_resource::<Models>()
            .init_resource::<Seats>()
            .init_resource::<FeelRes>()
            .init_resource::<Muzzles>()
            .init_resource::<FigureSounds>()
            .init_resource::<Kicks>()
            .add_systems(Startup, blob)
            .add_systems(Update, (load, (spawn, animate).chain().after(ViewSet).in_set(figure::FiguresSet)));
    }
}

fn blob(
    mut commands: Commands,
    look: Res<Look>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let tex = figure::disc(&mut images);
    let blob = Blob {
        mesh: meshes.add(Rectangle::new(1.0, 1.0)),
        mat: materials.add(StandardMaterial {
            base_color: Color::srgba(0.0, 0.0, 0.0, BLOB_DARK),
            base_color_texture: Some(tex),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            fog_enabled: false,
            ..default()
        }),
    };
    // One hidden from the start on the tier that draws them, so the warm-up (`warm.rs`, `unseen`)
    // builds its pipeline before a match: the champions' own are made as the match deals them.
    if !look.high {
        commands.spawn((
            Mesh3d(blob.mesh.clone()),
            MeshMaterial3d(blob.mat.clone()),
            Transform::from_xyz(0.0, -50.0, 0.0),
            Visibility::Hidden,
            bevy::light::NotShadowCaster,
        ));
    }
    commands.insert_resource(blob);
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
    // (Every frame: in an array, the seats' champions or none, not a new vector.)
    let mut dealt = [usize::MAX; SEATS];
    match (session.watch.as_deref(), session.client.as_deref(), session.select.as_ref()) {
        (Some(w), _, _) => dealt = std::array::from_fn(|s| usize::from(w.referee.m.champs[s].def)),
        (None, Some(c), _) => dealt = std::array::from_fn(|s| usize::from(c.world.champs[s].def)),
        (None, None, Some(s)) => {
            for (d, &p) in dealt.iter_mut().zip(&s.picks) {
                *d = usize::from(p);
            }
        }
        (None, None, None) => {}
    }
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
        let (graph, nodes) = figure::graph(gltf);
        models.ready[d] = Some(Art { scene, graph: graphs.add(graph), nodes, masked: false });
    }
}

/// Each seat's figure, once its champion's model is in: its scene under the unit, the primitive
/// figure hidden.
#[allow(clippy::too_many_arguments)]
fn spawn(
    mut commands: Commands,
    session: Res<Session>,
    models: Res<Models>,
    feel: Res<FeelRes>,
    look: Res<Look>,
    blob: Option<Res<Blob>>,
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
        // The low tier's blob of shadow, lying under the unit (not the figure, which leans).
        if seat.blob.is_none()
            && !look.high
            && let Some(b) = &blob
        {
            let size = CHAMPIONS[d].radius * S * BLOB;
            let e = commands
                .spawn((
                    Mesh3d(b.mesh.clone()),
                    MeshMaterial3d(b.mat.clone()),
                    Transform::from_xyz(0.0, 0.015, 0.0)
                        .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
                        .with_scale(Vec3::splat(size)),
                    bevy::light::NotShadowCaster,
                ))
                .id();
            commands.entity(root).add_child(e);
            seat.blob = Some(e);
        }
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
        let mut p = Poser::new(d);
        p.set_feel(feel.0);
        seat.poser = Some(p);
        seat.motion.reset();
    }
}

/// A figure's scene is in: the clips' graph on its player (its bones put in their groups, the
/// first time), materials of its own, what it holds made bigger, and its sockets found.
#[allow(clippy::too_many_arguments)]
fn ready(
    event: On<WorldInstanceReady>,
    mut commands: Commands,
    mut models: ResMut<Models>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    seats: Res<Seats>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    meshes: Query<&MeshMaterial3d<StandardMaterial>>,
    materials: Res<Assets<StandardMaterial>>,
    mut rims: ResMut<Assets<RimMat>>,
    mut figures: Query<&mut Figure>,
    session: Res<Session>,
    shapes: Query<(), With<Mesh3d>>,
    names: Query<(&Name, Option<&ChildOf>)>,
    targets: Query<(&Name, &AnimationTargetId)>,
    mut transforms: Query<&mut Transform>,
) {
    let e = event.event_target();
    let Some((seat, d)) =
        seats.seats.iter().enumerate().find_map(|(i, s)| s.figure.filter(|f| f.1 == e).map(|f| (i, f.0)))
    else {
        return;
    };
    let Some(art) = models.ready[d].as_mut() else { return };
    let Ok(mut figure) = figures.get_mut(e) else { return };
    if !art.masked
        && let Some(mut g) = graphs.get_mut(&art.graph)
    {
        figure::mask(&mut g, children.iter_descendants(e).filter_map(|c| targets.get(c).ok()));
        art.masked = true;
    }
    // The player's own champion and each ally are in a portrait too (`portrait.rs`): the own
    // first, then the allies in seat order.
    let view = session.client.as_deref().filter(|_| session.watch.is_none()).and_then(|c| {
        if c.seat == seat {
            Some(0)
        } else if team_of_seat(seat) == c.team {
            Some(1 + (0..seat).filter(|&s| s != c.seat && team_of_seat(s) == c.team).count())
        } else {
            None
        }
    });
    if let Some(v) = view {
        let head = crate::portrait::Head::find(
            children.iter_descendants(e).filter_map(|c| names.get(c).ok().map(|(n, _)| (c, n.as_str()))),
        );
        commands.entity(e).insert(PortraitSubject { view: v, still: false, head });
    }
    figure.sockets = Sockets::find(
        children
            .iter_descendants(e)
            .filter_map(|c| names.get(c).ok().map(|(n, p)| (c, n, p.map(|p| p.parent())))),
        |prop, tip| {
            // The tip's place is in the prop's frame, in the figure's heights.
            let reach = transforms.get(tip).map_or(0.0, |t| t.translation.length());
            if let Ok(mut t) = transforms.get_mut(prop) {
                t.scale *= figure::grown(reach);
            }
        },
    );
    // Whether a node hangs from a hand or a forearm.
    let held = |mut c: Entity| {
        while let Ok((name, parent)) = names.get(c) {
            if matches!(name.as_str(), "LeftHand" | "RightHand" | "LeftForeArm" | "RightForeArm") {
                return true;
            }
            let Some(p) = parent else { break };
            c = p.parent();
        }
        false
    };
    for c in children.iter_descendants(e) {
        // In its portrait too, but for what it holds: a shield raised or a blade at guard stands
        // between the face and the camera (what it wears on its head or its back stays).
        if let Some(v) = view
            && shapes.contains(c)
            && !held(c)
        {
            commands.entity(c).insert(RenderLayers::from_layers(&[0, crate::portrait::layer(v)]));
        }
        if players.contains(c) {
            commands.entity(c).insert(AnimationGraphHandle(art.graph.clone()));
            figure.player = Some(c);
        }
        if let Ok(m) = meshes.get(c)
            && let Some(own) = materials.get(&m.0)
        {
            let mat = rim_material(own, team_of_seat(seat));
            let glow = mat.base.emissive;
            let h = rims.add(mat);
            commands.entity(c).remove::<MeshMaterial3d<StandardMaterial>>().insert(MeshMaterial3d(h.clone()));
            figure.mats.push((h, glow));
        }
    }
}

/// A champion's mesh's own material as it's drawn: lit as the rest of the Causeway is, and a little
/// by its own colour, warmly, so it reads against the night's stone (Riot's skinned meshes are lit
/// by their own emissive as well as the scene), with its team's rim; a hit flashes this one alone
/// (its glow unhit is its emissive). The warm-up (`warm.rs`) makes the same, so the pipeline it
/// builds is the match's.
pub(crate) fn rim_material(own: &StandardMaterial, team: u8) -> RimMat {
    let mut own = own.clone();
    own.double_sided = true;
    own.cull_mode = None;
    if own.base_color_texture.is_some() && own.emissive_texture.is_none() {
        own.emissive_texture = own.base_color_texture.clone();
        own.emissive = LinearRgba::rgb(LIFT[0], LIFT[1], LIFT[2]);
    }
    ExtendedMaterial { base: own, extension: rim(team) }
}

/// A hit's flash on a figure's materials: `step` of [`FLASH_STEPS`] (0: none).
pub fn flash(mats: &[(Handle<RimMat>, LinearRgba)], step: u8, materials: &mut Assets<RimMat>) {
    let k = f32::from(step) / FLASH_STEPS * 0.55;
    for (h, glow) in mats {
        if let Some(mut m) = materials.get_mut(h) {
            m.base.emissive = *glow + LinearRgba::rgb(k, k, k);
        }
    }
}

/// The step of a hit's flash `age` seconds after it.
pub fn flash_step(age: f64) -> u8 {
    let t = (age / FLASH) as f32;
    if (0.0..1.0).contains(&t) { FLASH_STEPS as u8 - (t * FLASH_STEPS) as u8 } else { 0 }
}

/// A hit on a seat's champion: its flash, its flinch (a reel if it took a big share of its
/// health), and its push away from whoever hit it.
fn hit(seats: &mut Seats, drawn: &Drawn, feel: &Feel, target: u8, source: u8, amount: u16, now: f64) {
    let Some(seat) = seats.seats.get_mut(usize::from(target)) else { return };
    seat.flash_at = now;
    seats.hits += 1;
    let Some(t) = drawn.units[usize::from(target)] else { return };
    let share = f32::from(amount) / t.max_hp.max(1.0);
    if let Some(p) = seat.poser.as_mut() {
        p.hit(now, share > feel.heavy);
    }
    // From whoever hit it to it, in the scene's ground plane (the map's +y is the scene's −Z).
    if let Some(s) = drawn.units.get(usize::from(source)).copied().flatten() {
        let d = t.pos - s.pos;
        seat.motion.hit([d.x, -d.y], share, feel);
    }
}

/// What each seat's champion is doing, and its figure drawn doing it.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn animate(
    session: Res<Session>,
    models: Res<Models>,
    drawn: Res<Drawn>,
    feel: Res<FeelRes>,
    mut seats: ResMut<Seats>,
    mut views: Query<(&UnitView, &mut Transform, &mut Visibility), Without<Figure>>,
    mut parts: Query<&mut Visibility, (Without<UnitView>, Without<Figure>)>,
    mut figures: Query<
        (&mut Figure, &mut Transform, &mut Visibility, Option<&mut PortraitSubject>),
        Without<UnitView>,
    >,
    globals: Query<&GlobalTransform>,
    mut players: Query<&mut AnimationPlayer>,
    mut materials: ResMut<Assets<RimMat>>,
    mut arms: ResMut<Arms>,
    mut muzzles: ResMut<Muzzles>,
    mut kicks: ResMut<Kicks>,
    mut sounds: ResMut<FigureSounds>,
    mut dev: ResMut<DevStatus>,
) {
    let now = now_s();
    let watch = session.watch.as_deref();
    let client = session.client.as_deref().filter(|_| watch.is_none());
    muzzles.0 = [None; SEATS];
    if watch.is_none() && client.is_none() {
        return;
    }
    let feel = feel.0;
    let seats = &mut *seats;
    if seats.world != session.world {
        seats.world = session.world;
        seats.feed_read = 0;
        seats.own_read = 0;
    }
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
                        seats.casts += 1;
                    }
                }
                WireEvent::Death { unit, .. } if usize::from(unit) < SEATS => {
                    seats.seats[usize::from(unit)].fell = Some(now);
                }
                WireEvent::Damage { target, source, amount, .. } if usize::from(target) < SEATS => {
                    hit(seats, &drawn, &feel, target, source, amount, now);
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
                seats.casts += 1;
            }
        }
        seats.own_read = c.own_feed_n;
    }
    // A match watched: what it told as it was stepped, already at the tick drawn.
    if let Some(w) = watch {
        let (feed, feed_n) = (&w.told.feed, w.told.feed_n);
        for n in seats.feed_read.max(feed_n.saturating_sub(FEED as u64))..feed_n {
            match feed[(n % FEED as u64) as usize].1 {
                WireEvent::Cast { unit, slot, .. } if usize::from(unit) < SEATS => {
                    if let Some(p) = seats.seats[usize::from(unit)].poser.as_mut() {
                        p.cast(slot, now);
                        seats.casts += 1;
                    }
                }
                WireEvent::Damage { target, source, amount, .. } if usize::from(target) < SEATS => {
                    hit(seats, &drawn, &feel, target, source, amount, now);
                }
                _ => {}
            }
        }
        seats.feed_read = feed_n;
    }
    let outcome = match (watch, client) {
        (Some(w), _) => w.referee.m.outcome,
        (None, Some(c)) => c.world.outcome,
        (None, None) => None,
    };
    let own = client.map(|c| c.seat);
    let mut shown = Vec::new();
    for (view, mut tf, mut vis) in &mut views {
        let i = view.slot;
        if i >= SEATS {
            continue;
        }
        let seat = &mut seats.seats[i];
        let Some((d, entity)) = seat.figure else { continue };
        let Ok((mut figure, mut ftf, mut fvis, subject)) = figures.get_mut(entity) else { continue };
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
        // A portrait keeps the last face it drew while its champion is down (`portrait.rs`).
        if let Some(mut p) = subject
            && p.still != !alive
        {
            p.still = !alive;
        }
        let parts_shown = if lying { Visibility::Hidden } else { Visibility::Inherited };
        for e in [view.ring, view.bar, view.fill, view.shield, view.ticks] {
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
                // Back to its stance when it's next seen.
                *poser = Poser::new(d);
                poser.set_feel(feel);
                seat.motion.reset();
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
        let team = match (watch, client) {
            (Some(w), _) => w.referee.m.units[i].team,
            (None, Some(c)) => c.world.units[i].team,
            (None, None) => 0,
        };
        let mut s = seen.unwrap_or_default();
        s.alive = alive;
        s.won = outcome == Some(team);
        let pose = poser.step(now, &s);
        // Its model faces +Z; a unit's facing turns +X.
        let moved = seat.motion.step(
            now,
            facing + std::f32::consts::FRAC_PI_2,
            s.speed,
            poser.pace(),
            pose.beats,
            &feel,
        );
        ftf.set_if_neq(figure::place(&moved, height));
        if let Some(p) = figure.player
            && let Ok(mut player) = players.get_mut(p)
        {
            figure::play(&mut player, &art.nodes, &pose);
        }
        // A hit: a white flash, fading in steps.
        let step = flash_step(now - seat.flash_at);
        if step != figure.flash_step {
            figure.flash_step = step;
            flash(&figure.mats, step, &mut materials);
        }
        // Its sockets (as drawn last frame: the pose just set shows in the next).
        let at = |e: Option<Entity>| e.and_then(|e| globals.get(e).ok()).map(|g| g.translation());
        let sk = figure.sockets;
        let tip = sk.weapon.and_then(|(_, t)| at(Some(t)));
        let grip = sk.weapon.and_then(|(g, _)| at(Some(g)));
        let hand = at(sk.hands[1]).or(at(sk.hands[0]));
        muzzles.0[i] = tip.or(hand);
        let pal = Family::of(d as u8).palette();
        arms.color[i] = pal.edge;
        if alive && pose.swing && pose.main.clip.melee() {
            if let (Some(g), Some(t)) = (grip, tip) {
                // The outer part of the blade: the arc, not the fist.
                arms.swing[i] = Some((g.lerp(t, ARC_FROM), t));
            } else if let Some(h) = hand {
                arms.swing[i] = Some((h, h));
            }
        }
        if alive
            && let Some(c) =
                pose.charge.filter(|_| matches!(pose.action, el_client_core::pose::Action::Cast(_)))
            && let Some(p) = tip.or(hand)
        {
            arms.glow[i] = Some((p, c));
        }
        // The beats: a landing's ring, a dash's puff, a strike counted.
        let feet = world_feet(&pos);
        if pose.beats.has(Beats::LAND) {
            kicks.list.push(Kick::Land { at: feet, size: height });
        }
        if pose.beats.has(Beats::DASH) {
            let back = Quat::from_rotation_y(moved.yaw) * Vec3::NEG_Z;
            kicks.list.push(Kick::Dash { at: feet, back });
        }
        if pose.beats.has(Beats::STRIKE) {
            seats.strikes += 1;
        }
        // The player's own champion's foot, coming down as it runs, kicks up a little snow and is
        // heard. Only its own: ten champions' steps would take the burst pool's smoke slots from
        // the effects (`fx_bursts_stolen`).
        let running = alive && own == Some(i) && (pose.main.clip == Clip::Run || pose.legs.is_some());
        for p in seat.feet.step(sk.feet.map(at), running, height).into_iter().flatten() {
            kicks.list.push(Kick::Step { at: p });
            seats.steps += 1;
            sounds.list.push((Sfx::Step, pos));
        }
        shown.push(format!("{i}:{}", pose.main.clip.name()));
    }
    // The own champion's stance (`pose::Kit::idle`): what it's drawn doing standing.
    if let Some(seat) = own
        && let Some((d, _)) = seats.seats[seat].figure
    {
        dev.set("champion_stance", el_client_core::pose::kit(d).idle.name());
    }
    dev.set("champion_models", models.ready.iter().filter(|a| a.is_some()).count());
    dev.set("champion_poses", shown.join(","));
    dev.set("champion_casts", seats.casts as f64);
    dev.set("champion_hits", seats.hits as f64);
    dev.set("champion_steps", seats.steps as f64);
    dev.set("champion_strikes", seats.strikes as f64);
}

/// The floor under a map point.
fn world_feet(pos: &Vec2) -> Vec3 {
    crate::coords::world(*pos, 0.02)
}
