//! What every ability looks and sounds like (`docs/FX.md`, "The looks"). A look is derived from the
//! ability itself, as the sim derives what it does: how it reaches (a skillshot, a blast that lands
//! late, a field, a dash...) says which parts it has, its ops say how much it matters (Riot's four
//! things: damage, crowd control, how dodgeable it is, how much it changes the fight), and its
//! champion's family gives it a palette and a sound. A champion's look differs from another's of
//! the same family by what its abilities do.
//!
//! Importance is rated for each viewer, as Riot's artists rate it: the caster, the caster's allies,
//! the caster's enemies, each 1 to 10. The rating sets an effect's size and brightness, its sound's
//! place on the mixer's ladder, and whether a part is drawn for that viewer at all.

use el_sim::content::champions::{BLINK_SLOT, CHAMPIONS, MARK_SLOT, ability};
use el_sim::content::{AbilityDef, Effect, Filter, Op, Role, Target};
use el_sim::missile::Source;

use super::sound::{Part, Sfx};

/// Champions who share a palette and a sound, as Riot's regions share sonic traits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Ember,
    Frost,
    Tide,
    Wind,
    Dusk,
    Growth,
    Stone,
    Bronze,
    Steel,
}

/// Each champion's family, in the pool's order (`CHAMPIONS`).
const FAMILY_OF: [Family; 20] = [
    Family::Steel,  // Vessa
    Family::Ember,  // Orrin
    Family::Stone,  // Brakka
    Family::Steel,  // Kesh
    Family::Bronze, // Liora
    Family::Tide,   // Talmar
    Family::Dusk,   // Seren
    Family::Bronze, // Hollis
    Family::Steel,  // Vire
    Family::Bronze, // Oda
    Family::Steel,  // Marret
    Family::Frost,  // Nimra
    Family::Stone,  // Gorum
    Family::Dusk,   // Sable
    Family::Bronze, // Pell
    Family::Growth, // Ulla
    Family::Stone,  // Torek
    Family::Wind,   // Sefa
    Family::Ember,  // Brun
    Family::Ember,  // Mira
];

const _: () = assert!(FAMILY_OF.len() == CHAMPIONS.len());

/// A colour, linear, at a brightness past 1 for what glows.
pub type Rgb = [f32; 3];

/// A family's colours: what the part that does the gameplay is drawn in (its "area of focus":
/// the most value, saturation and detail), what's round it, and its smoke or dust.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub core: Rgb,
    pub edge: Rgb,
    pub smoke: Rgb,
}

impl Family {
    pub const COUNT: usize = 9;
    pub const ALL: [Family; Family::COUNT] = [
        Family::Ember,
        Family::Frost,
        Family::Tide,
        Family::Wind,
        Family::Dusk,
        Family::Growth,
        Family::Stone,
        Family::Bronze,
        Family::Steel,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Family::Ember => "ember",
            Family::Frost => "frost",
            Family::Tide => "tide",
            Family::Wind => "wind",
            Family::Dusk => "dusk",
            Family::Growth => "growth",
            Family::Stone => "stone",
            Family::Bronze => "bronze",
            Family::Steel => "steel",
        }
    }

    pub fn of(champ: u8) -> Family {
        FAMILY_OF[usize::from(champ) % FAMILY_OF.len()]
    }

    pub fn palette(self) -> Palette {
        let p = |core: Rgb, edge: Rgb, smoke: Rgb| Palette { core, edge, smoke };
        match self {
            Family::Ember => p([4.0, 1.6, 0.35], [1.6, 0.35, 0.06], [0.16, 0.10, 0.08]),
            Family::Frost => p([1.8, 3.0, 4.2], [0.35, 0.9, 1.8], [0.55, 0.65, 0.78]),
            Family::Tide => p([0.7, 2.6, 2.9], [0.10, 0.55, 0.9], [0.70, 0.85, 0.90]),
            Family::Wind => p([2.6, 3.2, 3.0], [0.6, 1.4, 1.1], [0.70, 0.78, 0.76]),
            Family::Dusk => p([2.4, 1.3, 4.2], [0.65, 0.18, 1.5], [0.10, 0.06, 0.16]),
            Family::Growth => p([1.4, 3.6, 0.9], [0.3, 1.2, 0.2], [0.30, 0.38, 0.20]),
            Family::Stone => p([2.6, 1.9, 1.1], [0.9, 0.55, 0.3], [0.42, 0.36, 0.30]),
            Family::Bronze => p([4.0, 3.0, 1.3], [1.6, 0.9, 0.25], [0.45, 0.38, 0.28]),
            Family::Steel => p([3.4, 3.5, 3.8], [1.1, 1.2, 1.5], [0.40, 0.42, 0.46]),
        }
    }
}

