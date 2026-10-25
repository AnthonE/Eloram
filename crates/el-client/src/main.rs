//! Eloram's browser client (Bevy 0.19 → wasm32).

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod camera;
#[cfg(target_arch = "wasm32")]
mod config;
#[cfg(target_arch = "wasm32")]
mod coords;
#[cfg(target_arch = "wasm32")]
mod dev_hooks;
#[cfg(target_arch = "wasm32")]
mod hud;
#[cfg(target_arch = "wasm32")]
mod input;
#[cfg(target_arch = "wasm32")]
mod models;
#[cfg(target_arch = "wasm32")]
mod net;
#[cfg(target_arch = "wasm32")]
mod session;
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
