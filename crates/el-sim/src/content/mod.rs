//! What the game is made of, as data: champions and their abilities, augments, items, and the
//! rules' numbers. An ability is a [`Effect`] (how it reaches: a skillshot, a missile at a unit, an
//! area, a dash...) carrying a short list of [`Op`]s (what it does to what it reaches). Augments and
//! items are stat changes and the same ops on a trigger. A champion needs code of its own only for
//! what this can't say (`docs/DESIGN.md`, "Champions").
//!
//! Numbers are in seconds and units here; the sim turns them into ticks. A change to any of them
//! changes the golden hashes.

pub mod augments;
pub mod champions;
pub mod items;
pub mod rules;

/// Damage types: armour mitigates physical, magic resist magic; true damage goes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmgKind {
    Physical,
    Magic,
    True,
}

/// Who an area or zone touches, relative to its caster's team.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Enemies,
    /// Allied champions, the caster among them.
    Allies,
}

/// What an ability is aimed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A direction: the caster aims at a point, and the effect goes that way (skillshots).
    Dir,
    /// A point on the ground, clamped to range.
    Point,
    /// An enemy unit.
    Enemy,
    /// An allied champion (or the caster).
    Ally,
    /// No aim: the caster.
    Caster,
}

/// One thing an effect does to a unit it reaches. Ranked values are by the ability's rank (1–5;
/// R uses the first three).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    /// Damage: `base[rank] + ad × attack damage + ap × ability power + missing × the target's
    /// missing health`.
    Damage {
        kind: DmgKind,
        base: [f32; 5],
        ad: f32,
        ap: f32,
        missing: f32,
    },
    Heal {
        base: [f32; 5],
        ap: f32,
    },
    Shield {
        base: [f32; 5],
        ap: f32,
        secs: f32,
    },
    Stun(f32),
    Root(f32),
    Silence(f32),
    /// Knocked up: stunned, and no Combo Breaker cleanses it early.
    Airborne(f32),
    Slow {
        share: f32,
        secs: f32,
    },
    /// Pushed straight away from the caster.
    Knockback(f32),
    /// Pulled to the caster's side.
    Pull,
    /// A buff on the unit reached.
    Buff {
        kind: crate::units::BuffKind,
        secs: f32,
        value: f32,
    },
    /// A buff on the caster.
    SelfBuff {
        kind: crate::units::BuffKind,
        secs: f32,
        value: f32,
    },
    /// Seen by the caster's enemies for a while.
    Reveal(f32),
}

/// How an ability reaches what it does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    /// A skillshot: a missile flying from the caster toward the aim. It stops at the first enemy
    /// unless it pierces.
    Line { speed: f32, width: f32, pierce: bool, ops: &'static [Op] },
    /// A missile at a unit that can't miss.
    Homing { speed: f32, ops: &'static [Op] },
    /// At once, on the unit aimed at.
    Strike { ops: &'static [Op] },
    /// After `delay`, everything of `filter` within `radius` of the aim (or of the caster, when
    /// the ability's target is [`Target::Caster`]).
    Area { radius: f32, delay: f32, filter: Filter, ops: &'static [Op] },
    /// For `secs`, every `period`, everything of `filter` within `radius`; on the caster if it
    /// `follows` them.
    Zone { radius: f32, secs: f32, period: f32, filter: Filter, follows: bool, ops: &'static [Op] },
    /// The caster dashes toward the aim (at most the ability's range), or to the unit aimed at,
    /// and on arrival does `ops` to enemies within `radius` (0: only the unit dashed to).
    Dash { speed: f32, radius: f32, ops: &'static [Op] },
    /// The caster is there at once, and does `ops` to enemies within `radius` of where it lands.
    Blink { radius: f32, ops: &'static [Op] },
    /// `ops` on the caster.
    OnSelf { ops: &'static [Op] },
}

impl Effect {
    pub fn ops(&self) -> &'static [Op] {
        match *self {
            Effect::Line { ops, .. }
            | Effect::Homing { ops, .. }
            | Effect::Strike { ops }
            | Effect::Area { ops, .. }
            | Effect::Zone { ops, .. }
            | Effect::Dash { ops, .. }
            | Effect::Blink { ops, .. }
            | Effect::OnSelf { ops } => ops,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AbilityDef {
    pub name: &'static str,
    pub target: Target,
    /// Units, from the caster's centre.
    pub range: f32,
    pub cooldown: [f32; 5],
    pub cost: [f32; 5],
    /// Seconds from the cast to the effect.
    pub windup: f32,
    pub effect: Effect,
}

/// Stat changes an item or augment brings.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StatMods {
    pub hp: f32,
    pub mana: f32,
    pub ad: f32,
    pub ap: f32,
    pub armor: f32,
    pub mr: f32,
    /// Attack speed, as a share of the champion's base.
    pub attack_speed: f32,
    pub move_speed: f32,
    /// Move speed, as a share.
    pub move_share: f32,
    pub haste: f32,
    pub lifesteal: f32,
    pub hp_regen: f32,
}

impl StatMods {
    pub const ZERO: StatMods = StatMods {
        hp: 0.0,
        mana: 0.0,
        ad: 0.0,
        ap: 0.0,
        armor: 0.0,
        mr: 0.0,
        attack_speed: 0.0,
        move_speed: 0.0,
        move_share: 0.0,
        haste: 0.0,
        lifesteal: 0.0,
        hp_regen: 0.0,
    };

    pub fn add(&mut self, o: &StatMods) {
        self.hp += o.hp;
        self.mana += o.mana;
        self.ad += o.ad;
        self.ap += o.ap;
        self.armor += o.armor;
        self.mr += o.mr;
        self.attack_speed += o.attack_speed;
        self.move_speed += o.move_speed;
        self.move_share += o.move_share;
        self.haste += o.haste;
        self.lifesteal += o.lifesteal;
        self.hp_regen += o.hp_regen;
    }
}

/// A champion: base stats and their growth a level, its attack, and its four abilities.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChampDef {
    pub name: &'static str,
    pub title: &'static str,
    pub base: crate::units::Stats,
    /// Added each level past the first.
    pub growth: crate::units::Stats,
    pub radius: f32,
    /// Its auto attack flies (at this speed); 0: it strikes in melee.
    pub missile_speed: f32,
    /// The share of an attack's period spent winding it up.
    pub windup: f32,
    pub abilities: [AbilityDef; 4],
}

/// The ranks of a value's five, for ops that don't scale.
pub const fn flat(v: f32) -> [f32; 5] {
    [v; 5]
}
