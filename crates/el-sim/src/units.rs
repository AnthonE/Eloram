//! Units: champions, minions and structures, in one fixed table.
//!
//! Slots are fixed by kind: the ten champions are slots 0–9 (a seat's champion is its seat), the
//! structures 10–23 (seven a team), minions the rest, handed out lowest first and given back when
//! they die. A [`Handle`] names a slot and the life in it, so an order aimed at a minion that died
//! is aimed at nothing even when another took its slot.

use glam::Vec2;

use crate::handle::Handle;
use crate::path::Path;

pub const MAX_UNITS: usize = 256;
pub const SEATS: usize = 10;
pub const TEAM_SIZE: usize = 5;
pub const FIRST_STRUCTURE: usize = SEATS;
/// Outer turret, inhibitor turret, inhibitor, two nexus turrets, nexus, fountain.
pub const STRUCTURES_PER_TEAM: usize = 7;
pub const FIRST_MINION: usize = FIRST_STRUCTURE + 2 * STRUCTURES_PER_TEAM;

/// The team a seat plays for: seats 0–4 Dawn, 5–9 Dusk.
#[inline]
pub fn team_of_seat(seat: usize) -> u8 {
    (seat / TEAM_SIZE) as u8
}

/// A structure's place in its team's seven.
pub mod structure {
    pub const OUTER: usize = 0;
    pub const INHIB_TURRET: usize = 1;
    pub const INHIBITOR: usize = 2;
    pub const NEXUS_TURRET_A: usize = 3;
    pub const NEXUS_TURRET_B: usize = 4;
    pub const NEXUS: usize = 5;
    pub const FOUNTAIN: usize = 6;
}

/// The slot of a team's structure `k` ([`structure`]).
#[inline]
pub fn structure_slot(team: u8, k: usize) -> usize {
    FIRST_STRUCTURE + usize::from(team) * STRUCTURES_PER_TEAM + k
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    #[default]
    Champion = 0,
    Melee = 1,
    Caster = 2,
    Siege = 3,
    Super = 4,
    Turret = 5,
    Inhibitor = 6,
    Nexus = 7,
    Fountain = 8,
}

impl Kind {
    pub const COUNT: usize = 9;

    pub fn is_minion(self) -> bool {
        matches!(self, Kind::Melee | Kind::Caster | Kind::Siege | Kind::Super)
    }
    pub fn is_structure(self) -> bool {
        matches!(self, Kind::Turret | Kind::Inhibitor | Kind::Nexus | Kind::Fountain)
    }
    /// Shoots at what comes near.
    pub fn shoots(self) -> bool {
        matches!(self, Kind::Turret | Kind::Fountain)
    }
    pub fn from_u8(v: u8) -> Kind {
        match v {
            1 => Kind::Melee,
            2 => Kind::Caster,
            3 => Kind::Siege,
            4 => Kind::Super,
            5 => Kind::Turret,
            6 => Kind::Inhibitor,
            7 => Kind::Nexus,
            8 => Kind::Fountain,
            _ => Kind::Champion,
        }
    }
}

/// What a unit is, in numbers. Champions' are worked out each tick from their level, items,
/// augments and buffs; everyone else's are set when they spawn.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub max_hp: f32,
    /// Health back a second.
    pub hp_regen: f32,
    pub max_mana: f32,
    pub mana_regen: f32,
    pub ad: f32,
    pub ap: f32,
    pub armor: f32,
    pub mr: f32,
    /// Attacks a second.
    pub attack_speed: f32,
    /// Edge to edge, units.
    pub range: f32,
    /// Units a second.
    pub move_speed: f32,
    /// Ability haste: cooldowns are divided by `1 + haste / 100`.
    pub haste: f32,
    /// The share of auto-attack damage healed back.
    pub lifesteal: f32,
}

/// Crowd control on a unit: each the tick it lasts until.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cc {
    pub stun: u32,
    pub root: u32,
    pub silence: u32,
    pub airborne: u32,
    pub slow_until: u32,
    /// The strongest slow on it, 0 to 0.9.
    pub slow: f32,
    /// Immune to crowd control until (Combo Breaker).
    pub immune: u32,
}

impl Cc {
    /// Can't act at all.
    pub fn disabled(&self, t: u32) -> bool {
        self.stun > t || self.airborne > t
    }
    /// Can't move (but may attack and cast).
    pub fn rooted(&self, t: u32) -> bool {
        self.disabled(t) || self.root > t
    }
    pub fn silenced(&self, t: u32) -> bool {
        self.disabled(t) || self.silence > t
    }
    pub fn slow(&self, t: u32) -> f32 {
        if self.slow_until > t { self.slow } else { 0.0 }
    }
}

