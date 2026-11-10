//! The launch's configuration: in the browser what `web/loader.js` injects as
//! `window.EL_CONFIG` (`web.rs`), on the desktop the command line over the settings file
//! (`native.rs`); and the settings and tokens kept for the next time, through `crate::platform`.

use el_client_core::controls::Layout;

use crate::platform::{forget_param, kv_get, kv_remove, kv_set, tab_get, tab_set};

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
    /// The anim stage (`?animstage=<champion>-<act>`, `animstage.rs`): one champion alone on the
    /// fx stage's clock, no server; `&knobs=` (in `fxknobs`) retunes its motion.
    pub animstage: Option<String>,
    /// The frame-rate bench (`?bench=1`, `bench.rs`): a practice match scripted over the map's
    /// places and a fight, its frame times summed up. `renderer` is what WebGL names the GPU.
    pub bench: bool,
    pub renderer: String,
    /// What a match draws, drawn before it (`warm.rs`); `?warm=0` leaves it to the match.
    pub warm: bool,
    /// The game server's HTTP side (`records.json`, `records/<id>`, `nonce`): the page's address,
    /// or the desktop's `--http`, or what the server list says.
    pub http_base: String,
    /// The desktop's: the server to play on (`--server`, a join link), the list to find one in
    /// (`--servers`), the wallet the launcher plays as (`--identity`), and no launcher at all.
    pub server: Option<String>,
    pub servers_url: Option<String>,
    pub identity: Option<String>,
    pub no_launcher: bool,
    /// A record named by its id (`--watch <id>`), or the newest live match (`?watch=live`).
    pub record_id: Option<String>,
    pub watch_newest: bool,
    /// The desktop's window: fullscreen, windowed, or as last time.
    pub fullscreen: Option<bool>,
    /// The desktop's test hooks: the status written to a file, commands read from one, and a
    /// screenshot saved at a moment, then quit.
    pub status_file: Option<String>,
    pub inbox_file: Option<String>,
    pub screenshot: Option<String>,
    pub screenshot_at: Option<String>,
    /// What the Elo launcher said at the start (the desktop's).
    #[cfg(not(target_arch = "wasm32"))]
    pub launcher: Option<crate::launcher::Found>,
}

pub(crate) fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(target_arch = "wasm32")]
mod web;

/// The game server's HTTP side, once found (the records, the nonce, the status), with its slash.
static HTTP_BASE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

pub fn set_http_base(base: &str) {
    if let Ok(mut b) = HTTP_BASE.lock() {
        *b = base.to_string();
    }
}

/// `path` on the game server's HTTP side (`records.json`, `records/<id>`), or `None` before the
/// server is found. A page with no base was served by the game server itself: the path is its own.
pub fn server_url(path: &str) -> Option<String> {
    let b = HTTP_BASE.lock().ok()?.clone();
    if b.is_empty() {
        return cfg!(target_arch = "wasm32").then(|| path.to_string());
    }
    Some(format!("{}{path}", if b.ends_with('/') { b } else { format!("{b}/") }))
}

impl LaunchConfig {
    /// Whether the launch opens on the main menu: not when a link says where to go (the queue, a
    /// practice match, a record, the fx stage, the bench, the transport's smoke test).
    pub fn menu(&self) -> bool {
        !(self.queue
            || self.practice.is_some()
            || self.record_url.is_some()
            || self.record_id.is_some()
            || self.watch_newest
            || self.echo
            || self.fxstage.is_some()
            || self.animstage.is_some()
            || self.bench)
    }
}

/// Keeps (or with `None`, forgets) the resume token for the next visit.
pub fn store_resume(token: Option<&[u8]>) {
    match token {
        Some(t) => kv_set("el.resume", &t.iter().map(|b| format!("{b:02x}")).collect::<String>()),
        None => kv_remove("el.resume"),
    }
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
/// The one place a setting is written: the browser's `localStorage`, the desktop's settings file.
pub fn remember(key: &str, value: &str, param: &str) {
    kv_set(key, value);
    forget_param(param);
}

/// A setting kept by an earlier visit.
pub fn recall(key: &str) -> Option<String> {
    kv_get(key)
}

/// Remembers the camera locked or free for the next visit (`localStorage["el.camera"]`, which
/// `web/loader.js` reads).
pub fn store_camera(locked: bool) {
    remember("el.camera", if locked { "locked" } else { "free" }, "camera");
}

/// Keeps (or with `None`, forgets) this tab's rejoin token (`sessionStorage["el.rejoin"]`, never
/// logged): a reload, or a dropped link, takes the seat back with it while the match lasts.
pub fn store_rejoin(token: Option<&str>) {
    tab_set("el.rejoin", token);
}

/// This tab's rejoin token, if its last match left one.
pub fn rejoin_token() -> Option<String> {
    tab_get("el.rejoin").filter(|t| !t.is_empty())
}
