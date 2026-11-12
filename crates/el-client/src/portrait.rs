//! The portraits: the player's own champion and each ally's, their models drawn live from the
//! chest up into small pictures the HUD shows (`hud.rs`), as League's portraits show the
//! champions. Each portrait has a camera of its own that sees only its own render layer
//! ([`layer`]), which its champion's figure is on as well as the scene's (`champions.rs` puts it
//! there once its model is in), and a key light of its own that turns with it; it stands in front
//! of the figure, a little to one side, and turns as it turns.
//!
//! A portrait draws only while its figure is shown. The player's own draws every frame; the
//! allies' take turns, one a frame, so four more pictures cost about one.

use bevy::camera::RenderTarget;
use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

/// The portraits: the player's own (0), then each ally's in seat order (F2 to F5's).
pub const VIEWS: usize = el_sim::units::TEAM_SIZE;

/// The render layer portrait `view`'s camera sees.
pub const fn layer(view: usize) -> usize {
    1 + view
}

/// Each picture's side, in pixels: about twice what the HUD shows, for a clean edge.
const SIDE: [u32; 2] = [136, 80];

/// The pictures the portraits' cameras draw into, by view.
#[derive(Resource)]
pub struct PortraitImage(pub [Handle<Image>; VIEWS]);

/// Which portraits are being drawn (the HUD hides the initials under one while it is).
#[derive(Resource, Default)]
pub struct PortraitShown(pub [bool; VIEWS]);

/// A figure a portrait shows, and which; `still` while its champion is down, when the portrait
/// keeps the last face it drew rather than the body on the ground (or nothing, once it's gone).
#[derive(Component)]
pub struct PortraitSubject {
    pub view: usize,
    pub still: bool,
    /// Its head and shoulders, which the camera frames and faces: a stance that crouches, leans
    /// or turns (an assassin's, a guard's behind its shield) carries the head out of any frame
    /// fixed to the body, or shows the back of it. `None` until they're found (the warm-up's
    /// piece): the frame is then a standing figure's.
    pub head: Option<Head>,
}

/// A figure's head and shoulders by their bones: the head's base (the neck's top) and its crown
/// (`Head`, `head_end` in the champions' models), and the two shoulders. Only where they are is
/// read, never how they're turned: these rigs are made by a machine, and a head's skin doesn't
/// always hang on its bone's turn.
#[derive(Clone, Copy)]
pub struct Head {
    pub base: Entity,
    pub top: Entity,
    /// The left shoulder and the right.
    pub shoulders: [Entity; 2],
}

impl Head {
    /// The head among a figure's nodes, by their names.
    pub fn find<'a>(nodes: impl IntoIterator<Item = (Entity, &'a str)>) -> Option<Head> {
        let (mut base, mut top, mut left, mut right) = (None, None, None, None);
        for (e, name) in nodes {
            match name {
                "Head" => base = Some(e),
                "head_end" => top = Some(e),
                "LeftArm" => left = Some(e),
                "RightArm" => right = Some(e),
                _ => {}
            }
        }
        Some(Head { base: base?, top: top?, shoulders: [left?, right?] })
    }
}

#[derive(Component)]
struct PortraitCam(usize);

/// A sitting (`{t:"face",on:true}`, `scripts/e2e.sh faces`): the player's own portrait drawn
/// [`SITTING`] pixels a side over a plain ground and shown at the window's top left, alone and as
/// it's drawn, for a picture of it to be taken (`faces.rs` shows those).
#[derive(Resource, Default)]
pub struct Sitting(pub bool);

/// A sitting's side, in pixels.
const SITTING: u32 = 256;
/// What's behind a face: the UI's slate, a little lit.
const SITTING_GROUND: Color = Color::srgb(0.085, 0.10, 0.135);

/// The sitting's picture on the screen.
#[derive(Component)]
struct SittingNode;

/// Whose turn it is among the allies' portraits.
#[derive(Resource, Default)]
struct Turn(usize);

pub struct PortraitPlugin;

