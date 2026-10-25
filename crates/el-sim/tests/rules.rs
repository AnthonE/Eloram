//! The rules of `docs/DESIGN.md`, one at a time: structures' protection, turrets answering a call
//! for help, skillshots that hit and dodges that miss, brush, augment screens, Combo Breaker, the
//! fountain, relics, deaths.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]

mod common;

use el_proto::{Aim, Order, UnitRef};
use el_sim::content::augments::AUGMENTS;
use el_sim::content::champions::CHAMPIONS;
use el_sim::content::headliners::HEADLINERS;
use el_sim::content::rules;
use el_sim::game::{Screen, SeatInput};
use el_sim::map::{self, DAWN, DUSK, MID_Y};
use el_sim::path::PathScratch;
use el_sim::units::BuffKind;
use el_sim::units::{SEATS, structure, structure_slot};
use el_sim::{Match, ticks};
use glam::Vec2;

/// A match with nobody doing anything yet; seat 0 Dawn's, seat 5 Dusk's. Its headliner is one that
/// changes no fight (Gold Fever), so no rule here depends on what the seed draws.
fn quiet(picks: [u8; SEATS]) -> (Match, PathScratch) {
    let mut m = Match::new(common::map(), 1, picks);
    m.set_headliner(headliner("Gold Fever"));
    (m, PathScratch::new())
}

fn headliner(name: &str) -> u8 {
    HEADLINERS.iter().position(|h| h.name == name).expect("a headliner") as u8
}

fn augment(name: &str) -> u8 {
    AUGMENTS.iter().position(|a| a.name == name).expect("an augment") as u8
}

fn idle(m: &mut Match, s: &mut PathScratch, n: u32) {
    for _ in 0..n {
        m.step(&[SeatInput::default(); SEATS], s);
    }
}

fn order(m: &mut Match, s: &mut PathScratch, seat: usize, o: Order) {
    let mut inputs = [SeatInput::default(); SEATS];
    inputs[seat].push(o.quantized());
    m.step(&inputs, s);
}

/// Puts the champion of `seat` at `at`.
fn place(m: &mut Match, seat: usize, at: Vec2) {
    m.units[seat].pos = at;
    m.units[seat].path.clear();
}

/// Moves every champion but those listed far out of the way, onto their own platforms.
fn bench_others(m: &mut Match, keep: &[usize]) {
    for s in 0..SEATS {
        if !keep.contains(&s) {
            let team = (s / 5) as u8;
            place(m, s, map::spawn_point(team, s % 5));
        }
    }
}

fn unit_ref(m: &Match, i: usize) -> UnitRef {
    UnitRef { slot: i as u8, generation: m.units[i].generation as u8 }
}

#[test]
fn a_structure_takes_no_damage_until_the_one_in_front_of_it_falls() {
    let (mut m, mut s) = quiet([0; SEATS]);
    let inhib_turret = structure_slot(DUSK, structure::INHIB_TURRET);
    let outer = structure_slot(DUSK, structure::OUTER);
    assert!(!m.exposed(inhib_turret));
    assert!(m.exposed(outer));
    m.units[outer].alive = false;
    assert!(m.exposed(inhib_turret));
    assert!(!m.exposed(structure_slot(DUSK, structure::NEXUS)));
    idle(&mut m, &mut s, 1);
}

#[test]
fn a_turret_turns_on_the_champion_who_hurts_an_ally_under_it() {
    // Vessa (Dawn, seat 0) next to Dusk's outer turret, attacking Dusk's Vessa (seat 5) under it.
    let (mut m, mut s) = quiet([0; SEATS]);
    let turret = structure_slot(DUSK, structure::OUTER);
    let tp = m.units[turret].pos;
    bench_others(&mut m, &[0, 5]);
    place(&mut m, 5, tp + Vec2::new(-150.0, 250.0));
    place(&mut m, 0, tp + Vec2::new(-600.0, 250.0));
    let target = unit_ref(&m, 5);
    // Let the gates open and vision settle.
    m.tick = ticks(16.0);
    idle(&mut m, &mut s, 1);
    order(&mut m, &mut s, 0, Order::Attack { unit: target });
    let mut hit = false;
    for _ in 0..90 {
        idle(&mut m, &mut s, 1);
        hit |= m.units[turret].target.idx == 0;
    }
    assert!(hit, "the turret should turn on the attacker");
    assert!(m.units[0].hp < m.units[0].stats.max_hp, "and hurt it");
}