/// The colours that don't belong to a champion: the summoner spells, the world's.
pub mod world {
    use super::Palette;
    /// Blink's arcane blue, Tag's snow.
    pub const BLINK: Palette =
        Palette { core: [1.6, 2.6, 4.5], edge: [0.4, 0.7, 2.0], smoke: [0.5, 0.6, 0.8] };
    pub const SNOW: Palette =
        Palette { core: [3.0, 3.2, 3.5], edge: [1.2, 1.4, 1.8], smoke: [0.8, 0.85, 0.9] };
    /// A relic's green, an augment's violet spark.
    pub const RELIC: Palette =
        Palette { core: [1.0, 4.0, 1.6], edge: [0.3, 1.6, 0.6], smoke: [0.4, 0.6, 0.5] };
    pub const AUGMENT: Palette =
        Palette { core: [3.2, 1.8, 4.0], edge: [1.2, 0.5, 1.6], smoke: [0.4, 0.3, 0.5] };
    /// A team's own light (its turrets' lanterns, its minions'), Dawn's and Dusk's.
    pub const TEAM: [Palette; 2] = [
        Palette { core: [1.1, 2.4, 4.6], edge: [0.25, 0.7, 2.0], smoke: [0.35, 0.45, 0.6] },
        Palette { core: [4.6, 1.4, 0.9], edge: [2.0, 0.35, 0.2], smoke: [0.55, 0.38, 0.35] },
    ];
    /// What every viewer reads the same, whoever caused it (`docs/FX.md`, rule: statuses are the
    /// flag's, not the champion's).
    pub const STUN: Palette =
        Palette { core: [4.0, 3.6, 1.2], edge: [1.5, 1.2, 0.3], smoke: [0.5, 0.5, 0.4] };
    pub const ROOT: Palette =
        Palette { core: [1.6, 3.2, 1.0], edge: [0.5, 1.2, 0.3], smoke: [0.3, 0.35, 0.25] };
    pub const SILENCE: Palette =
        Palette { core: [2.6, 1.4, 3.6], edge: [0.9, 0.4, 1.3], smoke: [0.3, 0.25, 0.35] };
    pub const SLOW: Palette =
        Palette { core: [1.2, 2.2, 3.6], edge: [0.3, 0.7, 1.4], smoke: [0.5, 0.6, 0.7] };
    pub const SHIELD: Palette =
        Palette { core: [2.8, 3.0, 3.4], edge: [1.0, 1.1, 1.4], smoke: [0.6, 0.6, 0.7] };
    pub const HEAL: Palette =
        Palette { core: [1.0, 4.0, 1.4], edge: [0.3, 1.5, 0.4], smoke: [0.4, 0.6, 0.45] };
    pub const EMPOWER: Palette =
        Palette { core: [4.2, 2.0, 0.8], edge: [1.6, 0.6, 0.2], smoke: [0.5, 0.4, 0.3] };
    pub const HASTE: Palette =
        Palette { core: [2.4, 3.6, 3.2], edge: [0.7, 1.3, 1.1], smoke: [0.6, 0.7, 0.7] };
    pub const IMMUNE: Palette =
        Palette { core: [4.4, 3.8, 2.0], edge: [1.8, 1.4, 0.6], smoke: [0.6, 0.55, 0.45] };
}

/// Who is looking, to whoever caused an effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// It's the viewer's own.
    Own,
    Ally,
    Enemy,
}

/// How much an effect matters to each viewer, 1 to 10.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ratings {
    pub own: u8,
    pub ally: u8,
    pub enemy: u8,
}

impl Ratings {
    pub const fn new(own: u8, ally: u8, enemy: u8) -> Self {
        Ratings { own, ally, enemy }
    }

    pub fn of(&self, side: Side) -> u8 {
        match side {
            Side::Own => self.own,
            Side::Ally => self.ally,
            Side::Enemy => self.enemy,
        }
    }
}

