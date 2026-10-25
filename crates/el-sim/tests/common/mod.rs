//! What the sim's tests share: the map, built once, and bot matches.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros, dead_code)]

use std::sync::OnceLock;

use el_sim::game::SeatInput;
use el_sim::map::Map;
use el_sim::path::PathScratch;
use el_sim::units::SEATS;
use el_sim::{Match, bot};

pub fn map() -> &'static Map {
    static M: OnceLock<&'static Map> = OnceLock::new();
    M.get_or_init(Map::leaked)
}

/// Each seat's champion for `seed`: the pool dealt round.
pub fn picks(seed: u64) -> [u8; SEATS] {
    std::array::from_fn(|s| ((seed as usize + s * 3) % el_sim::content::champions::CHAMPIONS.len()) as u8)
}

/// Ten bots' orders for this tick.
pub fn bot_inputs(m: &Match) -> [SeatInput; SEATS] {
    let mut inputs = [SeatInput::default(); SEATS];
    for (s, input) in inputs.iter_mut().enumerate() {
        bot::think(m, s, input);
    }
    inputs
}

/// Steps `m` with bots for `ticks` ticks, or until it ends.
pub fn run(m: &mut Match, scratch: &mut PathScratch, ticks: u32) {
    for _ in 0..ticks {
        if m.outcome.is_some() {
            return;
        }
        let inputs = bot_inputs(m);
        m.step(&inputs, scratch);
    }
}

pub fn bot_match(seed: u64) -> (Match, PathScratch) {
    (Match::new(map(), seed, picks(seed)), PathScratch::new())
}
