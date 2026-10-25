use bevy::prelude::*;

use crate::camera::CameraPlugin;
use crate::config::LaunchConfig;
use crate::dev_hooks::{DevHooksPlugin, InboxPlugin};
use crate::hud::HudPlugin;
use crate::input::InputPlugin;
use crate::net::{LaunchConfigRes, MapRes, NetPlugin};
use crate::view::ViewPlugin;

pub fn run() {
    console_error_panic_hook::set_once();
    let cfg = LaunchConfig::from_window();
    // The Causeway's grids: what the prediction steps on, the fog's sight, the floor drawn.
    let map = el_sim::map::Map::leaked();
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
        .insert_resource(MapRes(map))
        .insert_resource(ClearColor(Color::srgb(0.02, 0.03, 0.05)))
        .add_plugins((
            DevHooksPlugin,
            InboxPlugin,
            NetPlugin,
            InputPlugin,
            CameraPlugin,
            ViewPlugin,
            HudPlugin,
        ))
        .run();
}
