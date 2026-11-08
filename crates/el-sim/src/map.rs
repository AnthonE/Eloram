//! The Causeway: one lane over a chasm (`docs/DESIGN.md`, "The map").
//!
//! The map is shapes (the lane, the bases, the platforms, pillars and brush), defined for Dawn's
//! half and mirrored through the map's centre for Dusk's, so the two sides are the same to the
//! unit. From the shapes, once a process: the navigation grid (32-unit cells, a
//! unit's centre may stand on a cell whose centre is on the floor with [`CLEARANCE`] to spare) and
//! its cells merged into a navigation mesh ([`crate::nav`]: what paths are found on and minions
//! march by), the vision grid (120-unit cells, League's resolution) with each cell's window of what it sees at
//! each sight radius, and the brushes' masks ([`Map::leaked`]). Nothing here changes during a match.

use alloc::boxed::Box;
use glam::Vec2;

use crate::nav::NavMesh;
use crate::storage::{boxed, leak};
use crate::{MAP_H, MAP_W};

/// Navigation cells, units a side.
pub const CELL: f32 = 32.0;
pub const NAV_W: usize = (MAP_W / CELL) as usize;
pub const NAV_H: usize = (MAP_H / CELL) as usize;
/// How far a unit's centre keeps from a wall: a minion's radius, so a champion's shoulders brush it.
pub const CLEARANCE: f32 = 50.0;

/// Vision cells, units a side.
pub const VIS_CELL: f32 = 120.0;
pub const VIS_W: usize = (MAP_W / VIS_CELL) as usize;
pub const VIS_H: usize = (MAP_H / VIS_CELL) as usize;
/// Words in a vision grid's row (128 cells).
pub const VIS_WORDS: usize = VIS_W.div_ceil(64);

/// The two teams. Dawn's base is at small x, Dusk's at large x.
pub const DAWN: u8 = 0;
pub const DUSK: u8 = 1;

/// The lane's middle, across the map.
pub const MID_Y: f32 = MAP_H * 0.5;
/// The map's middle, along it.
pub const MAP_W_HALF: f32 = MAP_W * 0.5;
/// The map's extent (re-exported for whoever has this module in hand).
pub const MAP_W_FULL: f32 = MAP_W;
pub const MAP_H_FULL: f32 = MAP_H;

/// Sight radii a unit may have: a champion's, a minion's, a structure's.
pub const SIGHT: [f32; 3] = [1_200.0, 1_000.0, 1_100.0];
pub const SIGHT_CHAMPION: usize = 0;
pub const SIGHT_MINION: usize = 1;
pub const SIGHT_STRUCTURE: usize = 2;
/// The widest sight, in vision cells.
pub const SIGHT_CELLS: usize = 10;
/// A sight window's side, in vision cells.
pub const WINDOW: usize = 2 * SIGHT_CELLS + 1;

/// Brushes, both sides.
pub const MAX_BRUSHES: usize = 8;

/// An axis-aligned rectangle: min corner, max corner.
#[derive(Clone, Copy, Debug)]
pub struct Rect(pub Vec2, pub Vec2);

impl Rect {
    pub const fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Rect(Vec2::new(x0, y0), Vec2::new(x1, y1))
    }
    pub fn contains(&self, p: Vec2, inset: f32) -> bool {
        p.x >= self.0.x + inset
            && p.x <= self.1.x - inset
            && p.y >= self.0.y + inset
            && p.y <= self.1.y - inset
    }
    /// The same rectangle on Dusk's half.
    pub const fn mirrored(&self) -> Rect {
        Rect::new(MAP_W - self.1.x, MAP_H - self.1.y, MAP_W - self.0.x, MAP_H - self.0.y)
    }
}

/// A disc: centre, radius.
#[derive(Clone, Copy, Debug)]
pub struct Disc(pub Vec2, pub f32);

impl Disc {
    pub const fn new(x: f32, y: f32, r: f32) -> Self {
        Disc(Vec2::new(x, y), r)
    }
    pub fn contains(&self, p: Vec2, grow: f32) -> bool {
        let r = self.1 + grow;
        (p - self.0).length_squared() <= r * r
    }
    pub const fn mirrored(&self) -> Disc {
        Disc::new(MAP_W - self.0.x, MAP_H - self.0.y, self.1)
    }
}

/// `p` on the other team's half.
pub fn mirror(p: Vec2) -> Vec2 {
    Vec2::new(MAP_W - p.x, MAP_H - p.y)
}

