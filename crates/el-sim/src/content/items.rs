//! The shop: stat items, bought while it's open (dead, or on the platform).

use super::StatMods;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemDef {
    pub name: &'static str,
    pub cost: u32,
    pub mods: StatMods,
}

const fn item(name: &'static str, cost: u32, mods: StatMods) -> ItemDef {
    ItemDef { name, cost, mods }
}

const Z: StatMods = StatMods::ZERO;

pub const ITEMS: [ItemDef; 14] = [
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
];

/// An empty inventory slot.
pub const NONE: u8 = u8::MAX;
/// Inventory slots.
pub const SLOTS: usize = 6;
/// What selling gives back.
pub const SELL_SHARE: f32 = 0.7;
