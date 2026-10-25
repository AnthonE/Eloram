//! The records: a recorder thread a shard takes each match's record frames off its ring (the shard
//! thread wrote them as it stepped) and keeps them, the live ones and the last finished ones, for
//! the HTTP side: `/records.json` lists them, `/records/{id}` serves one. A live match's record is
//! served only as far as [`DELAY_TICKS`] behind its newest tick, so a spectator is always that far
//! behind the players, and can't be anything else (`docs/NETCODE.md`, "The record").
//!
//! With a directory (`--records`), a finished record is written there as `{id}.elr`.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use el_match::RecordEnds;
use el_sim::record::{self, Frame};

/// How far behind its players a live match is shown: 60 s.
pub const DELAY_TICKS: u32 = 60 * el_sim::TICK_HZ;
/// Finished records kept in memory.
const KEEP: usize = 32;
/// A live record with no new frame for this long is taken as finished (its match was ended).
const STALE: Duration = Duration::from_secs(30);

pub struct Rec {
    /// The match's seed in hex: what names it.
    pub id: String,
    pub shard: usize,
    pub m: u16,
    pub bytes: Vec<u8>,
    /// How far the frames have been read.
    parsed: usize,
    /// The next tick its match will step, as far as the record goes.
    pub ticks: u32,
    /// Where each hash frame ends, with its tick: where a delayed copy may be cut.
    cuts: Vec<(u32, usize)>,
    pub winner: Option<u8>,
    pub live: bool,
    touched: Instant,
}

impl Rec {
    fn new(shard: usize, m: u16, seed: u64) -> Self {
        Self {
            id: format!("{seed:016x}"),
            shard,
            m,
            bytes: Vec::new(),
            parsed: 0,
            ticks: 0,
            cuts: Vec::new(),
            winner: None,
            live: true,
            touched: Instant::now(),
        }
    }

    /// What may be served of it now: all of a finished one; of a live one, up to the last hash at
    /// least [`DELAY_TICKS`] behind its newest tick.
    pub fn served(&self) -> &[u8] {
        if !self.live {
            return &self.bytes;
        }
        let upto = self.ticks.saturating_sub(DELAY_TICKS);
        let end = self.cuts.iter().rev().find(|(t, _)| *t <= upto).map_or(0, |&(_, at)| at);
        &self.bytes[..end]
    }
}

#[derive(Default)]
pub struct RecordStore {
    /// Each shard's matches' records being written, by match.
    live: Vec<Vec<Option<Rec>>>,
    done: VecDeque<Rec>,
    pub dir: Option<PathBuf>,
    pub written: u64,
    pub broken: u64,
}

impl RecordStore {
    pub fn new(shards: usize, matches: usize, dir: Option<PathBuf>) -> Self {
        Self {
            live: (0..shards).map(|_| (0..matches).map(|_| None).collect()).collect(),
            done: VecDeque::new(),
            dir,
            written: 0,
            broken: 0,
        }
    }

    /// Takes match `m` of shard `k`'s new bytes.
    fn take(&mut self, k: usize, m: usize, new: &[u8]) {
        // A header starts a new record: whatever was being written for this place is finished.
        if let Some(rec) = self.live[k][m].as_mut() {
            rec.bytes.extend_from_slice(new);
            rec.touched = Instant::now();
        } else {
            // Bytes before any header: nothing to hang them on (a recorder started mid-match).
            match record::read_frame(new) {
                Ok(Some((Frame::Header(h), _))) => {
                    let mut rec = Rec::new(k, m as u16, h.seed);
                    rec.bytes.extend_from_slice(new);
                    self.live[k][m] = Some(rec);
                }
                _ => return,
            }
        }
        loop {
            let Some(rec) = self.live[k][m].as_mut() else { return };
            match record::read_frame(&rec.bytes[rec.parsed..]) {
                Ok(Some((f, used))) => {
                    let start = rec.parsed;
                    rec.parsed += used;
                    match f {
                        Frame::Header(h) if start > 0 => {
                            // The next match in this place: what came before is a record of its own.
                            let rest = rec.bytes.split_off(start);
                            let mut next = Rec::new(k, m as u16, h.seed);
                            next.bytes = rest;
                            next.parsed = used;
                            next.cuts.push((0, used));
                            let old = self.live[k][m].replace(next);
                            if let Some(old) = old {
                                self.finish(old);
                            }
                        }
                        // The header is served at once: a spectator sets the match up from it.
                        Frame::Header(_) => rec.cuts.push((0, start + used)),
                        Frame::Tick { tick, .. } => rec.ticks = tick + 1,
                        Frame::Hash { tick, .. } => rec.cuts.push((tick, start + used)),
                        Frame::Outcome { winner, .. } => rec.winner = Some(winner),
                        _ => {}
                    }
                }
                Ok(None) => break,
                Err(_) => {
                    self.broken += 1;
                    if let Some(old) = self.live[k][m].take() {
                        self.finish(old);
                    }
                    return;
                }
            }
        }
    }

