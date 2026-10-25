//! A unit as the wire carries it ([`el_proto::unit::QUnit`]), and back: what the server sends a
//! team, and what the client's model of the match takes from it.

use el_proto::quant::{
    dequantize_centered, facing_code, facing_from_code, pos_code, pos_from_code, quantize_centered,
};
use el_proto::unit::{self, QUnit, VEL_BITS, VEL_MAX, flags};
use glam::Vec2;

use crate::game::{Match, NO_SLOT};
use crate::units::{Kind, SEATS};

impl Match {
    /// Unit `i`'s record as `team` may have it: what it's attacking is named only if `team` sees
    /// that too.
    pub fn quantize(&self, i: usize, team: u8) -> QUnit {
        let now = self.tick;
        let u = &self.units[i];
        let max = u.stats.max_hp.max(1.0);
        let hp = if u.hp > 0.0 { ((u.hp / max) * 1023.0 + 0.5).clamp(1.0, 1023.0) as u32 } else { 0 };
        let mana = if u.stats.max_mana > 0.0 {
            ((u.mana / u.stats.max_mana).clamp(0.0, 1.0) * 127.0 + 0.5) as u32
        } else {
            0
        };
        let mut f = 0;
        let cc = &u.cc;
        for (on, bit) in [
            (cc.stun > now, flags::STUNNED),
            (cc.root > now, flags::ROOTED),
            (cc.silence > now, flags::SILENCED),
            (cc.airborne > now, flags::AIRBORNE),
            (cc.slow_until > now, flags::SLOWED),
            (u.windup > 0, flags::ATTACKING),
            (u.shield_until > now && u.shield > 0.0, flags::SHIELDED),
            (u.revealed > now, flags::REVEALED),
            (cc.immune > now, flags::IMMUNE),
        ] {
            if on {
                f |= bit;
            }
        }
        let (level, cast) = if i < SEATS {
            let c = &self.champs[i];
            if c.dash.active {
                f |= flags::DASHING;
            }
            (u32::from(c.level), if c.cast.slot == NO_SLOT { 0 } else { u32::from(c.cast.slot) + 1 })
        } else {
            (0, 0)
        };
        let target = self
            .live(u.target)
            .filter(|&t| self.vision.units[usize::from(team)].get(t))
            .map_or(unit::NO_TARGET, |t| t as u8);
        QUnit {
            slot: i as u8,
            generation: u.generation as u8,
            kind: u.kind as u8,
            team: u.team,
            pos: pos_code(u.pos),
            vel: [
                quantize_centered(u.moved.x, VEL_MAX, VEL_BITS),
                quantize_centered(u.moved.y, VEL_MAX, VEL_BITS),
            ],
            facing: facing_code(u.facing),
            hp,
            max_hp: (u.stats.max_hp + 0.5) as u32,
            mana,
            shield: if u.shield_until > now { (u.shield + 0.5) as u32 } else { 0 },
            level,
            flags: f,
            cast,
            target,
            corner: u.path.corner().map(pos_code),
        }
    }

    /// The client's model takes a record: the unit as the server says it is (seen by the team now).
    pub fn apply_record(&mut self, q: &QUnit) {
        let i = usize::from(q.slot);
        let now = self.tick;
        let kind = Kind::from_u8(q.kind);
        let u = &mut self.units[i];
        if !u.alive || u.generation as u8 != q.generation {
            u.buffs = Default::default();
            u.target = crate::handle::Handle::NONE;
        }
        u.alive = true;
        u.kind = kind;
        u.team = q.team;
        u.generation = (u.generation & 0xFF00) | u16::from(q.generation);
        u.pos = pos_from_code(q.pos);
        u.moved = Vec2::new(
            dequantize_centered(q.vel[0], VEL_MAX, VEL_BITS),
            dequantize_centered(q.vel[1], VEL_MAX, VEL_BITS),
        );
        u.facing = facing_from_code(q.facing);
        u.stats.max_hp = q.max_hp as f32;
        u.hp = q.hp as f32 / 1023.0 * u.stats.max_hp;
        u.shield = q.shield as f32;
        u.shield_until = if q.shield > 0 { now + 1 } else { 0 };
        if u.radius == 0.0 {
            u.radius = match kind {
                Kind::Champion => {
                    crate::content::champions::CHAMPIONS[usize::from(self.champs.get(i).map_or(0, |c| c.def))]
                        .radius
                }
                k if k.is_minion() => crate::content::rules::minion_def(k).radius,
                _ => 100.0,
            };
        }
        let hold = |on: bool| if on { now + 1 } else { 0 };
        u.cc.stun = hold(q.flags & flags::STUNNED != 0);
        u.cc.root = hold(q.flags & flags::ROOTED != 0);
        u.cc.silence = hold(q.flags & flags::SILENCED != 0);
        u.cc.airborne = hold(q.flags & flags::AIRBORNE != 0);
        u.revealed = hold(q.flags & flags::REVEALED != 0);
        u.path.clear();
        if let Some(c) = q.corner {
            u.path.points[0] = pos_from_code(c);
            u.path.len = 1;
        }
        if i < SEATS && q.level > 0 {
            self.champs[i].level = q.level as u8;
        }
        self.vision.units[usize::from(1 - q.team)].set(i, true);
    }

    /// The client's model forgets a unit (it left the team's sight, or died).
    pub fn forget(&mut self, slot: usize) {
        if let Some(u) = self.units.get_mut(slot) {
            u.alive = false;
            for seen in &mut self.vision.units {
                seen.set(slot, false);
            }
        }
    }
}
