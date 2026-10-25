//! Mayhem's augments: picked on the screens at the start and at levels 7, 11 and 15, three offers
//! a screen, one tier a screen (`docs/DESIGN.md`, "Mayhem: augments"). An augment is stat changes
//! and, for some, ops on a trigger.

use super::{DmgKind, Op, StatMods, flat};
use crate::units::BuffKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Tier {
    Silver = 0,
    Gold = 1,
    Prismatic = 2,
}

/// When an augment's ops run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Trigger {
    None,
    /// Every `every`th auto attack that lands, on its target.
    AutoHit {
        every: u8,
        ops: &'static [Op],
    },
    /// When an ability hits an enemy, on it (at most once every `cd` s).
    AbilityHit {
        cd: f32,
        ops: &'static [Op],
    },
    /// On a kill or an assist, on the champion.
    Takedown {
        ops: &'static [Op],
    },
    /// Falling under `below` of maximum health, on the champion (at most once every `cd` s).
    LowHealth {
        below: f32,
        cd: f32,
        ops: &'static [Op],
    },
    /// Every `period` s, on enemies within `radius`.
    Pulse {
        period: f32,
        radius: f32,
        ops: &'static [Op],
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AugmentDef {
    pub name: &'static str,
    pub tier: Tier,
    pub mods: StatMods,
    pub trigger: Trigger,
}

const fn mods(ad: f32, ap: f32, hp: f32, armor: f32, mr: f32) -> StatMods {
    StatMods { ad, ap, hp, armor, mr, ..StatMods::ZERO }
}

const fn stat(name: &'static str, tier: Tier, mods: StatMods) -> AugmentDef {
    AugmentDef { name, tier, mods, trigger: Trigger::None }
}

pub const AUGMENTS: [AugmentDef; 18] = [
    // Silver
    stat("Keen Edge", Tier::Silver, mods(15.0, 0.0, 0.0, 0.0, 0.0)),
    stat("Spark Plug", Tier::Silver, mods(0.0, 25.0, 0.0, 0.0, 0.0)),
    stat("Thick Hide", Tier::Silver, mods(0.0, 0.0, 0.0, 20.0, 20.0)),
    stat("Fleet Foot", Tier::Silver, StatMods { move_share: 0.08, ..StatMods::ZERO }),
    stat("Quick Hands", Tier::Silver, StatMods { attack_speed: 0.25, ..StatMods::ZERO }),
    AugmentDef {
        name: "Second Wind",
        tier: Tier::Silver,
        mods: StatMods::ZERO,
        trigger: Trigger::LowHealth {
            below: 0.3,
            cd: 30.0,
            ops: &[Op::Shield { base: flat(150.0), ap: 0.2, secs: 3.0 }],
        },
    },
    stat("Deep Well", Tier::Silver, StatMods { mana: 300.0, ..StatMods::ZERO }),
    // Gold
    stat("Bloodthirst", Tier::Gold, StatMods { ad: 10.0, lifesteal: 0.12, ..StatMods::ZERO }),
    stat("Overclock", Tier::Gold, StatMods { haste: 25.0, ..StatMods::ZERO }),
    AugmentDef {
        name: "Tremor",
        tier: Tier::Gold,
        mods: StatMods::ZERO,
        trigger: Trigger::AutoHit {
            every: 3,
            ops: &[
                Op::Damage { kind: DmgKind::Magic, base: flat(60.0), ad: 0.0, ap: 0.3, missing: 0.0 },
                Op::Slow { share: 0.3, secs: 1.0 },
            ],
        },
    },
    AugmentDef {
        name: "Warmonger",
        tier: Tier::Gold,
        mods: StatMods::ZERO,
        trigger: Trigger::Takedown {
            ops: &[
                Op::Heal { base: flat(150.0), ap: 0.0 },
                Op::Buff { kind: BuffKind::MoveSpeed, secs: 2.0, value: 0.3 },
            ],
        },
    },
    stat("Ironclad", Tier::Gold, mods(0.0, 0.0, 400.0, 15.0, 0.0)),
    AugmentDef {
        name: "Ember Heart",
        tier: Tier::Gold,
        mods: StatMods::ZERO,
        trigger: Trigger::Pulse {
            period: 2.0,
            radius: 300.0,
            ops: &[Op::Damage { kind: DmgKind::Magic, base: flat(40.0), ad: 0.0, ap: 0.15, missing: 0.0 }],
        },
    },
    // Prismatic
    AugmentDef {
        name: "Final Word",
        tier: Tier::Prismatic,
        mods: StatMods::ZERO,
        trigger: Trigger::AbilityHit {
            cd: 4.0,
            ops: &[Op::Damage { kind: DmgKind::True, base: flat(0.0), ad: 0.0, ap: 0.0, missing: 0.08 }],
        },
    },
    stat("Juggernaut", Tier::Prismatic, mods(0.0, 0.0, 900.0, 30.0, 30.0)),
    stat("Arcane Torrent", Tier::Prismatic, StatMods { ap: 80.0, haste: 30.0, ..StatMods::ZERO }),
    stat("Bladestorm", Tier::Prismatic, StatMods { ad: 30.0, attack_speed: 0.6, ..StatMods::ZERO }),
    AugmentDef {
        name: "Phantom",
        tier: Tier::Prismatic,
        mods: StatMods { move_share: 0.2, ..StatMods::ZERO },
        trigger: Trigger::LowHealth {
            below: 0.25,
            cd: 45.0,
            ops: &[
                Op::Shield { base: flat(400.0), ap: 0.3, secs: 3.0 },
                Op::Buff { kind: BuffKind::MoveSpeed, secs: 3.0, value: 0.4 },
            ],
        },
    },
];

/// "No augment".
pub const NONE: u8 = u8::MAX;
