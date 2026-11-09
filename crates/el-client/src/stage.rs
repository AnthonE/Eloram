//! The fx stage in the browser (`?fxstage=<name>`; `docs/FX.md`, "The stage"): one effect fired
//! twice at once in front of a dark card and a light card laid on the Causeway's middle, the
//! camera close over them, on a clock that steps 1/60 s a frame and holds where a shooter asks
//! (`window.__fxStep`), saying where it stands in `window.__fx`. `e2e/fxstrip.mjs` shoots a strip
//! of it and `scripts/fx.sh` runs the loop; what it fires and the clock are
//! `el_client_core::fx::stage`'s (the contract is Gates').
//!
//! The query: `fxstage=orrin-q-impact` (`scripts/fx.sh list` names them all), `knobs=size=1.2;...`
//! retunes the recipes without a rebuild (`stage::KNOBS`), `seed=` rolls the particles another way,
//! `side=own|ally|enemy` is whose view it's rated for, `size=` a blast's radius or a hit's damage,
//! and `cards=dark,light` the cards' greys (sRGB, 0 to 1), for tuning the plate.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::{TimeSystems, TimeUpdateStrategy};
use el_client_core::fx::look::Side;
use el_client_core::fx::stage::{self as core, Clock, Knobs, Staged};
use el_sim::MAP_W;
use el_sim::map::MID_Y;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

use crate::camera::{MainCamera, Rig, VIEW};
use crate::coords::world;
use crate::fx::burst::{Bursts, KnobsRes};
use crate::fx::{Cues, FxSet};
use crate::ground::FogTex;
use crate::look::Look;
use crate::net::LaunchConfigRes;
use crate::session::{Phase, Session};

/// The cards' greys, sRGB, so the plate reads about 25 and 185 of 255 through the picture's
/// tonemapping (`fxstrip.mjs` says `plate ok`; `?cards=` tries others).
pub const CARDS: [f32; 2] = [0.095, 0.875];

#[derive(Resource)]
pub struct Stage {
    pub staged: Staged,
    pub clock: Clock,
    pub seed: u32,
    pub side: Side,
    pub size: Option<f32>,
    pub cards: [f32; 2],
    /// Where the camera looks (map units), the screen's right there, and the zoom.
    pub focus: Vec2,
    pub right: Vec2,
    pub zoom: f32,
    /// This frame: fire the effect; clear what's left (a loop round).
    fire: bool,
    clear: bool,
    /// A `?knobs=` that didn't parse.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub bad_knobs: Option<String>,
}

impl Stage {
    /// Where the two effects fire: on the dark card and on the light one.
    pub fn anchors(&self) -> [Vec2; 2] {
        let r = self.staged.radius * 100.0 * 1.1;
        [self.focus - self.right * r, self.focus + self.right * r]
    }
}

/// One of the stage's cards.
#[derive(Component)]
struct Card;

pub struct StagePlugin;

