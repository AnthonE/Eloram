//! One player's session: the handshake on the control stream, then the lobby (lobby frames, JSON:
//! `el_client_core::lobby`), then a seat in a match: the client's input datagrams go into the
//! seat's ring, and the seat's egress sends it its snapshots.
//!
//! A Hello may sign in with a wallet (`crate::signin`): a challenge, the wallet's signature over
//! the SIWE text for it, checked; or a resume token from an earlier sign-in. Without either the
//! player is a guest, unless the server requires people to sign in.
//!
//! Every hostile byte is survivable: frames are bounded ([`el_proto::control::MAX_FRAME`], lobby
//! frames [`el_proto::control::MAX_LOBBY_FRAME`]), a session that sends what isn't a frame is
//! closed, a datagram that isn't an input packet is counted and dropped, and lobby requests are
//! rate-limited ([`Requests`]).
//!
//! Nothing a client does or fails to do stalls its session: the control stream is written on a
//! task of its own through a bounded outbox, every write has a deadline, and a client that stops
//! reading (its outbox full, or a write past [`WRITE_WAIT`]) is let go rather than waited on, so
//! its inputs keep being read until then and its seat goes to a bot after.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use el_client_core::lobby::{ToClient, ToServer};
use el_match::InputMsg;
use el_proto::auth::{Address, Domain, TOKEN_BYTES};
use el_proto::control::{
    ControlMsg, Frame, MAX_FRAME, MAX_LOBBY_FRAME, PlayerKind, RejectReason, encode_lobby, hello_flags,
    welcome_flags,
};
use el_proto::{InputPacket, MAX_DATAGRAM, PROTOCOL_VERSION, TICK_HZ};
use tokio::sync::mpsc;
use wtransport::{Connection, RecvStream, SendStream};

use super::NetStats;
use super::admit::{Asked, Requests};
use crate::lobby::{Lobby, LobbyEvent, Seating};
use crate::signin::{SignIn, pick_domain, short};

/// How long a client has to say Hello.
const HELLO_WAIT: Duration = Duration::from_secs(10);
/// A seated session that sends nothing for this long has gone.
const QUIET: Duration = Duration::from_secs(20);
/// How long one write to the control stream may take before the client is taken to have stopped
/// reading.
const WRITE_WAIT: Duration = Duration::from_secs(5);
/// Frames waiting to be written to a session's control stream. The lobby sends a few a second at
/// most (champion select's updates); a client this far behind isn't reading.
const OUTBOX: usize = 64;

/// The session's numbers, counted up from 1.
static NEXT_SESSION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// A frame off the control stream.
enum Incoming {
    Msg(ControlMsg),
    Lobby(Vec<u8>),
}