/// A point of Dawn's, or its mirror for Dusk.
pub fn for_team(team: u8, dawn: Vec2) -> Vec2 {
    if team == DAWN { dawn } else { mirror(dawn) }
}

// ---------------------------------------------------------------------------------------------
// Dawn's half (Dusk's is the mirror)
// ---------------------------------------------------------------------------------------------

/// The lane: one rectangle from base to base, 1,700 wide.
pub const LANE: Rect = Rect::new(1_350.0, MID_Y - 850.0, MAP_W - 1_350.0, MID_Y + 850.0);
/// The base round the nexus, and the platform behind it.
pub const BASE: Disc = Disc::new(2_050.0, MID_Y, 1_250.0);
pub const PLATFORM: Disc = Disc::new(700.0, MID_Y, 620.0);
/// The neck from the base to the platform.
pub const NECK: Rect = Rect::new(600.0, MID_Y - 380.0, 1_500.0, MID_Y + 380.0);

/// Rock in the lane: pillars, and the walls' jagged edges.
pub const ROCKS: [Disc; 7] = [
    Disc::new(4_350.0, MID_Y - 720.0, 260.0),
    Disc::new(4_900.0, MID_Y + 760.0, 300.0),
    Disc::new(6_150.0, MID_Y + 330.0, 120.0),
    Disc::new(6_700.0, MID_Y - 800.0, 340.0),
    Disc::new(7_350.0, MID_Y + 820.0, 280.0),
    Disc::new(3_000.0, MID_Y + 1_000.0, 420.0),
    Disc::new(3_000.0, MID_Y - 1_000.0, 420.0),
];

/// Brush, against the lane's edges.
pub const BRUSHES: [Rect; 3] = [
    Rect::new(5_550.0, MID_Y - 830.0, 6_150.0, MID_Y - 520.0),
    Rect::new(6_200.0, MID_Y + 500.0, 6_800.0, MID_Y + 830.0),
    Rect::new(7_450.0, MID_Y - 830.0, 7_910.0, MID_Y - 480.0),
];

/// Where the structures stand (each with its footprint, which nothing walks through).
pub const OUTER_TURRET: Disc = Disc::new(5_250.0, MID_Y - 120.0, 110.0);
pub const INHIB_TURRET: Disc = Disc::new(3_750.0, MID_Y + 120.0, 110.0);
pub const INHIBITOR: Disc = Disc::new(3_200.0, MID_Y, 150.0);
pub const NEXUS_TURRETS: [Disc; 2] =
    [Disc::new(2_620.0, MID_Y - 260.0, 100.0), Disc::new(2_620.0, MID_Y + 260.0, 100.0)];
pub const NEXUS: Disc = Disc::new(2_050.0, MID_Y, 260.0);
/// The fountain's laser stands at the platform's back.
pub const FOUNTAIN: Vec2 = Vec2::new(520.0, MID_Y);

/// Health relics: the outer pad and the inner pad.
pub const RELICS: [Vec2; 2] = [Vec2::new(6_500.0, MID_Y + 300.0), Vec2::new(4_550.0, MID_Y - 380.0)];

/// Where Dawn's champions stand at the start, and where they come back: in a ring on the platform.
pub fn spawn_point(team: u8, k: usize) -> Vec2 {
    let a = k as f32 * (core::f32::consts::TAU / 5.0);
    for_team(team, PLATFORM.0 + Vec2::new(crate::math::cos(a), crate::math::sin(a)) * 220.0)
}

/// Where Dawn's minions leave from, and the point they walk to (the enemy nexus).
pub fn wave_start(team: u8) -> Vec2 {
    for_team(team, Vec2::new(2_550.0, MID_Y))
}

// ---------------------------------------------------------------------------------------------
// The grids
// ---------------------------------------------------------------------------------------------

/// The map's grids: built once a process, shared by every match and by the browser.
pub struct Map {
    /// Navigation: bit set where a unit's centre may stand. Rows of [`NAV_W`] cells.
    walk: Box<[u64]>,
    /// Navigation cells' brush (0: none, else the brush's index + 1).
    brush_of: Box<[u8]>,
    /// Vision cells that block sight (rock, and the chasm beyond the lane).
    vis_wall: Box<[bool]>,
    /// Vision cells' brush (0: none, else its index + 1).
    vis_brush: Box<[u8]>,
    /// For each vision cell and sight radius, which cells of the window round it it sees: [`WINDOW`]
    /// rows of [`WINDOW`] bits.
    windows: Box<[u32]>,
    /// For each brush, its cells in a vision grid.
    brush_masks: Box<[[u64; VIS_WORDS * VIS_H]]>,
    brushes: usize,
    /// The walkable cells as a navigation mesh: what paths are found on, and minions march by.
    pub nav: NavMesh,
}

