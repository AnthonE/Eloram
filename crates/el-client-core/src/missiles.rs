//! The missiles the team sees, from their spawn events alone (`docs/NETCODE.md`, "Closed forms go
//! once"): a straight one is where its closed form says at the tick drawn, and the client draws it
//! at its own predicted clock, so where it is on screen is where the server will judge it.

use el_proto::event::WireEvent;
use el_proto::quant::{facing_from_code, pos_from_code};
use el_sim::missile::{MAX_MISSILES, Source};
use glam::Vec2;

/// "Not seen" (a caster or target the team can't see).
pub const HIDDEN: u8 = u8::MAX;

/// A missile as the client has it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shot {
    pub alive: bool,
    pub generation: u8,
    pub team: u8,
    pub source: Source,
    /// Its caster's slot, or [`HIDDEN`].
    pub owner: u8,
    /// Where it was at `t0`.
    pub origin: Vec2,
    pub t0: u32,
    pub dir: Vec2,
    /// Units a second.
    pub speed: f32,
    /// How far past `origin` a straight one flies (0 for a homing one).
    pub range: f32,
    pub width: f32,
    /// The slot a homing one follows, or [`HIDDEN`] for a straight one (or a homing one whose
    /// target the team can't see).
    pub target: u8,
    pub homing: bool,
}

impl Shot {
    /// How far it has flown by tick `t` (fractional).
    pub fn flown(&self, t: f64) -> f32 {
        let d = self.speed * ((t - f64::from(self.t0)).max(0.0) / f64::from(el_proto::TICK_HZ)) as f32;
        if self.homing { d } else { d.min(self.range) }
    }

    /// A straight one at tick `t`; a homing one given where its target is drawn.
    pub fn pos(&self, t: f64, target: Option<Vec2>) -> Vec2 {
        let d = self.flown(t);
        match (self.homing, target) {
            (true, Some(to)) => {
                let gap = to - self.origin;
                let len = gap.length();
                if len <= d { to } else { self.origin + gap * (d / len) }
            }
            _ => self.origin + self.dir * d,
        }
    }
}

/// Every missile slot the server has, as the team was told of it.
pub struct Missiles {
    pub shots: Box<[Shot]>,
}

impl Default for Missiles {
    fn default() -> Self {
        Self { shots: vec![Shot::default(); MAX_MISSILES].into_boxed_slice() }
    }
}

impl Missiles {
    pub fn clear(&mut self) {
        self.shots.fill(Shot::default());
    }

    /// Takes a `MissileSpawn` or a `MissileEnd`; anything else is no business of this.
    pub fn take(&mut self, e: &WireEvent) {
        match *e {
            WireEvent::MissileSpawn {
                id,
                generation,
                team,
                source,
                owner,
                origin,
                t0,
                target,
                dir,
                speed,
                range,
                width,
            } => {
                let Some(s) = self.shots.get_mut(usize::from(id)) else { return };
                let homing = range == 0;
                *s = Shot {
                    alive: true,
                    generation,
                    team,
                    source: Source::from_code(u32::from(source)),
                    owner,
                    origin: pos_from_code(origin),
                    t0,
                    dir: Vec2::from_angle(facing_from_code(dir)),
                    speed: f32::from(speed),
                    range: f32::from(range),
                    width: f32::from(width),
                    target,
                    homing,
                };
            }
            WireEvent::MissileEnd { id, generation, .. } => {
                if let Some(s) = self.shots.get_mut(usize::from(id))
                    && s.generation == generation
                {
                    s.alive = false;
                }
            }
            _ => {}
        }
    }

    pub fn live(&self) -> impl Iterator<Item = (usize, &Shot)> {
        self.shots.iter().enumerate().filter(|(_, s)| s.alive)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use el_proto::quant::{facing_code, pos_code};

    #[test]
    fn a_straight_missile_flies_its_closed_form_and_stops_at_its_range() {
        let mut m = Missiles::default();
        m.take(&WireEvent::MissileSpawn {
            id: 7,
            generation: 2,
            team: 0,
            source: 0x101,
            owner: 3,
            origin: pos_code(Vec2::new(2_000.0, 1_900.0)),
            t0: 300,
            target: HIDDEN,
            dir: facing_code(0.0),
            speed: 1_500,
            range: 1_000,
            width: 70,
        });
        let s = m.shots[7];
        assert!(s.alive && !s.homing);
        assert!((s.pos(315.0, None).x - 2_750.0).abs() < 2.0);
        assert!((s.pos(400.0, None).x - 3_000.0).abs() < 2.0, "no further than its range");
        m.take(&WireEvent::MissileEnd { id: 7, generation: 1, at: [0, 0] });
        assert!(m.shots[7].alive, "another life's end isn't this one's");
        m.take(&WireEvent::MissileEnd { id: 7, generation: 2, at: [0, 0] });
        assert_eq!(m.live().count(), 0);
    }
}
