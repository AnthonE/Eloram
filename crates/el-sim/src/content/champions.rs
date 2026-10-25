//! The champions: twenty, two to five a role (`docs/DESIGN.md`, "Champions"). Their numbers sit
//! where League's sit for the same roles; their names and kits are their own.

use super::{AbilityDef, ChampDef, DmgKind, Effect, Filter, Op, Role, Target, flat};
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
    role: Role::Marksman,
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
            name: "Roll",
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
    role: Role::Mage,
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
    role: Role::Tank,
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
    role: Role::Assassin,
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
    role: Role::Enchanter,
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

pub const TALMAR: ChampDef = ChampDef {
    name: "Talmar",
    title: "the Tidewarden",
    role: Role::Fighter,
    base: stats(670.0, 0.9, 320.0, 1.4, 64.0, 34.0, 32.0, 0.64, 175.0, 340.0),
    growth: growth(105.0, 3.5, 4.5, 40.0),
    radius: 70.0,
    missile_speed: 0.0,
    windup: 0.24,
    abilities: [
        AbilityDef {
            name: "Backwash",
            target: Target::Dir,
            range: 900.0,
            cooldown: [10.0, 9.5, 9.0, 8.5, 8.0],
            cost: flat(50.0),
            windup: 0.25,
            effect: Effect::Line {
                speed: 1_500.0,
                width: 80.0,
                pierce: true,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Physical,
                        base: ranked(70.0, 35.0),
                        ad: 0.8,
                        ap: 0.0,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.4, secs: 1.5 },
                ],
            },
        },
        AbilityDef {
            name: "Riptide",
            target: Target::Caster,
            range: 0.0,
            cooldown: flat(13.0),
            cost: flat(60.0),
            windup: 0.0,
            effect: Effect::Zone {
                radius: 300.0,
                secs: 3.0,
                period: 0.5,
                filter: Filter::Enemies,
                follows: true,
                ops: &[Op::Damage {
                    kind: DmgKind::Magic,
                    base: ranked(15.0, 8.0),
                    ad: 0.1,
                    ap: 0.1,
                    missing: 0.0,
                }],
            },
        },
        AbilityDef {
            name: "Breaker",
            target: Target::Point,
            range: 450.0,
            cooldown: [14.0, 13.0, 12.0, 11.0, 10.0],
            cost: flat(50.0),
            windup: 0.0,
            effect: Effect::Dash {
                speed: 1_300.0,
                radius: 200.0,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Physical,
                        base: ranked(60.0, 30.0),
                        ad: 0.5,
                        ap: 0.0,
                        missing: 0.0,
                    },
                    Op::Knockback(250.0),
                ],
            },
        },
        AbilityDef {
            name: "Maelstrom",
            target: Target::Point,
            range: 700.0,
            cooldown: ult(110.0, 95.0, 80.0),
            cost: flat(100.0),
            windup: 0.4,
            effect: Effect::Area {
                radius: 400.0,
                delay: 0.5,
                filter: Filter::Enemies,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Physical,
                        base: ult(200.0, 300.0, 400.0),
                        ad: 0.8,
                        ap: 0.0,
                        missing: 0.0,
                    },
                    Op::Airborne(1.0),
                ],
            },
        },
    ],
};

pub const SEREN: ChampDef = ChampDef {
    name: "Seren",
    title: "the Quiet Star",
    role: Role::Mage,
    base: stats(560.0, 0.6, 450.0, 1.7, 50.0, 22.0, 30.0, 0.62, 550.0, 330.0),
    growth: growth(90.0, 3.0, 4.0, 35.0),
    radius: 60.0,
    missile_speed: 1_700.0,
    windup: 0.2,
    abilities: [
        AbilityDef {
            name: "Lance of Dusk",
            target: Target::Dir,
            range: 1_300.0,
            cooldown: [9.0, 8.5, 8.0, 7.5, 7.0],
            cost: ranked(60.0, 5.0),
            windup: 0.4,
            effect: Effect::Line {
                speed: 2_200.0,
                width: 60.0,
                pierce: true,
                ops: &[Op::Damage {
                    kind: DmgKind::Magic,
                    base: ranked(90.0, 45.0),
                    ad: 0.0,
                    ap: 0.75,
                    missing: 0.0,
                }],
            },
        },
        AbilityDef {
            name: "Gravity Well",
            target: Target::Point,
            range: 900.0,
            cooldown: flat(14.0),
            cost: flat(70.0),
            windup: 0.25,
            effect: Effect::Zone {
                radius: 280.0,
                secs: 2.5,
                period: 0.5,
                filter: Filter::Enemies,
                follows: false,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(12.0, 6.0),
                        ad: 0.0,
                        ap: 0.08,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.5, secs: 0.6 },
                ],
            },
        },
        AbilityDef {
            name: "Drift",
            target: Target::Point,
            range: 400.0,
            cooldown: [16.0, 15.0, 14.0, 13.0, 12.0],
            cost: flat(50.0),
            windup: 0.0,
            effect: Effect::Blink { radius: 0.0, ops: &[] },
        },
        AbilityDef {
            name: "Skyfall",
            target: Target::Point,
            range: 2_400.0,
            cooldown: ult(120.0, 100.0, 80.0),
            cost: flat(100.0),
            windup: 0.5,
            effect: Effect::Area {
                radius: 300.0,
                delay: 1.5,
                filter: Filter::Enemies,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ult(300.0, 450.0, 600.0),
                        ad: 0.0,
                        ap: 1.0,
                        missing: 0.0,
                    },
                    Op::Reveal(3.0),
                ],
            },
        },
    ],
};

