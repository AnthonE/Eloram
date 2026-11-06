//! Launch configuration injected by `web/loader.js` as `window.EL_CONFIG`.

use el_client_core::controls::Layout;
use wasm_bindgen::JsValue;

#[derive(Clone, Debug, Default)]
pub struct LaunchConfig {
    /// WebTransport URL, e.g. `https://127.0.0.1:4433/el`.
    pub wt_url: String,
    /// SHA-256 of the dev server's self-signed certificate, if any.
    pub cert_hash: Option<Vec<u8>>,
    /// `?name=` player name.
    pub name: String,
    /// `?mode=echo`: transport smoke test only.
    pub echo: bool,
    /// Into the queue at once (`?queue=0`: not).
    pub queue: bool,
    /// A practice match instead of the queue (`?practice=1`), as this champion (`&champ=`).
    pub practice: Option<u8>,
    /// A record to watch instead of playing (`?watch=<id>`: a live match's, a minute behind;
    /// `?replay=<url>`: any), and whether it's live.
    pub record_url: Option<String>,
    pub watch_live: bool,
    /// The browser has no WebTransport (the page still loads, to say so).
    pub no_web_transport: bool,
    /// Sign in with the browser's wallet (`?signin=1`), or come back on the resume token the last
    /// sign-in left (`localStorage["el.resume"]`, never logged).
    pub sign_in: bool,
    /// The Elo Pros pass the page signed in with (`web/signin.js`): it goes with the first Hello.
    pub pass: Option<String>,
    pub resume: Option<[u8; el_proto::auth::TOKEN_BYTES]>,
    /// Cast with an indicator (`?indicators=1`, remembered): a held key shows the ability's reach,
    /// and letting go casts. Without it, a key casts at once (quick cast).
    pub indicators: bool,
    /// League's WASD layout (`?keys=wasd`, remembered): the keys steer, the mouse and Shift cast.
    pub wasd: bool,
    /// The camera starts locked on the champion (`?camera=locked`, remembered; Y changes it in a
    /// match, and the memory with it). Free without it, as MOBA players keep it.
    pub camera_locked: bool,
    /// The picture's tier (`?quality=low`, remembered; the default without a GPU): without HDR,
    /// bloom, shadows, multisampling or the braziers' lights, and the ground read from a few
    /// textures, for older machines.
    pub low: bool,
    /// The fx stage (`?fxstage=<name>`, `stage.rs`): one effect alone, no server. With it, the
    /// knobs (`&knobs=`), the seed (`&seed=`), whose view (`&side=`), the cue's size (`&size=`) and
    /// the cards' greys (`&cards=dark,light`).
    pub fxstage: Option<String>,
    pub fxknobs: String,
    pub fxseed: u32,
    pub fxside: String,
    pub fxsize: Option<f32>,
    pub fxcards: Option<[f32; 2]>,
    /// The frame-rate bench (`?bench=1`, `bench.rs`): a practice match scripted over the map's
    /// places and a fight, its frame times summed up. `renderer` is what WebGL names the GPU.
    pub bench: bool,
    pub renderer: String,
}

