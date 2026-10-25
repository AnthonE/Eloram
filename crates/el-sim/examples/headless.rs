//! Ten bots play a whole match, headless: `cargo run -p el-sim --release --example headless -- [seed]`.
//! Prints how it went, each minute and at the end, and the state's hash.

// An example may allocate: the bans are for the tick.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]

use el_sim::content::augments::AUGMENTS;
use el_sim::content::champions::CHAMPIONS;
use el_sim::game::SeatInput;
use el_sim::map::Map;
use el_sim::path::PathScratch;
use el_sim::units::{FIRST_MINION, SEATS};
use el_sim::{Match, bot, ticks};

fn main() {
    let seed: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(7);
    let map = Map::leaked();
    let picks = std::array::from_fn(|s| ((seed as usize + s * 3) % CHAMPIONS.len()) as u8);
    let mut m = Match::new(map, seed, picks);
    let mut scratch = PathScratch::new();
    let limit = ticks(40.0 * 60.0);
    let started = std::time::Instant::now();
    let mut worst = std::time::Duration::ZERO;
    while m.outcome.is_none() && m.tick < limit {
        let mut inputs = [SeatInput::default(); SEATS];
        for (s, input) in inputs.iter_mut().enumerate() {
            bot::think(&m, s, input);
        }
        let t = std::time::Instant::now();
        m.step(&inputs, &mut scratch);
        worst = worst.max(t.elapsed());
        if m.tick.is_multiple_of(ticks(60.0)) {
            let minions = m.units[FIRST_MINION..].iter().filter(|u| u.alive).count();
            let k: [u16; 2] =
                [0, 1].map(|t| (0..SEATS).filter(|&s| s / 5 == t).map(|s| m.champs[s].kills).sum());
            println!(
                "{:>2}:00  kills {:>2}-{:<2}  minions {minions:>3}  levels {:?}",
                m.tick / ticks(60.0),
                k[0],
                k[1],
                m.champs.iter().map(|c| c.level).collect::<Vec<_>>()
            );
        }
    }
    let spent = started.elapsed();
    println!();
    println!("headliner: {} ({})", m.headliner().name, m.headliner().blurb);
    match m.outcome {
        Some(w) => {
            println!("{} won at {}:{:02}", ["Dawn", "Dusk"][usize::from(w)], m.tick / 1800, m.tick / 30 % 60)
        }
        None => println!("no winner after {} minutes", limit / 1800),
    }
    for (s, c) in m.champs.iter().enumerate() {
        let augs: Vec<&str> = c.augments().iter().map(|&a| AUGMENTS[usize::from(a)].name).collect();
        let items: Vec<&str> = c
            .items
            .iter()
            .filter(|&&i| i != 255)
            .map(|&i| el_sim::content::items::ITEMS[usize::from(i)].name)
            .collect();
        println!(
            "  {} {:<7} lvl {:>2}  {:>2}/{:>2}/{:>2}  cs {:>3}  gold {:>5.0}  {:?} {:?}",
            ["Dawn", "Dusk"][s / 5],
            CHAMPIONS[usize::from(c.def)].name,
            c.level,
            c.kills,
            c.deaths,
            c.assists,
            c.last_hits,
            c.gold,
            augs,
            items
        );
    }
    println!();
    println!(
        "{} ticks in {:.2?}: {:.1} µs a tick, worst {:.1?}; {} path searches",
        m.tick,
        spent,
        spent.as_secs_f64() * 1e6 / f64::from(m.tick),
        worst,
        scratch.searches
    );
    println!("hash {:016x}", m.hash());
}
