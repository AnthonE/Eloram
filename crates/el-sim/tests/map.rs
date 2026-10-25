//! The Causeway's grids: the floor where it should be, the two halves the same, sight that stops
//! at rock and brush that hides.

// Tests may allocate: the bans are for the tick.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]

use std::sync::OnceLock;

use el_sim::map::{self, CELL, DAWN, DUSK, MID_Y, Map, NAV_H, NAV_W, VIS_H, VIS_WORDS};
use glam::Vec2;

fn shared() -> &'static Map {
    static M: OnceLock<&'static Map> = OnceLock::new();
    M.get_or_init(Map::leaked)
}

#[test]
fn every_place_a_unit_starts_is_walkable_and_the_nexus_is_not() {
    let m = shared();
    for team in [DAWN, DUSK] {
        for k in 0..5 {
            assert!(m.walkable(map::spawn_point(team, k)), "spawn {team} {k}");
        }
        assert!(m.walkable(map::wave_start(team)), "wave start {team}");
        assert!(!m.walkable(map::for_team(team, map::NEXUS.0)));
        assert!(!m.walkable(map::for_team(team, map::OUTER_TURRET.0)));
        for r in map::RELICS {
            assert!(m.walkable(map::for_team(team, r)), "relic {r}");
        }
    }
    assert!(m.walkable(Vec2::new(map::MAP_W_HALF, MID_Y)));
    assert!(!m.walkable(Vec2::new(7_680.0, 100.0)), "the chasm");
}

#[test]
fn the_halves_mirror_each_other_cell_for_cell() {
    let m = shared();
    for y in 0..NAV_H {
        for x in 0..NAV_W {
            assert_eq!(m.walkable_cell(x, y), m.walkable_cell(NAV_W - 1 - x, NAV_H - 1 - y), "({x}, {y})");
        }
    }
}

#[test]
fn a_straight_walk_down_the_lane_is_clear_and_one_through_rock_is_not() {
    let m = shared();
    // From in front of Dawn's outer turret to the middle, and its mirror.
    let (a, b) = (Vec2::new(5_600.0, MID_Y), Vec2::new(map::MAP_W_HALF, MID_Y));
    assert!(m.walk_clear(a, b));
    assert!(m.walk_clear(map::mirror(a), map::mirror(b)));
    // The structures stand in the lane: a line through one is not clear.
    assert!(!m.walk_clear(map::wave_start(DAWN), map::wave_start(DUSK)));
    let rock = map::ROCKS[3].0;
    assert!(!m.walk_clear(rock - Vec2::new(500.0, 0.0), rock + Vec2::new(500.0, 0.0)));
}

#[test]
fn sight_stops_at_rock_and_brush_hides_what_is_in_it() {
    let m = shared();
    let mut grid = [0u64; VIS_WORDS * VIS_H];
    let eye = Vec2::new(5_000.0, MID_Y);
    m.stamp_sight(eye, map::SIGHT_CHAMPION, &mut grid);
    assert!(Map::lit(&grid, eye));
    assert!(Map::lit(&grid, eye + Vec2::new(900.0, 0.0)));
    assert!(!Map::lit(&grid, eye + Vec2::new(1_500.0, 0.0)), "past sight");
    let brush = map::BRUSHES[0];
    let inside = (brush.0 + brush.1) * 0.5;
    let mut seen = grid;
    let b = (m.brush_at(inside) - 1) as usize;
    m.hide_brush(b, &mut seen);
    assert!(!Map::lit(&seen, inside), "a brush nobody stands in is dark");
}

#[test]
#[ignore = "prints the map: cargo test -p el-sim --test map -- --ignored --nocapture"]
fn print_the_map() {
    let m = shared();
    for y in (0..NAV_H).rev().step_by(3) {
        let row: String = (0..NAV_W)
            .step_by(3)
            .map(|x| {
                let p = Map::cell_centre(x, y);
                if m.brush_at(p) > 0 {
                    '%'
                } else if m.walkable_cell(x, y) {
                    '.'
                } else {
                    '#'
                }
            })
            .collect();
        println!("{row}");
    }
    println!("{} x {} cells of {CELL}", NAV_W, NAV_H);
}

#[test]
fn paths_go_round_rock_with_few_corners_and_every_leg_is_walkable() {
    use el_sim::path::{self, Path, PathScratch};
    let m = shared();
    let mut s = PathScratch::new();
    let mut p = Path::default();
    let mut rng = el_sim::math::Rng::new(9);
    let mut found = 0;
    for _ in 0..300 {
        let pick = |rng: &mut el_sim::math::Rng| loop {
            let q = Vec2::new(rng.next_f32() * map::MAP_W_FULL, rng.next_f32() * map::MAP_H_FULL);
            if m.walkable(q) {
                return q;
            }
        };
        let (a, b) = (pick(&mut rng), pick(&mut rng));
        if !path::find(m, &mut s, a, b, &mut p) {
            continue;
        }
        found += 1;
        let mut at = a;
        for k in 0..usize::from(p.len) {
            let c = p.points[k];
            assert!(m.walk_clear(at, c) || k == 0 && !m.walkable(a), "leg {k} of {a} -> {b}: {at} -> {c}");
            at = c;
        }
        assert!((p.goal().unwrap() - b).length() < 1.0, "{a} -> {b} ends at {:?}", p.goal());
    }
    assert!(found > 290, "{found} of 300 found");
    assert!(s.expanded / s.searches < 4_000, "{} cells a search", s.expanded / s.searches);
}

#[test]
fn every_walkable_cell_is_reachable_from_the_lane() {
    let m = shared();
    let mut seen = vec![false; NAV_W * NAV_H];
    let (sx, sy) = Map::cell(Vec2::new(map::MAP_W_HALF, MID_Y));
    let mut stack = vec![(sx, sy)];
    seen[sy * NAV_W + sx] = true;
    while let Some((x, y)) = stack.pop() {
        for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if nx < 0 || ny < 0 || nx >= NAV_W as i32 || ny >= NAV_H as i32 {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            if m.walkable_cell(nx, ny) && !seen[ny * NAV_W + nx] {
                seen[ny * NAV_W + nx] = true;
                stack.push((nx, ny));
            }
        }
    }
    let mut stranded = Vec::new();
    for y in 0..NAV_H {
        for x in 0..NAV_W {
            if m.walkable_cell(x, y) && !seen[y * NAV_W + x] {
                stranded.push(Map::cell_centre(x, y));
            }
        }
    }
    assert!(
        stranded.is_empty(),
        "{} cells cut off, e.g. {:?}",
        stranded.len(),
        &stranded[..stranded.len().min(5)]
    );
}
