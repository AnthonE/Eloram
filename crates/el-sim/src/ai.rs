//! Minions and structures: League's rules for who fights whom.
//!
//! A turret shoots the nearest minion in range until an enemy champion hurts an allied champion
//! in its range, then that champion; each consecutive shot at a champion hits harder. A minion
//! answers a call for help the same way, else fights the nearest minion, champion or structure,
//! and otherwise marches on the enemy's nexus.

use crate::content::DmgKind;
use crate::content::rules::{self, MINION_ACQUIRE};
use crate::events::Event;
use crate::game::Match;
use crate::math;
use crate::missile::Source;
use crate::path::PathScratch;
use crate::units::{FIRST_MINION, FIRST_STRUCTURE, Kind, MAX_UNITS, SEATS, structure, structure_slot};

/// How recently an enemy champion must have hurt an allied one to be answered.
const CALL_FOR_HELP: u32 = 60;
/// Ticks between a minion's looks for a better target.
const RETHINK: u32 = 8;

impl Match {
    pub(crate) fn minions_and_structures(&mut self, scratch: &mut PathScratch) {
        for i in FIRST_STRUCTURE..MAX_UNITS {
            if !self.units[i].alive {
                continue;
            }
            match self.units[i].kind {
                Kind::Turret => self.turret_tick(i),
                Kind::Fountain => self.fountain_tick(i),
                k if k.is_minion() => self.minion_tick(i, scratch),
                _ => {}
            }
        }
    }

    /// An enemy champion in `reach` of `i` that just hurt one of `i`'s allied champions.
    fn caller(&self, i: usize, reach: f32) -> Option<usize> {
        let team = self.units[i].team;
        (0..SEATS).find(|&c| {
            let u = &self.units[c];
            u.hit_champion_at > 0
                && self.tick.saturating_sub(u.hit_champion_at) <= CALL_FOR_HELP
                && self.attackable(team, c)
                && self.in_range(i, c, reach)
        })
    }

    fn turret_tick(&mut self, i: usize) {
        let now = self.tick;
        let team = self.units[i].team;
        let range = self.units[i].stats.range;
        let held = self
            .live(self.units[i].target)
            .filter(|&t| self.attackable(team, t) && self.in_range(i, t, range));
        let caller = self.caller(i, range);
        let target = match (held, caller) {
            (Some(t), _) if t < SEATS => Some(t),
            (_, Some(c)) => Some(c),
            (Some(t), None) => Some(t),
            (None, None) => self
                .nearest_enemy(i, range, false, true)
                .or_else(|| self.nearest_enemy(i, range, true, false)),
        };
        let Some(t) = target else {
            self.units[i].target = crate::handle::Handle::NONE;
            self.units[i].streak = 0;
            return;
        };
        let h = self.units[t].handle(t);
        if self.units[i].target != h {
            self.units[i].target = h;
            self.units[i].streak = 0;
        }
        if self.units[i].attack_ready > now {
            return;
        }
        let damage = if t < SEATS {
            let s = self.units[i].streak;
            self.units[i].streak = (s + 1).min(rules::TURRET_STREAK_MAX);
            rules::TURRET_DAMAGE * (1.0 + rules::TURRET_STREAK * f32::from(s))
        } else {
            self.units[t].stats.max_hp * rules::turret_share(self.units[t].kind)
        };
        self.units[i].attack_ready = now + self.period_ticks(i);
        self.units[i].facing = math::angle(self.units[t].pos - self.units[i].pos);
        self.event(Event::Attack { unit: i as u16, target: t as u16 });
        self.spawn_homing(i, t, Source::Turret, 0, rules::TURRET_MISSILE_SPEED, damage);
    }

    fn fountain_tick(&mut self, i: usize) {
        let team = self.units[i].team;
        let at = self.units[i].pos;
        for c in 0..SEATS {
            let u = &self.units[c];
            let r = rules::FOUNTAIN_RANGE;
            if u.alive && u.team != team && (u.pos - at).length_squared() <= r * r {
                self.deal(i, c, rules::FOUNTAIN_DPS * crate::DT, DmgKind::True, false);
            }
        }
    }