#[test]
fn a_skillshot_hits_what_stands_in_its_path_and_misses_what_moved() {
    // Orrin (seat 0) throws Ember (Q) down the lane at Dusk's Brakka (seat 5).
    let mut picks = [2; SEATS];
    picks[0] = 1;
    let (mut m, mut s) = quiet(picks);
    m.tick = ticks(16.0);
    bench_others(&mut m, &[0, 5]);
    place(&mut m, 0, Vec2::new(7_000.0, MID_Y));
    place(&mut m, 5, Vec2::new(7_700.0, MID_Y));
    order(&mut m, &mut s, 0, Order::LevelSkill { slot: 0 });
    let before = m.units[5].hp;
    order(&mut m, &mut s, 0, Order::Cast { slot: 0, aim: Aim::Point(Vec2::new(7_700.0, MID_Y)) });
    idle(&mut m, &mut s, 30);
    assert!(m.units[5].hp < before, "it hit: {} -> {}", before, m.units[5].hp);

    // Again, but the target steps out of the line as it's thrown.
    let (mut m, mut s) = quiet(picks);
    m.tick = ticks(16.0);
    bench_others(&mut m, &[0, 5]);
    place(&mut m, 0, Vec2::new(7_000.0, MID_Y));
    place(&mut m, 5, Vec2::new(7_700.0, MID_Y));
    order(&mut m, &mut s, 0, Order::LevelSkill { slot: 0 });
    let before = m.units[5].hp;
    order(&mut m, &mut s, 0, Order::Cast { slot: 0, aim: Aim::Point(Vec2::new(7_700.0, MID_Y)) });
    place(&mut m, 5, Vec2::new(7_700.0, MID_Y + 400.0));
    idle(&mut m, &mut s, 30);
    assert_eq!(m.units[5].hp, before, "it missed");
}

#[test]
fn a_champion_in_brush_is_hidden_from_enemies_outside_it() {
    let (mut m, mut s) = quiet([0; SEATS]);
    m.tick = ticks(16.0);
    bench_others(&mut m, &[0, 5]);
    let brush = map::BRUSHES[1];
    let inside = (brush.0 + brush.1) * 0.5;
    place(&mut m, 5, inside);
    place(&mut m, 0, inside + Vec2::new(-500.0, -400.0));
    idle(&mut m, &mut s, 1);
    assert!(!m.vision.units[usize::from(DAWN)].get(5), "seen in brush from outside it");
    assert!(m.map.walkable(inside + Vec2::new(-150.0, 0.0)));
    place(&mut m, 0, inside + Vec2::new(-150.0, 0.0));
    idle(&mut m, &mut s, 1);
    assert!(m.vision.units[usize::from(DAWN)].get(5), "seen from inside the same brush");
    // An attack order at what you can't see is refused.
    place(&mut m, 0, inside + Vec2::new(-500.0, -400.0));
    idle(&mut m, &mut s, 1);
    let target = unit_ref(&m, 5);
    order(&mut m, &mut s, 0, Order::Attack { unit: target });
    assert_eq!(m.champs[0].intent, el_sim::game::Intent::Idle);
}

#[test]
fn augment_screens_open_at_the_start_and_level_seven_and_take_picks_only_in_the_shop() {
    let (mut m, mut s) = quiet([0; SEATS]);
    idle(&mut m, &mut s, 1);
    let Screen { open, offers, .. } = m.champs[0].screen;
    assert!(open, "the first screen is open at the start");
    assert_eq!(offers.iter().collect::<std::collections::HashSet<_>>().len(), 3, "three different offers");
    // Reroll a card, once.
    order(&mut m, &mut s, 0, Order::Reroll { card: 1 });
    let after = m.champs[0].screen.offers;
    assert_ne!(after[1], offers[1]);
    order(&mut m, &mut s, 0, Order::Reroll { card: 1 });
    assert_eq!(m.champs[0].screen.offers, after, "only once");
    // Out on the lane: no pick.
    m.tick = ticks(16.0);
    place(&mut m, 0, Vec2::new(6_000.0, MID_Y));
    order(&mut m, &mut s, 0, Order::PickAugment { card: 0 });
    assert_eq!(m.champs[0].n_aug, 0);
    // Home: the pick lands.
    place(&mut m, 0, map::spawn_point(DAWN, 0));
    order(&mut m, &mut s, 0, Order::PickAugment { card: 0 });
    assert_eq!(m.champs[0].augments(), &[after[0]]);
    assert!(!m.champs[0].screen.open);
    // Level 7 earns the next (as gaining the levels would).
    m.champs[0].level = 7;
    m.champs[0].screens_earned = 2;
    idle(&mut m, &mut s, 1);
    assert!(m.champs[0].screen.open, "level 7's screen");
}