impl Plugin for StagePlugin {
    fn build(&self, app: &mut App) {
        let cfg = &app.world().resource::<LaunchConfigRes>().0;
        let Some(name) = cfg.fxstage.clone() else { return };
        let Some(staged) = core::find(&name) else {
            error!("fxstage: nothing named {name:?} (scripts/fx.sh list)");
            return;
        };
        let (knobs, bad_knobs) = match Knobs::parse(&cfg.fxknobs) {
            Ok(k) => (k, None),
            Err(e) => (Knobs::default(), Some(e)),
        };
        let side = match cfg.fxside.as_str() {
            "ally" => Side::Ally,
            "enemy" => Side::Enemy,
            _ => Side::Own,
        };
        let cards = cfg.fxcards.unwrap_or(CARDS);
        // Each half of the frame 2.2 radii wide: at zoom z the frame is 34.9 z metres across where
        // the camera looks (27 z metres off through a 40° lens, 16:9), and the camera looks at the
        // effect's middle, which a hit draws on a body's near side (`Staged::lift`).
        let (_, out) = staged.lift();
        let zoom = (staged.radius * 4.4 / 34.9 + out / VIEW.distance).clamp(0.08, 1.4);
        let (right, _) = VIEW.axes();
        let span_steps = (staged.span * core::HZ as f32) as u32 + 90;
        app.insert_resource(Stage {
            staged,
            clock: Clock::looping(span_steps),
            seed: cfg.fxseed,
            side,
            size: cfg.fxsize,
            cards,
            focus: Vec2::new(MAP_W * 0.5, MID_Y),
            right,
            zoom,
            fire: false,
            clear: false,
            bad_knobs,
        })
        .insert_resource(KnobsRes(knobs))
        .add_systems(PostStartup, setup.after(crate::look::dress_camera))
        .add_systems(First, clock.before(TimeSystems))
        .add_systems(PostUpdate, hide_world)
        .add_systems(Update, fire.in_set(FxSet).after(crate::fx::direct).before(crate::fx::burst::spawn))
        .add_systems(Last, publish);
    }
}

/// The stage dressed: the cards on the floor, the camera over them, no fog, no snow, no vignette,
/// no sound, nothing of the lobby's.
#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    stage: Res<Stage>,
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
    // Nothing is in the fog: the texture says seen everywhere (a match's client would write it).
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
    rig.focus = stage.focus;
    rig.height = stage.staged.lift().0;
    rig.set_zoom(stage.zoom);
    for mut n in &mut menu_button {
        n.display = Display::None;
    }
    // Two squares side by side on the floor, their seam under the screen's middle, each far wider
    // than its half of the frame: the frame is all card.
    let side_m = 80.0 * stage.zoom;
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let mesh = meshes.add(Rectangle::new(side_m, side_m));
    for (i, grey) in stage.cards.iter().enumerate() {
        let sign = if i == 0 { -1.0 } else { 1.0 };
        let centre = stage.focus + stage.right * (sign * side_m * 0.5 * 100.0);
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mats.add(StandardMaterial {
                base_color: Color::srgb(*grey, *grey, *grey),
                unlit: true,
                ..default()
            })),
            Transform::from_translation(world(centre, 0.02)).with_rotation(VIEW.turn() * flat),
            bevy::light::NotShadowCaster,
            bevy::light::NotShadowReceiver,
            Card,
        ));
    }
}

/// Nothing of the Causeway in the picture: every mesh but the cards and the effects' pools is
/// hidden, every frame (the models come in as they load). The spires in the chasm stood 9 m
/// tall over the cards' far edge otherwise, and the ground's shading cost a software rasteriser
/// most of each frame.
#[allow(clippy::type_complexity)]
fn hide_world(
    mut q: Query<
        &mut Visibility,
        (
            With<Mesh3d>,
            Without<MeshMaterial3d<crate::fx::burst::BurstMat>>,
            Without<crate::fx::live::LiveView>,
            Without<Card>,
        ),
    >,
) {
    for mut v in &mut q {
        v.set_if_neq(Visibility::Hidden);
    }
}

/// The fx stage is the browser's (`scripts/fx.sh` shoots the page): the desktop runs it free.
#[cfg(not(target_arch = "wasm32"))]
fn wanted() -> Option<u32> {
    None
}

/// The step a shooter asks for (`window.__fxStep`), or none: run free.
#[cfg(target_arch = "wasm32")]
fn wanted() -> Option<u32> {
    let window = web_sys::window()?;
    let v = js_sys::Reflect::get(&window, &JsValue::from_str("__fxStep")).ok()?;
    v.as_f64().map(|k| k.max(0.0) as u32)
}

