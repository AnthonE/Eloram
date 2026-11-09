//! The sound (`docs/FX.md`, "Sound"): which sounds start, where they sit and how loud, decided
//! the same way in the browser and on the desktop; played by Web Audio in the browser
//! (`web.rs`) and by the desktop's own mixing desk on its sound card (`native.rs`).
//!
//! Every effect is its own sound → a low pass (what's off the screen is dulled) → its gain → an
//! equal-power stereo panner (where it is across the screen) → the front of the effects bus (the
//! ladder's top three rungs) or its back (allies' and the world's, which duck under a big moment);
//! then the five buses (effects, voice, interface, ambience, music) → the master → a limiter →
//! the speakers. Which sounds may start, and which give way, is the mixer's
//! (`el_client_core::fx::mix`); which line the announcer says, `fx::voice`'s. The shared sounds
//! load at the start, a family's once a match deals one of its champions, and the announcer's
//! lines and the music whenever their files are there.

use bevy::prelude::*;
use el_client_core::fx::look::Family;
use el_client_core::fx::mix::{self, Admit, Mixer};
use el_client_core::fx::out::Route;
use el_client_core::fx::sound::{Bus, Part, Sfx};
use el_client_core::fx::voice::Music;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(not(target_arch = "wasm32"))]
use native::{Out, Voice};
#[cfg(target_arch = "wasm32")]
use web::{Out, Voice};

use crate::camera::MainCamera;
use crate::coords::world;
use crate::dev_hooks::DevStatus;
use crate::fx::{Cues, FxSet};
use crate::platform::now_s;
use crate::session::{Phase, Session};

/// The volumes: the master's, then each bus's (in `Bus::ALL`'s order), 0 to 1.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct AudioSettings {
    pub master: f32,
    pub buses: [f32; 5],
    pub muted: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self { master: 0.8, buses: [1.0, 1.0, 0.8, 0.7, 0.6], muted: false }
    }
}

/// Where they're kept (`localStorage` in the browser, the settings file on the desktop).
const SETTINGS_KEY: &str = "el.audio";

impl AudioSettings {
    fn load() -> Self {
        let mut s = AudioSettings::default();
        let text = crate::platform::kv_get(SETTINGS_KEY);
        let Some(v) = text.and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { return s };
        let get = |k: &str| v.get(k).and_then(|x| x.as_f64()).map(|x| (x as f32).clamp(0.0, 1.0));
        s.master = get("master").unwrap_or(s.master);
        for (k, b) in Bus::ALL.iter().enumerate() {
            s.buses[k] = get(b.name()).unwrap_or(s.buses[k]);
        }
        s.muted = v.get("muted").and_then(|x| x.as_bool()).unwrap_or(false);
        s
    }

    pub fn save(&self) {
        let mut v = serde_json::json!({ "master": self.master, "muted": self.muted });
        for (k, b) in Bus::ALL.iter().enumerate() {
            v[b.name()] = serde_json::json!(self.buses[k]);
        }
        crate::platform::kv_set(SETTINGS_KEY, &v.to_string());
    }

    /// A bus's volume set by its name (or the master's, by `"master"`).
    pub fn set(&mut self, bus: &str, v: f32) {
        let v = v.clamp(0.0, 1.0);
        if bus == "master" {
            self.master = v;
        } else if let Some(k) = Bus::ALL.iter().position(|b| b.name() == bus) {
            self.buses[k] = v;
        }
    }
}

/// A sound playing: its id (the mixer's), its voice, and when it's over.
struct Playing {
    id: u32,
    voice: Voice,
    ends: f64,
}

/// A bed that loops (the wind, a brazier's fire, the music): its voice, and whether it's placed.
struct Bed {
    sfx: Sfx,
    voice: Voice,
    placed: bool,
}

pub struct Engine {
    out: Out,
    mixer: Mixer,
    playing: Vec<Playing>,
    wind: Option<Bed>,
    fire: Option<Bed>,
    music: Option<(Music, Bed)>,
    voice_until: f64,
    shared: bool,
    families: [bool; Family::COUNT],
    applied: Option<AudioSettings>,
    // What the page did last frame, for the interface's sounds.
    phase: Phase,
    select: Option<(bool, u8, i32)>,
    stats: Stats,
}