#[test]
fn combo_breaker_cleanses_five_seconds_of_crowd_control_in_seven() {
    let (mut m, mut s) = quiet([0; SEATS]);
    m.tick = ticks(16.0);
    bench_others(&mut m, &[0]);
    place(&mut m, 0, Vec2::new(6_000.0, MID_Y));
    let t0 = m.tick;
    m.units[0].cc.stun = t0 + ticks(10.0);
    let mut freed = None;
    for _ in 0..ticks(8.0) {
        idle(&mut m, &mut s, 1);
        if freed.is_none() && !m.units[0].cc.disabled(m.tick) {
            freed = Some(m.tick - t0);
        }
    }
    let freed = freed.expect("cleansed");
    assert!((ticks(4.9)..=ticks(5.2)).contains(&freed), "after {freed} ticks");
    assert!(m.units[0].cc.immune > m.tick - ticks(3.0));
}

#[test]
fn the_fountain_kills_an_enemy_on_its_platform_and_the_dead_come_back() {
    let (mut m, mut s) = quiet([0; SEATS]);
    m.tick = ticks(16.0);
    bench_others(&mut m, &[0]);
    place(&mut m, 0, map::spawn_point(DUSK, 0));
    idle(&mut m, &mut s, 30);
    assert!(!m.units[0].alive, "the fountain killed it");
    let back = m.champs[0].dead_until;
    let wait = back - m.tick + 1;
    assert!(wait <= ticks(11.0), "dead for 11 s at level 3");
    idle(&mut m, &mut s, wait);
    assert!(m.units[0].alive, "back after its death timer");
    assert!((m.units[0].pos - map::spawn_point(DAWN, 0)).length() < 1.0, "on its own platform");
}

#[test]
fn a_health_relic_heals_who_takes_it_and_then_everyone_near() {
    let (mut m, mut s) = quiet([0; SEATS]);
    m.tick = ticks(rules_outer());
    bench_others(&mut m, &[0, 1, 5]);
    let pad = m.relics[0].pos;
    for k in [0, 1, 5] {
        m.units[k].hp = 100.0;
    }
    place(&mut m, 1, pad + Vec2::new(400.0, 0.0));
    place(&mut m, 5, pad + Vec2::new(0.0, 500.0));
    idle(&mut m, &mut s, 1);
    assert!(m.relics[0].up);
    place(&mut m, 0, pad);
    idle(&mut m, &mut s, 1);
    assert!(!m.relics[0].up, "taken");
    assert!(m.units[0].hp > 100.0);
    let before = [m.units[1].hp, m.units[5].hp];
    idle(&mut m, &mut s, ticks(2.6));
    assert!(m.units[1].hp > before[0] + 50.0, "the beam healed an ally");
    assert!(m.units[5].hp > before[1] + 50.0, "and an enemy: it heals everyone in it");
}

fn rules_outer() -> f32 {
    el_sim::content::rules::RELIC_OUTER_FIRST
}

#[test]
fn every_champion_can_cast_every_ability_without_breaking_the_match() {
    for def in 0..CHAMPIONS.len() as u8 {
        let mut picks = [0; SEATS];
        picks[0] = def;
        let (mut m, mut s) = quiet(picks);
        m.tick = ticks(16.0);
        bench_others(&mut m, &[0, 5]);
        place(&mut m, 0, Vec2::new(7_000.0, MID_Y));
        place(&mut m, 5, Vec2::new(7_350.0, MID_Y));
        m.champs[0].level = 18;
        m.champs[0].points = 18;
        for slot in [3u8, 3, 3, 0, 1, 2, 0, 1, 2] {
            order(&mut m, &mut s, 0, Order::LevelSkill { slot });
        }
        let target = unit_ref(&m, 5);
        for slot in 0..6u8 {
            let aim = match slot {
                4 => Aim::Point(Vec2::new(6_800.0, MID_Y)),
                _ => Aim::Unit(target),
            };
            let aim = if matches!(aim, Aim::Unit(_)) && ![3, 4].contains(&slot) {
                // Skillshots and areas aim at the enemy's feet; the rest at it.
                Aim::Point(m.units[5].pos)
            } else {
                aim
            };
            order(&mut m, &mut s, 0, Order::Cast { slot, aim });
            order(&mut m, &mut s, 0, Order::Cast { slot, aim: Aim::Unit(target) });
            idle(&mut m, &mut s, 45);
        }
        let used = m.champs[0].ready.iter().filter(|&&r| r > ticks(16.0)).count();
        assert!(used >= 4, "{} used {used} of its six", CHAMPIONS[usize::from(def)].name);
    }
}

