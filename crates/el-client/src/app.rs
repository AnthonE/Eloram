use bevy::prelude::*;

use crate::audio::AudioPlugin;
use crate::bench::BenchPlugin;
use crate::camera::CameraPlugin;
use crate::champions::ChampionsPlugin;
use crate::config::{LaunchConfig, Prefs};
use crate::dev_hooks::{DevHooksPlugin, InboxPlugin};
use crate::edges::EdgesPlugin;
use crate::front::FrontPlugin;
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

/// The browser's start: the page's config (`window.EL_CONFIG`).
#[cfg(target_arch = "wasm32")]
pub fn run_web() {
    console_error_panic_hook::set_once();
    run(LaunchConfig::from_window());
}

/// Where the art is: beside the page in the browser (`web/assets/`, copied into
/// `web/dist/assets/`); on the desktop, `assets/` beside the program (the depot), or the repo's
/// `web/assets` from a build in the checkout. Absolute, so no environment variable is set.
pub fn asset_root() -> std::path::PathBuf {
    #[cfg(target_arch = "wasm32")]
    {
        std::path::PathBuf::from("assets")
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(std::path::Path::to_path_buf));
        let candidates = [
            exe_dir.as_ref().map(|d| d.join("assets")),
            std::env::current_dir().ok().map(|d| d.join("web/assets")),
            exe_dir.as_ref().map(|d| d.join("../../web/assets")),
            exe_dir.as_ref().map(|d| d.join("../../../web/assets")),
            Some(std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/assets"))),
        ];
        candidates
            .into_iter()
            .flatten()
            .find(|d| d.join("models").is_dir())
            .and_then(|d| d.canonicalize().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("assets"))
    }
}

/// The window: the page's canvas in the browser; on the desktop a window of its own, its title,
/// its name (X11's class, Wayland's app id) and its mode as last time.
fn window(cfg: &LaunchConfig) -> Window {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = cfg;
        Window {
            title: "Eloram".into(),
            canvas: Some("#el".into()),
            fit_canvas_to_parent: true,
            prevent_default_event_handling: true,
            ..default()
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let full = cfg
            .fullscreen
            .unwrap_or_else(|| crate::platform::kv_get("el.fullscreen").is_some_and(|v| v == "1"));
        Window {
            title: "Eloram".into(),
            name: Some("eloram".into()),
            resolution: bevy::window::WindowResolution::new(1600, 900),
            mode: if full {
                bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current)
            } else {
                bevy::window::WindowMode::Windowed
            },
            present_mode: if crate::platform::kv_get("el.vsync").is_some_and(|v| v == "0") {
                bevy::window::PresentMode::AutoNoVsync
            } else {
                bevy::window::PresentMode::AutoVsync
            },
            ..default()
        }
    }
}

pub fn run(cfg: LaunchConfig) {
    // The Causeway's grids: what the prediction steps on, the fog's sight, the floor drawn.
    let map = el_sim::map::Map::leaked();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = crate::dev_hooks::STATUS_FILE.set(cfg.status_file.clone());
        let _ = crate::dev_hooks::INBOX_FILE.set(cfg.inbox_file.clone());
    }
    if !cfg.http_base.is_empty() {
        crate::config::set_http_base(&cfg.http_base);
    }
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin { primary_window: Some(window(&cfg)), ..default() })
            // The art, with no `.meta` files to ask for.
            .set(AssetPlugin {
                file_path: asset_root().to_string_lossy().into_owned(),
                meta_check: bevy::asset::AssetMetaCheck::Never,
                ..default()
            }),
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
    .add_plugins((
        SelectPlugin,
        LobbyPlugin,
        MenuPlugin,
        FirstPlugin,
        StagePlugin,
        BenchPlugin,
        FrontPlugin,
        crate::front::report::ReportPlugin,
    ));
    #[cfg(not(target_arch = "wasm32"))]
    app.add_plugins(crate::desktop::DesktopPlugin);
    app.run();
}
