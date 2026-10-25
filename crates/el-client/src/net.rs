//! The transport slot, the dial, and the first words with the server: the echo spike's pings
//! (`?mode=echo`), or the Hello and its Welcome.

use std::cell::RefCell;
use std::rc::Rc;

use bevy::prelude::*;
use el_proto::control::{ControlMsg, Frame, PlayerKind};

use crate::config::LaunchConfig;
use crate::dev_hooks::DevStatus;
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

/// What the session has heard back.
#[derive(Default)]
struct Link {
    hello_sent: bool,
    ctrl: Vec<u8>,
    session: Option<u32>,
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

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.insert_non_send(NetState::default())
            .insert_non_send(Link::default())
            .add_systems(Startup, connect)
            .add_systems(Update, (talk, report));
    }
}

pub fn now_ms() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now())
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
                t.start_pump(|| now_ms() / 1_000.0);
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

fn talk(net: NonSend<NetState>, mut link: NonSendMut<Link>, cfg: Res<LaunchConfigRes>) {
    let Some(t) = net.get() else { return };
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
            Frame::Msg(ControlMsg::Welcome { session, .. }) => link.session = Some(session),
            Frame::Msg(ControlMsg::Reject { reason }) => link.rejected = Some(format!("{reason:?}")),
            _ => {}
        }
        link.ctrl.drain(..used);
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

fn report(net: NonSend<NetState>, link: NonSend<Link>, mut dev: ResMut<DevStatus>) {
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
    if let Some(s) = link.session {
        dev.set("session", s);
    }
    if let Some(r) = &link.rejected {
        dev.set("rejected", r.as_str());
    }
    dev.set("pings", link.pings);
    dev.set("pongs", link.pongs);
    dev.set("rtt_ms", link.rtt_ms);
    dev.set("stream_ok", link.stream_ok);
}
