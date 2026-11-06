//! A frame-rate bench on a real GPU (`?bench=1`; `docs/ROADMAP.md`'s oldest open item: nobody
//! has measured the game on one). The page starts a practice match (`?practice=1` is implied) and,
//! once it's started and the art is in, runs a script: the camera over the map's places (the art
//! suite's spots), and a fight at the dummies with every ability cast on a loop, close, as the
//! game is played, and wide. Each phase keeps its frame times for [`PHASE_SECS`] and sums them up
//! (frames a second, the median and the 99th-percentile frame, the longest). The results go to
//! `window.__el.bench` (a line a phase, as it runs), to `window.__elBench` as JSON and to the
//! console at the end, and to the menu's Graphics page (Esc).
//!
//! Headless Chromium draws in software and its numbers mean nothing (`CLAUDE.md`); this is for a
//! person's own machine: `scripts/dev.sh` with `--practice`, then `?practice=1&bench=1`, then
//! `&quality=low` for the other tier and `&gfx=webgl2` for the other API.

use bevy::prelude::*;
use el_proto::{Aim, Order, UnitRef};
use el_sim::content::Target;
use el_sim::content::champions::ability;
use el_sim::map::{MAP_W_HALF, MID_Y};
use el_sim::practice;
use wasm_bindgen::JsValue;

use crate::camera::Rig;
use crate::dev_hooks::DevStatus;
use crate::input::Controls;
use crate::net::LaunchConfigRes;
use crate::session::{Phase, Session};
use crate::view::Drawn;

/// How long each phase runs (s), after a moment for the camera to settle.
pub const PHASE_SECS: f32 = 8.0;
const SETTLE_SECS: f32 = 1.0;
/// Seconds between casts in a fight (practice: nothing on cooldown).
const CAST_EVERY: f32 = 0.35;
/// The dummy the fight is at, and the ally beside the player (`el_sim::practice::PLACES`).
const ENEMY: usize = 5;
const ALLY: usize = 1;

/// A phase: where the camera looks, how close, and whether the player fights.
struct Spot {
    name: &'static str,
    at: Vec2,
    zoom: f32,
    fight: bool,
}

fn spots() -> [Spot; 8] {
    let s = |name, x, y, zoom, fight| Spot { name, at: Vec2::new(x, y), zoom, fight };
    [
        s("middle, idle", MAP_W_HALF - 550.0, MID_Y, 1.0, false),
        s("fight", MAP_W_HALF - 550.0, MID_Y - 60.0, 1.0, true),
        s("fight, close", MAP_W_HALF - 550.0, MID_Y - 60.0, 0.7, true),
        s("fight, wide", MAP_W_HALF - 550.0, MID_Y - 60.0, 1.4, true),
        s("dawn's base", 2_300.0, MID_Y, 1.0, false),
        s("the outer turret", 5_200.0, MID_Y, 1.0, false),
        s("the brush", 5_900.0, MID_Y - 400.0, 1.0, false),
        s("dusk's base", 15_360.0 - 2_300.0, MID_Y, 1.25, false),
    ]
}

/// One phase's frames, summed up.
#[derive(Clone, Debug)]
struct Result {
    name: &'static str,
    frames: u32,
    fps: f32,
    p50_ms: f32,
    p99_ms: f32,
    worst_ms: f32,
}

#[derive(Resource, Default)]
pub struct Bench {
    /// The phase running (past the last: done), and when it began (the page's clock, s).
    phase: usize,
    began: f32,
    /// Frames seen before the bench began, so pipelines are compiled and the art is in.
    warm: u32,
    deltas: Vec<f32>,
    results: Vec<Result>,
    next_cast: f32,
    slot: u8,
    done: bool,
}

pub struct BenchPlugin;

impl Plugin for BenchPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().resource::<LaunchConfigRes>().0.bench {
            return;
        }
        app.init_resource::<Bench>()
            .add_systems(Update, run.after(crate::net::NetSet::Receive).before(crate::net::NetSet::Send));
    }
}

/// Frames to draw before the first phase: the pipelines compile and the models come in over the
/// first seconds of a match, and none of that is the frame rate.
const WARM_FRAMES: u32 = 240;