/// A timed change to a unit's stats ([`BuffKind`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Buff {
    pub kind: u8,
    pub until: u32,
    pub value: f32,
}

pub const MAX_BUFFS: usize = 8;

/// What a buff changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BuffKind {
    None = 0,
    /// Move speed, as a share.
    MoveSpeed = 1,
    AttackSpeed = 2,
    AttackDamage = 3,
    AbilityPower = 4,
    Armor = 5,
    MagicResist = 6,
    /// Marked by a snowball: its thrower may dash to it.
    Marked = 7,
}

impl BuffKind {
    pub fn from_u8(v: u8) -> BuffKind {
        match v {
            1 => BuffKind::MoveSpeed,
            2 => BuffKind::AttackSpeed,
            3 => BuffKind::AttackDamage,
            4 => BuffKind::AbilityPower,
            5 => BuffKind::Armor,
            6 => BuffKind::MagicResist,
            7 => BuffKind::Marked,
            _ => BuffKind::None,
        }
    }
}

/// One unit's whole state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Unit {
    pub alive: bool,
    pub kind: Kind,
    pub team: u8,
    /// Bumped every time the slot is reused.
    pub generation: u16,
    pub pos: Vec2,
    /// How far it moved last tick (what a client extrapolates by).
    pub moved: Vec2,
    pub facing: f32,
    pub radius: f32,
    pub hp: f32,
    pub mana: f32,
    pub shield: f32,
    pub shield_until: u32,
    pub stats: Stats,
    pub cc: Cc,
    pub buffs: [Buff; MAX_BUFFS],
    /// Who it's fighting.
    pub target: Handle,
    /// The tick it may start its next attack.
    pub attack_ready: u32,
    /// Ticks left in the attack it's winding up (0: none).
    pub windup: u16,
    pub path: Path,
    /// The tick it may look for a new path.
    pub repath_at: u32,
    /// The seat whose champion last hurt it (+1; 0: none), and when.
    pub last_hit_by: u8,
    pub last_hit_at: u32,
    /// When each seat's champion last hurt it (assists).
    pub hurt_by: [u32; SEATS],
    /// When it last attacked an enemy champion (what turrets and minions answer).
    pub hit_champion_at: u32,
    /// A turret's consecutive shots at the same champion.
    pub streak: u8,
    /// Seen by its enemies until (revealed).
    pub revealed: u32,
    pub spawned: u32,
}

impl Default for Unit {
    fn default() -> Self {
        Self {
            alive: false,
            kind: Kind::Champion,
            team: 0,
            generation: 0,
            pos: Vec2::ZERO,
            moved: Vec2::ZERO,
            facing: 0.0,
            radius: 0.0,
            hp: 0.0,
            mana: 0.0,
            shield: 0.0,
            shield_until: 0,
            stats: Stats::default(),
            cc: Cc::default(),
            buffs: [Buff::default(); MAX_BUFFS],
            target: Handle::NONE,
            attack_ready: 0,
            windup: 0,
            path: Path::default(),
            repath_at: 0,
            last_hit_by: 0,
            last_hit_at: 0,
            hurt_by: [0; SEATS],
            hit_champion_at: 0,
            streak: 0,
            revealed: 0,
            spawned: 0,
        }
    }
}

impl Unit {
    pub fn handle(&self, idx: usize) -> Handle {
        Handle { idx: idx as u16, generation: self.generation }
    }

    /// Adds `kind` (replacing one of its kind, else into a free place) until `until`.
    pub fn add_buff(&mut self, kind: BuffKind, until: u32, value: f32, now: u32) {
        let k = kind as u8;
        let slot = self
            .buffs
            .iter()
            .position(|b| b.kind == k)
            .or_else(|| self.buffs.iter().position(|b| b.kind == 0 || b.until <= now));
        if let Some(i) = slot {
            self.buffs[i] = Buff { kind: k, until, value };
        }
    }

    /// The summed value of `kind`'s buffs in force at `now`.
    pub fn buff(&self, kind: BuffKind, now: u32) -> f32 {
        let k = kind as u8;
        self.buffs.iter().filter(|b| b.kind == k && b.until > now).map(|b| b.value).sum()
    }

    pub fn has_buff(&self, kind: BuffKind, now: u32) -> bool {
        let k = kind as u8;
        self.buffs.iter().any(|b| b.kind == k && b.until > now)
    }
}