/// How an effect reaches, as far as drawing it goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reach {
    /// A skillshot: its lane `width` wide, `range` long.
    Line {
        width: f32,
        range: f32,
    },
    Homing,
    Strike,
    /// An area that lands after `delay` s (0: at once), `radius` wide.
    Area {
        radius: f32,
        delay: f32,
    },
    /// A field that lasts.
    Field {
        radius: f32,
    },
    Dash,
    Blink,
    OnSelf,
    /// An auto attack (`ranged`: it flies).
    Auto {
        ranged: bool,
    },
    Turret,
    /// What the world does: a relic, an augment's answer, the platform.
    World,
}

/// The crowd control an ability brings, strongest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Control {
    Slow,
    Silence,
    Displace,
    Root,
    Stun,
    Airborne,
}

/// What a source looks and sounds like.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// Its champion's family, if it's a champion's.
    pub family: Option<Family>,
    pub palette: Palette,
    pub ratings: Ratings,
    pub reach: Reach,
    /// The strongest crowd control it brings.
    pub control: Option<Control>,
    /// It helps (heals, shields, buffs allies) rather than harms.
    pub friendly: bool,
    /// An ultimate.
    pub ult: bool,
    /// Its sound for each part (`Part`'s order), if that part sounds.
    pub sounds: [Option<Sfx>; Part::COUNT],
}

impl Look {
    pub fn sound(&self, part: Part) -> Option<Sfx> {
        self.sounds[part as usize]
    }
}

/// What a champion's ability amounts to, by its ops: damage at its middle rank (a ratio counted as
/// a hundred of the stat it scales with), the strongest control, and whether it helps.
fn weigh(a: &AbilityDef) -> (f32, Option<Control>, bool) {
    let mut damage = 0.0;
    let mut control = None;
    let mut friendly = false;
    let more = |c: Control, control: &mut Option<Control>| {
        if control.is_none_or(|k| c > k) {
            *control = Some(c);
        }
    };
    for op in a.effect.ops() {
        match *op {
            Op::Damage { base, ad, ap, missing, .. } => {
                damage += base[2] + 100.0 * (ad + ap) + 300.0 * missing
            }
            Op::Heal { .. } | Op::Shield { .. } => friendly = true,
            Op::Stun(_) => more(Control::Stun, &mut control),
            Op::Root(_) => more(Control::Root, &mut control),
            Op::Silence(_) => more(Control::Silence, &mut control),
            Op::Airborne(_) => more(Control::Airborne, &mut control),
            Op::Slow { .. } => more(Control::Slow, &mut control),
            Op::Knockback(_) | Op::Pull => more(Control::Displace, &mut control),
            Op::Buff { value, .. } => friendly |= value > 0.0,
            Op::SelfBuff { .. } | Op::Reveal(_) => {}
        }
    }
    let allies = matches!(
        a.effect,
        Effect::Area { filter: Filter::Allies, .. } | Effect::Zone { filter: Filter::Allies, .. }
    ) || a.target == Target::Ally;
    (damage, control, friendly || (allies && damage == 0.0))
}

/// How much an ability matters, by Riot's four things (`docs/FX.md`, "What Riot says", 2).
fn rate(a: &AbilityDef, damage: f32, control: Option<Control>, friendly: bool, ult: bool) -> Ratings {
    let hurt = (damage / 120.0).min(3.0);
    let cc = match control {
        Some(Control::Airborne | Control::Stun) => 3.0,
        Some(Control::Root | Control::Displace) => 2.5,
        Some(Control::Silence) => 1.5,
        Some(Control::Slow) => 1.0,
        None => 0.0,
    };
    let dodge = match a.effect {
        Effect::Line { .. } => 1.0,
        Effect::Area { delay, .. } if delay > 0.0 => 1.5,
        _ => 0.0,
    };
    let big = if ult { 2.0 } else { 0.0 };
    let clamp = |v: f32, lo: u8, hi: u8| (v.round() as u8).clamp(lo, hi);
    if friendly {
        // What helps matters to whom it helps, and little to the other side.
        let help = 4.0 + big;
        Ratings::new(clamp(help, 3, 9), clamp(help, 3, 9), clamp(1.0 + big, 1, 5))
    } else {
        let threat = 2.0 + hurt + cc + dodge + big;
        Ratings::new(clamp(threat - 1.0, 3, 9), clamp(threat - 3.0, 1, 6), clamp(threat, 2, 10))
    }
}

