//! The champions. Five archetypes to start: a marksman, a mage, a tank, an assassin, an enchanter.
//! Their numbers sit where League's sit for the same roles; their names and kits are their own.

use super::{AbilityDef, ChampDef, DmgKind, Effect, Filter, Op, Target, flat};
use crate::units::{BuffKind, Stats};

/// Base stats: health and its regen, mana and its regen, attack damage, armour, magic resist,
/// attacks a second, range, move speed.
#[allow(clippy::too_many_arguments)]
const fn stats(
    max_hp: f32,
    hp_regen: f32,
    max_mana: f32,
    mana_regen: f32,
    ad: f32,
    armor: f32,
    mr: f32,
    attack_speed: f32,
    range: f32,
    move_speed: f32,
) -> Stats {
    Stats {
        max_hp,
        hp_regen,
        max_mana,
        mana_regen,
        ad,
        ap: 0.0,
        armor,
        mr,
        attack_speed,
        range,
        move_speed,
        haste: 0.0,
        lifesteal: 0.0,
    }
}

/// What every champion gains a level, apart from what differs (health, damage, armour).
const fn growth(max_hp: f32, ad: f32, armor: f32, mana: f32) -> Stats {
    stats(max_hp, 0.08, mana, 0.06, ad, armor, 1.3, 0.02, 0.0, 0.0)
}

const fn ranked(a: f32, step: f32) -> [f32; 5] {
    [a, a + step, a + 2.0 * step, a + 3.0 * step, a + 4.0 * step]
}

/// An ultimate's three ranks (the last two repeat the third).
const fn ult(a: f32, b: f32, c: f32) -> [f32; 5] {
    [a, b, c, c, c]
}

pub const VESSA: ChampDef = ChampDef {
    name: "Vessa",
    title: "the Long Watch",
    base: stats(640.0, 0.7, 340.0, 1.4, 59.0, 26.0, 30.0, 0.65, 550.0, 330.0),
    growth: growth(100.0, 3.0, 4.2, 40.0),
    radius: 60.0,
    missile_speed: 2_000.0,
    windup: 0.18,
    abilities: [
        AbilityDef {
            name: "Piercing Bolt",
            target: Target::Dir,
            range: 1_100.0,
            cooldown: [7.0, 6.5, 6.0, 5.5, 5.0],
            cost: ranked(40.0, 5.0),
            windup: 0.25,
            effect: Effect::Line {
                speed: 2_000.0,
                width: 70.0,
                pierce: true,
                ops: &[Op::Damage {
                    kind: DmgKind::Physical,
                    base: ranked(60.0, 35.0),
                    ad: 1.0,
                    ap: 0.0,
                    missing: 0.0,
                }],
            },
        },
        AbilityDef {
            name: "Hobbling Shot",
            target: Target::Dir,
            range: 1_000.0,
            cooldown: flat(12.0),
            cost: flat(60.0),
            windup: 0.25,
            effect: Effect::Line {
                speed: 1_600.0,
                width: 60.0,
                pierce: false,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Physical,
                        base: ranked(50.0, 30.0),
                        ad: 0.6,
                        ap: 0.0,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.3, secs: 2.0 },
                ],
            },
        },
        AbilityDef {
            name: "Tumble",
            target: Target::Point,
            range: 300.0,
            cooldown: [14.0, 13.0, 12.0, 11.0, 10.0],
            cost: flat(30.0),
            windup: 0.0,
            effect: Effect::Dash {
                speed: 1_200.0,
                radius: 0.0,
                ops: &[Op::SelfBuff { kind: BuffKind::AttackSpeed, secs: 3.0, value: 0.4 }],
            },
        },
        AbilityDef {
            name: "Long Shot",
            target: Target::Dir,
            range: 3_000.0,
            cooldown: ult(100.0, 85.0, 70.0),
            cost: flat(100.0),
            windup: 0.75,
            effect: Effect::Line {
                speed: 2_600.0,
                width: 120.0,
                pierce: false,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Physical,
                        base: ult(250.0, 400.0, 550.0),
                        ad: 1.2,
                        ap: 0.0,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.5, secs: 1.0 },
                ],
            },
        },
    ],
};

