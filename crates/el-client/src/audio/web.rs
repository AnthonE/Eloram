//! The browser's speakers: Web Audio, straight, so the browser decodes the files (no decoder in
//! the wasm) and schedules each sound to the sample. Every sound is its own source → a low pass →
//! its gain → an equal-power panner → the front or back of the effects bus, or a bus; the buses →
//! the master → a limiter → the speakers. The files are found from `assets/audio/files.json`
//! (written by `scripts/build-web.sh`): Opus where the browser plays it, MP3 where it doesn't. A
//! browser starts its audio only after a click or a key: the first one resumes it.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use el_client_core::fx::out::Route;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioBuffer, AudioBufferSourceNode, AudioContext, AudioContextState, BiquadFilterType, GainNode,
};

/// A sound playing, by its number.
pub type Voice = u32;

/// The decoded files, by their stem, and what's on the server.
#[derive(Default)]
struct Store {
    /// The files `files.json` lists (their stems, for the format played), once it's in.
    listed: Option<HashSet<String>>,
    asked: HashSet<String>,
    buffers: HashMap<String, AudioBuffer>,
    failed: u32,
}

struct Node {
    src: AudioBufferSourceNode,
    gain: GainNode,
    pan: Option<web_sys::StereoPannerNode>,
}

pub struct Out {
    ctx: AudioContext,
    master: GainNode,
    buses: Vec<GainNode>,
    front: GainNode,
    back: GainNode,
    store: Rc<RefCell<Store>>,
    ext: &'static str,
    unlocked: Rc<Cell<bool>>,
    nodes: HashMap<Voice, Node>,
    next: Voice,
    pub dropped: u32,
}

fn js<T>(r: Result<T, JsValue>) -> Option<T> {
    r.ok()
}

impl Out {
    pub fn new(_asset_root: &std::path::Path) -> Option<Out> {
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
        for _ in 0..el_client_core::fx::out::BUSES {
            let b = gain(&ctx)?;
            js(b.connect_with_audio_node(&master))?;
            buses.push(b);
        }
        let front = gain(&ctx)?;
        let back = gain(&ctx)?;
        js(front.connect_with_audio_node(&buses[0]))?;
        js(back.connect_with_audio_node(&buses[0]))?;
        let ext = if can_play_opus() { "ogg" } else { "mp3" };
        let unlocked = Rc::new(Cell::new(false));
        unlock_on_gesture(&ctx, unlocked.clone());
        let out = Out {
            ctx,
            master,
            buses,
            front,
            back,
            store: Rc::new(RefCell::new(Store::default())),
            ext,
            unlocked,
            nodes: HashMap::new(),
            next: 1,
            dropped: 0,
        };
        out.list();
        Some(out)
    }

    /// Asks the server what files there are.
    fn list(&self) {
        let store = self.store.clone();
        let ext = self.ext;
        wasm_bindgen_futures::spawn_local(async move {
            let Some(text) = fetch_text("assets/audio/files.json").await else { return };
            let names: Vec<String> = serde_json::from_str(&text).unwrap_or_default();
            let suffix = format!(".{ext}");
            let listed: HashSet<String> =
                names.iter().filter_map(|n| n.strip_suffix(&suffix)).map(str::to_string).collect();
            store.borrow_mut().listed = Some(listed);
        });
    }

    pub fn time(&self) -> f64 {
        self.ctx.current_time()
    }

    pub fn state(&self) -> &'static str {
        match self.ctx.state() {
            AudioContextState::Running => "running",
            AudioContextState::Suspended => "suspended",
            _ => "closed",
        }
    }

    pub fn format(&self) -> &'static str {
        self.ext
    }

    /// Whether the list of files is in (nothing loads before it).
    pub fn listed(&self) -> bool {
        self.store.borrow().listed.is_some()
    }

    pub fn loaded(&self) -> usize {
        self.store.borrow().buffers.len()
    }

    pub fn failed(&self) -> u32 {
        self.store.borrow().failed
    }

    /// Fetches and decodes `stem` (once), if the server has it.
    pub fn want(&mut self, stem: &str) {
        load(&self.store, &self.ctx, stem.to_string(), self.ext);
    }

    /// The browser installs each file as it decodes: nothing to do each frame.
    pub fn poll(&mut self) {}

    pub fn duration(&self, stem: &str) -> Option<f64> {
        self.store.borrow().buffers.get(stem).map(|b| b.duration())
    }

    fn route(&self, r: Route) -> &GainNode {
        match r {
            Route::Front => &self.front,
            Route::Back => &self.back,
            Route::Bus(k) => &self.buses[usize::from(k).min(self.buses.len() - 1)],
        }
    }

    /// Starts `stem` on `route`: through a low pass at `lowpass` Hz (none for 0), at `gain`,
    /// panned to `pan` (none for a bus that isn't placed), at a pitch `cents` off. Its voice and
    /// length, if it started.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        stem: &str,
        route: Route,
        gain: f32,
        pan: Option<f32>,
        lowpass: f32,
        cents: f32,
        looped: bool,
    ) -> Option<(Voice, f64)> {
        let buf = self.store.borrow().buffers.get(stem).cloned()?;
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
        let out = self.route(route).clone();
        let mut panner = None;
        match pan {
            Some(p) => {
                let pn = js(ctx.create_stereo_panner())?;
                pn.pan().set_value(p);
                js(g.connect_with_audio_node(&pn))?;
                js(pn.connect_with_audio_node(&out))?;
                panner = Some(pn);
            }
            None => js(g.connect_with_audio_node(&out)).map(|_| ())?,
        }
        js(src.start())?;
        let secs = buf.duration() / f64::from(2f32.powf(cents / 1_200.0));
        let v = self.next;
        self.next = self.next.wrapping_add(1).max(1);
        self.nodes.insert(v, Node { src, gain: g, pan: panner });
        Some((v, secs))
    }

    pub fn gain(&mut self, v: Voice, target: f32, tau: f64) {
        if let Some(n) = self.nodes.get(&v) {
            let _ = n.gain.gain().set_target_at_time(target, self.ctx.current_time(), tau);
        }
    }

    pub fn pan(&mut self, v: Voice, target: f32, tau: f64) {
        if let Some(pn) = self.nodes.get(&v).and_then(|n| n.pan.as_ref()) {
            let _ = pn.pan().set_target_at_time(target, self.ctx.current_time(), tau);
        }
    }

    /// Faded out (time constant `tau`), and stopped `after` s from now.
    pub fn stop(&mut self, v: Voice, tau: f64, after: f64) {
        if let Some(n) = self.nodes.remove(&v) {
            let t = self.ctx.current_time();
            let _ = n.gain.gain().set_target_at_time(0.0, t, tau);
            let s: &web_sys::AudioScheduledSourceNode = &n.src;
            let _ = s.stop_with_when(t + after);
        }
    }

    /// A sound that ended by itself: its nodes let go.
    pub fn forget(&mut self, v: Voice) {
        self.nodes.remove(&v);
    }

    pub fn bus(&mut self, k: usize, target: f32, tau: f64) {
        if let Some(b) = self.buses.get(k) {
            let _ = b.gain().set_target_at_time(target, self.ctx.current_time(), tau);
        }
    }

    pub fn master(&mut self, target: f32, tau: f64) {
        let _ = self.master.gain().set_target_at_time(target, self.ctx.current_time(), tau);
    }

    pub fn back(&mut self, target: f32, tau: f64) {
        let _ = self.back.gain().set_target_at_time(target, self.ctx.current_time(), tau);
    }

    pub fn unlocked(&self) -> bool {
        self.unlocked.get()
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
