//! What the Causeway looks like that the match doesn't know about (`docs/ART.md`): the outline of
//! its floor, which the browser builds its walls and cliffs along, and where the props stand.
//! It lives here, not in the browser client, so the tests can check it against the navigation grid
//! the server uses: the drawn edge is where a champion stops, and no prop stands where a unit can.

use el_sim::MAP_W;
use el_sim::map::{self, BASE, Disc, LANE, NECK, PLATFORM, Rect};
use glam::Vec2;

/// How far a point is outside the floor (negative inside), in map units: the floor is the union of
/// the lane, the necks, the bases and the platforms (`map::on_floor`).
pub fn floor_distance(p: Vec2) -> f32 {
    let rects = [LANE, NECK, NECK.mirrored()];
    let discs = [BASE, BASE.mirrored(), PLATFORM, PLATFORM.mirrored()];
    let r = rects.iter().map(|r| rect_distance(r, p));
    let d = discs.iter().map(|d| (p - d.0).length() - d.1);
    r.chain(d).fold(f32::INFINITY, f32::min)
}

fn rect_distance(r: &Rect, p: Vec2) -> f32 {
    let c = (r.0 + r.1) * 0.5;
    let h = (r.1 - r.0) * 0.5;
    let q = (p - c).abs() - h;
    q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0)
}

/// The way off the floor at `p`: the distance's gradient, normalised.
pub fn outward(p: Vec2) -> Vec2 {
    let e = 4.0;
    let g = Vec2::new(
        floor_distance(p + Vec2::X * e) - floor_distance(p - Vec2::X * e),
        floor_distance(p + Vec2::Y * e) - floor_distance(p - Vec2::Y * e),
    );
    g.normalize_or(Vec2::Y)
}

/// The floor's edge as closed loops of points `step` map units apart, each loop counter-clockwise
/// (the floor on its left). Marching squares over the distance at a quarter of a navigation cell,
/// so the corners where the lane meets the bases are where the sim has them.
pub fn outline(step: f32) -> Vec<Vec<Vec2>> {
    const GRID: f32 = 8.0;
    // A margin round the map, so every loop closes.
    let (x0, y0) = (-64.0, -64.0);
    let nx = ((MAP_W + 128.0) / GRID) as usize + 1;
    let ny = ((el_sim::MAP_H + 128.0) / GRID) as usize + 1;
    let at = |i: usize, j: usize| Vec2::new(x0 + i as f32 * GRID, y0 + j as f32 * GRID);
    let d: Vec<f32> =
        (0..ny).flat_map(|j| (0..nx).map(move |i| (i, j))).map(|(i, j)| floor_distance(at(i, j))).collect();
    let v = |i: usize, j: usize| d[j * nx + i];
    // The crossing on an edge between two corners.
    let cross = |a: Vec2, da: f32, b: Vec2, db: f32| a + (b - a) * (da / (da - db));
    let mut segs: Vec<(Vec2, Vec2)> = Vec::new();
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            let c = [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
            let s = [v(i, j), v(i + 1, j), v(i + 1, j + 1), v(i, j + 1)];
            let mut pts = [Vec2::ZERO; 4];
            let mut n = 0;
            for k in 0..4 {
                let (a, b) = (k, (k + 1) % 4);
                if (s[a] < 0.0) != (s[b] < 0.0) {
                    pts[n] = cross(c[a], s[a], c[b], s[b]);
                    n += 1;
                }
            }
            // Two crossings make a segment; the saddle's four never happen on these shapes'
            // edges at this grid, and would only pair up wrongly by a few units if they did.
            for [a, b] in pts[..n].as_chunks::<2>().0 {
                segs.push((*a, *b));
            }
        }
    }
    let loops = chain(segs);
    loops
        .into_iter()
        .map(|mut l| {
            // Counter-clockwise: positive area.
            let area: f32 = l.iter().zip(l.iter().cycle().skip(1)).map(|(a, b)| a.perp_dot(*b)).sum();
            if area < 0.0 {
                l.reverse();
            }
            resample(&l, step)
        })
        .collect()
}

/// Segments joined end to end into loops.
fn chain(segs: Vec<(Vec2, Vec2)>) -> Vec<Vec<Vec2>> {
    use std::collections::HashMap;
    let key = |p: Vec2| ((p.x * 16.0).round() as i64, (p.y * 16.0).round() as i64);
    let mut at: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (k, (a, b)) in segs.iter().enumerate() {
        at.entry(key(*a)).or_default().push(k);
        at.entry(key(*b)).or_default().push(k);
    }
    let mut used = vec![false; segs.len()];
    let mut loops = Vec::new();
    for start in 0..segs.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut l = vec![segs[start].0];
        let mut end = segs[start].1;
        loop {
            l.push(end);
            let next = at.get(&key(end)).and_then(|ks| ks.iter().copied().find(|&k| !used[k]));
            let Some(k) = next else { break };
            used[k] = true;
            end = if key(segs[k].0) == key(end) { segs[k].1 } else { segs[k].0 };
        }
        if l.len() > 8 {
            l.pop();
            loops.push(l);
        }
    }
    loops
}

