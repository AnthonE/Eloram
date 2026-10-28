//! The rules' numbers: ARAM's, with Mayhem's on top (`docs/DESIGN.md`).

use crate::units::{Kind, Stats};

// ---- a match -------------------------------------------------------------------------------

pub const START_LEVEL: u8 = 3;
pub const MAX_LEVEL: u8 = 18;
pub const START_GOLD: f32 = 1_400.0;
/// Passive gold, from this second on, at this rate.
pub const PASSIVE_GOLD_FROM: f32 = 60.0;
pub const PASSIVE_GOLD_PER_SEC: f32 = 4.0;
/// The base gates hold champions on their side until this second.
pub const GATES_OPEN: f32 = 15.0;
/// Healing from every source (regeneration apart) is multiplied by this.
pub const HEALING: f32 = 0.5;
/// An enemy champion's damage from farther than this is reduced, to [`FALLOFF_MIN`] at
/// [`FALLOFF_TO`].
pub const FALLOFF_FROM: f32 = 1_000.0;
pub const FALLOFF_TO: f32 = 2_000.0;
pub const FALLOFF_MAX: f32 = 0.85;
pub const FALLOFF_MIN: f32 = 0.70;
/// Who counts for an assist: champions who hurt the victim this recently.
pub const ASSIST_SECS: f32 = 10.0;
/// A vote to surrender may open from this second; it stays open this long; it passes with this
/// share of the team's players (four of five; bots don't vote); after one fails, the team waits
/// this long for the next.
pub const SURRENDER_FROM: f32 = 8.0 * 60.0;
pub const SURRENDER_VOTE: f32 = 60.0;
pub const SURRENDER_SHARE: (u32, u32) = (4, 5);
pub const SURRENDER_AGAIN: f32 = 3.0 * 60.0;
/// Pings: this many at once, then one every this many seconds.
pub const PING_BURST: u32 = 5;
pub const PING_EVERY: f32 = 1.0;

/// Seconds dead at `level`: 11 at level 3, to 40 at 18.
pub fn death_secs(level: u8) -> f32 {
    let l = f32::from(level.clamp(START_LEVEL, MAX_LEVEL) - START_LEVEL);
    11.0 + l * (29.0 / 15.0)
}

/// Experience from `level` to the next.
pub fn xp_to_next(level: u8) -> f32 {
    80.0 + 75.0 * f32::from(level)
}

/// Shared among the killer's allied champions this near: each gets the larger of an even share
/// and [`XP_SHARE_MIN`] of the whole (ARAM's five are always together).
pub const XP_RADIUS: f32 = 1_600.0;
pub const XP_SHARE_MIN: f32 = 0.7;

/// Experience a kill gives: by what died.
pub fn xp_for(kind: Kind, victim_level: u8) -> f32 {
    match kind {
        Kind::Champion => 100.0 + 40.0 * f32::from(victim_level),
        Kind::Melee => 61.0,
        Kind::Caster => 30.0,
        Kind::Siege => 92.0,
        Kind::Super => 97.0,
        _ => 0.0,
    }
}

/// Gold the last hit gives.
pub fn gold_for(kind: Kind) -> f32 {
    match kind {
        Kind::Champion => 300.0,
        Kind::Melee => 21.0,
        Kind::Caster => 14.0,
        Kind::Siege => 60.0,
        Kind::Super => 40.0,
        _ => 0.0,
    }
}

/// Gold an assist gives, shared among the assists.
pub const ASSIST_GOLD: f32 = 150.0;
/// Gold each champion of a team gets when an enemy turret falls, and an inhibitor.
pub const TURRET_GOLD: f32 = 150.0;
pub const INHIBITOR_GOLD: f32 = 50.0;

// ---- minions -------------------------------------------------------------------------------

/// The first wave leaves at this second; then one every [`WAVE_SECS_START`], shrinking by
/// [`WAVE_SECS_STEP`] a wave to [`WAVE_SECS_MIN`].
pub const FIRST_WAVE: f32 = 50.0;
pub const WAVE_SECS_START: f32 = 25.0;
pub const WAVE_SECS_STEP: f32 = 0.75;
pub const WAVE_SECS_MIN: f32 = 13.0;
/// How far a minion looks for something to fight.
pub const MINION_ACQUIRE: f32 = 700.0;
/// Minions grow stronger every this many seconds, by this share.
pub const MINION_GROWTH_SECS: f32 = 90.0;
pub const MINION_GROWTH: f32 = 0.04;