#[derive(Default)]
struct Stats {
    played: u32,
    dropped: u32,
    missing: u32,
    peak: usize,
    lines: u32,
    last: &'static str,
    /// The own champion's casts heard: their sounds started.
    own_casts: u32,
}

impl Engine {
    fn new(asset_root: &std::path::Path) -> Option<Engine> {
        #[cfg(target_arch = "wasm32")]
        let out = Out::new(asset_root)?;
        #[cfg(not(target_arch = "wasm32"))]
        let out = Out::new(asset_root);
        Some(Engine {
            out,
            mixer: Mixer::default(),
            playing: Vec::new(),
            wind: None,
            fire: None,
            music: None,
            voice_until: 0.0,
            shared: false,
            families: [false; Family::COUNT],
            applied: None,
            phase: Phase::Connecting,
            select: None,
            stats: Stats::default(),
        })
    }

    /// The shared sounds (all but the families'), the announcer's and the music: once the list of
    /// files is in.
    fn load_shared(&mut self) {
        if self.shared || !self.out.listed() {
            return;
        }
        self.shared = true;
        for &s in Sfx::ALL.iter().skip(Family::COUNT * Part::COUNT) {
            for k in 0..s.def().takes {
                self.out.want(&s.file(k));
            }
        }
    }

    /// Loads family `f`'s sounds (once).
    fn family(&mut self, f: usize) {
        if self.families[f] || !self.out.listed() {
            return;
        }
        self.families[f] = true;
        for p in 0..Part::COUNT {
            let s = Sfx::ALL[f * Part::COUNT + p];
            for k in 0..s.def().takes {
                self.out.want(&s.file(k));
            }
        }
    }

    /// Applies the volumes (the master's and each bus's), smoothly.
    fn apply(&mut self, s: &AudioSettings) {
        if self.applied.as_ref() == Some(s) {
            return;
        }
        self.out.master(if s.muted { 0.0 } else { s.master }, 0.05);
        for (k, v) in s.buses.iter().enumerate() {
            self.out.bus(k, *v, 0.05);
        }
        self.applied = Some(s.clone());
    }

    /// Stops a sound the mixer cut, with a quick fade so it doesn't click.
    fn cut(&mut self, id: u32) {
        if let Some(k) = self.playing.iter().position(|p| p.id == id) {
            let p = self.playing.remove(k);
            self.out.stop(p.voice, 0.015, 0.08);
        }
    }

    /// A bed: started if it isn't playing, its gain and pan eased toward `gain` and `pan`.
    fn bed(&mut self, bed: &mut Option<Bed>, sfx: Sfx, bus: u8, gain: f32, pan: Option<f32>) {
        if bed.as_ref().is_some_and(|b| b.sfx != sfx)
            && let Some(b) = bed.take()
        {
            self.out.stop(b.voice, 0.4, 2.0);
        }
        if bed.is_none() && gain > 0.001 {
            let Some((voice, _)) = self.out.start(&sfx.file(0), Route::Bus(bus), 0.0, pan, 0.0, 0.0, true)
            else {
                return;
            };
            *bed = Some(Bed { sfx, voice, placed: pan.is_some() });
        }
        if let Some(b) = bed {
            self.out.gain(b.voice, gain, 0.5);
            if let (true, Some(p)) = (b.placed, pan) {
                self.out.pan(b.voice, p, 0.3);
            }
        }
    }
}

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        // No match to hear in the transport smoke test.
        if app.world().resource::<crate::look::Look>().art
            && let Some(e) = Engine::new(&crate::app::asset_root())
        {
            app.insert_non_send(e);
        }
        app.insert_resource(AudioSettings::load()).add_systems(Update, play.after(FxSet));
    }
}

