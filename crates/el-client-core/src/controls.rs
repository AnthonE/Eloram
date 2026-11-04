//! The keys, once (`docs/DESIGN.md`, "Controls"): the menu's Controls page, the first match's
//! strip and the key over each ability on the HUD are all read from here, so they can't disagree
//! with each other. The keys that cast are the input code's own table (`el-client/src/input.rs`
//! turns each [`Key`] into the button it reads); the rest are words, which the tests here hold to
//! a line of the page each and to the font (the HUD's default font has ASCII and no more: `·`, `×`
//! and `–` are boxes).
//!
//! Two layouts, League's: its default (right click walks, Q, W, E and R cast) and its WASD one
//! (the keys walk; right click, Shift, E and R cast). Either casts at the cursor on the press, or
//! with indicators on, shows the ability's reach while held and casts when let go.

use el_proto::order::slot;

/// A layout of the keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Layout {
    /// League's default: right click walks and attacks, Q, W, E and R cast, D and F the summoner
    /// spells.
    #[default]
    League,
    /// League's WASD: W, A, S and D walk, left click attacks; right click, Shift, E and R cast, Q
    /// and F the summoner spells.
    Wasd,
}

impl Layout {
    pub fn of(wasd: bool) -> Self {
        if wasd { Layout::Wasd } else { Layout::League }
    }

    /// What the page and the settings call it (`?keys=`, `localStorage["el.keys"]`).
    pub fn name(self) -> &'static str {
        match self {
            Layout::League => "league",
            Layout::Wasd => "wasd",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "league" => Some(Layout::League),
            "wasd" => Some(Layout::Wasd),
            _ => None,
        }
    }

    /// What casts each slot (Q, W, E, R, D, F, in [`slot`]'s order).
    pub fn cast(self) -> [Key; slot::COUNT] {
        match self {
            Layout::League => [Key::Q, Key::W, Key::E, Key::R, Key::D, Key::F],
            Layout::Wasd => [Key::RightMouse, Key::Shift, Key::E, Key::R, Key::Q, Key::F],
        }
    }

    /// Each slot's key as the HUD names it over the ability.
    pub fn ability_keys(self) -> [&'static str; slot::COUNT] {
        self.cast().map(Key::short)
    }
}

/// A key or button that casts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Q,
    W,
    E,
    R,
    D,
    F,
    Shift,
    RightMouse,
}

impl Key {
    /// Its name over an ability: short, it's drawn small.
    pub fn short(self) -> &'static str {
        match self {
            Key::Q => "Q",
            Key::W => "W",
            Key::E => "E",
            Key::R => "R",
            Key::D => "D",
            Key::F => "F",
            Key::Shift => "Shift",
            Key::RightMouse => "RMB",
        }
    }
}

/// A group of keys on the Controls page, in the order shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Moving,
    Abilities,
    Items,
    Camera,
    Team,
}

impl Group {
    pub const ALL: [Group; 5] = [Group::Moving, Group::Abilities, Group::Items, Group::Camera, Group::Team];

    pub fn title(self) -> &'static str {
        match self {
            Group::Moving => "MOVING AND FIGHTING",
            Group::Abilities => "ABILITIES",
            Group::Items => "ITEMS AND AUGMENTS",
            Group::Camera => "THE CAMERA",
            Group::Team => "THE TEAM",
        }
    }
}

/// The longest a line's keys and what they do may be, in characters: the page's columns hold this
/// much on one line (`el-client/src/menu.rs`).
pub const KEYS_CHARS: usize = 20;
pub const DOES_CHARS: usize = 29;

/// A line of the Controls page: the keys, and what they do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub group: Group,
    pub keys: &'static str,
    pub does: &'static str,
}

const fn b(group: Group, keys: &'static str, does: &'static str) -> Binding {
    Binding { group, keys, does }
}

/// The keys of a layout, cast quickly or with indicators, in [`Group::ALL`]'s order.
pub fn sheet(layout: Layout, indicators: bool) -> Vec<Binding> {
    use Group::*;
    let mut out = Vec::with_capacity(28);
    match layout {
        Layout::League => out.extend([
            b(Moving, "Right click", "Walk there, or attack a unit"),
            b(Moving, "A, then left click", "Attack-move: fight on the way"),
            b(Moving, "S", "Stop"),
            b(Moving, "H", "Hold: fight what comes near"),
            b(Moving, "Left click", "Pick a unit, for its frame"),
        ]),
        Layout::Wasd => out.extend([
            b(Moving, "W A S D", "Walk"),
            b(Moving, "Left click", "Attack the unit under it"),
            b(Moving, "H", "Hold: fight what comes near"),
        ]),
    }
    let (cast, spells, level, cancel) = match layout {
        Layout::League => ("Q W E R", "D F", "Ctrl + Q W E R", "Esc, right click"),
        Layout::Wasd => ("RMB, Shift, E, R", "Q F", "Ctrl + ability key", "C, Esc"),
    };
    if indicators {
        out.push(b(Abilities, cast, "Hold to aim, let go to cast"));
        out.push(b(Abilities, cancel, "Call a held cast off"));
    } else {
        out.push(b(Abilities, cast, "Cast at the cursor"));
    }
    out.extend([
        b(Abilities, spells, "The summoner spells"),
        b(Abilities, level, "Put a point in the ability"),
    ]);
    let reroll = match layout {
        Layout::League => "Shift + 1 2 3",
        Layout::Wasd => "Ctrl + 1 2 3",
    };
    out.extend([
        b(Items, "1 2 3 5 6 7", "Use an item's active"),
        b(Items, "4", "Eat a treat: health, speed"),
        b(Items, "1 2 3", "Take an augment card"),
        b(Items, reroll, "Reroll that card"),
        b(Items, "P", "Shop (platform, or dead)"),
    ]);
    out.extend([
        b(Camera, "Space or F1, held", "Look at your champion"),
        b(Camera, "F2 to F5, held", "Look at an ally"),
        b(Camera, "Y", "Lock the camera, or free it"),
        b(Camera, "Edges, arrows", "Pan the camera"),
        b(Camera, "Middle drag", "Pan the camera"),
        b(Camera, "Wheel", "Zoom"),
        b(Camera, "Minimap", "Left: look. Right: walk"),
    ]);
    out.extend([
        b(Team, "Tab, held", "Scores; surrender from 8:00"),
        b(Team, "G/Alt + left click", "Ping: look here"),
        b(Team, "Ctrl + left click", "Ping: careful"),
        b(Team, "Esc", "This menu"),
    ]);
    out
}

