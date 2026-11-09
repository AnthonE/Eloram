//! The sound engine (`docs/FX.md`, "Sound"): the browser's Web Audio, straight, so the browser
//! decodes the files (no decoder in the wasm) and schedules each sound to the sample.
//!
//! The graph: every effect is its own source → a low pass (what's off the screen is dulled) → its
//! gain → an equal-power stereo panner (where it is across the screen) → the front of the effects
//! bus (the ladder's top three rungs) or its back (allies' and the world's, which duck under a big
//! moment); then the six buses (effects, the champions' voices, the announcer, interface,
//! ambience, music) → the master → a
//! limiter → the speakers. Which sounds may start, and which give way, is the mixer's
//! (`el_client_core::fx::mix`); which line the announcer says, `fx::voice`'s; which line a
//! champion says, `fx::vo`'s. A champion's lines go to their own bus, placed where it stands as
//! the effects are (the player's own in the middle), one at a time a champion and three
//! champions at most at once, the player's own always heard; the announcer's line is shown on
//! the banner as it's said (`hud_banner.rs`), heard if its file is there.
//!
//! The files are found from `assets/audio/files.json` (written by `scripts/build-web.sh` from what
//! is there): Opus where the browser plays it, MP3 where it doesn't. The shared sounds load with
//! the page, a family's and a champion's lines once a match (or champion select) deals that
//! champion, and the announcer's lines and the music whenever their files are there. A browser
//! starts its audio only after a click or a key: the first one resumes it.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use bevy::prelude::*;
use el_client_core::fx::look::Family;
use el_client_core::fx::mix::{self, Admit, Mixer};
use el_client_core::fx::sound::{Bus, Part, Sfx};
use el_client_core::fx::vo::{self, Line};
use el_client_core::fx::voice::Music;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioBuffer, AudioBufferSourceNode, AudioContext, AudioContextState, BiquadFilterType, GainNode,
};

use crate::camera::MainCamera;
use crate::coords::world;
use crate::dev_hooks::DevStatus;
use crate::fx::{Cues, FxSet};
use crate::hud_banner::Banner;
use crate::net::now_s;
use crate::session::{Phase, Session};

/// The volumes on the page: the master's, then each bus's (in `Bus::ALL`'s order), 0 to 1.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct AudioSettings {
    pub master: f32,
    pub buses: [f32; Bus::ALL.len()],
    pub muted: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        let buses = Bus::ALL.map(|b| match b {
            Bus::Effects | Bus::Champions | Bus::Voice => 1.0,
            Bus::Ui => 0.8,
            Bus::Ambience => 0.7,
            Bus::Music => 0.6,
        });
        Self { master: 0.8, buses, muted: false }
    }
}

/// Where the page keeps them.
const SETTINGS_KEY: &str = "el.audio";

impl AudioSettings {
    fn load() -> Self {
        let text = web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item(SETTINGS_KEY).ok().flatten());
        let mut s = AudioSettings::default();
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
        if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = s.set_item(SETTINGS_KEY, &v.to_string());
        }
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

/// The decoded files, by their stem, and what's on the server.
#[derive(Default)]
struct Store {
    /// The files `files.json` lists (their stems, for the format played), once it's in.
    listed: Option<HashSet<String>>,
    asked: HashSet<String>,
    buffers: HashMap<String, AudioBuffer>,
    failed: u32,
}

/// A sound playing: its id (the mixer's), its source, its gain, and when it's over.
struct Playing {
    id: u32,
    src: AudioBufferSourceNode,
    gain: GainNode,
    ends: f64,
}

/// A champion's line playing: whose, its source and gain, when it's over.
struct Talking {
    seat: u8,
    own: bool,
    src: AudioBufferSourceNode,
    gain: GainNode,
    ends: f64,
}

/// Champions' lines sounding at once, at most (the player's own always gets through).
const VO_VOICES: usize = 3;

/// A bed that loops (the wind, a brazier's fire, the music): its source and gain.
struct Bed {
    sfx: Sfx,
    src: AudioBufferSourceNode,
    gain: GainNode,
    pan: Option<web_sys::StereoPannerNode>,
}