pub const HOLLIS: ChampDef = ChampDef {
    name: "Hollis",
    title: "the Bell Warden",
    role: Role::Tank,
    base: stats(700.0, 1.0, 320.0, 1.4, 60.0, 38.0, 34.0, 0.62, 175.0, 335.0),
    growth: growth(115.0, 3.0, 5.0, 40.0),
    radius: 80.0,
    missile_speed: 0.0,
    windup: 0.26,
    abilities: [
        AbilityDef {
            name: "Toll",
            target: Target::Enemy,
            range: 350.0,
            cooldown: [11.0, 10.5, 10.0, 9.5, 9.0],
            cost: flat(50.0),
            windup: 0.2,
            effect: Effect::Strike {
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(60.0, 35.0),
                        ad: 0.0,
                        ap: 0.5,
                        missing: 0.0,
                    },
                    Op::Stun(1.0),
                ],
            },
        },
        AbilityDef {
            name: "Bronze Hide",
            target: Target::Caster,
            range: 0.0,
            cooldown: flat(15.0),
            cost: flat(50.0),
            windup: 0.0,
            effect: Effect::OnSelf {
                ops: &[
                    Op::Shield { base: ranked(90.0, 45.0), ap: 0.4, secs: 4.0 },
                    Op::SelfBuff { kind: BuffKind::MagicResist, secs: 4.0, value: 25.0 },
                ],
            },
        },
        AbilityDef {
            name: "Peal",
            target: Target::Caster,
            range: 0.0,
            cooldown: flat(12.0),
            cost: flat(60.0),
            windup: 0.25,
            effect: Effect::Area {
                radius: 320.0,
                delay: 0.0,
                filter: Filter::Enemies,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(50.0, 35.0),
                        ad: 0.0,
                        ap: 0.4,
                        missing: 0.0,
                    },
                    Op::Knockback(200.0),
                ],
            },
        },
        AbilityDef {
            name: "Hold the Bell",
            target: Target::Caster,
            range: 0.0,
            cooldown: ult(110.0, 95.0, 80.0),
            cost: flat(100.0),
            windup: 0.0,
            effect: Effect::Zone {
                radius: 450.0,
                secs: 4.0,
                period: 0.5,
                filter: Filter::Enemies,
                follows: true,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ult(25.0, 40.0, 55.0),
                        ad: 0.0,
                        ap: 0.1,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.3, secs: 0.6 },
                ],
            },
        },
    ],
};

pub const VIRE: ChampDef = ChampDef {
    name: "Vire",
    title: "the Ashen Blade",
    role: Role::Fighter,
    base: stats(630.0, 0.8, 280.0, 1.3, 66.0, 32.0, 32.0, 0.67, 175.0, 345.0),
    growth: growth(100.0, 3.6, 4.2, 35.0),
    radius: 65.0,
    missile_speed: 0.0,
    windup: 0.22,
    abilities: [
        AbilityDef {
            name: "Sweep",
            target: Target::Caster,
            range: 0.0,
            cooldown: [7.0, 6.5, 6.0, 5.5, 5.0],
            cost: flat(35.0),
            windup: 0.15,
            effect: Effect::Area {
                radius: 280.0,
                delay: 0.0,
                filter: Filter::Enemies,
                ops: &[Op::Damage {
                    kind: DmgKind::Physical,
                    base: ranked(55.0, 30.0),
                    ad: 0.8,
                    ap: 0.0,
                    missing: 0.0,
                }],
            },
        },
        AbilityDef {
            name: "War Cry",
            target: Target::Caster,
            range: 0.0,
            cooldown: flat(14.0),
            cost: flat(40.0),
            windup: 0.0,
            effect: Effect::OnSelf {
                ops: &[
                    Op::SelfBuff { kind: BuffKind::AttackSpeed, secs: 4.0, value: 0.5 },
                    Op::SelfBuff { kind: BuffKind::MoveSpeed, secs: 2.0, value: 0.25 },
                ],
            },
        },
        AbilityDef {
            name: "Close In",
            target: Target::Enemy,
            range: 550.0,
            cooldown: [12.0, 11.0, 10.0, 9.0, 8.0],
            cost: flat(45.0),
            windup: 0.0,
            effect: Effect::Dash {
                speed: 1_500.0,
                radius: 0.0,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Physical,
                        base: ranked(60.0, 30.0),
                        ad: 0.6,
                        ap: 0.0,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.4, secs: 1.0 },
                ],
            },
        },
        AbilityDef {
            name: "Verdict",
            target: Target::Enemy,
            range: 400.0,
            cooldown: ult(100.0, 85.0, 70.0),
            cost: flat(100.0),
            windup: 0.3,
            effect: Effect::Strike {
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Physical,
                        base: ult(120.0, 220.0, 320.0),
                        ad: 0.9,
                        ap: 0.0,
                        missing: 0.25,
                    },
                    Op::Silence(1.5),
                ],
            },
        },
    ],
};

