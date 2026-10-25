//! The netcode end to end, in virtual time: a shard, clients over a 100 ms link with jitter and
//! loss, the own champion predicted with the server's own step (`docs/NETCODE.md`).
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]

mod common;

use std::collections::BTreeMap;

use common::{BAD, Brain, CLEAN, Rig, bot_brain, quantile, walker};
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
        let a = &p.agree;
        println!(
            "  what the team sees: {} of {} units not as the server had them at the snapshot's tick ({:.2}%, {} unknown), {:.2} units off on average",
            a.wrong + a.unknown,
            a.checked,
            100.0 * a.wrong_share(),
            a.unknown,
            a.off / a.wrong.max(1) as f64,
        );
        for (cause, (n, p50, p99, max)) in by_cause(rig, k) {
            println!("  {cause:?}: {n} corrections, p50 {p50:.2}, p99 {p99:.2}, max {max:.1}");
        }
    }
}

/// A record goes once (and once more), and again only if it's lost: the client still knows what
/// its team sees as the server had it, all but a few times in a thousand over 5% loss.
fn told_as_seen(rig: &Rig) {
    for p in &rig.players {
        assert!(p.agree.checked > 10_000, "seat {}: {} checked", p.seat, p.agree.checked);
        let share = p.agree.wrong_share();
        assert!(
            share < 0.005,
            "seat {}: {:.2}% of what it sees not as the server had it",
            p.seat,
            100.0 * share
        );
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
    told_as_seen(&rig);
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
    // The next frame that makes a command (a frame is half a tick: one or two of them) steps the
    // prediction through it.
    for _ in 0..3 {
        if rig.players[0].client.next_cmd() > at {
            break;
        }
        rig.run(1.0 / 60.0 + 0.001);
    }
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
        assert!(kbps < 6.0, "seat {}: {kbps:.1} KB/s down", p.seat);
        let up = p.up_bytes as f64 / rig.now / 1_000.0;
        assert!(up < 1.5, "seat {}: {up:.1} KB/s up", p.seat);
        assert!(p.max_datagram <= el_proto::MAX_DATAGRAM);
        let c = &m.champs[p.seat];
        assert!(c.level > 3, "seat {} grew past the start: level {}", p.seat, c.level);
    }
    orders_applied_once(&rig);
    told_as_seen(&rig);
    assert!(rig.players.iter().all(|p| p.seat < SEATS));
}

/// WASD: a steer held for three seconds, then let go, one way and the other.
fn steerer() -> Brain {
    Box::new(|_, _, k, out| {
        out.steer = match (k / 90) % 4 {
            0 => 1,
            2 => 5,
            _ => 0,
        }
    })
}

#[test]
fn steering_is_predicted_exactly_over_a_bad_link() {
    let mut rig = Rig::new(12, vec![(1, BAD, steerer())]);
    rig.run(60.0);
    rig.settle(3.0);
    report(&rig);
    let (n, _, p99, _) = by_cause(&rig, 0)[&Cause::Walking];
    assert!(n > 1_000, "{n} walking corrections");
    assert!(p99 < 1.0, "steering mispredicted, p99 {p99:.2} units");
}

#[test]
fn steering_is_predicted_exactly_at_a_frame_a_second() {
    // A frame a second makes thirty commands at once: every one of them goes, in as many packets
    // as it takes, or the server stands in for the rest and the prediction walks where it didn't.
    let mut rig = Rig::new(13, vec![(6, CLEAN, steerer())]);
    rig.players[0].period = 1.0;
    let home = rig.m().units[6].pos;
    let mut furthest: f32 = 0.0;
    for _ in 0..60 {
        rig.run(1.0);
        furthest = furthest.max(rig.m().units[6].pos.distance(home));
    }
    report(&rig);
    let p = &rig.players[0];
    assert!(furthest > 500.0, "it walked: {furthest:.0} units out at most");
    // While its clock first runs out ahead (a second, at this rate) the server stands in for what
    // came late; from the gates' opening on, every step is the server's.
    let late = p.corrections.iter().filter(|c| c.tick > 15 * 30).map(|c| c.error).fold(0.0, f32::max);
    assert!(late < 1.0, "steering mispredicted by {late:.2} units after the start");
    let walking = p.corrections.iter().filter(|c| c.cause == Cause::Walking).count();
    assert!(walking > 30, "{walking} walking corrections");
    assert_eq!(p.client.stats.errors, 0);
}

