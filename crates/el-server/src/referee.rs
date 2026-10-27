//! The referee: each live match's record re-stepped as it is written, every hash checked, and once
//! a minute the shard's checkpoint compared byte for byte with the referee's own
//! (`docs/NETCODE.md`, "The record").
//!
//! - It runs on its shard's recorder thread, outside the store's lock, one [`Referee`] a match
//!   place and one path scratch between them.
//! - A checkpoint is kept (served to spectators, written beside the record for a restart) only once
//!   the referee has re-stepped the record to it and found the same bytes: a checkpoint is a cache
//!   of the record, never a truth of its own.
//! - A hash that differs says when a match went astray (the 30 ticks since the last good one);
//!   the referee steps on to the next checkpoint, where comparing the two matches says what went
//!   astray: the unit, or the champion, and its field. Riot's referee does the same with a state
//!   log and a diff, since "only the first divergence really matters".

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use el_match::queues::checkpoint_due;
use el_sim::map::Map;
use el_sim::path::PathScratch;
use el_sim::record::{self, Divergence, Frame, Referee};
use el_sim::units::{Kind, SEATS};
use el_sim::{Match, snapshot};

/// What the referees have done, for `/status`.
#[derive(Default)]
pub struct RefereeStats {
    pub ticks: AtomicU64,
    pub hashes: AtomicU64,
    pub checkpoints: AtomicU64,
    pub divergences: AtomicU64,
    /// Time spent re-stepping, in µs.
    pub busy_us: AtomicU64,
}

impl RefereeStats {
    pub fn json(&self) -> serde_json::Value {
        let r = |c: &AtomicU64| c.load(Ordering::Relaxed);
        serde_json::json!({
            "ticks": r(&self.ticks),
            "hashes": r(&self.hashes),
            "checkpoints": r(&self.checkpoints),
            "divergences": r(&self.divergences),
            "busy_us": r(&self.busy_us),
        })
    }
}

/// What a pass found, for the store: a checkpoint to keep, a match gone astray.
pub enum Verdict {
    Checkpoint { m: usize, id: String, tick: u32, bytes: Arc<[u8]> },
    Astray { m: usize, id: String, why: String },
}

enum State {
    /// No header yet (a recorder that started mid-match), or the match is done with.
    Idle,
    Judging,
    /// A hash differed at this tick: on to the next checkpoint, to say what.
    Diverged {
        tick: u32,
        recorded: u64,
        replayed: u64,
    },
    Astray,
}

struct Place {
    id: String,
    r: Referee,
    state: State,
    /// Bytes not yet a whole frame.
    carry: Vec<u8>,
    /// The referee's own checkpoint at a due tick, waiting for the shard's.
    mine: Option<(u32, Vec<u8>)>,
    /// The shard's, come before the referee reached its tick.
    theirs: Option<(u32, Vec<u8>)>,
}

/// One shard's referees.
pub struct Referees {
    places: Vec<Place>,
    scratch: PathScratch,
    map: &'static Map,
    stats: Arc<RefereeStats>,
    /// Ticks between a match's checkpoints, as its shard writes them.
    every: u32,
    /// The two matches a divergence is named from, built the first time one is.
    spares: Option<Box<(Match, Match)>>,
}

impl Referees {
    pub fn new(map: &'static Map, matches: usize, every: u32, stats: Arc<RefereeStats>) -> Self {
        Self {
            places: (0..matches)
                .map(|_| Place {
                    id: String::new(),
                    r: Referee::sharing(map),
                    state: State::Idle,
                    carry: Vec::new(),
                    mine: None,
                    theirs: None,
                })
                .collect(),
            scratch: PathScratch::new(),
            map,
            stats,
            every,
            spares: None,
        }
    }

    /// Hands place `m` a referee already at its record's end (a match recovered after a restart,
    /// `crate::recover`): it goes on from there.
    pub fn adopt(&mut self, m: usize, id: &str, r: Referee) {
        let p = &mut self.places[m];
        p.id = id.to_string();
        p.r = r;
        p.state = State::Judging;
        p.carry.clear();
        p.mine = None;
        p.theirs = None;
    }

    /// Match `m`'s new record bytes.
    pub fn feed(&mut self, m: usize, bytes: &[u8], out: &mut Vec<Verdict>) {
        let started = Instant::now();
        let mut carry = std::mem::take(&mut self.places[m].carry);
        carry.extend_from_slice(bytes);
        let mut at = 0;
        let mut ticks = 0;
        loop {
            match record::read_frame(&carry[at..]) {
                Ok(Some((f, used))) => {
                    at += used;
                    if matches!(f, Frame::Tick { .. }) {
                        ticks += 1;
                    }
                    self.frame(m, &f, out);
                }
                Ok(None) => break,
                Err(e) => {
                    self.astray(m, format!("an unreadable frame: {e:?}"), out);
                    at = carry.len();
                    break;
                }
            }
        }
        carry.drain(..at);
        self.places[m].carry = carry;
        self.stats.ticks.fetch_add(ticks, Ordering::Relaxed);
        self.stats.busy_us.fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
    }

