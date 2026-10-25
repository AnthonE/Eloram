//! Browser WebTransport (via `web-transport-wasm`).
//!
//! A `spawn_local` pump receives datagrams as they arrive and stamps each with its arrival time,
//! so clock sync and RTT do not depend on the frame rate; the game loop drains them once per
//! frame. The rare control-stream traffic runs in two more small tasks.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use futures::StreamExt;
use futures::channel::mpsc;
use web_transport_wasm::Session;

/// Received datagrams with their arrival times (s).
type Inbox = Rc<RefCell<VecDeque<(Vec<u8>, f64)>>>;

/// What the control-stream writer does next.
enum CtrlOut {
    Bytes(Vec<u8>),
    /// Close the session once everything before this is written (a goodbye).
    Close,
}

#[derive(Clone)]
pub struct Transport {
    session: Session,
    ctrl_tx: mpsc::UnboundedSender<CtrlOut>,
    ctrl_in: Rc<RefCell<Vec<u8>>>,
    closed: Rc<Cell<bool>>,
    /// Datagrams stamped with their arrival time (s), filled by an async task so timing does not
    /// depend on the frame rate.
    inbox: Inbox,
}

impl Transport {
    /// Connects to `url`, pinning the server's self-signed certificate by SHA-256 when given.
    pub async fn connect(url: &str, cert_hash: Option<Vec<u8>>) -> Result<Transport, String> {
        let url = url::Url::parse(url).map_err(|e| format!("bad url {url}: {e}"))?;
        // The browser's WebTransport, built here (with the options the crate's `ClientBuilder` sets)
        // so its datagram queue can hold a slow frame's burst of input packets (`Client::poll`): at
        // the browser's default high-water mark it refuses all but the first of them.
        let options = web_sys::WebTransportOptions::new();
        options.set_require_unreliable(true);
        options.set_congestion_control(web_sys::WebTransportCongestionControl::LowLatency);
        if let Some(hash) = cert_hash {
            let entry = web_sys::WebTransportHash::new();
            entry.set_algorithm("sha-256");
            entry.set_value_u8_array(&js_sys::Uint8Array::new_from_slice(&hash));
            options.set_server_certificate_hashes(&[entry]);
        }
        let inner = web_sys::WebTransport::new_with_options(url.as_str(), &options)
            .map_err(|e| format!("connect: {e:?}"))?;
        wasm_bindgen_futures::JsFuture::from(inner.ready()).await.map_err(|e| format!("connect: {e:?}"))?;
        inner.datagrams().set_outgoing_high_water_mark(el_client_core::client::MAX_PACKETS as f64);
        let session = Session::new(inner, url);
        let (mut send, mut recv) =
            session.open_bi().await.map_err(|e| format!("open control stream: {e}"))?;

        let closed = Rc::new(Cell::new(false));
        let (ctrl_tx, mut ctrl_rx) = mpsc::unbounded::<CtrlOut>();
        let writer_closed = closed.clone();
        let writer_session = session.clone();
        wasm_bindgen_futures::spawn_local(async move {
            while let Some(out) = ctrl_rx.next().await {
                match out {
                    CtrlOut::Bytes(bytes) => {
                        if send.write(&bytes).await.is_err() {
                            writer_closed.set(true);
                            break;
                        }
                    }
                    CtrlOut::Close => {
                        writer_closed.set(true);
                        writer_session.close(0, "bye");
                        break;
                    }
                }
            }
        });
        let ctrl_in = Rc::new(RefCell::new(Vec::new()));
        let inbox = ctrl_in.clone();
        let reader_closed = closed.clone();
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                match recv.read(4096).await {
                    Ok(Some(chunk)) => inbox.borrow_mut().extend_from_slice(&chunk),
                    _ => {
                        reader_closed.set(true);
                        break;
                    }
                }
            }
        });
        let inbox = Rc::new(RefCell::new(VecDeque::new()));
        Ok(Transport { session, ctrl_tx, ctrl_in, closed, inbox })
    }

    /// Starts receiving datagrams in the background; each is stamped with `clock()` on arrival.
    pub fn start_pump(&self, clock: fn() -> f64) {
        let session = self.session.clone();
        let inbox = self.inbox.clone();
        let closed = self.closed.clone();
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                match session.recv_datagram().await {
                    Ok(bytes) => {
                        let mut q = inbox.borrow_mut();
                        q.push_back((bytes.to_vec(), clock()));
                        // A stalled tab must not grow this without bound: old snapshots are useless.
                        while q.len() > 512 {
                            q.pop_front();
                        }
                    }
                    Err(_) => {
                        closed.set(true);
                        break;
                    }
                }
            }
        });
    }

    /// Takes every datagram the pump has received, with its arrival time.
    pub fn drain(&self) -> Vec<(Vec<u8>, f64)> {
        self.inbox.borrow_mut().drain(..).collect()
    }

    /// Hands a datagram to the browser; `false` if it had no room (the datagram is dropped).
    pub fn send_datagram(&self, bytes: &[u8]) -> bool {
        match self.session.try_send_datagram(bytes) {
            Ok(sent) => sent,
            Err(_) => {
                self.closed.set(true);
                false
            }
        }
    }

    pub fn send_control(&self, bytes: Vec<u8>) {
        let _ = self.ctrl_tx.unbounded_send(CtrlOut::Bytes(bytes));
    }

    /// Sends `bye` on the control stream, then closes the session.
    pub fn close_with(&self, bye: Vec<u8>) {
        self.send_control(bye);
        if self.ctrl_tx.unbounded_send(CtrlOut::Close).is_err() {
            self.close();
        }
    }

    /// Closes the session now.
    pub fn close(&self) {
        self.closed.set(true);
        self.session.close(0, "");
    }

    /// Takes all control-stream bytes received so far.
    pub fn take_control(&self) -> Vec<u8> {
        std::mem::take(&mut *self.ctrl_in.borrow_mut())
    }

    pub fn is_closed(&self) -> bool {
        self.closed.get()
    }

    pub fn max_datagram_size(&self) -> usize {
        self.session.max_datagram_size()
    }
}
