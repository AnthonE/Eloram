//! Damage, healing, crowd control and death; auto attacks; what a structure's protection allows.

use glam::Vec2;

use crate::content::rules::{self, ASSIST_GOLD, ASSIST_SECS, HEALING};
use crate::content::{DmgKind, Op};
use crate::events::Event;
use crate::game::{Casting, Dash, Match};
use crate::math;
use crate::missile::Source;
use crate::ticks;
use crate::units::{BuffKind, Kind, SEATS, structure, structure_slot, team_of_seat};

impl Match {
    /// Whether a structure may be hurt: the one in front of it is down (`docs/DESIGN.md`).
    pub fn exposed(&self, i: usize) -> bool {
        let u = &self.units[i];
        if !u.kind.is_structure() {
            return true;
        }
        let t = u.team;
        let down = |k: usize| !self.units[structure_slot(t, k)].alive;
        match i - structure_slot(t, 0) {
            structure::OUTER => true,
            structure::INHIB_TURRET => down(structure::OUTER),
            structure::INHIBITOR => down(structure::INHIB_TURRET),
            structure::NEXUS_TURRET_A | structure::NEXUS_TURRET_B => down(structure::INHIBITOR),
            structure::NEXUS => down(structure::NEXUS_TURRET_A) && down(structure::NEXUS_TURRET_B),
            _ => false,
        }
    }

    /// Whether `team` may attack unit `i` now: alive, an enemy, seen by the team, and not a
    /// structure still protected.
    pub fn attackable(&self, team: u8, i: usize) -> bool {
        let u = &self.units[i];
        u.alive
            && u.team != team
            && u.kind != Kind::Fountain
            && self.vision.units[usize::from(team)].get(i)
            && (!u.kind.is_structure() || self.exposed(i))
    }

    /// Whether `j` is within `range` of `i`, edge to edge.
    pub fn in_range(&self, i: usize, j: usize, range: f32) -> bool {
        let (a, b) = (&self.units[i], &self.units[j]);
        let r = range + a.radius + b.radius;
        (a.pos - b.pos).length_squared() <= r * r
    }

    /// Hurts `dst` by `raw` of `kind` from `src`; returns what it took after mitigation and
    /// shields. Kills it if that was the last of its health.
    pub(crate) fn deal(&mut self, src: usize, dst: usize, raw: f32, kind: DmgKind, _ability: bool) -> f32 {
        let now = self.tick;
        if !self.units[dst].alive || raw <= 0.0 || !self.exposed(dst) {
            return 0.0;
        }
        let mut amount = raw;
        if src < SEATS && dst < SEATS && src != dst {
            let d = math::distance(self.units[src].pos, self.units[dst].pos);
            if d > rules::FALLOFF_FROM {
                let t =
                    ((d - rules::FALLOFF_FROM) / (rules::FALLOFF_TO - rules::FALLOFF_FROM)).clamp(0.0, 1.0);
                amount *= rules::FALLOFF_MAX + (rules::FALLOFF_MIN - rules::FALLOFF_MAX) * t;
            }
        }
        let u = &self.units[dst];
        let resist = match kind {
            DmgKind::Physical => u.stats.armor,
            DmgKind::Magic => u.stats.mr,
            DmgKind::True => 0.0,
        };
        amount *= if resist >= 0.0 { 100.0 / (100.0 + resist) } else { 2.0 - 100.0 / (100.0 - resist) };
        let u = &mut self.units[dst];
        if u.shield > 0.0 && u.shield_until > now {
            let soaked = u.shield.min(amount);
            u.shield -= soaked;
            amount -= soaked;
        }
        u.hp -= amount;
        if src < SEATS {
            u.last_hit_by = src as u8 + 1;
            u.last_hit_at = now;
            u.hurt_by[src] = now;
        }
        if dst < SEATS {
            self.units[src].hit_champion_at = now;
        }
        self.event(Event::Damage { target: dst as u16, source: src as u16, amount });
        if self.units[dst].hp <= 0.0 {
            self.kill(dst, src);
        }
        amount
    }