/// The picture's clock, stepped or held by the stage's: before Bevy's time system reads how far
/// to move this frame. It doesn't start until the flipbook sheet is in: a burst fired before
/// would play nothing of it.
fn clock(
    mut stage: ResMut<Stage>,
    mut strategy: ResMut<TimeUpdateStrategy>,
    bursts: Option<Res<Bursts>>,
    server: Res<AssetServer>,
) {
    stage.fire = false;
    stage.clear = false;
    *strategy = TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    if !bursts.is_some_and(|b| b.sheets_loaded(&server)) {
        return;
    }
    let tick = stage.clock.frame(wanted());
    *strategy = TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(f64::from(tick.dt)));
    stage.fire = tick.fire;
    stage.clear = tick.clear;
}

/// The effect fired, twice: on the dark card and on the light one, the particles rolled from the
/// seed each time, so a strip shot again is the same strip. A missile is flown instead: its shot
/// is the live pool's to draw at the stage's own tick while its flight lasts.
fn fire(
    stage: Res<Stage>,
    mut cues: ResMut<Cues>,
    mut bursts: ResMut<Bursts>,
    mut shots: ResMut<crate::fx::live::StageShots>,
) {
    if stage.clear {
        bursts.clear();
    }
    let t = stage.clock.t();
    let flying = stage.clock.step() >= 1 && t <= stage.staged.flight_s();
    let want: Vec<_> = if flying {
        stage.anchors().iter().filter_map(|at| stage.staged.shot(*at, stage.right)).collect()
    } else {
        Vec::new()
    };
    let at = f64::from(t) * f64::from(el_proto::TICK_HZ);
    if shots.shots != want || shots.at != at {
        shots.shots = want;
        shots.at = at;
    }
    if !stage.fire {
        return;
    }
    bursts.reseed(stage.seed);
    for at in stage.anchors() {
        cues.list.push(stage.staged.cue(at, stage.right, stage.side, stage.size));
    }
}

/// `window.__fx`: where the stage stands, for the shooter.
#[cfg(not(target_arch = "wasm32"))]
fn publish(_stage: Res<Stage>, _knobs: Res<KnobsRes>, _bursts: Res<Bursts>, _look: Res<Look>) {}

#[cfg(target_arch = "wasm32")]
fn publish(stage: Res<Stage>, knobs: Res<KnobsRes>, bursts: Res<Bursts>, look: Res<Look>) {
    let Some(window) = web_sys::window() else { return };
    let obj = js_sys::Object::new();
    let set = |k: &str, v: JsValue| {
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), &v);
    };
    set("game", JsValue::from_str("eloram"));
    set("name", JsValue::from_str(&stage.staged.name));
    set("what", JsValue::from_str(&stage.staged.what));
    set("span", JsValue::from_f64(f64::from(stage.staged.span)));
    set("radius", JsValue::from_f64(f64::from(stage.staged.radius)));
    set("ready", JsValue::from_bool(stage.clock.ready()));
    set("step", JsValue::from_f64(f64::from(stage.clock.step())));
    set("held", JsValue::from_f64(f64::from(stage.clock.held())));
    set("t", JsValue::from_f64(f64::from(stage.clock.t())));
    set("seed", JsValue::from_f64(f64::from(stage.seed)));
    set("knobs", JsValue::from_str(&knobs.0.text()));
    set("bloom", JsValue::from_bool(look.high && crate::look::WEBGPU));
    set("tier", JsValue::from_str(if look.high { "high" } else { "low" }));
    set(
        "side",
        JsValue::from_str(match stage.side {
            Side::Own => "own",
            Side::Ally => "ally",
            Side::Enemy => "enemy",
        }),
    );
    // Bursts live, of light and of smoke (the shooter's `live glow/soft`).
    let (glow, smoke) = bursts.live();
    let live = js_sys::Array::new();
    live.push(&JsValue::from_f64(f64::from(glow)));
    live.push(&JsValue::from_f64(f64::from(smoke)));
    set("live", live.into());
    if let Some(e) = &stage.bad_knobs {
        set("error", JsValue::from_str(&format!("knobs: {e}")));
    }
    let _ = js_sys::Reflect::set(&window, &JsValue::from_str("__fx"), &obj);
}
