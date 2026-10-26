use bevy::prelude::*;

use crate::camera::CameraPlugin;
use crate::config::LaunchConfig;
use crate::dev_hooks::{DevHooksPlugin, InboxPlugin};
use crate::edges::EdgesPlugin;
use crate::ground::GroundPlugin;
use crate::hud::HudPlugin;
use crate::input::InputPlugin;
use crate::look::LookPlugin;
use crate::mips::MipsPlugin;
use crate::net::{LaunchConfigRes, MapRes, NetPlugin};
use crate::scenery::SceneryPlugin;
use crate::view::ViewPlugin;
use crate::watch::WatchPlugin;

pub fn run() {
    console_error_panic_hook::set_once();
    let cfg = LaunchConfig::from_window();
    // The Causeway's grids: what the prediction steps on, the fog's sight, the floor drawn.
    let map = el_sim::map::Map::leaked();
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Eloram".into(),
                        canvas: Some("#el".into()),
                        fit_canvas_to_parent: true,
                        prevent_default_event_handling: true,
                        ..default()
                    }),
                    ..default()
                })
                // The art beside the page (`web/assets/`, copied into `web/dist/assets/`), with no
                // `.meta` files to ask for.
                .set(AssetPlugin { meta_check: bevy::asset::AssetMetaCheck::Never, ..default() }),
        )
        .insert_resource(bevy::winit::WinitSettings::continuous())
        .insert_resource(LaunchConfigRes(cfg))
        .insert_resource(MapRes(map))
        .add_plugins((
            DevHooksPlugin,
            InboxPlugin,
            NetPlugin,
            InputPlugin,
            CameraPlugin,
            LookPlugin,
            MipsPlugin,
            GroundPlugin,
            EdgesPlugin,
            SceneryPlugin,
            ViewPlugin,
            HudPlugin,
            WatchPlugin,
        ))
        .run();
}