    /// Heals `dst` by `amount` (ARAM's cut applied), up to its maximum.
    pub(crate) fn heal(&mut self, dst: usize, amount: f32) {
        let u = &mut self.units[dst];
        if !u.alive || amount <= 0.0 {
            return;
        }
        let got = (amount * HEALING).min(u.stats.max_hp - u.hp).max(0.0);
        u.hp += got;
        if got > 0.0 {
            self.event(Event::Heal { target: dst as u16, amount: got });
        }
    }

    fn shield(&mut self, dst: usize, amount: f32, secs: f32) {
        let now = self.tick;
        let u = &mut self.units[dst];
        u.shield = if u.shield_until > now { u.shield.max(amount) } else { amount };
        u.shield_until = now + ticks(secs);
    }

    /// Crowd control lands unless the unit is a structure or immune (Combo Breaker).
    fn cc_lands(&self, i: usize) -> bool {
        let u = &self.units[i];
        u.alive && !u.kind.is_structure() && u.cc.immune <= self.tick
    }

    /// Does `ops` from `caster` (at `rank`, 0-based) to `target`.
    pub(crate) fn apply_ops(&mut self, caster: usize, target: usize, ops: &[Op], rank: u8, ability: bool) {
        let r = usize::from(rank.min(4));
        let now = self.tick;
        for op in ops {
            if !self.units[target].alive {
                break;
            }
            let cs = self.units[caster].stats;
            match *op {
                Op::Damage { kind, base, ad, ap, missing } => {
                    let t = &self.units[target];
                    let raw = base[r] + ad * cs.ad + ap * cs.ap + missing * (t.stats.max_hp - t.hp).max(0.0);
                    self.deal(caster, target, raw, kind, ability);
                }
                Op::Heal { base, ap } => self.heal(target, base[r] + ap * cs.ap),
                Op::Shield { base, ap, secs } => self.shield(target, base[r] + ap * cs.ap, secs),
                Op::Stun(s) => {
                    if self.cc_lands(target) {
                        let cc = &mut self.units[target].cc;
                        cc.stun = cc.stun.max(now + ticks(s));
                    }
                }
                Op::Root(s) => {
                    if self.cc_lands(target) {
                        let cc = &mut self.units[target].cc;
                        cc.root = cc.root.max(now + ticks(s));
                    }
                }
                Op::Silence(s) => {
                    if self.cc_lands(target) {
                        let cc = &mut self.units[target].cc;
                        cc.silence = cc.silence.max(now + ticks(s));
                    }
                }
                Op::Airborne(s) => {
                    if self.cc_lands(target) {
                        let cc = &mut self.units[target].cc;
                        cc.airborne = cc.airborne.max(now + ticks(s));
                    }
                }
                Op::Slow { share, secs } => {
                    if self.cc_lands(target) {
                        let cc = &mut self.units[target].cc;
                        if cc.slow_until <= now || share >= cc.slow {
                            cc.slow = share.min(0.9);
                        }
                        cc.slow_until = cc.slow_until.max(now + ticks(secs));
                    }
                }
                Op::Knockback(d) => {
                    if self.cc_lands(target) {
                        let from = self.units[caster].pos;
                        let at = self.units[target].pos;
                        let dir = math::normalize_or(at - from, Vec2::X);
                        self.displace(target, at + dir * d);
                    }
                }
                Op::Pull => {
                    if self.cc_lands(target) && caster != target {
                        let c = &self.units[caster];
                        let at = self.units[target].pos;
                        let dir = math::normalize_or(at - c.pos, Vec2::X);
                        let to = c.pos + dir * (c.radius + self.units[target].radius + 10.0);
                        self.displace(target, to);
                    }
                }
                Op::Buff { kind, secs, value } => {
                    // A mark remembers who made it: only they may dash to it.
                    let value = if kind == BuffKind::Marked { caster as f32 } else { value };
                    self.units[target].add_buff(kind, now + ticks(secs), value, now);
                }
                Op::SelfBuff { kind, secs, value } => {
                    self.units[caster].add_buff(kind, now + ticks(secs), value, now)
                }
                Op::Reveal(s) => {
                    let u = &mut self.units[target];
                    u.revealed = u.revealed.max(now + ticks(s));
                }
            }
        }
    }

