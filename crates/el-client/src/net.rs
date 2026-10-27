//! The transport slot, the dial, and the session: the echo spike's pings (`?mode=echo`), or the
//! Hello and its Welcome (signing in with the browser's wallet on `?signin=1`), the queue, and a
//! match: the snapshot datagrams into the client core as they arrived, and each frame's orders and
//! command out.
//!
//! A link that drops mid-match (closed, or silent for [`STALE`]) is dialled again, with a backoff,
//! and the seat taken back with the match's rejoin token (`ToServer::Rejoin`); a page reloaded
//! mid-match does the same with the token this tab kept.

use std::cell::RefCell;
use std::rc::Rc;

use bevy::prelude::*;
use el_client_core::Client;
use el_client_core::lobby::{ToClient, ToServer};
use el_proto::control::{
    ControlMsg, Frame, PlayerKind, RejectReason, encode_lobby, hello_flags, welcome_flags,
};
use el_proto::quant::pos_code;

use crate::config::LaunchConfig;
use crate::dev_hooks::DevStatus;
use crate::input::Controls;
use crate::session::{Phase, SelectView, Session};
use crate::transport::Transport;

#[derive(Default)]
pub struct NetState {
    pub transport: Rc<RefCell<Option<Transport>>>,
    pub error: Rc<RefCell<Option<String>>>,
}

impl NetState {
    pub fn get(&self) -> Option<Transport> {
        self.transport.borrow().clone()
    }
}

/// The launch config as a resource.
#[derive(Resource, Clone)]
pub struct LaunchConfigRes(pub LaunchConfig);

/// The map, built once.
#[derive(Resource, Clone, Copy)]
pub struct MapRes(pub &'static el_sim::map::Map);

/// A match's link with no snapshot for this long (s) has gone: it is dialled again.
const STALE: f64 = 4.0;
/// Dials to take a seat back before giving up (about the server's `REJOIN_FOR`).
const REDIALS: u32 = 16;

/// How long (s) before the `n`th redial.
fn backoff(n: u32) -> f64 {
    (0.5 * f64::from(1u32 << n.min(3))).min(4.0)
}

/// What the session has heard back.
#[derive(Default)]
struct Link {
    hello_sent: bool,
    ctrl: Vec<u8>,
    rejected: Option<String>,
    /// The resume token this page was given (newer than the launch config's).
    resume: Option<[u8; el_proto::auth::TOKEN_BYTES]>,
    /// Reconnecting: when the next dial goes, and how many have.
    redial_at: Option<f64>,
    redials: u32,
    /// When the last snapshot came (s).
    last_datagram: f64,
    // The echo spike's counters.
    seq: u32,
    last_ping: f64,
    pings: u32,
    pongs: u32,
    rtt_ms: f64,
    stream_ok: bool,
    stream_sent: bool,
}

pub struct NetPlugin;

/// The order the frame's net work goes in.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum NetSet {
    /// What came in, into the session.
    Receive,
    /// The orders and the command out.
    Send,
}

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.insert_non_send(NetState::default())
            .insert_non_send(Link::default())
            .init_resource::<Session>()
            .configure_sets(Update, NetSet::Receive.before(NetSet::Send))
            .add_systems(Startup, connect)
            // `report` after `talk`: a frame that seats us and feeds the first snapshot shows
            // both, never `started` beside the phase it left.
            .add_systems(
                Update,
                (talk.in_set(NetSet::Receive), send.in_set(NetSet::Send), report.after(NetSet::Receive)),
            );
    }
}

pub fn now_ms() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now())
}

/// The page's clock, s: what the client core and the datagram pump both run on.
pub fn now_s() -> f64 {
    now_ms() / 1_000.0
}

