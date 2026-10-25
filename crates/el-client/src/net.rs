//! The transport slot, the dial, and the session: the echo spike's pings (`?mode=echo`), or the
//! Hello and its Welcome, the queue, and a match: the snapshot datagrams into the client core as
//! they arrived, and each frame's orders and command out.

use std::cell::RefCell;
use std::rc::Rc;

use bevy::prelude::*;
use el_client_core::Client;
use el_client_core::lobby::{ToClient, ToServer};
use el_proto::control::{ControlMsg, Frame, PlayerKind, encode_lobby};
use el_proto::quant::pos_code;

use crate::config::LaunchConfig;
use crate::dev_hooks::DevStatus;
use crate::input::Controls;
use crate::session::{Phase, Session};
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

/// What the session has heard back.
#[derive(Default)]
struct Link {
    hello_sent: bool,
    ctrl: Vec<u8>,
    rejected: Option<String>,
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
            .add_systems(Update, (talk.in_set(NetSet::Receive), send.in_set(NetSet::Send), report));
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
    let slot = net.transport.clone();
    let err = net.error.clone();
    let url = cfg.0.wt_url.clone();
    let hash = cfg.0.cert_hash.clone();
    if cfg.0.no_web_transport {
        *err.borrow_mut() = Some("this browser has no WebTransport: use Chrome or Edge".into());
        return;
    }
    wasm_bindgen_futures::spawn_local(async move {
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
    if !link.hello_sent {
        let name = if cfg.0.name.is_empty() { "Summoner" } else { cfg.0.name.as_str() };
        let mut buf = [0u8; el_proto::control::MAX_FRAME];
        if let Some(n) = ControlMsg::hello(PlayerKind::Human, name).encode(&mut buf) {
            t.send_control(buf[..n].to_vec());
        }
        link.hello_sent = true;
    }
    let incoming = t.take_control();
    link.ctrl.extend_from_slice(&incoming);
    while let Ok(Some((frame, used))) = Frame::decode(&link.ctrl) {
        match frame {
            Frame::Msg(ControlMsg::Welcome { session: id, .. }) => {
                session.session = Some(id);
                session.phase = Phase::Lobby;
                if cfg.0.queue {
                    send_lobby(&t, &ToServer::Queue);
                    session.phase = Phase::Queued;
                }
            }
            Frame::Msg(ControlMsg::Reject { reason }) => {
                link.rejected = Some(format!("{reason:?}"));
                session.phase = Phase::Failed;
                session.error = Some(format!("the server turned us away ({reason:?})"));
            }
            Frame::Lobby(bytes) => match serde_json::from_slice::<ToClient>(bytes) {
                Ok(ToClient::Queued { waiting, fill_in }) => {
                    session.waiting = waiting;
                    session.fill_in = fill_in;
                }
                Ok(m @ ToClient::Match { .. }) => {
                    if let (Some(seed), Some(picks), ToClient::Match { seat, roster, .. }) =
                        (m.seed(), m.picks(), &m)
                    {
                        let seat = usize::from(*seat).min(el_sim::units::SEATS - 1);
                        session.client = Some(Box::new(Client::new(map.0, seat, seed, picks)));
                        session.roster = roster.clone();
                        session.winner = None;
                        session.kd = Default::default();
                        session.feed.clear();
                        session.phase = Phase::Playing;
                    }
                }
                Ok(ToClient::Over { winner }) => {
                    session.winner = Some(winner);
                    session.phase = Phase::Over;
                }
                Err(e) => web_sys::console::warn_1(&format!("lobby message: {e}").into()),
            },
            _ => {}
        }
        link.ctrl.drain(..used);
    }
    // The snapshots, each at the time it arrived.
    let datagrams = t.drain();
    if let Some(c) = session.client.as_mut() {
        for (d, arrived) in datagrams {
            let _ = c.on_datagram(&d, arrived);
        }
    }
    if t.is_closed() && session.phase != Phase::Failed {
        session.phase = Phase::Failed;
        session.error.get_or_insert_with(|| "the link to the server dropped".into());
    }
}

/// The frame's orders given, the commands due made, and the input packet sent.
fn send(net: NonSend<NetState>, mut session: ResMut<Session>, mut controls: ResMut<Controls>) {
    let Some(t) = net.get() else { return };
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
    if let Some(p) = c.poll(now_s(), view, &mut |_, _, out| out.steer = steer) {
        let mut buf = [0u8; 256];
        if let Some(n) = p.encode(&mut buf) {
            t.send_datagram(&buf[..n]);
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
    dev.set("pings", link.pings);
    dev.set("pongs", link.pongs);
    dev.set("rtt_ms", link.rtt_ms);
    dev.set("stream_ok", link.stream_ok);
}