pub const ODA: ChampDef = ChampDef {
    name: "Oda",
    title: "of the Long Road",
    role: Role::Enchanter,
    base: stats(580.0, 0.6, 380.0, 1.6, 56.0, 24.0, 30.0, 0.65, 550.0, 330.0),
    growth: growth(92.0, 3.2, 4.0, 35.0),
    radius: 60.0,
    missile_speed: 1_800.0,
    windup: 0.19,
    abilities: [
        AbilityDef {
            name: "Road Stone",
            target: Target::Enemy,
            range: 650.0,
            cooldown: [7.0, 6.5, 6.0, 5.5, 5.0],
            cost: ranked(45.0, 5.0),
            windup: 0.2,
            effect: Effect::Homing {
                speed: 1_400.0,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(70.0, 40.0),
                        ad: 0.0,
                        ap: 0.6,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.25, secs: 1.5 },
                ],
            },
        },
        AbilityDef {
            name: "Tether",
            target: Target::Dir,
            range: 950.0,
            cooldown: [13.0, 12.5, 12.0, 11.5, 11.0],
            cost: flat(60.0),
            windup: 0.25,
            effect: Effect::Line {
                speed: 1_500.0,
                width: 70.0,
                pierce: false,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(60.0, 35.0),
                        ad: 0.0,
                        ap: 0.5,
                        missing: 0.0,
                    },
                    Op::Root(1.25),
                ],
            },
        },
        AbilityDef {
            name: "Hearth",
            target: Target::Ally,
            range: 650.0,
            cooldown: flat(14.0),
            cost: flat(60.0),
            windup: 0.0,
            effect: Effect::Strike {
                ops: &[
                    Op::Heal { base: ranked(60.0, 30.0), ap: 0.35 },
                    Op::Buff { kind: BuffKind::AbilityPower, secs: 4.0, value: 25.0 },
                ],
            },
        },
        AbilityDef {
            name: "Long Road",
            target: Target::Caster,
            range: 0.0,
            cooldown: ult(110.0, 95.0, 80.0),
            cost: flat(100.0),
            windup: 0.2,
            effect: Effect::Area {
                radius: 900.0,
                delay: 0.0,
                filter: Filter::Allies,
                ops: &[
                    Op::Buff { kind: BuffKind::MoveSpeed, secs: 4.0, value: 0.35 },
                    Op::Shield { base: ult(100.0, 160.0, 220.0), ap: 0.3, secs: 4.0 },
                ],
            },
        },
    ],
};

pub const MARRET: ChampDef = ChampDef {
    name: "Marret",
    title: "the Gunwright",
    role: Role::Marksman,
    base: stats(600.0, 0.7, 330.0, 1.4, 58.0, 25.0, 30.0, 0.66, 575.0, 325.0),
    growth: growth(95.0, 3.2, 4.0, 38.0),
    radius: 60.0,
    missile_speed: 2_200.0,
    windup: 0.17,
    abilities: [
        AbilityDef {
            name: "Ricochet",
            target: Target::Enemy,
            range: 650.0,
            cooldown: [8.0, 7.5, 7.0, 6.5, 6.0],
            cost: ranked(40.0, 5.0),
            windup: 0.2,
            effect: Effect::Homing {
                speed: 2_000.0,
                ops: &[Op::Damage {
                    kind: DmgKind::Physical,
                    base: ranked(40.0, 30.0),
                    ad: 1.1,
                    ap: 0.0,
                    missing: 0.0,
                }],
            },
        },
        AbilityDef {
            name: "Spent Brass",
            target: Target::Dir,
            range: 900.0,
            cooldown: flat(12.0),
            cost: flat(50.0),
            windup: 0.25,
            effect: Effect::Line {
                speed: 1_800.0,
                width: 90.0,
                pierce: true,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Physical,
                        base: ranked(50.0, 25.0),
                        ad: 0.6,
                        ap: 0.0,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.35, secs: 1.5 },
                ],
            },
        },
        AbilityDef {
            name: "Kick Back",
            target: Target::Point,
            range: 350.0,
            cooldown: [18.0, 16.5, 15.0, 13.5, 12.0],
            cost: flat(40.0),
            windup: 0.0,
            effect: Effect::Dash { speed: 1_400.0, radius: 0.0, ops: &[] },
        },
        AbilityDef {
            name: "Salvo",
            target: Target::Point,
            range: 1_400.0,
            cooldown: ult(100.0, 85.0, 70.0),
            cost: flat(100.0),
            windup: 0.3,
            effect: Effect::Zone {
                radius: 350.0,
                secs: 2.0,
                period: 0.25,
                filter: Filter::Enemies,
                follows: false,
                ops: &[Op::Damage {
                    kind: DmgKind::Physical,
                    base: ult(30.0, 45.0, 60.0),
                    ad: 0.2,
                    ap: 0.0,
                    missing: 0.0,
                }],
            },
        },
    ],
};

