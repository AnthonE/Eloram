//! How many cells A* expands for a few long searches.
#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_macros)]
use el_sim::map::{self, DAWN, DUSK, MID_Y, Map};
use el_sim::path::{self, Path, PathScratch};
use glam::Vec2;

fn main() {
    let m = Map::leaked();
    let mut s = PathScratch::new();
    let mut p = Path::default();
    let cases = [
        ("platform to platform", map::spawn_point(DAWN, 0), map::spawn_point(DUSK, 0)),
        ("base to mid", map::spawn_point(DAWN, 1), Vec2::new(7_680.0, MID_Y)),
        ("mid to enemy nexus front", Vec2::new(7_680.0, MID_Y + 300.0), map::march_goal(DAWN)),
        ("round a rock", Vec2::new(6_700.0, MID_Y - 300.0), Vec2::new(6_700.0, MID_Y - 1_200.0)),
        ("behind the inhibitor", Vec2::new(4_000.0, MID_Y), Vec2::new(2_800.0, MID_Y)),
    ];
    for (name, a, b) in cases {
        let e0 = s.expanded;
        let t = std::time::Instant::now();
        let ok = path::find(m, &mut s, a, b, &mut p);
        println!(
            "{name:<26} ok {ok} expanded {:>5} corners {:>2} in {:?}",
            s.expanded - e0,
            p.len,
            t.elapsed()
        );
    }
}