/// Where a team's minions march to: in front of the enemy's nexus.
pub fn march_goal(team: u8) -> Vec2 {
    for_team(1 - team, Vec2::new(NEXUS.0.x + 450.0, MID_Y))
}

/// Is `p` on the floor (the lane, the bases, the platforms), `margin` inside its edge?
pub fn on_floor(p: Vec2, margin: f32) -> bool {
    let floor_rects = [LANE, NECK, NECK.mirrored()];
    let floor_discs = [BASE, BASE.mirrored(), PLATFORM, PLATFORM.mirrored()];
    floor_rects.iter().any(|r| r.contains(p, margin)) || floor_discs.iter().any(|d| d.contains(p, -margin))
}

/// Is `p` within `grow` of rock or a structure's footprint?
fn on_rock(p: Vec2, grow: f32) -> bool {
    let dawn =
        ROCKS.iter().chain([&OUTER_TURRET, &INHIB_TURRET, &INHIBITOR, &NEXUS]).chain(NEXUS_TURRETS.iter());
    dawn.into_iter().any(|d| d.contains(p, grow) || d.mirrored().contains(p, grow))
}

/// Every brush on the map: Dawn's, then their mirrors.
pub fn brushes() -> [Rect; 2 * BRUSHES.len()] {
    let mut out = [BRUSHES[0]; 2 * BRUSHES.len()];
    for (i, b) in BRUSHES.iter().enumerate() {
        out[i] = *b;
        out[BRUSHES.len() + i] = b.mirrored();
    }
    out
}

impl Map {
    /// Builds the grids (tens of milliseconds: once a process).
    pub fn build() -> Map {
        let mut walk = boxed(NAV_W * NAV_H / 64, 0u64);
        let mut brush_of = boxed(NAV_W * NAV_H, 0u8);
        let brushes = brushes();
        for y in 0..NAV_H {
            for x in 0..NAV_W {
                let p = Vec2::new((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL);
                let i = y * NAV_W + x;
                if on_floor(p, CLEARANCE) && !on_rock(p, CLEARANCE) {
                    walk[i / 64] |= 1 << (i % 64);
                }
                if let Some(b) = brushes.iter().position(|r| r.contains(p, 0.0)) {
                    brush_of[i] = b as u8 + 1;
                }
            }
        }
        let mut vis_wall = boxed(VIS_W * VIS_H, false);
        let mut vis_brush = boxed(VIS_W * VIS_H, 0u8);
        let mut brush_masks = boxed(brushes.len(), [0u64; VIS_WORDS * VIS_H]);
        for y in 0..VIS_H {
            for x in 0..VIS_W {
                let p = Vec2::new((x as f32 + 0.5) * VIS_CELL, (y as f32 + 0.5) * VIS_CELL);
                let i = y * VIS_W + x;
                vis_wall[i] = !on_floor(p, 0.0) || on_rock(p, -40.0);
                if let Some(b) = brushes.iter().position(|r| r.contains(p, 0.0)) {
                    vis_brush[i] = b as u8 + 1;
                    brush_masks[b][y * VIS_WORDS + x / 64] |= 1 << (x % 64);
                }
            }
        }
        let mut map = Map {
            walk,
            brush_of,
            vis_wall,
            vis_brush,
            windows: boxed(VIS_W * VIS_H * SIGHT.len() * WINDOW, 0u32),
            brush_masks,
            brushes: brushes.len(),
            nav: NavMesh::build(|_, _| false, [Vec2::ZERO; 2]),
        };
        map.build_windows();
        map.nav = NavMesh::build(|x, y| map.walkable_cell(x, y), [march_goal(DAWN), march_goal(DUSK)]);
        map
    }

    /// The point a marching minion of `team` at `p` heads for: the next corner on its way down
    /// the lane to the enemy nexus, in a straight line from `p` (none once it's there).
    pub fn march_toward(&self, team: u8, p: Vec2) -> Option<Vec2> {
        self.nav.march_toward(team, p)
    }

    /// Builds the grids and keeps them for the life of the process: what every match in it
    /// shares. Call it once (the server and the client each hold the one they got).
    pub fn leaked() -> &'static Map {
        leak(Map::build())
    }