#[test]
fn each_headliner_changes_what_it_says() {
    let (plain, _) = quiet([0; SEATS]);
    let base = plain.units[0].stats;
    let with = |name: &str| {
        let (mut m, _) = quiet([0; SEATS]);
        m.set_headliner(headliner(name));
        m.units[0].stats
    };
    let s = with("Swift Winds");
    assert!((s.move_speed - base.move_speed * 1.15).abs() < 0.01, "{} {}", s.move_speed, base.move_speed);
    assert_eq!(with("Clockwork").haste, base.haste + 40.0);
    let s = with("Iron Age");
    assert_eq!((s.max_hp, s.armor, s.mr), (base.max_hp + 500.0, base.armor + 20.0, base.mr + 20.0));
    let s = with("Sharp Edges");
    assert_eq!((s.ad, s.ap), (base.ad + 20.0, base.ap + 40.0));

    // Gold Fever: passive gold twice as fast as under any other.
    let earned = |name: &str| {
        let (mut m, mut s) = quiet([0; SEATS]);
        m.set_headliner(headliner(name));
        m.tick = ticks(rules::PASSIVE_GOLD_FROM);
        let before = m.champs[0].gold;
        idle(&mut m, &mut s, ticks(10.0));
        m.champs[0].gold - before
    };
    let (fever, other) = (earned("Gold Fever"), earned("Clockwork"));
    assert!((fever - 2.0 * other).abs() < 0.5 && other > 30.0, "{fever} against {other}");

    // Short Rest: half the death timer.
    let dead_for = |name: &str| {
        let (mut m, mut s) = quiet([0; SEATS]);
        m.set_headliner(headliner(name));
        m.tick = ticks(16.0);
        bench_others(&mut m, &[0]);
        place(&mut m, 0, map::spawn_point(DUSK, 0));
        let mut t = 0;
        while m.units[0].alive {
            idle(&mut m, &mut s, 1);
            t += 1;
            assert!(t < ticks(5.0), "the fountain kills");
        }
        m.champs[0].dead_until - m.tick
    };
    let (short, full) = (dead_for("Short Rest"), dead_for("Clockwork"));
    assert!(short.abs_diff(full / 2) <= 1, "{short} ticks against {full}");
    assert!(full.abs_diff(ticks(rules::death_secs(3))) <= 1);
}

#[test]
fn a_cast_augment_answers_a_cast_and_waits_its_cooldown() {
    let (mut m, mut s) = quiet([0; SEATS]);
    m.tick = ticks(16.0);
    bench_others(&mut m, &[0]);
    place(&mut m, 0, Vec2::new(6_000.0, MID_Y));
    m.grant_augment(0, augment("Battle Trance"));
    order(&mut m, &mut s, 0, Order::LevelSkill { slot: 0 });
    let speed = |m: &Match| m.units[0].buff(BuffKind::AttackSpeed, m.tick);
    let aim = Aim::Point(Vec2::new(7_000.0, MID_Y));
    assert_eq!(speed(&m), 0.0);
    order(&mut m, &mut s, 0, Order::Cast { slot: 0, aim });
    assert!((speed(&m) - 0.4).abs() < 1e-6, "the cast's buff: {}", speed(&m));
    // Q is back in 7 s, the augment in 8: the second cast gets nothing, the third does.
    idle(&mut m, &mut s, ticks(7.1));
    m.units[0].mana = m.units[0].stats.max_mana;
    order(&mut m, &mut s, 0, Order::Cast { slot: 0, aim });
    assert!(m.champs[0].ready[0] > m.tick, "the second cast went off");
    assert_eq!(speed(&m), 0.0, "the augment is still cooling down");
    idle(&mut m, &mut s, ticks(7.1));
    m.units[0].mana = m.units[0].stats.max_mana;
    order(&mut m, &mut s, 0, Order::Cast { slot: 0, aim });
    assert!(speed(&m) > 0.0, "and answers again once it's ready");
}