fn get(obj: &JsValue, key: &str) -> JsValue {
    js_sys::Reflect::get(obj, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

pub(crate) fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

impl LaunchConfig {
    pub fn from_window() -> Self {
        let Some(window) = web_sys::window() else { return Self::default() };
        let cfg = get(&window, "EL_CONFIG");
        let string = |k: &str| get(&cfg, k).as_string().unwrap_or_default();
        let flag = |k: &str| get(&cfg, k).as_bool().unwrap_or(false);
        Self {
            wt_url: string("wtUrl"),
            cert_hash: get(&cfg, "certHash").as_string().and_then(|h| decode_hex(&h)),
            name: string("name"),
            echo: flag("echo"),
            queue: flag("queue"),
            practice: flag("practice")
                .then(|| get(&cfg, "practiceChamp").as_f64().unwrap_or(0.0).clamp(0.0, 255.0) as u8),
            record_url: get(&cfg, "recordUrl").as_string().filter(|s| !s.is_empty()),
            watch_live: flag("watchLive"),
            no_web_transport: flag("noWebTransport"),
            sign_in: flag("signIn"),
            pass: Some(string("pass")).filter(|p| !p.is_empty()),
            resume: get(&cfg, "resume")
                .as_string()
                .and_then(|h| decode_hex(&h))
                .and_then(|b| b.try_into().ok()),
            indicators: flag("indicators"),
            wasd: flag("wasd"),
            camera_locked: flag("cameraLocked"),
            low: string("quality") == "low",
            fxstage: get(&cfg, "fxstage").as_string().filter(|s| !s.is_empty()),
            fxknobs: string("fxknobs"),
            fxseed: get(&cfg, "fxseed").as_f64().unwrap_or(0.0).clamp(0.0, f64::from(u32::MAX)) as u32,
            fxside: string("fxside"),
            fxsize: get(&cfg, "fxsize").as_f64().map(|v| v as f32).filter(|v| *v > 0.0),
            bench: flag("bench"),
            renderer: string("renderer"),
            fxcards: {
                let c = string("fxcards");
                let mut it = c.split(',').map(|v| v.trim().parse::<f32>().ok());
                match (it.next().flatten(), it.next().flatten()) {
                    (Some(d), Some(l)) => Some([d.clamp(0.0, 1.0), l.clamp(0.0, 1.0)]),
                    _ => None,
                }
            },
        }
    }
}

/// Keeps (or with `None`, forgets) the resume token for the next visit.
pub fn store_resume(token: Option<&[u8]>) {
    let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) else { return };
    let _ = match token {
        Some(t) => storage.set_item("el.resume", &t.iter().map(|b| format!("{b:02x}")).collect::<String>()),
        None => storage.remove_item("el.resume"),
    };
}

/// The settings the menu changes while the page runs (`menu.rs`), started from the launch
/// config: each change is kept for the next visit ([`remember`]) and takes effect at once, but the
/// picture's tier, which the renderer is built with, takes effect on the next load. The camera's
/// lock is the rig's (`camera.rs`); the volumes are `audio.rs`'s.
#[derive(bevy::prelude::Resource, Clone, Debug)]
pub struct Prefs {
    pub layout: Layout,
    /// Cast with an indicator: a held key shows the ability's reach, letting go casts.
    pub indicators: bool,
    /// The tier drawn now, and the one chosen for the next load.
    pub low: bool,
    pub low_next: bool,
    /// The first match's strip of keys has been seen (`localStorage["el.taught"]`, `hud_first.rs`).
    pub taught: bool,
}

impl Prefs {
    pub fn of(cfg: &LaunchConfig) -> Self {
        Self {
            layout: Layout::of(cfg.wasd),
            indicators: cfg.indicators,
            low: cfg.low,
            low_next: cfg.low,
            taught: recall("el.taught").is_some_and(|v| v == "1"),
        }
    }

    pub fn set_layout(&mut self, layout: Layout) {
        self.layout = layout;
        remember("el.keys", layout.name(), "keys");
    }

    pub fn set_indicators(&mut self, on: bool) {
        self.indicators = on;
        remember("el.indicators", if on { "1" } else { "0" }, "indicators");
    }

    pub fn set_low_next(&mut self, low: bool) {
        self.low_next = low;
        remember("el.quality", if low { "low" } else { "high" }, "quality");
    }

    pub fn set_taught(&mut self) {
        self.taught = true;
        remember("el.taught", "1", "taught");
    }
}

/// Keeps a setting for the next visit (`localStorage[key]`, which `web/loader.js` reads), and takes
/// `?param=` off the address (where it was the setting asked for, it would win again at a reload).
/// The one place the browser's storage is written for a setting: a desktop build keeps them here
/// in a file.
pub fn remember(key: &str, value: &str, param: &str) {
    let Some(window) = web_sys::window() else { return };
    if let Some(storage) = window.local_storage().ok().flatten() {
        let _ = storage.set_item(key, value);
    }
    let Ok(href) = window.location().href() else { return };
    let Ok(mut url) = url::Url::parse(&href) else { return };
    if !url.query_pairs().any(|(k, _)| k == param) {
        return;
    }
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != param)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut().clear().extend_pairs(kept);
    }
    if let Ok(history) = window.history() {
        let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(url.as_str()));
    }
}

/// A setting kept by an earlier visit.
pub fn recall(key: &str) -> Option<String> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(key).ok().flatten())
}

/// Remembers the camera locked or free for the next visit (`localStorage["el.camera"]`, which
/// `web/loader.js` reads).
pub fn store_camera(locked: bool) {
    remember("el.camera", if locked { "locked" } else { "free" }, "camera");
}

/// Keeps (or with `None`, forgets) this tab's rejoin token (`sessionStorage["el.rejoin"]`, never
/// logged): a reload, or a dropped link, takes the seat back with it while the match lasts.
pub fn store_rejoin(token: Option<&str>) {
    let Some(storage) = web_sys::window().and_then(|w| w.session_storage().ok().flatten()) else { return };
    let _ = match token {
        Some(t) => storage.set_item("el.rejoin", t),
        None => storage.remove_item("el.rejoin"),
    };
}

/// This tab's rejoin token, if its last match left one.
pub fn rejoin_token() -> Option<String> {
    web_sys::window()
        .and_then(|w| w.session_storage().ok().flatten())
        .and_then(|s| s.get_item("el.rejoin").ok().flatten())
        .filter(|t| !t.is_empty())
}
