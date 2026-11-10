//! The desktop's half of the dial: where the server is (no page to ask), the session opened on the
//! network's runtime, the goodbye on the way out, and the sign-in signed by the Elo launcher.

use bevy::prelude::*;
use el_proto::control::ControlMsg;

use super::NetState;
use crate::config::LaunchConfig;
use crate::platform::{Req, Shared, fetch_blocking, now_s, spawn};
use crate::transport::Transport;

/// The public server's list, where a launch that names no server looks first.
pub const PUBLIC_SERVERS: &str = "https://elopros.com/api/launcher/servers/eloram";
/// And the server itself, if the list can't be read.
const PUBLIC_ADDR: &str = "game.elopros.com:61260";
const PUBLIC_HTTP: &str = "https://game.elopros.com/eloram/";
/// WebTransport's path on the server (`el-server`'s `WT_PATH`).
const WT_PATH: &str = "/el";

/// Whether `http` is this machine's dev server (plain HTTP, its own certificate pinned).
fn loopback(http: &str) -> bool {
    let host = http.trim_start_matches("http://").trim_start_matches("https://");
    let host = host.split('/').next().unwrap_or("");
    let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
}

fn get_json(url: &str) -> Result<serde_json::Value, String> {
    let body = fetch_blocking(&Req::get(url).cap(64 * 1024))?;
    serde_json::from_slice(&body).map_err(|e| format!("{url}: {e}"))
}

/// Where to dial, and the HTTP side's base (records, the nonce, the status): the server asked
/// for, or this machine's dev server by its `/cert-hash`, or the first row of the server list.
pub fn discover(cfg: &LaunchConfig) -> Result<(String, Option<Vec<u8>>, String), String> {
    if let Some(addr) = &cfg.server {
        let http = if cfg.http_base.is_empty() { http_of(addr, cfg) } else { cfg.http_base.clone() };
        return Ok((format!("https://{addr}{WT_PATH}"), cfg.cert_hash.clone(), http));
    }
    if !cfg.http_base.is_empty() && loopback(&cfg.http_base) {
        let base = with_slash(&cfg.http_base);
        let doc = get_json(&format!("{base}cert-hash"))?;
        let port = doc["port"].as_u64().ok_or("/cert-hash names no port")?;
        let path = doc["path"].as_str().unwrap_or(WT_PATH);
        let hash = doc["hash"].as_str().and_then(crate::config::decode_hex);
        let host = base.trim_start_matches("http://").split(['/', ':']).next().unwrap_or("127.0.0.1");
        return Ok((format!("https://{host}:{port}{path}"), hash.or(cfg.cert_hash.clone()), base));
    }
    let list = cfg.servers_url.clone().unwrap_or_else(|| PUBLIC_SERVERS.to_string());
    if let Ok(doc) = get_json(&list)
        && let Some(row) = doc["servers"].as_array().and_then(|r| r.first())
        && let Some(addr) = row["addr"].as_str()
    {
        let http = row["status_url"]
            .as_str()
            .and_then(|s| s.strip_suffix("status.json"))
            .map_or_else(|| http_of(addr, cfg), str::to_owned);
        return Ok((format!("https://{addr}{WT_PATH}"), None, http));
    }
    Ok((format!("https://{PUBLIC_ADDR}{WT_PATH}"), None, PUBLIC_HTTP.to_string()))
}

fn with_slash(s: &str) -> String {
    if s.ends_with('/') { s.to_string() } else { format!("{s}/") }
}

/// The HTTP side of a server named by its address alone.
fn http_of(addr: &str, cfg: &LaunchConfig) -> String {
    if !cfg.http_base.is_empty() {
        return with_slash(&cfg.http_base);
    }
    if addr.starts_with("game.elopros.com") {
        return PUBLIC_HTTP.to_string();
    }
    String::new()
}

