//! An agent's session: the handshake (as an agent: the store shows agents for what they are), a
//! wallet sign-in if it has a key (`el_auth::LocalWallet` signs the server's challenge, as a
//! browser wallet would), the queue, a seat, then frames of play: datagrams in, the brain asked
//! for its orders, the input packet out. Everything it knows comes from what its team is sent.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use el_client_core::Client;
use el_client_core::lobby::{Player, ToClient, ToServer};
use el_proto::control::{
    ControlMsg, Frame, MAX_FRAME, MAX_LOBBY_FRAME, PlayerKind, encode_lobby, hello_flags,
};
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

#[derive(Clone, Default)]
pub struct BotConfig {
    /// The server's HTTP side, `http://host:port`.
    pub server: String,
    pub name: String,
    /// The agent's wallet key, to sign in with (its own, on its machine); without one it plays as
    /// a guest.
    pub key: Option<[u8; 32]>,
}

/// Not printed: a key in a log is a key given away.
impl std::fmt::Debug for BotConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BotConfig")
            .field("server", &self.server)
            .field("name", &self.name)
            .field("key", &self.key.map(|_| "…"))
            .finish()
    }
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
    /// The wallet it signed in as, if it did.
    pub address: Option<el_proto::auth::Address>,
    /// The token its match gave it, to take the seat back on another connection.
    pub rejoin: Option<String>,
    /// A rejoin found no seat.
    pub no_seat: bool,
    /// In a champion select it hasn't yet said it's ready in (it plays what it's dealt).
    ready_due: bool,
    start: Instant,
}

impl BotClient {
    /// Connects and says hello as an agent named `cfg.name`, signing in with its key if it has
    /// one.
    pub async fn connect(cfg: &BotConfig) -> anyhow::Result<BotClient> {
        let info = transport::discover(&cfg.server).await?;
        let conn = transport::connect(&info.url, info.cert_hash).await?;
        let (mut tx, rx) = conn.open_bi().await?.await?;
        let wallet = cfg.key.as_ref().and_then(el_auth::LocalWallet::from_secret);
        anyhow::ensure!(cfg.key.is_none() || wallet.is_some(), "the key isn't a valid secp256k1 secret");
        let mut hello = ControlMsg::hello(PlayerKind::Agent, &cfg.name);
        if let (ControlMsg::Hello { flags, .. }, Some(_)) = (&mut hello, &wallet) {
            *flags |= hello_flags::SIGN_IN;
        }
        let mut buf = [0u8; MAX_FRAME];
        let n = hello.encode(&mut buf).expect("a hello fits");
        tx.write_all(&buf[..n]).await?;
        let (frames_tx, mut frames) = mpsc::channel(16);
        tokio::spawn(read_frames(rx, frames_tx));
        let mut address = None;
        let session = loop {
            match tokio::time::timeout(Duration::from_secs(10), frames.recv()).await {
                Ok(Some(Incoming::Msg(ControlMsg::Challenge { nonce, issued_at, domain }))) => {
                    let Some(w) = &wallet else { anyhow::bail!("challenged, with no key to sign with") };
                    let signature = w.sign_in(domain.as_str(), &nonce, issued_at);
                    let n = (ControlMsg::Auth { address: w.address(), signature })
                        .encode(&mut buf)
                        .expect("an auth fits");
                    tx.write_all(&buf[..n]).await?;
                    address = Some(w.address());
                }
                Ok(Some(Incoming::Msg(ControlMsg::Welcome { session, .. }))) => break session,
                Ok(Some(Incoming::Msg(ControlMsg::Reject { reason }))) => {
                    anyhow::bail!("refused: {reason:?}")
                }
                _ => anyhow::bail!("no welcome"),
            }
        };
        Ok(BotClient {
            conn,
            tx,
            frames,
            session,
            client: None,
            roster: Vec::new(),
            over: None,
            address,
            rejoin: None,
            no_seat: false,
            ready_due: false,
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

    /// Asks for the seat `token` was given for (an earlier connection's), instead of queueing.
    pub async fn rejoin(&mut self, token: &str) -> anyhow::Result<()> {
        self.over = None;
        self.no_seat = false;
        self.send_lobby(&ToServer::Rejoin { token: token.to_string() }).await
    }

    /// Drops the connection without a goodbye, as a lost link would.
    pub fn drop_link(self) {
        self.conn.close(0u32.into(), b"");
    }

    fn take(&mut self, f: Incoming) -> anyhow::Result<()> {
        match f {
            Incoming::Lobby(bytes) => match serde_json::from_slice::<ToClient>(&bytes)? {
                m @ ToClient::Match { .. } => {
                    let picks = m.picks();
                    let ToClient::Match { seat, tiers, headliner, roster, rejoin, .. } = m else {
                        unreachable!()
                    };
                    let Some(picks) = picks else { anyhow::bail!("a match without ten picks") };
                    anyhow::ensure!(usize::from(seat) < el_sim::units::SEATS, "seat {seat}");
                    self.client = Some(Client::new(map(), usize::from(seat), picks, tiers, headliner));
                    self.roster = roster;
                    self.rejoin = (!rejoin.is_empty()).then_some(rejoin);
                }
                ToClient::NoSeat => self.no_seat = true,
                ToClient::Over { winner } => self.over = Some(winner),
                ToClient::Queued { .. } => {}
                ToClient::Select { .. } => self.ready_due = true,
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
            anyhow::ensure!(!self.no_seat, "no seat to rejoin");
            match tokio::time::timeout_at(end, self.frames.recv()).await {
                Ok(Some(f)) => self.take(f)?,
                Ok(None) => anyhow::bail!("the control stream closed"),
                Err(_) => anyhow::bail!("not seated in {timeout:?}"),
            }
            if std::mem::take(&mut self.ready_due) {
                self.send_lobby(&ToServer::Ready).await?;
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
            if let Some(packets) = c.poll(now, view, &mut |m, k, out| brain(m, seat, k, out)) {
                for p in packets.iter() {
                    let mut buf = [0u8; 256];
                    if let Some(n) = p.encode(&mut buf) {
                        self.conn.send_datagram(&buf[..n])?;
                    }
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