#[test]
fn a_page_drawing_twice_a_second_still_plays() {
    // Software rendering, a page in the background: a frame (and so a poll) every half second,
    // with commands for thirty ticks made at once.
    let mut rig = Rig::new(5, vec![(4, BAD, walker(9))]);
    rig.players[0].period = 0.5;
    rig.run(60.0);
    rig.players[0].period = 1.0 / 60.0;
    rig.settle(5.0);
    report(&rig);
    let p = &rig.players[0];
    assert_eq!((p.client.stats.errors, p.client.stats.no_baseline), (0, 0));
    assert!(p.given.len() > 20, "{} orders", p.given.len());
    orders_applied_once(&rig);
}

#[test]
fn an_outage_is_ridden_out() {
    // Three seconds with nothing through either way: nothing acked, every datagram lost. After it,
    // what was lost goes again and the client knows its team's view as before.
    let outage = common::LinkSpec { outage: Some((40.0, 43.0)), ..BAD };
    let mut rig = Rig::new(21, vec![(3, outage, bot_brain())]);
    rig.run(40.0);
    let before = rig.players[0].agree;
    rig.run(10.0);
    rig.players[0].agree = common::Agreement::default();
    rig.run(30.0);
    rig.settle(5.0);
    report(&rig);
    let p = &rig.players[0];
    assert_eq!((p.client.stats.errors, p.client.stats.no_baseline), (0, 0));
    assert!(before.checked > 10_000);
    told_as_seen(&rig);
    orders_applied_once(&rig);
}

#[test]
fn a_ping_reaches_the_team_alone_and_a_vote_ends_the_match() {
    use el_proto::event::WireEvent;
    use el_proto::order::ping;
    use el_sim::content::rules;
    use el_sim::ticks;
    let from = ticks(rules::SURRENDER_FROM);
    // Seat 0 pings once, and opens a vote at 8:00; seat 1, its teammate, says yes when it sees one
    // open; seat 5 is the enemy's.
    let opener: Brain = Box::new(move |_, _, k, out| {
        if k == 150 {
            out.push(el_proto::Order::Ping {
                kind: ping::COMING,
                at: glam::Vec2::new(3_000.0, 1_900.0),
                unit: None,
            });
        }
        if k == from + 30 {
            out.push(el_proto::Order::Surrender { yes: true });
        }
    });
    let mut voted = false;
    let follower: Brain = Box::new(move |m, _, _, out| {
        if m.surrender[0].open() && !voted {
            voted = true;
            out.push(el_proto::Order::Surrender { yes: true });
        }
    });
    let mut rig =
        Rig::new(3, vec![(0, BAD, opener), (1, BAD, follower), (5, BAD, Box::new(|_, _, _, _| {}))]);
    rig.run(10.0);
    let pings = |k: usize| {
        let c = &rig.players[k].client;
        c.feed[..(c.feed_n as usize).min(c.feed.len())]
            .iter()
            .filter(|(_, e)| matches!(e, WireEvent::Ping { seat: 0, kind: ping::COMING, .. }))
            .count()
    };
    assert_eq!((pings(0), pings(1), pings(2)), (1, 1, 0), "the team's, once each, and not the enemy's");
    rig.run_until(f64::from(from) / 30.0 + 10.0, |r| r.m().outcome.is_some());
    assert_eq!(rig.m().outcome, Some(el_sim::map::DUSK), "two players of two said yes");
    rig.run(1.0);
    for p in &rig.players {
        assert!(p.client.over, "seat {} was told", p.seat);
    }
    // The record holds the votes and who could vote: it replays to the same end.
    let mut referee = el_sim::record::Referee::new(common::map());
    for f in el_sim::record::Frames::new(&rig.record) {
        referee.take(&f).expect("the replay keeps to the record");
    }
    referee.flush();
    assert_eq!(referee.m.hash(), rig.m().hash());
    assert_eq!(referee.m.outcome, Some(el_sim::map::DUSK));
}
