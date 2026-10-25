//! What the map does by itself: minion waves, health relics, inhibitors coming back.

use glam::Vec2;

use crate::content::rules;
use crate::events::Event;
use crate::game::Match;
use crate::map;
use crate::ticks;
use crate::units::{Kind, SEATS, structure, structure_slot};

impl Match {
    pub(crate) fn world_tick(&mut self) {
        let now = self.tick;
        if now >= self.next_wave {
            for team in [map::DAWN, map::DUSK] {
                self.spawn_wave(team);
            }
            self.waves += 1;
            let gap = (rules::WAVE_SECS_START - rules::WAVE_SECS_STEP * f32::from(self.waves))
                .max(rules::WAVE_SECS_MIN);
            self.next_wave += ticks(gap);
        }
        for k in 0..self.relics.len() {
            self.relic_tick(k);
        }
        for team in [map::DAWN, map::DUSK] {
            let back = self.inhib_back[usize::from(team)];
            if back != 0 && now >= back {
                self.inhib_back[usize::from(team)] = 0;
                let i = structure_slot(team, structure::INHIBITOR);
                let u = &mut self.units[i];
                u.alive = true;
                u.hp = u.stats.max_hp;
                u.generation = u.generation.wrapping_add(1);
            }
        }
    }

    /// Three melee and three casters; a siege minion every second wave from the third; a super
    /// minion while the enemy's inhibitor is down.
    fn spawn_wave(&mut self, team: u8) {
        let start = map::wave_start(team);
        let forward = if team == map::DAWN { Vec2::X } else { -Vec2::X };
        let side = Vec2::new(-forward.y, forward.x);
        let w = self.waves;
        let mut kinds = [Kind::Melee; 9];
        let mut n = 0;
        let mut add = |k: Kind| {
            kinds[n] = k;
            n += 1;
        };
        for _ in 0..3 {
            add(Kind::Melee);
        }
        if w >= 2 && w.is_multiple_of(2) {
            add(Kind::Siege);
        }
        if self.inhibitor_down(1 - team) {
            add(Kind::Super);
        }
        for _ in 0..3 {
            add(Kind::Caster);
        }
        for (k, &kind) in kinds[..n].iter().enumerate() {
            let row = (k / 3) as f32;
            let col = (k % 3) as f32 - 1.0;
            let pos = self.map.snap(start - forward * (row * 110.0) + side * (col * 100.0));
            self.spawn_minion(team, kind, pos);
        }
    }

    fn relic_tick(&mut self, k: usize) {
        let now = self.tick;
        let r = self.relics[k];
        if r.beam_at != 0 && now >= r.beam_at {
            self.relics[k].beam_at = 0;
            self.event(Event::RelicBeam { pad: k as u8 });
            for s in 0..SEATS {
                let u = &mut self.units[s];
                let reach = rules::RELIC_BEAM_RADIUS;
                if u.alive && (u.pos - r.pos).length_squared() <= reach * reach {
                    u.hp += (u.stats.max_hp - u.hp) * rules::RELIC_BEAM;
                    u.mana += (u.stats.max_mana - u.mana) * rules::RELIC_BEAM;
                }
            }
        }
        if !r.up {
            if r.beam_at == 0 && now >= r.next {
                self.relics[k].up = true;
            }
            return;
        }
        let taker = (0..SEATS).find(|&s| {
            let u = &self.units[s];
            u.alive && (u.pos - r.pos).length_squared() <= rules::RELIC_PICKUP * rules::RELIC_PICKUP
        });
        if let Some(s) = taker {
            let u = &mut self.units[s];
            u.hp += (u.stats.max_hp - u.hp) * rules::RELIC_NOW;
            u.mana += (u.stats.max_mana - u.mana) * rules::RELIC_NOW;
            self.relics[k].up = false;
            self.relics[k].beam_at = now + ticks(rules::RELIC_BEAM_DELAY);
            self.relics[k].next = now + ticks(rules::RELIC_RESPAWN);
            self.event(Event::RelicTaken { pad: k as u8, unit: s as u16 });
        }
    }
}