/// Dials the server on the network's runtime; the transport lands in `net`'s slot, or the reason
/// it didn't in its error. Every dial finds the server again (a restarted dev server has a new
/// certificate to pin).
pub(super) fn dial(net: &NetState, cfg: &LaunchConfig, _fresh: bool) {
    let (slot, err, cfg) = (net.transport.clone(), net.error.clone(), cfg.clone());
    spawn(async move {
        let found = tokio::task::spawn_blocking(move || discover(&cfg)).await;
        let (url, hash, http) = match found {
            Ok(Ok(f)) => f,
            Ok(Err(e)) => {
                *err.borrow_mut() = Some(e);
                return;
            }
            Err(e) => {
                *err.borrow_mut() = Some(e.to_string());
                return;
            }
        };
        crate::config::set_http_base(&http);
        match Transport::connect(&url, hash).await {
            Ok(t) => {
                t.start_pump(now_s);
                remember(&t);
                *slot.borrow_mut() = Some(t);
            }
            Err(e) => {
                error!("{url}: {e}");
                *err.borrow_mut() = Some(e);
            }
        }
    });
}

/// The session open now, for the goodbye on the way out.
fn last() -> &'static std::sync::Mutex<Option<Transport>> {
    static LAST: std::sync::OnceLock<std::sync::Mutex<Option<Transport>>> = std::sync::OnceLock::new();
    LAST.get_or_init(|| std::sync::Mutex::new(None))
}

fn remember(t: &Transport) {
    if let Ok(mut l) = last().lock() {
        *l = Some(t.clone());
    }
}

/// The window closed (or the menu's QUIT): the server is told, so it frees the place at once
/// instead of waiting out the silence.
pub(super) fn goodbye_on_exit(mut exits: MessageReader<AppExit>) {
    if exits.read().next().is_none() {
        return;
    }
    let Some(t) = last().lock().ok().and_then(|mut l| l.take()) else { return };
    let mut buf = [0u8; el_proto::control::MAX_FRAME];
    match (ControlMsg::Bye { reason: el_proto::control::bye::LEAVE }).encode(&mut buf) {
        Some(n) => t.close_with(buf[..n].to_vec()),
        None => t.close(),
    }
    t.flush_blocking(150);
}

/// Answers the server's challenge with the launcher's proof: the same sign-in text the server
/// rebuilds (its domain, its nonce, its `issued_at`), signed by the player's wallet in the
/// launcher, on a thread (the launcher may wait for the player).
pub(super) fn sign_with_wallet(
    t: Transport,
    nonce: [u8; el_proto::auth::NONCE_BYTES],
    issued_at: u64,
    domain: el_proto::auth::Domain,
    err: Shared<Option<String>>,
) {
    let domain = domain.as_str().to_string();
    let spawned = std::thread::Builder::new().name("el-sign-in".into()).spawn(move || {
        let nonce_hex: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
        let signed = crate::launcher::prove_at(&domain, &nonce_hex, issued_at).and_then(|p| {
            let address = el_auth::parse_address(&p.address.to_ascii_lowercase())
                .ok_or("the launcher's wallet doesn't read as an address")?;
            let mut buf = [0u8; el_proto::control::MAX_FRAME];
            let n = (ControlMsg::Auth { address, signature: el_proto::auth::Signature(p.signature) })
                .encode(&mut buf)
                .ok_or("the answer didn't fit")?;
            t.send_control(buf[..n].to_vec());
            Ok(())
        });
        if let Err(e) = signed {
            *err.borrow_mut() = Some(format!("Didn't sign in: {e}"));
            t.close();
        }
    });
    if spawned.is_err() {
        error!("no thread to sign in on");
    }
}

/// Signs `text` through the Elo launcher (it shows the player the text and asks): the signature's
/// text in the slot. `account` must be the launcher's own.
pub fn sign_text(text: String, account: String) -> crate::platform::Slot {
    let slot = crate::platform::Slot::default();
    let out = slot.clone();
    let spawned = std::thread::Builder::new().name("el-sign".into()).spawn(move || {
        let got = crate::launcher::sign(&text, &account).map(String::into_bytes);
        out.put(got);
    });
    if let Err(e) = spawned {
        slot.put(Err(e.to_string()));
    }
    slot
}