pub async fn run(
    conn: Connection,
    stats: Arc<NetStats>,
    lobby: Arc<Lobby>,
    signin: Arc<SignIn>,
    origin: Option<String>,
) {
    let Ok(Ok((mut tx, mut rx))) = tokio::time::timeout(HELLO_WAIT, async {
        let stream = conn.accept_bi().await?;
        anyhow::Ok(stream)
    })
    .await
    else {
        return;
    };
    let mut inbox = Vec::with_capacity(MAX_FRAME);
    let hello = match tokio::time::timeout(HELLO_WAIT, read_frame(&mut rx, &mut inbox)).await {
        Ok(Some(Incoming::Msg(msg))) => msg,
        _ => return,
    };
    let ControlMsg::Hello { version, player, name, flags, resume } = hello else {
        NetStats::add(&stats.sessions_rejected, 1);
        let _ = send(&mut tx, ControlMsg::Reject { reason: RejectReason::BadHello }).await;
        return;
    };
    if version != PROTOCOL_VERSION {
        NetStats::add(&stats.sessions_rejected, 1);
        let _ = send(&mut tx, ControlMsg::Reject { reason: RejectReason::VersionMismatch }).await;
        return;
    }
    let ask = Ask { flags, resume: &resume, player, origin: origin.as_deref() };
    let address = match identify(&signin, &mut tx, &mut rx, &mut inbox, ask).await {
        Ok(a) => a,
        Err(reason) => {
            NetStats::add(&stats.sessions_rejected, 1);
            let _ = send(&mut tx, ControlMsg::Reject { reason }).await;
            return;
        }
    };
    let session = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
    let max_datagram = conn.max_datagram_size().unwrap_or(MAX_DATAGRAM).min(MAX_DATAGRAM) as u16;
    let welcome = ControlMsg::Welcome {
        version: PROTOCOL_VERSION,
        session,
        tick_hz: TICK_HZ as u8,
        max_datagram,
        flags: if address.is_some() { welcome_flags::SIGNED_IN } else { 0 },
    };
    if send(&mut tx, welcome).await.is_err() {
        return;
    }
    let agent = player == PlayerKind::Agent;
    let name = name.as_str().to_string();
    // Signed in: a token to come back on without the wallet, and a place on the store's roster
    // while this session lasts.
    let _here = match address {
        Some(a) => {
            let token = signin.tokens.issue(a, signin.token_ttl);
            if send(&mut tx, ControlMsg::Token { token }).await.is_err() {
                return;
            }
            Some(signin.online.enter(a, session, &name, agent))
        }
        None => None,
    };
    let present = if agent { &stats.agents_active } else { &stats.humans_active };
    NetStats::add(present, 1);
    let wallet = address.map(|a| short(&a));
    tracing::info!(
        session,
        name = name.as_str(),
        ?player,
        wallet = wallet.as_deref().unwrap_or("guest"),
        "welcomed"
    );

    // The control stream is read on its own task, so a frame half-read is never dropped by the
    // select below.
    let (frames_tx, mut frames) = mpsc::channel(16);
    let reader = tokio::spawn(async move {
        while let Some(f) = read_frame(&mut rx, &mut inbox).await {
            if frames_tx.send(f).await.is_err() {
                break;
            }
        }
    });
    // And written on its own: a client that stops reading never holds up its inputs.
    let (out, mut outbox) = mpsc::channel::<Vec<u8>>(OUTBOX);
    let mut writer = tokio::spawn(async move {
        while let Some(bytes) = outbox.recv().await {
            match tokio::time::timeout(WRITE_WAIT, tx.write_all(&bytes)).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => break,
                Err(_) => {
                    tracing::info!(session, "not reading its control stream: let go");
                    break;
                }
            }
        }
    });
    let (events_tx, mut events) = mpsc::unbounded_channel();
    let mut seated: Option<Box<Seating>> = None;
    let mut last_heard = tokio::time::Instant::now();
    let mut asks = Requests::new(std::time::Instant::now());
    loop {
        tokio::select! {
            _ = &mut writer => break,
            f = frames.recv() => match f {
                None | Some(Incoming::Msg(ControlMsg::Bye { .. })) => break,
                Some(Incoming::Lobby(bytes)) => match asks.ask(std::time::Instant::now()) {
                    Asked::End => {
                        tracing::info!(session, "flooding the lobby: let go");
                        break;
                    }
                    Asked::Refuse => NetStats::add(&stats.lobby_refused, 1),
                    Asked::Take => match serde_json::from_slice::<ToServer>(&bytes) {
                        Ok(ToServer::Queue) => {
                            if let Some(s) = seated.take() {
                                lobby.unseat(*s);
                            }
                            lobby.enqueue(session, &name, agent, max_datagram, events_tx.clone());
                        }
                        Ok(ToServer::Leave) => lobby.leave(session),
                        Ok(ToServer::Reroll) => lobby.reroll(session),
                        Ok(ToServer::Swap { champ }) => lobby.swap(session, champ),
                        Ok(ToServer::Ready) => lobby.ready(session),
                        Ok(ToServer::Rejoin { token }) => {
                            if let Some(s) = seated.take() {
                                lobby.unseat(*s);
                            }
                            if !lobby.rejoin(session, &name, agent, max_datagram, events_tx.clone(), &token)
                                && !post_lobby(&out, &ToClient::NoSeat)
                            {
                                break;
                            }
                        }
                        Ok(ToServer::Practice { champ }) => {
                            if let Some(s) = seated.take() {
                                lobby.unseat(*s);
                            }
                            if !lobby.practice(session, &name, agent, max_datagram, events_tx.clone(), champ)
                                && !post_lobby(&out, &ToClient::NoSeat)
                            {
                                break;
                            }
                        }
                        Err(_) => NetStats::add(&stats.malformed, 1),
                    },
                },
                Some(Incoming::Msg(_)) => {}
            },
            Some(ev) = events.recv() => match ev {
                LobbyEvent::Queued { waiting, fill_in } => {
                    if !post_lobby(&out, &ToClient::Queued { waiting, fill_in }) {
                        break;
                    }
                }
                LobbyEvent::Select(msg) => {
                    if !post_lobby(&out, &msg) {
                        break;
                    }
                }
                LobbyEvent::Seated(s) => {
                    if let Some(old) = seated.take() {
                        lobby.unseat(*old);
                    }
                    lobby.game.attach(s.shard, s.g, conn.clone());
                    let ok = post_lobby(&out, &s.msg);
                    tracing::info!(session, shard = s.shard, m = s.m, seat = s.seat, "seated");
                    seated = Some(s);
                    last_heard = tokio::time::Instant::now();
                    if !ok {
                        break;
                    }
                }
                LobbyEvent::Replaced => {
                    tracing::info!(session, "its seat was taken back on another connection");
                    break;
                }
                LobbyEvent::Over { winner } => {
                    if let Some(s) = seated.take() {
                        lobby.unseat(*s);
                    }
                    if !post_lobby(&out, &ToClient::Over { winner }) {
                        break;
                    }
                }
            },
            dg = conn.receive_datagram(), if seated.is_some() => {
                let Ok(dg) = dg else { break };
                let bytes = dg.payload();
                NetStats::add(&stats.datagrams_in, 1);
                NetStats::add(&stats.bytes_in, bytes.len() as u64);
                let Some(s) = seated.as_mut() else { continue };
                match InputPacket::decode(&bytes) {
                    Ok(packet) => {
                        last_heard = tokio::time::Instant::now();
                        let recv_us = s.shared.now_us();
                        let _ = s.lease.input.push(InputMsg { packet, recv_us });
                    }
                    Err(_) => NetStats::add(&stats.malformed, 1),
                }
            },
            _ = tokio::time::sleep_until(last_heard + QUIET), if seated.is_some() => {
                tracing::info!(session, "quiet for {}s: gone", QUIET.as_secs());
                break;
            }
        }
    }
    reader.abort();
    writer.abort();
    if let Some(s) = seated.take() {
        lobby.unseat(*s);
    }
    lobby.leave(session);
    present.fetch_sub(1, Ordering::Relaxed);
    tracing::info!(session, "left");
}