pub const NIMRA: ChampDef = ChampDef {
    name: "Nimra",
    title: "the Frostglass",
    role: Role::Mage,
    base: stats(570.0, 0.6, 440.0, 1.6, 50.0, 22.0, 30.0, 0.625, 525.0, 330.0),
    growth: growth(88.0, 3.0, 3.8, 35.0),
    radius: 60.0,
    missile_speed: 1_600.0,
    windup: 0.2,
    abilities: [
        AbilityDef {
            name: "Shard",
            target: Target::Dir,
            range: 1_050.0,
            cooldown: [8.0, 7.5, 7.0, 6.5, 6.0],
            cost: ranked(55.0, 5.0),
            windup: 0.25,
            effect: Effect::Line {
                speed: 1_600.0,
                width: 70.0,
                pierce: false,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(80.0, 40.0),
                        ad: 0.0,
                        ap: 0.7,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.5, secs: 1.25 },
                ],
            },
        },
        AbilityDef {
            name: "Rime",
            target: Target::Point,
            range: 800.0,
            cooldown: flat(15.0),
            cost: flat(70.0),
            windup: 0.25,
            effect: Effect::Zone {
                radius: 300.0,
                secs: 3.0,
                period: 0.5,
                filter: Filter::Enemies,
                follows: false,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(10.0, 6.0),
                        ad: 0.0,
                        ap: 0.06,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.3, secs: 0.6 },
                ],
            },
        },
        AbilityDef {
            name: "Cold Snap",
            target: Target::Enemy,
            range: 600.0,
            cooldown: [14.0, 13.0, 12.0, 11.0, 10.0],
            cost: flat(60.0),
            windup: 0.25,
            effect: Effect::Strike {
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(60.0, 30.0),
                        ad: 0.0,
                        ap: 0.5,
                        missing: 0.0,
                    },
                    Op::Root(1.0),
                ],
            },
        },
        AbilityDef {
            name: "Whiteout",
            target: Target::Caster,
            range: 0.0,
            cooldown: ult(120.0, 100.0, 80.0),
            cost: flat(100.0),
            windup: 0.4,
            effect: Effect::Area {
                radius: 450.0,
                delay: 0.0,
                filter: Filter::Enemies,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ult(200.0, 325.0, 450.0),
                        ad: 0.0,
                        ap: 0.8,
                        missing: 0.0,
                    },
                    Op::Stun(1.25),
                ],
            },
        },
    ],
};

pub const GORUM: ChampDef = ChampDef {
    name: "Gorum",
    title: "the Old Ox",
    role: Role::Tank,
    base: stats(710.0, 1.0, 300.0, 1.3, 63.0, 38.0, 34.0, 0.6, 175.0, 340.0),
    growth: growth(118.0, 3.2, 4.8, 38.0),
    radius: 85.0,
    missile_speed: 0.0,
    windup: 0.28,
    abilities: [
        AbilityDef {
            name: "Horn Toss",
            target: Target::Enemy,
            range: 300.0,
            cooldown: [12.0, 11.5, 11.0, 10.5, 10.0],
            cost: flat(50.0),
            windup: 0.15,
            effect: Effect::Strike {
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Physical,
                        base: ranked(60.0, 35.0),
                        ad: 0.5,
                        ap: 0.0,
                        missing: 0.0,
                    },
                    Op::Airborne(1.0),
                ],
            },
        },
        AbilityDef {
            name: "Stamp",
            target: Target::Caster,
            range: 0.0,
            cooldown: [9.0, 8.5, 8.0, 7.5, 7.0],
            cost: flat(40.0),
            windup: 0.2,
            effect: Effect::Area {
                radius: 300.0,
                delay: 0.0,
                filter: Filter::Enemies,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(40.0, 25.0),
                        ad: 0.0,
                        ap: 0.3,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.4, secs: 1.5 },
                ],
            },
        },
        AbilityDef {
            name: "Charge",
            target: Target::Point,
            range: 650.0,
            cooldown: [15.0, 14.0, 13.0, 12.0, 11.0],
            cost: flat(55.0),
            windup: 0.0,
            effect: Effect::Dash {
                speed: 1_200.0,
                radius: 160.0,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Physical,
                        base: ranked(50.0, 30.0),
                        ad: 0.4,
                        ap: 0.0,
                        missing: 0.0,
                    },
                    Op::Knockback(220.0),
                ],
            },
        },
        AbilityDef {
            name: "Unbroken",
            target: Target::Caster,
            range: 0.0,
            cooldown: ult(100.0, 85.0, 70.0),
            cost: flat(100.0),
            windup: 0.0,
            effect: Effect::OnSelf {
                ops: &[
                    Op::Shield { base: ult(300.0, 500.0, 700.0), ap: 0.5, secs: 5.0 },
                    Op::SelfBuff { kind: BuffKind::Armor, secs: 5.0, value: 40.0 },
                    Op::SelfBuff { kind: BuffKind::MagicResist, secs: 5.0, value: 40.0 },
                ],
            },
        },
    ],
};

