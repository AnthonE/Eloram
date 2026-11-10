//! The browser's launch: what `web/loader.js` hands the page as `window.EL_CONFIG`.

use wasm_bindgen::JsValue;

use super::{LaunchConfig, decode_hex};

fn get(obj: &JsValue, key: &str) -> JsValue {
    js_sys::Reflect::get(obj, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
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
            animstage: get(&cfg, "animstage").as_string().filter(|s| !s.is_empty()),
            fxknobs: string("fxknobs"),
            fxseed: get(&cfg, "fxseed").as_f64().unwrap_or(0.0).clamp(0.0, f64::from(u32::MAX)) as u32,
            fxside: string("fxside"),
            fxsize: get(&cfg, "fxsize").as_f64().map(|v| v as f32).filter(|v| *v > 0.0),
            bench: flag("bench"),
            renderer: string("renderer"),
            // Off only when asked: a page whose loader predates it warms.
            warm: get(&cfg, "warm").as_bool() != Some(false),
            fxcards: {
                let c = string("fxcards");
                let mut it = c.split(',').map(|v| v.trim().parse::<f32>().ok());
                match (it.next().flatten(), it.next().flatten()) {
                    (Some(d), Some(l)) => Some([d.clamp(0.0, 1.0), l.clamp(0.0, 1.0)]),
                    _ => None,
                }
            },
            http_base: string("httpBase"),
            watch_newest: flag("watchNewest"),
            ..Self::default()
        }
    }
}
