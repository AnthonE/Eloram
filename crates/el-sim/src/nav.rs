//! The navigation mesh: the walkable cells of the map's grid merged into rectangles, each joined to
//! the ones it shares an edge with by a portal (the stretch of edge they share). Rectangles are
//! convex, so a unit crosses one in a straight line; a route is the rectangles A* chooses across
//! the graph (a few hundred nodes, not tens of thousands of cells), pulled tight through their
//! portals by the funnel (the shortest line through them, turning only at a wall's corner).
//!
//! It is the grid's walkable set exactly, cut differently: what [`Map::walkable`] says a unit may
//! stand on is what the mesh covers, so a path's legs never cross a cell the motion refuses.
//! Portals stop [`INSET`] short of their ends, so a corner is turned just clear of the wall, never
//! on the edge between a walkable cell and a wall's.
//!
//! Built once a process with the map (allocating then); searching and steering allocate nothing.

use alloc::boxed::Box;
use glam::Vec2;

use crate::map::{CELL, NAV_H, NAV_W};
use crate::math;
use crate::storage::boxed;

/// Rectangles the mesh may hold (asserted when it's built; a search's scratch is this size).
pub const MAX_RECTS: usize = 2_048;
/// Portals (each way) the mesh may hold.
pub const MAX_LINKS: usize = 8 * MAX_RECTS;
/// No rectangle.
pub const NONE: u16 = u16::MAX;
/// How far short of its ends a portal stops, and how far inside a rectangle's edge a point is
/// put when one is moved onto the mesh: a corner is turned this far off the wall.
pub const INSET: f32 = 4.0;

/// A rectangle of walkable cells: `[x0, x1) × [y0, y1)` in cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NavRect {
    pub x0: u16,
    pub y0: u16,
    pub x1: u16,
    pub y1: u16,
}

impl NavRect {
    /// Its corners in the world, `INSET` in from each edge.
    fn inner(&self) -> (Vec2, Vec2) {
        (
            Vec2::new(f32::from(self.x0) * CELL + INSET, f32::from(self.y0) * CELL + INSET),
            Vec2::new(f32::from(self.x1) * CELL - INSET, f32::from(self.y1) * CELL - INSET),
        )
    }

    /// Its middle, in the world.
    pub fn centre(&self) -> Vec2 {
        Vec2::new(
            (f32::from(self.x0) + f32::from(self.x1)) * 0.5 * CELL,
            (f32::from(self.y0) + f32::from(self.y1)) * 0.5 * CELL,
        )
    }

    /// The point of it (inset) nearest `p`.
    pub fn clamp(&self, p: Vec2) -> Vec2 {
        let (lo, hi) = self.inner();
        Vec2::new(p.x.clamp(lo.x, hi.x), p.y.clamp(lo.y, hi.y))
    }
}

/// A way out of a rectangle: the one it leads `to`, and the stretch of edge between them as seen
/// crossing it, its `left` end and its `right` (inset from both).
#[derive(Clone, Copy, Debug, Default)]
pub struct Link {
    pub to: u16,
    pub left: Vec2,
    pub right: Vec2,
}

impl Link {
    /// The point of the portal nearest `p`.
    pub fn nearest(&self, p: Vec2) -> Vec2 {
        let d = self.right - self.left;
        let len2 = d.length_squared();
        if len2 <= 1e-6 {
            return self.left;
        }
        let t = ((p - self.left).dot(d) / len2).clamp(0.0, 1.0);
        self.left + d * t
    }

    pub fn middle(&self) -> Vec2 {
        (self.left + self.right) * 0.5
    }
}

/// The mesh.
pub struct NavMesh {
    rects: Box<[NavRect]>,
    /// Rectangle `r`'s links are `links[first[r]..first[r + 1]]`.
    first: Box<[u32]>,
    links: Box<[Link]>,
    /// Each navigation cell's rectangle ([`NONE`] for a wall's).
    rect_of: Box<[u16]>,
    /// For each team, each rectangle's next one on the way to where its minions march, and the
    /// march's end ([`NONE`] in the goal's rectangle, and where it can't be reached).
    march_next: [Box<[u16]>; 2],
    march_goal: [Vec2; 2],
}

