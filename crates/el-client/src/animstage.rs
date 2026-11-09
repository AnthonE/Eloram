//! The anim stage in the browser (`?animstage=<champion>-<act>`; `docs/CHAMPIONS.md`, "The
//! stage"): one champion's model twice, in profile over the dark card and three-quarters toward the
//! camera over the light one, each posed by a `Poser` and moved by a `Motion` of its own from the
//! act's script (`el_client_core::anim`), on the fx stage's clock: a step a sixtieth of a second,
//! held where a shooter asks (`window.__fxStep`), saying where it stands in `window.__fx`, so
//! `e2e/fxstrip.mjs` shoots a strip of it as it does an effect's (`scripts/anim.sh`). What the
//! figures kick up and draw (dust, a swing's arc, a hand's light) is the game's own, and so are
//! their materials (a hit flashes). `&knobs=snap=0.4;hold_melee=0.1` retunes the motion
//! (`el_client_core::motion::Feel`) without a rebuild.

use std::time::Duration;

use bevy::animation::AnimationTargetId;
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::time::{TimeSystems, TimeUpdateStrategy};
use bevy::world_serialization::WorldInstanceReady;
use el_client_core::anim::{self as core, Staged};
use el_client_core::fx::look::Family;
use el_client_core::fx::stage::Clock;
use el_client_core::motion::{Feel, Motion};
use el_client_core::pose::{Action, Beats, Clip, Poser};
use el_sim::MAP_W;
use el_sim::content::champions::CHAMPIONS;
use el_sim::map::MID_Y;
use wasm_bindgen::JsValue;

use crate::camera::{MainCamera, Rig, VIEW};
use crate::champions::{self, FeelRes, RimMat};
use crate::coords::{S, world};
use crate::figure::{self, Feet, FiguresSet, Kick, Kicks, Nodes, Sockets};
use crate::ground::FogTex;
use crate::look::Look;
use crate::net::LaunchConfigRes;
use crate::session::{Phase, Session};
use crate::stage::CARDS;
use crate::trails::{ARC_FROM, Arms, TrailView};

/// How far each figure stands from the cards' seam (m), the camera's zoom (each half of the frame
/// some 3.3 m across), and the height it looks at (m).
const APART: f32 = 1.6;
const ZOOM: f32 = 0.19;
const LOOK_AT: f32 = 0.85;
/// The poser's clock at the stage's step 1 (s): far from its own start.
const T0: f64 = 100.0;

#[derive(Default)]
struct Fig {
    player: Option<Entity>,
    mats: Vec<(Handle<RimMat>, LinearRgba)>,
    sockets: Sockets,
    poser: Option<Poser>,
    motion: Motion,
    feet: Feet,
    flash_at: f64,
    flash_step: u8,
    /// What it plays now, for `window.__fx`.
    clip: &'static str,
}

#[derive(Resource)]
struct Anim {
    staged: Staged,
    clock: Clock,
    feel: Feel,
    bad_knobs: Option<String>,
    focus: Vec2,
    right: Vec2,
    /// Which way each figure faces at rest (the map's angle): the screen's right; toward the camera
    /// and a little right.
    rest: [f32; 2],
    model: Handle<Gltf>,
    art: Option<(Handle<WorldAsset>, Handle<AnimationGraph>, Nodes)>,
    masked: bool,
    figs: [Fig; 2],
    /// The script's time at the last step, and whether the clock went back to the plate.
    prev: f32,
    clear: bool,
}

impl Anim {
    /// Where each figure stands.
    fn anchors(&self) -> [Vec2; 2] {
        let r = APART / S;
        [self.focus - self.right * r, self.focus + self.right * r]
    }

    fn ready(&self) -> bool {
        self.masked && self.figs.iter().all(|f| f.player.is_some())
    }
}

/// A part of a staged figure: kept in the picture.
#[derive(Component)]
struct Part;

/// A staged figure's root, and which it is.
#[derive(Component)]
struct StageFig(usize);

/// One of the stage's cards.
#[derive(Component)]
struct Card;

pub struct AnimStagePlugin;

