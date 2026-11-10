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
use el_proto::auth::{Domain, PlayerId, TOKEN_BYTES};
use el_proto::control::{
    ControlMsg, Frame, MAX_FRAME, MAX_LOBBY_FRAME, PlayerKind, RejectReason, bye, encode_lobby, hello_flags,
    welcome_flags,
};
use el_proto::{InputPacket, MAX_DATAGRAM, PROTOCOL_VERSION, TICK_HZ};
use tokio::sync::mpsc;
use wtransport::{Connection, RecvStream, SendStream};

use super::NetStats;
use super::admit::{Asked, Requests};
use crate::lobby::{Lobby, LobbyEvent, Seating};
use crate::signin::{SignIn, host_of, pick_domain, short, status, unix_now};

/// How long a client has to say Hello.
const HELLO_WAIT: Duration = Duration::from_secs(10);
/// A seated session that sends nothing for this long has gone.
const QUIET: Duration = Duration::from_secs(20);
/// A session out of any match (at the menu) that asks for nothing for this long is let go
/// (`bye::IDLE`): a tab left open earns no hosted hours.
const IDLE: Duration = Duration::from_secs(30 * 60);
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
    Pass(String),
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
    let here = match address {
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
    // Who we know them as, and what this server holds: the menu's identity and its PRACTICE and
    // RANKED states.
    let about = ToClient::About {
        player: address.map(|a| a.to_string()),
        practice: lobby.practice_allowed(),
        ranked: lobby.elopros.get().is_some_and(|e| e.ranked_on),
    };
    if !post_lobby(&out, &about) {
        reader.abort();
        writer.abort();
        present.fetch_sub(1, Ordering::Relaxed);
        return;
    }
    let say = |now: &'static str| {
        if let Some(h) = here.as_ref() {
            h.set_status(now);
        }
    };
    let (events_tx, mut events) = mpsc::unbounded_channel();
    // Who this session is, as the lobby seats it: kept by the parties, so a leader can queue it.
    let me = crate::lobby::Waiting {
        session,
        name: name.clone(),
        agent,
        max_datagram,
        tx: events_tx.clone(),
        token: String::new(),
        wallet: address,
        party: 0,
    };
    lobby.parties.register(me.clone());
    let mut seated: Option<Box<Seating>> = None;
    let mut last_heard = tokio::time::Instant::now();
    let mut asks = Requests::new(std::time::Instant::now());
    // One timer for the session, put forward when it goes off early, rather than a new one each
    // time round the loop (once an input datagram: a timer registered and dropped each time).
    let quiet = tokio::time::sleep_until(last_heard + QUIET);
    tokio::pin!(quiet);
    // And one for the menu's idle: put forward by anything asked or told in the lobby.
    let mut last_asked = tokio::time::Instant::now();
    let idle = tokio::time::sleep_until(last_asked + IDLE);
    tokio::pin!(idle);
    let mut idled = false;
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
                            last_asked = tokio::time::Instant::now();
                            let party = lobby.parties.party_of(session);
                            let no = match &party {
                                Some(p) if p[0] != session => Some("The party's leader queues for the party.".to_string()),
                                Some(p) => p
                                    .iter()
                                    .skip(1)
                                    .find(|&&m| lobby.busy(m))
                                    .map(|&m| format!("{} is still in a match.", lobby.parties.name(m))),
                                None => None,
                            };
                            if let Some(why) = no {
                                if !post_lobby(&out, &refused("queue", why)) {
                                    break;
                                }
                                continue;
                            }
                            if let Some(s) = seated.take() {
                                lobby.unseat(*s);
                            }
                            match party {
                                Some(p) => lobby.enqueue_party(lobby.parties.waiting(&p)),
                                None => lobby.enqueue(session, &name, agent, max_datagram, events_tx.clone(), address),
                            }
                            say(status::QUEUED);
                        }
                        Ok(ToServer::QueueRanked) => {
                            last_asked = tokio::time::Instant::now();
                            let elo = lobby.elopros.get().filter(|e| e.ranked_on);
                            let members = match lobby.parties.party_of(session) {
                                Some(p) => lobby.parties.waiting(&p),
                                None => vec![me.clone()],
                            };
                            let leader = members.first().map_or(session, |w| w.session);
                            let wallet = |w: &crate::lobby::Waiting| w.wallet.filter(|a| !a.as_str().starts_with("acct_"));
                            let no = if elo.is_none() {
                                Some("This server doesn't hold ranked matches.".to_string())
                            } else if leader != session {
                                Some("The party's leader queues for the party.".to_string())
                            } else if let Some(w) = members.iter().find(|w| wallet(w).is_none()) {
                                Some(if w.session == session {
                                    "Ranked is for wallets: sign in with yours to queue.".to_string()
                                } else {
                                    format!("Ranked is for wallets: {} isn't signed in with one.", w.name)
                                })
                            } else if seated.is_some() || members.iter().any(|w| lobby.busy(w.session)) {
                                Some("Leave the match first.".to_string())
                            } else {
                                None
                            };
                            match (elo, no) {
                                (Some(elo), None) => {
                                    let party = if members.len() > 1 { leader } else { 0 };
                                    let mut queued = Vec::with_capacity(members.len());
                                    for w in members {
                                        let _ = lobby.leave(w.session);
                                        let id = wallet(&w).map(|a| a.to_string()).unwrap_or_default();
                                        queued.push((id, crate::lobby::Waiting { party, ..w }));
                                    }
                                    crate::elopros::ranked::join_party(elo, queued, None);
                                    say(status::RANKED);
                                }
                                (_, why) => {
                                    let why = why.unwrap_or_default();
                                    if !post_lobby(&out, &refused("ranked", why)) {
                                        break;
                                    }
                                }
                            }
                        }
                        // Out of the queue or select; or, seated, out of the match for good.
                        Ok(ToServer::Leave) => {
                            last_asked = tokio::time::Instant::now();
                            let mut gone = Vec::new();
                            if let Some(elo) = lobby.elopros.get() {
                                gone.extend(crate::elopros::ranked::leave(elo, session));
                            }
                            match seated.take() {
                                Some(s) => {
                                    tracing::info!(session, m = s.m, seat = s.seat, "left its match");
                                    lobby.leave_seat(*s);
                                }
                                None => gone.extend(lobby.leave(session)),
                            }
                            lobby.parties.tell_left(&gone, &name);
                            say(status::MENU);
                        }
                        Ok(ToServer::PartyInvite { player }) => {
                            last_asked = tokio::time::Instant::now();
                            let to = el_proto::auth::PlayerId::parse(&player).and_then(|id| signin.online.session_of(&id));
                            let got = match to {
                                Some(to) => lobby.parties.invite(session, to),
                                None => Err("They aren't signed in on this server.".to_string()),
                            };
                            if let Err(why) = got
                                && !post_lobby(&out, &refused("party", why))
                            {
                                break;
                            }
                        }
                        Ok(ToServer::PartyAccept { leader }) => {
                            last_asked = tokio::time::Instant::now();
                            if let Err(why) = lobby.parties.accept(session, leader)
                                && !post_lobby(&out, &refused("party", why))
                            {
                                break;
                            }
                        }
                        Ok(ToServer::PartyDecline { leader }) => lobby.parties.decline(session, leader),
                        Ok(ToServer::PartyLeave) => {
                            last_asked = tokio::time::Instant::now();
                            lobby.parties.leave(session);
                        }
                        Ok(ToServer::Reroll) => lobby.reroll(session),
                        Ok(ToServer::Swap { champ }) => lobby.swap(session, champ),
                        Ok(ToServer::Ready) => lobby.ready(session),
                        Ok(ToServer::Rejoin { token }) => {
                            if let Some(s) = seated.take() {
                                lobby.unseat(*s);
                            }
                            if !lobby.rejoin(
                                session,
                                &name,
                                agent,
                                max_datagram,
                                events_tx.clone(),
                                &token,
                                address,
                            ) && !post_lobby(&out, &ToClient::NoSeat)
                            {
                                break;
                            }
                        }
                        Ok(ToServer::Practice { champ }) => {
                            last_asked = tokio::time::Instant::now();
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
                Some(Incoming::Msg(_) | Incoming::Pass(_)) => {}
            },
            Some(ev) = events.recv() => match ev {
                LobbyEvent::Queued { waiting, fill_in } => {
                    last_asked = tokio::time::Instant::now();
                    say(status::QUEUED);
                    if !post_lobby(&out, &ToClient::Queued { waiting, fill_in }) {
                        break;
                    }
                }
                LobbyEvent::Select(msg) => {
                    last_asked = tokio::time::Instant::now();
                    say(status::SELECT);
                    if !post_lobby(&out, &msg) {
                        break;
                    }
                }
                LobbyEvent::Seated(s) => {
                    if let Some(old) = seated.take() {
                        lobby.unseat(*old);
                    }
                    lobby.game.attach(s.shard, s.g, conn.clone());
                    let practice = matches!(s.msg, ToClient::Match { practice: true, .. });
                    say(if practice { status::PRACTICE } else { status::PLAYING });
                    let ok = post_lobby(&out, &s.msg);
                    tracing::info!(session, shard = s.shard, m = s.m, seat = s.seat, "seated");
                    seated = Some(s);
                    last_heard = tokio::time::Instant::now();
                    if !ok {
                        break;
                    }
                }
                LobbyEvent::Tell(msg) => {
                    last_asked = tokio::time::Instant::now();
                    if !post_lobby(&out, &msg) {
                        break;
                    }
                }
                LobbyEvent::Replaced => {
                    tracing::info!(session, "its seat was taken back on another connection");
                    break;
                }
                LobbyEvent::Over { winner } => {
                    last_asked = tokio::time::Instant::now();
                    if let Some(s) = seated.take() {
                        lobby.unseat(*s);
                    }
                    say(status::MENU);
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
            () = &mut quiet, if seated.is_some() => {
                // Heard from since it was set: set again, for QUIET after that.
                if last_heard + QUIET > tokio::time::Instant::now() {
                    quiet.as_mut().reset(last_heard + QUIET);
                    continue;
                }
                tracing::info!(session, "quiet for {}s: gone", QUIET.as_secs());
                break;
            }
            () = &mut idle, if seated.is_none() => {
                if last_asked + IDLE > tokio::time::Instant::now() {
                    idle.as_mut().reset(last_asked + IDLE);
                    continue;
                }
                tracing::info!(session, "idle at the menu for {} min: let go", IDLE.as_secs() / 60);
                idled = true;
                break;
            }
        }
    }
    reader.abort();
    if idled {
        // Said, then the writer finishes what it has (it ends once its outbox is closed).
        let mut buf = [0u8; MAX_FRAME];
        if let Some(n) = (ControlMsg::Bye { reason: bye::IDLE }).encode(&mut buf) {
            let _ = out.try_send(buf[..n].to_vec());
        }
        drop(out);
        let _ = tokio::time::timeout(WRITE_WAIT, &mut writer).await;
    }
    writer.abort();
    if let Some(s) = seated.take() {
        lobby.unseat(*s);
    }
    let mut gone = lobby.leave(session);
    if let Some(elo) = lobby.elopros.get() {
        gone.extend(crate::elopros::ranked::leave(elo, session));
    }
    lobby.parties.tell_left(&gone, &name);
    lobby.parties.unregister(session);
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

