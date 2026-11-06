//! Walking on the navigation mesh, as a player feels it: a click anywhere (the chasm too) is walked
//! to, never stood still at; and minions marching down the lane never stick on a turret's edge.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]

mod common;

use el_proto::Order;
use el_sim::Match;
use el_sim::game::SeatInput;
use el_sim::map;
use el_sim::path::PathScratch;
use el_sim::units::{FIRST_MINION, MAX_UNITS, SEATS};
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
