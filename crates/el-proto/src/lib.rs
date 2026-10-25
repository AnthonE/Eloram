//! Eloram wire protocol.
//!
//! - `#![no_std]` with **no `alloc`**: nothing here can allocate, by construction. Every encoder
//!   writes into a caller-provided buffer and every decoder reads from a borrowed slice.
//! - Datagrams (unreliable, one QUIC packet each, never fragmented, at most [`MAX_DATAGRAM`]):
//!   client → server input, server → client snapshots.
//! - Control stream (reliable, length-prefixed frames): [`control`] handshake, sign-in and lobby
//!   messages.
//!
//! `docs/NETCODE.md` says why the wire is shaped this way.

#![no_std]
#![forbid(unsafe_code)]

pub mod ack;
pub mod auth;
pub mod bits;
pub mod control;
pub mod event;
pub mod input;
pub mod order;
pub mod quant;
pub mod snapshot;
pub mod unit;

pub use bits::{BitReader, BitWriter};
pub use input::{InputCmd, InputPacket, StampedOrder};
pub use order::{Aim, Order, UnitRef};

/// Bumped on any incompatible wire change; the handshake rejects mismatches.
pub const PROTOCOL_VERSION: u16 = 1;

/// Upper bound for every datagram we send. 1200 bytes is the smallest UDP payload QUIC guarantees;
/// the QUIC short header, AEAD tag and HTTP/3 datagram prefix need ~30–40 of those.
pub const MAX_DATAGRAM: usize = 1100;

/// The match's ticks a second (League's server rate, at a fixed step).
pub const TICK_HZ: u32 = 30;

/// The map's extent, in game units: positions run over `[0, MAP_W] × [0, MAP_H]`, the lane along
/// +x (`docs/DESIGN.md`, "The map").
pub const MAP_W: f32 = 15_360.0;
pub const MAP_H: f32 = 3_840.0;

/// First 4 bits of every datagram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketKind {
    Input = 1,
    Snapshot = 2,
}

pub const PACKET_KIND_BITS: u32 = 4;

/// Peeks at a datagram's kind without decoding it.
pub fn packet_kind(bytes: &[u8]) -> Option<PacketKind> {
    match bytes.first()? & 0x0f {
        1 => Some(PacketKind::Input),
        2 => Some(PacketKind::Snapshot),
        _ => None,
    }
}

/// Why a decode failed. Decoders never panic on hostile input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Ran past the end of the buffer.
    Truncated,
    /// Wrong packet kind or message tag.
    WrongKind,
    /// A field held a value outside its domain.
    Invalid,
    /// A delta record whose baseline the reader doesn't hold.
    NoBaseline,
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            DecodeError::Truncated => "truncated packet",
            DecodeError::WrongKind => "wrong packet kind",
            DecodeError::Invalid => "invalid field",
            DecodeError::NoBaseline => "a delta without its baseline",
        };
        f.write_str(s)
    }
}
