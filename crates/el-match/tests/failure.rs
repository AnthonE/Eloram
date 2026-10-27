//! A match that panics is stopped, alone: the shard's other matches go on, and the network side is
//! told (`Outcome::Failed`) so the lobby can end it for its players.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]

mod common;

use el_match::{Control, MatchState, Outcome, ShardConfig, build, queues::CHECKPOINT_EVERY};

#[test]
fn a_panicking_match_stops_and_the_shard_goes_on() {
    let (mut shard, shared, _leases, _egress, _records) = build(ShardConfig {
        matches: 2,
        map: common::map(),
        hot_guard: None,
        checkpoint_every: CHECKPOINT_EVERY,
    });
    for m in 0..2u16 {
        let seed = 40 + u64::from(m);
        shared
            .control
            .push(Control::Start { m, seed, picks: common::picks(seed), humans: 0, secret: 1 })
            .unwrap();
    }
    for t in 0..60 * 30 {
        shard.tick_at(t * 33_333);
    }
    // A champion that isn't one: its stats, worked out every tick, index past the table.
    shard.matches[1].champs[3].def = u8::MAX;
    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let before = (shard.matches[0].tick, shard.matches[1].tick);
    for t in 0..30 {
        shard.tick_at((1_800 + t) * 33_333);
    }
    std::panic::set_hook(quiet);
    assert_eq!(shard.matches[0].tick, before.0 + 30, "the other match went on");
    assert_eq!(shard.matches[1].tick, before.1, "the one that panicked stopped where it stood");
    assert_eq!(shared.state(1), MatchState::Over);
    assert_eq!(shared.state(0), MatchState::Running);
    let failed: Vec<_> = std::iter::from_fn(|| shared.outcomes.pop()).collect();
    assert_eq!(failed, [Outcome::Failed { m: 1, tick: before.1 }]);
    assert_eq!(shared.metrics.failed.load(std::sync::atomic::Ordering::Relaxed), 1);
}
