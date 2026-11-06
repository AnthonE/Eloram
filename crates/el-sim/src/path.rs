//! Paths across the navigation mesh ([`crate::nav`]): the straight line when it's clear (most of
//! a lane), else A* over the mesh's rectangles (each reached at the point of its portal nearest
//! where the last was entered, the straight-line distance to the goal its estimate), then the
//! funnel through their portals, so the path is the few corners a wall makes it turn. A goal off
//! the floor, however far, moves to the nearest place on it. All of it in a [`PathScratch`] built
//! once and reused: a query allocates nothing, and leaves nothing behind that changes the next.

use alloc::boxed::Box;
use glam::Vec2;

use crate::map::Map;
use crate::math;
use crate::nav::{MAX_LINKS, MAX_RECTS, NONE};
use crate::storage::boxed;

/// Corners a path keeps (the last is the goal).
pub const MAX_WAYPOINTS: usize = 16;

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

    /// Whether it's full (and so may stop short of where it was found to).
    pub fn is_full(&self) -> bool {
        usize::from(self.len) >= MAX_WAYPOINTS
    }

    fn push(&mut self, p: Vec2) {
        if usize::from(self.len) < MAX_WAYPOINTS {
            self.points[usize::from(self.len)] = p;
            self.len += 1;
        }
        // Out of room: the path stops at its last corner, and whoever walks it finds the rest of
        // the way from there (a corner dropped for the goal would cut through a wall).
    }
}

/// What a search works in: per-rectangle costs, parents and entry points stamped with the query
/// they belong to, an open list, and the corridor's portals. One a thread: a shard steps its
/// matches one after another with the same one.
pub struct PathScratch {
    g: Box<[f32]>,
    parent: Box<[u16]>,
    entry: Box<[Vec2]>,
    stamp: Box<[u16]>,
    closed: Box<[u16]>,
    /// (estimate's bits, distance-to-goal's bits, rectangle): non-negative floats order as their bits.
    heap: Box<[(u32, u32, u16)]>,
    heap_len: usize,
    query: u16,
    trail: Box<[u16]>,
    portals: Box<[(Vec2, Vec2)]>,
    /// Searches run, and rectangles expanded, since it was built (for the bench).
    pub searches: u64,
    pub expanded: u64,
    /// The last search: from, to, rectangles expanded (for finding a slow one).
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
        Self::sized(0, 0)
    }

    pub fn new() -> Self {
        Self::sized(MAX_RECTS, MAX_LINKS + 1)
    }

    fn sized(rects: usize, heap: usize) -> Self {
        Self {
            g: boxed(rects, 0.0f32),
            parent: boxed(rects, NONE),
            entry: boxed(rects, Vec2::ZERO),
            stamp: boxed(rects, 0u16),
            closed: boxed(rects, 0u16),
            heap: boxed(heap, (0u32, 0u32, 0u16)),
            heap_len: 0,
            query: 0,
            trail: boxed(rects, 0u16),
            portals: boxed(rects + 2, (Vec2::ZERO, Vec2::ZERO)),
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

    /// Pushes a rectangle by its estimate `f`; among equal estimates the one nearer the goal
    /// (`h`) comes first, then the lower index.
    fn push(&mut self, f: f32, h: f32, r: u16) {
        if self.heap_len == self.heap.len() {
            return;
        }
        let mut i = self.heap_len;
        self.heap[i] = (f.to_bits(), h.to_bits(), r);
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

    fn pop(&mut self) -> Option<(u32, u32, u16)> {
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

/// Finds a path from `from` to `to` into `out`. The goal moves to the nearest place on the floor
/// if it's off it, and so does the start (the path's first corner, then). Returns whether a path
/// (possibly to a nearer goal) was found.
pub fn find(map: &Map, s: &mut PathScratch, from: Vec2, to: Vec2, out: &mut Path) -> bool {
    out.clear();
    s.searches += 1;
    let nav = &map.nav;
    let (goal, target) = nav.nearest(to);
    if map.walk_clear(from, goal) {
        out.push(goal);
        return true;
    }
    let (start_at, start) = nav.nearest(from);
    if start_at != from {
        // Off the floor (pushed, or the grid's edge): back onto it first.
        out.push(start_at);
    }
    s.begin();
    let q = s.query;
    let si = usize::from(start);
    s.g[si] = 0.0;
    s.parent[si] = NONE;
    s.entry[si] = start_at;
    s.stamp[si] = q;
    let h0 = math::distance(start_at, goal);
    s.push(h0, h0, start);
    let mut best = (h0, start);
    let mut expansions = 0u32;
    let mut reached = false;
    while let Some((_, _, r)) = s.pop() {
        let ri = usize::from(r);
        if s.closed[ri] == q {
            continue;
        }
        s.closed[ri] = q;
        if r == target {
            reached = true;
            break;
        }
        expansions += 1;
        let at = s.entry[ri];
        let h = math::distance(at, goal);
        if h < best.0 {
            best = (h, r);
        }
        for l in nav.links(r) {
            let ni = usize::from(l.to);
            if s.closed[ni] == q {
                continue;
            }
            // Entered at the point of the portal nearest the straight line on to the goal's side:
            // the nearest to where this one was entered, which is a real walk (a rectangle is
            // convex), so `g` never undercounts.
            let e = l.nearest(at);
            let g = s.g[ri] + math::distance(at, e);
            if s.stamp[ni] != q || g < s.g[ni] {
                s.stamp[ni] = q;
                s.g[ni] = g;
                s.parent[ni] = r;
                s.entry[ni] = e;
                let h = math::distance(e, goal);
                s.push(g + h, h, l.to);
            }
        }
    }
    s.expanded += u64::from(expansions);
    s.last = (from, to, expansions);
    let (end, last) = if reached {
        (target, goal)
    } else {
        // Cut off (the mesh is one piece, so never on this map): as near as it got.
        (best.1, nav.rect(best.1).clamp(goal))
    };
    // The corridor, start to end, then its portals, then the funnel through them.
    let mut n = 0;
    let mut c = end;
    while c != NONE && n < s.trail.len() {
        s.trail[n] = c;
        n += 1;
        c = s.parent[usize::from(c)];
    }
    s.trail[..n].reverse();
    s.portals[0] = (start_at, start_at);
    let mut k = 1;
    for w in 0..n.saturating_sub(1) {
        let Some(l) = nav.link(s.trail[w], s.trail[w + 1]) else { break };
        s.portals[k] = (l.left, l.right);
        k += 1;
    }
    s.portals[k] = (last, last);
    crate::nav::funnel(&s.portals[..=k], |c| {
        out.push(if c == last { c } else { nav.turn_point(c) });
        true
    });
    out.len > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heap_pops_in_order_and_breaks_ties_toward_the_goal() {
        let mut s = PathScratch::new();
        s.begin();
        for (f, h, c) in [(5.0, 0.0, 9), (3.0, 2.0, 4), (5.0, 0.0, 2), (1.0, 9.0, 7), (3.0, 1.0, 1)] {
            s.push(f, h, c);
        }
        let mut out = [0u16; 5];
        for o in &mut out {
            *o = s.pop().unwrap().2;
        }
        assert_eq!(out, [7, 1, 4, 2, 9]);
        assert_eq!(s.pop(), None);
    }
}