/// This frame's sounds: the cues', the announcer's line, the beds, the music.
#[allow(clippy::too_many_arguments)]
fn play(
    engine: Option<NonSendMut<Engine>>,
    mut cues: ResMut<Cues>,
    session: Res<Session>,
    settings: Res<AudioSettings>,
    rig: Res<crate::camera::Rig>,
    cam: Single<(&Camera, &GlobalTransform), With<MainCamera>>,
    mut status: ResMut<DevStatus>,
) {
    let Some(mut e) = engine else { return };
    let e = &mut *e;
    let now = now_s();
    e.out.poll();
    e.load_shared();
    e.apply(&settings);
    let (camera, view) = *cam;
    let ndc = |at: Vec2| camera.world_to_ndc(view, world(at, 1.0)).map_or(Vec2::splat(9.0), |n| n.truncate());
    // A match's families, once it's dealt.
    let champs = match (session.watch.as_deref(), session.client.as_deref()) {
        (Some(w), _) => Some(&w.referee.m.champs),
        (None, Some(c)) => Some(&c.world.champs),
        (None, None) => None,
    };
    if let Some(champs) = champs {
        for c in champs.iter().take(el_sim::units::SEATS) {
            e.family(Family::of(c.def) as usize);
        }
    }
    let t = e.out.time();
    let ended: Vec<Voice> = e.playing.iter().filter(|p| p.ends <= t).map(|p| p.voice).collect();
    for v in ended {
        e.out.forget(v);
    }
    e.playing.retain(|p| p.ends > t);
    // Under a big moment, the lower rungs sit back.
    let duck = mix::db(e.mixer.duck(now));
    e.out.back(duck, 0.06);
    for q in &cues.list {
        let Some(sfx) = q.sound else { continue };
        let d = sfx.def();
        // The announcer's lines, the beds and the music don't come as cues.
        if !matches!(d.bus, Bus::Effects | Bus::Ui) {
            continue;
        }
        // Where it sits: an effect where it happens on the screen; the interface in the middle.
        let place = (d.bus == Bus::Effects).then(|| mix::place(ndc(q.at)));
        if place.is_some_and(|p| p.gain <= 0.0) {
            continue;
        }
        // The mixer says whether it starts, and draws its take; one not loaded yet isn't heard.
        let rung = q.rung.min(mix::RUNGS - 1);
        let secs = e.out.duration(&sfx.file(0)).unwrap_or(0.5);
        let (id, take, cents) = match e.mixer.admit(sfx, rung, now, secs) {
            Admit::Drop => {
                e.stats.dropped += 1;
                continue;
            }
            Admit::Play { id, cut, take, cents } => {
                if let Some(c) = cut {
                    e.cut(c);
                }
                (id, take, cents)
            }
        };
        let file = sfx.file(take);
        let level = mix::db(d.gain_db);
        let started = match place {
            Some(p) => {
                // What matters more is a little louder.
                let weight = 0.55 + 0.05 * f32::from(q.rating.clamp(1, 10));
                let out = if rung <= 2 { Route::Front } else { Route::Back };
                e.out.start(&file, out, level * p.gain * weight, Some(p.pan), p.lowpass, cents, false)
            }
            None => e.out.start(&file, Route::Bus(2), level, None, 0.0, 0.0, false),
        };
        match started {
            Some((voice, secs)) => {
                e.playing.push(Playing { id, voice, ends: t + secs });
                e.stats.played += 1;
                e.stats.last = d.name;
                if q.own && q.what == el_client_core::fx::director::What::Part(Part::Cast) {
                    e.stats.own_casts += 1;
                }
            }
            None => e.stats.missing += 1,
        }
    }
    e.stats.peak = e.stats.peak.max(e.playing.len());
    // The announcer: one line at a time, the music dipping under it.
    let busy = now < e.voice_until;
    if let Some(line) = cues.director().and_then(|d| d.announcer.next(now, busy))
        && let Some((_, secs)) =
            e.out.start(&line.file(0), Route::Bus(1), mix::db(line.def().gain_db), None, 0.0, 0.0, false)
    {
        e.voice_until = now + secs + 0.25;
        e.stats.lines += 1;
    }
    interface(e, &session, now);
    beds(e, &session, &rig, ndc, now);
    status.set("audio_state", e.out.state());
    status.set("audio_format", e.out.format());
    status.set("audio_loaded", e.out.loaded() as u32);
    status.set("audio_failed", e.out.failed());
    status.set("audio_played", e.stats.played);
    status.set("audio_dropped", e.stats.dropped);
    // Commands the output couldn't take (the desktop's ring full, or no device).
    status.set("audio_refused", e.out.dropped);
    status.set("audio_missing", e.stats.missing);
    status.set("audio_peak", e.stats.peak as u32);
    status.set("audio_lines", e.stats.lines);
    status.set("audio_own_casts", e.stats.own_casts);
    status.set("audio_last", e.stats.last);
    status.set("audio_unlocked", e.out.unlocked());
}