impl NavMesh {
    /// Cuts the walkable cells (`walkable(x, y)`) into rectangles and joins them. `march_goals`
    /// are where each team's minions walk to.
    #[allow(clippy::disallowed_types, clippy::disallowed_methods)]
    pub fn build(walkable: impl Fn(usize, usize) -> bool, march_goals: [Vec2; 2]) -> NavMesh {
        extern crate alloc;
        use alloc::vec::Vec;
        // Greedy: the first cell not yet taken (row by row) grows right as far as it can, then
        // the whole row grows up as far as it can. The same cells give the same rectangles.
        let mut rect_of = boxed(NAV_W * NAV_H, NONE);
        let mut rects: Vec<NavRect> = Vec::new();
        let free = |rect_of: &[u16], x: usize, y: usize| walkable(x, y) && rect_of[y * NAV_W + x] == NONE;
        for y in 0..NAV_H {
            for x in 0..NAV_W {
                if !free(&rect_of, x, y) {
                    continue;
                }
                let mut x1 = x + 1;
                while x1 < NAV_W && free(&rect_of, x1, y) {
                    x1 += 1;
                }
                let mut y1 = y + 1;
                while y1 < NAV_H && (x..x1).all(|cx| free(&rect_of, cx, y1)) {
                    y1 += 1;
                }
                let id = rects.len() as u16;
                assert!(rects.len() < MAX_RECTS, "the mesh outgrew MAX_RECTS");
                for cy in y..y1 {
                    for cx in x..x1 {
                        rect_of[cy * NAV_W + cx] = id;
                    }
                }
                rects.push(NavRect { x0: x as u16, y0: y as u16, x1: x1 as u16, y1: y1 as u16 });
            }
        }
        // Links: every pair of rectangles sharing an edge of positive length, found along each
        // rectangle's border, in the order of the cells beyond it (so the same every build).
        let mut first = Vec::with_capacity(rects.len() + 1);
        let mut links: Vec<Link> = Vec::new();
        for (r, a) in rects.iter().enumerate() {
            first.push(links.len() as u32);
            let push = |links: &mut Vec<Link>, to: u16| {
                if to == NONE || to as usize == r || links[first[r] as usize..].iter().any(|l| l.to == to) {
                    return;
                }
                let b = rects[usize::from(to)];
                links.push(portal(a, &b, to));
            };
            let (x0, y0, x1, y1) =
                (usize::from(a.x0), usize::from(a.y0), usize::from(a.x1), usize::from(a.y1));
            for x in x0..x1 {
                if y0 > 0 {
                    push(&mut links, rect_of[(y0 - 1) * NAV_W + x]);
                }
                if y1 < NAV_H {
                    push(&mut links, rect_of[y1 * NAV_W + x]);
                }
            }
            for y in y0..y1 {
                if x0 > 0 {
                    push(&mut links, rect_of[y * NAV_W + x0 - 1]);
                }
                if x1 < NAV_W {
                    push(&mut links, rect_of[y * NAV_W + x1]);
                }
            }
        }
        first.push(links.len() as u32);
        assert!(links.len() <= MAX_LINKS, "the mesh outgrew MAX_LINKS");
        let mut mesh = NavMesh {
            rects: boxed(rects.len(), NavRect::default()),
            first: boxed(first.len(), 0u32),
            links: boxed(links.len(), Link::default()),
            rect_of,
            march_next: [boxed(0, NONE), boxed(0, NONE)],
            march_goal: march_goals,
        };
        mesh.rects.copy_from_slice(&rects);
        mesh.first.copy_from_slice(&first);
        mesh.links.copy_from_slice(&links);
        mesh.march_next = march_goals.map(|g| mesh.flow_to(g));
        mesh
    }

