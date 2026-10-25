//! A shard of matches, every seat a player's, timed tick by tick:
//! `cargo bench -p el-match --bench shard [-- matches minutes]` (64 and 4 by default).
//!
//! Each seat is played over a loopback: its bot's orders go in as input packets (the bot reads the
//! server's own match, which is cheap and not what's timed), and its datagrams are read back and
//! acked, so replication runs as it does for players: deltas against acked records, every seat a
//! team's view. Only the shard's tick is timed.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]

use std::time::Instant;

use el_match::{Control, InputMsg, ShardConfig, build, read_packet};
use el_proto::ack::AckWindow;
use el_proto::snapshot::SnapshotHeader;
use el_proto::{BitReader, InputCmd, InputPacket, StampedOrder};
use el_sim::game::SeatInput;
use el_sim::map::Map;
use el_sim::units::SEATS;
use el_sim::{TICK_HZ, bot};

#[derive(Default, Clone, Copy)]
struct Loopback {
    acks: AckWindow,
    next_seq: u16,
    bytes: u64,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with('-')).collect();
    let matches: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(64);
    let minutes: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(4.0);
    let map = Map::leaked();
    let (mut shard, shared, mut leases, mut egress, mut records) =
        build(ShardConfig { matches, map, hot_guard: None });
    for m in 0..matches {
        let seed = 1 + m as u64;
        let picks = std::array::from_fn(|s| {
            ((seed as usize + s * 3) % el_sim::content::champions::CHAMPIONS.len()) as u8
        });
        shared.control.push(Control::Start { m: m as u16, seed, picks, humans: (1 << SEATS) - 1 }).unwrap();
    }
    let mut seats = vec![Loopback::default(); matches * SEATS];
    let total = (minutes * 60.0 * f64::from(TICK_HZ)) as u32;
    let warm = total / 2;
    let mut times = Vec::new();
    let mut buf = [0u8; 2_048];
    let mut sent_bytes = 0u64;
    let mut now_us = 0u64;
    for t in 0..total {
        now_us += 1_000_000 / u64::from(TICK_HZ);
        // The seats' packets for the next tick (not timed).
        for mi in 0..matches {
            let m = &shard.matches[mi];
            for seat in 0..SEATS {
                let g = mi * SEATS + seat;
                let lb = &mut seats[g];
                let mut si = SeatInput::default();
                bot::think(m, seat, &mut si);
                let (ack_stream, ack_seq, ack_bits) = lb.acks.ack();
                let mut p = InputPacket {
                    ack_stream,
                    ack_seq,
                    ack_bits,
                    client_time_ms: (now_us / 1_000) as u16,
                    n_cmds: 1,
                    ..InputPacket::default()
                };
                p.cmds[0] = InputCmd { tick: m.tick, steer: si.steer, view: [0, 0] };
                for o in si.orders() {
                    p.orders[usize::from(p.n_orders)] =
                        StampedOrder { seq: lb.next_seq, tick: m.tick, order: *o };
                    p.n_orders += 1;
                    lb.next_seq = lb.next_seq.wrapping_add(1);
                }
                let _ = leases[g].input.push(InputMsg { packet: p, recv_us: now_us });
            }
        }
        let start = Instant::now();
        shard.tick_at(now_us);
        let us = start.elapsed().as_micros() as u64;
        // Read back and ack what each seat was sent (not timed).
        for (g, lb) in seats.iter_mut().enumerate() {
            while let Some(n) = read_packet(&mut egress.rings[g], &mut buf) {
                let h = SnapshotHeader::read(&mut BitReader::new(&buf[..n])).expect("a header");
                let v = lb.acks.check(h.stream, h.seq, h.tick);
                lb.acks.commit(h.stream, v, h.tick);
                lb.bytes += n as u64;
                if t >= warm {
                    sent_bytes += n as u64;
                }
            }
        }
        if t >= warm {
            times.push(us);
        }
        // The records, as a recorder would take them (not timed).
        for ring in &mut records.rings {
            let n = ring.slots();
            if let Ok(chunk) = ring.read_chunk(n) {
                chunk.commit_all();
            }
        }
    }
    times.sort_unstable();
    let q = |p: f64| times[((times.len() as f64 - 1.0) * p) as usize];
    let budget = 1_000_000 / u64::from(TICK_HZ);
    let live = (0..matches).filter(|&m| shard.matches[m].outcome.is_none()).count();
    let secs = times.len() as f64 / f64::from(TICK_HZ);
    println!(
        "{matches} matches ({live} still playing), {} seats, minutes {:.1} to {:.1}:",
        matches * SEATS,
        f64::from(warm) / f64::from(TICK_HZ) / 60.0,
        minutes
    );
    println!(
        "  shard tick: p50 {} µs, p99 {}, max {} ({:.0}% of a {budget} µs tick at p99); a match's tick {:.0} µs at p50",
        q(0.5),
        q(0.99),
        q(1.0),
        q(0.99) as f64 * 100.0 / budget as f64,
        q(0.5) as f64 / matches as f64
    );
    let m = &shared.metrics;
    println!(
        "  a match's sim p50 ≤{} µs, p99 ≤{}; its replication p50 ≤{}, p99 ≤{} (log2 buckets)",
        m.sim_hist.quantile_us(0.5),
        m.sim_hist.quantile_us(0.99),
        m.replicate_hist.quantile_us(0.5),
        m.replicate_hist.quantile_us(0.99)
    );
    println!("  down a seat: {:.1} KB/s", sent_bytes as f64 / secs / (matches * SEATS) as f64 / 1_000.0);
    println!("  matches a core at p99: {}", (budget * matches as u64) / q(0.99).max(1));
    let rec = shared.metrics.record_bytes.load(std::sync::atomic::Ordering::Relaxed) as f64;
    println!(
        "  the record: {:.1} KB a match a minute ({} frames lost)",
        rec / matches as f64 / minutes / 1_000.0,
        shared.metrics.record_drops.load(std::sync::atomic::Ordering::Relaxed)
    );
}