pub struct Engine {
    ctx: AudioContext,
    master: GainNode,
    buses: Vec<GainNode>,
    front: GainNode,
    back: GainNode,
    store: Rc<RefCell<Store>>,
    ext: &'static str,
    mixer: Mixer,
    playing: Vec<Playing>,
    wind: Option<Bed>,
    fire: Option<Bed>,
    music: Option<(Music, Bed)>,
    voice_until: f64,
    talking: Vec<Talking>,
    families: [bool; Family::COUNT],
    champions: [bool; el_sim::content::champions::CHAMPIONS.len()],
    /// The champion select's own champion, and its pick line still to say (while it loads).
    picked: Option<u8>,
    pick_due: Option<(u8, f64)>,
    unlocked: Rc<Cell<bool>>,
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
    /// Champions' lines heard, not loaded (or not there), and cut for room; the last heard.
    vo_lines: u32,
    vo_missing: u32,
    vo_dropped: u32,
    vo_last: String,
}

fn js<T>(r: Result<T, JsValue>) -> Option<T> {
    r.ok()
}

impl Engine {
    fn new() -> Option<Engine> {
        let ctx = js(AudioContext::new())?;
        let gain = |ctx: &AudioContext| js(ctx.create_gain());
        let limiter = js(ctx.create_dynamics_compressor())?;
        // A limiter on everything: fast, hard, just under the top.
        limiter.threshold().set_value(-6.0);
        limiter.knee().set_value(4.0);
        limiter.ratio().set_value(14.0);
        limiter.attack().set_value(0.002);
        limiter.release().set_value(0.12);
        let master = gain(&ctx)?;
        js(master.connect_with_audio_node(&limiter))?;
        js(limiter.connect_with_audio_node(&ctx.destination()))?;
        let mut buses = Vec::new();
        for _ in Bus::ALL {
            let b = gain(&ctx)?;
            js(b.connect_with_audio_node(&master))?;
            buses.push(b);
        }
        let front = gain(&ctx)?;
        let back = gain(&ctx)?;
        js(front.connect_with_audio_node(&buses[Bus::Effects.index()]))?;
        js(back.connect_with_audio_node(&buses[Bus::Effects.index()]))?;
        let ext = if can_play_opus() { "ogg" } else { "mp3" };
        let unlocked = Rc::new(Cell::new(false));
        unlock_on_gesture(&ctx, unlocked.clone());
        let e = Engine {
            ctx,
            master,
            buses,
            front,
            back,
            store: Rc::new(RefCell::new(Store::default())),
            ext,
            mixer: Mixer::default(),
            playing: Vec::new(),
            wind: None,
            fire: None,
            music: None,
            voice_until: 0.0,
            talking: Vec::new(),
            families: [false; Family::COUNT],
            champions: [false; el_sim::content::champions::CHAMPIONS.len()],
            picked: None,
            pick_due: None,
            unlocked,
            applied: None,
            phase: Phase::Connecting,
            select: None,
            stats: Stats::default(),
        };
        e.list();
        Some(e)
    }

    /// Asks the server what files there are; then loads the shared sounds.
    fn list(&self) {
        let store = self.store.clone();
        let ctx = self.ctx.clone();
        let ext = self.ext;
        wasm_bindgen_futures::spawn_local(async move {
            let Some(text) = fetch_text("assets/audio/files.json").await else { return };
            let names: Vec<String> = serde_json::from_str(&text).unwrap_or_default();
            let suffix = format!(".{ext}");
            let listed: HashSet<String> =
                names.iter().filter_map(|n| n.strip_suffix(&suffix)).map(str::to_string).collect();
            store.borrow_mut().listed = Some(listed);
            // The shared sounds (all but the families'), the announcer's and the music.
            for &s in Sfx::ALL.iter().skip(Family::COUNT * Part::COUNT) {
                for k in 0..s.def().takes {
                    load(&store, &ctx, s.file(k), ext);
                }
            }
        });
    }

    /// Loads family `f`'s sounds (once).
    fn family(&mut self, f: usize) {
        if self.families[f] || self.store.borrow().listed.is_none() {
            return;
        }
        self.families[f] = true;
        for p in 0..Part::COUNT {
            let s = Sfx::ALL[f * Part::COUNT + p];
            for k in 0..s.def().takes {
                load(&self.store, &self.ctx, s.file(k), self.ext);
            }
        }
    }