    fn minion_tick(&mut self, i: usize, scratch: &mut PathScratch) {
        let now = self.tick;
        let start = self.units[i].pos;
        self.units[i].moved = glam::Vec2::ZERO;
        if self.units[i].cc.disabled(now) {
            self.units[i].windup = 0;
            return;
        }
        if self.units[i].windup > 0 {
            self.attack_tick(i);
            return;
        }
        let team = self.units[i].team;
        let held = self
            .live(self.units[i].target)
            .filter(|&t| self.attackable(team, t) && self.in_range(i, t, MINION_ACQUIRE + 200.0));
        let rethink = (now + i as u32).is_multiple_of(RETHINK);
        let target = if held.is_some() && !rethink {
            held
        } else {
            self.caller(i, MINION_ACQUIRE)
                .or_else(|| held.filter(|&t| t >= FIRST_MINION))
                .or_else(|| self.nearest_enemy(i, MINION_ACQUIRE, false, true))
                .or(held)
                .or_else(|| self.nearest_enemy(i, MINION_ACQUIRE, true, false))
                .or_else(|| self.nearest_structure(i, MINION_ACQUIRE))
        };
        let rooted = self.units[i].cc.rooted(now);
        match target {
            Some(t) => {
                self.units[i].target = self.units[t].handle(t);
                if !self.try_attack(i, t) && !rooted {
                    self.chase(i, t, scratch);
                }
            }
            None => {
                self.units[i].target = crate::handle::Handle::NONE;
                if !rooted {
                    self.march(i);
                }
            }
        }
        self.units[i].moved = self.units[i].pos - start;
    }

    /// The nearest enemy structure a minion may hit within `reach`.
    fn nearest_structure(&self, i: usize, reach: f32) -> Option<usize> {
        let team = self.units[i].team;
        let mut best: Option<(f32, usize)> = None;
        for j in FIRST_STRUCTURE..FIRST_MINION {
            if self.units[j].kind == Kind::Fountain
                || !self.attackable(team, j)
                || !self.in_range(i, j, reach)
            {
                continue;
            }
            let d = (self.units[j].pos - self.units[i].pos).length_squared();
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, j));
            }
        }
        best.map(|(_, j)| j)
    }

    /// No fight in sight: on down the lane to the enemy nexus, by the map's march field (no
    /// search: the field was laid once for the whole process).
    fn march(&mut self, i: usize) {
        let team = self.units[i].team;
        self.units[i].path.clear();
        if let Some(to) = self.map.march_toward(team, self.units[i].pos) {
            let step = self.step_len(i);
            self.slide_toward(i, to, step);
        }
    }

    /// Minions push each other apart, and give way to champions.
    pub(crate) fn separate(&mut self) {
        for i in FIRST_MINION..MAX_UNITS {
            if !self.units[i].alive {
                continue;
            }
            for j in 0..MAX_UNITS {
                if i == j || !self.units[j].alive {
                    continue;
                }
                let (a, b) = (&self.units[i], &self.units[j]);
                if b.kind.is_structure() {
                    continue;
                }
                let d = a.pos - b.pos;
                let min = a.radius + b.radius;
                let d2 = d.length_squared();
                if d2 >= min * min || d2 < 1e-6 {
                    continue;
                }
                let len = math::sqrt(d2);
                let share = if b.kind.is_minion() { 0.5 } else { 1.0 };
                let p = a.pos + d / len * ((min - len) * share);
                if self.map.walkable(p) {
                    self.units[i].pos = p;
                }
            }
        }
    }

    /// Whether `team`'s inhibitor is down (the other side's waves bring super minions).
    pub fn inhibitor_down(&self, team: u8) -> bool {
        !self.units[structure_slot(team, structure::INHIBITOR)].alive
    }
}
