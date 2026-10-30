//! The picture's light and finish (`docs/ART.md`, step 1): a cold moon over the Causeway with
//! shadows, a blue ambient, tonemapping, a faint haze with distance. `?quality=low` keeps the light
//! and drops the costly parts: shadows, multisampling (FXAA instead), the braziers' lights and most
//! of the snow.
//!
//! The WebGPU build's high tier adds what WebGL2 shouldn't have: HDR with bloom (Bevy's bloom isn't
//! for WebGL2, and Gates saw it blow a moon out into a white glow there) and a sharper shadow map.
//! Not ambient occlusion: Bevy's wants five storage textures a stage, more than many adapters
//! give, and its depth and normal prepasses add view textures that take the ground's material
//! past the sixteen a stage WebGPU allows (measured: 17, and Bevy quits on the error).
//!
//! On the WebGPU build a render error doesn't end the client: the page goes back to the WebGL2
//! module (loader.js, `elGpuFallBack`), once for that build, and a reload takes the seat back.

use bevy::anti_alias::fxaa::Fxaa;
use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::Msaa;

use crate::camera::{MainCamera, VIEW};
use crate::net::LaunchConfigRes;

/// The WebGPU build (Bevy picks its GPU API when it's compiled).
pub const WEBGPU: bool = cfg!(feature = "webgpu");

/// How the picture is made.
#[derive(Resource)]
pub struct Look {
    /// The high tier: shadows, multisampling (on WebGPU: HDR and bloom).
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
            .insert_resource(DirectionalLightShadowMap {
                size: match (high, WEBGPU) {
                    (true, true) => 4096,
                    (true, false) => 2048,
                    _ => 512,
                },
            })
            .insert_resource(ClearColor(HAZE))
            .add_systems(PostStartup, (dress_camera, light));
        if WEBGPU {
            app.insert_resource(bevy::render::error_handler::RenderErrorHandler(on_render_error));
        }
    }
}

fn dress_camera(mut commands: Commands, look: Res<Look>, cam: Single<Entity, With<MainCamera>>) {
    let mut cam = commands.entity(*cam);
    cam.insert((
        Tonemapping::TonyMcMapface,
        // The haze from a little past what the camera looks at (at its widest zoom), into the chasm.
        DistanceFog {
            color: HAZE,
            falloff: FogFalloff::Linear { start: VIEW.distance * 1.45, end: VIEW.distance * 5.2 },
            ..default()
        },
    ));
    // HDR is for the bloom: without it, the picture tonemaps in its shaders, with no float targets
    // and no pass of its own.
    match (look.high, WEBGPU) {
        (true, true) => {
            cam.insert((Hdr, Msaa::Sample4, Bloom { intensity: 0.18, ..Bloom::NATURAL }));
        }
        (true, false) => {
            cam.insert(Msaa::Sample4);
        }
        (false, _) => {
            cam.insert((Msaa::Off, Fxaa::default()));
        }
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
        // Turned with the camera, so the light falls the same way across the screen.
        Transform::from_translation(crate::camera::VIEW.turn() * Vec3::new(-20.0, 30.0, 7.0))
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// A render error on the WebGPU build: back to the WebGL2 module if the page can take us there
/// (it reloads), else Bevy's own answer (stop, and say so).
fn on_render_error(
    error: &bevy::render::error_handler::RenderError,
    main_world: &mut World,
    _: &mut World,
) -> bevy::render::error_handler::RenderErrorPolicy {
    use bevy::render::error_handler::RenderErrorPolicy;
    use wasm_bindgen::{JsCast, JsValue};
    let fell_back = web_sys::window()
        .and_then(|w| js_sys::Reflect::get(&w, &JsValue::from_str("elGpuFallBack")).ok())
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .and_then(|f| f.call1(&JsValue::NULL, &JsValue::from_str(&error.description)).ok())
        .is_some_and(|r| r.is_truthy());
    if !fell_back {
        bevy::log::error!("Quitting the application due to {:?} RenderError", error.ty);
        main_world.write_message(AppExit::error());
    }
    RenderErrorPolicy::StopRendering
}
