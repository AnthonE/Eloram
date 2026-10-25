//! The map in the scene: a game unit is a centimetre, a scene unit a metre (so the Causeway is
//! 153.6 by 38.4), the lane along +X, the map's +y away from the camera (−Z).

use bevy::math::{Vec2, Vec3};
use el_sim::MAP_W;
use el_sim::map::MID_Y;

/// Scene units a game unit.
pub const S: f32 = 0.01;

/// Where a point of the map is in the scene, `h` above the floor.
pub fn world(p: Vec2, h: f32) -> Vec3 {
    Vec3::new((p.x - MAP_W * 0.5) * S, h, (MID_Y - p.y) * S)
}

/// The point of the map under a point of the scene.
pub fn map_point(w: Vec3) -> Vec2 {
    Vec2::new(w.x / S + MAP_W * 0.5, MID_Y - w.z / S)
}