    /// Loads champion `c`'s lines (once).
    fn champion(&mut self, c: u8) {
        let k = usize::from(c);
        if k >= self.champions.len() || self.champions[k] || self.store.borrow().listed.is_none() {
            return;
        }
        self.champions[k] = true;
        for line in Line::ALL {
            for t in 0..vo::script(c).takes(line).len() as u8 {
                load(&self.store, &self.ctx, vo::file(c, line, t), self.ext);
            }
        }
    }

    fn buffer(&self, stem: &str) -> Option<AudioBuffer> {
        self.store.borrow().buffers.get(stem).cloned()
    }

    /// Applies the page's volumes (the master's and each bus's), smoothly.
    fn apply(&mut self, s: &AudioSettings) {
        if self.applied.as_ref() == Some(s) {
            return;
        }
        let t = self.ctx.current_time();
        let m = if s.muted { 0.0 } else { s.master };
        let _ = self.master.gain().set_target_at_time(m, t, 0.05);
        for (k, b) in self.buses.iter().enumerate() {
            let _ = b.gain().set_target_at_time(s.buses[k], t, 0.05);
        }
        self.applied = Some(s.clone());
    }

    /// Starts `stem` on `out`: through a low pass at `lowpass` Hz (none for 0), at `gain`, panned to
    /// `pan` (none for a bus that isn't placed), at a pitch `cents` off. Its length, if it started.
    #[allow(clippy::too_many_arguments)]
    fn start(
        &self,
        stem: &str,
        out: &GainNode,
        gain: f32,
        pan: Option<f32>,
        lowpass: f32,
        cents: f32,
        looped: bool,
    ) -> Option<(AudioBufferSourceNode, GainNode, f64, Option<web_sys::StereoPannerNode>)> {
        let buf = self.buffer(stem)?;
        let ctx = &self.ctx;
        let src = js(ctx.create_buffer_source())?;
        src.set_buffer(Some(&buf));
        src.set_loop(looped);
        if cents != 0.0 {
            src.playback_rate().set_value(2f32.powf(cents / 1_200.0));
        }
        let g = js(ctx.create_gain())?;
        g.gain().set_value(gain);
        let mut head: web_sys::AudioNode = src.clone().into();
        if lowpass > 0.0 && lowpass < 17_000.0 {
            let f = js(ctx.create_biquad_filter())?;
            f.set_type(BiquadFilterType::Lowpass);
            f.frequency().set_value(lowpass);
            js(head.connect_with_audio_node(&f))?;
            head = f.into();
        }
        js(head.connect_with_audio_node(&g))?;
        let mut panner = None;
        match pan {
            Some(p) => {
                let pn = js(ctx.create_stereo_panner())?;
                pn.pan().set_value(p);
                js(g.connect_with_audio_node(&pn))?;
                js(pn.connect_with_audio_node(out))?;
                panner = Some(pn);
            }
            None => {
                js(g.connect_with_audio_node(out))?;
            }
        }
        js(src.start())?;
        let secs = buf.duration() / f64::from(2f32.powf(cents / 1_200.0));
        Some((src, g, secs, panner))
    }

    /// Stops a sound the mixer cut, with a quick fade so it doesn't click.
    fn cut(&mut self, id: u32) {
        let t = self.ctx.current_time();
        if let Some(k) = self.playing.iter().position(|p| p.id == id) {
            let p = self.playing.remove(k);
            let _ = p.gain.gain().set_target_at_time(0.0, t, 0.015);
            let _ = stop(&p.src, t + 0.08);
        }
    }

    /// A bed: started if it isn't playing, its gain and pan eased toward `gain` and `pan`.
    fn bed(&self, bed: &mut Option<Bed>, sfx: Sfx, bus: Bus, gain: f32, pan: Option<f32>) {
        let t = self.ctx.current_time();
        if bed.as_ref().is_some_and(|b| b.sfx != sfx)
            && let Some(b) = bed.take()
        {
            let _ = b.gain.gain().set_target_at_time(0.0, t, 0.4);
            let _ = stop(&b.src, t + 2.0);
        }
        if bed.is_none() && gain > 0.001 {
            let Some((src, g, _, pn)) =
                self.start(&sfx.file(0), &self.buses[bus.index()], 0.0, pan, 0.0, 0.0, true)
            else {
                return;
            };
            *bed = Some(Bed { sfx, src, gain: g, pan: pn });
        }
        if let Some(b) = bed {
            let _ = b.gain.gain().set_target_at_time(gain, t, 0.5);
            if let (Some(pn), Some(p)) = (&b.pan, pan) {
                let _ = pn.pan().set_target_at_time(p, t, 0.3);
            }
        }
    }
}