/// The game's own sounds: a match found, champion select's last seconds, ready, a reroll.
fn interface(e: &mut Engine, session: &Session, now: f64) {
    let ui = |e: &mut Engine, s: Sfx| {
        if e.out.start(&s.file(0), Route::Bus(2), mix::db(s.def().gain_db), None, 0.0, 0.0, false).is_some() {
            e.stats.played += 1;
            e.stats.last = s.def().name;
        }
    };
    if session.phase == Phase::Selecting && e.phase != Phase::Selecting {
        ui(e, Sfx::UiFound);
    }
    e.phase = session.phase;
    let sel = session.select.as_ref().map(|s| (s.ready, s.rerolls, (s.ends_at - now).ceil() as i32));
    if let (Some((ready, rerolls, left)), Some((was_ready, was_rerolls, was_left))) = (sel, e.select) {
        if ready && !was_ready {
            ui(e, Sfx::UiReady);
        }
        if rerolls < was_rerolls {
            ui(e, Sfx::UiReroll);
        }
        if left != was_left && (1..=5).contains(&left) {
            ui(e, Sfx::UiTick);
        }
    }
    e.select = sel;
}

/// The beds: the wind over a match, a brazier's fire when the camera is near one, the music.
fn beds(e: &mut Engine, session: &Session, rig: &crate::camera::Rig, ndc: impl Fn(Vec2) -> Vec2, now: f64) {
    let on_map =
        matches!(session.phase, Phase::Playing | Phase::Reconnecting | Phase::Watching | Phase::Over);
    let mut wind = e.wind.take();
    e.bed(&mut wind, Sfx::AmbWind, 3, if on_map { 1.0 } else { 0.0 }, None);
    e.wind = wind;
    // The nearest brazier: louder as the camera comes to it, panned where it stands.
    let near = el_client_core::scenery::props()
        .filter(|p| p.prop == el_client_core::scenery::Prop::Brazier)
        .map(|p| (p.at, p.at.distance(rig.focus)))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let (gain, pan) = match near {
        Some((at, d)) if on_map => {
            let g = (1.0 - d / 1_600.0).clamp(0.0, 1.0);
            (g * g * 0.9, mix::place(ndc(at)).pan)
        }
        _ => (0.0, 0.0),
    };
    let mut fire = e.fire.take();
    e.bed(&mut fire, Sfx::AmbFire, 3, gain, Some(pan));
    e.fire = fire;
    // The music, by where the player is.
    let team = session.client.as_deref().map(|c| c.team);
    let want = match session.phase {
        Phase::Selecting => Music::Select,
        Phase::Playing | Phase::Reconnecting | Phase::Watching => Music::Match,
        Phase::Over => match (session.winner, team) {
            (Some(w), _) if w > 1 => Music::Quiet,
            (Some(w), Some(t)) if w == t => Music::Victory,
            (Some(_), Some(_)) => Music::Defeat,
            _ => Music::Match,
        },
        _ => Music::Quiet,
    };
    if e.music.as_ref().map(|(m, _)| *m) != Some(want) {
        if let Some((_, b)) = e.music.take() {
            e.out.stop(b.voice, 0.6, 3.0);
        }
        if let Some(s) = want.sound()
            && let Some((voice, _)) =
                e.out.start(&s.file(0), Route::Bus(4), 0.0, None, 0.0, 0.0, s.def().looped)
        {
            e.music = Some((want, Bed { sfx: s, voice, placed: false }));
        }
    }
    // Under the announcer's line, the music dips.
    if let Some((_, b)) = &e.music {
        let level = mix::db(b.sfx.def().gain_db) * if now < e.voice_until { 0.5 } else { 1.0 };
        let v = b.voice;
        e.out.gain(v, level, 0.4);
    }
}