pub const SABLE: ChampDef = ChampDef {
    name: "Sable",
    title: "the Night Courier",
    role: Role::Assassin,
    base: stats(590.0, 0.7, 380.0, 1.6, 54.0, 25.0, 30.0, 0.64, 425.0, 340.0),
    growth: growth(92.0, 3.0, 4.0, 36.0),
    radius: 60.0,
    missile_speed: 1_500.0,
    windup: 0.2,
    abilities: [
        AbilityDef {
            name: "Hex Bolt",
            target: Target::Enemy,
            range: 625.0,
            cooldown: [6.0, 5.5, 5.0, 4.5, 4.0],
            cost: ranked(40.0, 5.0),
            windup: 0.15,
            effect: Effect::Homing {
                speed: 1_500.0,
                ops: &[Op::Damage {
                    kind: DmgKind::Magic,
                    base: ranked(60.0, 35.0),
                    ad: 0.0,
                    ap: 0.55,
                    missing: 0.0,
                }],
            },
        },
        AbilityDef {
            name: "Shade Step",
            target: Target::Point,
            range: 450.0,
            cooldown: [14.0, 13.0, 12.0, 11.0, 10.0],
            cost: flat(60.0),
            windup: 0.0,
            effect: Effect::Blink {
                radius: 220.0,
                ops: &[Op::Damage {
                    kind: DmgKind::Magic,
                    base: ranked(50.0, 30.0),
                    ad: 0.0,
                    ap: 0.4,
                    missing: 0.0,
                }],
            },
        },
        AbilityDef {
            name: "Hush",
            target: Target::Enemy,
            range: 600.0,
            cooldown: [12.0, 11.5, 11.0, 10.5, 10.0],
            cost: flat(50.0),
            windup: 0.15,
            effect: Effect::Strike {
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(40.0, 20.0),
                        ad: 0.0,
                        ap: 0.3,
                        missing: 0.0,
                    },
                    Op::Silence(1.25),
                ],
            },
        },
        AbilityDef {
            name: "Dark Hour",
            target: Target::Point,
            range: 700.0,
            cooldown: ult(90.0, 75.0, 60.0),
            cost: flat(100.0),
            windup: 0.25,
            effect: Effect::Area {
                radius: 350.0,
                delay: 0.75,
                filter: Filter::Enemies,
                ops: &[Op::Damage {
                    kind: DmgKind::Magic,
                    base: ult(200.0, 300.0, 400.0),
                    ad: 0.0,
                    ap: 0.8,
                    missing: 0.15,
                }],
            },
        },
    ],
};

pub const PELL: ChampDef = ChampDef {
    name: "Pell",
    title: "the Hedge Knight",
    role: Role::Fighter,
    base: stats(650.0, 0.9, 300.0, 1.4, 64.0, 34.0, 32.0, 0.65, 175.0, 340.0),
    growth: growth(105.0, 3.5, 4.5, 38.0),
    radius: 70.0,
    missile_speed: 0.0,
    windup: 0.24,
    abilities: [
        AbilityDef {
            name: "Shield Bash",
            target: Target::Enemy,
            range: 300.0,
            cooldown: [10.0, 9.5, 9.0, 8.5, 8.0],
            cost: flat(45.0),
            windup: 0.15,
            effect: Effect::Strike {
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Physical,
                        base: ranked(50.0, 30.0),
                        ad: 0.6,
                        ap: 0.0,
                        missing: 0.0,
                    },
                    Op::Stun(0.75),
                ],
            },
        },
        AbilityDef {
            name: "Rally",
            target: Target::Caster,
            range: 0.0,
            cooldown: flat(16.0),
            cost: flat(60.0),
            windup: 0.0,
            effect: Effect::Area {
                radius: 600.0,
                delay: 0.0,
                filter: Filter::Allies,
                ops: &[
                    Op::Shield { base: ranked(60.0, 25.0), ap: 0.3, secs: 3.0 },
                    Op::Buff { kind: BuffKind::AttackSpeed, secs: 3.0, value: 0.3 },
                ],
            },
        },
        AbilityDef {
            name: "Spur",
            target: Target::Caster,
            range: 0.0,
            cooldown: flat(12.0),
            cost: flat(40.0),
            windup: 0.0,
            effect: Effect::OnSelf {
                ops: &[
                    Op::SelfBuff { kind: BuffKind::MoveSpeed, secs: 2.5, value: 0.4 },
                    Op::SelfBuff { kind: BuffKind::AttackDamage, secs: 4.0, value: 20.0 },
                ],
            },
        },
        AbilityDef {
            name: "Banner Charge",
            target: Target::Point,
            range: 900.0,
            cooldown: ult(110.0, 95.0, 80.0),
            cost: flat(100.0),
            windup: 0.2,
            effect: Effect::Dash {
                speed: 1_500.0,
                radius: 300.0,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Physical,
                        base: ult(150.0, 250.0, 350.0),
                        ad: 0.7,
                        ap: 0.0,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.5, secs: 1.5 },
                ],
            },
        },
    ],
};