/// Whether the browser plays Opus in Ogg.
fn can_play_opus() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.create_element("audio").ok())
        .and_then(|e| e.dyn_into::<web_sys::HtmlMediaElement>().ok())
        .is_some_and(|a| !a.can_play_type("audio/ogg; codecs=opus").is_empty())
}

/// Resumes the audio on the page's first click or key (a browser keeps it suspended until one).
fn unlock_on_gesture(ctx: &AudioContext, unlocked: Rc<Cell<bool>>) {
    let Some(window) = web_sys::window() else { return };
    let ctx = ctx.clone();
    let f = Closure::<dyn FnMut()>::new(move || {
        if !unlocked.get() {
            unlocked.set(true);
            let _ = ctx.resume();
        }
    });
    for ev in ["pointerdown", "keydown", "touchstart"] {
        let _ = window.add_event_listener_with_callback(ev, f.as_ref().unchecked_ref());
    }
    f.forget();
}

/// Stops a source at `t` (the scheduled node's call: the buffer source's own is deprecated).
fn stop(src: &AudioBufferSourceNode, t: f64) -> Result<(), JsValue> {
    let s: &web_sys::AudioScheduledSourceNode = src;
    s.stop_with_when(t)
}

async fn fetch_text(url: &str) -> Option<String> {
    let window = web_sys::window()?;
    let res: web_sys::Response = JsFuture::from(window.fetch_with_str(url)).await.ok()?.dyn_into().ok()?;
    if !res.ok() {
        return None;
    }
    JsFuture::from(res.text().ok()?).await.ok()?.as_string()
}

