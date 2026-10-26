//! Missiles: skillshots fly a straight line from where they were cast, a closed form of the tick
//! (`origin + dir × speed × (t − t0)`), so a client draws one from its spawn event alone; homing
//! missiles (autos, tower shots, targeted spells) follow their target and can't miss. Hits are
//! judged on the server's present (`docs/NETCODE.md`, "Closed forms go once").

use glam::Vec2;

use crate::content::champions::ability;
use crate::content::{DmgKind, Effect, Op};
use crate::game::Match;
use crate::handle::Handle;
use crate::math;

pub const MAX_MISSILES: usize = 512;
/// Bits a [`Source::code`] takes on the wire.
pub const CODE_BITS: u32 = el_proto::event::SOURCE_BITS;
const _: () = assert!(Source::Augment(u8::MAX).code() < 1 << CODE_BITS);

/// Where a missile (or a zone) came from: what it does when it lands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    Auto,
    Turret,
    /// The platform's laser.
    Fountain,
    Ability {
        champ: u8,
        slot: u8,
    },
    Relic,
    Augment(u8),
}

impl Source {
    /// The ops it does, if it's an ability's.
    pub fn ops(self) -> &'static [Op] {
        match self {
            Source::Ability { champ, slot } => ability(champ, slot).effect.ops(),
            _ => &[],
        }
    }

    /// The source a [`code`](Self::code) stands for.
    pub fn from_code(c: u32) -> Source {
        match c {
            1 => Source::Turret,
            2 => Source::Fountain,
            3 => Source::Relic,
            c if c & 0x200 != 0 => Source::Augment((c & 0xFF) as u8),
            c if c & 0x100 != 0 => Source::Ability { champ: ((c >> 3) & 0x1F) as u8, slot: (c & 0x7) as u8 },
            _ => Source::Auto,
        }
    }

    /// A small number for hashes and the wire, in [`CODE_BITS`]: a champion in five bits and a
    /// slot in three.
    pub const fn code(self) -> u32 {
        match self {
            Source::Auto => 0,
            Source::Turret => 1,
            Source::Fountain => 2,
            Source::Ability { champ, slot } => 0x100 | ((champ & 0x1F) as u32) << 3 | (slot & 0x7) as u32,
            Source::Relic => 3,
            Source::Augment(a) => 0x200 | a as u32,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Missile {
    pub alive: bool,
    pub generation: u8,
    pub team: u8,
    pub owner: Handle,
    pub source: Source,
    pub rank: u8,
    /// Follows `target` (else flies `dir` from `origin`).
    pub homing: bool,
    pub target: Handle,
    pub dir: Vec2,
    pub range: f32,
    pub width: f32,
    pub pierce: bool,
    pub origin: Vec2,
    pub pos: Vec2,
    pub speed: f32,
    pub t0: u32,
    /// What an auto or a tower shot does when it lands (worked out as it left).
    pub damage: f32,
    /// Units a piercing missile has hit already.
    pub hits: [u64; 4],
}

impl Missile {
    /// Where a straight missile is at tick `t` (the closed form both ends use).
    pub fn line_pos(&self, t: u32) -> Vec2 {
        let d = (self.speed * crate::DT * t.saturating_sub(self.t0) as f32).min(self.range);
        self.origin + self.dir * d
    }
}

impl Match {
    /// Fires a straight missile; returns its index.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spawn_line(
        &mut self,
        owner: usize,
        source: Source,
        rank: u8,
        dir: Vec2,
        range: f32,
        speed: f32,
        width: f32,
        pierce: bool,
    ) -> Option<usize> {
        let u = &self.units[owner];
        let m = Missile {
            alive: true,
            generation: 0,
            team: u.team,
            owner: u.handle(owner),
            source,
            rank,
            homing: false,
            target: Handle::NONE,
            dir,
            range,
            width,
            pierce,
            origin: u.pos,
            pos: u.pos,
            speed,
            t0: self.tick,
            damage: 0.0,
            hits: [0; 4],
        };
        self.add_missile(m)
    }

    /// Fires a missile that follows `target`.
    pub(crate) fn spawn_homing(
        &mut self,
        owner: usize,
        target: usize,
        source: Source,
        rank: u8,
        speed: f32,
        damage: f32,
    ) -> Option<usize> {
        let u = &self.units[owner];
        let m = Missile {
            alive: true,
            generation: 0,
            team: u.team,
            owner: u.handle(owner),
            source,
            rank,
            homing: true,
            target: self.units[target].handle(target),
            dir: math::normalize_or(self.units[target].pos - u.pos, Vec2::X),
            range: 0.0,
            width: 0.0,
            pierce: false,
            origin: u.pos,
            pos: u.pos,
            speed,
            t0: self.tick,
            damage,
            hits: [0; 4],
        };
        self.add_missile(m)
    }

    fn add_missile(&mut self, mut m: Missile) -> Option<usize> {
        let i = self.missiles.iter().position(|k| !k.alive)?;
        m.generation = self.missiles[i].generation.wrapping_add(1);
        self.missiles[i] = m;
        self.event(crate::events::Event::MissileSpawn { missile: i as u16 });
        Some(i)
    }

    fn end_missile(&mut self, i: usize) {
        let at = self.missiles[i].pos;
        self.missiles[i].alive = false;
        self.event(crate::events::Event::MissileEnd { missile: i as u16, at });
    }

    /// Moves every missile and lands the ones that hit.
    pub(crate) fn step_missiles(&mut self) {
        for i in 0..self.missiles.len() {
            if !self.missiles[i].alive {
                continue;
            }
            let m = self.missiles[i];
            if m.homing {
                let Some(t) = self.live(m.target) else {
                    self.end_missile(i);
                    continue;
                };
                let goal = self.units[t].pos;
                let step = m.speed * crate::DT;
                let reach = self.units[t].radius;
                if math::distance(m.pos, goal) <= step + reach {
                    self.missiles[i].pos = goal;
                    self.land(i, t);
                    self.end_missile(i);
                } else {
                    let p = math::move_toward(m.pos, goal, step);
                    self.missiles[i].dir = math::normalize_or(goal - m.pos, m.dir);
                    self.missiles[i].pos = p;
                }
                continue;
            }
            // A straight missile: from where it was to where the closed form puts it now, the
            // nearest enemy the segment touches.
            let from = m.pos;
            let to = m.line_pos(self.tick + 1);
            // Out of range once the closed form stops moving it.
            let mut ended = m.line_pos(self.tick + 1) == m.line_pos(self.tick + 2);
            let mut stopped_at = None;
            loop {
                let mut best: Option<(f32, usize)> = None;
                let seg = (to - from).dot(m.dir);
                for (j, u) in self.units.iter().enumerate() {
                    let hit = self.missiles[i].hits[j / 64] & (1 << (j % 64)) != 0;
                    if !u.alive || u.team == m.team || hit || u.kind.is_structure() {
                        continue;
                    }
                    let along = (u.pos - from).dot(m.dir);
                    if along < -u.radius || along > seg + u.radius {
                        continue;
                    }
                    let closest = from + m.dir * along.clamp(0.0, seg.max(0.0));
                    let reach = u.radius + m.width;
                    if (u.pos - closest).length_squared() <= reach * reach
                        && best.is_none_or(|(b, _)| along < b)
                    {
                        best = Some((along, j));
                    }
                }
                let Some((along, j)) = best else { break };
                self.missiles[i].hits[j / 64] |= 1 << (j % 64);
                self.land(i, j);
                if !m.pierce {
                    stopped_at = Some(from + m.dir * along.clamp(0.0, seg.max(0.0)));
                    ended = true;
                    break;
                }
            }
            self.missiles[i].pos = stopped_at.unwrap_or(to);
            if ended {
                self.end_missile(i);
            }
        }
    }

    /// What a missile does to the unit it reached.
    fn land(&mut self, i: usize, target: usize) {
        let m = self.missiles[i];
        let Some(owner) = self.live_or_dead(m.owner) else { return };
        match m.source {
            Source::Auto => self.auto_landed(owner, target, m.damage),
            Source::Turret => {
                self.deal(owner, target, m.damage, DmgKind::Physical, Source::Turret);
            }
            src => {
                let ops = src.ops();
                self.apply_ops(owner, target, ops, m.rank, src);
                if let Source::Ability { .. } = src {
                    self.ability_hit(owner, target);
                }
            }
        }
    }

    /// The live unit a handle names.
    pub fn live(&self, h: Handle) -> Option<usize> {
        let i = usize::from(h.idx);
        (!h.is_none()
            && i < self.units.len()
            && self.units[i].alive
            && self.units[i].generation == h.generation)
            .then_some(i)
    }

    /// The unit a handle names, alive or not (a missile's owner may have died while it flew).
    pub fn live_or_dead(&self, h: Handle) -> Option<usize> {
        let i = usize::from(h.idx);
        (!h.is_none() && i < self.units.len() && self.units[i].generation == h.generation).then_some(i)
    }
}

/// An ability's effect, if it's a straight missile: its speed, width and whether it pierces.
pub fn line_of(effect: &Effect) -> Option<(f32, f32, bool)> {
    match *effect {
        Effect::Line { speed, width, pierce, .. } => Some((speed, width, pierce)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::champions::{CHAMPIONS, MARK_SLOT};

    #[test]
    fn every_source_has_its_own_code() {
        const SLOTS: usize = MARK_SLOT as usize + 1;
        let mut all = [Source::Auto; 4 + CHAMPIONS.len() * SLOTS + 256];
        all[..4].copy_from_slice(&[Source::Auto, Source::Turret, Source::Fountain, Source::Relic]);
        for (k, s) in all[4..4 + CHAMPIONS.len() * SLOTS].iter_mut().enumerate() {
            *s = Source::Ability { champ: (k / SLOTS) as u8, slot: (k % SLOTS) as u8 };
        }
        for (a, s) in all[4 + CHAMPIONS.len() * SLOTS..].iter_mut().enumerate() {
            *s = Source::Augment(a as u8);
        }
        let mut codes = all.map(Source::code);
        for (s, &c) in all.iter().zip(&codes) {
            assert_eq!(Source::from_code(c), *s, "{s:?} comes back as itself");
            assert!(c < 1 << CODE_BITS);
        }
        codes.sort_unstable();
        assert!(codes.windows(2).all(|w| w[0] != w[1]), "no two sources share a code");
    }
}
