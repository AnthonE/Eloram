//! The records: a recorder thread a shard takes each match's record frames off its ring (the shard
//! thread wrote them as it stepped) and keeps them, the live ones and the last finished ones, for
//! the HTTP side: `/records.json` lists them, `/records/{id}` serves one. A live match's record is
//! served only as far as [`DELAY_TICKS`] behind its newest tick, so a spectator is always that far
//! behind the players, and can't be anything else (`docs/NETCODE.md`, "The record").
//!
//! With a directory (`--records`), a live record is written there as it comes, `{id}.elr.part`,
//! renamed `{id}.elr` once it's finished, and the referee's last verified checkpoint beside it,
//! `{id}.ck`: what a restart recovers a match from (`crate::recover`). A server stopped with
//! matches running leaves their parts for the next one to take up.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use el_match::RecordEnds;
use el_match::queues::read_checkpoint;
use el_sim::record::{self, Frame};

use crate::referee::{Referees, Verdict};

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
    /// The checkpoints the referee verified, oldest first, with their ticks: the newest (what a
    /// restart takes up), and back to the newest a spectator may be given (served a minute
    /// behind, as the record is). Older ones go.
    pub checkpoints: Vec<(u32, Arc<[u8]>)>,
    /// Why the referee says it went astray, if it did.
    pub astray: Option<String>,
    /// Its part on disk (with a directory), and how much of it is written: whole frames only.
    part: Option<File>,
    on_disk: usize,
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
            checkpoints: Vec::new(),
            astray: None,
            part: None,
            on_disk: 0,
        }
    }

    /// A record taken up from its part after a restart (`crate::recover`): `bytes` cut at the
    /// last hash, already on disk as `part`.
    pub fn resumed(shard: usize, m: u16, seed: u64, bytes: Vec<u8>, part: Option<File>) -> Self {
        let mut r = Self::new(shard, m, seed);
        r.on_disk = bytes.len();
        r.part = part;
        r.bytes = bytes;
        let mut at = 0;
        while let Ok(Some((f, used))) = record::read_frame(&r.bytes[at..]) {
            match f {
                Frame::Header(_) => r.cuts.push((0, at + used)),
                Frame::Tick { tick, .. } => r.ticks = tick + 1,
                Frame::Hash { tick, .. } => r.cuts.push((tick, at + used)),
                Frame::Outcome { winner, .. } => r.winner = Some(winner),
                _ => {}
            }
            at += used;
        }
        r.parsed = at;
        r
    }

    /// The tick the served copy ends at.
    fn served_tick(&self) -> u32 {
        if !self.live {
            return u32::MAX;
        }
        let upto = self.ticks.saturating_sub(DELAY_TICKS);
        self.cuts.iter().rev().find(|(t, _)| *t <= upto).map_or(0, |&(t, _)| t)
    }

    /// The checkpoint a spectator may start from: the last verified one inside what's served.
    pub fn served_checkpoint(&self) -> Option<Arc<[u8]>> {
        let upto = self.served_tick();
        self.checkpoints.iter().rev().find(|(t, _)| *t <= upto).map(|(_, b)| b.clone())
    }

    /// That checkpoint's tick.
    fn served_checkpoint_tick(&self) -> Option<u32> {
        let upto = self.served_tick();
        self.checkpoints.iter().rev().map(|(t, _)| *t).find(|t| *t <= upto)
    }

    /// A checkpoint verified: kept, and those older than the one a spectator would be given let go.
    pub fn keep_checkpoint(&mut self, tick: u32, bytes: Arc<[u8]>) {
        self.checkpoints.push((tick, bytes));
        let upto = self.served_tick();
        if let Some(k) = self.checkpoints.iter().rposition(|(t, _)| *t <= upto) {
            self.checkpoints.drain(..k);
        }
    }

    /// Writes the whole frames read so far to its part.
    fn write_part(&mut self) {
        if let Some(f) = self.part.as_mut()
            && self.on_disk < self.parsed
        {
            if let Err(e) = f.write_all(&self.bytes[self.on_disk..self.parsed]) {
                tracing::warn!(id = self.id.as_str(), "writing the record's part: {e}");
                self.part = None;
            }
            self.on_disk = self.parsed;
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

    /// Where record `id`'s files go, with `ext`.
    fn path(&self, id: &str, ext: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(format!("{id}.{ext}")))
    }

    /// A new live record: its part opened on disk, if there's a directory.
    fn open(&self, mut rec: Rec) -> Rec {
        if let Some(path) = self.path(&rec.id, "elr.part") {
            match File::create(&path) {
                Ok(f) => rec.part = Some(f),
                Err(e) => tracing::warn!("{}: {e}", path.display()),
            }
        }
        rec
    }

    /// Places record `rec` (recovered after a restart) as match `m` of shard `k`'s live record.
    pub fn resume(&mut self, k: usize, m: usize, rec: Rec) {
        self.live[k][m] = Some(rec);
    }

    /// What the referee found.
    fn verdict(&mut self, k: usize, v: Verdict) {
        let (m, id) = match &v {
            Verdict::Checkpoint { m, id, .. } | Verdict::Astray { m, id, .. } => (*m, id.clone()),
        };
        let ck_path = self.path(&id, "ck");
        let rec = match self.live[k].get_mut(m).and_then(|r| r.as_mut()).filter(|r| r.id == id) {
            Some(r) => r,
            None => match self.done.iter_mut().find(|r| r.id == id) {
                Some(r) => r,
                None => return,
            },
        };
        match v {
            Verdict::Checkpoint { tick, bytes, .. } => {
                if let Some(path) = ck_path {
                    write_atomically(&path, &bytes);
                }
                rec.keep_checkpoint(tick, bytes);
            }
            Verdict::Astray { why, .. } => rec.astray = Some(why),
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
                    let mut rec = self.open(Rec::new(k, m as u16, h.seed));
                    rec.bytes.extend_from_slice(new);
                    self.live[k][m] = Some(rec);
                }
                _ => return,
            }
        }
        loop {
            let Some(rec) = self.live[k][m].as_mut() else { return };
            match record::read_frame(&rec.bytes[rec.parsed..]) {
                Ok(Some((Frame::Header(h), used))) if rec.parsed > 0 => {
                    // The next match in this place: what came before is a record of its own.
                    rec.write_part();
                    let start = rec.parsed;
                    let rest = rec.bytes.split_off(start);
                    let mut next = self.open(Rec::new(k, m as u16, h.seed));
                    next.bytes = rest;
                    next.parsed = used;
                    next.cuts.push((0, used));
                    let old = self.live[k][m].replace(next);
                    if let Some(old) = old {
                        self.finish(old);
                    }
                }
                Ok(Some((f, used))) => {
                    let start = rec.parsed;
                    rec.parsed += used;
                    match f {
                        // The header is served at once: a spectator sets the match up from it.
                        Frame::Header(_) => rec.cuts.push((0, start + used)),
                        Frame::Tick { tick, .. } => rec.ticks = tick + 1,
                        Frame::Hash { tick, .. } => rec.cuts.push((tick, start + used)),
                        Frame::Outcome { winner, .. } => rec.winner = Some(winner),
                        _ => {}
                    }
                }
                Ok(None) => {
                    rec.write_part();
                    break;
                }
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
        if let (Some(path), Some(part)) = (self.path(&rec.id, "elr"), self.path(&rec.id, "elr.part")) {
            // The part has every whole frame; a record cut inside one (it broke) is written whole
            // as far as it goes.
            rec.write_part();
            let done = match rec.part.take() {
                Some(f) if rec.on_disk == rec.bytes.len() => {
                    f.sync_all().and_then(|()| std::fs::rename(&part, &path))
                }
                _ => std::fs::write(&path, &rec.bytes).and_then(|()| std::fs::remove_file(&part).or(Ok(()))),
            };
            match done {
                Ok(()) => self.written += 1,
                Err(e) => tracing::warn!("writing {}: {e}", path.display()),
            }
            if let Some(live) = self.path(&rec.id, "live") {
                let _ = std::fs::remove_file(live);
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
                "checkpoint": r.checkpoints.last().map(|(t, _)| *t),
                "served_checkpoint": r.served_checkpoint_tick(),
                "astray": r.astray,
            })
        };
        let live = self.live.iter().flatten().flatten().map(row);
        let done = self.done.iter().rev().map(row);
        serde_json::Value::Array(live.chain(done).collect())
    }

    fn find(&self, id: &str) -> Option<&Rec> {
        self.live.iter().flatten().flatten().chain(self.done.iter()).find(|r| r.id == id)
    }

    /// Record `id` as it may be served.
    pub fn get(&self, id: &str) -> Option<Vec<u8>> {
        self.find(id).map(|r| r.served().to_vec())
    }

    /// The checkpoint record `id`'s spectators may start from.
    pub fn checkpoint(&self, id: &str) -> Option<Arc<[u8]>> {
        self.find(id)?.served_checkpoint()
    }

    /// Records with a referee's complaint, live or kept.
    pub fn astray(&self) -> usize {
        self.live.iter().flatten().flatten().chain(self.done.iter()).filter(|r| r.astray.is_some()).count()
    }
}

