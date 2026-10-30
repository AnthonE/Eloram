//! The camera: high over the lane at a fixed pitch, as a MOBA's is, and turned as League's ARAM's
//! is, the lane running from the screen's bottom left to its top right ([`VIEW`]); and moved the
//! way League's hands know. Free to start (`?camera=locked` or Y the other way, remembered): it pans when the
//! cursor touches the window's edge, with the arrow keys, or dragged with the middle button; Space
//! (or F1) held brings it to the champion, F2 to F5 held to each ally; a left click on the minimap
//! (or a drag) looks there. Locked (Y), it rides the champion a little ahead, towards the enemy's
//! end, and a look at the minimap lasts while the button is held. It comes back to the champion
//! when the match starts and at each respawn. The wheel zooms a little, smoothly.

use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use el_sim::units::{SEATS, team_of_seat};
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
    /// The zoom drawn, and the one the wheel asks for (the first eases to the second).
    pub zoom: f32,
    pub zoom_to: f32,
    pub locked: bool,
    /// It has been put on the champion once.
    pub placed: bool,
    /// The minimap held this frame: the camera looks there (`hud.rs`).
    pub peek: Option<Vec2>,
    /// The middle button's drag: the map point it took hold of.
    grab: Option<Vec2>,
    /// The champion was alive last frame (a respawn brings the camera back).
    alive: bool,
}

impl Default for Rig {
    fn default() -> Self {
        Self {
            focus: Vec2::new(1_200.0, el_sim::map::MID_Y),
            zoom: 1.0,
            zoom_to: 1.0,
            locked: false,
            placed: false,
            peek: None,
            grab: None,
            alive: true,
        }
    }
}

impl Rig {
    /// Locks the camera on the champion or frees it, and remembers which for the next visit.
    pub fn set_locked(&mut self, on: bool) {
        self.locked = on;
        crate::config::store_camera(on);
    }

    /// Put at a zoom at once, not eased to it.
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom;
        self.zoom_to = zoom;
    }
}

/// A shake of the camera, kicked when something heavy lands on the viewer's champion (`fx.rs`):
/// how hard, and when.
#[derive(Resource, Default)]
pub struct Shake {
    amp: f32,
    at: f64,
}

impl Shake {
    pub fn kick(&mut self, amp: f32, now: f64) {
        // A new kick on a fading one: the stronger of what's left and what's new.
        let left = self.now(now);
        self.amp = amp.max(left).min(0.25);
        self.at = now;
    }

    /// How far (m) it moves the camera at `now`.
    fn now(&self, now: f64) -> f32 {
        self.amp * (-(now - self.at).max(0.0) as f32 * 9.0).exp()
    }

    fn offset(&self, now: f64) -> Vec3 {
        let a = self.now(now);
        if a < 0.002 {
            return Vec3::ZERO;
        }
        let t = now as f32;
        Vec3::new((t * 53.0).sin(), (t * 41.0 + 1.3).sin() * 0.5, (t * 47.0 + 2.1).sin()) * a
    }
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Rig>()
            .init_resource::<Shake>()
            .add_systems(Startup, setup)
            .add_systems(PostUpdate, follow.before(TransformSystems::Propagate));
    }
}

/// How the camera sees the map: turned about the vertical from looking straight up the map (+y),
/// so the lane runs from the screen's bottom left to its top right as League's ARAM does; its pitch
/// below the horizon; its vertical field of view; its distance (scene units) at zoom 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,
    pub distance: f32,
}

const DEG: f32 = std::f32::consts::PI / 180.0;

pub const VIEW: View = View { yaw: 45.0 * DEG, pitch: 52.0 * DEG, fov: 36.0 * DEG, distance: 30.0 };

impl View {
    /// The screen's right and up, as directions on the map.
    pub fn axes(&self) -> (Vec2, Vec2) {
        let (s, c) = self.yaw.sin_cos();
        (Vec2::new(c, -s), Vec2::new(s, c))
    }

    /// From what the camera looks at to the camera, at `zoom`.
    pub fn offset(&self, zoom: f32) -> Vec3 {
        let (_, up) = self.axes();
        // Back down the screen, in the scene (the map's +y is the scene's −Z).
        let back = Vec3::new(-up.x, 0.0, up.y);
        (back * self.pitch.cos() + Vec3::Y * self.pitch.sin()) * self.distance * zoom
    }

    /// The turn that faces a flat thing lying in the scene's XY to the camera, its +X to the
    /// screen's right.
    pub fn facing(&self) -> Quat {
        Quat::from_rotation_y(-self.yaw) * Quat::from_rotation_x(-self.pitch)
    }

    /// The scene's turn about the vertical that the camera has.
    pub fn turn(&self) -> Quat {
        Quat::from_rotation_y(-self.yaw)
    }
}