fn connect(net: NonSend<NetState>, cfg: Res<LaunchConfigRes>) {
    if cfg.0.record_url.is_some() {
        // Watching: the record comes over HTTP (`watch`), nothing is dialled.
        return;
    }
    if cfg.0.no_web_transport {
        *net.error.borrow_mut() = Some("this browser has no WebTransport: use Chrome or Edge".into());
        return;
    }
    dial(&net, &cfg.0, false);
}

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
fn dial(net: &NetState, cfg: &LaunchConfig, fresh: bool) {
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
                web_sys::console::error_1(&e.clone().into());
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

/// Answers the server's challenge: the wallet's account, the SIWE text for it (`el_auth`, the same
/// text the server rebuilds to check), the wallet's signature over it.
fn sign_with_wallet(
    t: Transport,
    nonce: [u8; el_proto::auth::NONCE_BYTES],
    issued_at: u64,
    domain: el_proto::auth::Domain,
    err: Rc<RefCell<Option<String>>>,
) {
    wasm_bindgen_futures::spawn_local(async move {
        let signed: Result<(), String> = async {
            let account =
                wallet_call("connect", &[]).await?.as_string().ok_or("the wallet gave no account")?;
            let address =
                el_auth::parse_address(&account).ok_or("the wallet's account doesn't read as an address")?;
            let mut text = [0u8; el_auth::SIWE_MAX];
            let n = el_auth::siwe_message(domain.as_str(), &address, &nonce, issued_at, &mut text);
            let text = std::str::from_utf8(&text[..n]).map_err(|_| "the sign-in text")?;
            let sig = wallet_call("sign", &[text.into(), account.as_str().into()]).await?;
            let signature = el_auth::parse_signature(&sig.as_string().unwrap_or_default())
                .ok_or("the wallet's answer isn't a signature")?;
            let mut buf = [0u8; el_proto::control::MAX_FRAME];
            let n =
                (ControlMsg::Auth { address, signature }).encode(&mut buf).ok_or("the answer didn't fit")?;
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

/// Sends a lobby message.
pub fn send_lobby(t: &Transport, msg: &ToServer) {
    let Ok(json) = serde_json::to_vec(msg) else { return };
    let mut buf = vec![0u8; json.len() + 3];
    if let Some(n) = encode_lobby(&json, &mut buf) {
        t.send_control(buf[..n].to_vec());
    }
}

fn talk(
    net: NonSend<NetState>,
    mut link: NonSendMut<Link>,
    cfg: Res<LaunchConfigRes>,
    map: Res<MapRes>,
    mut session: ResMut<Session>,
) {
    if session.phase == Phase::Reconnecting && !redial(&net, &mut link, &cfg.0, &mut session) {
        return;
    }
    let Some(t) = net.get() else {
        if let Some(e) = net.error.borrow().as_ref() {
            session.phase = Phase::Failed;
            session.error = Some(e.clone());
        }
        return;
    };
    if cfg.0.echo {
        echo(&t, &mut link);
        return;
    }
    if let Some(e) = net.error.borrow().as_ref() {
        session.phase = Phase::Failed;
        session.error = Some(e.clone());
    }
    if !link.hello_sent {
        let name = if cfg.0.name.is_empty() { "Summoner" } else { cfg.0.name.as_str() };
        let mut hello = ControlMsg::hello(PlayerKind::Human, name);
        if let ControlMsg::Hello { flags, resume, .. } = &mut hello {
            match (cfg.0.sign_in, link.resume.or(cfg.0.resume)) {
                (true, Some(token)) => {
                    *flags |= hello_flags::RESUME;
                    *resume = token;
                }
                (true, None) => *flags |= hello_flags::SIGN_IN,
                (false, _) => {}
            }
        }
        let mut buf = [0u8; el_proto::control::MAX_FRAME];
        if let Some(n) = hello.encode(&mut buf) {
            t.send_control(buf[..n].to_vec());
        }
        link.hello_sent = true;
    }
    let incoming = t.take_control();
    link.ctrl.extend_from_slice(&incoming);
    loop {
        let (frame, used) = match Frame::decode(&link.ctrl) {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(_) => {
                // Never left waiting on a stream it can't read: say so and hang up.
                session.phase = Phase::Failed;
                session.error = Some("The server sent something this page can't read. Reload.".into());
                link.ctrl.clear();
                t.close();
                return;
            }
        };
        match frame {
            Frame::Msg(ControlMsg::Welcome { session: id, flags, .. }) => {
                session.session = Some(id);
                session.signed_in = flags & welcome_flags::SIGNED_IN != 0;
                session.phase = Phase::Lobby;
                if cfg.0.queue {
                    // Back to the seat this tab had, if its match may still be on.
                    match crate::config::rejoin_token() {
                        Some(token) => {
                            send_lobby(&t, &ToServer::Rejoin { token });
                            session.phase = Phase::Reconnecting;
                        }
                        None => {
                            send_lobby(&t, &ToServer::Queue);
                            session.phase = Phase::Queued;
                        }
                    }
                }
            }
            Frame::Msg(ControlMsg::Challenge { nonce, issued_at, domain }) => {
                session.phase = Phase::SigningIn;
                sign_with_wallet(t.clone(), nonce, issued_at, domain, net.error.clone());
            }
            // Kept for the next visit, so the wallet isn't asked again while it's good.
            Frame::Msg(ControlMsg::Token { token }) => {
                crate::config::store_resume(Some(&token));
                link.resume = Some(token);
            }
            Frame::Msg(ControlMsg::Reject { reason }) => {
                link.rejected = Some(format!("{reason:?}"));
                session.phase = Phase::Failed;
                session.error = Some(match reason {
                    RejectReason::ResumeExpired => {
                        crate::config::store_resume(None);
                        "Your sign-in has expired. Reload to sign in again.".into()
                    }
                    RejectReason::AuthFailed => {
                        "The signature didn't check out. Reload to sign in again.".into()
                    }
                    RejectReason::AuthRequired => {
                        "This server is for signed-in players: add ?signin=1 to sign in with a wallet.".into()
                    }
                    RejectReason::AuthTimeout => {
                        "The wallet took too long to sign. Reload to try again.".into()
                    }
                    RejectReason::VersionMismatch => {
                        "This page is older than the server. Reload to update the game.".into()
                    }
                    RejectReason::ServerFull => "The server is full. Try again in a moment.".into(),
                    RejectReason::BadHello => "The server didn't understand this client.".into(),
                });
            }
            Frame::Lobby(bytes) => match serde_json::from_slice::<ToClient>(bytes) {
                Ok(ToClient::Queued { waiting, fill_in }) => {
                    session.waiting = waiting;
                    session.fill_in = fill_in;
                }
                Ok(ToClient::Select { seat, picks, bench, rerolls, ends_in, roster }) => {
                    let ready = session.select.as_ref().is_some_and(|s| s.ready);
                    session.select = Some(SelectView {
                        seat: usize::from(seat).min(el_sim::units::SEATS - 1),
                        picks,
                        bench,
                        rerolls,
                        ends_at: now_s() + f64::from(ends_in),
                        ready,
                    });
                    session.roster = roster;
                    session.phase = Phase::Selecting;
                }
                Ok(m @ ToClient::Match { .. }) => {
                    if let (Some(picks), ToClient::Match { seat, tiers, headliner, roster, rejoin, .. }) =
                        (m.picks(), &m)
                    {
                        crate::config::store_rejoin(Some(rejoin.as_str()).filter(|r| !r.is_empty()));
                        link.redials = 0;
                        link.last_datagram = now_s();
                        let seat = usize::from(*seat).min(el_sim::units::SEATS - 1);
                        session.client = Some(Box::new(Client::new(map.0, seat, picks, *tiers, *headliner)));
                        session.roster = roster.clone();
                        session.winner = None;
                        session.kd = Default::default();
                        session.feed.clear();
                        session.select = None;
                        session.phase = Phase::Playing;
                    }
                }
                Ok(ToClient::Over { winner }) => {
                    crate::config::store_rejoin(None);
                    session.winner = Some(winner);
                    session.phase = Phase::Over;
                }
                // The match it would have come back to is gone: a new one.
                Ok(ToClient::NoSeat) => {
                    crate::config::store_rejoin(None);
                    session.client = None;
                    if cfg.0.queue {
                        send_lobby(&t, &ToServer::Queue);
                        session.phase = Phase::Queued;
                    } else {
                        session.phase = Phase::Lobby;
                    }
                }
                Err(e) => web_sys::console::warn_1(&format!("lobby message: {e}").into()),
            },
            _ => {}
        }
        link.ctrl.drain(..used);
    }
    // The snapshots, each at the time it arrived.
    let datagrams = t.drain();
    if let Some((_, at)) = datagrams.last() {
        link.last_datagram = *at;
    }
    if let Some(c) = session.client.as_mut() {
        for (d, arrived) in datagrams {
            let _ = c.on_datagram(&d, arrived);
        }
    }
    // A match's link that closed or went quiet is dialled again, to take the seat back.
    let quiet = session.phase == Phase::Playing && now_s() - link.last_datagram > STALE;
    let in_match = matches!(session.phase, Phase::Playing | Phase::Reconnecting);
    if in_match && (t.is_closed() || quiet) && crate::config::rejoin_token().is_some() {
        t.close();
        *net.transport.borrow_mut() = None;
        link.hello_sent = false;
        link.ctrl.clear();
        link.redial_at = Some(now_s() + backoff(link.redials));
        session.phase = Phase::Reconnecting;
        return;
    }
    if t.is_closed() && session.phase != Phase::Failed {
        session.phase = Phase::Failed;
        session.error.get_or_insert_with(|| "the link to the server dropped".into());
    }
}

/// While reconnecting: the next dial when it's due, a failed one counted (and the next put off),
/// and true once a transport is up to say hello on.
fn redial(net: &NetState, link: &mut Link, cfg: &LaunchConfig, session: &mut Session) -> bool {
    if let Some(at) = link.redial_at {
        if now_s() >= at {
            link.redial_at = None;
            link.redials += 1;
            dial(net, cfg, true);
        }
        return false;
    }
    if net.error.borrow_mut().take().is_some() {
        if link.redials >= REDIALS {
            crate::config::store_rejoin(None);
            session.phase = Phase::Failed;
            session.error = Some("Lost the server, and couldn't get back. Reload to try again.".into());
        } else {
            link.redial_at = Some(now_s() + backoff(link.redials));
        }
        return false;
    }
    net.get().is_some()
}

/// The frame's orders given, the commands due made, and the input packet sent.
fn send(net: NonSend<NetState>, mut session: ResMut<Session>, mut controls: ResMut<Controls>) {
    let Some(t) = net.get() else { return };
    // Champion select's choices, while it lasts.
    for msg in controls.lobby.drain(..) {
        if session.phase == Phase::Selecting {
            if msg == ToServer::Ready
                && let Some(s) = session.select.as_mut()
            {
                s.ready = true;
            }
            send_lobby(&t, &msg);
        }
    }
    if session.phase != Phase::Playing {
        controls.pending.clear();
        return;
    }
    let Some(c) = session.client.as_mut() else { return };
    for o in controls.pending.drain(..) {
        c.order(o);
    }
    let view = pos_code(controls.view);
    let steer = controls.steer;
    if let Some(packets) = c.poll(now_s(), view, &mut |_, _, out| out.steer = steer) {
        for p in packets.iter() {
            let mut buf = [0u8; 256];
            if let Some(n) = p.encode(&mut buf) {
                t.send_datagram(&buf[..n]);
            }
        }
    }
}

/// The transport spike: datagrams stamped with their send time come straight back, and a control
/// stream line too.
fn echo(t: &Transport, s: &mut Link) {
    // Timed by when each arrived (the pump stamps it), not by when this frame drains it.
    for (d, arrived_s) in t.drain() {
        if d.len() == 12 {
            let sent = f64::from_le_bytes(d[4..12].try_into().unwrap_or([0; 8]));
            let rtt = arrived_s * 1_000.0 - sent;
            s.rtt_ms = if s.pongs == 0 { rtt } else { s.rtt_ms * 0.8 + rtt * 0.2 };
            s.pongs += 1;
        }
    }
    if !s.stream_sent {
        t.send_control(b"hello causeway".to_vec());
        s.stream_sent = true;
    }
    if t.take_control().as_slice() == b"hello causeway" {
        s.stream_ok = true;
    }
    let now = now_ms();
    if now - s.last_ping > 100.0 {
        s.last_ping = now;
        s.seq += 1;
        let mut pkt = [0u8; 12];
        pkt[..4].copy_from_slice(&s.seq.to_le_bytes());
        pkt[4..].copy_from_slice(&now.to_le_bytes());
        if t.send_datagram(&pkt) {
            s.pings += 1;
        }
    }
}

fn report(net: NonSend<NetState>, link: NonSend<Link>, session: Res<Session>, mut dev: ResMut<DevStatus>) {
    match net.get() {
        Some(t) => {
            dev.set("connected", !t.is_closed());
            dev.set("max_datagram", t.max_datagram_size() as u32);
        }
        None => {
            dev.set("connected", false);
            if let Some(e) = net.error.borrow().as_ref() {
                dev.set("error", e.as_str());
            }
        }
    }
    if let Some(s) = session.session {
        dev.set("session", s);
    }
    if let Some(r) = &link.rejected {
        dev.set("rejected", r.as_str());
    }
    dev.set("phase", session.phase.name());
    dev.set("signed_in", session.signed_in);
    if let Some(e) = &session.error {
        dev.set("session_error", e.as_str());
    }
    dev.set("pings", link.pings);
    dev.set("pongs", link.pongs);
    dev.set("rtt_ms", link.rtt_ms);
    dev.set("stream_ok", link.stream_ok);
}