    fn build_windows(&mut self) {
        for cy in 0..VIS_H {
            for cx in 0..VIS_W {
                for (k, &radius) in SIGHT.iter().enumerate() {
                    let base = ((cy * VIS_W + cx) * SIGHT.len() + k) * WINDOW;
                    let reach = radius / VIS_CELL;
                    for wy in 0..WINDOW {
                        let mut row = 0u32;
                        for wx in 0..WINDOW {
                            let dx = wx as i32 - SIGHT_CELLS as i32;
                            let dy = wy as i32 - SIGHT_CELLS as i32;
                            if (dx * dx + dy * dy) as f32 > reach * reach {
                                continue;
                            }
                            let (tx, ty) = (cx as i32 + dx, cy as i32 + dy);
                            if tx < 0 || ty < 0 || tx >= VIS_W as i32 || ty >= VIS_H as i32 {
                                continue;
                            }
                            if self.vis_line_clear(cx as i32, cy as i32, tx, ty) {
                                row |= 1 << wx;
                            }
                        }
                        self.windows[base + wy] = row;
                    }
                }
            }
        }
    }

    /// Whether sight runs from one vision cell to another: no wall on the cells between (the ends
    /// excepted: a wall is seen, and a viewer standing against one sees out).
    fn vis_line_clear(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        loop {
            if (x, y) == (x1, y1) {
                return true;
            }
            if (x, y) != (x0, y0) && self.vis_wall[y as usize * VIS_W + x as usize] {
                return false;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    // ---- navigation ------------------------------------------------------------------------

    /// The navigation cell holding `p` (clamped onto the map).
    #[inline]
    pub fn cell(p: Vec2) -> (usize, usize) {
        let x = (p.x / CELL).clamp(0.0, (NAV_W - 1) as f32) as usize;
        let y = (p.y / CELL).clamp(0.0, (NAV_H - 1) as f32) as usize;
        (x, y)
    }

    /// The centre of a navigation cell.
    #[inline]
    pub fn cell_centre(x: usize, y: usize) -> Vec2 {
        Vec2::new((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL)
    }

    #[inline]
    pub fn walkable_cell(&self, x: usize, y: usize) -> bool {
        let i = y * NAV_W + x;
        self.walk[i / 64] & (1 << (i % 64)) != 0
    }

    /// May a unit's centre stand at `p`?
    #[inline]
    pub fn walkable(&self, p: Vec2) -> bool {
        if p.x < 0.0 || p.y < 0.0 || p.x >= MAP_W || p.y >= MAP_H {
            return false;
        }
        let (x, y) = Map::cell(p);
        self.walkable_cell(x, y)
    }

    /// The point nearest `p` where a unit may stand (`p` itself if it may), however far off the
    /// floor `p` is.
    pub fn snap(&self, p: Vec2) -> Vec2 {
        self.nav.nearest(p).0
    }

    /// Whether a unit can walk straight from `a` to `b`: every navigation cell the segment touches
    /// is walkable (a supercover walk, so no corner is cut).
    pub fn walk_clear(&self, a: Vec2, b: Vec2) -> bool {
        let (x, y) = (a.x / CELL, a.y / CELL);
        let (ex, ey) = (b.x / CELL, b.y / CELL);
        let (dx, dy) = (ex - x, ey - y);
        let (mut cx, mut cy) = (crate::math::floor(x) as i32, crate::math::floor(y) as i32);
        let (tx, ty) = (crate::math::floor(ex) as i32, crate::math::floor(ey) as i32);
        let step_x = if dx > 0.0 { 1 } else { -1 };
        let step_y = if dy > 0.0 { 1 } else { -1 };
        // Distance along the segment (as a fraction) to the next vertical and horizontal grid line.
        let inv_dx = if dx != 0.0 { 1.0 / dx.abs() } else { f32::INFINITY };
        let inv_dy = if dy != 0.0 { 1.0 / dy.abs() } else { f32::INFINITY };
        // (Along an axis the segment doesn't move on, the next line is never reached: infinity,
        // never 0 × infinity.)
        let mut t_x = if dx > 0.0 {
            (cx as f32 + 1.0 - x) * inv_dx
        } else if dx < 0.0 {
            (x - cx as f32) * inv_dx
        } else {
            f32::INFINITY
        };
        let mut t_y = if dy > 0.0 {
            (cy as f32 + 1.0 - y) * inv_dy
        } else if dy < 0.0 {
            (y - cy as f32) * inv_dy
        } else {
            f32::INFINITY
        };
        let max_steps = NAV_W + NAV_H;
        for _ in 0..max_steps {
            if cx < 0 || cy < 0 || cx >= NAV_W as i32 || cy >= NAV_H as i32 {
                return false;
            }
            if !self.walkable_cell(cx as usize, cy as usize) {
                return false;
            }
            // At the end's cell, or the next grid line is past the end (an end on a grid line,
            // or a corner, rounds either way: never walk on beyond it).
            if (cx, cy) == (tx, ty) || t_x.min(t_y) > 1.0 {
                return true;
            }
            if t_x < t_y {
                cx += step_x;
                t_x += inv_dx;
            } else if t_y < t_x {
                cy += step_y;
                t_y += inv_dy;
            } else {
                // Through a corner exactly: both neighbours must be walkable too.
                let (nx, ny) = (cx + step_x, cy + step_y);
                if nx < 0 || ny < 0 || nx >= NAV_W as i32 || ny >= NAV_H as i32 {
                    return false;
                }
                if !self.walkable_cell(nx as usize, cy as usize)
                    || !self.walkable_cell(cx as usize, ny as usize)
                {
                    return false;
                }
                cx = nx;
                cy = ny;
                t_x += inv_dx;
                t_y += inv_dy;
            }
        }
        false
    }

    /// The brush `p` stands in (0: none, else the brush's index + 1).
    #[inline]
    pub fn brush_at(&self, p: Vec2) -> u8 {
        let (x, y) = Map::cell(p);
        self.brush_of[y * NAV_W + x]
    }

    pub fn brush_count(&self) -> usize {
        self.brushes
    }

    // ---- vision ----------------------------------------------------------------------------

    /// The vision cell holding `p` (clamped onto the map).
    #[inline]
    pub fn vis_cell(p: Vec2) -> (usize, usize) {
        let x = (p.x / VIS_CELL).clamp(0.0, (VIS_W - 1) as f32) as usize;
        let y = (p.y / VIS_CELL).clamp(0.0, (VIS_H - 1) as f32) as usize;
        (x, y)
    }

    /// ORs what a unit at `p` with sight `class` sees into `grid` ([`VIS_WORDS`] words a row).
    pub fn stamp_sight(&self, p: Vec2, class: usize, grid: &mut [u64; VIS_WORDS * VIS_H]) {
        const { assert!(VIS_WORDS == 2, "a row is two words") };
        let (cx, cy) = Map::vis_cell(p);
        let base = ((cy * VIS_W + cx) * SIGHT.len() + class) * WINDOW;
        // The window's rows that are on the map (row `wy` is the map's row `cy + wy - SIGHT_CELLS`),
        // and its bit `wx` as cell `cx + wx - SIGHT_CELLS` of each: the same shift every row.
        let (y0, y1) = (cy.saturating_sub(SIGHT_CELLS), (cy + SIGHT_CELLS + 1).min(VIS_H));
        let rows = &self.windows[base + y0 + SIGHT_CELLS - cy..base + y1 + SIGHT_CELLS - cy];
        let shift = cx as i32 - SIGHT_CELLS as i32;
        let (lines, _) = grid[y0 * VIS_WORDS..y1 * VIS_WORDS].as_chunks_mut::<2>();
        for (&row, words) in rows.iter().zip(lines) {
            let row = u128::from(row);
            let bits = if shift >= 0 { row << shift } else { row >> -shift };
            words[0] |= bits as u64;
            words[1] |= (bits >> 64) as u64;
        }
    }

    /// Clears brush `b`'s cells out of `grid`: what is in it is hidden from a team with nobody in it.
    pub fn hide_brush(&self, b: usize, grid: &mut [u64; VIS_WORDS * VIS_H]) {
        for (g, m) in grid.iter_mut().zip(self.brush_masks[b].iter()) {
            *g &= !m;
        }
    }

    /// Is the vision cell holding `p` lit in `grid`?
    #[inline]
    pub fn lit(grid: &[u64; VIS_WORDS * VIS_H], p: Vec2) -> bool {
        let (x, y) = Map::vis_cell(p);
        grid[y * VIS_WORDS + x / 64] & (1 << (x % 64)) != 0
    }

    /// Whether the vision cell `(x, y)` blocks sight (for drawing the map's edge).
    pub fn vis_wall(&self, x: usize, y: usize) -> bool {
        self.vis_wall[y * VIS_W + x]
    }

    /// The brush of vision cell `(x, y)` (0: none).
    pub fn vis_brush(&self, x: usize, y: usize) -> u8 {
        self.vis_brush[y * VIS_W + x]
    }
}