    /// Dijkstra over the rectangles from the one holding `goal` (costs between their middles):
    /// each rectangle's next one toward it.
    #[allow(clippy::disallowed_types, clippy::disallowed_methods)]
    fn flow_to(&self, goal: Vec2) -> Box<[u16]> {
        extern crate alloc;
        use alloc::collections::BinaryHeap;
        use core::cmp::Reverse;
        let n = self.rects.len();
        let mut next = boxed(n, NONE);
        let mut dist = boxed(n, f32::INFINITY);
        let Some(g) = self.locate(goal) else { return next };
        dist[usize::from(g)] = 0.0;
        let mut heap = BinaryHeap::new();
        heap.push(Reverse((0u32, g)));
        while let Some(Reverse((d, r))) = heap.pop() {
            let d = f32::from_bits(d);
            if d > dist[usize::from(r)] {
                continue;
            }
            let here = self.rects[usize::from(r)].centre();
            for l in self.links(r) {
                let there = self.rects[usize::from(l.to)].centre();
                let via = l.nearest((here + there) * 0.5);
                let nd = d + math::distance(here, via) + math::distance(via, there);
                if nd < dist[usize::from(l.to)] {
                    dist[usize::from(l.to)] = nd;
                    next[usize::from(l.to)] = r;
                    heap.push(Reverse((nd.to_bits(), l.to)));
                }
            }
        }
        next
    }