impl Plugin for AnimStagePlugin {
    fn build(&self, app: &mut App) {
        let cfg = &app.world().resource::<LaunchConfigRes>().0;
        let Some(name) = cfg.animstage.clone() else { return };
        let Some(staged) = core::find(&name) else {
            web_sys::console::error_1(
                &format!("animstage: nothing named {name:?} (scripts/anim.sh list)").into(),
            );
            return;
        };
        let (feel, bad_knobs) = match Feel::GAME.with(&cfg.fxknobs) {
            Ok(f) => (f, None),
            Err(e) => (Feel::GAME, Some(e)),
        };
        let (right, up) = VIEW.axes();
        let toward = (-up * 0.8 + right * 0.5).normalize();
        let span_steps = (staged.span * 60.0) as u32 + 60;
        app.insert_resource(Anim {
            staged,
            clock: Clock::looping(span_steps),
            feel,
            bad_knobs,
            focus: Vec2::new(MAP_W * 0.5, MID_Y),
            right,
            rest: [right.y.atan2(right.x), toward.y.atan2(toward.x)],
            model: Handle::default(),
            art: None,
            masked: false,
            figs: Default::default(),
            prev: -1.0,
            clear: false,
        })
        .insert_resource(FeelRes(feel))
        .add_systems(PostStartup, setup.after(crate::look::dress_camera))
        .add_systems(First, clock.before(TimeSystems))
        .add_systems(Update, (load, pose.in_set(FiguresSet)).chain())
        .add_systems(PostUpdate, hide_world)
        .add_systems(Last, publish);
    }
}

/// The stage dressed as the fx stage is (`stage.rs`): the cards on the floor, the camera over
/// them, no fog, no snow, no vignette, no sound; and the champion's model asked for.
#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    mut anim: ResMut<Anim>,
    server: Res<AssetServer>,
    mut session: ResMut<Session>,
    mut look: ResMut<Look>,
    mut rig: ResMut<Rig>,
    mut audio: ResMut<crate::audio::AudioSettings>,
    fog: Res<FogTex>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut menu_button: Query<&mut Node, With<crate::menu::TopButton>>,
    cam: Single<Entity, With<MainCamera>>,
) {
    session.phase = Phase::Stage;
    look.fog = false;
    audio.muted = true;
    if let Some(mut img) = images.get_mut(&fog.0)
        && let Some(data) = img.data.as_mut()
    {
        for px in data.as_chunks_mut::<4>().0 {
            px[0] = 0;
        }
    }
    commands.entity(*cam).remove::<bevy::post_process::effect_stack::Vignette>();
    rig.locked = false;
    rig.placed = true;
    rig.focus = anim.focus;
    rig.height = LOOK_AT;
    rig.set_zoom(ZOOM);
    for mut n in &mut menu_button {
        n.display = Display::None;
    }
    let side_m = 80.0 * ZOOM;
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let mesh = meshes.add(Rectangle::new(side_m, side_m));
    for (i, grey) in CARDS.iter().enumerate() {
        let sign = if i == 0 { -1.0 } else { 1.0 };
        let centre = anim.focus + anim.right * (sign * side_m * 0.5 / S);
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mats.add(StandardMaterial {
                base_color: Color::srgb(*grey, *grey, *grey),
                unlit: true,
                ..default()
            })),
            Transform::from_translation(world(centre, 0.0)).with_rotation(VIEW.turn() * flat),
            bevy::light::NotShadowCaster,
            bevy::light::NotShadowReceiver,
            Card,
        ));
    }
    let c = &CHAMPIONS[anim.staged.champ % CHAMPIONS.len()];
    anim.model = server.load(format!("models/champions/{}.glb", c.name.to_lowercase()));
}

/// The model in: its clips' graph, and the two figures spawned.
fn load(
    mut commands: Commands,
    mut anim: ResMut<Anim>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    if anim.art.is_some() {
        return;
    }
    let Some(gltf) = gltfs.get(&anim.model) else { return };
    let Some(scene) = gltf.default_scene.clone().or_else(|| gltf.scenes.first().cloned()) else { return };
    let (graph, nodes) = figure::graph(gltf);
    let graph = graphs.add(graph);
    for (k, at) in anim.anchors().into_iter().enumerate() {
        commands
            .spawn((
                WorldAssetRoot(scene.clone()),
                Transform::from_translation(world(at, 0.0)),
                Visibility::Visible,
                StageFig(k),
            ))
            .observe(ready);
    }
    anim.art = Some((scene, graph, nodes));
}

