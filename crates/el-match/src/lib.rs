//! Eloram's match runtime: the hot loop.
//!
//! A shard thread steps its pool of matches at 30 Hz. It talks to the network side only through
//! preallocated lock-free queues ([`queues`]): an `rtrb` input ring and output ring a seat, a
//! control queue in and an outcome queue out. It never allocates or locks after [`build`]
//! (enforced by `clippy.toml` and the `no_alloc` tests). `docs/NETCODE.md`, "Many matches a
//! process".

mod jitter;
pub mod metrics;
pub mod queues;
pub mod replicate;
mod runtime;
pub mod seat;
mod shard;

pub use jitter::{JitterBuffer, Ticked};
pub use metrics::{Hist, Metrics};
pub use queues::{
    Control, EgressEnds, InputMsg, MatchState, Outcome, SeatLease, ShardShared, build, read_packet,
};
pub use runtime::{ShardThread, spawn};
pub use shard::{Shard, ShardConfig};