pub const ULLA: ChampDef = ChampDef {
    name: "Ulla",
    title: "the Mender",
    role: Role::Enchanter,
    base: stats(550.0, 0.6, 420.0, 1.9, 48.0, 22.0, 30.0, 0.625, 550.0, 325.0),
    growth: growth(88.0, 2.8, 3.8, 40.0),
    radius: 60.0,
    missile_speed: 1_500.0,
    windup: 0.2,
    abilities: [
        AbilityDef {
            name: "Sap",
            target: Target::Dir,
            range: 1_000.0,
            cooldown: [9.0, 8.5, 8.0, 7.5, 7.0],
            cost: ranked(50.0, 5.0),
            windup: 0.25,
            effect: Effect::Line {
                speed: 1_500.0,
                width: 70.0,
                pierce: false,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(60.0, 35.0),
                        ad: 0.0,
                        ap: 0.5,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.4, secs: 1.5 },
                ],
            },
        },
        AbilityDef {
            name: "Bloom",
            target: Target::Ally,
            range: 700.0,
            cooldown: [10.0, 9.5, 9.0, 8.5, 8.0],
            cost: flat(70.0),
            windup: 0.1,
            effect: Effect::Strike { ops: &[Op::Heal { base: ranked(70.0, 30.0), ap: 0.4 }] },
        },
        AbilityDef {
            name: "Thorns",
            target: Target::Ally,
            range: 700.0,
            cooldown: flat(14.0),
            cost: flat(50.0),
            windup: 0.0,
            effect: Effect::Strike {
                ops: &[
                    Op::Buff { kind: BuffKind::Armor, secs: 4.0, value: 30.0 },
                    Op::Buff { kind: BuffKind::MagicResist, secs: 4.0, value: 30.0 },
                ],
            },
        },
        AbilityDef {
            name: "Verdance",
            target: Target::Caster,
            range: 0.0,
            cooldown: ult(130.0, 115.0, 100.0),
            cost: flat(100.0),
            windup: 0.3,
            effect: Effect::Area {
                radius: 1_200.0,
                delay: 0.0,
                filter: Filter::Allies,
                ops: &[
                    Op::Heal { base: ult(150.0, 250.0, 350.0), ap: 0.5 },
                    Op::Buff { kind: BuffKind::MoveSpeed, secs: 3.0, value: 0.3 },
                ],
            },
        },
    ],
};

pub const TOREK: ChampDef = ChampDef {
    name: "Torek",
    title: "the Siege Engine",
    role: Role::Mage,
    base: stats(560.0, 0.6, 440.0, 1.6, 50.0, 22.0, 30.0, 0.625, 600.0, 325.0),
    growth: growth(88.0, 3.0, 3.6, 36.0),
    radius: 65.0,
    missile_speed: 1_600.0,
    windup: 0.22,
    abilities: [
        AbilityDef {
            name: "Shell",
            target: Target::Point,
            range: 1_100.0,
            cooldown: [7.0, 6.5, 6.0, 5.5, 5.0],
            cost: ranked(50.0, 5.0),
            windup: 0.25,
            effect: Effect::Area {
                radius: 220.0,
                delay: 0.6,
                filter: Filter::Enemies,
                ops: &[Op::Damage {
                    kind: DmgKind::Magic,
                    base: ranked(70.0, 40.0),
                    ad: 0.0,
                    ap: 0.6,
                    missing: 0.0,
                }],
            },
        },
        AbilityDef {
            name: "Snare Charge",
            target: Target::Point,
            range: 900.0,
            cooldown: flat(16.0),
            cost: flat(70.0),
            windup: 0.25,
            effect: Effect::Area {
                radius: 260.0,
                delay: 1.0,
                filter: Filter::Enemies,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(40.0, 25.0),
                        ad: 0.0,
                        ap: 0.3,
                        missing: 0.0,
                    },
                    Op::Root(1.25),
                ],
            },
        },
        AbilityDef {
            name: "Brace",
            target: Target::Caster,
            range: 0.0,
            cooldown: flat(18.0),
            cost: flat(60.0),
            windup: 0.0,
            effect: Effect::OnSelf {
                ops: &[
                    Op::Shield { base: ranked(80.0, 40.0), ap: 0.4, secs: 3.0 },
                    Op::SelfBuff { kind: BuffKind::Armor, secs: 3.0, value: 25.0 },
                ],
            },
        },
        AbilityDef {
            name: "Bombardment",
            target: Target::Point,
            range: 3_000.0,
            cooldown: ult(130.0, 110.0, 90.0),
            cost: flat(100.0),
            windup: 0.6,
            effect: Effect::Zone {
                radius: 500.0,
                secs: 3.0,
                period: 0.5,
                filter: Filter::Enemies,
                follows: false,
                ops: &[Op::Damage {
                    kind: DmgKind::Magic,
                    base: ult(40.0, 60.0, 80.0),
                    ad: 0.0,
                    ap: 0.15,
                    missing: 0.0,
                }],
            },
        },
    ],
};

