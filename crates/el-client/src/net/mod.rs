//! The transport slot, the dial, and the session: the echo spike's pings (`?mode=echo`), or the
//! Hello and its Welcome (signing in with the browser's wallet on `?signin=1`), the queue, and a
//! match: the snapshot datagrams into the client core as they arrived, and each frame's orders and
//! command out.
//!
//! A link that drops mid-match (closed, or silent for [`STALE`]) is dialled again, with a backoff,
//! and the seat taken back with the match's rejoin token (`ToServer::Rejoin`); a page reloaded
//! mid-match does the same with the token this tab kept.

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
use crate::platform::Shared;
use crate::session::{Phase, SelectView, Session};
use crate::transport::Transport;

#[derive(Default)]
pub struct NetState {
    pub transport: Shared<Option<Transport>>,
    pub error: Shared<Option<String>>,
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
    /// The launch's Elo Pros pass has gone with a Hello: it's good for one join.
    pass_used: bool,
    ctrl: Vec<u8>,
    rejected: Option<String>,
    /// Playing as a guest whatever the launch said (its sign-in ran out).
    guest: bool,
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
            // The desktop's goodbye, as the browser's page says it on `pagehide` (`web.rs`).
            .add_systems(Last, goodbye_on_exit)
            // `report` after `talk`: a frame that seats us and feeds the first snapshot shows
            // both, never `started` beside the phase it left.
            .add_systems(
                Update,
                (talk.in_set(NetSet::Receive), send.in_set(NetSet::Send), report.after(NetSet::Receive)),
            );
    }
}

pub use crate::platform::{now_ms, now_s};

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(not(target_arch = "wasm32"))]
pub use native::sign_text;
#[cfg(not(target_arch = "wasm32"))]
use native::{dial, goodbye_on_exit, sign_with_wallet};
#[cfg(target_arch = "wasm32")]
pub use web::sign_text;
#[cfg(target_arch = "wasm32")]
use web::{dial, sign_with_wallet};
/// The page says its goodbye on `pagehide` (`web.rs`).
#[cfg(target_arch = "wasm32")]
fn goodbye_on_exit() {}

/// Set to dial now (the menu's way back from watching a record, which dialled nothing).
pub static DIAL_NOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn connect(net: NonSend<NetState>, cfg: Res<LaunchConfigRes>) {
    if cfg.0.record_url.is_some()
        || cfg.0.watch_newest
        || cfg.0.fxstage.is_some()
        || cfg.0.animstage.is_some()
    {
        // Watching: the record comes over HTTP (`watch`), nothing is dialled. The stages have no
        // match at all.
        return;
    }
    if cfg.0.no_web_transport {
        *net.error.borrow_mut() = Some("this browser has no WebTransport: use Chrome or Edge".into());
        return;
    }
    dial(&net, &cfg.0, false);
}

