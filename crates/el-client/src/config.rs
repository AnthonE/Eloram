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
    /// The browser has no WebTransport (the page still loads, to say so).
    pub no_web_transport: bool,
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
            no_web_transport: flag("noWebTransport"),
        }
    }
}