/// Fetches and decodes `stem`, if the server has it.
fn load(store: &Rc<RefCell<Store>>, ctx: &AudioContext, stem: String, ext: &'static str) {
    {
        let mut s = store.borrow_mut();
        let listed = s.listed.as_ref().is_some_and(|l| l.contains(&stem));
        if !listed || !s.asked.insert(stem.clone()) {
            return;
        }
    }
    let store = store.clone();
    let ctx = ctx.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let url = format!("assets/audio/{stem}.{ext}");
        let got = async {
            let window = web_sys::window()?;
            let res: web_sys::Response =
                JsFuture::from(window.fetch_with_str(&url)).await.ok()?.dyn_into().ok()?;
            if !res.ok() {
                return None;
            }
            let bytes: js_sys::ArrayBuffer =
                JsFuture::from(res.array_buffer().ok()?).await.ok()?.dyn_into().ok()?;
            let buf: AudioBuffer =
                JsFuture::from(ctx.decode_audio_data(&bytes).ok()?).await.ok()?.dyn_into().ok()?;
            Some(buf)
        }
        .await;
        let mut s = store.borrow_mut();
        match got {
            Some(b) => {
                s.buffers.insert(stem, b);
            }
            None => s.failed += 1,
        }
    });
}

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        // No match to hear in the transport smoke test.
        if app.world().resource::<crate::look::Look>().art
            && let Some(e) = Engine::new()
        {
            app.insert_non_send(e);
        }
        app.insert_resource(AudioSettings::load()).add_systems(Update, (play, announce).chain().after(FxSet));
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
            e.champion(c.def);
        }
    }
    let t = e.ctx.current_time();
    e.playing.retain(|p| p.ends > t);
    // Under a big moment, the lower rungs sit back.
    let duck = mix::db(e.mixer.duck(now));
    let _ = e.back.gain().set_target_at_time(duck, t, 0.06);
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
        let secs = e.buffer(&sfx.file(0)).map_or(0.5, |b| b.duration());
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
                let out = if rung <= 2 { e.front.clone() } else { e.back.clone() };
                e.start(&file, &out, level * p.gain * weight, Some(p.pan), p.lowpass, cents, false)
            }
            None => {
                let out = e.buses[Bus::Ui.index()].clone();
                e.start(&file, &out, level, None, 0.0, 0.0, false)
            }
        };
        match started {
            Some((src, gain, secs, _)) => {
                e.playing.push(Playing { id, src, gain, ends: t + secs });
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
    voices(e, &mut cues, &ndc, now, t);
    interface(e, &session, now);
    beds(e, &session, &rig, ndc, now);
    status.set(
        "audio_state",
        match e.ctx.state() {
            AudioContextState::Running => "running",
            AudioContextState::Suspended => "suspended",
            _ => "closed",
        },
    );
    status.set("audio_format", e.ext);
    status.set("audio_loaded", e.store.borrow().buffers.len() as u32);
    status.set("audio_failed", e.store.borrow().failed);
    status.set("audio_played", e.stats.played);
    status.set("audio_dropped", e.stats.dropped);
    status.set("audio_missing", e.stats.missing);
    status.set("audio_peak", e.stats.peak as u32);
    status.set("audio_lines", e.stats.lines);
    status.set("audio_own_casts", e.stats.own_casts);
    status.set("audio_last", e.stats.last);
    status.set("audio_unlocked", e.unlocked.get());
    status.set("vo_lines", e.stats.vo_lines);
    status.set("vo_missing", e.stats.vo_missing);
    status.set("vo_dropped", e.stats.vo_dropped);
    status.set("vo_last", e.stats.vo_last.as_str());
    status.set("vo_talking", e.talking.len() as u32);
}

/// The champions' lines of this frame (`fx::vo`): each where its champion stands (the player's
/// own in the middle), never two of one champion's at once (a callout cuts in, talk waits), three
/// champions at most at once (the player's own always gets through); a little under the
/// announcer while it speaks.
fn voices(e: &mut Engine, cues: &mut Cues, ndc: &impl Fn(Vec2) -> Vec2, now: f64, t: f64) {
    e.talking.retain(|v| v.ends > t);
    let spoken = std::mem::take(&mut cues.voice);
    for s in &spoken {
        let place = s.at.map(|at| mix::place(ndc(at)));
        if place.is_some_and(|p| p.gain <= 0.0) {
            continue;
        }
        if let Some(k) = e.talking.iter().position(|v| v.seat == s.seat) {
            if !s.interrupts {
                continue;
            }
            let v = e.talking.remove(k);
            let _ = v.gain.gain().set_target_at_time(0.0, t, 0.02);
            let _ = stop(&v.src, t + 0.1);
        }
        if e.talking.len() >= VO_VOICES {
            // The player's own makes room: the oldest of the others gives way.
            let k = (0..e.talking.len())
                .filter(|&k| !e.talking[k].own)
                .min_by(|&a, &b| e.talking[a].ends.total_cmp(&e.talking[b].ends));
            match k {
                Some(k) if s.rung == 0 => {
                    let v = e.talking.remove(k);
                    let _ = v.gain.gain().set_target_at_time(0.0, t, 0.02);
                    let _ = stop(&v.src, t + 0.1);
                }
                _ => {
                    e.stats.vo_dropped += 1;
                    continue;
                }
            }
        }
        let file = s.file();
        let weight = [1.0, 0.95, 0.85, 0.7, 0.6][usize::from(s.rung.min(4))];
        let under = if now < e.voice_until { 0.7 } else { 1.0 };
        let gain = place.map_or(1.0, |p| p.gain) * weight * under;
        let out = e.buses[Bus::Champions.index()].clone();
        let lowpass = place.map_or(0.0, |p| p.lowpass);
        match e.start(&file, &out, gain, place.map(|p| p.pan), lowpass, 0.0, false) {
            Some((src, g, secs, _)) => {
                e.talking.push(Talking { seat: s.seat, own: s.rung == 0, src, gain: g, ends: t + secs });
                e.stats.vo_lines += 1;
                e.stats.vo_last = file;
                if let Some(d) = cues.director() {
                    d.chatter.talking(usize::from(s.seat), now + secs);
                }
            }
            None => e.stats.vo_missing += 1,
        }
    }
    cues.voice = spoken;
    cues.voice.clear();
}

/// The announcer: one line at a time, shown on the banner as it's said and heard if its file is
/// there (a line without its recording is shown for about as long as it would be said), the
/// music dipping under it.
fn announce(
    engine: Option<NonSendMut<Engine>>,
    mut cues: ResMut<Cues>,
    mut banner: ResMut<Banner>,
    mut until: Local<f64>,
) {
    let now = now_s();
    let busy = now < *until;
    let Some(said) = cues.director().and_then(|d| d.announcer.next(now, busy)) else { return };
    let mut secs = 0.6 + 0.065 * said.says().len() as f64;
    if let Some(mut e) = engine {
        let out = e.buses[Bus::Voice.index()].clone();
        if let Some((_, _, s, _)) =
            e.start(&said.line.file(0), &out, mix::db(said.line.def().gain_db), None, 0.0, 0.0, false)
        {
            secs = s;
            e.stats.lines += 1;
        }
        e.voice_until = now + secs + 0.25;
    }
    *until = now + secs + 0.25;
    banner.show(said, now);
}

/// The page's own sounds: a match found, champion select's last seconds, ready, a reroll.
fn interface(e: &mut Engine, session: &Session, now: f64) {
    let ui = |e: &mut Engine, s: Sfx| {
        let out = e.buses[Bus::Ui.index()].clone();
        if e.start(&s.file(0), &out, mix::db(s.def().gain_db), None, 0.0, 0.0, false).is_some() {
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
    // The champion dealt (or rerolled, or swapped to) says its pick line, once its file is in.
    let mine = session.select.as_ref().and_then(|s| s.mine());
    if let Some(c) = mine.filter(|&c| Some(c) != e.picked) {
        e.champion(c);
        e.pick_due = Some((c, now));
    }
    e.picked = mine;
    if let Some((c, at)) = e.pick_due {
        // Asked for again while it's due (the file list may not have been in when it was dealt).
        e.champion(c);
        let file = vo::file(c, Line::Pick, 0);
        if mine != Some(c) || now - at > 3.0 {
            e.pick_due = None;
        } else if e.buffer(&file).is_some() {
            e.pick_due = None;
            for v in e.talking.drain(..) {
                let _ = v.gain.gain().set_target_at_time(0.0, e.ctx.current_time(), 0.02);
                let _ = stop(&v.src, e.ctx.current_time() + 0.1);
            }
            let out = e.buses[Bus::Champions.index()].clone();
            if let Some((src, gain, secs, _)) = e.start(&file, &out, 1.0, None, 0.0, 0.0, false) {
                let ends = e.ctx.current_time() + secs;
                e.talking.push(Talking { seat: 0, own: true, src, gain, ends });
                e.stats.vo_lines += 1;
                e.stats.vo_last = file;
            }
        }
    }
}

/// The beds: the wind over a match, a brazier's fire when the camera is near one, the music.
fn beds(e: &mut Engine, session: &Session, rig: &crate::camera::Rig, ndc: impl Fn(Vec2) -> Vec2, now: f64) {
    let on_map =
        matches!(session.phase, Phase::Playing | Phase::Reconnecting | Phase::Watching | Phase::Over);
    let mut wind = e.wind.take();
    e.bed(&mut wind, Sfx::AmbWind, Bus::Ambience, if on_map { 1.0 } else { 0.0 }, None);
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
    e.bed(&mut fire, Sfx::AmbFire, Bus::Ambience, gain, Some(pan));
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
    let t = e.ctx.current_time();
    if e.music.as_ref().map(|(m, _)| *m) != Some(want) {
        if let Some((_, b)) = e.music.take() {
            let _ = b.gain.gain().set_target_at_time(0.0, t, 0.6);
            let _ = stop(&b.src, t + 3.0);
        }
        if let Some(s) = want.sound() {
            let out = e.buses[Bus::Music.index()].clone();
            if let Some((src, g, _, _)) = e.start(&s.file(0), &out, 0.0, None, 0.0, 0.0, s.def().looped) {
                e.music = Some((want, Bed { sfx: s, src, gain: g, pan: None }));
            }
        }
    }
    // Under the announcer's line, the music dips.
    if let Some((_, b)) = &e.music {
        let level = mix::db(b.sfx.def().gain_db) * if now < e.voice_until { 0.5 } else { 1.0 };
        let _ = b.gain.gain().set_target_at_time(level, t, 0.4);
    }
}
