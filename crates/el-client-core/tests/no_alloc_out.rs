//! The desktop's mixing desk runs on the sound card's thread: once built, rendering and every
//! command allocate nothing and free nothing (`fx::out`).

use std::sync::Arc;

use el_alloc::CountingAlloc;
use el_client_core::fx::out::{BLOCK, Cmd, Pcm, Renderer, Route};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

#[test]
fn the_desk_allocates_nothing_once_built() {
    let mut desk = Renderer::new(48_000, 16, 4);
    let pcm = Arc::new(Pcm { rate: 44_100, channels: 2, frames: vec![0.25; 2 * 44_100].into() });
    // The game thread keeps its own handle, as `audio/native.rs` does.
    let kept = pcm.clone();
    let mut out = [0f32; 2 * BLOCK];
    let ((), n) = el_alloc::count(|| {
        desk.apply(Cmd::Install { slot: 0, pcm });
        for v in 0..20 {
            let route = match v % 3 {
                0 => Route::Front,
                1 => Route::Back,
                _ => Route::Bus(4),
            };
            let looped = v % 2 == 0;
            desk.apply(Cmd::Start {
                voice: v,
                slot: 0,
                route,
                gain: 0.5,
                pan: Some(0.3),
                lowpass: 2_000.0,
                rate: 1.1,
                looped,
            });
            desk.apply(Cmd::Gain { voice: v, target: 0.2, tau: 0.05 });
        }
        desk.apply(Cmd::Back { target: 0.5, tau: 0.06 });
        desk.apply(Cmd::Bus { k: 4, target: 0.6, tau: 0.05 });
        for k in 0..2_000 {
            if k == 500 {
                desk.apply(Cmd::Stop { voice: 2, fade: 0.01 });
            }
            desk.render(&mut out);
        }
    });
    assert_eq!(n, 0, "heap operations while mixing");
    assert!(Arc::strong_count(&kept) >= 2);
}
