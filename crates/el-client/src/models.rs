//! The champions' figures until there's art: one a role, built from Bevy's primitives into a single
//! mesh, feet on the floor at the origin and facing +X (the unit's facing turns +X to it). Each is
//! drawn in its champion's colour; the role shows in the silhouette: a marksman's long gun, a
//! mage's robe and staff, a tank's shoulders and shield, a fighter's blade, an assassin's crouch and
//! two knives, an enchanter's robe, halo and lantern.

use bevy::prelude::*;
use el_sim::content::Role;

/// A part: `shape` at `at`, turned by `turn`.
fn part(shape: impl Into<Mesh>, at: Vec3, turn: Quat) -> Mesh {
    shape.into().transformed_by(Transform::from_translation(at).with_rotation(turn))
}

fn put(shape: impl Into<Mesh>, x: f32, y: f32, z: f32) -> Mesh {
    part(shape, Vec3::new(x, y, z), Quat::IDENTITY)
}

/// The parts, as one mesh.
fn join(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut mesh = it.next().unwrap_or_else(|| Sphere::new(0.5).into());
    for p in it {
        if let Err(e) = mesh.merge(&p) {
            bevy::log::warn!("a figure's part didn't join: {e}");
        }
    }
    mesh
}

/// Along +X, lying down (a cylinder stands on Y).
fn lying() -> Quat {
    Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)
}

/// The figure for a champion of `role`.
pub fn figure(role: Role) -> Mesh {
    join(match role {
        Role::Marksman => vec![
            put(Capsule3d::new(0.32, 0.7), 0.0, 0.67, 0.0),
            put(Sphere::new(0.23), 0.0, 1.55, 0.0),
            // The gun, held at the hip, out in front.
            part(Cylinder::new(0.06, 1.2), Vec3::new(0.5, 0.95, 0.24), lying()),
            put(Cuboid::new(0.3, 0.16, 0.12), 0.05, 0.92, 0.24),
            // A long coat's tails.
            put(Cuboid::new(0.12, 0.5, 0.55), -0.28, 0.45, 0.0),
        ],
        Role::Mage => vec![
            put(Cone { radius: 0.55, height: 1.25 }, 0.0, 0.62, 0.0),
            put(Sphere::new(0.24), 0.0, 1.4, 0.0),
            // A pointed hood.
            put(Cone { radius: 0.27, height: 0.45 }, 0.0, 1.72, 0.0),
            // The staff and its stone.
            put(Cylinder::new(0.045, 1.9), 0.22, 0.95, 0.46),
            put(Sphere::new(0.15), 0.22, 1.98, 0.46),
        ],
        Role::Tank => vec![
            put(Cuboid::new(0.65, 0.5, 0.85), 0.0, 0.25, 0.0),
            put(Cuboid::new(0.95, 0.85, 1.15), 0.0, 0.92, 0.0),
            put(Sphere::new(0.34), 0.0, 1.45, 0.62),
            put(Sphere::new(0.34), 0.0, 1.45, -0.62),
            put(Sphere::new(0.26), 0.05, 1.6, 0.0),
            // The shield, held before it.
            put(Cuboid::new(0.12, 1.0, 0.8), 0.62, 0.85, 0.32),
        ],
        Role::Fighter => vec![
            put(Capsule3d::new(0.38, 0.7), 0.0, 0.73, 0.0),
            put(Sphere::new(0.25), 0.0, 1.66, 0.0),
            put(Cuboid::new(0.5, 0.18, 1.0), 0.0, 1.3, 0.0),
            // The blade, held forward at the side.
            put(Cuboid::new(1.3, 0.09, 0.18), 0.6, 0.95, -0.46),
            put(Cuboid::new(0.08, 0.1, 0.4), 0.0, 0.95, -0.46),
        ],
        Role::Assassin => vec![
            // Crouched, leaning in.
            part(Capsule3d::new(0.3, 0.55), Vec3::new(0.05, 0.6, 0.0), Quat::from_rotation_z(-0.4)),
            put(Sphere::new(0.21), 0.28, 1.15, 0.0),
            // A hood's point behind.
            part(Cone { radius: 0.2, height: 0.4 }, Vec3::new(0.12, 1.3, 0.0), Quat::from_rotation_z(0.6)),
            // Two knives, low and forward.
            put(Cuboid::new(0.65, 0.05, 0.09), 0.45, 0.62, 0.36),
            put(Cuboid::new(0.65, 0.05, 0.09), 0.45, 0.62, -0.36),
        ],
        Role::Enchanter => vec![
            put(Cone { radius: 0.5, height: 1.1 }, 0.0, 0.55, 0.0),
            put(Sphere::new(0.22), 0.0, 1.25, 0.0),
            // The halo, flat over the head.
            put(Torus::new(0.26, 0.34), 0.0, 1.66, 0.0),
            // The lantern, held out.
            put(Cylinder::new(0.03, 0.4), 0.42, 1.15, 0.32),
            put(Sphere::new(0.16), 0.42, 0.9, 0.32),
        ],
    })
}
