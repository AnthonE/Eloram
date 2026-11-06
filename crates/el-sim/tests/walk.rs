//! Walking on the navigation mesh, as a player feels it: a click anywhere (the chasm too) is walked
//! to, never stood still at; minions marching down the lane never stick on a turret's edge; and a
//! champion walks round minions, never through them.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]

mod common;

use el_proto::Order;
use el_sim::Match;
use el_sim::game::SeatInput;
use el_sim::map;
use el_sim::path::PathScratch;
use el_sim::units::{FIRST_MINION, Kind, MAX_UNITS, SEATS, Unit};
use glam::Vec2;

#[test]
fn a_move_order_arrives_wherever_it_is_clicked() {
    let m0 = common::map();
    let mut m = Match::new(m0, 5, common::picks(1));
    m.set_practice();
    let mut s = PathScratch::new();
    let mut rng = el_sim::math::Rng::new(3);
    let anywhere = |rng: &mut el_sim::math::Rng| {
        Vec2::new(rng.next_f32() * map::MAP_W_FULL, rng.next_f32() * map::MAP_H_FULL)
    };
    let pick = |rng: &mut el_sim::math::Rng| loop {
        let q = anywhere(rng);
        if m0.walkable(q) {
            return q;
        }
    };
    for k in 0..200 {
        // A quarter of the clicks off the floor: the chasm, rock, a structure.
        let (a, b) = (pick(&mut rng), if k % 4 == 0 { anywhere(&mut rng) } else { pick(&mut rng) });
        m.units[0].pos = a;
        m.units[0].path.clear();
        let order = Order::Move { to: b }.quantized();
        let Order::Move { to } = order else { unreachable!() };
        let mut inputs = [SeatInput::default(); SEATS];
        inputs[0].push(order);
        m.step(&inputs, &mut s);
        let goal = m0.snap(to);
        let speed = m.units[0].stats.move_speed;
        // Time enough for the way round everything, half again.
        let limit = (1.5 * 1.5 * a.distance(goal) / speed * el_sim::TICK_HZ as f32) as u32 + 30;
        let mut still = 0;
        for _ in 0..limit {
            let before = m.units[0].pos;
            m.step(&[SeatInput::default(); SEATS], &mut s);
            still = if m.units[0].pos.distance(before) < 0.01 { still + 1 } else { 0 };
            if still > 15 {
                break;
            }
        }
        let end = m.units[0].pos;
        assert!(
            end.distance(goal) < 2.0,
            "{a} -> {b}: stopped at {end}, {} short of {goal}",
            end.distance(goal)
        );
    }
}

#[test]
fn marching_minions_never_stand_still() {
    let (mut m, mut s) = common::bot_match(7);
    let (mut marching, mut stood) = (0u32, 0u32);
    for _ in 0..el_sim::ticks(240.0) {
        let inputs = common::bot_inputs(&m);
        let before: [Vec2; MAX_UNITS] = std::array::from_fn(|i| m.units[i].pos);
        m.step(&inputs, &mut s);
        for (i, at) in before.iter().enumerate().skip(FIRST_MINION) {
            let u = &m.units[i];
            if !u.alive || u.windup > 0 || !u.target.is_none() || u.cc.rooted(m.tick) {
                continue;
            }
            marching += 1;
            if at.distance(u.pos) < 0.5 {
                stood += 1;
            }
        }
    }
    assert!(marching > 20_000, "{marching} minion ticks marching");
    assert!(stood * 500 < marching, "{stood} of {marching} marching minion ticks stood still");
}

/// A practice match with minions standing (stunned for good) at `at`, the player at `from`.
fn among_minions(at: &[Vec2], from: Vec2) -> Match {
    let mut m = Match::new(common::map(), 5, common::picks(1));
    m.set_practice();
    let def = el_sim::content::rules::minion_def(Kind::Melee);
    for (k, &p) in at.iter().enumerate() {
        let mut u = Unit {
            alive: true,
            kind: Kind::Melee,
            team: map::DUSK,
            pos: p,
            radius: def.radius,
            ..Unit::default()
        };
        u.stats = def.stats;
        u.hp = def.stats.max_hp;
        u.cc.stun = u32::MAX;
        m.units[FIRST_MINION + k] = u;
    }
    m.units[0].pos = from;
    m
}

/// Walks the player to `to` for up to `secs`: where it ended, and the deepest it went into a
/// minion's body.
fn walk_among(m: &mut Match, to: Vec2, secs: f32) -> (Vec2, f32) {
    let mut s = PathScratch::new();
    let mut inputs = [SeatInput::default(); SEATS];
    inputs[0].push(Order::Move { to }.quantized());
    m.step(&inputs, &mut s);
    let mut deepest = 0.0f32;
    for _ in 0..el_sim::ticks(secs) {
        m.step(&[SeatInput::default(); SEATS], &mut s);
        let me = &m.units[0];
        for u in m.units[FIRST_MINION..].iter().filter(|u| u.alive) {
            deepest = deepest.max(me.radius + u.radius - me.pos.distance(u.pos));
        }
    }
    (m.units[0].pos, deepest)
}

#[test]
fn a_champion_walks_round_a_line_of_minions_not_through_it() {
    let x = 11_000.0;
    let line: Vec<Vec2> = (-3..=3).map(|k| Vec2::new(x, map::MID_Y + k as f32 * 100.0)).collect();
    for dy in [0.0, 40.0, -170.0, 290.0] {
        let from = Vec2::new(x - 500.0, map::MID_Y + dy);
        let to = Vec2::new(x + 500.0, map::MID_Y + dy);
        let mut m = among_minions(&line, from);
        let (end, deepest) = walk_among(&mut m, to, 8.0);
        assert!(deepest < 1.0, "from {from}: {deepest} into a minion");
        // (Where the click lands on the floor: one of these is in a turret's footprint.)
        let to = common::map().snap(to);
        assert!(end.distance(to) < 2.0, "from {from}: stopped at {end}, short of {to}");
    }
}

#[test]
fn a_champion_walks_round_a_clump_of_minions() {
    let c = Vec2::new(11_000.0, map::MID_Y);
    let mut clump = vec![c];
    for k in 0..6 {
        clump.push(c + el_sim::math::dir(k as f32 * std::f32::consts::TAU / 6.0) * 98.0);
    }
    for a in 0..8 {
        let d = el_sim::math::dir(a as f32 * std::f32::consts::TAU / 8.0 + 0.1);
        let (from, to) = (c - d * 350.0, c + d * 350.0);
        assert!(common::map().walk_clear(from, to), "{from} -> {to}: the floor round the clump");
        let mut m = among_minions(&clump, from);
        let (end, deepest) = walk_among(&mut m, to, 8.0);
        assert!(deepest < 1.0, "from {from}: {deepest} into a minion");
        // (Where the click lands on the floor: one of these is in a turret's footprint.)
        let to = common::map().snap(to);
        assert!(end.distance(to) < 2.0, "from {from}: stopped at {end}, short of {to}");
    }
}

#[test]
fn a_champion_sent_onto_a_minion_stops_against_it() {
    let at = Vec2::new(11_000.0, map::MID_Y);
    let from = at - Vec2::new(400.0, 0.0);
    let mut m = among_minions(&[at], from);
    let (end, deepest) = walk_among(&mut m, at, 4.0);
    assert!(deepest < 1.0, "{deepest} into it");
    let reach = m.units[0].radius + m.units[FIRST_MINION].radius;
    assert!(end.distance(at) < reach + 12.0, "stopped at {end}, {} off", end.distance(at));
}