pub struct MinionDef {
    pub stats: Stats,
    pub radius: f32,
    /// 0: melee.
    pub missile_speed: f32,
}

const fn minion(
    max_hp: f32,
    ad: f32,
    armor: f32,
    attack_speed: f32,
    range: f32,
    radius: f32,
    missile: f32,
) -> MinionDef {
    MinionDef {
        stats: Stats {
            max_hp,
            hp_regen: 0.0,
            max_mana: 0.0,
            mana_regen: 0.0,
            ad,
            ap: 0.0,
            armor,
            mr: 0.0,
            attack_speed,
            range,
            move_speed: 325.0,
            haste: 0.0,
            lifesteal: 0.0,
        },
        radius,
        missile_speed: missile,
    }
}

/// A minion's windup: this share of the time between its attacks.
pub const MINION_WINDUP: f32 = 0.3;

pub fn minion_def(kind: Kind) -> MinionDef {
    match kind {
        Kind::Caster => minion(296.0, 23.0, 0.0, 0.67, 550.0, 48.0, 650.0),
        Kind::Siege => minion(912.0, 41.0, 0.0, 0.5, 300.0, 65.0, 1_200.0),
        Kind::Super => minion(1_600.0, 230.0, 30.0, 0.85, 170.0, 65.0, 0.0),
        _ => minion(477.0, 12.0, 0.0, 0.8, 110.0, 48.0, 0.0),
    }
}

// ---- structures ----------------------------------------------------------------------------

pub const TURRET_RANGE: f32 = 775.0;
pub const TURRET_ATTACK_SPEED: f32 = 0.83;
pub const TURRET_MISSILE_SPEED: f32 = 1_200.0;
/// A turret's shot at a champion, and how much more each consecutive one does, to a cap.
pub const TURRET_DAMAGE: f32 = 152.0;
pub const TURRET_STREAK: f32 = 0.4;
pub const TURRET_STREAK_MAX: u8 = 3;
/// A turret's shot at a minion, as a share of its maximum health.
pub fn turret_share(kind: Kind) -> f32 {
    match kind {
        Kind::Melee => 0.45,
        Kind::Caster => 0.7,
        Kind::Siege => 0.14,
        Kind::Super => 0.07,
        _ => 0.0,
    }
}
pub const OUTER_TURRET_HP: f32 = 4_500.0;
pub const INHIB_TURRET_HP: f32 = 4_500.0;
/// Mayhem's: the nexus turrets and the nexus.
pub const NEXUS_TURRET_HP: f32 = 3_000.0;
pub const NEXUS_HP: f32 = 3_000.0;
pub const INHIBITOR_HP: f32 = 3_000.0;
pub const INHIBITOR_RESPAWN: f32 = 300.0;
pub const STRUCTURE_ARMOR: f32 = 40.0;
/// The fountain's laser: true damage a second to enemy champions this near it.
pub const FOUNTAIN_DPS: f32 = 1_000.0;
pub const FOUNTAIN_RANGE: f32 = 1_100.0;

// ---- health relics -------------------------------------------------------------------------

pub const RELIC_OUTER_FIRST: f32 = 105.0;
pub const RELIC_INNER_FIRST: f32 = 150.0;
pub const RELIC_RESPAWN: f32 = 90.0;
pub const RELIC_PICKUP: f32 = 120.0;
/// Of missing health and mana, at once to who took it...
pub const RELIC_NOW: f32 = 0.08;
/// ...and after the delay to every champion within the radius (Mayhem's 22%).
pub const RELIC_BEAM_DELAY: f32 = 2.5;
pub const RELIC_BEAM_RADIUS: f32 = 850.0;
pub const RELIC_BEAM: f32 = 0.22;

// ---- Mayhem --------------------------------------------------------------------------------

/// The levels each augment screen opens at (the first is the start).
pub const AUGMENT_LEVELS: [u8; 4] = [3, 7, 11, 15];
/// Combo Breaker: immobilised for this many of the last [`COMBO_WINDOW`] seconds cleanses, and
/// crowd control can't land for [`COMBO_IMMUNE`] seconds.
pub const COMBO_IMMOBILE: f32 = 5.0;
pub const COMBO_WINDOW: f32 = 7.0;
pub const COMBO_IMMUNE: f32 = 3.0;
