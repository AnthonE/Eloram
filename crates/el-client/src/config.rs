//! Launch configuration injected by `web/loader.js` as `window.EL_CONFIG`.

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
    /// A record to watch instead of playing (`?watch=<id>`: a live match's, a minute behind;
    /// `?replay=<url>`: any), and whether it's live.
    pub record_url: Option<String>,
    pub watch_live: bool,
    /// The browser has no WebTransport (the page still loads, to say so).
    pub no_web_transport: bool,
    /// Sign in with the browser's wallet (`?signin=1`), or come back on the resume token the last
    /// sign-in left (`localStorage["el.resume"]`, never logged).
    pub sign_in: bool,
    pub resume: Option<[u8; el_proto::auth::TOKEN_BYTES]>,
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
            record_url: get(&cfg, "recordUrl").as_string().filter(|s| !s.is_empty()),
            watch_live: flag("watchLive"),
            no_web_transport: flag("noWebTransport"),
            sign_in: flag("signIn"),
            resume: get(&cfg, "resume")
                .as_string()
                .and_then(|h| decode_hex(&h))
                .and_then(|b| b.try_into().ok()),
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
