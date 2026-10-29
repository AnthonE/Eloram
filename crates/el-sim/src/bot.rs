//! A scripted brain for a seat: what fills empty seats, and what plays the test matches. It gives
//! orders, as a person's client does, from what its team sees, nothing more; so it is fair, and the
//! record holds its orders like anyone's.

use el_proto::{Aim, Order, UnitRef};
use glam::Vec2;

use crate::content::champions::ability;
use crate::content::items::{self, ITEMS};
use crate::content::{Role, Target};
use crate::game::{Match, SeatInput};
use crate::map;
use crate::math;
use crate::units::{FIRST_MINION, MAX_UNITS, SEATS, structure, structure_slot, team_of_seat};

/// Ticks between a bot's thoughts (5 a second).
pub const THINK_EVERY: u32 = 6;
/// How far a bot looks for an enemy champion to fight.
const ENGAGE: f32 = 1_100.0;
/// Under this share of health, it backs off to heal (`think`).
const RETREAT: f32 = 0.25;

/// What each champion builds, in order (items' indices), by its role: its big items, one of each.
fn build(def: u8) -> &'static [u8] {
    match crate::content::champions::CHAMPIONS[usize::from(def)].role {
        Role::Marksman => &[11, 8, 12, 6, 10, 13],
        Role::Mage | Role::Enchanter => &[9, 13, 6, 10, 8, 11],
        Role::Tank => &[10, 13, 6, 12, 8, 11],
        Role::Fighter | Role::Assassin => &[8, 12, 6, 11, 10, 13],
    }
}

/// One shopping step: the next big item if it's affordable (selling the cheapest part to make
/// room), else a part toward it while there's room for parts.
fn shop(m: &Match, seat: usize) -> Option<Order> {
    let c = &m.champs[seat];
    let next = build(c.def).iter().copied().find(|it| !c.items.contains(it))?;
    let cost = |it: u8| ITEMS[usize::from(it)].cost as f32;
    let free = c.items.iter().position(|&it| it == items::NONE);
    let cheapest = (0..c.items.len())
        .filter(|&k| c.items[k] != items::NONE && !build(c.def).contains(&c.items[k]))
        .min_by(|&a, &b| cost(c.items[a]).total_cmp(&cost(c.items[b])));
    if c.gold >= cost(next) {
        return match (free, cheapest) {
            (Some(_), _) => Some(Order::Buy { item: next }),
            (None, Some(k)) => Some(Order::Sell { inv: k as u8 }),
            (None, None) => None,
        };
    }
    let empty = c.items.iter().filter(|&&it| it == items::NONE).count();
    let part = match next {
        8 | 11 | 12 => 0,
        9 => 1,
        10 => 2,
        13 => 3,
        _ => 4,
    };
    (empty > 2 && c.gold >= cost(part)).then_some(Order::Buy { item: part })
}

/// The unit as an order names it.
fn unit_ref(m: &Match, i: usize) -> UnitRef {
    UnitRef { slot: i as u8, generation: m.units[i].generation as u8 }
}