    /// Moves a unit toward `to` as far as the ground lets it, at once.
    fn displace(&mut self, i: usize, to: Vec2) {
        let from = self.units[i].pos;
        let mut at = from;
        let steps = (math::distance(from, to) / 16.0) as u32 + 1;
        for k in 1..=steps {
            let p = from + (to - from) * (k as f32 / steps as f32);
            if !self.map.walkable(p) {
                break;
            }
            at = p;
        }
        if at != from {
            let u = &mut self.units[i];
            u.pos = at;
            u.path.clear();
            u.repath_at = self.tick;
            self.event(Event::Blink { unit: i as u16, from, to: at });
        }
    }

    /// `victim` is dead, by `killer`'s hand: rewards, timers, and what its death opens.
    pub(crate) fn kill(&mut self, victim: usize, killer: usize) {
        let now = self.tick;
        let v = self.units[victim];
        self.units[victim].alive = false;
        self.units[victim].hp = 0.0;
        self.event(Event::Death { unit: victim as u16, killer: killer as u16 });
        let enemy = 1 - v.team;
        match v.kind {
            Kind::Champion => {
                let seat = victim;
                let respawn = self.headliner().respawn;
                let c = &mut self.champs[seat];
                c.deaths += 1;
                c.dead_until = now + ticks(rules::death_secs(c.level) * respawn);
                c.cast = Casting::default();
                c.dash = Dash::default();
                let window = ticks(ASSIST_SECS);
                let recent = |at: u32| at > 0 && now.saturating_sub(at) <= window;
                let credit = if killer < SEATS {
                    Some(killer)
                } else if v.last_hit_by > 0 && recent(v.last_hit_at) {
                    Some(usize::from(v.last_hit_by - 1))
                } else {
                    None
                };
                if let Some(k) = credit {
                    self.champs[k].gold += rules::gold_for(Kind::Champion);
                    self.champs[k].kills += 1;
                    self.takedown(k);
                }
                let assists = (0..SEATS)
                    .filter(|&s| team_of_seat(s) == enemy && Some(s) != credit && recent(v.hurt_by[s]))
                    .count();
                for s in 0..SEATS {
                    if team_of_seat(s) == enemy && Some(s) != credit && recent(v.hurt_by[s]) {
                        self.champs[s].assists += 1;
                        self.champs[s].gold += ASSIST_GOLD / assists as f32;
                        self.takedown(s);
                    }
                }
                let level = self.champs[seat].level;
                self.share_xp(enemy, v.pos, rules::xp_for(Kind::Champion, level));
            }
            k if k.is_minion() => {
                self.free_minion(victim);
                if killer < SEATS {
                    self.champs[killer].gold += rules::gold_for(k);
                    self.champs[killer].last_hits += 1;
                }
                self.share_xp(enemy, v.pos, rules::xp_for(k, 1));
            }
            Kind::Turret => {
                for s in 0..SEATS {
                    if team_of_seat(s) == enemy {
                        self.champs[s].gold += rules::TURRET_GOLD;
                    }
                }
            }
            Kind::Inhibitor => {
                for s in 0..SEATS {
                    if team_of_seat(s) == enemy {
                        self.champs[s].gold += rules::INHIBITOR_GOLD;
                    }
                }
                self.inhib_back[usize::from(v.team)] = now + ticks(rules::INHIBITOR_RESPAWN);
            }
            Kind::Nexus => {
                self.outcome = Some(enemy);
                self.event(Event::Outcome { winner: enemy });
            }
            _ => {}
        }
    }

    /// Experience for `team`'s living champions near `at`, shared.
    fn share_xp(&mut self, team: u8, at: Vec2, xp: f32) {
        let near = |m: &Match, s: usize| {
            team_of_seat(s) == team
                && m.units[s].alive
                && (m.units[s].pos - at).length_squared() <= rules::XP_RADIUS * rules::XP_RADIUS
        };
        let n = (0..SEATS).filter(|&s| near(self, s)).count();
        if n == 0 {
            return;
        }
        let each = xp * (1.0 / n as f32).max(rules::XP_SHARE_MIN);
        for s in 0..SEATS {
            if near(self, s) {
                self.gain_xp(s, each);
            }
        }
    }

    // ---- auto attacks --------------------------------------------------------------------

