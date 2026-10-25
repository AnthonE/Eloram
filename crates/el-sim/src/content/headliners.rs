//! Mayhem's headliners: one match-wide change a game, drawn from the match's seed, the same for
//! all ten (`docs/DESIGN.md`, "Mayhem: augments").

use super::StatMods;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeadlinerDef {
    pub name: &'static str,
    /// What it changes, in a line for the page.
    pub blurb: &'static str,
    /// Added to every champion's stats.
    pub mods: StatMods,
    /// Passive gold, as a multiple.
    pub gold: f32,
    /// Death timers, as a multiple.
    pub respawn: f32,
}

const fn headliner(name: &'static str, blurb: &'static str, mods: StatMods) -> HeadlinerDef {
    HeadlinerDef { name, blurb, mods, gold: 1.0, respawn: 1.0 }
}

const Z: StatMods = StatMods::ZERO;

pub const HEADLINERS: [HeadlinerDef; 6] = [
    headliner("Swift Winds", "Every champion moves 15% faster.", StatMods { move_share: 0.15, ..Z }),
    headliner("Clockwork", "Every champion has 40 ability haste.", StatMods { haste: 40.0, ..Z }),
    HeadlinerDef { gold: 2.0, ..headliner("Gold Fever", "Passive gold comes twice as fast.", Z) },
    HeadlinerDef { respawn: 0.5, ..headliner("Short Rest", "Death timers are halved.", Z) },
    headliner(
        "Iron Age",
        "Every champion has 500 more health, 20 armour and 20 magic resist.",
        StatMods { hp: 500.0, armor: 20.0, mr: 20.0, ..Z },
    ),
    headliner(
        "Sharp Edges",
        "Every champion has 20 attack damage and 40 ability power more.",
        StatMods { ad: 20.0, ap: 40.0, ..Z },
    ),
];
