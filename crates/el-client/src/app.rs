use bevy::prelude::*;

use crate::audio::AudioPlugin;
use crate::bench::BenchPlugin;
use crate::camera::CameraPlugin;
use crate::champions::ChampionsPlugin;
use crate::config::{LaunchConfig, Prefs};
use crate::dev_hooks::{DevHooksPlugin, InboxPlugin};
use crate::edges::EdgesPlugin;
use crate::fx::FxPlugin;
use crate::ground::GroundPlugin;
use crate::hud::HudPlugin;
use crate::hud_death::DeathPlugin;
use crate::hud_first::FirstPlugin;
use crate::hud_lobby::LobbyPlugin;
use crate::hud_select::SelectPlugin;
use crate::hud_target::TargetPlugin;
use crate::input::InputPlugin;
use crate::look::LookPlugin;
use crate::menu::MenuPlugin;
use crate::minions::MinionsPlugin;
use crate::mips::MipsPlugin;
use crate::net::{LaunchConfigRes, MapRes, NetPlugin};
use crate::portrait::PortraitPlugin;
use crate::scenery::SceneryPlugin;
use crate::stage::StagePlugin;
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
        .insert_resource(Prefs::of(&cfg))
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
            FxPlugin,
            AudioPlugin,
            HudPlugin,
            WatchPlugin,
        ))
        // Past the sixteen a tuple of plugins takes.
        .add_plugins((ChampionsPlugin, MinionsPlugin, PortraitPlugin, DeathPlugin, TargetPlugin))
        .add_plugins((SelectPlugin, LobbyPlugin, MenuPlugin, FirstPlugin, StagePlugin, BenchPlugin))
        .run();
}
