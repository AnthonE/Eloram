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
    /// When the champion casts one of its four abilities, on the champion (at most once every
    /// `cd` s).
    Cast {
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

const fn on(name: &'static str, tier: Tier, trigger: Trigger) -> AugmentDef {
    AugmentDef { name, tier, mods: StatMods::ZERO, trigger }
}

const fn magic(base: f32, ad: f32, ap: f32) -> Op {
    Op::Damage { kind: DmgKind::Magic, base: flat(base), ad, ap, missing: 0.0 }
}

const fn physical(base: f32, ad: f32) -> Op {
    Op::Damage { kind: DmgKind::Physical, base: flat(base), ad, ap: 0.0, missing: 0.0 }
}

const fn self_buff(kind: BuffKind, secs: f32, value: f32) -> Op {
    Op::SelfBuff { kind, secs, value }
}

/// The pool, in the order augments are numbered (the numbers are in the record: add at the end of
/// a tier's run only with `CONTENT_VERSION` bumped).
pub const AUGMENTS: [AugmentDef; 41] = [
    // Silver
    stat("Keen Edge", Tier::Silver, mods(15.0, 0.0, 0.0, 0.0, 0.0)),
    stat("Spark Plug", Tier::Silver, mods(0.0, 25.0, 0.0, 0.0, 0.0)),
    stat("Thick Hide", Tier::Silver, mods(0.0, 0.0, 0.0, 20.0, 20.0)),
    stat("Light Step", Tier::Silver, StatMods { move_share: 0.08, ..StatMods::ZERO }),
    stat("Quick Hands", Tier::Silver, StatMods { attack_speed: 0.25, ..StatMods::ZERO }),
    on(
        "Hold Fast",
        Tier::Silver,
        Trigger::LowHealth {
            below: 0.3,
            cd: 30.0,
            ops: &[Op::Shield { base: flat(150.0), ap: 0.2, secs: 3.0 }],
        },
    ),
    stat("Deep Well", Tier::Silver, StatMods { mana: 300.0, ..StatMods::ZERO }),
    // Gold
    stat("Red Tithe", Tier::Gold, StatMods { ad: 10.0, lifesteal: 0.12, ..StatMods::ZERO }),
    stat("Overclock", Tier::Gold, StatMods { haste: 25.0, ..StatMods::ZERO }),
    on(
        "Tremor",
        Tier::Gold,
        Trigger::AutoHit { every: 3, ops: &[magic(60.0, 0.0, 0.3), Op::Slow { share: 0.3, secs: 1.0 }] },
    ),
    on(
        "War Song",
        Tier::Gold,
        Trigger::Takedown {
            ops: &[
                Op::Heal { base: flat(150.0), ap: 0.0 },
                Op::Buff { kind: BuffKind::MoveSpeed, secs: 2.0, value: 0.3 },
            ],
        },
    ),
    stat("Ironclad", Tier::Gold, mods(0.0, 0.0, 400.0, 15.0, 0.0)),
    on(
        "Ember Heart",
        Tier::Gold,
        Trigger::Pulse { period: 2.0, radius: 300.0, ops: &[magic(40.0, 0.0, 0.15)] },
    ),
    // Prismatic
    on(
        "Final Word",
        Tier::Prismatic,
        Trigger::AbilityHit {
            cd: 4.0,
            ops: &[Op::Damage { kind: DmgKind::True, base: flat(0.0), ad: 0.0, ap: 0.0, missing: 0.08 }],
        },
    ),
    stat("Mountain", Tier::Prismatic, mods(0.0, 0.0, 900.0, 30.0, 30.0)),
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
    // Silver, the second set
    stat("Hearty", Tier::Silver, StatMods { hp: 250.0, ..StatMods::ZERO }),
    stat("Sharp Mind", Tier::Silver, StatMods { haste: 15.0, mana: 150.0, ..StatMods::ZERO }),
    stat("Steady Pulse", Tier::Silver, StatMods { hp_regen: 4.0, ..StatMods::ZERO }),
    on("Heavy Swing", Tier::Silver, Trigger::AutoHit { every: 4, ops: &[physical(40.0, 0.3)] }),
    on("Kindling", Tier::Silver, Trigger::AbilityHit { cd: 3.0, ops: &[magic(30.0, 0.0, 0.2)] }),
    on(
        "Spring Step",
        Tier::Silver,
        Trigger::Cast { cd: 6.0, ops: &[self_buff(BuffKind::MoveSpeed, 1.5, 0.2)] },
    ),
    on("Scrapper", Tier::Silver, Trigger::Takedown { ops: &[self_buff(BuffKind::AttackDamage, 6.0, 20.0)] }),
    // Gold, the second set
    on(
        "Venom",
        Tier::Gold,
        Trigger::AbilityHit { cd: 2.0, ops: &[magic(20.0, 0.0, 0.1), Op::Slow { share: 0.3, secs: 1.0 }] },
    ),
    on(
        "Bastion",
        Tier::Gold,
        Trigger::LowHealth {
            below: 0.4,
            cd: 40.0,
            ops: &[
                Op::Shield { base: flat(300.0), ap: 0.3, secs: 3.0 },
                self_buff(BuffKind::Armor, 3.0, 30.0),
                self_buff(BuffKind::MagicResist, 3.0, 30.0),
            ],
        },
    ),
    on(
        "Battle Trance",
        Tier::Gold,
        Trigger::Cast { cd: 8.0, ops: &[self_buff(BuffKind::AttackSpeed, 3.0, 0.4)] },
    ),
    stat("Quickening", Tier::Gold, StatMods { haste: 15.0, move_share: 0.05, ..StatMods::ZERO }),
    on("Chill Touch", Tier::Gold, Trigger::AutoHit { every: 1, ops: &[Op::Slow { share: 0.2, secs: 0.5 }] }),
    stat("Storm Rider", Tier::Gold, StatMods { move_share: 0.1, attack_speed: 0.2, ..StatMods::ZERO }),
    stat("Fragile Edge", Tier::Gold, StatMods { ad: 25.0, ap: 40.0, hp: -150.0, ..StatMods::ZERO }),
    on("Snare Wire", Tier::Gold, Trigger::AbilityHit { cd: 8.0, ops: &[Op::Root(0.75)] }),
    // Prismatic, the second set
    on(
        "Earthshaker",
        Tier::Prismatic,
        Trigger::Pulse {
            period: 3.0,
            radius: 400.0,
            ops: &[magic(60.0, 0.0, 0.2), Op::Slow { share: 0.4, secs: 1.0 }],
        },
    ),
    on(
        "Reaper",
        Tier::Prismatic,
        Trigger::AbilityHit {
            cd: 6.0,
            ops: &[Op::Damage { kind: DmgKind::True, base: flat(0.0), ad: 0.0, ap: 0.0, missing: 0.12 }],
        },
    ),
    stat("Overdrive", Tier::Prismatic, StatMods { haste: 50.0, ..StatMods::ZERO }),
    on(
        "Undying",
        Tier::Prismatic,
        Trigger::LowHealth {
            below: 0.2,
            cd: 60.0,
            ops: &[
                Op::Shield { base: flat(800.0), ap: 0.5, secs: 4.0 },
                Op::Heal { base: flat(300.0), ap: 0.0 },
            ],
        },
    ),
    on(
        "Tempo",
        Tier::Prismatic,
        Trigger::Cast {
            cd: 2.0,
            ops: &[self_buff(BuffKind::AttackSpeed, 2.0, 0.5), self_buff(BuffKind::MoveSpeed, 2.0, 0.15)],
        },
    ),
    stat(
        "Bloodstorm",
        Tier::Prismatic,
        StatMods { ad: 40.0, lifesteal: 0.2, attack_speed: 0.3, ..StatMods::ZERO },
    ),
    stat("Grand Design", Tier::Prismatic, StatMods { ap: 120.0, mana: 500.0, ..StatMods::ZERO }),
    on("Stormbrand", Tier::Prismatic, Trigger::AutoHit { every: 2, ops: &[magic(80.0, 0.4, 0.4)] }),
];

/// "No augment".
pub const NONE: u8 = u8::MAX;