    /// Ticks between attacks for unit `i`.
    pub(crate) fn period_ticks(&self, i: usize) -> u32 {
        let aps = self.units[i].stats.attack_speed.clamp(0.2, 2.5);
        ((crate::TICK_HZ as f32 / aps + 0.5) as u32).max(1)
    }

    fn windup_share(&self, i: usize) -> f32 {
        match self.units[i].kind {
            Kind::Champion => self.champs[i].def().windup,
            Kind::Turret | Kind::Fountain => 0.0,
            _ => 0.3,
        }
    }

    /// Unit `i` attacks `t` if it's in range: starts the windup when the last attack is done.
    /// Returns whether `t` is in range (so the attacker stands rather than walks).
    pub(crate) fn try_attack(&mut self, i: usize, t: usize) -> bool {
        let range = self.units[i].stats.range;
        if !self.in_range(i, t, range) {
            return false;
        }
        let now = self.tick;
        let to = self.units[t].pos - self.units[i].pos;
        if to.length_squared() > 1.0 {
            self.units[i].facing = math::angle(to);
        }
        let u = &self.units[i];
        if u.windup > 0 || u.attack_ready > now {
            return true;
        }
        let period = self.period_ticks(i);
        let windup = ((period as f32 * self.windup_share(i) + 0.5) as u16).max(1);
        let handle = self.units[t].handle(t);
        let u = &mut self.units[i];
        u.windup = windup;
        u.target = handle;
        u.attack_ready = now + period;
        true
    }

    /// One tick of a windup; the attack lets go at its end, if its target is still there.
    pub(crate) fn attack_tick(&mut self, i: usize) {
        self.units[i].windup -= 1;
        if self.units[i].windup > 0 {
            return;
        }
        let Some(t) = self.live(self.units[i].target) else { return };
        let range = self.units[i].stats.range + 150.0;
        if !self.in_range(i, t, range) {
            return;
        }
        self.event(Event::Attack { unit: i as u16, target: t as u16 });
        let damage = self.units[i].stats.ad;
        let speed = match self.units[i].kind {
            Kind::Champion => self.champs[i].def().missile_speed,
            k if k.is_minion() => rules::minion_def(k).missile_speed,
            _ => 0.0,
        };
        if speed > 0.0 {
            self.spawn_homing(i, t, Source::Auto, 0, speed, damage);
        } else {
            self.auto_landed(i, t, damage);
        }
    }

    /// An auto attack landed: its damage, lifesteal, and on-hit augments.
    pub(crate) fn auto_landed(&mut self, attacker: usize, target: usize, damage: f32) {
        let dealt = self.deal(attacker, target, damage, DmgKind::Physical, false);
        if attacker < SEATS && self.units[attacker].alive {
            let steal = self.units[attacker].stats.lifesteal;
            if steal > 0.0 {
                self.heal(attacker, dealt * steal);
            }
            self.auto_hit_triggers(attacker, target);
        }
    }

    /// The nearest unit `i` may attack within `reach` of it (edge to edge), champions or not.
    pub(crate) fn nearest_enemy(
        &self,
        i: usize,
        reach: f32,
        champions: bool,
        minions: bool,
    ) -> Option<usize> {
        let team = self.units[i].team;
        let at = self.units[i].pos;
        let mut best: Option<(f32, usize)> = None;
        for (j, u) in self.units.iter().enumerate() {
            let kind_ok = (champions && u.kind == Kind::Champion) || (minions && u.kind.is_minion());
            if !kind_ok || !self.attackable(team, j) {
                continue;
            }
            let r = reach + self.units[i].radius + u.radius;
            let d2 = (u.pos - at).length_squared();
            if d2 <= r * r && best.is_none_or(|(b, _)| d2 < b) {
                best = Some((d2, j));
            }
        }
        best.map(|(_, j)| j)
    }

    /// Whether the champion of `seat` has a mark on a unit it may dash to.
    pub fn marked_target(&self, seat: usize) -> Option<usize> {
        let team = team_of_seat(seat);
        let mark = BuffKind::Marked as u8;
        (0..self.units.len()).find(|&j| {
            let u = &self.units[j];
            u.alive
                && u.team != team
                && u.buffs.iter().any(|b| b.kind == mark && b.until > self.tick && b.value == seat as f32)
        })
    }
}