/// Writes `bytes` to `path` through a temporary file and a rename: a reader sees the old file or
/// the new one, never half of one.
pub fn write_atomically(path: &Path, bytes: &[u8]) {
    let tmp = path.with_extension("tmp");
    let done = File::create(&tmp)
        .and_then(|mut f| f.write_all(bytes).and_then(|()| f.sync_data()))
        .and_then(|()| std::fs::rename(&tmp, path));
    if let Err(e) = done {
        tracing::warn!("{}: {e}", path.display());
    }
}

/// The recorder thread of shard `k`: every tenth of a second, its matches' new record bytes
/// through the referee (if there is one) and into the store, and the shard's checkpoints to the
/// referee. Stopped, it takes what's left on the rings once more, and leaves the live records'
/// parts for a restart.
pub fn spawn(
    k: usize,
    mut ends: RecordEnds,
    store: Arc<Mutex<RecordStore>>,
    mut referees: Option<Referees>,
    stop: Arc<AtomicBool>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new().name(format!("recorder-{k}")).spawn(move || {
        let mut bufs: Vec<Vec<u8>> = (0..ends.rings.len()).map(|_| Vec::new()).collect();
        let mut snap = vec![0u8; ends.checkpoints.buffer().capacity()];
        let mut verdicts = Vec::new();
        loop {
            let last = stop.load(Ordering::Acquire);
            if !last {
                std::thread::sleep(Duration::from_millis(100));
            }
            for (ring, buf) in ends.rings.iter_mut().zip(&mut bufs) {
                buf.clear();
                let n = ring.slots();
                if n == 0 {
                    continue;
                }
                if let Ok(chunk) = ring.read_chunk(n) {
                    let (a, b) = chunk.as_slices();
                    buf.extend_from_slice(a);
                    buf.extend_from_slice(b);
                    chunk.commit_all();
                }
            }
            if let Some(r) = referees.as_mut() {
                for (m, buf) in bufs.iter().enumerate().filter(|(_, b)| !b.is_empty()) {
                    r.feed(m, buf, &mut verdicts);
                }
            }
            while let Some((m, n)) = read_checkpoint(&mut ends.checkpoints, &mut snap) {
                if let Some(r) = referees.as_mut() {
                    r.checkpoint(usize::from(m), &snap[..n], &mut verdicts);
                }
            }
            let Ok(mut store) = store.lock() else { return };
            for (m, buf) in bufs.iter().enumerate().filter(|(_, b)| !b.is_empty()) {
                store.take(k, m, buf);
            }
            for v in verdicts.drain(..) {
                store.verdict(k, v);
            }
            if last {
                return;
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

    #[test]
    fn a_spectator_gets_the_newest_checkpoint_a_minute_behind_and_older_ones_go() {
        let mut s = RecordStore::new(1, 1, None);
        s.take(0, 0, &frames(9, 0, 3_000, true));
        let id = format!("{:016x}", 9);
        let ck = |t: u32| -> Arc<[u8]> { vec![t as u8; 4].into() };
        let rec = s.live[0][0].as_mut().unwrap();
        // Served to the hash at or before 3,000 - 1,800 = 1,200.
        for t in [300, 600, 900, 1_200, 1_500, 1_800, 2_100] {
            rec.keep_checkpoint(t, ck(t));
        }
        let ticks: Vec<u32> = rec.checkpoints.iter().map(|(t, _)| *t).collect();
        assert_eq!(ticks, [1_200, 1_500, 1_800, 2_100], "the newest served one and those after it");
        assert_eq!(s.checkpoint(&id).as_deref(), Some(&ck(1_200)[..]));
    }
}