fn events(m: &Match) -> Vec<el_sim::events::Event> {
    m.events.as_slice().to_vec()
}

#[test]
fn pings_are_a_few_at_once_then_one_a_second_and_name_only_what_the_team_sees() {
    use el_proto::order::ping;
    use el_sim::events::Event;
    let (mut m, mut s) = quiet([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    idle(&mut m, &mut s, 30);
    let at = Vec2::new(4_000.0, MID_Y);
    let mut pinged = 0;
    for _ in 0..8 {
        order(&mut m, &mut s, 0, Order::Ping { kind: ping::DANGER, at, unit: None });
        pinged += events(&m).iter().filter(|e| matches!(e, Event::Ping { seat: 0, .. })).count();
    }
    assert_eq!(pinged, rules::PING_BURST as usize, "a burst, then no more");
    idle(&mut m, &mut s, ticks(rules::PING_EVERY));
    order(&mut m, &mut s, 0, Order::Ping { kind: ping::LOOK, at, unit: None });
    assert!(events(&m).iter().any(|e| matches!(e, Event::Ping { seat: 0, kind: ping::LOOK, .. })));
    // An enemy across the map isn't seen: the ping keeps its place and drops the unit.
    let enemy = UnitRef { slot: 5, generation: m.units[5].generation as u8 };
    assert!(!m.vision.units[0].get(5));
    idle(&mut m, &mut s, 5 * ticks(rules::PING_EVERY));
    order(&mut m, &mut s, 1, Order::Ping { kind: ping::ATTACK, at, unit: Some(enemy) });
    let named = events(&m).iter().find_map(|e| match *e {
        Event::Ping { seat: 1, unit, .. } => Some(unit),
        _ => None,
    });
    assert_eq!(named, Some(el_sim::voice::NO_UNIT));
    // An ally is: and the dead may ping.
    m.units[2].hp = 0.0;
    let ally = UnitRef { slot: 3, generation: m.units[3].generation as u8 };
    order(&mut m, &mut s, 2, Order::Ping { kind: ping::COMING, at, unit: Some(ally) });
    assert!(events(&m).iter().any(|e| matches!(e, Event::Ping { seat: 2, unit: 3, .. })));
}

#[test]
fn a_team_surrenders_with_four_of_five_of_its_players_and_only_from_eight_minutes() {
    use el_sim::events::Event;
    let (mut m, mut s) = quiet([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    // Dawn has two players (the rest are bots); Dusk one.
    for seat in [0, 3, 7] {
        m.set_seated(seat, true);
    }
    let yes = Order::Surrender { yes: true };
    order(&mut m, &mut s, 0, yes);
    assert!(!m.surrender[0].open(), "not before 8:00");
    m.tick = ticks(rules::SURRENDER_FROM);
    order(&mut m, &mut s, 1, yes);
    assert!(!m.surrender[0].open(), "a bot's seat has no vote");
    order(&mut m, &mut s, 0, yes);
    assert!(m.surrender[0].open());
    assert!(events(&m).iter().any(|e| matches!(e, Event::Vote { team: 0, yes: 1, open: true, .. })));
    // One no of two players: four of five can't be had, so it fails, and the next waits.
    order(&mut m, &mut s, 3, Order::Surrender { yes: false });
    assert!(!m.surrender[0].open());
    assert!(events(&m).iter().any(|e| matches!(e, Event::Vote { team: 0, open: false, passed: false, .. })));
    order(&mut m, &mut s, 0, yes);
    assert!(!m.surrender[0].open(), "a failed vote's wait");
    // Time's up on one nobody else answers.
    m.tick += ticks(rules::SURRENDER_AGAIN);
    order(&mut m, &mut s, 0, yes);
    assert!(m.surrender[0].open());
    idle(&mut m, &mut s, ticks(rules::SURRENDER_VOTE) + 1);
    assert!(!m.surrender[0].open() && m.outcome.is_none());
    // Both say yes: Dusk wins. Dusk's lone player could have alone.
    m.tick += ticks(rules::SURRENDER_AGAIN);
    order(&mut m, &mut s, 3, yes);
    order(&mut m, &mut s, 0, yes);
    assert_eq!(m.outcome, Some(DUSK));
    assert!(events(&m).iter().any(|e| matches!(e, Event::Outcome { winner: DUSK })));
}