pub const ORRIN: ChampDef = ChampDef {
    name: "Orrin",
    title: "of the Ash Choir",
    base: stats(590.0, 0.6, 420.0, 1.6, 52.0, 22.0, 30.0, 0.63, 525.0, 330.0),
    growth: growth(95.0, 3.0, 4.5, 30.0),
    radius: 60.0,
    missile_speed: 1_600.0,
    windup: 0.2,
    abilities: [
        AbilityDef {
            name: "Ember",
            target: Target::Dir,
            range: 1_000.0,
            cooldown: [6.0, 5.5, 5.0, 4.5, 4.0],
            cost: ranked(50.0, 5.0),
            windup: 0.25,
            effect: Effect::Line {
                speed: 1_600.0,
                width: 60.0,
                pierce: false,
                ops: &[Op::Damage {
                    kind: DmgKind::Magic,
                    base: ranked(80.0, 40.0),
                    ad: 0.0,
                    ap: 0.7,
                    missing: 0.0,
                }],
            },
        },
        AbilityDef {
            name: "Ash Field",
            target: Target::Point,
            range: 850.0,
            cooldown: flat(9.0),
            cost: flat(70.0),
            windup: 0.25,
            effect: Effect::Area {
                radius: 220.0,
                delay: 0.6,
                filter: Filter::Enemies,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(70.0, 45.0),
                        ad: 0.0,
                        ap: 0.6,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.35, secs: 1.5 },
                ],
            },
        },
        AbilityDef {
            name: "Step Through",
            target: Target::Point,
            range: 450.0,
            cooldown: [18.0, 16.0, 14.0, 12.0, 10.0],
            cost: flat(60.0),
            windup: 0.0,
            effect: Effect::Blink { radius: 0.0, ops: &[] },
        },
        AbilityDef {
            name: "Starfall",
            target: Target::Point,
            range: 900.0,
            cooldown: ult(110.0, 95.0, 80.0),
            cost: flat(100.0),
            windup: 0.25,
            effect: Effect::Area {
                radius: 350.0,
                delay: 1.0,
                filter: Filter::Enemies,
                ops: &[Op::Damage {
                    kind: DmgKind::Magic,
                    base: ult(250.0, 375.0, 500.0),
                    ad: 0.0,
                    ap: 0.9,
                    missing: 0.0,
                }],
            },
        },
    ],
};

pub const BRAKKA: ChampDef = ChampDef {
    name: "Brakka",
    title: "the Quarry",
    base: stats(690.0, 0.9, 300.0, 1.4, 62.0, 36.0, 32.0, 0.62, 175.0, 335.0),
    growth: growth(110.0, 3.5, 4.5, 40.0),
    radius: 75.0,
    missile_speed: 0.0,
    windup: 0.25,
    abilities: [
        AbilityDef {
            name: "Chain Hook",
            target: Target::Dir,
            range: 1_050.0,
            cooldown: [16.0, 15.0, 14.0, 13.0, 12.0],
            cost: flat(60.0),
            windup: 0.3,
            effect: Effect::Line {
                speed: 1_800.0,
                width: 70.0,
                pierce: false,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(80.0, 40.0),
                        ad: 0.0,
                        ap: 0.6,
                        missing: 0.0,
                    },
                    Op::Pull,
                    Op::Stun(0.5),
                ],
            },
        },
        AbilityDef {
            name: "Stone Skin",
            target: Target::Caster,
            range: 0.0,
            cooldown: flat(14.0),
            cost: flat(50.0),
            windup: 0.0,
            effect: Effect::OnSelf {
                ops: &[
                    Op::Shield { base: ranked(80.0, 40.0), ap: 0.5, secs: 4.0 },
                    Op::SelfBuff { kind: BuffKind::Armor, secs: 4.0, value: 20.0 },
                ],
            },
        },
        AbilityDef {
            name: "Quake",
            target: Target::Caster,
            range: 0.0,
            cooldown: flat(9.0),
            cost: flat(50.0),
            windup: 0.2,
            effect: Effect::Area {
                radius: 350.0,
                delay: 0.0,
                filter: Filter::Enemies,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(60.0, 40.0),
                        ad: 0.0,
                        ap: 0.5,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.4, secs: 1.5 },
                ],
            },
        },
        AbilityDef {
            name: "Upheaval",
            target: Target::Point,
            range: 600.0,
            cooldown: ult(120.0, 105.0, 90.0),
            cost: flat(100.0),
            windup: 0.0,
            effect: Effect::Dash {
                speed: 1_500.0,
                radius: 300.0,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ult(150.0, 250.0, 350.0),
                        ad: 0.0,
                        ap: 0.7,
                        missing: 0.0,
                    },
                    Op::Airborne(1.0),
                ],
            },
        },
    ],
};