/// A figure's scene is in: its graph on its player (the bones in their groups, once), its
/// materials as a match draws them, what it holds made bigger, its sockets found.
#[allow(clippy::too_many_arguments)]
fn ready(
    event: On<WorldInstanceReady>,
    mut commands: Commands,
    mut anim: ResMut<Anim>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    figs: Query<&StageFig>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    meshes: Query<&MeshMaterial3d<StandardMaterial>>,
    materials: Res<Assets<StandardMaterial>>,
    mut rims: ResMut<Assets<RimMat>>,
    names: Query<(&Name, Option<&ChildOf>)>,
    targets: Query<(&Name, &AnimationTargetId)>,
    mut transforms: Query<&mut Transform>,
) {
    let e = event.event_target();
    let Ok(&StageFig(k)) = figs.get(e) else { return };
    let Some((_, graph, _)) = anim.art.clone() else { return };
    if !anim.masked
        && let Some(mut g) = graphs.get_mut(&graph)
    {
        figure::mask(&mut g, children.iter_descendants(e).filter_map(|c| targets.get(c).ok()));
        anim.masked = true;
    }
    let sockets = Sockets::find(
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
    let fig = &mut anim.figs[k];
    fig.sockets = sockets;
    for c in children.iter_descendants(e) {
        commands.entity(c).insert(Part);
        if players.contains(c) {
            commands.entity(c).insert(AnimationGraphHandle(graph.clone()));
            fig.player = Some(c);
        }
        if let Ok(m) = meshes.get(c)
            && let Some(own) = materials.get(&m.0).cloned()
        {
            let (mat, glow) = champions::dress(own, 0);
            let h = rims.add(mat);
            commands.entity(c).remove::<MeshMaterial3d<StandardMaterial>>().insert(MeshMaterial3d(h.clone()));
            fig.mats.push((h, glow));
        }
    }
}

/// The step a shooter asks for (`window.__fxStep`), or none: run free.
fn wanted() -> Option<u32> {
    let window = web_sys::window()?;
    let v = js_sys::Reflect::get(&window, &JsValue::from_str("__fxStep")).ok()?;
    v.as_f64().map(|k| k.max(0.0) as u32)
}

/// The picture's clock, stepped or held by the stage's, before Bevy's time reads it. It doesn't
/// start until both figures are in.
fn clock(mut anim: ResMut<Anim>, mut strategy: ResMut<TimeUpdateStrategy>) {
    anim.clear = false;
    *strategy = TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    if !anim.ready() {
        return;
    }
    let tick = anim.clock.frame(wanted());
    *strategy = TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(f64::from(tick.dt)));
    anim.clear = tick.clear;
}

/// Each figure told what its script says now, posed, moved and drawn; what it kicks up and draws
/// handed on as a match's figures hand theirs (`champions.rs`).
#[allow(clippy::too_many_arguments)]
fn pose(
    mut anim: ResMut<Anim>,
    mut roots: Query<(&StageFig, &mut Transform)>,
    globals: Query<&GlobalTransform>,
    mut players: Query<&mut AnimationPlayer>,
    mut rims: ResMut<Assets<RimMat>>,
    mut arms: ResMut<Arms>,
    mut kicks: ResMut<Kicks>,
) {
    let anim = &mut *anim;
    if !anim.ready() {
        return;
    }
    let Some((_, _, nodes)) = anim.art.as_ref() else { return };
    let step = anim.clock.step();
    let t = anim.clock.t();
    if anim.clear || step == 0 {
        for f in &mut anim.figs {
            f.poser = None;
            f.motion.reset();
            f.feet = Feet::default();
            f.flash_at = f64::NEG_INFINITY;
        }
        anim.prev = -1.0;
    }
    // At the plate (step 0) as it stands at the start, with nothing begun.
    let moment = if step == 0 { anim.staged.at(0.0, 0.0) } else { anim.staged.at(anim.prev, t) };
    if step >= 1 {
        anim.prev = t;
    }
    let now = T0 + f64::from(t);
    let champ = anim.staged.champ;
    let height = CHAMPIONS[champ % CHAMPIONS.len()].radius * 3.0 * S;
    let pal = Family::of(champ as u8).palette();
    let anchors = anim.anchors();
    let feel = anim.feel;
    for (k, fig) in anim.figs.iter_mut().enumerate() {
        let poser = fig.poser.get_or_insert_with(|| {
            let mut p = Poser::new(champ);
            p.set_feel(feel);
            p
        });
        if let Some(slot) = moment.cast {
            poser.cast(slot, now);
        }
        let facing = anim.rest[k] + moment.turn;
        if let Some((from, share)) = moment.hit {
            poser.hit(now, share > feel.heavy);
            fig.flash_at = now;
            // Pushed from whoever hit it, `from` round from its front at rest.
            let a = anim.rest[k] + from;
            fig.motion.hit([-a.cos(), a.sin()], share, &feel);
        }
        let pose = poser.step(now, &moment.seen);
        let moved = fig.motion.step(
            now,
            facing + std::f32::consts::FRAC_PI_2,
            moment.seen.speed,
            poser.pace(),
            pose.beats,
            &feel,
        );
        fig.clip = pose.main.clip.name();
        for (root, mut tf) in &mut roots {
            if root.0 == k {
                let place = figure::place(&moved, height);
                tf.set_if_neq(Transform { translation: world(anchors[k], 0.0), ..place });
            }
        }
        if let Some(p) = fig.player
            && let Ok(mut player) = players.get_mut(p)
        {
            figure::play(&mut player, nodes, &pose);
        }
        let flash = champions::flash_step(now - fig.flash_at);
        if flash != fig.flash_step {
            fig.flash_step = flash;
            champions::flash(&fig.mats, flash, &mut rims);
        }
        // Its sockets, as drawn last frame.
        let at = |e: Option<Entity>| e.and_then(|e| globals.get(e).ok()).map(|g| g.translation());
        let sk = fig.sockets;
        let tip = sk.weapon.and_then(|(_, t)| at(Some(t)));
        let grip = sk.weapon.and_then(|(g, _)| at(Some(g)));
        let hand = at(sk.hands[1]).or(at(sk.hands[0]));
        arms.color[k] = pal.edge;
        if pose.swing && pose.main.clip.melee() {
            if let (Some(g), Some(t)) = (grip, tip) {
                arms.swing[k] = Some((g.lerp(t, ARC_FROM), t));
            } else if let Some(h) = hand {
                arms.swing[k] = Some((h, h));
            }
        }
        if let Some(c) = pose.charge.filter(|_| matches!(pose.action, Action::Cast(_)))
            && let Some(p) = tip.or(hand)
        {
            arms.glow[k] = Some((p, c));
        }
        let feet = world(anchors[k], 0.02);
        if pose.beats.has(Beats::LAND) {
            kicks.list.push(Kick::Land { at: feet, size: height });
        }
        if pose.beats.has(Beats::DASH) {
            kicks.list.push(Kick::Dash { at: feet, back: Quat::from_rotation_y(moved.yaw) * Vec3::NEG_Z });
        }
        let running = pose.main.clip == Clip::Run || pose.legs.is_some();
        for p in fig.feet.step(sk.feet.map(at), running, height).into_iter().flatten() {
            kicks.list.push(Kick::Step { at: p });
        }
    }
}