/// This tick's orders for `seat`'s bot, into `out` (nothing most ticks: orders persist).
pub fn think(m: &Match, seat: usize, out: &mut SeatInput) {
    // In practice the seats are dummies (`practice`).
    if m.practice {
        return crate::practice::think(m, seat, out);
    }
    let now = m.tick;
    if !(now + seat as u32).is_multiple_of(THINK_EVERY) || m.outcome.is_some() {
        return;
    }
    let c = &m.champs[seat];
    let team = team_of_seat(seat);
    // Growing: augments, skill points, items.
    if c.screen.open && m.in_shop(seat) {
        let card = math::hash32(seat as u32 ^ m.seed as u32, u32::from(c.screens_done)) % 3;
        out.push(Order::PickAugment { card: card as u8 });
    }
    if c.points > 0 {
        let order = [3u8, 0, 1, 2];
        let pick =
            order.into_iter().filter(|&s| c.ranks[usize::from(s)] < Match::max_rank(s, c.level)).min_by_key(
                |&s| if s == 3 { 0 } else { u32::from(c.ranks[usize::from(s)]) * 4 + u32::from(s) + 1 },
            );
        if let Some(slot) = pick {
            out.push(Order::LevelSkill { slot });
        }
    }
    if m.in_shop(seat)
        && let Some(o) = shop(m, seat)
    {
        out.push(o);
    }
    if !m.units[seat].alive {
        return;
    }
    let me = &m.units[seat];
    let forward = if team == map::DAWN { 1.0 } else { -1.0 };
    // Our foremost turret standing: where a hurt bot waits, and where the others wait for a wave.
    let turret = {
        let k = if m.units[structure_slot(team, structure::OUTER)].alive {
            structure::OUTER
        } else {
            structure::INHIB_TURRET
        };
        m.units[structure_slot(team, k)].pos
    };
    if me.hp < me.stats.max_hp * RETREAT {
        // Nothing heals at the fountain in ARAM: a relic on our side if one is up, else behind our
        // turret, out of the fight but near enough to share the wave's experience.
        let side = usize::from(team) * 2;
        let relic = m.relics[side..side + 2]
            .iter()
            .filter(|r| r.up)
            .map(|r| r.pos)
            .min_by(|a, b| math::distance(me.pos, *a).total_cmp(&math::distance(me.pos, *b)));
        let to = m.map.snap(relic.unwrap_or(turret - Vec2::new(forward * 450.0, 0.0)));
        if math::distance(me.pos, to) > 100.0 {
            out.push(Order::Move { to }.quantized());
        }
        return;
    }
    // A fight: the nearest enemy champion it can see.
    let enemy = (0..SEATS)
        .filter(|&e| m.attackable(team, e))
        .map(|e| (math::distance(me.pos, m.units[e].pos), e))
        .filter(|&(d, _)| d <= ENGAGE)
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    if let Some((dist, e)) = enemy {
        if let Some(o) = cast_at(m, seat, e, dist) {
            out.push(o);
            return;
        }
        let them = &m.units[e];
        let braver = me.hp / me.stats.max_hp >= them.hp / them.stats.max_hp - 0.15;
        if braver || dist <= me.stats.range + me.radius + them.radius {
            out.push(Order::Attack { unit: unit_ref(m, e) });
            return;
        }
    }
    // No fight: behind the wave's front, or at the turret waiting for one.
    let front = (FIRST_MINION..MAX_UNITS)
        .filter(|&i| m.units[i].alive && m.units[i].team == team)
        .map(|i| m.units[i].pos)
        .max_by(|a, b| (a.x * forward).total_cmp(&(b.x * forward)));
    let post = match front {
        Some(p) => p - Vec2::new(forward * 250.0, 0.0),
        None => turret - Vec2::new(forward * 200.0, 0.0),
    };
    let jitter = Vec2::new(0.0, (seat % 5) as f32 * 90.0 - 180.0);
    let to = m.map.snap(post + jitter);
    if math::distance(me.pos, to) > 120.0 || front.is_some() {
        out.push(Order::AttackMove { to }.quantized());
    }
}

/// An ability worth casting at enemy champion `e`, `dist` away, if one is ready.
fn cast_at(m: &Match, seat: usize, e: usize, dist: f32) -> Option<Order> {
    let now = m.tick;
    let c = &m.champs[seat];
    let me = &m.units[seat];
    let them = &m.units[e];
    for slot in [3u8, 0, 1, 2] {
        let rank = c.ranks[usize::from(slot)];
        if rank == 0 || c.ready[usize::from(slot)] > now {
            continue;
        }
        let a = ability(c.def, slot);
        if me.mana < a.cost[usize::from(rank - 1)] {
            continue;
        }
        // Lead a moving target by the time the missile (or the windup) takes to get there.
        let speed = match a.effect {
            crate::content::Effect::Line { speed, .. } => speed,
            _ => 0.0,
        };
        let flight = a.windup + if speed > 0.0 { dist / speed } else { 0.0 };
        let lead = them.pos + them.moved * (flight * crate::TICK_HZ as f32);
        let order = match a.target {
            Target::Dir if dist <= a.range => Order::Cast { slot, aim: Aim::Point(lead) },
            Target::Point if dist <= a.range + 150.0 => Order::Cast { slot, aim: Aim::Point(lead) },
            Target::Enemy if dist <= a.range + 100.0 => Order::Cast { slot, aim: Aim::Unit(unit_ref(m, e)) },
            Target::Caster if dist <= 300.0 || (slot == 1 && me.hp < me.stats.max_hp * 0.6) => {
                Order::Cast { slot, aim: Aim::None }
            }
            Target::Ally if me.hp < me.stats.max_hp * 0.7 => Order::Cast { slot, aim: Aim::None },
            _ => continue,
        };
        return Some(order.quantized());
    }
    None
}
