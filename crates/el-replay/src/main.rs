//! Records (`.elr`, `el_sim::record`) from the command line.
//!
//!   el-replay verify records/            # every record: re-stepped, every hash checked
//!   el-replay verify a.elr b.elr
//!   el-replay dump a.elr [--ticks]       # the header, the hashes, the seats, the outcome
//!
//! A record is fetched from a server with `curl -o x.elr http://host:8080/records/<id>`
//! (`/records.json` lists them; a live match's comes a minute behind).

use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Parser, Subcommand};
use el_sim::map::Map;
use el_sim::record::{Divergence, Frame, Frames, Referee};

#[derive(Parser)]
#[command(name = "el-replay", about = "Verify or dump Eloram match records")]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Re-step each record and check every hash in it; fails on the first that differs.
    Verify { paths: Vec<PathBuf> },
    /// Print a record's frames.
    Dump {
        path: PathBuf,
        /// Every tick's inputs too, not only the ticks with orders.
        #[arg(long)]
        ticks: bool,
    },
}

fn files(paths: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            let mut inner: Vec<PathBuf> = std::fs::read_dir(p)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|e| e == "elr"))
                .collect();
            inner.sort();
            out.extend(inner);
        } else {
            out.push(p.clone());
        }
    }
    Ok(out)
}

fn verify(map: &'static Map, path: &Path) -> anyhow::Result<()> {
    let bytes = std::fs::read(path)?;
    let started = Instant::now();
    let mut r = Referee::new(map);
    let mut frames = Frames::new(&bytes);
    for f in frames.by_ref() {
        if let Err(d) = r.take(&f) {
            let why = match d {
                Divergence::Hash { tick, recorded, replayed } => format!(
                    "the state at tick {tick} hashes to {replayed:016x}, the record says {recorded:016x}: it went astray in ticks {}..{tick}",
                    tick.saturating_sub(el_sim::record::HASH_EVERY)
                ),
                Divergence::Gap { expected, got } => {
                    format!("the record skips from tick {expected} to {got}")
                }
                Divergence::Version { protocol, content } => format!(
                    "made by protocol {protocol}, content {content}; this build is protocol {}, content {}",
                    el_proto::PROTOCOL_VERSION,
                    el_sim::record::CONTENT_VERSION
                ),
                Divergence::NoHeader => "no header".into(),
            };
            anyhow::bail!("{}: {why}", path.display());
        }
    }
    if let Some((at, e)) = frames.error {
        anyhow::bail!("{}: a frame at byte {at} doesn't read ({e})", path.display());
    }
    let outcome = r.outcome.map_or("unfinished".to_string(), |(t, w)| format!("team {w} won at tick {t}"));
    println!(
        "{}: ok, {} ticks, {} hashes, {outcome}, re-stepped in {:.2} s",
        path.display(),
        r.m.tick,
        r.checked,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn dump(path: &Path, all: bool) -> anyhow::Result<()> {
    let bytes = std::fs::read(path)?;
    let mut frames = Frames::new(&bytes);
    for f in frames.by_ref() {
        match f {
            Frame::Header(h) => println!(
                "header: protocol {}, content {}, seed {:016x}, champions {:?}",
                h.protocol, h.content, h.seed, h.picks
            ),
            Frame::Tick { tick, inputs } => {
                let busy = inputs.iter().any(|i| i.n > 0);
                if all || busy {
                    let seats: Vec<String> = inputs
                        .iter()
                        .enumerate()
                        .filter(|(_, i)| i.n > 0 || i.steer != 0)
                        .map(|(s, i)| format!("{s}: steer {} {:?}", i.steer, i.orders()))
                        .collect();
                    println!("tick {tick}: {}", seats.join("; "));
                }
            }
            Frame::Hash { tick, hash } => println!("hash at {tick}: {hash:016x}"),
            Frame::Outcome { tick, winner } => println!("over at {tick}: team {winner} won"),
            Frame::Seat { tick, seat, joined } => {
                println!(
                    "tick {tick}: seat {seat} {}",
                    if joined { "taken by a player" } else { "left to a bot" }
                )
            }
        }
    }
    if let Some((at, e)) = frames.error {
        println!("a frame at byte {at} doesn't read: {e}");
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    match Args::parse().cmd {
        Cmd::Verify { paths } => {
            let map = Map::leaked();
            let list = files(&paths)?;
            anyhow::ensure!(!list.is_empty(), "no records given");
            let mut failed = 0;
            for p in &list {
                if let Err(e) = verify(map, p) {
                    eprintln!("FAILED {e:#}");
                    failed += 1;
                }
            }
            anyhow::ensure!(failed == 0, "{failed} of {} records failed", list.len());
            Ok(())
        }
        Cmd::Dump { path, ticks } => dump(&path, ticks),
    }
}