    fn finish(&mut self, mut rec: Rec) {
        rec.live = false;
        if let Some(dir) = &self.dir {
            let path = dir.join(format!("{}.elr", rec.id));
            match std::fs::write(&path, &rec.bytes) {
                Ok(()) => self.written += 1,
                Err(e) => tracing::warn!("writing {}: {e}", path.display()),
            }
        }
        tracing::info!(id = rec.id.as_str(), bytes = rec.bytes.len(), ticks = rec.ticks, "record finished");
        self.done.push_back(rec);
        while self.done.len() > KEEP {
            self.done.pop_front();
        }
    }

    /// Finishes what was won (its last frames are in) and what's gone quiet.
    fn sweep(&mut self) {
        for k in 0..self.live.len() {
            for m in 0..self.live[k].len() {
                let over = self.live[k][m]
                    .as_ref()
                    .is_some_and(|r| r.winner.is_some() || r.touched.elapsed() > STALE);
                if over && let Some(rec) = self.live[k][m].take() {
                    self.finish(rec);
                }
            }
        }
    }

    /// Every record kept, the live ones first.
    pub fn list(&self) -> serde_json::Value {
        let row = |r: &Rec| {
            serde_json::json!({
                "id": r.id,
                "live": r.live,
                "ticks": r.ticks,
                "bytes": r.bytes.len(),
                "served_bytes": r.served().len(),
                "winner": r.winner,
            })
        };
        let live = self.live.iter().flatten().flatten().map(row);
        let done = self.done.iter().rev().map(row);
        serde_json::Value::Array(live.chain(done).collect())
    }

    /// Record `id` as it may be served.
    pub fn get(&self, id: &str) -> Option<Vec<u8>> {
        self.live
            .iter()
            .flatten()
            .flatten()
            .chain(self.done.iter())
            .find(|r| r.id == id)
            .map(|r| r.served().to_vec())
    }
}

/// The recorder thread of shard `k`: every tenth of a second, its matches' new record bytes into
/// the store.
pub fn spawn(
    k: usize,
    mut ends: RecordEnds,
    store: Arc<Mutex<RecordStore>>,
    stop: Arc<AtomicBool>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new().name(format!("recorder-{k}")).spawn(move || {
        let mut buf = Vec::with_capacity(el_match::queues::RECORD_RING_BYTES);
        while !stop.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(100));
            let Ok(mut store) = store.lock() else { return };
            for (m, ring) in ends.rings.iter_mut().enumerate() {
                let n = ring.slots();
                if n == 0 {
                    continue;
                }
                buf.clear();
                if let Ok(chunk) = ring.read_chunk(n) {
                    let (a, b) = chunk.as_slices();
                    buf.extend_from_slice(a);
                    buf.extend_from_slice(b);
                    chunk.commit_all();
                }
                store.take(k, m, &buf);
            }
            store.sweep();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use el_sim::game::SeatInput;
    use el_sim::units::SEATS;

    fn frames(seed: u64, from: u32, to: u32, header: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = [0u8; record::MAX_FRAME];
        if header {
            let n = record::write_header(&mut buf, &record::Header::new(seed, [0; SEATS]));
            out.extend_from_slice(&buf[..n]);
        }
        for t in from..to {
            let n = record::write_tick(&mut buf, t, &[SeatInput::default(); SEATS]);
            out.extend_from_slice(&buf[..n]);
            if (t + 1) % record::HASH_EVERY == 0 {
                let n = record::write_hash(&mut buf, t + 1, u64::from(t));
                out.extend_from_slice(&buf[..n]);
            }
        }
        out
    }

    #[test]
    fn a_live_record_is_served_a_minute_behind_and_a_finished_one_whole() {
        let mut s = RecordStore::new(1, 2, None);
        let bytes = frames(7, 0, 3_000, true);
        // In two pieces, the cut falling inside a frame.
        s.take(0, 1, &bytes[..1_001]);
        s.take(0, 1, &bytes[1_001..]);
        let served = s.get(&format!("{:016x}", 7)).unwrap();
        let last_tick = el_sim::record::Frames::new(&served)
            .filter_map(|f| if let Frame::Tick { tick, .. } = f { Some(tick) } else { None })
            .last()
            .unwrap();
        assert!(last_tick < 3_000 - DELAY_TICKS, "served to {last_tick}");
        assert!(last_tick + record::HASH_EVERY >= 3_000 - DELAY_TICKS);
        // The next match in the same place finishes it: then it's served whole.
        s.take(0, 1, &frames(8, 0, 10, true));
        assert_eq!(s.get(&format!("{:016x}", 7)).unwrap(), bytes);
        assert_eq!(s.list().as_array().unwrap().len(), 2);
    }
}