#[allow(clippy::too_many_arguments)]
fn run(
    time: Res<Time<Real>>,
    session: Res<Session>,
    drawn: Res<Drawn>,
    cfg: Res<LaunchConfigRes>,
    mut bench: ResMut<Bench>,
    mut rig: ResMut<Rig>,
    mut controls: ResMut<Controls>,
    mut status: ResMut<DevStatus>,
) {
    if bench.done {
        return;
    }
    let Some(client) = session.client.as_deref() else { return };
    if session.phase != Phase::Playing || !client.started() || !client.pred.practice {
        return;
    }
    let art = status.fields.iter().any(|(k, v)| *k == "art" && *v == crate::dev_hooks::Field::Bool(true));
    if !art || bench.warm < WARM_FRAMES {
        bench.warm += 1;
        status.set("bench", "warming up");
        return;
    }
    let now = time.elapsed_secs();
    let spots = spots();
    if bench.began == 0.0 {
        bench.began = now;
        place(&mut rig, &spots[0]);
    }
    let spot = &spots[bench.phase];
    let since = now - bench.began;
    // The phase's frames, once the camera has settled.
    if since >= SETTLE_SECS {
        bench.deltas.push(time.delta_secs());
    }
    // The fight: every ability in turn at what it's for.
    if spot.fight && now >= bench.next_cast {
        bench.next_cast = now + CAST_EVERY;
        let slot = bench.slot;
        bench.slot = (slot + 1) % 4;
        let unit_ref =
            |i: usize| UnitRef { slot: i as u8, generation: drawn.units[i].map_or(0, |d| d.generation) };
        let enemy_at = drawn.units[ENEMY].map_or(practice::PLACES[ENEMY].0, |d| d.pos);
        let aim = match ability(client.pred.champs[client.seat].def, slot).target {
            Target::Dir | Target::Point => Aim::Point(enemy_at),
            Target::Enemy => Aim::Unit(unit_ref(ENEMY)),
            Target::Ally => Aim::Unit(unit_ref(ALLY)),
            Target::Caster => Aim::None,
        };
        controls.pending.push(Order::Cast { slot, aim });
    }
    if since < SETTLE_SECS + PHASE_SECS {
        status
            .set("bench", format!("{} ({} of {}): {:.0} s", spot.name, bench.phase + 1, spots.len(), since));
        return;
    }
    // The phase is over: sum it up, and on to the next.
    let mut d = std::mem::take(&mut bench.deltas);
    d.sort_unstable_by(|a, b| a.total_cmp(b));
    let n = d.len();
    let at = |q: f32| d.get(((n as f32 - 1.0) * q).round() as usize).copied().unwrap_or(0.0) * 1000.0;
    let total: f32 = d.iter().sum();
    let r = Result {
        name: spot.name,
        frames: n as u32,
        fps: if total > 0.0 { n as f32 / total } else { 0.0 },
        p50_ms: at(0.5),
        p99_ms: at(0.99),
        worst_ms: d.last().copied().unwrap_or(0.0) * 1000.0,
    };
    bench.results.push(r);
    bench.phase += 1;
    bench.began = now;
    if bench.phase < spots.len() {
        place(&mut rig, &spots[bench.phase]);
        status.set("bench", summary(&bench.results));
        return;
    }
    bench.done = true;
    let text = summary(&bench.results);
    status.set("bench", text.clone());
    publish(&cfg.0, &bench.results, &text);
}

fn place(rig: &mut Rig, spot: &Spot) {
    rig.locked = false;
    rig.placed = true;
    rig.focus = spot.at;
    rig.set_zoom(spot.zoom);
}

/// A line a phase: `fight: 61 fps, 16.2 ms p50, 24.0 p99, 41 worst`.
fn summary(results: &[Result]) -> String {
    results
        .iter()
        .map(|r| {
            format!(
                "{}: {:.0} fps, {:.1} ms p50, {:.1} p99, {:.0} worst",
                r.name, r.fps, r.p50_ms, r.p99_ms, r.worst_ms
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `window.__elBench` (the JSON a person copies into `ROADMAP.md`) and the console.
fn publish(cfg: &crate::config::LaunchConfig, results: &[Result], text: &str) {
    let Some(window) = web_sys::window() else { return };
    let phases: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            // Rounded as f64, so the JSON reads 61.3 and not a float's noise.
            let round = |v: f32, places: f64| (f64::from(v) * places).round() / places;
            serde_json::json!({
                "name": r.name, "frames": r.frames, "fps": round(r.fps, 10.0),
                "p50_ms": round(r.p50_ms, 100.0), "p99_ms": round(r.p99_ms, 100.0),
                "worst_ms": round(r.worst_ms, 10.0),
            })
        })
        .collect();
    let size = window
        .document()
        .and_then(|d| d.get_element_by_id("el"))
        .map(|c| (c.client_width(), c.client_height()));
    let out = serde_json::json!({
        "gfx": if crate::look::WEBGPU { "webgpu" } else { "webgl2" },
        "tier": if cfg.low { "low" } else { "high" },
        "renderer": cfg.renderer,
        "canvas": size.map(|(w, h)| vec![w, h]),
        "phase_secs": PHASE_SECS,
        "phases": phases,
    });
    let json = out.to_string();
    let _ = js_sys::Reflect::set(&window, &JsValue::from_str("__elBench"), &JsValue::from_str(&json));
    web_sys::console::log_1(&format!("eloram bench\n{text}\n{json}").into());
}
