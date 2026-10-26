//! The zones the team sees, from their spawn events alone, as missiles are: an area that lands
//! late (what its telegraph fills toward) or a field that pulses (`docs/FX.md`, "The wire"). A
//! zone that follows its owner stands wherever the owner is drawn.

use el_proto::event::WireEvent;
use el_proto::quant::pos_from_code;
use el_sim::missile::Source;
use el_sim::zone::MAX_ZONES;
use glam::Vec2;

/// A zone as the client has it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Field {
    pub alive: bool,
    pub generation: u8,
    pub team: u8,
    pub source: Source,
    /// Its caster's slot, or [`crate::missiles::HIDDEN`].
    pub owner: u8,
    pub follows: bool,
    /// Where it was when told (where it stays, unless it follows).
    pub at: Vec2,
    pub radius: f32,
    /// The tick it first fires (as told: a field told late, its next pulse), every `period`
    /// ticks after that (0: once), and its last.
    pub next: u32,
    pub period: u32,
    pub end: u32,
}

impl Field {
    /// The tick it fires next at or after `t` (fractional), if it does again.
    pub fn next_at(&self, t: f64) -> Option<f64> {
        let first = f64::from(self.next);
        if t <= first {
            return Some(first);
        }
        if self.period == 0 {
            return None;
        }
        let p = f64::from(self.period);
        let at = first + ((t - first) / p).ceil() * p;
        (at <= f64::from(self.end)).then_some(at)
    }

    /// How far toward its first landing it is at tick `t`, 0 to 1, from `since` (the tick the
    /// client took it): what a telegraph fills by.
    pub fn charge(&self, since: u32, t: f64) -> f32 {
        let span = f64::from(self.next.saturating_sub(since)).max(1.0);
        ((t - f64::from(since)) / span).clamp(0.0, 1.0) as f32
    }
}

/// Every zone slot the server has, as the team was told of it.
pub struct Zones {
    pub fields: [Field; MAX_ZONES],
    /// The tick each was told at.
    pub since: [u32; MAX_ZONES],
}

impl Default for Zones {
    fn default() -> Self {
        Self { fields: [Field::default(); MAX_ZONES], since: [0; MAX_ZONES] }
    }
}

impl Zones {
    pub fn clear(&mut self) {
        self.fields.fill(Field::default());
    }

    /// Takes a `ZoneSpawn` or a `ZoneEnd` of tick `tick`; anything else is no business of this.
    pub fn take(&mut self, tick: u32, e: &WireEvent) {
        match *e {
            WireEvent::ZoneSpawn {
                id,
                generation,
                team,
                source,
                owner,
                follows,
                at,
                radius,
                next,
                period,
                end,
            } => {
                let i = usize::from(id);
                let Some(f) = self.fields.get_mut(i) else { return };
                *f = Field {
                    alive: true,
                    generation,
                    team,
                    source: Source::from_code(u32::from(source)),
                    owner,
                    follows,
                    at: pos_from_code(at),
                    radius: f32::from(radius),
                    next,
                    period: u32::from(period),
                    end,
                };
                self.since[i] = tick;
            }
            WireEvent::ZoneEnd { id, generation } => {
                if let Some(f) = self.fields.get_mut(usize::from(id))
                    && f.generation == generation
                {
                    f.alive = false;
                }
            }
            _ => {}
        }
    }

    /// Forgets the zones whose last tick is past `now` (a zone's end may never be told: its
    /// `ZoneEnd` is only for the team that was told it).
    pub fn expire(&mut self, now: u32) {
        for f in self.fields.iter_mut().filter(|f| f.alive && f.end < now) {
            f.alive = false;
        }
    }

    pub fn live(&self) -> impl Iterator<Item = (usize, &Field)> {
        self.fields.iter().enumerate().filter(|(_, f)| f.alive)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use el_proto::quant::pos_code;

    #[test]
    fn a_zone_is_told_fills_toward_its_landing_and_pulses_until_its_end() {
        let mut z = Zones::default();
        z.take(
            100,
            &WireEvent::ZoneSpawn {
                id: 5,
                generation: 2,
                team: 1,
                source: 0x10A,
                owner: 255,
                follows: false,
                at: pos_code(Vec2::new(3_000.0, 1_500.0)),
                radius: 300,
                next: 130,
                period: 15,
                end: 190,
            },
        );
        let f = z.fields[5];
        assert!(f.alive && (f.at - Vec2::new(3_000.0, 1_500.0)).length() < 2.0);
        assert_eq!(f.source, Source::Ability { champ: 1, slot: 2 });
        assert!((f.charge(z.since[5], 115.0) - 0.5).abs() < 1e-6);
        assert_eq!(f.next_at(120.0), Some(130.0));
        assert_eq!(f.next_at(131.0), Some(145.0));
        assert_eq!(f.next_at(176.0), Some(190.0), "its last tick is a pulse");
        assert_eq!(f.next_at(191.0), None, "none past it");
        z.take(140, &WireEvent::ZoneEnd { id: 5, generation: 1 });
        assert!(z.fields[5].alive, "another life's end isn't this one's");
        z.expire(191);
        assert_eq!(z.live().count(), 0);
    }
}
