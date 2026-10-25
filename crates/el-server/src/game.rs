//! The matches: shard threads stepping them (`el_match`), an egress thread a shard sending the
//! datagrams they queue, and each seat's lease (the right to feed it inputs), kept here while no
//! session holds it (`docs/NETCODE.md`, "Many matches a process").
//!
//! A shard thread never touches tokio: it unparks its egress thread after every tick, and the
//! egress thread hands each datagram to its seat's connection, which sends without awaiting.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_queue::ArrayQueue;
use el_match::{EgressEnds, SeatLease, ShardConfig, ShardShared, ShardThread, build, read_packet};

use crate::records::RecordStore;
use el_sim::map::Map;
use el_sim::units::SEATS;
use wtransport::Connection;

/// What a session tells its shard's egress thread.
enum EgressCmd {
    /// Seat `g`'s datagrams go to `conn` from now.
    Attach { g: u32, conn: Connection },
    /// And no longer.
    Detach { g: u32 },
}

#[derive(Default)]
pub struct EgressStats {
    pub datagrams: AtomicU64,
    pub bytes: AtomicU64,
    /// Datagrams the connection refused (gone, or too big for its path).
    pub refused: AtomicU64,
}

struct Egress {
    cmds: ArrayQueue<EgressCmd>,
    stop: AtomicBool,
    stats: EgressStats,
}

/// One shard: its thread, its egress thread, its seats' leases.
pub struct Shard {
    pub shared: Arc<ShardShared>,
    thread: Option<ShardThread>,
    egress: Arc<Egress>,
    egress_thread: Option<JoinHandle<()>>,
    leases: Mutex<Vec<Option<SeatLease>>>,
}

impl Shard {
    pub fn egress_stats(&self) -> &EgressStats {
        &self.egress.stats
    }
}

/// Every shard of the process.
pub struct Game {
    pub shards: Vec<Shard>,
    pub matches_per_shard: usize,
    /// Every match's record, live and finished.
    pub records: Arc<Mutex<RecordStore>>,
    recorders: Vec<JoinHandle<()>>,
    recorders_stop: Arc<AtomicBool>,
}

impl Game {
    /// Starts `shards` shard threads of `matches` matches each (with their egress and recorder
    /// threads), writing finished records to `records_dir` if there is one.
    pub fn start(
        shards: usize,
        matches: usize,
        map: &'static Map,
        records_dir: Option<std::path::PathBuf>,
    ) -> std::io::Result<Game> {
        let mut out = Vec::with_capacity(shards);
        let records = Arc::new(Mutex::new(RecordStore::new(shards, matches, records_dir)));
        let recorders_stop = Arc::new(AtomicBool::new(false));
        let mut recorders = Vec::with_capacity(shards);
        for k in 0..shards {
            let (shard, shared, leases, ends, record_ends) =
                build(ShardConfig { matches, map, hot_guard: Some(el_alloc::set_hot) });
            recorders.push(crate::records::spawn(k, record_ends, records.clone(), recorders_stop.clone())?);
            let egress = Arc::new(Egress {
                cmds: ArrayQueue::new(matches * SEATS * 2 + 16),
                stop: AtomicBool::new(false),
                stats: EgressStats::default(),
            });
            let e = egress.clone();
            let egress_thread =
                thread::Builder::new().name(format!("egress-{k}")).spawn(move || egress_loop(ends, &e))?;
            let thread = el_match::spawn(shard, Some(egress_thread.thread().clone()), &format!("shard-{k}"))?;
            out.push(Shard {
                shared,
                thread: Some(thread),
                egress,
                egress_thread: Some(egress_thread),
                leases: Mutex::new(leases.into_iter().map(Some).collect()),
            });
        }
        Ok(Game { shards: out, matches_per_shard: matches, records, recorders, recorders_stop })
    }

    /// Takes seat `g`'s lease from shard `k` (none if a session holds it already).
    pub fn take_lease(&self, k: usize, g: u32) -> Option<SeatLease> {
        self.shards.get(k)?.leases.lock().ok()?.get_mut(g as usize)?.take()
    }

    /// Gives a lease back.
    pub fn give_lease(&self, k: usize, lease: SeatLease) {
        if let Some(s) = self.shards.get(k)
            && let Ok(mut l) = s.leases.lock()
        {
            let g = lease.g as usize;
            l[g] = Some(lease);
        }
    }

    /// Seat `g` of shard `k` is sent to `conn`.
    pub fn attach(&self, k: usize, g: u32, conn: Connection) {
        if let Some(s) = self.shards.get(k) {
            let _ = s.egress.cmds.push(EgressCmd::Attach { g, conn });
        }
    }

    pub fn detach(&self, k: usize, g: u32) {
        if let Some(s) = self.shards.get(k) {
            let _ = s.egress.cmds.push(EgressCmd::Detach { g });
        }
    }

    /// Tells every thread to stop, without waiting for them.
    pub fn halt(&self) {
        self.recorders_stop.store(true, Ordering::Release);
        for s in &self.shards {
            s.shared.stop.store(true, Ordering::Release);
            s.egress.stop.store(true, Ordering::Release);
            if let Some(t) = &s.egress_thread {
                t.thread().unpark();
            }
        }
    }

    /// Stops every thread and waits for them.
    pub fn stop(&mut self) {
        self.recorders_stop.store(true, Ordering::Release);
        for t in self.recorders.drain(..) {
            let _ = t.join();
        }
        for s in &mut self.shards {
            if let Some(t) = s.thread.take() {
                t.stop();
            }
            s.egress.stop.store(true, Ordering::Release);
            if let Some(t) = s.egress_thread.take() {
                t.thread().unpark();
                let _ = t.join();
            }
        }
    }
}

impl Drop for Game {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The egress thread: woken after each tick, it sends every queued datagram to its seat's
/// connection (or drops it, for a seat no one is attached to).
fn egress_loop(mut ends: EgressEnds, e: &Egress) {
    let mut sinks: Vec<Option<Connection>> = (0..ends.rings.len()).map(|_| None).collect();
    let mut buf = [0u8; el_proto::MAX_DATAGRAM + 64];
    while !e.stop.load(Ordering::Acquire) {
        thread::park_timeout(Duration::from_millis(50));
        while let Some(c) = e.cmds.pop() {
            match c {
                EgressCmd::Attach { g, conn } => sinks[g as usize] = Some(conn),
                EgressCmd::Detach { g } => sinks[g as usize] = None,
            }
        }
        for (g, ring) in ends.rings.iter_mut().enumerate() {
            while let Some(n) = read_packet(ring, &mut buf) {
                let Some(conn) = &sinks[g] else { continue };
                if conn.send_datagram(&buf[..n]).is_ok() {
                    e.stats.datagrams.fetch_add(1, Ordering::Relaxed);
                    e.stats.bytes.fetch_add(n as u64, Ordering::Relaxed);
                } else {
                    e.stats.refused.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}
