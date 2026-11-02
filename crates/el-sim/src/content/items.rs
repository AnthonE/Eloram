//! The shop: stat items, bought while it's open (dead, or on the platform), a few of them with an
//! active used from their slot; and the trinket, treats eaten from a slot of their own.

use super::{DmgKind, Op, StatMods, flat};
use crate::units::BuffKind;

/// What an item does when it's used: `ops` on its holder, and `hits` on every enemy within
/// `radius` of it (structures aside), then `cd` s before it can be used again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Active {
    pub cd: f32,
    pub ops: &'static [Op],
    pub radius: f32,
    pub hits: &'static [Op],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemDef {
    pub name: &'static str,
    pub cost: u32,
    pub mods: StatMods,
    pub active: Option<Active>,
}

const fn item(name: &'static str, cost: u32, mods: StatMods) -> ItemDef {
    ItemDef { name, cost, mods, active: None }
}

const fn used(name: &'static str, cost: u32, mods: StatMods, active: Active) -> ItemDef {
    ItemDef { name, cost, mods, active: Some(active) }
}

const Z: StatMods = StatMods::ZERO;

/// The catalogue, in the order items are numbered (the numbers are in the record: add at the end
/// only with `CONTENT_VERSION` bumped).
pub const ITEMS: [ItemDef; 18] = [
    item("Iron Knife", 350, StatMods { ad: 15.0, ..Z }),
    item("Spell Primer", 435, StatMods { ap: 30.0, ..Z }),
    item("Mail Shirt", 300, StatMods { armor: 20.0, ..Z }),
    item("Quiet Cloak", 400, StatMods { mr: 25.0, ..Z }),
    item("Red Bloom", 400, StatMods { hp: 200.0, ..Z }),
    item("Stiletto", 300, StatMods { attack_speed: 0.15, ..Z }),
    item("Road Boots", 300, StatMods { move_speed: 40.0, ..Z }),
    item("Still Water", 350, StatMods { mana: 250.0, ..Z }),
    item("Warblade", 1_300, StatMods { ad: 40.0, haste: 20.0, ..Z }),
    item("Witchwood Staff", 1_200, StatMods { ap: 70.0, ..Z }),
    item("Rampart", 1_500, StatMods { hp: 400.0, armor: 40.0, ..Z }),
    item("Stormcaller", 1_400, StatMods { ad: 20.0, attack_speed: 0.35, ..Z }),
    item("Lifedrinker", 1_400, StatMods { ad: 30.0, lifesteal: 0.12, ..Z }),
    item("Spellguard", 1_300, StatMods { hp: 300.0, mr: 45.0, ..Z }),
    // With an active.
    used(
        "Bulwark Charm",
        1_300,
        StatMods { hp: 250.0, armor: 20.0, mr: 20.0, ..Z },
        Active {
            cd: 60.0,
            ops: &[Op::Shield { base: flat(300.0), ap: 0.3, secs: 2.5 }],
            radius: 0.0,
            hits: &[],
        },
    ),
    used(
        "Windstride",
        1_100,
        StatMods { move_speed: 45.0, attack_speed: 0.15, ..Z },
        Active {
            cd: 45.0,
            ops: &[Op::SelfBuff { kind: BuffKind::MoveSpeed, secs: 3.0, value: 0.4 }],
            radius: 0.0,
            hits: &[],
        },
    ),
    used(
        "Thunder Censer",
        1_400,
        StatMods { ap: 60.0, haste: 10.0, ..Z },
        Active {
            cd: 40.0,
            ops: &[],
            radius: 400.0,
            hits: &[
                Op::Damage { kind: DmgKind::Magic, base: flat(150.0), ad: 0.0, ap: 0.4, missing: 0.0 },
                Op::Slow { share: 0.3, secs: 1.5 },
            ],
        },
    ),
    used(
        "Mending Chalice",
        1_200,
        StatMods { hp: 200.0, mana: 300.0, ..Z },
        Active { cd: 75.0, ops: &[Op::Heal { base: flat(300.0), ap: 0.3 }], radius: 0.0, hits: &[] },
    ),
];

/// An empty inventory slot.
pub const NONE: u8 = u8::MAX;
/// Inventory slots.
pub const SLOTS: usize = 6;
/// What selling gives back.
pub const SELL_SHARE: f32 = 0.7;

/// The trinket: treats, eaten one at a time from their own slot (`el_proto::order::TRINKET`).
pub const TREAT_NAME: &str = "Hearthbread";
/// What a treat does: a little health back and a moment's speed.
pub const TREAT: Active = Active {
    cd: 10.0,
    ops: &[
        Op::Heal { base: flat(120.0), ap: 0.1 },
        Op::SelfBuff { kind: BuffKind::MoveSpeed, secs: 2.0, value: 0.15 },
    ],
    radius: 0.0,
    hits: &[],
};
/// Treats a champion starts with; one more for each enemy turret its team destroys, up to the most.
pub const TREATS_START: u8 = 1;
pub const TREATS_MAX: u8 = 6;
