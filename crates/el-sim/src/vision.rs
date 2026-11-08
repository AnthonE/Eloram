//! What each team sees: computed once a team a tick (two answers a unit, not ten), from the map's
//! precomputed sight windows, with brush hiding what's in it from a team with nobody inside
//! (`docs/NETCODE.md`, "Fog is the server's, a team at a time"). Replication sends a team only what
//! this says it sees; the client draws its fog from the same windows.

use crate::game::Match;
use crate::map::{self, Map};
use crate::units::{Kind, SEATS};

impl Match {
    pub(crate) fn update_vision(&mut self) {
        let now = self.tick;
        let (end, missile_end) = (self.live_end(), self.missile_end());
        for team in 0..2u8 {
            let t = usize::from(team);
            let mut grid = [0u64; map::VIS_WORDS * map::VIS_H];
            // The cells each sight radius has been stamped from: a wave's minions stand a few to a
            // cell, and a window ORed in twice lights nothing more.
            let mut stamped = [[0u64; map::VIS_WORDS * map::VIS_H]; map::SIGHT.len()];
            let mut brushes = 0u32;
            for (i, u) in self.units[..end].iter().enumerate() {
                if !u.alive || u.team != team {
                    continue;
                }
                let class = match u.kind {
                    Kind::Champion => map::SIGHT_CHAMPION,
                    k if k.is_minion() => map::SIGHT_MINION,
                    _ => map::SIGHT_STRUCTURE,
                };
                let (x, y) = Map::vis_cell(u.pos);
                let (word, bit) = (y * map::VIS_WORDS + x / 64, 1u64 << (x % 64));
                if stamped[class][word] & bit == 0 {
                    stamped[class][word] |= bit;
                    self.map.stamp_sight(u.pos, class, &mut grid);
                }
                let b = self.map.brush_at(u.pos);
                if b > 0 && (i < SEATS || u.kind.is_minion()) {
                    brushes |= 1 << (b - 1);
                }
            }
            for b in 0..self.map.brush_count() {
                if brushes & (1 << b) == 0 {
                    self.map.hide_brush(b, &mut grid);
                }
            }
            self.vision.grid[t] = grid;
            let seen = &mut self.vision.units[t];
            seen.clear();
            for (i, u) in self.units[..end].iter().enumerate() {
                if !u.alive {
                    continue;
                }
                // A unit in brush is seen only from in it: a unit's own cell may be lit by
                // sight through the brush's edge, so the brush test is the unit's, not the cell's.
                let b = self.map.brush_at(u.pos);
                let in_dark_brush = b > 0 && brushes & (1 << (b - 1)) == 0;
                let visible = u.team == team
                    || u.kind.is_structure()
                    || u.revealed > now
                    || (!in_dark_brush && Map::lit(&grid, u.pos));
                if visible {
                    seen.set(i, true);
                }
            }
            let seen_m = &mut self.vision.missiles[t];
            seen_m.clear();
            for (i, m) in self.missiles[..missile_end].iter().enumerate() {
                if m.alive && (m.team == team || Map::lit(&grid, m.pos)) {
                    seen_m.set(i, true);
                }
            }
        }
    }
}