pub const SEFA: ChampDef = ChampDef {
    name: "Sefa",
    title: "the Wind Dancer",
    role: Role::Fighter,
    base: stats(620.0, 0.8, 300.0, 1.3, 62.0, 30.0, 32.0, 0.68, 175.0, 345.0),
    growth: growth(98.0, 3.3, 4.2, 35.0),
    radius: 62.0,
    missile_speed: 0.0,
    windup: 0.2,
    abilities: [
        AbilityDef {
            name: "Gale Cut",
            target: Target::Dir,
            range: 500.0,
            cooldown: [5.0, 4.6, 4.2, 3.8, 3.4],
            cost: flat(25.0),
            windup: 0.2,
            effect: Effect::Line {
                speed: 2_500.0,
                width: 90.0,
                pierce: true,
                ops: &[Op::Damage {
                    kind: DmgKind::Physical,
                    base: ranked(30.0, 25.0),
                    ad: 1.0,
                    ap: 0.0,
                    missing: 0.0,
                }],
            },
        },
        AbilityDef {
            name: "Updraft",
            target: Target::Caster,
            range: 0.0,
            cooldown: [16.0, 15.0, 14.0, 13.0, 12.0],
            cost: flat(50.0),
            windup: 0.2,
            effect: Effect::Area {
                radius: 250.0,
                delay: 0.0,
                filter: Filter::Enemies,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(40.0, 25.0),
                        ad: 0.4,
                        ap: 0.0,
                        missing: 0.0,
                    },
                    Op::Airborne(0.75),
                ],
            },
        },
        AbilityDef {
            name: "Glide",
            target: Target::Point,
            range: 450.0,
            cooldown: [12.0, 11.0, 10.0, 9.0, 8.0],
            cost: flat(30.0),
            windup: 0.0,
            effect: Effect::Dash { speed: 1_600.0, radius: 0.0, ops: &[] },
        },
        AbilityDef {
            name: "Tempest",
            target: Target::Caster,
            range: 0.0,
            cooldown: ult(100.0, 85.0, 70.0),
            cost: flat(100.0),
            windup: 0.0,
            effect: Effect::Zone {
                radius: 350.0,
                secs: 3.0,
                period: 0.5,
                filter: Filter::Enemies,
                follows: true,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Physical,
                        base: ult(30.0, 45.0, 60.0),
                        ad: 0.25,
                        ap: 0.0,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.25, secs: 0.6 },
                ],
            },
        },
    ],
};

pub const BRUN: ChampDef = ChampDef {
    name: "Brun",
    title: "the Smith",
    role: Role::Fighter,
    base: stats(680.0, 0.9, 320.0, 1.4, 64.0, 36.0, 32.0, 0.63, 175.0, 335.0),
    growth: growth(110.0, 3.4, 4.6, 38.0),
    radius: 75.0,
    missile_speed: 0.0,
    windup: 0.26,
    abilities: [
        AbilityDef {
            name: "Hammerfall",
            target: Target::Point,
            range: 450.0,
            cooldown: [10.0, 9.5, 9.0, 8.5, 8.0],
            cost: flat(50.0),
            windup: 0.3,
            effect: Effect::Area {
                radius: 200.0,
                delay: 0.25,
                filter: Filter::Enemies,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Physical,
                        base: ranked(60.0, 35.0),
                        ad: 0.7,
                        ap: 0.0,
                        missing: 0.0,
                    },
                    Op::Stun(0.75),
                ],
            },
        },
        AbilityDef {
            name: "Temper",
            target: Target::Caster,
            range: 0.0,
            cooldown: flat(14.0),
            cost: flat(40.0),
            windup: 0.0,
            effect: Effect::OnSelf {
                ops: &[
                    Op::SelfBuff { kind: BuffKind::AttackDamage, secs: 4.0, value: 25.0 },
                    Op::SelfBuff { kind: BuffKind::Armor, secs: 4.0, value: 25.0 },
                ],
            },
        },
        AbilityDef {
            name: "Bellows",
            target: Target::Dir,
            range: 600.0,
            cooldown: [13.0, 12.0, 11.0, 10.0, 9.0],
            cost: flat(50.0),
            windup: 0.25,
            effect: Effect::Line {
                speed: 1_400.0,
                width: 140.0,
                pierce: true,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(50.0, 30.0),
                        ad: 0.0,
                        ap: 0.4,
                        missing: 0.0,
                    },
                    Op::Knockback(250.0),
                ],
            },
        },
        AbilityDef {
            name: "Anvil",
            target: Target::Enemy,
            range: 400.0,
            cooldown: ult(110.0, 95.0, 80.0),
            cost: flat(100.0),
            windup: 0.3,
            effect: Effect::Strike {
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Physical,
                        base: ult(150.0, 250.0, 350.0),
                        ad: 0.6,
                        ap: 0.0,
                        missing: 0.1,
                    },
                    Op::Airborne(1.25),
                ],
            },
        },
    ],
};