/// Signs in from the menu: the browser's page loads again with `?signin=1` (its sign-in card, then
/// a Hello that asks); the desktop starts again, signing in through the launcher.
pub fn ask_sign_in() {
    crate::platform::reload_with("signin", "1");
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
    if DIAL_NOW.swap(false, std::sync::atomic::Ordering::Relaxed) && net.get().is_none() {
        *link = Link { guest: link.guest, ..Link::default() };
        *net.error.borrow_mut() = None;
        session.error = None;
        session.phase = Phase::Connecting;
        dial(&net, &cfg.0, true);
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
        let name = if cfg.0.name.is_empty() { "Player" } else { cfg.0.name.as_str() };
        let mut hello = ControlMsg::hello(PlayerKind::Human, name);
        let pass = cfg.0.pass.clone().filter(|_| !link.pass_used);
        if let ControlMsg::Hello { flags, resume, .. } = &mut hello {
            // A sign-in kept from an earlier visit is used whether or not this load asks to
            // sign in (a plain load after signing in stays signed in).
            let kept = if link.guest { None } else { link.resume.or(cfg.0.resume) };
            match (cfg.0.sign_in && !link.guest, kept, &pass) {
                (_, Some(token), _) => {
                    *flags |= hello_flags::RESUME;
                    *resume = token;
                }
                (true, None, Some(_)) => *flags |= hello_flags::PASS,
                (true, None, None) => *flags |= hello_flags::SIGN_IN,
                (false, ..) => {}
            }
        }
        let mut buf = [0u8; el_proto::control::MAX_FRAME];
        if let Some(n) = hello.encode(&mut buf) {
            t.send_control(buf[..n].to_vec());
        }
        // A pass is good for one join: it goes with the first Hello that asks for it, and never again.
        if let (ControlMsg::Hello { flags, .. }, Some(pass)) = (&hello, pass)
            && flags & hello_flags::PASS != 0
        {
            let mut out = vec![0u8; pass.len() + 3];
            if let Some(n) = el_proto::control::encode_pass(pass.as_bytes(), &mut out) {
                t.send_control(out[..n].to_vec());
            }
            link.pass_used = true;
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
                if let Some(champ) = session.practice.or(cfg.0.practice) {
                    // A practice match is never come back to: a new one, as the champion last
                    // asked for.
                    session.practice = Some(champ);
                    send_lobby(&t, &ToServer::Practice { champ });
                } else if let Some(token) = crate::config::rejoin_token() {
                    // Back to the seat this tab had, if its match may still be on (menu or not: a
                    // reload or a redial mid-match takes the seat back).
                    send_lobby(&t, &ToServer::Rejoin { token });
                    session.phase = Phase::Reconnecting;
                } else if cfg.0.queue {
                    send_lobby(&t, &ToServer::Queue);
                    session.phase = Phase::Queued;
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
            // A sign-in kept from an earlier visit that's run out: forgotten, and in again as a
            // guest (the menu's identity signs in again).
            Frame::Msg(ControlMsg::Reject { reason: RejectReason::ResumeExpired }) if !link.guest => {
                crate::config::store_resume(None);
                link.resume = None;
                t.close();
                *net.transport.borrow_mut() = None;
                *link = Link { guest: true, ..Link::default() };
                session.phase = Phase::Connecting;
                DIAL_NOW.store(true, std::sync::atomic::Ordering::Relaxed);
                return;
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
                        "The sign-in didn't check out. Reload to sign in again.".into()
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
            Frame::Pass(_) => {}
            Frame::Lobby(bytes) => match serde_json::from_slice::<ToClient>(bytes) {
                Ok(ToClient::Queued { waiting, fill_in }) => {
                    // Queued by the party's leader, from the menu.
                    if matches!(session.phase, Phase::Lobby | Phase::Over) {
                        session.client = None;
                        session.winner = None;
                        session.phase = Phase::Queued;
                    }
                    session.waiting = waiting;
                    session.fill_in = fill_in;
                    session.fill_at = now_s() + f64::from(fill_in);
                }
                Ok(ToClient::Select { seat, picks, bench, rerolls, ends_in, roster, .. }) => {
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
                    if let (
                        Some(picks),
                        ToClient::Match { seat, tiers, headliner, roster, rejoin, practice, stream, .. },
                    ) = (m.picks(), &m)
                    {
                        crate::config::store_rejoin(Some(rejoin.as_str()).filter(|r| !r.is_empty()));
                        link.redials = 0;
                        link.last_datagram = now_s();
                        let seat = usize::from(*seat).min(el_sim::units::SEATS - 1);
                        info!(
                            "seated: seat {seat}, champion {}{}",
                            picks[seat],
                            if *practice { ", practice" } else { "" }
                        );
                        let mut c = Client::new(map.0, seat, picks, *tiers, *headliner);
                        // The last match's snapshots, still queued from a slow frame or in
                        // flight, are on another stream: none of them is taken for this one.
                        c.set_stream(*stream);
                        if *practice {
                            c.set_practice();
                        }
                        session.begin_world();
                        session.client = Some(Box::new(c));
                        session.roster = roster.clone();
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
                    if cfg.0.practice.is_some() {
                        session.phase = Phase::Failed;
                        session.error =
                            Some("This server holds no practice matches (el-server --practice).".into());
                    } else if cfg.0.queue {
                        send_lobby(&t, &ToServer::Queue);
                        session.phase = Phase::Queued;
                    } else {
                        // PRACTICE from the menu on a server without it: said, and back to the menu.
                        if session.practice.take().is_some() {
                            session.refused = Some("This server holds no practice matches.".into());
                        }
                        session.phase = Phase::Lobby;
                    }
                }
                Ok(ToClient::About { player, practice, ranked }) => {
                    session.player = player;
                    session.practice_here = practice;
                    session.ranked_here = ranked;
                }
                Ok(ToClient::Ranked { state, waited_s, why }) => {
                    let line = el_client_core::lobby::ranked_line(&state, waited_s, why.as_deref());
                    // The store let the wait go (or never took it): back to the menu, saying why.
                    if el_client_core::lobby::ranked_ended(&state) && session.phase == Phase::Queued {
                        session.phase = Phase::Lobby;
                        session.refused = Some(line.clone());
                        session.ranked_line = None;
                    } else {
                        // Queued again (a select called off), or by the party's leader.
                        if state == "waiting"
                            && matches!(session.phase, Phase::Lobby | Phase::Over | Phase::Selecting)
                        {
                            session.select = None;
                            session.client = None;
                            session.phase = Phase::Queued;
                        }
                        session.ranked_line = Some(line);
                    }
                }
                Ok(ToClient::Party { leader, members, invites_in }) => {
                    session.party = crate::session::PartyView { leader, members, invites_in };
                }
                Ok(ToClient::Refused { why, .. }) => {
                    if session.phase == Phase::Queued {
                        session.phase = Phase::Lobby;
                    }
                    session.ranked_line = None;
                    session.refused = Some(why);
                }
                Err(e) => warn!("lobby message: {e}"),
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
        // Practice again, as another champion: whenever it's asked.
        if let ToServer::Practice { champ } = msg {
            session.practice = Some(champ);
            send_lobby(&t, &msg);
            continue;
        }
        // Into the queue from the lobby, or again once a match is over (the server lets the old
        // seat go); out of it while waiting.
        match (&msg, session.phase) {
            (ToServer::Queue | ToServer::QueueRanked, Phase::Lobby | Phase::Over) => {
                send_lobby(&t, &msg);
                session.client = None;
                session.winner = None;
                session.waiting = 0;
                session.ranked_line = None;
                session.phase = Phase::Queued;
                continue;
            }
            (ToServer::Leave, Phase::Queued) => {
                send_lobby(&t, &msg);
                session.ranked_line = None;
                session.phase = Phase::Lobby;
                continue;
            }
            // Out of a match on purpose: the seat is let go for good (the server forgets its
            // token, and so does this page), so a reload doesn't pull the player back in.
            (ToServer::Leave, Phase::Playing | Phase::Reconnecting | Phase::Selecting) => {
                send_lobby(&t, &msg);
                crate::config::store_rejoin(None);
                session.practice = None;
                session.client = None;
                session.begin_world();
                session.phase = Phase::Lobby;
                session.left = true;
                continue;
            }
            _ => {}
        }
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
