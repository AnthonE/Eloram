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
    r.flush();
    Ok(r)
}

/// The record written back frame by frame, each changed (or dropped, `None`) by `edit`.
fn rewrite(bytes: &[u8], mut edit: impl FnMut(Frame) -> Option<Frame>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; record::MAX_FRAME];
    for f in Frames::new(bytes) {
        let Some(f) = edit(f) else { continue };
        let n = match f {
            Frame::Tick { tick, inputs } => record::write_tick(&mut buf, tick, &inputs),
            Frame::Header(h) => record::write_header(&mut buf, &h),
            Frame::Hash { tick, hash } => record::write_hash(&mut buf, tick, hash),
            Frame::Outcome { tick, winner } => record::write_outcome(&mut buf, tick, winner),
            Frame::Seat { tick, seat, joined } => record::write_seat(&mut buf, tick, seat, joined),
            Frame::Key { tick, seat, screen, key } => record::write_key(&mut buf, tick, seat, screen, key),
        };
        out.extend_from_slice(&buf[..n]);
    }
    out
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

/// The shard's checkpoints, once a minute, are the referee's to the byte: re-stepping the record
/// to each one's tick comes to the same match. And a referee started from one goes on to the same
/// end as one that stepped the whole record.
#[test]
fn every_checkpoint_is_the_record_re_stepped_and_resumes_alike() {
    let mut rig = Rig::new(29, vec![(1, BAD, walker(3)), (7, BAD, bot_brain())]);
    rig.run(250.0);
    let bytes = rig.record.clone();
    let cks = rig.checkpoints.clone();
    assert!(cks.len() >= 4, "{} checkpoints in four minutes", cks.len());
    assert_eq!(rig.shared.metrics.checkpoint_drops.load(std::sync::atomic::Ordering::Relaxed), 0);
    let mut r = Referee::new(map());
    let mut buf = vec![0u8; el_sim::snapshot::len(&r.m)];
    let mut matched = 0;
    for f in Frames::new(&bytes) {
        r.take(&f).expect("the record replays");
        if let Frame::Hash { tick, .. } = f
            && let Some(ck) = cks.iter().find(|c| el_sim::snapshot::tick_of(c) == Some(tick))
        {
            let n = r.checkpoint(&mut buf);
            assert_eq!(&buf[..n], &ck[..], "the checkpoint at {tick} differs: {:?}", {
                let mut theirs = el_sim::Match::new(map(), 0, [0; el_sim::units::SEATS]);
                el_sim::snapshot::read(ck, &mut theirs).unwrap();
                el_sim::snapshot::diff(&theirs, &r.m).map(|w| w.to_string())
            });
            matched += 1;
        }
    }
    assert_eq!(matched, cks.len(), "every checkpoint's tick is a hash in the record");
    r.flush();
    let whole = (r.m.tick, r.m.hash());
    // From the second checkpoint: the record read from its start, the frames before it passed over.
    let mut from = Referee::new(map());
    from.resume(&cks[1]).unwrap();
    for f in Frames::new(&bytes) {
        from.take(&f).expect("resumed, the record replays");
    }
    from.flush();
    assert!(!from.skipping());
    assert_eq!((from.m.tick, from.m.hash()), whole, "resumed from a checkpoint, the same end");
    assert!(from.checked > 0 && from.checked < r.checked, "only the hashes after it checked");
}

/// A live match draws its augment offers from the server's secret: the record gives each screen's
/// key only as the screen opens, it replays with them, and without them (all a watcher could take
/// from the header) the offers come out otherwise and the replay goes astray.
#[test]
fn augment_offers_come_from_keys_the_record_gives_as_their_screens_open() {
    let mut rig = Rig::new(21, vec![(1, BAD, bot_brain())]);
    rig.run(300.0);
    let bytes = rig.record.clone();
    let keys: Vec<(u32, u8, u8)> = Frames::new(&bytes)
        .filter_map(|f| match f {
            Frame::Key { tick, seat, screen, .. } => Some((tick, seat, screen)),
            _ => None,
        })
        .collect();
    let first: Vec<_> = keys.iter().filter(|k| k.2 == 0).collect();
    assert_eq!(first.len(), 10, "every seat's first screen: {keys:?}");
    assert!(first.iter().all(|k| k.0 == 0), "the first screens open at the first tick: {keys:?}");
    assert!(keys.iter().any(|k| k.2 == 1 && k.0 > 0), "a level 7 screen later, its key with it: {keys:?}");
    let r = replay(&bytes).expect("with its keys, the record replays");
    assert_eq!(r.m.hash(), rig.m().hash());
    let stripped = rewrite(&bytes, |f| (!matches!(f, Frame::Key { .. })).then_some(f));
    match replay(&stripped) {
        Err(Divergence::Hash { tick, .. }) => assert!(tick <= 2 * record::HASH_EVERY, "astray by {tick}"),
        other => panic!("the header alone gave the offers: {:?}", other.map(|r| r.checked)),
    }
}

#[test]
fn one_order_changed_is_caught_at_the_next_hash() {
    let mut rig = Rig::new(3, vec![(2, BAD, walker(5))]);
    rig.run(60.0);
    let bytes = rig.record.clone();
    // The walker's first move order, taken somewhere else.
    let mut changed_at = None;
    let out = rewrite(&bytes, |f| match f {
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
            Some(Frame::Tick { tick, inputs })
        }
        f => Some(f),
    });
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