/// A steer (`el_sim::steer_dir`: 0 none, 1 to 8 counter-clockwise by eighths from +x) given in the
/// screen's directions, turned into the map's.
pub fn steer_on_map(screen: u8) -> u8 {
    if screen == 0 {
        return 0;
    }
    let eighths = (VIEW.yaw / (std::f32::consts::PI / 4.0)).round() as i32;
    ((i32::from(screen) - 1 - eighths).rem_euclid(8) + 1) as u8
}
/// Game units a second the camera pans at the edge or with the arrows (at zoom 1).
const PAN: f32 = 2_600.0;
/// Pixels from the window's edge that pan.
const EDGE: f32 = 12.0;
/// How far ahead of the champion (game units, towards the enemy's end) the locked camera looks:
/// League's "per-side offset", more of the lane ahead than behind.
const AHEAD: f32 = 260.0;
/// How quickly the locked camera catches the champion, and the zoom the wheel's (a second's
/// exponent): quick enough to feel held, slow enough that a blink or a respawn glides.
const CATCH: f32 = 18.0;
const EASE_ZOOM: f32 = 12.0;
/// The wheel's reach.
const ZOOM_IN: f32 = 0.7;
const ZOOM_OUT: f32 = 1.4;

fn setup(mut commands: Commands, cfg: Res<crate::net::LaunchConfigRes>, mut rig: ResMut<Rig>) {
    rig.locked = cfg.0.camera_locked;
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { fov: VIEW.fov, ..default() }),
        MainCamera,
        Transform::from_xyz(0.0, 12.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// The function keys that look at a champion while held: F1 the player's own, F2 to F5 the
/// allies in seat order.
const LOOK_AT: [KeyCode; 5] = [KeyCode::F1, KeyCode::F2, KeyCode::F3, KeyCode::F4, KeyCode::F5];

#[allow(clippy::too_many_arguments)]
fn follow(
    mut rig: ResMut<Rig>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    window: Single<&Window>,
    time: Res<Time>,
    session: Res<Session>,
    drawn: Res<crate::view::Drawn>,
    shake: Res<Shake>,
    mut controls: ResMut<Controls>,
    mut cam: Single<&mut Transform, With<MainCamera>>,
) {
    let dt = time.delta_secs();
    let client = session.client.as_deref();
    let me = client.map(|c| c.own_pos(crate::net::now_s()));
    // Watching, there's no champion of one's own: the camera is free from the start.
    if session.watch.is_some() && !rig.placed {
        rig.locked = false;
        rig.placed = true;
        rig.focus = Vec2::new(MAP_W * 0.5, el_sim::map::MID_Y);
    }
    if keys.just_pressed(KeyCode::KeyY) && session.watch.is_none() {
        let on = !rig.locked;
        rig.set_locked(on);
    }
    // Back on the champion at a respawn, as at the start.
    let alive = client.is_none_or(|c| c.pred.units[c.seat].alive);
    let respawned = alive && !rig.alive;
    rig.alive = alive;
    // A champion held to look at: Space or F1 the player's own, F2 to F5 an ally's.
    let held = client.and_then(|c| {
        if keys.pressed(KeyCode::Space) || keys.pressed(LOOK_AT[0]) {
            return me;
        }
        let allies = (0..SEATS).filter(|&s| s != c.seat && team_of_seat(s) == c.team);
        LOOK_AT[1..]
            .iter()
            .zip(allies)
            .find(|(k, _)| keys.pressed(**k))
            .and_then(|(_, s)| drawn.units[s].map(|d| d.pos))
    });
    if let Some(me) = me
        && (!rig.placed || respawned)
    {
        rig.focus = me;
        rig.placed = true;
    }
    if let Some(at) = rig.peek.take() {
        // The minimap: a look while it's held.
        rig.focus = at;
        rig.grab = None;
    } else if let Some(at) = held {
        rig.focus = at;
        rig.grab = None;
    } else if rig.locked {
        if let (Some(me), Some(c)) = (me, client) {
            let ahead = if c.team == 0 { AHEAD } else { -AHEAD };
            let want = me + Vec2::new(ahead, 0.0);
            rig.focus = rig.focus.lerp(want, 1.0 - (-CATCH * dt).exp());
        }
        rig.grab = None;
    } else {
        // Free: the middle button drags the map under the cursor, else the edges and the arrows.
        if mouse.just_pressed(MouseButton::Middle) {
            rig.grab = controls.cursor;
        }
        if !mouse.pressed(MouseButton::Middle) {
            rig.grab = None;
        }
        if let (Some(grab), Some(at)) = (rig.grab, controls.cursor) {
            // The cursor's ground point is read through last frame's camera, which already moved by
            // last frame's drag: what's left is this frame's.
            rig.focus += grab - at;
        } else {
            let mut pan = Vec2::ZERO;
            if let Some(p) = window.cursor_position().filter(|_| window.focused) {
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
            // The screen's directions on the map.
            let (right, up) = VIEW.axes();
            let speed = PAN * rig.zoom;
            rig.focus += (right * pan.x + up * pan.y).normalize_or_zero() * speed * dt;
        }
    }
    if scroll.delta.y != 0.0 {
        rig.zoom_to = (rig.zoom_to * (1.0 - 0.1 * scroll.delta.y.signum())).clamp(ZOOM_IN, ZOOM_OUT);
    }
    rig.zoom += (rig.zoom_to - rig.zoom) * (1.0 - (-EASE_ZOOM * dt).exp());
    rig.focus = rig.focus.clamp(Vec2::new(600.0, 900.0), Vec2::new(MAP_W - 600.0, MAP_H - 900.0));
    controls.view = rig.focus;
    let target = world(rig.focus, 0.0);
    let jolt = shake.offset(crate::net::now_s());
    **cam =
        Transform::from_translation(target + VIEW.offset(rig.zoom) + jolt).looking_at(target + jolt, Vec3::Y);
}
