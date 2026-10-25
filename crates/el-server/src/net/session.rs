//! One player's session: the handshake on the control stream, then the lobby (lobby frames, JSON:
//! `el_client_core::lobby`), then a seat in a match: the client's input datagrams go into the
//! seat's ring, and the seat's egress sends it its snapshots.
//!
//! Every hostile byte is survivable: frames are bounded ([`el_proto::control::MAX_FRAME`], lobby
//! frames [`el_proto::control::MAX_LOBBY_FRAME`]), a session that sends what isn't a frame is
//! closed, and a datagram that isn't an input packet is counted and dropped.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use el_client_core::lobby::{ToClient, ToServer};
use el_match::InputMsg;
use el_proto::control::{
    ControlMsg, Frame, MAX_FRAME, MAX_LOBBY_FRAME, PlayerKind, RejectReason, encode_lobby,
};
use el_proto::{InputPacket, MAX_DATAGRAM, PROTOCOL_VERSION, TICK_HZ};
use tokio::sync::mpsc;
use wtransport::{Connection, RecvStream, SendStream};

use super::NetStats;
use crate::lobby::{Lobby, LobbyEvent, Seating};

/// How long a client has to say Hello.
const HELLO_WAIT: Duration = Duration::from_secs(10);
/// A seated session that sends nothing for this long has gone.
const QUIET: Duration = Duration::from_secs(20);

/// The session's numbers, counted up from 1.
static NEXT_SESSION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// A frame off the control stream.
enum Incoming {
    Msg(ControlMsg),
    Lobby(Vec<u8>),
}

pub async fn run(conn: Connection, stats: Arc<NetStats>, lobby: Arc<Lobby>) {
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
    let ControlMsg::Hello { version, player, name, .. } = hello else {
        NetStats::add(&stats.sessions_rejected, 1);
        let _ = send(&mut tx, ControlMsg::Reject { reason: RejectReason::BadHello }).await;
        return;
    };
    if version != PROTOCOL_VERSION {
        NetStats::add(&stats.sessions_rejected, 1);
        let _ = send(&mut tx, ControlMsg::Reject { reason: RejectReason::VersionMismatch }).await;
        return;
    }
    let session = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
    let max_datagram = conn.max_datagram_size().unwrap_or(MAX_DATAGRAM).min(MAX_DATAGRAM) as u16;
    let welcome = ControlMsg::Welcome {
        version: PROTOCOL_VERSION,
        session,
        tick_hz: TICK_HZ as u8,
        max_datagram,
        flags: 0,
    };
    if send(&mut tx, welcome).await.is_err() {
        return;
    }
    let agent = player == PlayerKind::Agent;
    let present = if agent { &stats.agents_active } else { &stats.humans_active };
    NetStats::add(present, 1);
    let name = name.as_str().to_string();
    tracing::info!(session, name = name.as_str(), ?player, "welcomed");

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
    let (events_tx, mut events) = mpsc::unbounded_channel();
    let mut seated: Option<Box<Seating>> = None;
    let mut last_heard = tokio::time::Instant::now();
    loop {
        tokio::select! {
            f = frames.recv() => match f {
                None | Some(Incoming::Msg(ControlMsg::Bye { .. })) => break,
                Some(Incoming::Lobby(bytes)) => match serde_json::from_slice::<ToServer>(&bytes) {
                    Ok(ToServer::Queue) => {
                        if let Some(s) = seated.take() {
                            lobby.unseat(*s);
                        }
                        lobby.enqueue(session, &name, agent, max_datagram, events_tx.clone());
                    }
                    Ok(ToServer::Leave) => lobby.leave(session),
                    Err(_) => NetStats::add(&stats.malformed, 1),
                },
                Some(Incoming::Msg(_)) => {}
            },
            Some(ev) = events.recv() => match ev {
                LobbyEvent::Queued { waiting, fill_in } => {
                    if send_lobby(&mut tx, &ToClient::Queued { waiting, fill_in }).await.is_err() {
                        break;
                    }
                }
                LobbyEvent::Seated(s) => {
                    if let Some(old) = seated.take() {
                        lobby.unseat(*old);
                    }
                    lobby.game.attach(s.shard, s.g, conn.clone());
                    let ok = send_lobby(&mut tx, &s.msg).await.is_ok();
                    tracing::info!(session, shard = s.shard, m = s.m, seat = s.seat, "seated");
                    seated = Some(s);
                    last_heard = tokio::time::Instant::now();
                    if !ok {
                        break;
                    }
                }
                LobbyEvent::Over { winner } => {
                    if let Some(s) = seated.take() {
                        lobby.unseat(*s);
                    }
                    if send_lobby(&mut tx, &ToClient::Over { winner }).await.is_err() {
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
    if let Some(s) = seated.take() {
        lobby.unseat(*s);
    }
    lobby.leave(session);
    present.fetch_sub(1, Ordering::Relaxed);
    tracing::info!(session, "left");
}

/// Writes one control message.
async fn send(tx: &mut SendStream, msg: ControlMsg) -> anyhow::Result<()> {
    let mut buf = [0u8; MAX_FRAME];
    let n = msg.encode(&mut buf).ok_or_else(|| anyhow::anyhow!("control frame too big"))?;
    tx.write_all(&buf[..n]).await?;
    Ok(())
}

/// Writes one lobby message.
async fn send_lobby(tx: &mut SendStream, msg: &ToClient) -> anyhow::Result<()> {
    let json = serde_json::to_vec(msg)?;
    let mut buf = vec![0u8; json.len() + 3];
    let n = encode_lobby(&json, &mut buf).ok_or_else(|| anyhow::anyhow!("lobby frame too big"))?;
    tx.write_all(&buf[..n]).await?;
    Ok(())
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
