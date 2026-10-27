//! Paths across the navigation grid: the straight line when it's clear (most of a lane), else A*
//! over the 8-connected grid with an octile estimate, then pulled tight so the path is a few
//! corners, not a staircase. All of it in a [`PathScratch`] built once and reused: a query
//! allocates nothing, and leaves nothing behind that changes the next one.

use alloc::boxed::Box;
use glam::Vec2;

use crate::map::{Map, NAV_H, NAV_W};
use crate::storage::boxed;

/// Corners a path keeps (the last is the goal).
pub const MAX_WAYPOINTS: usize = 16;
/// Cells A* may expand before it settles for the cell nearest the goal it found.
pub const MAX_EXPANSIONS: usize = 12_000;
/// How far from an unwalkable goal A* looks for a walkable one, in cells.
const SNAP_CELLS: usize = 24;

/// A unit's route: the corners still to reach, nearest first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Path {
    pub points: [Vec2; MAX_WAYPOINTS],
    pub len: u8,
    /// The next corner to reach.
    pub next: u8,
}

impl Default for Path {
    fn default() -> Self {
        Self { points: [Vec2::ZERO; MAX_WAYPOINTS], len: 0, next: 0 }
    }
}

impl Path {
    pub fn is_empty(&self) -> bool {
        self.next >= self.len
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.next = 0;
    }

    /// The corner being walked to.
    pub fn corner(&self) -> Option<Vec2> {
        (!self.is_empty()).then(|| self.points[usize::from(self.next)])
    }

    /// The point the path ends at.
    pub fn goal(&self) -> Option<Vec2> {
        (self.len > 0).then(|| self.points[usize::from(self.len - 1)])
    }

    fn push(&mut self, p: Vec2) {
        if usize::from(self.len) < MAX_WAYPOINTS {
            self.points[usize::from(self.len)] = p;
            self.len += 1;
        } else {
            // Out of room: the last corner becomes the goal itself.
            self.points[MAX_WAYPOINTS - 1] = p;
        }
    }
}

const STRAIGHT: u32 = 10;
const DIAGONAL: u32 = 14;
const NONE: u32 = u32::MAX;

/// What a search works in: per-cell costs and parents stamped with the query they belong to, and
/// an open list. One a thread: a shard steps its matches one after another with the same one.
pub struct PathScratch {
    g: Box<[u32]>,
    parent: Box<[u32]>,
    stamp: Box<[u16]>,
    closed: Box<[u16]>,
    heap: Box<[(u32, u32, u32)]>,
    heap_len: usize,
    query: u16,
    trail: Box<[u32]>,
    /// Searches run, and cells expanded, since it was built (for the bench).
    pub searches: u64,
    pub expanded: u64,
    /// The last search: from, to, cells expanded (for finding a slow one).
    pub last: (Vec2, Vec2, u32),
}

impl Default for PathScratch {
    fn default() -> Self {
        Self::new()
    }
}

impl PathScratch {
    /// A scratch that owns nothing, to stand in while the real one is lent out: searching with it
    /// panics.
    pub fn empty() -> Self {
        Self {
            g: boxed(0, 0u32),
            parent: boxed(0, NONE),
            stamp: boxed(0, 0u16),
            closed: boxed(0, 0u16),
            heap: boxed(0, (0u32, 0u32, 0u32)),
            heap_len: 0,
            query: 0,
            trail: boxed(0, 0u32),
            searches: 0,
            expanded: 0,
            last: (Vec2::ZERO, Vec2::ZERO, 0),
        }
    }

    pub fn new() -> Self {
        let n = NAV_W * NAV_H;
        Self {
            g: boxed(n, 0u32),
            parent: boxed(n, NONE),
            stamp: boxed(n, 0u16),
            closed: boxed(n, 0u16),
            heap: boxed(MAX_EXPANSIONS * 8 + 8, (0u32, 0u32, 0u32)),
            heap_len: 0,
            query: 0,
            trail: boxed(n.min(4096), 0u32),
            searches: 0,
            expanded: 0,
            last: (Vec2::ZERO, Vec2::ZERO, 0),
        }
    }

    fn begin(&mut self) {
        self.query = self.query.wrapping_add(1);
        if self.query == 0 {
            // The stamps went round: forget them all, once every 65,535 queries.
            self.stamp.fill(0);
            self.closed.fill(0);
            self.query = 1;
        }
        self.heap_len = 0;
    }

    /// Pushes a cell by its estimate `f`; among equal estimates the one nearer the goal (`h`)
    /// comes first, so the search runs along the path instead of flooding the band of ties.
    fn push(&mut self, f: u32, h: u32, cell: u32) {
        if self.heap_len == self.heap.len() {
            return;
        }
        let mut i = self.heap_len;
        self.heap[i] = (f, h, cell);
        self.heap_len += 1;
        while i > 0 {
            let up = (i - 1) / 2;
            if self.heap[up] <= self.heap[i] {
                break;
            }
            self.heap.swap(up, i);
            i = up;
        }
    }

    fn pop(&mut self) -> Option<(u32, u32, u32)> {
        if self.heap_len == 0 {
            return None;
        }
        let top = self.heap[0];
        self.heap_len -= 1;
        self.heap[0] = self.heap[self.heap_len];
        let mut i = 0;
        loop {
            let (l, r) = (2 * i + 1, 2 * i + 2);
            let mut m = i;
            if l < self.heap_len && self.heap[l] < self.heap[m] {
                m = l;
            }
            if r < self.heap_len && self.heap[r] < self.heap[m] {
                m = r;
            }
            if m == i {
                break;
            }
            self.heap.swap(m, i);
            i = m;
        }
        Some(top)
    }
}

