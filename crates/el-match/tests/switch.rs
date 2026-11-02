//! A player switched from one match to another (a practice champion changed, a match left for the
//! next) can still have the old match's snapshots in flight, or queued in a page that draws a frame
//! every two seconds, when it's seated in the new one. Those must never reach the new match's
//! client: one taken there set its clock to the old match's tick, every snapshot of the new match
//! was stale after it, and the page sat in the new match seeing nothing new while datagrams kept
//! coming (so it never dialled again). The lobby names each seating's stream; the client takes no
//! other.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]

mod common;

use common::{CLEAN, Rig, walker};

#[test]
fn a_new_match_takes_no_snapshot_of_the_last() {
    // The match the player was in, a minute on, and the last snapshot it sent them.
    let mut old = Rig::practice_streams(41, vec![(0, CLEAN, walker(1))], 7);
    let last = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let keep = last.clone();
    old.tap = Some(Box::new(move |_, _, bytes| *keep.borrow_mut() = bytes.to_vec()));
    old.run(60.0);
    let leftover = last.borrow().clone();
    assert!(!leftover.is_empty());
    assert!(old.m().tick > 1_500, "{}", old.m().tick);

    // The next one, its seat's stream the next the lobby gives out. The old match's snapshot is
    // there before any of its own.
    let mut new = Rig::practice_streams(42, vec![(0, CLEAN, walker(2))], 8);
    let p = &mut new.players[0];
    let taken = p.client.on_datagram(&leftover, 0.0).expect("it decodes");
    assert_eq!(taken, el_client_core::Taken::Stale, "the last match's snapshot was taken");
    assert_eq!(p.client.stats.foreign, 1);
    new.run(5.0);
    let c = &new.players[0].client;
    assert!(c.started(), "the new match never started");
    let server = new.m().tick;
    assert!(
        c.world.tick <= server && c.world.tick + 10 >= server,
        "client at {}, server at {server}",
        c.world.tick
    );
    assert!(c.stats.stale < 10, "{} of the new match's snapshots stale", c.stats.stale);
}

/// Told no stream (a server from before streams were named), the last match's snapshots queued
/// in a slow page reach the new match's client first. None of them decodes there (each needs a
/// baseline it never had), and one that doesn't decode changes nothing: it once moved the clock
/// to its tick all the same, and the prediction stepped on from the old match's minute, every
/// command it sent far in the server's future.
#[test]
fn snapshots_that_do_not_decode_move_nothing() {
    let mut old = Rig::practice_streams(41, vec![(0, CLEAN, walker(1))], 7);
    let kept = std::rc::Rc::new(std::cell::RefCell::new(std::collections::VecDeque::new()));
    let keep = kept.clone();
    // A frame's worth at a frame every two seconds.
    old.tap = Some(Box::new(move |_, _, bytes| {
        let mut k = keep.borrow_mut();
        k.push_back(bytes.to_vec());
        if k.len() > 60 {
            k.pop_front();
        }
    }));
    old.run(60.0);
    let queued: Vec<Vec<u8>> = kept.borrow().iter().cloned().collect();
    assert_eq!(queued.len(), 60);

    let mut new = Rig::practice_streams(42, vec![(0, CLEAN, walker(2))], 7);
    new.players[0].client.set_stream(0);
    let p = &mut new.players[0];
    for d in &queued {
        assert!(!matches!(p.client.on_datagram(d, 0.0), Ok(el_client_core::Taken::Taken { .. })));
    }
    assert_eq!(p.client.world.tick, 0, "a snapshot not taken moved the clock");
    assert!(!p.client.started());
    new.run(5.0);
    let c = &new.players[0].client;
    assert!(c.started(), "the new match never started");
    let server = new.m().tick;
    assert!(
        c.world.tick <= server && c.world.tick + 10 >= server,
        "client at {}, server at {server}",
        c.world.tick
    );
}
