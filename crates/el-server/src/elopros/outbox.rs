//! What's owed to Elo Pros, kept on disk until it's taken: a job a file in
//! `{records}/elopros/outbox/` (written whole, then renamed), tried again a minute later, then
//! doubling to an hour, while the store is down or busy; one it refuses (a 4xx) goes to
//! `failed/` with the reason beside it, for a person to read. A restart takes up what's left.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::EloPros;
use super::report::{self, MatchDone};

/// How often the outbox looks for what's due.
const EVERY: Duration = Duration::from_secs(15);
/// A ranked match's record may still be on its way to disk at the end: its hash waits this long
/// before the result goes without it.
const RECORD_WAIT_S: u64 = 10 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Ratings,
    Gamestats,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub kind: Kind,
    pub done: MatchDone,
    /// When it was made, and when it's next tried (unix seconds).
    pub made: u64,
    pub next: u64,
    pub tries: u32,
}

impl Job {
    fn name(&self) -> String {
        format!("{}-{}.json", self.done.record, if self.kind == Kind::Ratings { "ratings" } else { "stats" })
    }
}

pub struct Outbox {
    dir: Option<PathBuf>,
}

impl Outbox {
    /// Under `records` (without one, nothing survives a restart, and nothing is kept at all: a
    /// server without a records directory has no record to rate a match by either).
    pub fn new(records: Option<&Path>) -> Outbox {
        let dir = records.map(|r| r.join("elopros").join("outbox"));
        if let Some(d) = &dir
            && let Err(e) = std::fs::create_dir_all(d.join("failed"))
        {
            tracing::warn!("{}: {e}", d.display());
        }
        Outbox { dir }
    }

    /// The jobs a finished match owes: its rating (ranked) and its players' stats.
    pub fn owe(&self, done: &MatchDone) {
        let now = crate::signin::unix_now();
        for kind in [Kind::Ratings, Kind::Gamestats] {
            let wanted = match kind {
                Kind::Ratings => report::ratings(done, None, None).is_some(),
                Kind::Gamestats => report::gamestats(done).is_some(),
            };
            if wanted {
                self.put(&Job { kind, done: done.clone(), made: now, next: now, tries: 0 });
            }
        }
    }

    fn put(&self, job: &Job) {
        let Some(dir) = &self.dir else { return };
        match serde_json::to_vec_pretty(job) {
            Ok(b) => crate::records::write_atomically(&dir.join(job.name()), &b),
            Err(e) => tracing::warn!("outbox: {e}"),
        }
    }

    /// The jobs waiting, oldest first.
    pub fn waiting(&self) -> Vec<(PathBuf, Job)> {
        let Some(dir) = &self.dir else { return Vec::new() };
        let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
        let mut jobs: Vec<(PathBuf, Job)> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter_map(|p| Some((p.clone(), serde_json::from_slice(&std::fs::read(&p).ok()?).ok()?)))
            .collect();
        jobs.sort_by_key(|(_, j)| j.made);
        jobs
    }

    fn failed(&self, path: &Path, job: &Job, why: &str) {
        let Some(dir) = &self.dir else { return };
        tracing::warn!(record = job.done.record.as_str(), kind = ?job.kind, "Elo Pros refused a report: {why}");
        let to = dir.join("failed").join(job.name());
        let _ = std::fs::rename(path, &to);
        let _ = std::fs::write(to.with_extension("why.txt"), why);
    }
}

/// The record's sha256, once it's finished on disk.
fn record_sha(records: Option<&Path>, id: &str) -> Option<String> {
    let bytes = std::fs::read(records?.join(format!("{id}.elr"))).ok()?;
    Some(super::key::sha256_hex(&bytes))
}

/// The body a job sends now, or `None` if it should wait (the record isn't written yet).
fn body(elo: &EloPros, job: &Job, now: u64) -> Option<Option<(String, serde_json::Value)>> {
    match job.kind {
        Kind::Ratings => {
            let sha = record_sha(elo.records.as_deref(), &job.done.record);
            if sha.is_none() && now < job.made + RECORD_WAIT_S {
                return None;
            }
            let url =
                elo.records_url.as_ref().map(|u| format!("{}/{}", u.trim_end_matches('/'), job.done.record));
            Some(
                report::ratings(&job.done, sha.as_deref(), url.as_deref())
                    .map(|b| ("/ratings/eloram/report".into(), b)),
            )
        }
        Kind::Gamestats => Some(report::gamestats(&job.done).map(|b| ("/gamestats/eloram/report".into(), b))),
    }
}

/// Sends what's due, for as long as the server runs.
pub async fn run(elo: Arc<EloPros>) {
    let mut tick = tokio::time::interval(EVERY);
    loop {
        tick.tick().await;
        let e = elo.clone();
        let _ = tokio::task::spawn_blocking(move || pass(&e)).await;
    }
}

/// One look at the outbox (blocking).
pub fn pass(elo: &EloPros) {
    let now = crate::signin::unix_now();
    for (path, mut job) in elo.outbox.waiting() {
        if job.next > now {
            continue;
        }
        let Some(sending) = body(elo, &job, now) else { continue };
        let Some((route, b)) = sending else {
            let _ = std::fs::remove_file(&path);
            continue;
        };
        let bytes = serde_json::to_vec(&b).unwrap_or_default();
        match elo.api.post(&route, &bytes) {
            Ok(a) if a.ok() => {
                tracing::info!(record = job.done.record.as_str(), kind = ?job.kind, "reported to Elo Pros");
                let _ = std::fs::remove_file(&path);
            }
            Ok(a) if !a.retry() => elo.outbox.failed(&path, &job, &format!("{}: {}", a.status, a.why())),
            got => {
                let why = match got {
                    Ok(a) => format!("{}: {}", a.status, a.why()),
                    Err(e) => e,
                };
                job.tries += 1;
                job.next = now + (60u64 << job.tries.min(6)).min(3600);
                tracing::info!(record = job.done.record.as_str(), kind = ?job.kind, tries = job.tries, "Elo Pros didn't take a report yet: {why}");
                elo.outbox.put(&job);
            }
        }
    }
}