pub const KESH: ChampDef = ChampDef {
    name: "Kesh",
    title: "the Quiet Knife",
    base: stats(600.0, 0.8, 300.0, 1.3, 64.0, 30.0, 32.0, 0.66, 150.0, 345.0),
    growth: growth(95.0, 3.5, 4.0, 35.0),
    radius: 60.0,
    missile_speed: 0.0,
    windup: 0.22,
    abilities: [
        AbilityDef {
            name: "Twin Blades",
            target: Target::Enemy,
            range: 500.0,
            cooldown: [8.0, 7.5, 7.0, 6.5, 6.0],
            cost: flat(40.0),
            windup: 0.15,
            effect: Effect::Homing {
                speed: 1_500.0,
                ops: &[Op::Damage {
                    kind: DmgKind::Physical,
                    base: ranked(70.0, 40.0),
                    ad: 0.9,
                    ap: 0.0,
                    missing: 0.0,
                }],
            },
        },
        AbilityDef {
            name: "Shadow Step",
            target: Target::Point,
            range: 550.0,
            cooldown: [13.0, 12.0, 11.0, 10.0, 9.0],
            cost: flat(50.0),
            windup: 0.0,
            effect: Effect::Dash {
                speed: 1_400.0,
                radius: 150.0,
                ops: &[Op::Damage {
                    kind: DmgKind::Physical,
                    base: ranked(60.0, 30.0),
                    ad: 0.6,
                    ap: 0.0,
                    missing: 0.0,
                }],
            },
        },
        AbilityDef {
            name: "Cripple",
            target: Target::Caster,
            range: 0.0,
            cooldown: flat(12.0),
            cost: flat(40.0),
            windup: 0.1,
            effect: Effect::Area {
                radius: 300.0,
                delay: 0.0,
                filter: Filter::Enemies,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Physical,
                        base: ranked(40.0, 20.0),
                        ad: 0.4,
                        ap: 0.0,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.5, secs: 2.0 },
                ],
            },
        },
        AbilityDef {
            name: "Execution",
            target: Target::Enemy,
            range: 450.0,
            cooldown: ult(90.0, 75.0, 60.0),
            cost: flat(100.0),
            windup: 0.2,
            effect: Effect::Strike {
                ops: &[Op::Damage {
                    kind: DmgKind::Physical,
                    base: ult(150.0, 250.0, 350.0),
                    ad: 1.0,
                    ap: 0.0,
                    missing: 0.3,
                }],
            },
        },
    ],
};

