//! The browser's half of [`super`].

use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use wasm_bindgen::JsCast;

use super::{Opened, Req, Slot};

/// State a network task shares with the frame: the page has one thread.
pub struct Shared<T>(Rc<RefCell<T>>);

impl<T> Shared<T> {
    pub fn new(v: T) -> Self {
        Self(Rc::new(RefCell::new(v)))
    }
    pub fn borrow(&self) -> Ref<'_, T> {
        self.0.borrow()
    }
    pub fn borrow_mut(&self) -> RefMut<'_, T> {
        self.0.borrow_mut()
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Default> Default for Shared<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

pub const DESKTOP: bool = false;

pub fn now_ms() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now())
}

fn local() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

fn tab() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.session_storage().ok().flatten())
}

/// A setting kept by an earlier visit (`localStorage`, which `web/loader.js` also reads).
pub fn kv_get(key: &str) -> Option<String> {
    local().and_then(|s| s.get_item(key).ok().flatten())
}

pub fn kv_set(key: &str, value: &str) {
    if let Some(s) = local() {
        let _ = s.set_item(key, value);
    }
}

pub fn kv_remove(key: &str) {
    if let Some(s) = local() {
        let _ = s.remove_item(key);
    }
}

/// Kept while this tab is open (`sessionStorage`): a reload keeps it, another tab doesn't.
pub fn tab_get(key: &str) -> Option<String> {
    tab().and_then(|s| s.get_item(key).ok().flatten())
}

pub fn tab_set(key: &str, value: Option<&str>) {
    if let Some(s) = tab() {
        let _ = match value {
            Some(v) => s.set_item(key, v),
            None => s.remove_item(key),
        };
    }
}

/// Takes `?param=` off the address, where it was the setting chosen (it would win at a reload).
pub fn forget_param(param: &str) {
    let Some(window) = web_sys::window() else { return };
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

/// The page loaded again (the picture's tier is the renderer's, built at load).
pub fn reload() {
    if let Some(w) = web_sys::window() {
        let _ = w.location().reload();
    }
}

/// The page loaded again with `?param=value` added (or changed).
pub fn reload_with(param: &str, value: &str) {
    let Some(window) = web_sys::window() else { return };
    let Ok(href) = window.location().href() else { return };
    let Ok(mut url) = url::Url::parse(&href) else { return };
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != param)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.query_pairs_mut().clear().extend_pairs(kept).append_pair(param, value);
    let _ = window.location().set_href(url.as_str());
}

/// Opens `url` in a new tab.
pub fn open_url(url: &str) -> Opened {
    match web_sys::window().map(|w| w.open_with_url_and_target_and_features(url, "_blank", "noopener")) {
        // With `noopener` the browser hands back no window even when it opened one.
        Some(Ok(_)) => Opened::Opened,
        _ => Opened::Refused,
    }
}

/// The page's `fetch`: the answer lands in the slot.
pub fn fetch(req: Req) -> Slot {
    let slot = Slot::default();
    let out = slot.clone();
    wasm_bindgen_futures::spawn_local(async move {
        out.put(fetch_now(&req).await);
    });
    slot
}

async fn fetch_now(req: &Req) -> super::Answer {
    let window = web_sys::window().ok_or("no window")?;
    let init = web_sys::RequestInit::new();
    if let Some(body) = &req.post {
        init.set_method("POST");
        let headers = web_sys::Headers::new().map_err(|_| "headers")?;
        let _ = headers.set("content-type", if req.plain { "text/plain" } else { "application/json" });
        init.set_headers(&headers);
        init.set_body(&js_sys::Uint8Array::from(body.as_slice()));
    }
    init.set_cache(web_sys::RequestCache::NoStore);
    let resp = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str_and_init(&req.url, &init))
        .await
        .map_err(|_| format!("{}: unreachable", req.url))?;
    let resp: web_sys::Response = resp.dyn_into().map_err(|_| "not a response")?;
    let status = resp.status();
    let buf = wasm_bindgen_futures::JsFuture::from(resp.array_buffer().map_err(|_| "no body")?)
        .await
        .map_err(|_| "the body didn't arrive")?;
    let bytes = js_sys::Uint8Array::new(&buf).to_vec();
    if !(200..300).contains(&status) {
        return Err(answer_error(status, &bytes));
    }
    if bytes.len() > req.cap {
        return Err(format!("{}: too long ({} bytes)", req.url, bytes.len()));
    }
    Ok(bytes)
}

/// A refusal in words: the body's `why`/`detail`/`error` when it says one.
pub(crate) fn answer_error(status: u16, body: &[u8]) -> String {
    let said = serde_json::from_slice::<serde_json::Value>(body).ok().and_then(|v| {
        ["why", "detail", "error"].iter().find_map(|k| v.get(*k).and_then(|x| x.as_str()).map(str::to_owned))
    });
    match said {
        Some(w) => format!("{status}: {w}"),
        None => format!("HTTP {status}"),
    }
}

/// Runs a future on the page's one thread.
pub fn spawn(f: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(f);
}

/// A page leaves no crash notes (the browser's console has the panic).
pub fn last_crash() -> Option<(std::path::PathBuf, String)> {
    None
}

pub fn put_away_crash(_path: &std::path::Path) {}
