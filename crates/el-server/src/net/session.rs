//! One player's session: the handshake on the control stream, then the lobby.
//!
//! Every hostile byte is survivable: frames are bounded ([`el_proto::control::MAX_FRAME`], lobby
//! frames [`el_proto::control::MAX_LOBBY_FRAME`]), and a session that sends what isn't a frame is
//! closed.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use el_proto::control::{ControlMsg, Frame, MAX_FRAME, MAX_LOBBY_FRAME, PlayerKind, RejectReason};
use el_proto::{MAX_DATAGRAM, PROTOCOL_VERSION, TICK_HZ};
use wtransport::{Connection, RecvStream, SendStream};

use super::NetStats;

/// How long a client has to say Hello.
const HELLO_WAIT: Duration = Duration::from_secs(10);

/// The session's numbers, counted up from 1.
static NEXT_SESSION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

pub async fn run(conn: Connection, stats: Arc<NetStats>) {
    let Ok(Ok((mut tx, mut rx))) = tokio::time::timeout(HELLO_WAIT, async {
        let stream = conn.accept_bi().await?;
        anyhow::Ok(stream)
    })
    .await
    else {
        return;
    };
    let mut inbox = Vec::with_capacity(MAX_FRAME);
    let hello = match tokio::time::timeout(HELLO_WAIT, read_msg(&mut rx, &mut inbox)).await {
        Ok(Some(msg)) => msg,
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
    let present = match player {
        PlayerKind::Human => &stats.humans_active,
        PlayerKind::Agent => &stats.agents_active,
    };
    NetStats::add(present, 1);
    tracing::info!(session, name = name.as_str(), ?player, "welcomed");
    // Until the lobby is built, the session holds its place until the client goes.
    while let Some(_msg) = read_msg(&mut rx, &mut inbox).await {}
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

/// Reads until a whole control message is in `inbox`, and takes it (a lobby frame is skipped until
/// the lobby is built). `None` when the stream ends or sends what isn't a frame.
async fn read_msg(rx: &mut RecvStream, inbox: &mut Vec<u8>) -> Option<ControlMsg> {
    let mut chunk = [0u8; 1024];
    loop {
        match Frame::decode(inbox) {
            Ok(Some((frame, used))) => {
                let msg = match frame {
                    Frame::Msg(m) => Some(m),
                    Frame::Lobby(_) => None,
                };
                inbox.drain(..used);
                if let Some(m) = msg {
                    return Some(m);
                }
                continue;
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
