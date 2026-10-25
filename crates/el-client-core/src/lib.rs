//! Eloram's client without its transport: the clock, the snapshots taken into a model of the match
//! as the team sees it, the own champion predicted with the server's own step, everyone else
//! interpolated, and the orders and commands sent. The browser client and the agents both run it;
//! it never reads a clock or touches a socket, so all of it is tested natively.

pub mod baselines;
pub mod client;
pub mod clock;
pub mod inputs;
pub mod interp;
pub mod lobby;
pub mod missiles;
pub mod orders;

pub use client::{Cause, Client, Correction, Stats, Taken};
pub use clock::Clock;
