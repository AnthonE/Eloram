//! What happened this tick, for the clients to draw and hear. Each event names the units it is
//! about, so replication can give each team only the ones it may know (`docs/NETCODE.md`, "Fog
//! is the server's": every event has a team rule).

use glam::Vec2;

/// Most events a tick keeps; past it they're counted and dropped.
pub const MAX_EVENTS: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// A champion began a cast: its slot, and where it aimed.
    Cast {
        unit: u16,
        slot: u8,
        at: Vec2,
    },
    /// A unit released an attack at another (a swing lands at once; a shot is a missile).
    Attack {
        unit: u16,
        target: u16,
    },
    /// A missile left (its state is in the match's table).
    MissileSpawn {
        missile: u16,
    },
    /// A missile ended: hit something, or ran out of range.
    MissileEnd {
        missile: u16,
        at: Vec2,
    },
    Damage {
        target: u16,
        source: u16,
        amount: f32,
    },
    Heal {
        target: u16,
        amount: f32,
    },
    Death {
        unit: u16,
        killer: u16,
    },
    Respawn {
        unit: u16,
    },
    LevelUp {
        unit: u16,
        level: u8,
    },
    /// A unit was somewhere else at once (a blink, a pull).
    Blink {
        unit: u16,
        from: Vec2,
        to: Vec2,
    },
    /// An area's effect landed (a delayed blast, a zone's pulse).
    Area {
        at: Vec2,
        radius: f32,
        team: u8,
    },
    RelicTaken {
        pad: u8,
        unit: u16,
    },
    RelicBeam {
        pad: u8,
    },
    Augment {
        unit: u16,
        augment: u8,
    },
    Outcome {
        winner: u8,
    },
}

impl Default for Event {
    fn default() -> Self {
        Event::Outcome { winner: 0 }
    }
}
