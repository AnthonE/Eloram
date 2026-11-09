//! The browser's half of the dial: where the server is (the page's `elDiscover()`), the session
//! opened on the page's thread, the goodbye when the page goes, and the sign-in signed by the
//! browser's wallet (`window.elWallet`).

use bevy::prelude::*;
use el_proto::control::ControlMsg;

use super::NetState;
use crate::config::LaunchConfig;
use crate::platform::{Shared, now_s};
use crate::transport::Transport;

/// Where to dial now, as the page's `elDiscover()` finds it: a server restarted since the page
/// loaded may have a new certificate (and its hash) to pin. `None` if the page can't say.
async fn discover() -> Option<(String, Option<Vec<u8>>)> {
    use wasm_bindgen::JsCast;
    let window = web_sys::window()?;
    let f: js_sys::Function = js_sys::Reflect::get(&window, &"elDiscover".into()).ok()?.dyn_into().ok()?;
    let promise: js_sys::Promise = f.call0(&window).ok()?.dyn_into().ok()?;
    let found = wasm_bindgen_futures::JsFuture::from(promise).await.ok()?;
    let url = js_sys::Reflect::get(&found, &"wtUrl".into()).ok()?.as_string()?;
    let hash = js_sys::Reflect::get(&found, &"certHash".into())
        .ok()?
        .as_string()
        .and_then(|h| crate::config::decode_hex(&h));
    Some((url, hash))
}

/// Dials the server; the transport lands in `net`'s slot, or the reason it didn't in its error. A
/// redial (`fresh`) asks the page where the server is first.
pub(super) fn dial(net: &NetState, cfg: &LaunchConfig, fresh: bool) {
    let slot = net.transport.clone();
    let err = net.error.clone();
    let (mut url, mut hash) = (cfg.wt_url.clone(), cfg.cert_hash.clone());
    wasm_bindgen_futures::spawn_local(async move {
        if fresh && let Some(found) = discover().await {
            (url, hash) = found;
        }
        match Transport::connect(&url, hash).await {
            Ok(t) => {
                t.start_pump(now_s);
                say_goodbye_on_leaving(&t);
                *slot.borrow_mut() = Some(t);
            }
            Err(e) => {
                error!("{e}");
                *err.borrow_mut() = Some(e);
            }
        }
    });
}

/// A page closed or navigated away says goodbye first, so the server frees its place at once
/// instead of waiting out the silence.
fn say_goodbye_on_leaving(t: &Transport) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    let t = t.clone();
    let bye = Closure::<dyn FnMut()>::new(move || {
        let mut buf = [0u8; el_proto::control::MAX_FRAME];
        match (ControlMsg::Bye { reason: el_proto::control::bye::LEAVE }).encode(&mut buf) {
            Some(n) => t.close_with(buf[..n].to_vec()),
            None => t.close(),
        }
    });
    if let Some(w) = web_sys::window() {
        let _ = w.add_event_listener_with_callback("pagehide", bye.as_ref().unchecked_ref());
    }
    // The listener lives as long as the page.
    bye.forget();
}

/// What a JavaScript call threw, in words.
fn js_said(e: wasm_bindgen::JsValue) -> String {
    js_sys::Reflect::get(&e, &"message".into())
        .ok()
        .and_then(|m| m.as_string())
        .or_else(|| e.as_string())
        .unwrap_or_else(|| "it said no".into())
}

/// Calls `window.elWallet[method](args…)` and waits for its promise.
async fn wallet_call(method: &str, args: &[wasm_bindgen::JsValue]) -> Result<wasm_bindgen::JsValue, String> {
    use wasm_bindgen::JsCast;
    let window = web_sys::window().ok_or("no window")?;
    let wallet = js_sys::Reflect::get(&window, &"elWallet".into()).map_err(js_said)?;
    let f: js_sys::Function = js_sys::Reflect::get(&wallet, &method.into())
        .map_err(js_said)?
        .dyn_into()
        .map_err(|_| "no wallet script on the page")?;
    let list = js_sys::Array::new();
    for a in args {
        list.push(a);
    }
    let promise: js_sys::Promise =
        f.apply(&wallet, &list).map_err(js_said)?.dyn_into().map_err(|_| "the wallet didn't answer")?;
    wasm_bindgen_futures::JsFuture::from(promise).await.map_err(js_said)
}

/// Answers the server's challenge: the wallet's account (an `0x` one or a Solana one, whichever the
/// page's sign-in chose), the sign-in text for it (`el_auth`, the same text the server rebuilds to
/// check), the wallet's signature over it.
pub(super) fn sign_with_wallet(
    t: Transport,
    nonce: [u8; el_proto::auth::NONCE_BYTES],
    issued_at: u64,
    domain: el_proto::auth::Domain,
    err: Shared<Option<String>>,
) {
    wasm_bindgen_futures::spawn_local(async move {
        let signed: Result<(), String> = async {
            let account =
                wallet_call("connect", &[]).await?.as_string().ok_or("the wallet gave no account")?;
            let mut text = [0u8; el_auth::SIWE_MAX];
            let msg = if let Some(key) = el_proto::auth::base58_key(&account) {
                let n = el_auth::siws_message(domain.as_str(), &account, &nonce, issued_at, &mut text);
                let text = std::str::from_utf8(&text[..n]).map_err(|_| "the sign-in text")?;
                let sig = wallet_call("signSolana", &[text.into(), account.as_str().into()]).await?;
                let signature = el_auth::parse_solana_signature(&sig.as_string().unwrap_or_default())
                    .ok_or("the wallet's answer isn't a signature")?;
                ControlMsg::AuthSolana { key, signature }
            } else {
                let address = el_auth::parse_address(&account)
                    .ok_or("the wallet's account doesn't read as an address")?;
                let n = el_auth::siwe_message(domain.as_str(), &address, &nonce, issued_at, &mut text);
                let text = std::str::from_utf8(&text[..n]).map_err(|_| "the sign-in text")?;
                let sig = wallet_call("sign", &[text.into(), account.as_str().into()]).await?;
                let signature = el_auth::parse_signature(&sig.as_string().unwrap_or_default())
                    .ok_or("the wallet's answer isn't a signature")?;
                ControlMsg::Auth { address, signature }
            };
            let mut buf = [0u8; el_proto::control::MAX_FRAME];
            let n = msg.encode(&mut buf).ok_or("the answer didn't fit")?;
            t.send_control(buf[..n].to_vec());
            Ok(())
        }
        .await;
        if let Err(e) = signed {
            *err.borrow_mut() = Some(format!("The wallet didn't sign in: {e}"));
            t.close();
        }
    });
}
