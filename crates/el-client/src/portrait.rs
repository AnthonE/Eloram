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
use bevy::camera::visibility::RenderLayers;
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
}

#[derive(Component)]
struct PortraitCam(usize);

/// Whose turn it is among the allies' portraits.
#[derive(Resource, Default)]
struct Turn(usize);

pub struct PortraitPlugin;

impl Plugin for PortraitPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PortraitShown>()
            .init_resource::<Turn>()
            .add_systems(Startup, setup)
            .add_systems(PostUpdate, follow.before(TransformSystems::Propagate));
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

/// Each camera in front of its subject's face, a little to its right, looking at its chest.
fn follow(
    subjects: Query<(&PortraitSubject, &GlobalTransform, &InheritedVisibility)>,
    mut cams: Query<(&PortraitCam, &mut Camera, &mut Transform)>,
    mut dev: ResMut<crate::dev_hooks::DevStatus>,
    mut drawn: ResMut<PortraitShown>,
    mut turn: ResMut<Turn>,
) {
    let mut at: [Option<&GlobalTransform>; VIEWS] = [None; VIEWS];
    // Down: still shown (its last face), not drawn again until it's up.
    let mut still = [false; VIEWS];
    for (s, gt, vis) in &subjects {
        if s.view < VIEWS && s.still {
            still[s.view] = true;
        } else if s.view < VIEWS && vis.get() {
            at[s.view] = Some(gt);
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
    for (cam, mut camera, mut tf) in &mut cams {
        let view = cam.0;
        let draw = at[view].is_some() && (view == 0 || Some(view) == ally_now);
        if let Some(gt) = at[view].filter(|_| draw) {
            // The figure is scaled to its champion's height: these are shares of it.
            let eye = gt.transform_point(Vec3::new(0.2, 0.86, 0.72));
            let look = gt.transform_point(Vec3::new(0.0, 0.8, 0.0));
            *tf = Transform::from_translation(eye).looking_at(look, Vec3::Y);
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
