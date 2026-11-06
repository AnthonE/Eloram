//! Eloram's browser client (Bevy 0.19 → wasm32).

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod audio;
#[cfg(target_arch = "wasm32")]
mod camera;
#[cfg(target_arch = "wasm32")]
mod champions;
#[cfg(target_arch = "wasm32")]
mod config;
#[cfg(target_arch = "wasm32")]
mod coords;
#[cfg(target_arch = "wasm32")]
mod dev_hooks;
#[cfg(target_arch = "wasm32")]
mod edges;
#[cfg(target_arch = "wasm32")]
mod fx;
#[cfg(target_arch = "wasm32")]
mod ground;
#[cfg(target_arch = "wasm32")]
mod hud;
#[cfg(target_arch = "wasm32")]
mod hud_death;
#[cfg(target_arch = "wasm32")]
mod hud_first;
#[cfg(target_arch = "wasm32")]
mod hud_lobby;
#[cfg(target_arch = "wasm32")]
mod hud_select;
#[cfg(target_arch = "wasm32")]
mod hud_target;
#[cfg(target_arch = "wasm32")]
mod input;
#[cfg(target_arch = "wasm32")]
mod look;
#[cfg(target_arch = "wasm32")]
mod menu;
#[cfg(target_arch = "wasm32")]
mod minions;
#[cfg(target_arch = "wasm32")]
mod mips;
#[cfg(target_arch = "wasm32")]
mod models;
#[cfg(target_arch = "wasm32")]
mod net;
#[cfg(target_arch = "wasm32")]
mod portrait;
#[cfg(target_arch = "wasm32")]
mod scenery;
#[cfg(target_arch = "wasm32")]
mod session;
#[cfg(target_arch = "wasm32")]
mod stage;
#[cfg(target_arch = "wasm32")]
mod transport;
#[cfg(target_arch = "wasm32")]
mod view;
#[cfg(target_arch = "wasm32")]
mod watch;

#[cfg(target_arch = "wasm32")]
fn main() {
    app::run();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("el-client runs in the browser: build it with scripts/build-web.sh");
}
