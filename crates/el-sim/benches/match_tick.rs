//! A bot match timed tick by tick and phase by phase: `cargo bench -p el-sim --bench match_tick`.
//! p50, p99 and the slowest ticks with what they spent (the sim never reads a clock; it marks).
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]

use el_sim::game::SeatInput;
use el_sim::map::Map;
use el_sim::path::PathScratch;
use el_sim::units::SEATS;
use el_sim::{Match, bot, ticks};

fn main() {
    let seed: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(7);
    let map = Map::leaked();
    let picks = std::array::from_fn(|s| ((seed as usize + s * 3) % 5) as u8);
    let mut m = Match::new(map, seed, picks);
    let mut scratch = PathScratch::new();
    let mut times = Vec::new();
    let mut phase_sum = [0u64; 7];
    while m.outcome.is_none() && m.tick < ticks(40.0 * 60.0) {
        let mut inputs = [SeatInput::default(); SEATS];
        for (s, input) in inputs.iter_mut().enumerate() {
            bot::think(&m, s, input);
        }
        let (s0, e0) = (scratch.searches, scratch.expanded);
        let t = std::time::Instant::now();
        let mut last = t;
        let mut split = [0u64; 7];
        m.step_marked(&inputs, &mut scratch, &mut |p| {
            let now = std::time::Instant::now();
            split[p as usize] += (now - last).as_micros() as u64;
            last = now;
        });
        for (a, b) in phase_sum.iter_mut().zip(split) {
            *a += b;
        }
        times.push((
            t.elapsed().as_micros() as u64,
            m.tick,
            scratch.searches - s0,
            scratch.expanded - e0,
            split,
            scratch.last,
        ));
    }
    let mut sorted: Vec<u64> = times.iter().map(|t| t.0).collect();
    sorted.sort();
    let q = |p: f64| sorted[((sorted.len() as f64 - 1.0) * p) as usize];
    println!(
        "ticks {}: p50 {} µs, p90 {}, p99 {}, p99.9 {}, max {}",
        sorted.len(),
        q(0.5),
        q(0.9),
        q(0.99),
        q(0.999),
        q(1.0)
    );
    times.sort_by_key(|t| std::cmp::Reverse(t.0));
    let names = ["orders", "champions", "minions", "separate", "missiles", "world", "vision"];
    let n = sorted.len() as u64;
    println!(
        "mean by phase: {}",
        names
            .iter()
            .zip(phase_sum)
            .map(|(k, v)| format!("{k} {:.1}", v as f64 / n as f64))
            .collect::<Vec<_>>()
            .join(", ")
    );
    for (us, tick, searches, expanded, split, last) in times.iter().take(10) {
        println!(
            "  tick {tick:>6}: {us:>6} µs, {searches} searches, {expanded} cells; {split:?}; last {last:?}"
        );
    }
}