/// A closed loop's points again, `step` apart along it.
fn resample(l: &[Vec2], step: f32) -> Vec<Vec2> {
    let total: f32 = l.iter().zip(l.iter().cycle().skip(1)).map(|(a, b)| a.distance(*b)).sum();
    let n = (total / step).round().max(3.0) as usize;
    let each = total / n as f32;
    let mut out = Vec::with_capacity(n);
    let (mut seg, mut along) = (0usize, 0.0f32);
    for k in 0..n {
        let want = k as f32 * each;
        loop {
            let (a, b) = (l[seg % l.len()], l[(seg + 1) % l.len()]);
            let len = a.distance(b);
            if along + len >= want || seg >= 2 * l.len() {
                let t = if len > 0.0 { (want - along) / len } else { 0.0 };
                out.push(a + (b - a) * t.clamp(0.0, 1.0));
                break;
            }
            along += len;
            seg += 1;
        }
    }
    out
}

/// The props: the browser loads a model each (`web/assets/models/<name>.glb`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prop {
    Brazier,
    Pillar,
    Statue,
    Banner,
    Arch,
    Spire,
    IceSpikes,
}

impl Prop {
    pub const fn model(self) -> &'static str {
        match self {
            Prop::Brazier => "brazier",
            Prop::Pillar => "pillar",
            Prop::Statue => "statue",
            Prop::Banner => "banner",
            Prop::Arch => "arch",
            Prop::Spire => "spire",
            Prop::IceSpikes => "rock_b",
        }
    }
}

/// A prop placed: where (map units), its height (game units, as tall as it stands), turned by
/// `yaw` (radians), its foot `depth` game units below the floor (things in the chasm stand far
/// down), and the room it takes (its radius on the ground).
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub prop: Prop,
    pub at: Vec2,
    pub height: f32,
    pub yaw: f32,
    pub depth: f32,
    pub radius: f32,
}

const fn put(prop: Prop, x: f32, y: f32, height: f32, yaw: f32, depth: f32, radius: f32) -> Placed {
    Placed { prop, at: Vec2::new(x, y), height, yaw, depth, radius }
}

/// The lane's edges: its near side (low y) and far side (high y), and a step out past the
/// parapet, where the props stand on rock that rises out of the chasm.
const NEAR: f32 = map::MID_Y - 850.0 - 140.0;
const FAR: f32 = map::MID_Y + 850.0 + 140.0;
const MID: f32 = map::MID_Y;

/// Dawn's half; Dusk's is its mirror ([`props`]).
pub const DAWN_PROPS: [Placed; 26] = [
    // Braziers along the lane's edges, between the turrets, and at the base's mouth.
    put(Prop::Brazier, 4_050.0, NEAR, 150.0, 0.3, 0.0, 70.0),
    put(Prop::Brazier, 4_050.0, FAR, 150.0, 1.1, 0.0, 70.0),
    put(Prop::Brazier, 5_750.0, NEAR, 150.0, 2.0, 0.0, 70.0),
    put(Prop::Brazier, 5_750.0, FAR, 150.0, 0.7, 0.0, 70.0),
    put(Prop::Brazier, 7_250.0, NEAR, 150.0, 1.4, 0.0, 70.0),
    put(Prop::Brazier, 7_250.0, FAR, 150.0, 2.6, 0.0, 70.0),
    // Broken columns between them.
    put(Prop::Pillar, 4_850.0, NEAR - 30.0, 380.0, 0.2, 0.0, 70.0),
    put(Prop::Pillar, 6_500.0, FAR + 30.0, 380.0, 1.9, 0.0, 70.0),
    put(Prop::Pillar, 3_350.0, FAR + 40.0, 420.0, 0.9, 0.0, 70.0),
    // Banners at the base's mouth.
    put(Prop::Banner, 3_380.0, NEAR, 420.0, 0.0, 0.0, 40.0),
    put(Prop::Banner, 2_700.0, MID + 1_380.0, 420.0, 0.0, 0.0, 40.0),
    // Sentinels flanking the platform, and over the base's back.
    put(Prop::Statue, 260.0, MID - 520.0, 520.0, 1.57, 0.0, 90.0),
    put(Prop::Statue, 260.0, MID + 520.0, 520.0, 1.57, 0.0, 90.0),
    put(Prop::Statue, 1_250.0, MID - 1_080.0, 480.0, 0.9, 0.0, 90.0),
    put(Prop::Statue, 1_250.0, MID + 1_080.0, 480.0, 2.2, 0.0, 90.0),
    // Ruins and ice in the chasm, far below: most of it beyond the far edge, where the camera
    // sees most.
    put(Prop::Spire, 2_400.0, MID + 1_950.0, 1_400.0, 0.4, 500.0, 300.0),
    put(Prop::Spire, 4_500.0, MID + 1_650.0, 1_100.0, 1.3, 600.0, 250.0),
    put(Prop::Spire, 6_200.0, MID + 2_100.0, 1_600.0, 2.1, 700.0, 300.0),
    put(Prop::Spire, 7_300.0, MID + 1_500.0, 900.0, 0.2, 500.0, 220.0),
    put(Prop::Arch, 3_500.0, MID + 1_850.0, 800.0, 0.2, 550.0, 300.0),
    put(Prop::Arch, 5_300.0, MID + 2_300.0, 900.0, 0.0, 650.0, 300.0),
    put(Prop::IceSpikes, 5_000.0, MID + 1_350.0, 700.0, 0.8, 450.0, 220.0),
    put(Prop::IceSpikes, 6_900.0, MID + 1_800.0, 800.0, 2.4, 500.0, 250.0),
    put(Prop::Spire, 3_100.0, MID - 1_300.0, 900.0, 1.1, 650.0, 200.0),
    put(Prop::IceSpikes, 5_400.0, MID - 1_200.0, 600.0, 0.3, 500.0, 200.0),
    put(Prop::Spire, 6_900.0, MID - 1_350.0, 1_000.0, 2.8, 700.0, 220.0),
];

