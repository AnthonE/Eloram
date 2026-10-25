//! Eloram's match simulation.
//!
//! - `no_std`: the server (native) and the browser (wasm32) step it alike; it is a closed function
//!   of its seed and its inputs (`docs/NETCODE.md`, "The law").
//! - Storage is sized at construction and never grows: [`storage`] is the only place that
//!   allocates, and the `no_alloc` test counts a whole match's ticks at zero heap operations.
//! - Every transcendental goes through [`math`] (libm).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod handle;
pub mod hash;
pub mod map;
pub mod math;
pub mod path;
pub mod storage;

pub use el_proto::{MAP_H, MAP_W, TICK_HZ};

/// Seconds in a tick.
pub const DT: f32 = 1.0 / TICK_HZ as f32;

/// Ticks in `secs` seconds (rounded to the nearest).
pub const fn ticks(secs: f32) -> u32 {
    (secs * TICK_HZ as f32 + 0.5) as u32
}
