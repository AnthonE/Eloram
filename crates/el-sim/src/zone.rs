//! Zones: an area that lands after a delay (once), or a field that pulses for a while. Both are
//! closed forms of the tick they were cast: where, how wide, when they fire.

use glam::Vec2;

use crate::content::Filter;
use crate::events::Event;
use crate::game::Match;
use crate::handle::Handle;
use crate::missile::Source;
use crate::units::{Kind, SEATS};

pub const MAX_ZONES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zone {
    pub alive: bool,
    pub team: u8,
    pub owner: Handle,
    pub source: Source,
    pub rank: u8,
    pub pos: Vec2,
    pub radius: f32,
    pub filter: Filter,
    /// Moves with its owner.
    pub follows: bool,
    /// When it next fires, and the last tick it may.
    pub next: u32,
    pub end: u32,
    /// Ticks between pulses (0: it fires once).
    pub period: u32,
}

impl Default for Zone {
    fn default() -> Self {
        Self {
            alive: false,
            team: 0,
            owner: Handle::NONE,
            source: Source::Auto,
            rank: 0,
            pos: Vec2::ZERO,
            radius: 0.0,
            filter: Filter::Enemies,
            follows: false,
            next: 0,
            end: 0,
            period: 0,
        }
    }
}

impl Match {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spawn_zone(
        &mut self,
        owner: usize,
        source: Source,
        rank: u8,
        pos: Vec2,
        radius: f32,
        filter: Filter,
        follows: bool,
        delay: u32,
        lasts: u32,
        period: u32,
    ) {
        let Some(i) = self.zones.iter().position(|z| !z.alive) else { return };
        let u = &self.units[owner];
        let next = self.tick + delay;
        self.zones[i] = Zone {
            alive: true,
            team: u.team,
            owner: u.handle(owner),
            source,
            rank,
            pos,
            radius,
            filter,
            follows,
            next,
            end: next + lasts,
            period,
        };
    }

    /// Fires the zones whose time has come, on what's in them.
    pub(crate) fn step_zones(&mut self) {
        let now = self.tick;
        for i in 0..self.zones.len() {
            let z = self.zones[i];
            if !z.alive {
                continue;
            }
            let Some(owner) = self.live_or_dead(z.owner) else {
                self.zones[i].alive = false;
                continue;
            };
            if z.follows && self.units[owner].alive {
                self.zones[i].pos = self.units[owner].pos;
            }
            if now < z.next {
                continue;
            }
            let at = self.zones[i].pos;
            self.event(Event::Area { at, radius: z.radius, team: z.team });
            let ops = z.source.ops();
            for j in 0..self.units.len() {
                let u = &self.units[j];
                if !u.alive || u.kind == Kind::Fountain {
                    continue;
                }
                let reach = z.radius + u.radius;
                if (u.pos - at).length_squared() > reach * reach {
                    continue;
                }
                let hits = match z.filter {
                    Filter::Enemies => self.attackable(z.team, j) && !u.kind.is_structure(),
                    Filter::Allies => u.team == z.team && j < SEATS,
                };
                if hits {
                    self.apply_ops(owner, j, ops, z.rank, true);
                    if z.filter == Filter::Enemies {
                        self.ability_hit(owner, j);
                    }
                }
            }
            if z.period == 0 || now + z.period > z.end {
                self.zones[i].alive = false;
            } else {
                self.zones[i].next = now + z.period;
            }
        }
    }
}