/// An ability of champion `champ`'s, in `slot` (Q, W, E, R; then Blink and Tag).
pub fn ability_look(champ: u8, slot: u8) -> Look {
    let a = ability(champ, slot);
    if slot == BLINK_SLOT {
        let mut sounds = [None; Part::COUNT];
        sounds[Part::Dash as usize] = Some(Sfx::Blink);
        return Look {
            family: None,
            palette: world::BLINK,
            ratings: Ratings::new(5, 2, 6),
            reach: Reach::Blink,
            control: None,
            friendly: false,
            ult: false,
            sounds,
        };
    }
    if slot == MARK_SLOT {
        let mut sounds = [None; Part::COUNT];
        sounds[Part::Launch as usize] = Some(Sfx::TagThrow);
        sounds[Part::Impact as usize] = Some(Sfx::TagHit);
        sounds[Part::Dash as usize] = Some(Sfx::TagDash);
        return Look {
            family: None,
            palette: world::SNOW,
            ratings: Ratings::new(5, 2, 6),
            reach: Reach::Line { width: 60.0, range: a.range },
            control: None,
            friendly: false,
            ult: false,
            sounds,
        };
    }
    let family = Family::of(champ);
    let f = family as usize;
    let ult = slot == 3;
    let (damage, control, friendly) = weigh(a);
    let ratings = rate(a, damage, control, friendly, ult);
    let mut sounds = [None; Part::COUNT];
    let mut put = |p: Part| sounds[p as usize] = Some(Sfx::family(f, p));
    put(Part::Cast);
    if ult {
        put(Part::Ult);
    }
    let reach = match a.effect {
        Effect::Line { width, .. } => {
            put(Part::Launch);
            put(Part::Impact);
            Reach::Line { width, range: a.range }
        }
        Effect::Homing { .. } => {
            put(Part::Launch);
            put(Part::Impact);
            Reach::Homing
        }
        Effect::Strike { .. } => {
            put(Part::Impact);
            Reach::Strike
        }
        Effect::Area { radius, delay, .. } => {
            if delay > 0.0 {
                put(Part::Windup);
            }
            put(Part::Blast);
            Reach::Area { radius, delay }
        }
        Effect::Zone { radius, .. } => {
            put(Part::Blast);
            Reach::Field { radius }
        }
        Effect::Dash { .. } => {
            put(Part::Dash);
            if !a.effect.ops().is_empty() {
                put(Part::Impact);
            }
            Reach::Dash
        }
        Effect::Blink { .. } => {
            put(Part::Dash);
            put(Part::Impact);
            Reach::Blink
        }
        Effect::OnSelf { .. } => Reach::OnSelf,
    };
    if friendly || matches!(a.effect, Effect::OnSelf { .. }) {
        put(Part::Buff);
    }
    Look { family: Some(family), palette: family.palette(), ratings, reach, control, friendly, ult, sounds }
}