    fn frame(&mut self, m: usize, f: &Frame, out: &mut Vec<Verdict>) {
        let p = &mut self.places[m];
        if let Frame::Header(h) = f {
            p.id = format!("{:016x}", h.seed);
            p.state = State::Judging;
            p.mine = None;
            p.theirs = None;
        }
        if matches!(p.state, State::Idle | State::Astray) {
            return;
        }
        match p.r.take_with(f, &mut self.scratch) {
            Ok(()) => {}
            Err(Divergence::Hash { tick, recorded, replayed }) if matches!(p.state, State::Judging) => {
                self.stats.divergences.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(
                    id = p.id.as_str(),
                    tick,
                    "the record's hash differs: stepping on to the next checkpoint"
                );
                p.state = State::Diverged { tick, recorded, replayed };
            }
            // Already astray: it's the first that matters.
            Err(Divergence::Hash { .. }) => {}
            Err(e) => return self.astray(m, format!("{e:?}"), out),
        }
        let Frame::Hash { tick, .. } = *f else { return };
        if matches!(p.state, State::Judging) {
            self.stats.hashes.fetch_add(1, Ordering::Relaxed);
        }
        if !checkpoint_due(m, tick, self.every) {
            return;
        }
        let mut mine = vec![0u8; snapshot::len(&p.r.m)];
        let n = p.r.checkpoint(&mut mine);
        mine.truncate(n);
        match p.theirs.take() {
            Some((t, theirs)) if t == tick => self.compare(m, tick, mine, theirs, out),
            _ => p.mine = Some((tick, mine)),
        }
    }

    /// The shard's checkpoint for match `m`.
    pub fn checkpoint(&mut self, m: usize, bytes: &[u8], out: &mut Vec<Verdict>) {
        let Some(p) = self.places.get_mut(m) else { return };
        let Some(tick) = snapshot::tick_of(bytes) else { return };
        if matches!(p.state, State::Idle | State::Astray) {
            return;
        }
        match p.mine.take() {
            Some((t, mine)) if t == tick => self.compare(m, tick, mine, bytes.to_vec(), out),
            mine => {
                p.mine = mine;
                // Not there yet: wait for it. Past it (a place adopted after a restart): let it go.
                if p.r.m.tick < tick || p.r.skipping() {
                    p.theirs = Some((tick, bytes.to_vec()));
                }
            }
        }
    }

    fn compare(&mut self, m: usize, tick: u32, mine: Vec<u8>, theirs: Vec<u8>, out: &mut Vec<Verdict>) {
        let diverged = match self.places[m].state {
            State::Diverged { tick, recorded, replayed } => Some((tick, recorded, replayed)),
            _ => None,
        };
        if mine == theirs && diverged.is_none() {
            self.stats.checkpoints.fetch_add(1, Ordering::Relaxed);
            let id = self.places[m].id.clone();
            out.push(Verdict::Checkpoint { m, id, tick, bytes: theirs.into() });
            return;
        }
        let what = self.name(&mine, &theirs).unwrap_or_else(|| "the same checkpoint".into());
        let why = match diverged {
            Some((at, recorded, replayed)) => format!(
                "the hash at tick {at} differs (recorded {recorded:#018x}, re-stepped {replayed:#018x}): \
                 something stepped differently in ticks {}..{at}; at the checkpoint at {tick}, {what}",
                at.saturating_sub(record::HASH_EVERY)
            ),
            None => format!("the checkpoint at tick {tick} differs from the record's: {what}"),
        };
        if diverged.is_none() {
            self.stats.divergences.fetch_add(1, Ordering::Relaxed);
        }
        self.astray(m, why, out);
    }

    /// What differs between two checkpoints, in words.
    fn name(&mut self, mine: &[u8], theirs: &[u8]) -> Option<String> {
        let map = self.map;
        let spares = self.spares.get_or_insert_with(|| {
            Box::new((Match::new(map, 0, [0; SEATS]), Match::new(map, 0, [0; SEATS])))
        });
        if let Err(e) = snapshot::read(theirs, &mut spares.0) {
            return Some(format!("the shard's checkpoint won't read ({e})"));
        }
        snapshot::read(mine, &mut spares.1).ok()?;
        let w = snapshot::diff(&spares.0, &spares.1)?;
        let mut what = format!("{w} differs");
        if let (true, Some(slot)) = (w.part == "unit", w.slot) {
            let u = &spares.0.units[usize::from(slot)];
            let kind = if u.kind == Kind::Champion { "champion" } else { kind_name(u.kind) };
            what = format!("{w} ({kind}, team {}) differs", u.team);
        }
        Some(what)
    }

    fn astray(&mut self, m: usize, why: String, out: &mut Vec<Verdict>) {
        let p = &mut self.places[m];
        tracing::error!(id = p.id.as_str(), "the referee: {why}");
        p.state = State::Astray;
        p.mine = None;
        p.theirs = None;
        out.push(Verdict::Astray { m, id: p.id.clone(), why });
    }
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Champion => "champion",
        Kind::Melee => "melee minion",
        Kind::Caster => "caster minion",
        Kind::Siege => "siege minion",
        Kind::Super => "super minion",
        Kind::Turret => "turret",
        Kind::Inhibitor => "inhibitor",
        Kind::Nexus => "nexus",
        Kind::Fountain => "fountain",
    }
}
