//! The record is the truth (`docs/NETCODE.md`, "The record"): a match played over the wire, with
//! people's orders and the bots', replays on a fresh match to every hash; and a record changed by
//! one order is caught at the first hash after it.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]

mod common;

use common::{BAD, Rig, bot_brain, map, walker};
use el_sim::record::{self, Divergence, Frame, Frames, Referee};

fn replay(bytes: &[u8]) -> Result<Referee, Divergence> {
    let mut r = Referee::new(map());
    let mut frames = Frames::new(bytes);
    for f in frames.by_ref() {
        r.take(&f)?;
    }
    assert_eq!(frames.error, None, "every frame reads");
    Ok(r)
}

#[test]
fn a_match_replays_from_its_record_to_every_hash() {
    let mut rig = Rig::new(17, vec![(0, BAD, walker(2)), (6, BAD, bot_brain())]);
    rig.run(240.0);
    let bytes = rig.record.clone();
    let ticks = rig.m().tick;
    let r = replay(&bytes).expect("the replay keeps to the record");
    println!(
        "{} bytes for {ticks} ticks ({:.1} KB a minute), {} hashes checked",
        bytes.len(),
        bytes.len() as f64 / (f64::from(ticks) / 1_800.0) / 1_000.0,
        r.checked
    );
    assert_eq!(r.m.tick, ticks, "every tick is in it");
    assert_eq!(r.m.hash(), rig.m().hash(), "and it comes to the server's match");
    assert!(r.checked >= ticks / record::HASH_EVERY - 1);
    assert_eq!(rig.shared.metrics.record_drops.load(std::sync::atomic::Ordering::Relaxed), 0);
    // Both people joined: the record says so.
    let seats = Frames::new(&bytes).filter(|f| matches!(f, Frame::Seat { joined: true, .. })).count();
    assert_eq!(seats, 2);
}

#[test]
fn one_order_changed_is_caught_at_the_next_hash() {
    let mut rig = Rig::new(3, vec![(2, BAD, walker(5))]);
    rig.run(60.0);
    let bytes = rig.record.clone();
    // The walker's first move order, taken somewhere else.
    let mut out = Vec::new();
    let mut changed_at = None;
    let mut buf = [0u8; record::MAX_FRAME];
    for f in Frames::new(&bytes) {
        let n = match f {
            Frame::Tick { tick, mut inputs }
                if changed_at.is_none()
                    && inputs[2].orders().iter().any(|o| matches!(o, el_proto::Order::Move { .. })) =>
            {
                for o in &mut inputs[2].orders[..usize::from(inputs[2].n)] {
                    if let el_proto::Order::Move { to } = o {
                        to.x += 300.0;
                    }
                }
                changed_at = Some(tick);
                record::write_tick(&mut buf, tick, &inputs)
            }
            Frame::Tick { tick, inputs } => record::write_tick(&mut buf, tick, &inputs),
            Frame::Header(h) => record::write_header(&mut buf, &h),
            Frame::Hash { tick, hash } => record::write_hash(&mut buf, tick, hash),
            Frame::Outcome { tick, winner } => record::write_outcome(&mut buf, tick, winner),
            Frame::Seat { tick, seat, joined } => record::write_seat(&mut buf, tick, seat, joined),
        };
        out.extend_from_slice(&buf[..n]);
    }
    assert_eq!(out.len(), bytes.len(), "written back the same, but for one order");
    let at = changed_at.expect("the walker moved");
    match replay(&out) {
        Err(Divergence::Hash { tick, .. }) => {
            assert!(tick > at && tick <= at + record::HASH_EVERY, "caught at {tick}, changed at {at}");
        }
        other => panic!("not caught: {:?}", other.map(|r| r.checked)),
    }
    assert!(replay(&bytes).is_ok(), "the record as it was still replays");
}
