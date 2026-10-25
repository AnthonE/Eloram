//! An agent's session: the handshake (as an agent: the store shows agents for what they are), the
//! queue, a seat, then frames of play: datagrams in, the brain asked for its orders, the input
//! packet out. Everything it knows comes from what its team is sent.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use el_client_core::Client;
use el_client_core::lobby::{Player, ToClient, ToServer};
use el_proto::control::{ControlMsg, Frame, MAX_FRAME, MAX_LOBBY_FRAME, PlayerKind, encode_lobby};
use el_proto::quant::pos_code;
use el_sim::Match;
use el_sim::game::SeatInput;
use el_sim::map::Map;
use tokio::sync::mpsc;
use wtransport::{Connection, RecvStream, SendStream};

use crate::transport;

/// The map, built once for every agent of the process.
pub fn map() -> &'static Map {
    static M: OnceLock<&'static Map> = OnceLock::new();
    M.get_or_init(Map::leaked)
}

/// A browser frame.
const FRAME: Duration = Duration::from_millis(16);

#[derive(Clone, Debug)]
pub struct BotConfig {
    /// The server's HTTP side, `http://host:port`.
    pub server: String,
    pub name: String,
}

/// What plays the seat: given the prediction, the seat and the tick, its steer and orders.
pub type Brain<'a> = dyn FnMut(&Match, usize, u32, &mut SeatInput) + Send + 'a;

enum Incoming {
    Msg(ControlMsg),
    Lobby(Vec<u8>),
}

pub struct BotClient {
    conn: Connection,
    tx: SendStream,
    frames: mpsc::Receiver<Incoming>,
    pub session: u32,
    /// The match, once seated.
    pub client: Option<Client>,
    pub roster: Vec<Player>,
    /// The winner, once the match is over.
    pub over: Option<u8>,
    start: Instant,
}

impl BotClient {
    /// Connects and says hello as an agent named `cfg.name`.
    pub async fn connect(cfg: &BotConfig) -> anyhow::Result<BotClient> {
        let info = transport::discover(&cfg.server).await?;
        let conn = transport::connect(&info.url, info.cert_hash).await?;
        let (mut tx, rx) = conn.open_bi().await?.await?;
        let mut buf = [0u8; MAX_FRAME];
        let n = ControlMsg::hello(PlayerKind::Agent, &cfg.name).encode(&mut buf).expect("a hello fits");
        tx.write_all(&buf[..n]).await?;
        let (frames_tx, mut frames) = mpsc::channel(16);
        tokio::spawn(read_frames(rx, frames_tx));
        let session = match tokio::time::timeout(Duration::from_secs(10), frames.recv()).await {
            Ok(Some(Incoming::Msg(ControlMsg::Welcome { session, .. }))) => session,
            Ok(Some(Incoming::Msg(ControlMsg::Reject { reason }))) => anyhow::bail!("refused: {reason:?}"),
            _ => anyhow::bail!("no welcome"),
        };
        Ok(BotClient {
            conn,
            tx,
            frames,
            session,
            client: None,
            roster: Vec::new(),
            over: None,
            start: Instant::now(),
        })
    }

    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    async fn send_lobby(&mut self, msg: &ToServer) -> anyhow::Result<()> {
        let json = serde_json::to_vec(msg)?;
        let mut buf = vec![0u8; json.len() + 3];
        let n = encode_lobby(&json, &mut buf).ok_or_else(|| anyhow::anyhow!("lobby frame too big"))?;
        self.tx.write_all(&buf[..n]).await?;
        Ok(())
    }

    /// Joins the queue.
    pub async fn queue(&mut self) -> anyhow::Result<()> {
        self.over = None;
        self.send_lobby(&ToServer::Queue).await
    }

