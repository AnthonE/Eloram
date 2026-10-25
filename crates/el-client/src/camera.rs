//! The camera: high over the lane at a fixed pitch, as a MOBA's is. Y locks it to the champion (the
//! default) or frees it; free, it pans when the cursor touches the window's edge or with the arrow
//! keys; Space, held, brings it back to the champion. The wheel zooms a little.

use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use el_sim::{MAP_H, MAP_W};

use crate::coords::world;
use crate::input::Controls;
use crate::session::Session;

#[derive(Component)]
pub struct MainCamera;

#[derive(Resource)]
pub struct Rig {
    /// The map point the camera looks at.
    pub focus: Vec2,
    pub zoom: f32,
    pub locked: bool,
    /// It has been put on the champion once.
    placed: bool,
}

impl Default for Rig {
    fn default() -> Self {
        Self { focus: Vec2::new(1_200.0, el_sim::map::MID_Y), zoom: 1.0, locked: true, placed: false }
    }
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Rig>()
            .add_systems(Startup, setup)
            .add_systems(PostUpdate, follow.before(TransformSystems::Propagate));
    }
}

/// The camera's distance from what it looks at (scene units), and its pitch below the horizon.
const DISTANCE: f32 = 23.0;
const PITCH: f32 = 56.0 * std::f32::consts::PI / 180.0;
/// Game units a second the camera pans.
const PAN: f32 = 2_400.0;
/// Pixels from the window's edge that pan.
const EDGE: f32 = 10.0;

fn setup(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        MainCamera,
        Transform::from_xyz(0.0, 12.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

pub fn offset(zoom: f32) -> Vec3 {
    Vec3::new(0.0, PITCH.sin(), PITCH.cos()) * DISTANCE * zoom
}

#[allow(clippy::too_many_arguments)]
fn follow(
    mut rig: ResMut<Rig>,
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    window: Single<&Window>,
    time: Res<Time>,
    session: Res<Session>,
    mut controls: ResMut<Controls>,
    mut cam: Single<&mut Transform, With<MainCamera>>,
) {
    let me = session.client.as_ref().map(|c| c.own_pos(crate::net::now_s()));
    if keys.just_pressed(KeyCode::KeyY) {
        rig.locked = !rig.locked;
    }
    if let Some(me) = me
        && (!rig.placed || rig.locked || keys.pressed(KeyCode::Space))
    {
        rig.focus = me;
        rig.placed = true;
    }
    if !rig.locked {
        let dt = time.delta_secs();
        let mut pan = Vec2::ZERO;
        if let Some(p) = window.cursor_position() {
            let size = window.size();
            if p.x <= EDGE {
                pan.x -= 1.0;
            }
            if p.x >= size.x - EDGE {
                pan.x += 1.0;
            }
            if p.y <= EDGE {
                pan.y += 1.0;
            }
            if p.y >= size.y - EDGE {
                pan.y -= 1.0;
            }
        }
        for (k, d) in [
            (KeyCode::ArrowLeft, Vec2::NEG_X),
            (KeyCode::ArrowRight, Vec2::X),
            (KeyCode::ArrowUp, Vec2::Y),
            (KeyCode::ArrowDown, Vec2::NEG_Y),
        ] {
            if keys.pressed(k) {
                pan += d;
            }
        }
        rig.focus += pan.normalize_or_zero() * PAN * dt;
    }
    if scroll.delta.y != 0.0 {
        rig.zoom = (rig.zoom * (1.0 - 0.08 * scroll.delta.y.signum())).clamp(0.7, 1.4);
    }
    rig.focus = rig.focus.clamp(Vec2::new(600.0, 900.0), Vec2::new(MAP_W - 600.0, MAP_H - 900.0));
    controls.view = rig.focus;
    let target = world(rig.focus, 0.0);
    **cam = Transform::from_translation(target + offset(rig.zoom)).looking_at(target, Vec3::Y);
}
