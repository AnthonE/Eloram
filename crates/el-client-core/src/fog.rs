//! The team's sight, for drawing its fog: the grid the server worked out for the team, worked out
//! again from the allies the client holds (every ally is always sent), with the same sight windows
//! and brush (`el_sim::vision`). It decides nothing: what the team may see is the server's, and
//! only what it sent is drawn.

use el_sim::Match;
use el_sim::map::{self, VIS_H, VIS_WORDS};
use el_sim::units::{Kind, SEATS};

/// A team's lit vision cells, a bit each, rows of [`VIS_WORDS`] words.
pub type Grid = [u64; VIS_WORDS * VIS_H];

/// What `team` sees in `m`, as its allies stand there.
pub fn team_grid(m: &Match, team: u8) -> Grid {
    let mut grid = [0u64; VIS_WORDS * VIS_H];
    let mut brushes = 0u32;
    for (i, u) in m.units.iter().enumerate() {
        if !u.alive || u.team != team {
            continue;
        }
        let class = match u.kind {
            Kind::Champion => map::SIGHT_CHAMPION,
            k if k.is_minion() => map::SIGHT_MINION,
            _ => map::SIGHT_STRUCTURE,
        };
        m.map.stamp_sight(u.pos, class, &mut grid);
        let b = m.map.brush_at(u.pos);
        if b > 0 && (i < SEATS || u.kind.is_minion()) {
            brushes |= 1 << (b - 1);
        }
    }
    for b in 0..m.map.brush_count() {
        if brushes & (1 << b) == 0 {
            m.map.hide_brush(b, &mut grid);
        }
    }
    grid
}

/// Whether vision cell `(x, y)` is lit in `grid`.
pub fn lit(grid: &Grid, x: usize, y: usize) -> bool {
    grid[y * VIS_WORDS + x / 64] & (1 << (x % 64)) != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use el_sim::game::SeatInput;
    use el_sim::map::Map;
    use el_sim::path::PathScratch;
    use el_sim::{bot, ticks};

    #[test]
    fn the_fog_drawn_is_the_fog_the_server_keeps() {
        let picks = std::array::from_fn(|s| (s % el_sim::content::champions::CHAMPIONS.len()) as u8);
        let mut m = Match::new(Map::leaked(), 3, picks);
        let mut scratch = PathScratch::new();
        for t in 0..ticks(120.0) {
            let mut inputs = [SeatInput::default(); SEATS];
            for (s, input) in inputs.iter_mut().enumerate() {
                bot::think(&m, s, input);
            }
            m.step(&inputs, &mut scratch);
            if t % 30 == 0 {
                for team in 0..2 {
                    assert!(
                        team_grid(&m, team) == m.vision.grid[usize::from(team)],
                        "tick {}, team {team}",
                        m.tick
                    );
                }
            }
        }
    }
}