/// What a Hello asks about who the player is.
struct Ask<'a> {
    flags: u8,
    resume: &'a [u8; TOKEN_BYTES],
    player: PlayerKind,
    /// The page's origin (the WebTransport request's), which picks the challenge's domain.
    origin: Option<&'a str>,
}

/// Who a Hello says the player is: a wallet proved by a signature (or a resume token), or a guest.
async fn identify(
    signin: &SignIn,
    tx: &mut SendStream,
    rx: &mut RecvStream,
    inbox: &mut Vec<u8>,
    ask: Ask<'_>,
) -> Result<Option<Address>, RejectReason> {
    if ask.flags & hello_flags::RESUME != 0 {
        return signin.tokens.redeem(ask.resume).map(Some).ok_or(RejectReason::ResumeExpired);
    }
    if ask.flags & hello_flags::SIGN_IN == 0 {
        return if signin.required && ask.player == PlayerKind::Human {
            Err(RejectReason::AuthRequired)
        } else {
            Ok(None)
        };
    }
    // Only so many sign-ins may wait on a wallet at once.
    let Ok(_permit) = signin.waiting.clone().try_acquire_owned() else {
        return Err(RejectReason::ServerFull);
    };
    let (nonce, issued_at) = SignIn::challenge();
    let domain = Domain::new(pick_domain(&signin.domains, ask.origin));
    send(tx, ControlMsg::Challenge { nonce, issued_at, domain }).await.map_err(|_| RejectReason::BadHello)?;
    let answer = tokio::time::timeout(signin.wait, read_frame(rx, inbox))
        .await
        .map_err(|_| RejectReason::AuthTimeout)?;
    let Some(Incoming::Msg(ControlMsg::Auth { address, signature })) = answer else {
        return Err(RejectReason::BadHello);
    };
    match el_auth::verify(domain.as_str(), &nonce, issued_at, &address, &signature) {
        Ok(()) => Ok(Some(address)),
        Err(e) => {
            tracing::info!(address = short(&address).as_str(), "sign-in refused: {e:?}");
            Err(RejectReason::AuthFailed)
        }
    }
}

/// Writes one control message (before the session's writer task takes the stream over), within
/// [`WRITE_WAIT`].
async fn send(tx: &mut SendStream, msg: ControlMsg) -> anyhow::Result<()> {
    let mut buf = [0u8; MAX_FRAME];
    let n = msg.encode(&mut buf).ok_or_else(|| anyhow::anyhow!("control frame too big"))?;
    tokio::time::timeout(WRITE_WAIT, tx.write_all(&buf[..n])).await??;
    Ok(())
}

/// Hands one lobby message to the session's writer. False if the client isn't keeping up (its
/// outbox full) or the writer has stopped: either way the session is over.
fn post_lobby(out: &mpsc::Sender<Vec<u8>>, msg: &ToClient) -> bool {
    let Ok(json) = serde_json::to_vec(msg) else { return false };
    let mut buf = vec![0u8; json.len() + 3];
    let Some(n) = encode_lobby(&json, &mut buf) else { return false };
    buf.truncate(n);
    match out.try_send(buf) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(_)) => {
            tracing::info!("a session's control stream is {OUTBOX} frames behind: let go");
            false
        }
        Err(mpsc::error::TrySendError::Closed(_)) => false,
    }
}

/// Reads until a whole frame is in `inbox`, and takes it. `None` when the stream ends or sends
/// what isn't a frame.
async fn read_frame(rx: &mut RecvStream, inbox: &mut Vec<u8>) -> Option<Incoming> {
    let mut chunk = [0u8; 1024];
    loop {
        match Frame::decode(inbox) {
            Ok(Some((frame, used))) => {
                let f = match frame {
                    Frame::Msg(m) => Incoming::Msg(m),
                    Frame::Lobby(bytes) => Incoming::Lobby(bytes.to_vec()),
                };
                inbox.drain(..used);
                return Some(f);
            }
            Ok(None) => {}
            Err(_) => return None,
        }
        if inbox.len() >= MAX_LOBBY_FRAME {
            return None;
        }
        match rx.read(&mut chunk).await {
            Ok(Some(n)) => inbox.extend_from_slice(&chunk[..n]),
            _ => return None,
        }
    }
}