/// Every prop: Dawn's, then their mirrors on Dusk's half (turned half round).
pub fn props() -> impl Iterator<Item = Placed> {
    DAWN_PROPS.iter().copied().chain(DAWN_PROPS.iter().map(|p| Placed {
        at: map::mirror(p.at),
        yaw: p.yaw + core::f32::consts::PI,
        ..*p
    }))
}

/// The rock pillars props on the edge stand on: one under each prop at floor height that is off
/// the floor.
pub fn plinths() -> impl Iterator<Item = Disc> {
    props().filter(|p| p.depth == 0.0 && floor_distance(p.at) > 0.0).map(|p| Disc(p.at, p.radius + 50.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use el_sim::map::{CELL, CLEARANCE, Map, NAV_H, NAV_W};

    fn walkable_near(m: &Map, p: Vec2, r: f32) -> bool {
        let (cx, cy) = Map::cell(p);
        let k = (r / CELL).ceil() as isize + 1;
        for dy in -k..=k {
            for dx in -k..=k {
                let (x, y) = (cx as isize + dx, cy as isize + dy);
                if x < 0 || y < 0 || x >= NAV_W as isize || y >= NAV_H as isize {
                    continue;
                }
                let (x, y) = (x as usize, y as usize);
                if m.walkable_cell(x, y) && Map::cell_centre(x, y).distance(p) < r {
                    return true;
                }
            }
        }
        false
    }

    /// One loop, round the whole floor.
    #[test]
    fn the_outline_is_one_loop() {
        let loops = outline(50.0);
        assert_eq!(loops.len(), 1, "{:?}", loops.iter().map(Vec::len).collect::<Vec<_>>());
        assert!(loops[0].len() > 500);
    }

    /// The drawn edge is where a champion stops: no unit's centre may stand within the clearance
    /// of it, and there's always floor a unit can stand on a couple of cells inside it.
    #[test]
    fn the_edge_is_where_the_walking_stops() {
        let m = Map::build();
        // Where rock stands over the edge, the rock is what stops a champion.
        let under_rock = |p: Vec2| {
            map::ROCKS.iter().flat_map(|d| [*d, d.mirrored()]).any(|d| d.contains(p, CLEARANCE + 2.5 * CELL))
        };
        for p in &outline(25.0)[0] {
            assert!(floor_distance(*p).abs() < 1.0, "{p} is {} off the edge", floor_distance(*p));
            assert!(!walkable_near(&m, *p, CLEARANCE - 8.0), "a unit can stand on the wall at {p}");
            assert!(
                under_rock(*p) || walkable_near(&m, *p, CLEARANCE + 2.5 * CELL),
                "the wall at {p} is far from any floor"
            );
        }
    }

    /// The parapet stands outward of the edge: `outward` points off the floor all round. (Where
    /// a base and its platform all but touch, the rock between is a few units thick: a short step
    /// off the floor is all there is.)
    #[test]
    fn outward_points_off_the_floor() {
        for p in &outline(50.0)[0] {
            assert!(floor_distance(*p + outward(*p) * 10.0) > 2.0, "at {p}");
        }
    }

    /// No prop, nor the rock it stands on, takes room a unit can walk in.
    #[test]
    fn props_stand_off_the_walkable_floor() {
        let m = Map::build();
        for p in props() {
            assert!(!walkable_near(&m, p.at, p.radius), "{:?} at {} stands on walkable floor", p.prop, p.at);
        }
        for d in plinths() {
            assert!(!walkable_near(&m, d.0, d.1 - 30.0), "a plinth at {} stands on walkable floor", d.0);
        }
    }
}
