//! The desktop's WebTransport (wtransport, as `el-bot`'s `transport.rs`, from which the dial is
//! lifted rather than depended on: el-bot builds the sim with glam's scalar maths, which Bevy's
//! glam won't take). The same face as the browser's (`web.rs`): a pump stamps each datagram as it
//! arrives, the frame drains them, and the control stream's few bytes come and go in two tasks.
//!
//! Trust: a named host by the system's roots (a CA-issued certificate, as the public server's); a
//! server's self-signed certificate only when pinned by its SHA-256 (`--cert-hash`, or a dev
//! server's own `/cert-hash` on this machine), exactly as the browser pins one.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use wtransport::config::IpBindConfig;
use wtransport::tls::Sha256Digest;
use wtransport::{ClientConfig, Connection, Endpoint};

/// The most datagrams kept for the frame, as the browser's pump: a stalled frame must not grow
/// this without bound, and old snapshots are useless.
const INBOX: usize = 512;

/// Received datagrams with their arrival times (s).
type Inbox = Arc<Mutex<VecDeque<(Vec<u8>, f64)>>>;

enum CtrlOut {
    Bytes(Vec<u8>),
    /// Close the session once everything before this is written (a goodbye).
    Close,
}

#[derive(Clone)]
pub struct Transport {
    conn: Connection,
    ctrl_tx: mpsc::UnboundedSender<CtrlOut>,
    ctrl_in: Arc<Mutex<Vec<u8>>>,
    closed: Arc<AtomicBool>,
    inbox: Inbox,
    /// Set once the writer has sent everything it was given, the goodbye included.
    flushed: Arc<AtomicBool>,
}

/// Installs rustls' ring provider (once is enough; again is harmless).
pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

impl Transport {
    /// Connects to `url`, pinning the server's certificate by SHA-256 when given.
    pub async fn connect(url: &str, cert_hash: Option<Vec<u8>>) -> Result<Transport, String> {
        install_crypto_provider();
        let pin: Option<[u8; 32]> = match cert_hash {
            Some(h) => Some(h.try_into().map_err(|_| "a certificate hash is 32 bytes".to_string())?),
            None => None,
        };
        let build = |bind: IpBindConfig| -> Result<ClientConfig, String> {
            let builder = ClientConfig::builder().with_bind_config(bind);
            Ok(match pin {
                Some(hash) => builder.with_server_certificate_hashes([Sha256Digest::new(hash)]),
                None => builder.with_native_certs(),
            }
            .keep_alive_interval(Some(Duration::from_secs(3)))
            .max_idle_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| e.to_string())?
            .build())
        };
        // Dual-stack where the machine has IPv6, IPv4 where it hasn't.
        let endpoint = match Endpoint::client(build(IpBindConfig::InAddrAnyDual)?) {
            Ok(e) => e,
            Err(_) => Endpoint::client(build(IpBindConfig::InAddrAnyV4)?).map_err(|e| e.to_string())?,
        };
        let conn = endpoint.connect(url).await.map_err(|e| format!("connect: {e}"))?;
        let (mut send, mut recv) = conn
            .open_bi()
            .await
            .map_err(|e| format!("open control stream: {e}"))?
            .await
            .map_err(|e| format!("open control stream: {e}"))?;
        let closed = Arc::new(AtomicBool::new(false));
        let flushed = Arc::new(AtomicBool::new(false));
        let (ctrl_tx, mut ctrl_rx) = mpsc::unbounded_channel::<CtrlOut>();
        let (writer_closed, writer_flushed, writer_conn) = (closed.clone(), flushed.clone(), conn.clone());
        tokio::spawn(async move {
            while let Some(out) = ctrl_rx.recv().await {
                match out {
                    CtrlOut::Bytes(bytes) => {
                        if send.write_all(&bytes).await.is_err() {
                            writer_closed.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                    CtrlOut::Close => {
                        let _ = send.finish().await;
                        writer_closed.store(true, Ordering::Relaxed);
                        writer_conn.close(0u32.into(), b"bye");
                        break;
                    }
                }
            }
            writer_flushed.store(true, Ordering::Relaxed);
        });
        let ctrl_in = Arc::new(Mutex::new(Vec::new()));
        let (into, reader_closed) = (ctrl_in.clone(), closed.clone());
        tokio::spawn(async move {
            let mut chunk = [0u8; 4096];
            loop {
                match recv.read(&mut chunk).await {
                    Ok(Some(n)) => {
                        if let Ok(mut c) = into.lock() {
                            c.extend_from_slice(&chunk[..n]);
                        }
                    }
                    _ => {
                        reader_closed.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            }
        });
        Ok(Transport {
            conn,
            ctrl_tx,
            ctrl_in,
            closed,
            inbox: Arc::new(Mutex::new(VecDeque::new())),
            flushed,
        })
    }

    /// Starts receiving datagrams in the background; each is stamped with `clock()` on arrival.
    pub fn start_pump(&self, clock: fn() -> f64) {
        let (conn, inbox, closed) = (self.conn.clone(), self.inbox.clone(), self.closed.clone());
        crate::platform::spawn(async move {
            loop {
                match conn.receive_datagram().await {
                    Ok(d) => {
                        let Ok(mut q) = inbox.lock() else { break };
                        q.push_back((d.payload().to_vec(), clock()));
                        while q.len() > INBOX {
                            q.pop_front();
                        }
                    }
                    Err(_) => {
                        closed.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            }
        });
    }

    /// Takes every datagram the pump has received, with its arrival time.
    pub fn drain(&self) -> Vec<(Vec<u8>, f64)> {
        self.inbox.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default()
    }

    /// Sends a datagram; `false` if it couldn't go (the oldest are dropped under congestion, never
    /// waited for).
    pub fn send_datagram(&self, bytes: &[u8]) -> bool {
        match self.conn.send_datagram(bytes) {
            Ok(()) => true,
            Err(wtransport::error::SendDatagramError::NotConnected) => {
                self.closed.store(true, Ordering::Relaxed);
                false
            }
            Err(_) => false,
        }
    }

    pub fn send_control(&self, bytes: Vec<u8>) {
        let _ = self.ctrl_tx.send(CtrlOut::Bytes(bytes));
    }

    /// Sends `bye` on the control stream, then closes the session.
    pub fn close_with(&self, bye: Vec<u8>) {
        self.send_control(bye);
        if self.ctrl_tx.send(CtrlOut::Close).is_err() {
            self.close();
        }
    }

    /// Waits (at most `ms`) for the goodbye to go before the program ends.
    pub fn flush_blocking(&self, ms: u64) {
        let until = std::time::Instant::now() + Duration::from_millis(ms);
        while !self.flushed.load(Ordering::Relaxed) && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Closes the session now.
    pub fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
        self.conn.close(0u32.into(), b"");
    }

    /// Takes all control-stream bytes received so far.
    pub fn take_control(&self) -> Vec<u8> {
        self.ctrl_in.lock().map(|mut c| std::mem::take(&mut *c)).unwrap_or_default()
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    pub fn max_datagram_size(&self) -> usize {
        self.conn.max_datagram_size().unwrap_or(1200)
    }
}