    fn take(&mut self, f: Incoming) -> anyhow::Result<()> {
        match f {
            Incoming::Lobby(bytes) => match serde_json::from_slice::<ToClient>(&bytes)? {
                m @ ToClient::Match { .. } => {
                    let (seed, picks) = (m.seed(), m.picks());
                    let ToClient::Match { seat, roster, .. } = m else { unreachable!() };
                    let (Some(seed), Some(picks)) = (seed, picks) else {
                        anyhow::bail!("a match without its seed or picks")
                    };
                    anyhow::ensure!(usize::from(seat) < el_sim::units::SEATS, "seat {seat}");
                    self.client = Some(Client::new(map(), usize::from(seat), seed, picks));
                    self.roster = roster;
                }
                ToClient::Over { winner } => self.over = Some(winner),
                ToClient::Queued { .. } => {}
            },
            Incoming::Msg(ControlMsg::Bye { reason }) => anyhow::bail!("the server said goodbye ({reason})"),
            Incoming::Msg(_) => {}
        }
        Ok(())
    }

    /// Waits up to `timeout` to be seated.
    pub async fn wait_match(&mut self, timeout: Duration) -> anyhow::Result<()> {
        let end = tokio::time::Instant::now() + timeout;
        while self.client.is_none() {
            match tokio::time::timeout_at(end, self.frames.recv()).await {
                Ok(Some(f)) => self.take(f)?,
                Ok(None) => anyhow::bail!("the control stream closed"),
                Err(_) => anyhow::bail!("not seated in {timeout:?}"),
            }
        }
        Ok(())
    }

    /// One frame: what arrives in it taken, then the brain's command made and sent.
    pub async fn step(&mut self, brain: &mut Brain<'_>) -> anyhow::Result<()> {
        let deadline = tokio::time::Instant::now() + FRAME;
        loop {
            tokio::select! {
                dg = self.conn.receive_datagram() => {
                    let dg = dg?;
                    let now = self.now();
                    if let Some(c) = &mut self.client {
                        // A datagram that doesn't decode is counted in the client's stats.
                        let _ = c.on_datagram(&dg.payload(), now);
                    }
                }
                f = self.frames.recv() => match f {
                    Some(f) => self.take(f)?,
                    None => anyhow::bail!("the control stream closed"),
                },
                _ = tokio::time::sleep_until(deadline) => break,
            }
        }
        let now = self.now();
        if let Some(c) = &mut self.client {
            let seat = c.seat;
            let view = pos_code(c.pred.units[seat].pos);
            if let Some(p) = c.poll(now, view, &mut |m, k, out| brain(m, seat, k, out)) {
                let mut buf = [0u8; 256];
                if let Some(n) = p.encode(&mut buf) {
                    self.conn.send_datagram(&buf[..n])?;
                }
            }
        }
        Ok(())
    }

    /// Plays for `dur`, or until the match is over.
    pub async fn play(&mut self, dur: Duration, brain: &mut Brain<'_>) -> anyhow::Result<()> {
        let end = Instant::now() + dur;
        while Instant::now() < end && self.over.is_none() {
            self.step(brain).await?;
        }
        Ok(())
    }

    /// Says goodbye and closes the session.
    pub async fn close(mut self) {
        let mut buf = [0u8; MAX_FRAME];
        if let Some(n) = (ControlMsg::Bye { reason: el_proto::control::bye::LEAVE }).encode(&mut buf) {
            let _ = self.tx.write_all(&buf[..n]).await;
        }
        let _ = self.tx.finish().await;
        self.conn.close(0u32.into(), b"bye");
    }
}

async fn read_frames(mut rx: RecvStream, out: mpsc::Sender<Incoming>) {
    let mut inbox = Vec::with_capacity(MAX_FRAME);
    let mut chunk = [0u8; 4096];
    loop {
        match Frame::decode(&inbox) {
            Ok(Some((frame, used))) => {
                let f = match frame {
                    Frame::Msg(m) => Incoming::Msg(m),
                    Frame::Lobby(bytes) => Incoming::Lobby(bytes.to_vec()),
                };
                inbox.drain(..used);
                if out.send(f).await.is_err() {
                    return;
                }
                continue;
            }
            Ok(None) => {}
            Err(_) => return,
        }
        if inbox.len() >= MAX_LOBBY_FRAME {
            return;
        }
        match rx.read(&mut chunk).await {
            Ok(Some(n)) => inbox.extend_from_slice(&chunk[..n]),
            _ => return,
        }
    }
}
