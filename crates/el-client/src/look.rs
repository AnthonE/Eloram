//! The picture's light and finish (`docs/ART.md`, step 1): a cold moon over the Causeway with
//! shadows, a blue ambient, the camera in HDR with tonemapping and bloom so fire and crystals
//! glow, a faint haze with distance. `?quality=low` keeps the light and drops the costly parts:
//! HDR and bloom, shadows, multisampling (FXAA instead), the braziers' lights and most of the
//! snow.

use bevy::anti_alias::fxaa::Fxaa;
use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::Msaa;

use crate::camera::MainCamera;
use crate::net::LaunchConfigRes;

/// How the picture is made.
#[derive(Resource)]
pub struct Look {
    /// The high tier: shadows, bloom, multisampling.
    pub high: bool,
    /// The fog of war's darkness drawn (a test turns it off to look at the whole map).
    pub fog: bool,
    /// The map's art loaded and drawn at all: not for the transport smoke test (`?mode=echo`),
    /// which has no match to draw.
    pub art: bool,
}

/// The moon's colour and the night's.
pub const MOON: Color = Color::srgb(0.78, 0.86, 1.0);
pub const NIGHT: Color = Color::srgb(0.40, 0.52, 0.78);
/// The haze far off and in the chasm.
pub const HAZE: Color = Color::srgb(0.035, 0.06, 0.12);

pub struct LookPlugin;

impl Plugin for LookPlugin {
    fn build(&self, app: &mut App) {
        let cfg = &app.world().resource::<LaunchConfigRes>().0;
        let (high, art) = (!cfg.low, !cfg.echo);
        app.insert_resource(Look { high, fog: true, art })
            .insert_resource(GlobalAmbientLight { color: NIGHT, brightness: 110.0, ..default() })
            .insert_resource(DirectionalLightShadowMap { size: if high { 2048 } else { 512 } })
            .insert_resource(ClearColor(HAZE))
            .add_systems(PostStartup, (dress_camera, light));
    }
}

fn dress_camera(mut commands: Commands, look: Res<Look>, cam: Single<Entity, With<MainCamera>>) {
    let mut cam = commands.entity(*cam);
    cam.insert((
        Tonemapping::TonyMcMapface,
        DistanceFog { color: HAZE, falloff: FogFalloff::Linear { start: 34.0, end: 120.0 }, ..default() },
    ));
    // HDR is for the bloom: without it, the low tier tonemaps in its shaders, with no float targets
    // and no pass of its own.
    if look.high {
        cam.insert((Hdr, Msaa::Sample4, Bloom { intensity: 0.18, ..Bloom::NATURAL }));
    } else {
        cam.insert((Msaa::Off, Fxaa::default()));
    }
}

fn light(mut commands: Commands, look: Res<Look>) {
    // From high up on the left and a little behind the camera: shadows fall to the right and a
    // little up the screen, and the cliffs below the near edge, which face the camera, are lit.
    commands.spawn((
        DirectionalLight {
            color: MOON,
            illuminance: 3_200.0,
            shadow_maps_enabled: look.high,
            shadow_depth_bias: 0.04,
            shadow_normal_bias: 1.2,
            ..default()
        },
        // The camera never sees more than about 40 m across: one cascade covers it all.
        CascadeShadowConfigBuilder {
            num_cascades: 1,
            minimum_distance: 1.0,
            maximum_distance: 70.0,
            first_cascade_far_bound: 70.0,
            overlap_proportion: 0.0,
        }
        .build(),
        Transform::from_xyz(-20.0, 30.0, 7.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