impl Plugin for PortraitPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PortraitShown>()
            .init_resource::<Turn>()
            .init_resource::<Sitting>()
            .add_systems(Update, sit.run_if(resource_changed::<Sitting>))
            .add_systems(Startup, setup)
            // After this frame's transforms (the figure and its bones where they'll be drawn: a
            // frame behind, a champion running at a low frame rate is out of its own picture)
            // and before the cameras' frusta are made from theirs.
            .add_systems(
                PostUpdate,
                follow.after(TransformSystems::Propagate).before(VisibilitySystems::UpdateFrusta),
            );
    }
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let handles: [Handle<Image>; VIEWS] = std::array::from_fn(|view| {
        let side = SIDE[usize::from(view > 0)];
        // sRGB itself, with no second view of it: WebGL2 has no view formats.
        images.add(Image::new_target_texture(side, side, TextureFormat::Rgba8UnormSrgb, None))
    });
    for (view, image) in handles.iter().enumerate() {
        let layer = RenderLayers::layer(layer(view));
        commands
            .spawn((
                Camera3d::default(),
                Camera {
                    // Before the main view, and clear where there's no champion: the HUD's well
                    // shows.
                    order: -1 - view as isize,
                    is_active: false,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                RenderTarget::Image(image.clone().into()),
                Projection::Perspective(PerspectiveProjection { fov: 26f32.to_radians(), ..default() }),
                // Brighter than the night's, so the face reads.
                AmbientLight { color: Color::srgb(0.75, 0.82, 1.0), brightness: 420.0, ..default() },
                Msaa::Sample4,
                layer.clone(),
                PortraitCam(view),
                Transform::default(),
            ))
            .with_children(|cam| {
                // A warm key from above the camera's left, turning with it.
                cam.spawn((
                    DirectionalLight {
                        illuminance: 4_200.0,
                        color: Color::srgb(1.0, 0.92, 0.82),
                        ..default()
                    },
                    Transform::from_xyz(-1.0, 1.4, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
                    layer,
                ));
            });
    }
    commands.insert_resource(PortraitImage(handles));
}

/// Where the camera stands from the middle of the face, in the figure's heights: a little to the
/// face's right, just under its eyes (a bowed head's face still shows), and out in front. The
/// frame is then the head, a hood or a hat and the shoulders, whatever the stance.
const EYE: Vec3 = Vec3::new(0.2, -0.03, 0.72);
/// The middle of the face over the head's base, of the head's length.
const FACE: f32 = 0.42;
/// Where a standing figure's face is, of its height: the frame of one whose head isn't found.
const FACE_AT: f32 = 0.83;
/// How fast the frame follows the head (a share a second): the head bobs and turns, the picture
/// breathes.
const FOLLOW: f32 = 10.0;

/// Each camera in front of its subject's face, a little to its right, looking at it.
#[allow(clippy::too_many_arguments)]
fn follow(
    time: Res<Time>,
    subjects: Query<(&PortraitSubject, &GlobalTransform, &InheritedVisibility), Without<PortraitCam>>,
    globals: Query<&GlobalTransform, Without<PortraitCam>>,
    mut cams: Query<(&PortraitCam, &mut Camera, &mut Transform, &mut GlobalTransform)>,
    mut dev: ResMut<crate::dev_hooks::DevStatus>,
    mut drawn: ResMut<PortraitShown>,
    mut turn: ResMut<Turn>,
    // Where each camera looked last, and from which way.
    mut frames: Local<[Option<(Vec3, Vec3)>; VIEWS]>,
) {
    let mut at: [Option<(&GlobalTransform, Option<Head>)>; VIEWS] = [None; VIEWS];
    // Down: still shown (its last face), not drawn again until it's up.
    let mut still = [false; VIEWS];
    for (s, gt, vis) in &subjects {
        if s.view < VIEWS && s.still {
            still[s.view] = true;
        } else if s.view < VIEWS && vis.get() && (s.head.is_some() || at[s.view].is_none()) {
            // (A figure with its head found over one without: the match's over the warm-up's.)
            at[s.view] = Some((gt, s.head));
        }
    }
    // The next ally shown takes this frame's turn.
    let (mut allies, mut n) = ([0usize; VIEWS], 0);
    for v in (1..VIEWS).filter(|&v| at[v].is_some()) {
        allies[n] = v;
        n += 1;
    }
    let ally_now = (n > 0).then(|| allies[turn.0 % n]);
    turn.0 = turn.0.wrapping_add(1);
    let ease = (FOLLOW * time.delta_secs()).min(1.0);
    let place = |e: Entity| globals.get(e).ok().map(GlobalTransform::translation);
    for (cam, mut camera, mut tf, mut global) in &mut cams {
        let view = cam.0;
        let draw = at[view].is_some() && (view == 0 || Some(view) == ally_now);
        if let Some((gt, head)) = at[view].filter(|_| draw) {
            // The figure is scaled to its champion's height: the frame is in shares of it.
            let height = gt.affine().transform_vector3(Vec3::Y).length();
            // The face's middle and the way it looks (level: the camera doesn't climb over a
            // bowed head); a standing figure's, facing as its body does, where there's no head.
            let body = gt.affine().transform_vector3(Vec3::Z).with_y(0.0).normalize_or(Vec3::Z);
            let (want, facing) = head
                .and_then(|h| {
                    Some((place(h.base)?, place(h.top)?, place(h.shoulders[0])?, place(h.shoulders[1])?))
                })
                .map_or((gt.transform_point(Vec3::Y * FACE_AT), body), |(base, top, left, right)| {
                    // Up from the neck by the head's own length; out the way the chest faces
                    // (square to the line from the right shoulder to the left).
                    let face = base + Vec3::Y * base.distance(top) * FACE;
                    (face, (left - right).cross(Vec3::Y).with_y(0.0).normalize_or(body))
                });
            // Eased while it's near (the head's bob and turn); at once when it isn't (a blink, a
            // respawn, an ally's turn come round again).
            let (look, facing) = match frames[view] {
                Some((was, way)) if was.distance(want) < height * 0.25 => {
                    (was.lerp(want, ease), way.lerp(facing, ease).normalize_or(facing))
                }
                _ => (want, facing),
            };
            frames[view] = Some((look, facing));
            let right = Vec3::Y.cross(facing);
            let eye = look + (right * EYE.x + Vec3::Y * EYE.y + facing * EYE.z) * height;
            *tf = Transform::from_translation(eye).looking_at(look, Vec3::Y);
            // This frame's, since the propagation has run (the cameras have no parent).
            *global = GlobalTransform::from(*tf);
        }
        if camera.is_active != draw {
            camera.is_active = draw;
        }
    }
    let shown: [bool; VIEWS] = std::array::from_fn(|v| at[v].is_some() || still[v]);
    if drawn.0 != shown {
        drawn.0 = shown;
    }
    dev.set("portrait", shown[0]);
    dev.set("ally_portraits", shown[1..].iter().filter(|&&s| s).count());
}

/// A sitting begun or ended: the own portrait's picture bigger and on a ground, and on the screen.
fn sit(
    mut commands: Commands,
    sitting: Res<Sitting>,
    pictures: Option<Res<PortraitImage>>,
    mut images: ResMut<Assets<Image>>,
    mut cams: Query<(&PortraitCam, &mut Camera)>,
    shown: Query<Entity, With<SittingNode>>,
) {
    let Some(pictures) = pictures else { return };
    for e in &shown {
        commands.entity(e).despawn();
    }
    let side = if sitting.0 { SITTING } else { SIDE[0] };
    if let Some(mut image) = images.get_mut(&pictures.0[0])
        && image.width() != side
    {
        image.resize(bevy::render::render_resource::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        });
    }
    for (cam, mut camera) in &mut cams {
        if cam.0 == 0 {
            camera.clear_color =
                ClearColorConfig::Custom(if sitting.0 { SITTING_GROUND } else { Color::NONE });
        }
    }
    if sitting.0 {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                // The window's height a side, whatever the UI's scale: the suite's window is as
                // tall as the picture.
                width: Val::Vh(100.0),
                height: Val::Vh(100.0),
                ..default()
            },
            ImageNode::new(pictures.0[0].clone()),
            GlobalZIndex(i32::MAX),
            SittingNode,
        ));
    }
}
