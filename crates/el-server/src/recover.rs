//! Taking up, after a restart, the matches the last server left running (`docs/NETCODE.md`, "The
//! record"): the record re-stepped, as Riot's Chronobreak does, and the match carried on from its
//! last hash.
//!
//! - In the records directory a live match has its record's part (`{id}.elr.part`), the referee's
//!   last verified checkpoint (`{id}.ck`) and what the lobby told its players with their rejoin
//!   tokens' hashes (`{id}.live`).
//! - The record is re-stepped from the checkpoint (or its first tick) to its last hash, and cut
//!   there: at most a second of play is lost. Every hash on the way is checked.
//! - The match goes into a free place, its bots playing every seat, and the lobby keeps it
//!   [`crate::lobby::REJOIN_FOR`] for its players, who come back with their rejoin tokens as
//!   after any dropped link. The augment keys already drawn are in the record; screens not yet
//!   opened draw from a new secret.
//! - A match whose record ends in an outcome, or that can't be re-stepped, or has no lobby file, or
//!   no place, is finished as it stands (its record kept as `{id}.elr`).

use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};

use el_client_core::lobby::ToClient;
use el_sim::map::Map;
use el_sim::path::PathScratch;
use el_sim::record::{self, Frame, Referee};
use el_sim::snapshot;
use el_sim::units::SEATS;
use serde::{Deserialize, Serialize};

use crate::records::{Rec, RecordStore};
use crate::referee::Referees;

/// What the lobby keeps of a live match, beside its record (`{id}.live`): what its players were
/// told, and each seat's rejoin token's keccak-256 (never the token itself).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveFile {
    pub told: ToClient,
    /// Hex, one a seat; empty for a seat a bot held from the start.
    pub tokens: Vec<String>,
}

impl LiveFile {
    pub fn path(dir: &Path, id: &str) -> PathBuf {
        dir.join(format!("{id}.live"))
    }

    pub fn write(&self, dir: &Path, id: &str) {
        match serde_json::to_vec(self) {
            Ok(b) => crate::records::write_atomically(&Self::path(dir, id), &b),
            Err(e) => tracing::warn!("{id}.live: {e}"),
        }
    }

    pub fn read(dir: &Path, id: &str) -> Option<LiveFile> {
        let f = LiveFile::path(dir, id);
        serde_json::from_slice(&std::fs::read(f).ok()?).ok()
    }

    pub fn token_hashes(&self) -> [[u8; 32]; SEATS] {
        let mut out = [[0u8; 32]; SEATS];
        for (o, t) in out.iter_mut().zip(&self.tokens) {
            if let Ok(b) = hex::decode(t)
                && let Ok(h) = <[u8; 32]>::try_from(b.as_slice())
            {
                *o = h;
            }
        }
        out
    }
}

/// A match taken up: where it went, and what the lobby needs to keep it for its players.
pub struct Recovered {
    pub shard: usize,
    pub m: u16,
    pub id: String,
    pub tick: u32,
    pub live: LiveFile,
}

/// What a part re-stepped came to.
struct Stepped {
    referee: Referee,
    seed: u64,
    /// The bytes up to the end of the last hash.
    cut: usize,
    over: bool,
    /// The checkpoint it started from, if one was good.
    checkpoint: Option<(u32, Vec<u8>)>,
}

/// Re-steps a part: from its checkpoint if there's a good one, else from its first tick, to its
/// last hash. An error names what went wrong.
fn restep(
    map: &'static Map,
    bytes: &[u8],
    ck: Option<Vec<u8>>,
    scratch: &mut PathScratch,
) -> Result<Stepped, String> {
    // Where the last hash ends, and whether an outcome came: the cut.
    let (mut at, mut cut, mut over, mut seed) = (0, 0, false, None);
    while let Ok(Some((f, used))) = record::read_frame(&bytes[at..]) {
        at += used;
        match f {
            Frame::Header(h) => {
                seed = Some(h.seed);
                cut = at;
            }
            Frame::Hash { .. } => cut = at,
            Frame::Outcome { .. } => over = true,
            _ => {}
        }
    }
    let seed = seed.ok_or("no header")?;
    let mut referee = Referee::sharing(map);
    let checkpoint = ck.and_then(|ck| {
        let tick = snapshot::tick_of(&ck)?;
        match referee.resume(&ck) {
            Ok(()) => Some((tick, ck)),
            Err(e) => {
                tracing::warn!("a checkpoint passed over: {e}");
                referee = Referee::sharing(map);
                None
            }
        }
    });
    let mut at = 0;
    while at < cut {
        let Ok(Some((f, used))) = record::read_frame(&bytes[at..cut]) else { break };
        at += used;
        referee.take_with(&f, scratch).map_err(|e| format!("re-stepping: {e:?}"))?;
    }
    if referee.skipping() {
        return Err("the record ends before its checkpoint".into());
    }
    Ok(Stepped { referee, seed, cut, over, checkpoint })
}