pub const LIORA: ChampDef = ChampDef {
    name: "Liora",
    title: "the Lantern",
    base: stats(560.0, 0.6, 400.0, 1.8, 50.0, 24.0, 30.0, 0.63, 525.0, 330.0),
    growth: growth(90.0, 3.0, 4.0, 40.0),
    radius: 60.0,
    missile_speed: 1_500.0,
    windup: 0.2,
    abilities: [
        AbilityDef {
            name: "Bind",
            target: Target::Dir,
            range: 1_100.0,
            cooldown: [11.0, 10.5, 10.0, 9.5, 9.0],
            cost: flat(60.0),
            windup: 0.25,
            effect: Effect::Line {
                speed: 1_300.0,
                width: 70.0,
                pierce: false,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(70.0, 40.0),
                        ad: 0.0,
                        ap: 0.6,
                        missing: 0.0,
                    },
                    Op::Root(1.75),
                ],
            },
        },
        AbilityDef {
            name: "Mend",
            target: Target::Caster,
            range: 0.0,
            cooldown: flat(12.0),
            cost: flat(70.0),
            windup: 0.2,
            effect: Effect::Area {
                radius: 600.0,
                delay: 0.0,
                filter: Filter::Allies,
                ops: &[Op::Heal { base: ranked(60.0, 30.0), ap: 0.4 }],
            },
        },
        AbilityDef {
            name: "Aegis",
            target: Target::Ally,
            range: 700.0,
            cooldown: flat(10.0),
            cost: flat(50.0),
            windup: 0.0,
            effect: Effect::Strike {
                ops: &[
                    Op::Shield { base: ranked(70.0, 40.0), ap: 0.5, secs: 3.0 },
                    Op::Buff { kind: BuffKind::MoveSpeed, secs: 2.0, value: 0.2 },
                ],
            },
        },
        AbilityDef {
            name: "Sanctuary",
            target: Target::Point,
            range: 800.0,
            cooldown: ult(110.0, 95.0, 80.0),
            cost: flat(100.0),
            windup: 0.25,
            effect: Effect::Zone {
                radius: 400.0,
                secs: 4.0,
                period: 0.5,
                filter: Filter::Allies,
                follows: false,
                ops: &[Op::Heal { base: ult(25.0, 40.0, 55.0), ap: 0.1 }],
            },
        },
    ],
};

/// The pool, in the order champions are numbered.
pub static CHAMPIONS: [ChampDef; 5] = [VESSA, ORRIN, BRAKKA, KESH, LIORA];

/// The summoner spells every champion has: D, a blink; F, a thrown snowball that marks what it hits,
/// and cast again while the mark lasts, a dash to it.
pub static FLASH: AbilityDef = AbilityDef {
    name: "Flash",
    target: Target::Point,
    range: 400.0,
    cooldown: flat(240.0),
    cost: flat(0.0),
    windup: 0.0,
    effect: Effect::Blink { radius: 0.0, ops: &[] },
};

pub static MARK: AbilityDef = AbilityDef {
    name: "Mark",
    target: Target::Dir,
    range: 1_600.0,
    cooldown: flat(40.0),
    cost: flat(0.0),
    windup: 0.0,
    effect: Effect::Line {
        speed: 1_500.0,
        width: 60.0,
        pierce: false,
        ops: &[
            Op::Damage { kind: DmgKind::True, base: flat(30.0), ad: 0.0, ap: 0.0, missing: 0.0 },
            Op::Buff { kind: BuffKind::Marked, secs: 3.0, value: 0.0 },
        ],
    },
};

/// The dash to a marked target.
pub const MARK_DASH_SPEED: f32 = 1_600.0;

/// The ability in `slot` (Q, W, E, R, then D and F) of champion `champ`.
pub fn ability(champ: u8, slot: u8) -> &'static AbilityDef {
    match slot {
        4 => &FLASH,
        5 => &MARK,
        s => &CHAMPIONS[usize::from(champ)].abilities[usize::from(s.min(3))],
    }
}

/// The summoner slots.
pub const FLASH_SLOT: u8 = 4;
pub const MARK_SLOT: u8 = 5;