/// What a first match is told, in a strip along the bottom: the least that plays a match.
pub fn essentials(layout: Layout) -> [(&'static str, &'static str); 6] {
    match layout {
        Layout::League => [
            ("Right click", "walk, attack"),
            ("Q W E R", "cast"),
            ("Ctrl + Q W E R", "level up"),
            ("P", "shop"),
            ("Tab", "scores"),
            ("Esc", "every key, settings"),
        ],
        Layout::Wasd => [
            ("W A S D", "walk"),
            ("Left click", "attack"),
            ("RMB Shift E R", "cast"),
            ("Ctrl + key", "level up"),
            ("P", "shop"),
            ("Esc", "every key, settings"),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAYOUTS: [Layout; 2] = [Layout::League, Layout::Wasd];

    #[test]
    fn every_word_is_in_the_font() {
        for l in LAYOUTS {
            for ind in [false, true] {
                for x in sheet(l, ind) {
                    assert!(x.keys.is_ascii() && x.does.is_ascii(), "{x:?}");
                }
            }
            for (k, d) in essentials(l) {
                assert!(k.is_ascii() && d.is_ascii(), "{k}: {d}");
            }
        }
        for g in Group::ALL {
            assert!(g.title().is_ascii());
        }
    }

    #[test]
    fn every_line_is_one_line_on_the_page() {
        for l in LAYOUTS {
            for ind in [false, true] {
                for x in sheet(l, ind) {
                    assert!(x.keys.len() <= KEYS_CHARS && x.does.len() <= DOES_CHARS, "{x:?}");
                }
            }
        }
    }

    #[test]
    fn the_sheet_is_in_its_groups_order_and_has_each() {
        for l in LAYOUTS {
            for ind in [false, true] {
                let s = sheet(l, ind);
                let order: Vec<usize> =
                    s.iter().map(|x| Group::ALL.iter().position(|g| *g == x.group).unwrap()).collect();
                assert!(order.windows(2).all(|w| w[0] <= w[1]), "{l:?}: {order:?}");
                for g in Group::ALL {
                    assert!(s.iter().any(|x| x.group == g), "{l:?} has nothing in {g:?}");
                }
            }
        }
    }

    #[test]
    fn the_sheet_names_the_keys_that_cast() {
        // Each layout's cast keys, as the HUD names them, are on its page: League's Q W E R and
        // D F, WASD's RMB, Shift, E, R and Q F.
        for (l, keys) in [(Layout::League, ["Q W E R", "D F"]), (Layout::Wasd, ["RMB, Shift, E, R", "Q F"])] {
            let s = sheet(l, false);
            for k in keys {
                assert!(s.iter().any(|x| x.keys.contains(k)), "{l:?}: {k}");
            }
        }
        assert_eq!(Layout::League.ability_keys(), ["Q", "W", "E", "R", "D", "F"]);
        assert_eq!(Layout::Wasd.ability_keys(), ["RMB", "Shift", "E", "R", "Q", "F"]);
    }

    #[test]
    fn a_layout_casts_each_slot_with_a_key_of_its_own() {
        for l in LAYOUTS {
            let keys = l.cast();
            for (i, a) in keys.iter().enumerate() {
                assert!(!keys[i + 1..].contains(a), "{l:?}: {a:?} twice");
            }
        }
    }

    #[test]
    fn a_layout_is_its_name() {
        for l in LAYOUTS {
            assert_eq!(Layout::parse(l.name()), Some(l));
        }
        assert_eq!(Layout::parse("dvorak"), None);
        assert_eq!(Layout::of(true), Layout::Wasd);
    }

    #[test]
    fn indicators_say_how_to_call_a_cast_off() {
        for l in LAYOUTS {
            assert!(sheet(l, true).iter().any(|x| x.does.starts_with("Call")));
            assert!(!sheet(l, false).iter().any(|x| x.does.starts_with("Call")));
        }
    }
}