fn octile(ax: usize, ay: usize, bx: usize, by: usize) -> u32 {
    let dx = ax.abs_diff(bx) as u32;
    let dy = ay.abs_diff(by) as u32;
    let (lo, hi) = if dx < dy { (dx, dy) } else { (dy, dx) };
    DIAGONAL * lo + STRAIGHT * (hi - lo)
}

/// Finds a path from `from` to `to` into `out`. The goal moves to the nearest walkable place if
/// it's in rock; if A* runs out of expansions it goes as near as it got. Returns whether a path
/// (possibly to a nearer goal) was found.
pub fn find(map: &Map, s: &mut PathScratch, from: Vec2, to: Vec2, out: &mut Path) -> bool {
    out.clear();
    s.searches += 1;
    let goal = map.snap(to);
    if map.walk_clear(from, goal) {
        out.push(goal);
        return true;
    }
    let (sx, sy) = Map::cell(from);
    let Some((sx, sy)) = map.nearest_walkable(sx, sy, SNAP_CELLS) else { return false };
    let (gx, gy) = Map::cell(goal);
    let Some((gx, gy)) = map.nearest_walkable(gx, gy, SNAP_CELLS) else { return false };
    s.begin();
    let q = s.query;
    let start = (sy * NAV_W + sx) as u32;
    let target = (gy * NAV_W + gx) as u32;
    let lm = map.landmark_dists(target as usize);
    let estimate = |x: usize, y: usize| octile(x, y, gx, gy).max(map.alt(y * NAV_W + x, &lm));
    s.g[start as usize] = 0;
    s.parent[start as usize] = NONE;
    s.stamp[start as usize] = q;
    let h0 = estimate(sx, sy);
    s.push(h0, h0, start);
    let mut best = (h0, start);
    let mut expansions = 0;
    let mut reached = false;
    while let Some((_, _, cell)) = s.pop() {
        let ci = cell as usize;
        if s.closed[ci] == q {
            continue;
        }
        s.closed[ci] = q;
        if cell == target {
            reached = true;
            break;
        }
        expansions += 1;
        if expansions > MAX_EXPANSIONS {
            break;
        }
        let (cx, cy) = (ci % NAV_W, ci / NAV_W);
        let h = estimate(cx, cy);
        if (h, cell) < best {
            best = (h, cell);
        }
        for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let (nx, ny) = (cx as i32 + dx, cy as i32 + dy);
            if nx < 0 || ny < 0 || nx >= NAV_W as i32 || ny >= NAV_H as i32 {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            if !map.walkable_cell(nx, ny) {
                continue;
            }
            let diagonal = dx != 0 && dy != 0;
            // No squeezing between two blocked cells.
            if diagonal && (!map.walkable_cell(nx, cy) || !map.walkable_cell(cx, ny)) {
                continue;
            }
            let ni = ny * NAV_W + nx;
            if s.closed[ni] == q {
                continue;
            }
            let g = s.g[ci] + if diagonal { DIAGONAL } else { STRAIGHT };
            if s.stamp[ni] != q || g < s.g[ni] {
                s.stamp[ni] = q;
                s.g[ni] = g;
                s.parent[ni] = cell;
                let h = estimate(nx, ny);
                s.push(g + h, h, ni as u32);
            }
        }
    }
    s.expanded += expansions as u64;
    s.last = (from, to, expansions as u32);
    let end = if reached { target } else { best.1 };
    // Walk the parents back to the start, then pull the route tight from the start's end.
    let mut n = 0;
    let mut c = end;
    while c != NONE && n < s.trail.len() {
        s.trail[n] = c;
        n += 1;
        c = s.parent[c as usize];
    }
    let centre = |cell: u32| Map::cell_centre(cell as usize % NAV_W, cell as usize / NAV_W);
    let last = if reached { goal } else { centre(end) };
    // trail[n-1] is the start, trail[0] the end. From where the unit is, the farthest cell of the
    // route it can walk to straight: gallop out along the route until one can't be seen, then
    // bisect between the last seen and that. A few dozen line checks a path, not one a cell.
    let point = |k: usize| if k == 0 { last } else { centre(s.trail[k]) };
    let mut at = from;
    let mut i = n - 1;
    while i > 0 {
        let mut seen = i;
        let mut step = 1;
        let mut blocked = None;
        loop {
            let j = i.saturating_sub(step);
            if map.walk_clear(at, point(j)) {
                seen = j;
                if j == 0 {
                    break;
                }
                step *= 2;
            } else {
                blocked = Some(j);
                break;
            }
        }
        if let Some(mut hi) = blocked {
            // seen > hi: the farthest seen is between them.
            while seen - hi > 1 {
                let mid = (seen + hi) / 2;
                if map.walk_clear(at, point(mid)) {
                    seen = mid;
                } else {
                    hi = mid;
                }
            }
        }
        if seen == i {
            // Not even the next cell in a straight line (the unit stands off the grid's route):
            // take the next cell as it is.
            seen = i - 1;
        }
        if seen == 0 {
            break;
        }
        out.push(point(seen));
        at = point(seen);
        i = seen;
    }
    out.push(last);
    out.len > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heap_pops_in_order_and_breaks_ties_toward_the_goal() {
        let mut s = PathScratch::new();
        s.begin();
        for (f, h, c) in [(5, 0, 9), (3, 2, 4), (5, 0, 2), (1, 9, 7), (3, 1, 1)] {
            s.push(f, h, c);
        }
        let mut out = [(0, 0, 0); 5];
        for o in &mut out {
            *o = s.pop().unwrap();
        }
        assert_eq!(out, [(1, 9, 7), (3, 1, 1), (3, 2, 4), (5, 0, 2), (5, 0, 9)]);
        assert_eq!(s.pop(), None);
    }
}