/// Who a Hello says the player is: a wallet proved by a signature, an Elo Pros player proved by a
/// pass (or either by a resume token), or a guest.
async fn identify(
    signin: &SignIn,
    tx: &mut SendStream,
    rx: &mut RecvStream,
    inbox: &mut Vec<u8>,
    ask: Ask<'_>,
) -> Result<Option<PlayerId>, RejectReason> {
    if ask.flags & hello_flags::RESUME != 0 {
        return signin.tokens.redeem(ask.resume).map(Some).ok_or(RejectReason::ResumeExpired);
    }
    if ask.flags & hello_flags::PASS != 0 {
        let frame = tokio::time::timeout(HELLO_WAIT, read_frame(rx, inbox)).await;
        let Ok(Some(Incoming::Pass(pass))) = frame else { return Err(RejectReason::AuthFailed) };
        let key = signin.pass_key.ok_or(RejectReason::AuthFailed)?;
        let hosts = signin.domains.iter().map(|d| host_of(d));
        return match el_auth::verify_pass(
            &pass,
            &key,
            el_auth::GAME,
            hosts,
            |n| signin.nonces.take(n),
            unix_now(),
        ) {
            Ok(passed) => Ok(Some(passed.player())),
            Err(e) => {
                tracing::info!("pass refused: {e:?}");
                Err(RejectReason::AuthFailed)
            }
        };
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
    let (id, checked) = match answer {
        Some(Incoming::Msg(ControlMsg::Auth { address, signature })) => (
            PlayerId::from(address),
            el_auth::verify(domain.as_str(), &nonce, issued_at, &address, &signature),
        ),
        Some(Incoming::Msg(ControlMsg::AuthSolana { key, signature })) => (
            PlayerId::solana(&key),
            el_auth::verify_solana(domain.as_str(), &nonce, issued_at, &key, &signature),
        ),
        _ => return Err(RejectReason::BadHello),
    };
    match checked {
        Ok(()) => Ok(Some(id)),
        Err(e) => {
            tracing::info!(address = short(&id).as_str(), "sign-in refused: {e:?}");
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

/// Something this session asked for that the server won't do, in words.
fn refused(what: &str, why: impl Into<String>) -> ToClient {
    ToClient::Refused { what: what.into(), why: why.into() }
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
                    Frame::Pass(bytes) => Incoming::Pass(String::from_utf8_lossy(bytes).into_owned()),
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
