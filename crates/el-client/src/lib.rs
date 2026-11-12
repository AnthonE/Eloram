//! Eloram's client (Bevy 0.19): the browser's, compiled to wasm32 (`src/main.rs`), and the
//! desktop's (`src/bin/eloram.rs`, `--features desktop`). Natively without the feature it is
//! empty, so the workspace's native tests and lints never build Bevy.
#![cfg(any(target_arch = "wasm32", feature = "desktop"))]

#[cfg(all(target_arch = "wasm32", feature = "desktop"))]
compile_error!("the desktop feature is for a native build");

/// The anim stage is a page of its own (`?animstage=`): the browser's alone.
#[cfg(target_arch = "wasm32")]
mod animstage;
pub mod app;
mod audio;
mod bench;
mod camera;
mod champions;
mod config;
mod coords;
mod dev_hooks;
mod edges;
mod figure;
mod frame;
mod front;
mod fx;
mod ground;
mod hud;
mod hud_banner;
mod hud_death;
mod hud_first;
mod hud_lobby;
mod hud_select;
mod hud_shop;
mod hud_target;
mod input;
mod look;
mod menu;
mod minions;
mod mips;
mod models;
mod net;
pub mod platform;
mod portrait;
mod scenery;
mod session;
mod skin;
mod stage;
mod trails;
mod transport;
mod view;
mod warm;
mod watch;

#[cfg(not(target_arch = "wasm32"))]
pub mod desktop;
#[cfg(not(target_arch = "wasm32"))]
mod launcher;

/// The build's name (`build.rs`: the version and the commit), for the menu's footer, the crash
/// note and a bug report.
pub fn build_name() -> &'static str {
    env!("EL_BUILD")
}

/// Whether the Elo launcher answered at the start (the browser opens its own pages, so: false).
pub fn launcher_running() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        launcher::running()
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}
