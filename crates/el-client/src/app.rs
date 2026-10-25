use bevy::prelude::*;

use crate::config::LaunchConfig;
use crate::dev_hooks::DevHooksPlugin;
use crate::net::{LaunchConfigRes, NetPlugin};

pub fn run() {
    console_error_panic_hook::set_once();
    let cfg = LaunchConfig::from_window();
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Eloram".into(),
                canvas: Some("#el".into()),
                fit_canvas_to_parent: true,
                prevent_default_event_handling: true,
                ..default()
            }),
            ..default()
        }))
        .insert_resource(bevy::winit::WinitSettings::continuous())
        .insert_resource(LaunchConfigRes(cfg))
        .insert_resource(ClearColor(Color::srgb(0.02, 0.03, 0.05)))
        .add_plugins((DevHooksPlugin, NetPlugin))
        .add_systems(Startup, setup_scene)
        .run();
}

/// A stand-in for the Causeway until the match is drawn: the lane as a slab over the dark, a
/// turret's post at each end, seen from the camera a MOBA player expects (high, tilted, looking
/// up the lane).
fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands
        .spawn((Camera3d::default(), Transform::from_xyz(0.0, 22.0, 16.0).looking_at(Vec3::ZERO, Vec3::Y)));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(40.0, 0.5, 6.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.62, 0.70),
            perceptual_roughness: 0.8,
            ..default()
        })),
        Transform::from_rotation(Quat::from_rotation_y(-0.6)),
    ));
    for (x, color) in [(-14.0, Color::srgb(0.25, 0.55, 0.95)), (14.0, Color::srgb(0.95, 0.35, 0.25))] {
        let at = Quat::from_rotation_y(-0.6) * Vec3::new(x, 1.5, 0.0);
        commands.spawn((
            Mesh3d(meshes.add(Cylinder::new(0.8, 3.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                emissive: LinearRgba::from(color) * 0.6,
                ..default()
            })),
            Transform::from_translation(at),
        ));
    }
    commands.spawn((
        DirectionalLight { illuminance: 8_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(6.0, 14.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