/// Nothing of the Causeway in the picture: every mesh but the cards, the figures, what they draw
/// and the bursts is hidden, every frame.
#[allow(clippy::type_complexity)]
fn hide_world(
    mut q: Query<
        &mut Visibility,
        (
            With<Mesh3d>,
            Without<MeshMaterial3d<crate::fx::burst::BurstMat>>,
            Without<Part>,
            Without<TrailView>,
            Without<Card>,
        ),
    >,
) {
    for mut v in &mut q {
        v.set_if_neq(Visibility::Hidden);
    }
}

/// `window.__fx`: where the stage stands, for the shooter (the fx stage's contract).
fn publish(anim: Res<Anim>, look: Res<Look>) {
    let Some(window) = web_sys::window() else { return };
    let obj = js_sys::Object::new();
    let set = |k: &str, v: JsValue| {
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), &v);
    };
    set("game", JsValue::from_str("eloram"));
    set("stage", JsValue::from_str("anim"));
    set("name", JsValue::from_str(&anim.staged.name));
    set("what", JsValue::from_str(anim.staged.act.what()));
    set("span", JsValue::from_f64(f64::from(anim.staged.span)));
    set("ready", JsValue::from_bool(anim.ready() && anim.clock.ready()));
    set("step", JsValue::from_f64(f64::from(anim.clock.step())));
    set("held", JsValue::from_f64(f64::from(anim.clock.held())));
    set("t", JsValue::from_f64(f64::from(anim.clock.t())));
    set("seed", JsValue::from_f64(0.0));
    let knobs: Vec<String> = Feel::KNOBS
        .iter()
        .filter_map(|k| {
            let v = anim.feel.get(k)?;
            (Feel::GAME.get(k) != Some(v)).then(|| format!("{k}={v}"))
        })
        .collect();
    set("knobs", JsValue::from_str(&knobs.join(";")));
    set("tier", JsValue::from_str(if look.high { "high" } else { "low" }));
    set("clips", JsValue::from_str(&format!("{},{}", anim.figs[0].clip, anim.figs[1].clip)));
    if let Some(e) = &anim.bad_knobs {
        set("error", JsValue::from_str(&format!("knobs: {e}")));
    }
    let _ = js_sys::Reflect::set(&window, &JsValue::from_str("__fx"), &obj);
}