    pub fn len(&self) -> usize {
        self.rects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    pub fn rect(&self, r: u16) -> &NavRect {
        &self.rects[usize::from(r)]
    }

    /// Rectangle `r`'s ways out.
    #[inline]
    pub fn links(&self, r: u16) -> &[Link] {
        let r = usize::from(r);
        &self.links[self.first[r] as usize..self.first[r + 1] as usize]
    }

    /// The link from `a` to `b`.
    pub fn link(&self, a: u16, b: u16) -> Option<&Link> {
        self.links(a).iter().find(|l| l.to == b)
    }

    /// The rectangle `p` stands in, if it stands on the mesh.
    #[inline]
    pub fn locate(&self, p: Vec2) -> Option<u16> {
        if !(p.x >= 0.0 && p.y >= 0.0 && p.x < crate::MAP_W && p.y < crate::MAP_H) {
            return None;
        }
        let (x, y) = ((p.x / CELL) as usize, (p.y / CELL) as usize);
        let r = self.rect_of[y.min(NAV_H - 1) * NAV_W + x.min(NAV_W - 1)];
        (r != NONE).then_some(r)
    }

    /// The point of the mesh nearest `p` (`p` itself if it's on it), and its rectangle: however
    /// far off the floor `p` is (a click in the chasm walks to its edge).
    pub fn nearest(&self, p: Vec2) -> (Vec2, u16) {
        if let Some(r) = self.locate(p) {
            return (p, r);
        }
        let mut best = (f32::INFINITY, Vec2::ZERO, 0u16);
        for (r, rect) in self.rects.iter().enumerate() {
            let q = rect.clamp(p);
            let d = (q - p).length_squared();
            if d < best.0 {
                best = (d, q, r as u16);
            }
        }
        (best.1, best.2)
    }

    /// Whether cell `(x, y)` (which may be off the map) is a wall's.
    fn wall(&self, x: i32, y: i32) -> bool {
        x < 0
            || y < 0
            || x >= NAV_W as i32
            || y >= NAV_H as i32
            || self.rect_of[y as usize * NAV_W + x as usize] == NONE
    }

    /// A corner the funnel turned at (a portal's end, [`INSET`] along the portal from the grid
    /// point where it stops), moved [`INSET`] across the portal too, off the walls that meet at
    /// that grid point: so off a wall's corner diagonally, and off a flat face square. A leg walked
    /// to it in a straight line never runs along a wall's edge (which the grid counts as the wall's).
    pub fn turn_point(&self, c: Vec2) -> Vec2 {
        let (gx, gy) = (math::floor(c.x / CELL + 0.5) as i32, math::floor(c.y / CELL + 0.5) as i32);
        let v = Vec2::new(gx as f32, gy as f32) * CELL;
        if (c - v).length_squared() > (INSET + 0.01) * (INSET + 0.01) {
            return c;
        }
        let mut away = (0i32, 0i32);
        for (dx, dy) in [(-1i32, -1i32), (0, -1), (-1, 0), (0, 0)] {
            if self.wall(gx + dx, gy + dy) {
                // The cell's side of the point, pushed back from.
                away.0 -= 2 * dx + 1;
                away.1 -= 2 * dy + 1;
            }
        }
        // Across the portal (the axis the point is on a grid line of) it moves off the walls;
        // along it, it keeps its inset.
        let x = if c.x == v.x { v.x + away.0.signum() as f32 * INSET } else { c.x };
        let y = if c.y == v.y { v.y + away.1.signum() as f32 * INSET } else { c.y };
        Vec2::new(x, y)
    }

    /// Where a minion of `team` at `p` heads next on its march: the first corner of the funnel
    /// through the next few rectangles toward the march's end (in a straight line from `p`, and
    /// clear of every wall). `None` once it's there.
    pub fn march_toward(&self, team: u8, p: Vec2) -> Option<Vec2> {
        const AHEAD: usize = 6;
        let (from, mut r) = self.nearest(p);
        if from != p {
            return Some(from);
        }
        let goal = self.march_goal[usize::from(team)];
        let next = &self.march_next[usize::from(team)];
        let mut portals = [(from, from); AHEAD + 2];
        let mut n = 1;
        let mut end = None;
        while n <= AHEAD {
            let to = next[usize::from(r)];
            if to == NONE {
                // At the goal's rectangle (or cut off from it).
                end = Some(goal);
                break;
            }
            let Some(l) = self.link(r, to) else { break };
            portals[n] = (l.left, l.right);
            n += 1;
            r = to;
        }
        let end = match end {
            Some(g) if n == 1 && math::distance(p, g) <= CELL => return None,
            Some(g) if self.locate(g).is_some() => g,
            Some(_) if n == 1 => return None,
            // Out of lookahead: aim through the middle of the last portal.
            _ => (portals[n - 1].0 + portals[n - 1].1) * 0.5,
        };
        portals[n] = (end, end);
        let mut corner = end;
        funnel(&portals[..=n], |c| {
            let c = if c == end { c } else { self.turn_point(c) };
            // A corner it's already at (moved off the wall, it may stand on one) is passed.
            if math::distance(c, p) <= 1.0 {
                return true;
            }
            corner = c;
            false
        });
        Some(corner)
    }
}

/// The portal from `a` into its neighbour `b` (id `to`), its ends as seen crossing it.
fn portal(a: &NavRect, b: &NavRect, to: u16) -> Link {
    let inset = |lo: f32, hi: f32| {
        if hi - lo > 2.0 * INSET { (lo + INSET, hi - INSET) } else { ((lo + hi) * 0.5, (lo + hi) * 0.5) }
    };
    if a.x1 == b.x0 || b.x1 == a.x0 {
        // Side by side: a vertical edge.
        let x = f32::from(if a.x1 == b.x0 { a.x1 } else { a.x0 }) * CELL;
        let (y0, y1) = inset(f32::from(a.y0.max(b.y0)) * CELL, f32::from(a.y1.min(b.y1)) * CELL);
        if a.x1 == b.x0 {
            // Going +x, left is +y.
            Link { to, left: Vec2::new(x, y1), right: Vec2::new(x, y0) }
        } else {
            Link { to, left: Vec2::new(x, y0), right: Vec2::new(x, y1) }
        }
    } else {
        // One over the other: a horizontal edge.
        let y = f32::from(if a.y1 == b.y0 { a.y1 } else { a.y0 }) * CELL;
        let (x0, x1) = inset(f32::from(a.x0.max(b.x0)) * CELL, f32::from(a.x1.min(b.x1)) * CELL);
        if a.y1 == b.y0 {
            // Going +y, left is -x.
            Link { to, left: Vec2::new(x0, y), right: Vec2::new(x1, y) }
        } else {
            Link { to, left: Vec2::new(x1, y), right: Vec2::new(x0, y) }
        }
    }
}

#[inline]
fn cross(u: Vec2, v: Vec2) -> f32 {
    u.x * v.y - u.y * v.x
}

/// The funnel (the "simple stupid funnel algorithm"): the shortest line from `portals[0]` (a
/// point: both ends the same) through each portal in turn to the last (a point too), as its
/// corners after the start, the last point included. `emit` takes each; returning `false` stops.
pub fn funnel(portals: &[(Vec2, Vec2)], mut emit: impl FnMut(Vec2) -> bool) {
    let n = portals.len();
    if n < 2 {
        return;
    }
    let mut apex = portals[0].0;
    let (mut left, mut right) = (apex, apex);
    let (mut left_i, mut right_i) = (0usize, 0usize);
    let mut last = apex;
    let mut i = 1;
    let mut guard = 0;
    while i < n {
        guard += 1;
        if guard > 4 * n + 16 {
            break;
        }
        let (pl, pr) = portals[i];
        // The right edge closes in (or holds) ...
        if cross(right - apex, pr - apex) >= 0.0 {
            if apex == right || cross(left - apex, pr - apex) < 0.0 {
                right = pr;
                right_i = i;
            } else {
                // ... past the left one: the left's end is a corner.
                apex = left;
                let apex_i = left_i;
                if math::distance(apex, last) > 1e-3 {
                    if !emit(apex) {
                        return;
                    }
                    last = apex;
                }
                (left, right) = (apex, apex);
                (left_i, right_i) = (apex_i, apex_i);
                i = apex_i + 1;
                continue;
            }
        }
        // The left edge closes in (or holds) ...
        if cross(left - apex, pl - apex) <= 0.0 {
            if apex == left || cross(right - apex, pl - apex) > 0.0 {
                left = pl;
                left_i = i;
            } else {
                // ... past the right one: the right's end is a corner.
                apex = right;
                let apex_i = right_i;
                if math::distance(apex, last) > 1e-3 {
                    if !emit(apex) {
                        return;
                    }
                    last = apex;
                }
                (left, right) = (apex, apex);
                (left_i, right_i) = (apex_i, apex_i);
                i = apex_i + 1;
                continue;
            }
        }
        i += 1;
    }
    let end = portals[n - 1].0;
    if math::distance(end, last) > 1e-3 {
        emit(end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corners(portals: &[(Vec2, Vec2)]) -> ([Vec2; 8], usize) {
        let mut out = [Vec2::ZERO; 8];
        let mut n = 0;
        funnel(portals, |c| {
            out[n] = c;
            n += 1;
            true
        });
        (out, n)
    }

    #[test]
    fn a_straight_corridor_has_no_corner() {
        let a = Vec2::new(0.0, 0.0);
        let b = Vec2::new(100.0, 0.0);
        let p = [(a, a), (Vec2::new(50.0, 10.0), Vec2::new(50.0, -10.0)), (b, b)];
        let (out, n) = corners(&p);
        assert_eq!(&out[..n], &[b]);
    }

    #[test]
    fn a_bend_turns_at_the_inside_corner() {
        // East along y = 0, then north through a gap at x in [90, 110] at y = 50.
        let a = Vec2::new(0.0, 0.0);
        let b = Vec2::new(100.0, 200.0);
        let p = [
            (a, a),
            (Vec2::new(90.0, 10.0), Vec2::new(90.0, -10.0)),
            // Going +y, left is -x.
            (Vec2::new(90.0, 50.0), Vec2::new(110.0, 50.0)),
            (b, b),
        ];
        let (out, n) = corners(&p);
        // The straight line passes x = 90 at y = 180, far above the first gap: it turns at the
        // gap's top end, and from there it's clear through the second.
        assert_eq!(&out[..n], &[Vec2::new(90.0, 10.0), b]);
    }
}
