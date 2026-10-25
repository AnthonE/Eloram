//! Eloram's match runtime: the hot loop.
//!
//! A shard thread steps its pool of matches at 30 Hz. It talks to the network side only through
//! preallocated lock-free queues, and never allocates or locks after start-up (enforced by
//! `clippy.toml` and the `no_alloc` tests). `docs/NETCODE.md`, "Many matches a process".

mod jitter;
pub mod metrics;

pub use jitter::{JitterBuffer, Ticked};
pub use metrics::{Hist, Metrics};
