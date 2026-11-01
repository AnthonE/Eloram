//! The player's portrait: their own champion's model, drawn live from the chest up into a small
//! picture the HUD shows (`hud.rs`), as League's portrait shows the champion. A camera of its own
//! sees only [`LAYER`], which the own champion's figure is on as well as the scene's (`champions.rs`
//! puts it there once its model is in), with a key light of its own that turns with it; it stands
//! in front of the figure, a little to one side, and turns as it turns. It draws only while the
//! figure is shown.

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

/// The render layer the portrait's camera sees.
pub const LAYER: usize = 1;
/// The picture's side, in pixels: about twice what the HUD shows, for a clean edge.
const SIDE: u32 = 136;

/// The picture the portrait's camera draws into.
#[derive(Resource)]
pub struct PortraitImage(pub Handle<Image>);

/// Whether the portrait is being drawn (the HUD hides the initials under it while it is).
#[derive(Resource, Default)]
pub struct PortraitShown(pub bool);

/// The figure the portrait shows: the own champion's.
#[derive(Component)]
pub struct PortraitSubject;

#[derive(Component)]
struct PortraitCam;

pub struct PortraitPlugin;

impl Plugin for PortraitPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PortraitShown>()
            .add_systems(Startup, setup)
            .add_systems(PostUpdate, follow.before(TransformSystems::Propagate));
    }
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // sRGB itself, with no second view of it: WebGL2 has no view formats.
    let image = images.add(Image::new_target_texture(SIDE, SIDE, TextureFormat::Rgba8UnormSrgb, None));
    commands.insert_resource(PortraitImage(image.clone()));
    let layer = RenderLayers::layer(LAYER);
    commands
        .spawn((
            Camera3d::default(),
            Camera {
                // Before the main view, and clear where there's no champion: the HUD's well shows.
                order: -1,
                is_active: false,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(image.into()),
            Projection::Perspective(PerspectiveProjection { fov: 26f32.to_radians(), ..default() }),
            // Brighter than the night's, so the face reads.
            AmbientLight { color: Color::srgb(0.75, 0.82, 1.0), brightness: 420.0, ..default() },
            Msaa::Sample4,
            layer.clone(),
            PortraitCam,
            Transform::default(),
        ))
        .with_children(|cam| {
            // A warm key from above the camera's left, turning with it.
            cam.spawn((
                DirectionalLight { illuminance: 4_200.0, color: Color::srgb(1.0, 0.92, 0.82), ..default() },
                Transform::from_xyz(-1.0, 1.4, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
                layer,
            ));
        });
}

/// The camera in front of the subject's face, a little to its right, looking at its chest.
fn follow(
    subject: Query<(&GlobalTransform, &InheritedVisibility), With<PortraitSubject>>,
    mut cam: Single<(&mut Camera, &mut Transform), With<PortraitCam>>,
    mut dev: ResMut<crate::dev_hooks::DevStatus>,
    mut drawn: ResMut<PortraitShown>,
) {
    let (camera, tf) = &mut *cam;
    // The figure is scaled to its champion's height: these are shares of it.
    let shown = subject.iter().find(|(_, v)| v.get());
    if let Some((at, _)) = shown {
        let eye = at.transform_point(Vec3::new(0.2, 0.86, 0.72));
        let look = at.transform_point(Vec3::new(0.0, 0.8, 0.0));
        **tf = Transform::from_translation(eye).looking_at(look, Vec3::Y);
    }
    if camera.is_active != shown.is_some() {
        camera.is_active = shown.is_some();
    }
    if drawn.0 != shown.is_some() {
        drawn.0 = shown.is_some();
    }
    dev.set("portrait", shown.is_some());
}