pub const MIRA: ChampDef = ChampDef {
    name: "Mira",
    title: "the Lamplighter",
    role: Role::Enchanter,
    base: stats(560.0, 0.6, 420.0, 1.8, 50.0, 24.0, 30.0, 0.63, 525.0, 330.0),
    growth: growth(88.0, 3.0, 3.8, 38.0),
    radius: 60.0,
    missile_speed: 1_600.0,
    windup: 0.2,
    abilities: [
        AbilityDef {
            name: "Wick",
            target: Target::Enemy,
            range: 650.0,
            cooldown: [7.0, 6.5, 6.0, 5.5, 5.0],
            cost: ranked(45.0, 5.0),
            windup: 0.2,
            effect: Effect::Homing {
                speed: 1_500.0,
                ops: &[Op::Damage {
                    kind: DmgKind::Magic,
                    base: ranked(65.0, 35.0),
                    ad: 0.0,
                    ap: 0.55,
                    missing: 0.0,
                }],
            },
        },
        AbilityDef {
            name: "Glow",
            target: Target::Caster,
            range: 0.0,
            cooldown: flat(15.0),
            cost: flat(60.0),
            windup: 0.0,
            effect: Effect::Area {
                radius: 600.0,
                delay: 0.0,
                filter: Filter::Allies,
                ops: &[
                    Op::Buff { kind: BuffKind::AbilityPower, secs: 4.0, value: 30.0 },
                    Op::Buff { kind: BuffKind::AttackDamage, secs: 4.0, value: 15.0 },
                    Op::Buff { kind: BuffKind::MoveSpeed, secs: 2.0, value: 0.2 },
                ],
            },
        },
        AbilityDef {
            name: "Snuff",
            target: Target::Dir,
            range: 900.0,
            cooldown: [12.0, 11.5, 11.0, 10.5, 10.0],
            cost: flat(55.0),
            windup: 0.25,
            effect: Effect::Line {
                speed: 1_600.0,
                width: 70.0,
                pierce: false,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ranked(50.0, 30.0),
                        ad: 0.0,
                        ap: 0.4,
                        missing: 0.0,
                    },
                    Op::Silence(1.0),
                ],
            },
        },
        AbilityDef {
            name: "Bonfire",
            target: Target::Point,
            range: 800.0,
            cooldown: ult(110.0, 95.0, 80.0),
            cost: flat(100.0),
            windup: 0.3,
            effect: Effect::Zone {
                radius: 350.0,
                secs: 4.0,
                period: 0.5,
                filter: Filter::Enemies,
                follows: false,
                ops: &[
                    Op::Damage {
                        kind: DmgKind::Magic,
                        base: ult(35.0, 55.0, 75.0),
                        ad: 0.0,
                        ap: 0.15,
                        missing: 0.0,
                    },
                    Op::Slow { share: 0.2, secs: 0.6 },
                ],
            },
        },
    ],
};

/// The pool, in the order champions are numbered (the numbers are in the record).
pub static CHAMPIONS: [ChampDef; 20] = [
    VESSA, ORRIN, BRAKKA, KESH, LIORA, TALMAR, SEREN, HOLLIS, VIRE, ODA, MARRET, NIMRA, GORUM, SABLE, PELL,
    ULLA, TOREK, SEFA, BRUN, MIRA,
];

/// The summoner spells every champion has: D, Blink; F, Tag, a thrown snowball that marks what it
/// hits, and cast again while the mark lasts, a dash to it.
pub static BLINK: AbilityDef = AbilityDef {
    name: "Blink",
    target: Target::Point,
    range: 400.0,
    cooldown: flat(240.0),
    cost: flat(0.0),
    windup: 0.0,
    effect: Effect::Blink { radius: 0.0, ops: &[] },
};

pub static MARK: AbilityDef = AbilityDef {
    name: "Tag",
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
        4 => &BLINK,
        5 => &MARK,
        s => &CHAMPIONS[usize::from(champ)].abilities[usize::from(s.min(3))],
    }
}

/// The summoner slots.
pub const BLINK_SLOT: u8 = 4;
pub const MARK_SLOT: u8 = 5;