/// Takes up every match `dir` holds a part of, into the shards' free places (before their
/// threads start), the store, and the referees. Returns the ones taken up, for the lobby.
pub fn recover(
    dir: &Path,
    map: &'static Map,
    shards: &mut [el_match::Shard],
    store: &mut RecordStore,
    referees: &mut [Option<Referees>],
) -> Vec<Recovered> {
    let mut parts: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(d) => {
            d.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".elr.part")).collect()
        }
        Err(_) => return Vec::new(),
    };
    parts.sort();
    let mut free = shards
        .iter()
        .enumerate()
        .flat_map(|(k, s)| (0..s.config().matches).map(move |m| (k, m)))
        .collect::<Vec<_>>()
        .into_iter();
    let mut scratch = PathScratch::new();
    let mut out = Vec::new();
    for part in parts {
        let name = part.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let id = name.trim_end_matches(".elr.part").to_string();
        let started = std::time::Instant::now();
        let Ok(bytes) = std::fs::read(&part) else { continue };
        let ck = std::fs::read(dir.join(format!("{id}.ck"))).ok();
        // A match whose re-step panics is finished, not taken up (it would panic again).
        let stepped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // A checkpoint that won't take the record to its end is passed over for the record alone.
            restep(map, &bytes, ck.clone(), &mut scratch).or_else(|e| match ck {
                Some(_) => {
                    tracing::warn!(id = id.as_str(), "from its checkpoint: {e}; from its first tick");
                    restep(map, &bytes, None, &mut scratch)
                }
                None => Err(e),
            })
        }))
        .unwrap_or_else(|_| Err("re-stepping panicked".into()));
        let live = LiveFile::read(dir, &id);
        let stepped = match (stepped, live) {
            (Ok(s), Some(live)) if !s.over => (s, live),
            (s, live) => {
                let why = match (&s, &live) {
                    (Err(e), _) => e.clone(),
                    (Ok(s), _) if s.over => "it was over".into(),
                    _ => "no lobby file".into(),
                };
                tracing::info!(
                    id = id.as_str(),
                    "a record left by the last server, finished as it stands: {why}"
                );
                finish(dir, &id, &part, &bytes);
                continue;
            }
        };
        let (s, live) = stepped;
        let Some((k, m)) = free.next() else {
            tracing::warn!(id = id.as_str(), "no place to take a match up in: finished as it stands");
            finish(dir, &id, &part, &bytes);
            continue;
        };
        let mut buf = vec![0u8; snapshot::len(&s.referee.m)];
        let n = snapshot::write(&s.referee.m, &mut buf);
        let secret = getrandom::u64().unwrap_or(s.seed.rotate_left(17) ^ 0x0FF5_E7C0_DE5E_C2E7) | 1;
        if let Err(e) = shards[k].restore(m, &buf[..n], secret) {
            tracing::warn!(id = id.as_str(), "taking a match up: {e}");
            finish(dir, &id, &part, &bytes);
            continue;
        }
        let tick = s.referee.m.tick;
        let file = cut_part(&part, s.cut);
        let mut rec = Rec::resumed(k, m as u16, s.seed, bytes[..s.cut].to_vec(), file);
        if let Some((t, b)) = s.checkpoint {
            rec.keep_checkpoint(t, b.into());
        }
        store.resume(k, m, rec);
        if let Some(r) = referees.get_mut(k).and_then(|r| r.as_mut()) {
            r.adopt(m, &id, s.referee);
        }
        tracing::info!(
            id = id.as_str(),
            shard = k,
            m,
            tick,
            lost_bytes = bytes.len() - s.cut,
            ms = started.elapsed().as_millis() as u64,
            "a match taken up where the last server left it"
        );
        out.push(Recovered { shard: k, m: m as u16, id, tick, live });
    }
    out
}

/// The part cut at its last hash and opened to go on writing.
fn cut_part(part: &Path, cut: usize) -> Option<File> {
    let open = || -> std::io::Result<File> {
        let mut f = OpenOptions::new().write(true).open(part)?;
        f.set_len(cut as u64)?;
        f.seek(SeekFrom::End(0))?;
        Ok(f)
    };
    open().map_err(|e| tracing::warn!("{}: {e}", part.display())).ok()
}

/// A part not taken up: kept as a finished record, its lobby file gone.
fn finish(dir: &Path, id: &str, part: &Path, bytes: &[u8]) {
    let done = dir.join(format!("{id}.elr"));
    if bytes.is_empty() || std::fs::rename(part, &done).is_err() {
        let _ = std::fs::remove_file(part);
    }
    let _ = std::fs::remove_file(LiveFile::path(dir, id));
}
