//! The netcode end to end, in virtual time: a shard, clients over a 100 ms link with jitter and
//! loss, the own champion predicted with the server's own step (`docs/NETCODE.md`).
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]

mod common;

use std::collections::BTreeMap;

use common::{BAD, CLEAN, Rig, bot_brain, quantile, walker};
use el_client_core::Cause;
use el_sim::game::Intent;
use el_sim::units::SEATS;

/// Corrections by cause: how many, p50, p99, max (units).
fn by_cause(rig: &Rig, k: usize) -> BTreeMap<Cause, (usize, f32, f32, f32)> {
    let mut out: BTreeMap<Cause, Vec<f32>> = BTreeMap::new();
    for c in &rig.players[k].corrections {
        out.entry(c.cause).or_default().push(c.error);
    }
    out.into_iter()
        .map(|(cause, mut xs)| {
            let n = xs.len();
            let p50 = quantile(&mut xs, 0.5);
            let p99 = quantile(&mut xs, 0.99);
            (cause, (n, p50, p99, *xs.last().unwrap()))
        })
        .collect()
}

fn report(rig: &Rig) {
    for (k, p) in rig.players.iter().enumerate() {
        let s = &p.client.stats;
        let secs = rig.now;
        println!(
            "seat {}: {} datagrams ({:.1} KB/s, largest {} B), {} stale, {} records, {} events, {} packets up ({:.1} KB/s), rtt {:.0} ms, lead {:.1} ticks, missed {}",
            p.seat,
            s.datagrams,
            p.sent_bytes as f64 / secs / 1_000.0,
            p.max_datagram,
            s.stale,
            s.records,
            s.events,
            s.packets,
            p.up_bytes as f64 / secs / 1_000.0,
            p.client.clock.rtt * 1_000.0,
            p.client.clock.lead,
            s.missed,
        );
        for (cause, (n, p50, p99, max)) in by_cause(rig, k) {
            println!("  {cause:?}: {n} corrections, p50 {p50:.2}, p99 {p99:.2}, max {max:.1}");
        }
    }
}

/// Every order a client gave reached the server and was applied there exactly once.
fn orders_applied_once(rig: &Rig) {
    for p in &rig.players {
        let s = &rig.shard.seats[p.seat];
        assert_eq!(p.client.orders.outstanding(), 0, "seat {}: orders still unacked", p.seat);
        assert_eq!(s.next_seq, p.client.orders.next_seq, "seat {}: the server's count", p.seat);
        assert_eq!(s.orders_applied, p.given.len() as u64, "seat {}: applied once each", p.seat);
        assert_eq!(p.client.orders.refused, 0);
    }
}

#[test]
fn walking_is_predicted_exactly_over_a_bad_link() {
    let mut rig = Rig::new(11, vec![(0, BAD, walker(1)), (6, BAD, walker(2))]);
    rig.run(120.0);
    rig.settle(5.0);
    report(&rig);
    for (k, p) in rig.players.iter().enumerate() {
        let s = &p.client.stats;
        assert_eq!((s.errors, s.no_baseline), (0, 0), "seat {}", p.seat);
        assert!(s.datagrams > 3_000, "{} datagrams", s.datagrams);
        assert!(p.max_datagram <= el_proto::MAX_DATAGRAM);
        assert!(p.given.len() > 50, "seat {}: {} orders", p.seat, p.given.len());
        let causes = by_cause(&rig, k);
        let (n, _, p99, _) = causes[&Cause::Walking];
        assert!(n > 2_000, "seat {}: {n} walking corrections", p.seat);
        assert!(p99 < 1.0, "seat {}: walking mispredicted, p99 {p99:.2} units", p.seat);
        assert!((0.08..0.16).contains(&p.client.clock.rtt), "rtt {:.3}", p.client.clock.rtt);
    }
    orders_applied_once(&rig);
}

#[test]
fn a_click_moves_the_prediction_the_next_tick() {
    let mut rig = Rig::new(4, vec![(2, CLEAN, Box::new(|_, _, _, _| {}))]);
    rig.run(3.0);
    let p = &mut rig.players[0];
    let at = p.client.next_cmd();
    let from = p.client.pred.units[2].pos;
    let to = el_sim::map::for_team(0, glam::Vec2::new(3_000.0, el_sim::map::MID_Y));
    p.client.order(el_proto::Order::Move { to }).unwrap();
    // The next frame makes the next command, and steps the prediction through it.
    rig.run(1.0 / 60.0 + 0.001);
    let p = &rig.players[0];
    assert!(p.client.next_cmd() > at);
    assert!(
        matches!(p.client.pred.champs[2].intent, Intent::Move(_)),
        "{:?}",
        p.client.pred.champs[2].intent
    );
    assert!(p.client.pred.units[2].pos.distance(to) < from.distance(to), "it set off at once");
}

#[test]
fn bots_play_a_match_through_the_wire() {
    // Two of the ten play through clients (one a side), on the team view they're sent.
    let mut rig = Rig::new(7, vec![(1, BAD, bot_brain()), (8, BAD, bot_brain())]);
    rig.run(240.0);
    rig.settle(5.0);
    report(&rig);
    let m = rig.m();
    let kills: u16 = m.champs.iter().map(|c| c.kills).sum();
    println!(
        "tick {}, {kills} kills, levels {:?}",
        m.tick,
        m.champs.iter().map(|c| c.level).collect::<Vec<_>>()
    );
    assert!(kills > 5, "a match was played: {kills} kills");
    for p in &rig.players {
        let s = &p.client.stats;
        assert_eq!((s.errors, s.no_baseline), (0, 0), "seat {}", p.seat);
        assert!(p.given.len() > 100, "seat {} gave {} orders", p.seat, p.given.len());
        let kbps = p.sent_bytes as f64 / rig.now / 1_000.0;
        assert!(kbps < 30.0, "seat {}: {kbps:.1} KB/s down", p.seat);
        assert!(p.max_datagram <= el_proto::MAX_DATAGRAM);
        let c = &m.champs[p.seat];
        assert!(c.level > 3, "seat {} grew past the start: level {}", p.seat, c.level);
    }
    orders_applied_once(&rig);
    assert!(rig.players.iter().all(|p| p.seat < SEATS));
}