/// What `source` looks and sounds like. `champ` is its owner's champion, when the owner is one
/// the viewer knows (an auto's look is its champion's); `team` its owner's team.
pub fn look(source: Source, champ: Option<u8>, team: u8) -> Look {
    let mut sounds = [None; Part::COUNT];
    let none = |palette, ratings, reach| Look {
        family: None,
        palette,
        ratings,
        reach,
        control: None,
        friendly: false,
        ult: false,
        sounds: [None; Part::COUNT],
    };
    match source {
        Source::Ability { champ, slot } => ability_look(champ, slot),
        Source::Auto => match champ {
            Some(c) => {
                let d = &CHAMPIONS[usize::from(c) % CHAMPIONS.len()];
                let family = Family::of(c);
                let ranged = d.missile_speed > 0.0;
                if ranged {
                    sounds[Part::Launch as usize] = Some(Sfx::family(family as usize, Part::Launch));
                    sounds[Part::Impact as usize] = Some(match d.role {
                        Role::Marksman => Sfx::family(family as usize, Part::Impact),
                        _ => Sfx::AutoHit,
                    });
                } else {
                    sounds[Part::Launch as usize] = Some(Sfx::AutoSwing);
                    sounds[Part::Impact as usize] = Some(Sfx::AutoHit);
                }
                Look {
                    sounds,
                    family: Some(family),
                    ..none(family.palette(), Ratings::new(3, 2, 3), Reach::Auto { ranged })
                }
            }
            // A minion's, or a champion the viewer can't see.
            None => {
                sounds[Part::Impact as usize] = Some(Sfx::MinionHit);
                Look {
                    sounds,
                    ..none(
                        world::TEAM[usize::from(team.min(1))],
                        Ratings::new(2, 1, 2),
                        Reach::Auto { ranged: false },
                    )
                }
            }
        },
        Source::Turret => {
            sounds[Part::Launch as usize] = Some(Sfx::TurretShot);
            sounds[Part::Impact as usize] = Some(Sfx::TurretHit);
            Look {
                sounds,
                ..none(world::TEAM[usize::from(team.min(1))], Ratings::new(6, 3, 6), Reach::Turret)
            }
        }
        Source::Fountain => none(world::TEAM[usize::from(team.min(1))], Ratings::new(5, 2, 5), Reach::World),
        Source::Relic => {
            sounds[Part::Buff as usize] = Some(Sfx::RelicTake);
            Look { sounds, friendly: true, ..none(world::RELIC, Ratings::new(5, 4, 3), Reach::World) }
        }
        Source::Augment(_) => {
            sounds[Part::Impact as usize] = Some(Sfx::AugmentProc);
            Look { sounds, ..none(world::AUGMENT, Ratings::new(3, 2, 3), Reach::World) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ability_has_a_look_and_sounds_for_its_parts() {
        for champ in 0..CHAMPIONS.len() as u8 {
            for slot in 0..=MARK_SLOT {
                let l = ability_look(champ, slot);
                let a = ability(champ, slot);
                let name = format!("{} {}", CHAMPIONS[usize::from(champ)].name, a.name);
                for r in [l.ratings.own, l.ratings.ally, l.ratings.enemy] {
                    assert!((1..=10).contains(&r), "{name}: ratings {:?}", l.ratings);
                }
                // Something is heard when it's cast or when it goes.
                assert!(
                    l.sound(Part::Cast).or(l.sound(Part::Dash)).or(l.sound(Part::Launch)).is_some(),
                    "{name} makes no sound"
                );
                // A blast that lands late warns by ear (Riot: dangerous spells warn before they land).
                if let Effect::Area { delay, .. } = a.effect
                    && delay > 0.0
                {
                    assert!(l.sound(Part::Windup).is_some(), "{name} lands late and gives no warning");
                }
                if slot == 3 {
                    assert!(l.ult && l.sound(Part::Ult).is_some(), "{name} is an ultimate");
                }
                for s in l.sounds.into_iter().flatten() {
                    assert!(!s.def().optional, "{name}'s sound {} is one we make", s.def().name);
                }
            }
        }
    }

    #[test]
    fn what_matters_more_is_rated_higher() {
        // An ultimate with a stun to its enemies, above a plain poke.
        let stun_ult = (0..CHAMPIONS.len() as u8)
            .map(|c| ability_look(c, 3))
            .filter(|l| l.control >= Some(Control::Stun))
            .map(|l| l.ratings.enemy)
            .max()
            .unwrap();
        let pokes = (0..CHAMPIONS.len() as u8)
            .map(|c| ability_look(c, 0))
            .filter(|l| l.control.is_none() && !l.friendly)
            .map(|l| l.ratings.enemy)
            .min()
            .unwrap();
        assert!(stun_ult > pokes + 2, "a stunning ult {stun_ult}, a poke {pokes}");
        // What helps is rated by those it helps.
        for c in 0..CHAMPIONS.len() as u8 {
            for s in 0..4 {
                let l = ability_look(c, s);
                if l.friendly {
                    assert!(l.ratings.ally >= l.ratings.enemy, "{:?}", l);
                } else {
                    assert!(l.ratings.enemy >= l.ratings.ally, "{:?}", l);
                }
            }
        }
    }

    #[test]
    fn every_source_has_a_look() {
        for team in 0..2 {
            for s in [Source::Auto, Source::Turret, Source::Fountain, Source::Relic, Source::Augment(3)] {
                let _ = look(s, None, team);
            }
            for c in 0..CHAMPIONS.len() as u8 {
                let l = look(Source::Auto, Some(c), team);
                assert!(
                    l.sound(Part::Impact).is_some(),
                    "{}'s auto lands with a sound",
                    CHAMPIONS[usize::from(c)].name
                );
            }
        }
    }
}
