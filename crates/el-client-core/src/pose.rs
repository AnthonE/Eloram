//! What a champion's figure does (`docs/CHAMPIONS.md`, "The runtime"): which clip it plays, how far
//! into it, and how fast, from what the client knows of it each frame: its flags, the ability it's
//! casting, how fast it moves, whether it lives; and the casts it began and the hits it took, from
//! the events.
//!
//! The clips are timed to the game, never the other way round (Riot: an animation is a tell of
//! what the game does and never "exceeds" its time): an attack's clip is played so its strike lands
//! as the server's windup ends, a cast's so its release lands as the ability's windup does. The own
//! champion's windup is known to the tick; anyone else's is taken from its champion, its level and
//! the last one seen. A champion that moves cancels into its run, as an order cancels a windup.
//!
//! What makes it read as hand-keyed rather than captured (`docs/CHAMPIONS.md`, "The motion"):
//! - **The timing's shape.** A strike's wind-up is eased in (slow, then fast into the hit:
//!   `Feel::snap`), the strike holds on its contact a few frames (`Feel::hold_*`), and the
//!   follow-through plays on at its own pace. The strike still lands as the windup ends.
//! - **Two attacks**, alternated, as League's champions swing (`Kit::attacks`).
//! - **The legs run while the arms cast**: a cast walked through plays on the upper body over the
//!   run (`Pose::legs`).
//! - **A flinch** over the upper body when it's hit, a reel when the hit is heavy (`Pose::flinch`).
//! - **Beats**: the moments the motion layer (`motion`) and the effects key off (a strike, a
//!   landing, a dash, a fall).
//!
//! The minions are posed the same way, with fewer clips: they walk, swing, flinch and fall, timed to
//! their own windups (`el_sim::content::rules`), and never cast.
//!
//! Nothing here touches the sim: the figures are drawn from what the client was told.

use el_proto::quant::dequantize_centered;
use el_proto::unit::{QUnit, VEL_BITS, VEL_MAX, flags};
use el_sim::content::champions::CHAMPIONS;
use el_sim::content::rules::{MINION_WINDUP, minion_def};
use el_sim::content::{Effect, Role, Target};
use el_sim::units::Kind;

use crate::motion::Feel;

/// A clip a champion's model carries (`web/assets/models/champions/<name>.glb`, by name). The
/// first twenty are Meshy's library's (`art/meshy.json`); the rest are Quaternius's (CC0,
/// `art/champions.json`, `ual`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Clip {
    Idle,
    Run,
    /// A flinch; looped, being stunned.
    Hit,
    Death,
    /// Thrown up and coming down.
    Airborne,
    Roll,
    /// A charge forward: a dash, looped.
    Charge,
    Victory,
    /// A blade raised high: a buff to oneself.
    Shout,
    /// Both hands to the ground.
    Slam,
    /// A great blade stroke.
    Judgment,
    Slash,
    Thrust,
    /// Overhead, both hands.
    Smash,
    Punch,
    /// From the hip.
    Shoot,
    /// Both hands pushed out.
    Bolt,
    /// One hand, a flick.
    Flick,
    /// One hand thrust out to aim.
    Point,
    /// Gathered low, then released high.
    Channel,
    /// A hit taken in the chest: played over the upper body.
    Flinch,
    /// A heavy hit, to the head.
    Reel,
    /// A rising cut from a guard, and back to it.
    Cut,
    /// A wide sweep, and back.
    Sweep,
    /// A step in and a swing.
    Lunge,
    /// The left fist.
    Jab,
    /// The right fist across.
    Cross,
    /// A hook, and back.
    Hook,
    /// One hand, a shot.
    Fire,
    /// A hand raised, a spell sent from it, lowered.
    Spell,
    /// A step and an overhand throw.
    Throw,
    /// Standing in a blade's guard.
    Guard,
    /// Standing behind a shield.
    Brace,
    /// Crouched, ready.
    Crouch,
    /// Standing with a weapon raised in both hands.
    Aim,
    /// Standing with a light held up in the left hand.
    Torch,
    /// Standing with a light held out in the right.
    Lamp,
    /// Standing with its arms folded.
    Arms,
}

/// A clip as it ships: its length (s), when its strike or release lands (s in), and whether it
/// loops.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Info {
    pub name: &'static str,
    pub length: f32,
    pub strike: f32,
    pub looped: bool,
}

const fn info(name: &'static str, length: f32, strike: f32, looped: bool) -> Info {
    Info { name, length, strike, looped }
}

impl Clip {
    pub const ALL: [Clip; 38] = [
        Clip::Idle,
        Clip::Run,
        Clip::Hit,
        Clip::Death,
        Clip::Airborne,
        Clip::Roll,
        Clip::Charge,
        Clip::Victory,
        Clip::Shout,
        Clip::Slam,
        Clip::Judgment,
        Clip::Slash,
        Clip::Thrust,
        Clip::Smash,
        Clip::Punch,
        Clip::Shoot,
        Clip::Bolt,
        Clip::Flick,
        Clip::Point,
        Clip::Channel,
        Clip::Flinch,
        Clip::Reel,
        Clip::Cut,
        Clip::Sweep,
        Clip::Lunge,
        Clip::Jab,
        Clip::Cross,
        Clip::Hook,
        Clip::Fire,
        Clip::Spell,
        Clip::Throw,
        Clip::Guard,
        Clip::Brace,
        Clip::Crouch,
        Clip::Aim,
        Clip::Torch,
        Clip::Lamp,
        Clip::Arms,
    ];

    /// Its name in the models and its timing, as `art/tools/champions.mjs` cut it
    /// (`art/champions.json`, `clips` and `ual`). A swing's strike is where the weapon's tip (or
    /// the hand) moves fastest, as `champions.mjs --ual` measures it (the slash's and the slam's
    /// were a tenth of a second early, until the anim stage showed the blade still winding up).
    pub const fn info(self) -> Info {
        match self {
            Clip::Idle => info("idle", 4.0, 0.0, true),
            Clip::Run => info("run", 0.667, 0.0, true),
            Clip::Hit => info("hit", 1.633, 0.0, true),
            Clip::Death => info("death", 1.9, 0.0, false),
            Clip::Airborne => info("airborne", 0.8, 0.0, false),
            Clip::Roll => info("roll", 1.3, 0.0, false),
            Clip::Charge => info("charge", 0.633, 0.0, true),
            Clip::Victory => info("victory", 3.2, 0.0, true),
            Clip::Shout => info("shout", 2.1, 1.25, false),
            Clip::Slam => info("slam", 2.0, 1.12, false),
            Clip::Judgment => info("judgment", 2.1, 0.7, false),
            Clip::Slash => info("slash", 1.3, 0.69, false),
            Clip::Thrust => info("thrust", 1.2, 0.73, false),
            Clip::Smash => info("smash", 1.2, 0.93, false),
            Clip::Punch => info("punch", 1.4, 0.53, false),
            Clip::Shoot => info("shoot", 2.0, 0.85, false),
            Clip::Bolt => info("bolt", 1.6, 0.7, false),
            Clip::Flick => info("flick", 1.5, 0.5, false),
            Clip::Point => info("point", 2.2, 1.45, false),
            Clip::Channel => info("channel", 2.667, 1.8, false),
            Clip::Flinch => info("flinch", 0.333, 0.0, false),
            Clip::Reel => info("reel", 0.433, 0.0, false),
            Clip::Cut => info("cut", 1.4, 0.25, false),
            Clip::Sweep => info("sweep", 1.567, 0.25, false),
            Clip::Lunge => info("lunge", 1.533, 0.41, false),
            Clip::Jab => info("jab", 0.867, 0.2, false),
            Clip::Cross => info("cross", 1.0, 0.27, false),
            Clip::Hook => info("hook", 1.067, 0.25, false),
            Clip::Fire => info("fire", 0.633, 0.05, false),
            Clip::Spell => info("spell", 1.467, 0.58, false),
            Clip::Throw => info("throw", 1.333, 0.39, false),
            Clip::Guard => info("guard", 1.667, 0.0, true),
            Clip::Brace => info("brace", 2.5, 0.0, true),
            Clip::Crouch => info("crouch", 2.933, 0.0, true),
            Clip::Aim => info("aim", 1.667, 0.0, true),
            Clip::Torch => info("torch", 1.267, 0.0, true),
            Clip::Lamp => info("lamp", 2.5, 0.0, true),
            Clip::Arms => info("arms", 2.5, 0.0, true),
        }
    }

    pub const fn name(self) -> &'static str {
        self.info().name
    }

    /// Its index in [`Clip::ALL`].
    pub fn index(self) -> usize {
        Clip::ALL.iter().position(|&c| c == self).unwrap_or(0)
    }

    /// A swing up close: what a trail follows (`el-client/src/champions.rs`).
    pub const fn melee(self) -> bool {
        matches!(
            self,
            Clip::Slash
                | Clip::Thrust
                | Clip::Smash
                | Clip::Punch
                | Clip::Judgment
                | Clip::Cut
                | Clip::Sweep
                | Clip::Lunge
                | Clip::Jab
                | Clip::Cross
                | Clip::Hook
        )
    }
}

/// The clips a champion plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kit {
    /// What it does standing.
    pub idle: Clip,
    /// Its attacks, one after the other.
    pub attacks: [Clip; 2],
    /// Q, W, E and R.
    pub casts: [Clip; 4],
    /// While its own dash carries it.
    pub dash: Clip,
}

impl Kit {
    /// Every clip it plays, each once: what its model needs.
    pub fn clips(&self) -> Vec<Clip> {
        let mut out = vec![
            self.idle,
            Clip::Run,
            Clip::Hit,
            Clip::Death,
            Clip::Airborne,
            Clip::Victory,
            Clip::Flinch,
            Clip::Reel,
        ];
        for c in self.attacks.into_iter().chain([self.dash]).chain(self.casts) {
            if !out.contains(&c) {
                out.push(c);
            }
        }
        out
    }
}

/// Each champion's two attacks, by what it holds (`art/champions.json`, `held`), in [`CHAMPIONS`]'
/// order.
const ATTACKS: [[Clip; 2]; 20] = [
    [Clip::Shoot, Clip::Fire],  // Vessa: a crossbow
    [Clip::Bolt, Clip::Spell],  // Orrin
    [Clip::Punch, Clip::Hook],  // Brakka: a hook on a chain, and his fists
    [Clip::Thrust, Clip::Jab],  // Kesh: a knife in each hand
    [Clip::Flick, Clip::Spell], // Liora
    [Clip::Thrust, Clip::Cut],  // Talmar: a trident
    [Clip::Bolt, Clip::Spell],  // Seren
    [Clip::Smash, Clip::Lunge], // Hollis: a mace
    [Clip::Slash, Clip::Sweep], // Vire: a greatsword
    [Clip::Flick, Clip::Spell], // Oda
    [Clip::Shoot, Clip::Fire],  // Marret: a blunderbuss
    [Clip::Bolt, Clip::Spell],  // Nimra
    [Clip::Smash, Clip::Hook],  // Gorum: his fists
    [Clip::Flick, Clip::Spell], // Sable
    [Clip::Cut, Clip::Slash],   // Pell: a sword
    [Clip::Flick, Clip::Spell], // Ulla
    [Clip::Bolt, Clip::Spell],  // Torek
    [Clip::Cut, Clip::Sweep],   // Sefa: a crescent blade
    [Clip::Smash, Clip::Lunge], // Brun: a forge hammer
    [Clip::Flick, Clip::Spell], // Mira
];

/// How each stands, by what it holds: a blade's guard, a shield, a crouch, a weapon raised, a light
/// held up; Meshy's own idle for the rest.
const IDLES: [Clip; 20] = [
    Clip::Aim,    // Vessa
    Clip::Idle,   // Orrin
    Clip::Idle,   // Brakka
    Clip::Crouch, // Kesh
    Clip::Torch,  // Liora: a lantern in her left hand
    Clip::Guard,  // Talmar
    Clip::Idle,   // Seren
    Clip::Idle,   // Hollis
    Clip::Guard,  // Vire
    Clip::Idle,   // Oda
    Clip::Aim,    // Marret
    Clip::Idle,   // Nimra
    Clip::Arms,   // Gorum
    Clip::Torch,  // Sable: her shade lantern, left-handed
    Clip::Brace,  // Pell: his shield
    Clip::Idle,   // Ulla
    Clip::Idle,   // Torek
    Clip::Guard,  // Sefa
    Clip::Idle,   // Brun
    Clip::Lamp,   // Mira: her lamplighter's pole, right-handed
];

/// A champion's clips: its attacks by what it holds; each ability's from how it reaches and
/// whether its champion fights up close (as `fx::look` derives an ability's look).
pub fn kit(def: usize) -> Kit {
    let d = &CHAMPIONS[def % CHAMPIONS.len()];
    let attacks = ATTACKS[def % ATTACKS.len()];
    let melee = d.missile_speed == 0.0;
    let blade = attacks.iter().any(|a| matches!(a, Clip::Slash | Clip::Thrust | Clip::Cut | Clip::Sweep));
    let dash = if melee { Clip::Charge } else { Clip::Roll };
    let casts = std::array::from_fn(|slot| {
        let a = &d.abilities[slot];
        let ult = slot == 3;
        match a.effect {
            Effect::Dash { .. } => dash,
            Effect::Blink { .. } => Clip::Flick,
            Effect::OnSelf { .. } if melee || d.role == Role::Tank => Clip::Shout,
            Effect::OnSelf { .. } => Clip::Flick,
            // At a unit, up close: the weapon, or for an ultimate its great stroke.
            Effect::Strike { .. } | Effect::Homing { .. } if melee && a.target == Target::Enemy => {
                if ult && blade {
                    Clip::Judgment
                } else if ult {
                    Clip::Slam
                } else {
                    attacks[0]
                }
            }
            Effect::Strike { .. } | Effect::Homing { .. } if a.target == Target::Ally => Clip::Flick,
            // A line from up close is something thrown: a hook, a wave.
            Effect::Line { .. } if melee => Clip::Throw,
            Effect::Strike { .. } | Effect::Homing { .. } if melee => Clip::Thrust,
            Effect::Strike { .. } | Effect::Homing { .. } | Effect::Line { .. } => Clip::Point,
            Effect::Area { .. } | Effect::Zone { .. } if melee => Clip::Slam,
            // Brought down from the sky, or raised around the caster: gathered and released.
            Effect::Area { .. } | Effect::Zone { .. } if ult || a.target == Target::Caster => Clip::Channel,
            Effect::Area { .. } | Effect::Zone { .. } => Clip::Point,
        }
    });
    Kit { idle: IDLES[def % IDLES.len()], attacks, casts, dash }
}

/// The minions' models (`web/assets/models/minions/<name>.glb`), by kind, with the attack each
/// swings; the siege cart has no rig, so no clips at all.
pub const MINIONS: [(Kind, &str, Option<Clip>); 4] = [
    (Kind::Melee, "melee", Some(Clip::Slash)),   // a sword and a shield
    (Kind::Caster, "caster", Some(Clip::Flick)), // a staff
    (Kind::Siege, "siege", None),                // a cannon on a cart
    (Kind::Super, "super", Some(Clip::Smash)),   // a great hammer
];

/// A minion's model and the attack it swings (`None` for what isn't a minion).
pub fn minion(kind: Kind) -> Option<(&'static str, Option<Clip>)> {
    MINIONS.iter().find(|m| m.0 == kind).map(|m| (m.1, m.2))
}

/// The clips a minion's model carries: it walks, swings, flinches, is thrown up and falls.
pub fn minion_clips(kind: Kind) -> Vec<Clip> {
    match minion(kind) {
        Some((_, Some(attack))) => {
            vec![Clip::Idle, Clip::Run, Clip::Hit, Clip::Death, Clip::Airborne, Clip::Flinch, attack]
        }
        _ => Vec::new(),
    }
}

/// What each champion's model carries, for `art/tools/champions.mjs`: every clip's name and
/// timing, and each champion's clips (its kit's) and each minion's. `art/clips.json`, written by
/// `cargo test -p el-client-core pose -- --ignored write_clips`.
pub fn clips_json() -> String {
    let clips: serde_json::Map<String, serde_json::Value> = Clip::ALL
        .iter()
        .map(|c| {
            let i = c.info();
            (i.name.into(), serde_json::json!({ "length": i.length, "strike": i.strike, "loop": i.looped }))
        })
        .collect();
    let champions: serde_json::Map<String, serde_json::Value> = CHAMPIONS
        .iter()
        .enumerate()
        .map(|(d, c)| (c.name.to_lowercase(), kit(d).clips().iter().map(|c| c.name()).collect()))
        .collect();
    let minions: serde_json::Map<String, serde_json::Value> = MINIONS
        .iter()
        .filter(|m| m.2.is_some())
        .map(|&(k, name, _)| (name.into(), minion_clips(k).iter().map(|c| c.name()).collect()))
        .collect();
    let mut out = serde_json::to_string_pretty(&serde_json::json!({
        "about": "The clips each champion's model carries (el_client_core::pose). Written by `cargo test -p \
                  el-client-core pose -- --ignored write_clips`; art/tools/champions.mjs cuts them to these \
                  lengths from art/champions.json's windows.",
        "clips": clips,
        "champions": champions,
        "minions": minions,
    }))
    .unwrap_or_default();
    out.push('\n');
    out
}

/// How fast (units a second) a champion's run plays at its own pace: a stride in step with the
/// floor, as tall as its radius makes it.
pub fn stride(def: usize) -> f32 {
    6.0 * CHAMPIONS[def % CHAMPIONS.len()].radius
}

/// Below this (units a second) a champion stands.
pub const MOVING: f32 = 30.0;
/// How long one clip fades into the next (s).
pub const FADE: f64 = 0.12;
/// A cast or dash of its own shows as its dash this long after (s); after that a unit carried is
/// one knocked about.
const OWN_DASH: f64 = 1.5;
/// How long a flinch takes to come in (s).
const FLINCH_IN: f32 = 0.03;
/// A swing's trail shows from this long before its strike (s, or less of a short wind-up) to this
/// long after its hold.
const SWING_BEFORE: f32 = 0.12;
const SWING_AFTER: f32 = 0.08;

/// What the client knows of a champion this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Seen {
    pub alive: bool,
    /// Units a second.
    pub speed: f32,
    /// `el_proto::unit::flags`.
    pub flags: u32,
    /// The ability being cast: its slot + 1, or 0.
    pub cast: u8,
    pub level: u8,
    /// The match is over and its team won.
    pub won: bool,
    /// Its attack's windup left (s), where it's known (the own champion's, predicted).
    pub windup_left: Option<f32>,
}

impl Seen {
    /// What a unit's record says of it (the match's outcome and its own windup aside).
    pub fn of(q: &QUnit) -> Seen {
        let v = |c: i32| dequantize_centered(c, VEL_MAX, VEL_BITS);
        Seen {
            alive: true,
            speed: v(q.vel[0]).hypot(v(q.vel[1])) * el_sim::TICK_HZ as f32,
            flags: q.flags,
            cast: q.cast as u8,
            level: q.level as u8,
            won: false,
            windup_left: None,
        }
    }
}

/// A clip at a time, at a weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    pub clip: Clip,
    /// Seconds into the clip.
    pub time: f32,
    pub weight: f32,
}

/// The moments of a frame: what the motion (`motion`) and the effects key off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Beats(pub u8);

impl Beats {
    /// A swing or a cast reached its strike.
    pub const STRIKE: u8 = 1;
    /// An attack began.
    pub const ATTACK: u8 = 2;
    /// A cast began.
    pub const CAST: u8 = 4;
    /// A dash of its own began.
    pub const DASH: u8 = 8;
    /// It came down from being thrown up.
    pub const LAND: u8 = 16;
    /// It fell.
    pub const FALL: u8 = 32;
    /// It was hit.
    pub const HIT: u8 = 64;

    pub fn has(self, b: u8) -> bool {
        self.0 & b != 0
    }
}

/// What the main clip is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Action {
    #[default]
    None,
    Attack,
    /// Casting the ability in this slot.
    Cast(u8),
}

/// What the figure shows: its clip, and the one it's leaving, fading; the legs' run under a cast
/// walked through; a flinch over the upper body; and what happened this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub main: Layer,
    pub fading: Option<Layer>,
    /// The fading clip is the arms' alone: the legs were running under it, and run on.
    pub fading_upper: bool,
    /// The run on the legs while the arms act; its weight is how much of the legs it has.
    pub legs: Option<Layer>,
    /// A flinch over the upper body, on top of the rest.
    pub flinch: Option<Layer>,
    pub beats: Beats,
    pub action: Action,
    /// At its strike's edge: from a little before it to a little after its hold (a trail's
    /// window).
    pub swing: bool,
    /// How far through its wind-up an action is (0 to 1), while it winds up.
    pub charge: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Why {
    State,
    Attack,
    Cast,
}

/// The shape of a strike's time: `lead` (s) to the strike, eased in by `snap`; held `hold` (s);
/// then followed through at `follow` (clip seconds a second).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Warp {
    lead: f32,
    hold: f32,
    snap: f32,
    follow: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Playing {
    clip: Clip,
    start: f64,
    rate: f32,
    why: Why,
    warp: Option<Warp>,
}

/// The share of a wind-up's clip time played `u` of the way through its real time.
fn ease(u: f32, snap: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    (1.0 - snap) * u + snap * u * u * u
}

impl Playing {
    fn elapsed(&self, now: f64) -> f32 {
        (now - self.start).max(0.0) as f32
    }

    fn at(&self, now: f64) -> f32 {
        let i = self.clip.info();
        let e = self.elapsed(now);
        match self.warp {
            Some(w) if i.strike > 0.0 => {
                if e < w.lead {
                    i.strike * ease(e / w.lead, w.snap)
                } else if e < w.lead + w.hold {
                    i.strike
                } else {
                    (i.strike + (e - w.lead - w.hold) * w.follow).min(i.length)
                }
            }
            _ => {
                let t = e * self.rate;
                if i.looped { t % i.length } else { t.min(i.length) }
            }
        }
    }

    fn done(&self, now: f64) -> bool {
        let i = self.clip.info();
        if i.looped {
            return false;
        }
        let e = self.elapsed(now);
        match self.warp {
            Some(w) if i.strike > 0.0 => e >= w.lead + w.hold + (i.length - i.strike) / w.follow.max(1e-3),
            _ => e * self.rate >= i.length,
        }
    }
}

/// Whose figure it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Body {
    /// A champion, by its index in [`CHAMPIONS`].
    Champion(usize),
    Minion(Kind),
}

/// A champion's figure, or a minion's, frame to frame.
#[derive(Clone, Copy, Debug)]
pub struct Poser {
    body: Body,
    kit: Kit,
    feel: Feel,
    now: Playing,
    /// The clip it left, held where it was left, and when; whether only its arms fade.
    prev: Option<(Layer, f64, bool)>,
    was_attacking: bool,
    attack_rose: f64,
    /// The last windup seen to end (s), or 0.
    measured: f32,
    attack_rate: f32,
    /// The attack it's swinging, and how many it has begun.
    attack_clip: Clip,
    swings: u32,
    was_cast: u8,
    /// The ability whose cast began, and when.
    casting: Option<(u8, f64)>,
    cast_rate: f32,
    /// When it last began a dash of its own.
    dashed: f64,
    was_dashing: bool,
    was_airborne: bool,
    was_alive: bool,
    /// The run's cycle (s into it), its pace, and the last frame's time.
    run_t: f32,
    run_rate: f32,
    last: f64,
    /// How much of the legs the run has while the arms act.
    legs: f32,
    /// The strike of the clip playing has been reached.
    struck: bool,
    /// A flinch: its clip and when it began.
    flinch: Option<(Clip, f64)>,
    hit_now: bool,
}

impl Poser {
    pub fn new(def: usize) -> Self {
        Self::of(Body::Champion(def), kit(def))
    }

    /// A minion's: its attack is its only action (it has no casts or dash of its own).
    pub fn minion(kind: Kind) -> Self {
        let attack = minion(kind).and_then(|m| m.1).unwrap_or(Clip::Slash);
        Self::of(
            Body::Minion(kind),
            Kit { idle: Clip::Idle, attacks: [attack; 2], casts: [attack; 4], dash: Clip::Hit },
        )
    }

    fn of(body: Body, kit: Kit) -> Self {
        Self {
            body,
            kit,
            feel: Feel::GAME,
            now: Playing { clip: kit.idle, start: 0.0, rate: 1.0, why: Why::State, warp: None },
            prev: None,
            was_attacking: false,
            attack_rose: f64::NEG_INFINITY,
            measured: 0.0,
            attack_rate: 1.0,
            attack_clip: kit.attacks[0],
            swings: 0,
            was_cast: 0,
            casting: None,
            cast_rate: 1.0,
            dashed: f64::NEG_INFINITY,
            was_dashing: false,
            was_airborne: false,
            was_alive: true,
            run_t: 0.0,
            run_rate: 1.0,
            last: f64::NEG_INFINITY,
            legs: 0.0,
            struck: false,
            flinch: None,
            hit_now: false,
        }
    }

    pub fn kit(&self) -> Kit {
        self.kit
    }

    /// Its timing's shape from now on (the anim stage's knobs).
    pub fn set_feel(&mut self, feel: Feel) {
        self.feel = feel;
    }

    /// It began casting ability `slot` (an event, or its record's cast rising).
    pub fn cast(&mut self, slot: u8, now: f64) {
        let Body::Champion(def) = self.body else { return };
        let s = usize::from(slot.min(3));
        let a = &CHAMPIONS[def % CHAMPIONS.len()].abilities[s];
        let clip = self.kit.casts[s];
        // Its release lands as the ability does; one with no windup is a flick of its clip's
        // gathering (a fifth of a second to the release).
        let windup = a.windup.max(0.2);
        self.cast_rate = (clip.info().strike / windup).clamp(0.5, 6.0);
        if matches!(clip, Clip::Roll | Clip::Charge) {
            self.dashed = now;
            self.cast_rate = 1.4;
        }
        self.casting = Some((slot, now));
    }

    /// It was hit: a flinch over its upper body, a reel if `heavy`. A flinch doesn't start over
    /// within a tenth of a second of the last (a reel does).
    pub fn hit(&mut self, now: f64, heavy: bool) {
        let clip = if heavy { Clip::Reel } else { Clip::Flinch };
        if let Some((c, t)) = self.flinch
            && now - t < 0.1
            && !(heavy && c == Clip::Flinch)
        {
            return;
        }
        self.flinch = Some((clip, now));
        self.hit_now = true;
    }

    /// The windup its next attack is timed to (s): its own, known; else the last one seen; else
    /// its champion's at its level with no attack speed bought, or its kind of minion's.
    fn windup(&self, seen: &Seen) -> f32 {
        if let Some(w) = seen.windup_left {
            return w.max(1.0 / 30.0);
        }
        if self.measured > 0.0 {
            return self.measured;
        }
        let (aps, share) = match self.body {
            Body::Champion(def) => {
                let d = &CHAMPIONS[def % CHAMPIONS.len()];
                let lvl = f32::from(seen.level.max(1) - 1);
                (d.base.attack_speed + d.growth.attack_speed * lvl, d.windup)
            }
            Body::Minion(kind) => (minion_def(kind).stats.attack_speed, MINION_WINDUP),
        };
        let period = (30.0 / aps.clamp(0.2, 2.5) + 0.5).floor().max(1.0);
        ((period * share + 0.5).floor().max(1.0)) / 30.0
    }

    /// How fast it moves at its run's own pace.
    fn stride(&self) -> f32 {
        match self.body {
            Body::Champion(def) => stride(def),
            Body::Minion(kind) => 6.0 * minion_def(kind).radius,
        }
    }

    /// Its run's own pace (`motion` leans by its speed against it).
    pub fn pace(&self) -> f32 {
        self.stride()
    }

    /// The shape of a strike played at `rate` clip seconds a second, held `hold`.
    fn warp(&self, clip: Clip, rate: f32, hold: f32) -> Warp {
        let lead = clip.info().strike / rate.max(1e-3);
        Warp { lead, hold, snap: self.feel.snap.clamp(0.0, 1.0), follow: (rate * self.feel.follow).max(0.05) }
    }

    pub fn step(&mut self, now: f64, seen: &Seen) -> Pose {
        let dt = if self.last.is_finite() { (now - self.last).clamp(0.0, 0.25) as f32 } else { 0.0 };
        self.last = now;
        let mut beats = 0;
        let attacking = seen.flags & flags::ATTACKING != 0;
        let moving = seen.speed > MOVING;
        let mut restart = false;
        if attacking && !self.was_attacking {
            self.attack_rose = now;
            self.attack_clip = self.kit.attacks[(self.swings % 2) as usize];
            self.swings = self.swings.wrapping_add(1);
            self.attack_rate = (self.attack_clip.info().strike / self.windup(seen)).clamp(0.4, 6.0);
            restart = true;
            beats |= Beats::ATTACK;
        }
        if !attacking && self.was_attacking {
            let w = (now - self.attack_rose) as f32;
            // A windup that ran its course (one cut short by an order moves the champion).
            if (0.03..2.0).contains(&w) && !moving {
                self.measured = w;
            }
        }
        self.was_attacking = attacking;
        if seen.cast != 0 && seen.cast != self.was_cast {
            let fresh = self.casting.is_some_and(|(s, t)| s + 1 == seen.cast && now - t < 0.3);
            if !fresh {
                self.cast(seen.cast - 1, now);
            }
        }
        self.was_cast = seen.cast;
        // A cast that just began starts its clip over.
        if let Some((_, t)) = self.casting
            && t == now
        {
            restart = true;
            beats |= Beats::CAST;
        }
        let dashing = seen.flags & flags::DASHING != 0;
        if dashing && !self.was_dashing && now - self.dashed < OWN_DASH {
            beats |= Beats::DASH;
        }
        self.was_dashing = dashing;
        let airborne = seen.flags & flags::AIRBORNE != 0;
        if !airborne && self.was_airborne && seen.alive {
            beats |= Beats::LAND;
        }
        self.was_airborne = airborne;
        if !seen.alive && self.was_alive {
            beats |= Beats::FALL;
        }
        self.was_alive = seen.alive;
        if std::mem::take(&mut self.hit_now) {
            beats |= Beats::HIT;
        }
        // The run's cycle, kept whether the whole body or only the legs run.
        if moving {
            self.run_rate = (seen.speed / self.stride()).clamp(0.5, 2.0);
            self.run_t = (self.run_t + dt * self.run_rate) % Clip::Run.info().length;
        }

        // A cast's follow-through plays on while it walks (its legs run); an attack's doesn't.
        let following =
            |why: Why| self.now.why == why && !self.now.done(now) && (why == Why::Cast || !moving);
        let (clip, rate, why) = if !seen.alive {
            (Clip::Death, 1.0, Why::State)
        } else if airborne {
            (Clip::Airborne, 1.0, Why::State)
        } else if seen.flags & flags::STUNNED != 0 {
            (Clip::Hit, 0.8, Why::State)
        } else if dashing {
            if now - self.dashed < OWN_DASH {
                (self.kit.dash, 1.4, Why::State)
            } else {
                (Clip::Hit, 1.0, Why::State)
            }
        } else if let Some((slot, _)) = self.casting.filter(|_| seen.cast != 0 || following(Why::Cast)) {
            (self.kit.casts[usize::from(slot.min(3))], self.cast_rate, Why::Cast)
        } else if attacking || following(Why::Attack) {
            (self.attack_clip, self.attack_rate, Why::Attack)
        } else if moving {
            (Clip::Run, self.run_rate, Why::State)
        } else if seen.won && matches!(self.body, Body::Champion(_)) {
            (Clip::Victory, 1.0, Why::State)
        } else {
            (self.kit.idle, 1.0, Why::State)
        };
        if why != Why::Cast && self.casting.is_some_and(|(_, t)| t < now) && seen.cast == 0 {
            self.casting = None;
        }
        let switch = clip != self.now.clip || why != self.now.why || (restart && why != Why::State);
        if switch {
            let left = Layer { clip: self.now.clip, time: self.time(now), weight: 1.0 };
            // The legs were running and run on: only the arms fade from what they did.
            let upper = (self.now.why == Why::Cast && self.legs > 0.5 && clip == Clip::Run)
                || (why == Why::Cast && self.now.clip == Clip::Run);
            self.prev = Some((left, now, upper));
            let warp = match why {
                Why::State => None,
                Why::Attack => {
                    let hold = if clip.melee() { self.feel.hold_melee } else { self.feel.hold_ranged };
                    Some(self.warp(clip, rate, hold))
                }
                Why::Cast => Some(self.warp(clip, rate, self.feel.hold_cast)),
            };
            self.now = Playing { clip, start: now, rate, why, warp };
            self.struck = false;
            // A cast begun on the run keeps the legs running.
            if why == Why::Cast {
                self.legs = if moving { 1.0 } else { 0.0 };
            }
        } else {
            self.now.rate = rate;
        }
        // The legs: the run's while the arms cast on the move, faded in and out.
        if why == Why::Cast {
            let to = if moving { 1.0 } else { 0.0 };
            let step = dt / FADE as f32;
            self.legs = if to > self.legs { (self.legs + step).min(to) } else { (self.legs - step).max(to) };
        } else {
            self.legs = 0.0;
        }
        let time = self.time(now);
        let info = self.now.clip.info();
        let reached = match self.now.warp {
            // By the clock, not the clip's time (a float a hair short of the strike is still it).
            Some(w) => self.now.elapsed(now) >= w.lead - 1e-4,
            None => time >= info.strike,
        };
        if why != Why::State && info.strike > 0.0 && !self.struck && reached {
            self.struck = true;
            beats |= Beats::STRIKE;
        }
        let (fading, fading_upper) = match self.prev {
            Some((l, at, upper)) => {
                let w = 1.0 - ((now - at) / FADE) as f32;
                if w > 0.0 { (Some(Layer { weight: w, ..l }), upper) } else { (None, false) }
            }
            None => (None, false),
        };
        if fading.is_none() {
            self.prev = None;
        }
        let main = Layer { clip: self.now.clip, time, weight: 1.0 - fading.map_or(0.0, |f| f.weight) };
        let legs =
            (self.legs > 0.0).then_some(Layer { clip: Clip::Run, time: self.run_t, weight: self.legs });
        // A flinch, but not over a fall, a stun, being thrown up or carried.
        let flinch = match self.flinch {
            Some((c, t))
                if matches!(why, Why::Attack | Why::Cast)
                    || matches!(clip, Clip::Run | Clip::Victory)
                    || clip == self.kit.idle =>
            {
                let age = (now - t) as f32;
                let len = c.info().length;
                if age >= len {
                    self.flinch = None;
                    None
                } else {
                    // In at once, out over its second half.
                    let out = 1.0 - ((age - len * 0.5).max(0.0) / (len * 0.5));
                    let w = self.feel.flinch * (age / FLINCH_IN + 0.25).min(1.0) * out;
                    (w > 0.0).then_some(Layer { clip: c, time: age, weight: w })
                }
            }
            Some(_) => {
                self.flinch = None;
                None
            }
            None => None,
        };
        let (swing, charge) = match self.now.warp {
            Some(w) => {
                let e = self.now.elapsed(now);
                let before = SWING_BEFORE.min(0.6 * w.lead);
                let swing = e >= w.lead - before && e <= w.lead + w.hold + SWING_AFTER;
                (swing, (e < w.lead).then(|| e / w.lead.max(1e-3)))
            }
            None => (false, None),
        };
        let action = match (why, self.casting) {
            (Why::Attack, _) => Action::Attack,
            (Why::Cast, Some((slot, _))) => Action::Cast(slot),
            _ => Action::None,
        };
        Pose { main, fading, fading_upper, legs, flinch, beats: Beats(beats), action, swing, charge }
    }

    /// The main clip's time now (the run's own cycle when it runs).
    fn time(&self, now: f64) -> f32 {
        if self.now.clip == Clip::Run && self.now.why == Why::State { self.run_t } else { self.now.at(now) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    #[ignore = "writes art/clips.json"]
    fn write_clips() {
        std::fs::write(root().join("art/clips.json"), clips_json()).unwrap();
    }

    #[test]
    fn the_clip_list_is_current() {
        let path = root().join("art/clips.json");
        assert!(
            std::fs::read_to_string(&path).unwrap_or_default() == clips_json(),
            "{} is out of date: cargo test -p el-client-core pose -- --ignored write_clips",
            path.display()
        );
    }

    /// A model's clips and their lengths, from its glTF's JSON (an animation's input accessor
    /// carries its last time as its `max`).
    fn model_clips(model: &str) -> Vec<(String, f32)> {
        let path = root().join(format!("web/assets/models/{model}.glb"));
        let glb = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(&glb[..4], b"glTF");
        let len = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
        let json: serde_json::Value = serde_json::from_slice(&glb[20..20 + len]).unwrap();
        let accessors = json["accessors"].as_array().unwrap();
        // A model with no clips (the cart) has no `animations` at all.
        json["animations"]
            .as_array()
            .map_or(&[][..], |a| a.as_slice())
            .iter()
            .map(|a| {
                let input = a["samplers"][0]["input"].as_u64().unwrap() as usize;
                let end = accessors[input]["max"][0].as_f64().unwrap() as f32;
                (a["name"].as_str().unwrap().to_string(), end)
            })
            .collect()
    }

    /// The node names in a model.
    fn model_nodes(model: &str) -> Vec<String> {
        let glb = std::fs::read(root().join(format!("web/assets/models/{model}.glb"))).unwrap();
        let len = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
        let json: serde_json::Value = serde_json::from_slice(&glb[20..20 + len]).unwrap();
        json["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|n| n["name"].as_str().map(String::from))
            .collect()
    }

    #[test]
    fn every_model_carries_its_clips_at_their_lengths_and_nothing_else() {
        for (d, c) in CHAMPIONS.iter().enumerate() {
            let name = format!("champions/{}", c.name.to_lowercase());
            let have = model_clips(&name);
            let want = kit(d).clips();
            for clip in &want {
                let i = clip.info();
                let Some((_, end)) = have.iter().find(|(n, _)| n == i.name) else {
                    panic!("{}'s model has no {}", c.name, i.name);
                };
                // A frame either way (the run is each champion's own).
                assert!(
                    (end - i.length).abs() < 0.04,
                    "{}'s {} is {end} s, not {}",
                    c.name,
                    i.name,
                    i.length
                );
            }
            assert_eq!(have.len(), want.len(), "{}: {have:?}", c.name);
            // What it holds has its tip (where a trail is drawn from and a shot leaves).
            let nodes = model_nodes(&name);
            for n in nodes.iter().filter(|n| n.ends_with(".tip")) {
                assert!(nodes.contains(&n.trim_end_matches(".tip").to_string()), "{}: {n}", c.name);
            }
        }
    }

    #[test]
    fn every_minion_model_carries_its_clips_and_the_cart_none() {
        for &(kind, name, attack) in &MINIONS {
            let have = model_clips(&format!("minions/{name}"));
            assert_eq!(have.len(), minion_clips(kind).len(), "{name}: {have:?}");
            assert_eq!(attack.is_none(), have.is_empty(), "{name}");
            for clip in minion_clips(kind) {
                let i = clip.info();
                let Some((_, end)) = have.iter().find(|(n, _)| n == i.name) else {
                    panic!("the {name} minion's model has no {}", i.name);
                };
                assert!(
                    (end - i.length).abs() < 0.04,
                    "the {name}'s {} is {end} s, not {}",
                    i.name,
                    i.length
                );
            }
            // Dusk's colours beside it.
            assert!(root().join(format!("web/assets/models/minions/{name}.dusk.jpg")).exists(), "{name}");
        }
        assert!(minion(Kind::Champion).is_none() && minion(Kind::Turret).is_none());
    }

    #[test]
    fn a_minion_swings_as_its_windup_ends_and_does_nothing_else() {
        // A melee minion: 0.8 attacks a second, a period of 38 ticks, 11 of them its windup (the
        // sim's sum); a caster 0.67, 45 and 14.
        for (kind, clip, ticks) in [(Kind::Melee, Clip::Slash, 11.0), (Kind::Caster, Clip::Flick, 14.0)] {
            let mut p = Poser::minion(kind);
            p.step(0.0, &seen());
            let pose = p.step(1.0, &Seen { flags: flags::ATTACKING, ..seen() });
            assert_eq!((pose.main.clip, pose.main.time), (clip, 0.0));
            let first = clip.info().strike / p.attack_rate;
            assert!((first - ticks / 30.0).abs() < 1e-3, "{kind:?}: {first}");
            // No casts, no cheering: it stands when its side has won.
            p.cast(0, 2.0);
            assert!(p.casting.is_none());
            let after = run(&mut p, 3.0, 3.0, |_| Seen { won: true, ..seen() });
            assert_eq!(after.last().unwrap().1.main.clip, Clip::Idle);
            // It walks at its own stride (a radius of 48, 325 units a second).
            let walking = p.step(7.0, &Seen { speed: 325.0, ..seen() });
            assert_eq!(walking.main.clip, Clip::Run);
            assert!((p.now.rate - 325.0 / 288.0).abs() < 1e-3);
        }
    }

    fn seen() -> Seen {
        Seen { alive: true, level: 1, ..Seen::default() }
    }

    fn def(name: &str) -> usize {
        CHAMPIONS.iter().position(|c| c.name == name).unwrap()
    }

    /// Steps a poser through `secs` at 60 frames a second, `f` giving what's seen at each time.
    fn run(p: &mut Poser, from: f64, secs: f64, mut f: impl FnMut(f64) -> Seen) -> Vec<(f64, Pose)> {
        let mut out = Vec::new();
        let mut t = from;
        while t < from + secs {
            out.push((t, p.step(t, &f(t))));
            t += 1.0 / 60.0;
        }
        out
    }

    #[test]
    fn every_clip_has_a_name_of_its_own_and_its_strike_inside_it() {
        for (k, c) in Clip::ALL.iter().enumerate() {
            let i = c.info();
            assert!(i.length > 0.0 && (0.0..i.length).contains(&i.strike), "{}", i.name);
            assert!(Clip::ALL[..k].iter().all(|o| o.name() != i.name), "{} twice", i.name);
            assert_eq!(c.index(), k);
            // What's swung or cast has a strike; what loops doesn't.
            let unstruck = matches!(c, Clip::Death | Clip::Airborne | Clip::Roll | Clip::Flinch | Clip::Reel);
            assert_eq!(i.strike > 0.0, !i.looped && !unstruck, "{}", i.name);
        }
    }

    #[test]
    fn kits_follow_what_each_ability_does() {
        assert_eq!(kit(def("Vessa")).casts, [Clip::Point, Clip::Point, Clip::Roll, Clip::Point]);
        assert_eq!(kit(def("Brakka")).casts, [Clip::Throw, Clip::Shout, Clip::Slam, Clip::Charge]);
        assert_eq!(kit(def("Vire")).casts, [Clip::Slam, Clip::Shout, Clip::Charge, Clip::Judgment]);
        assert_eq!(kit(def("Seren")).casts[3], Clip::Channel);
        assert_eq!(kit(def("Liora")).casts[2], Clip::Flick, "a ward on an ally");
        assert_eq!(kit(def("Marret")).dash, Clip::Roll);
        assert_eq!(kit(def("Gorum")).dash, Clip::Charge);
        assert_eq!((kit(def("Pell")).idle, kit(def("Gorum")).idle), (Clip::Brace, Clip::Arms));
        for (d, c) in CHAMPIONS.iter().enumerate() {
            let k = kit(d);
            // The melee swing with what they hold; the rest cast. Two attacks, not the same.
            let melee = c.missile_speed == 0.0;
            assert!(k.attacks.iter().all(|a| a.melee() == melee), "{}", c.name);
            assert_ne!(k.attacks[0], k.attacks[1], "{}", c.name);
            assert!(k.idle.info().looped && k.idle.info().strike == 0.0);
            assert!(k.clips().len() <= 16, "{}: {} clips", c.name, k.clips().len());
        }
    }

    #[test]
    fn an_attack_strikes_as_the_windup_ends() {
        let d = def("Vire");
        let mut p = Poser::new(d);
        p.step(0.0, &seen());
        // The own champion: its windup is known (0.3 s).
        let mut s = Seen { flags: flags::ATTACKING, windup_left: Some(0.3), ..seen() };
        let pose = p.step(1.0, &s);
        assert_eq!((pose.main.clip, pose.main.time), (Clip::Slash, 0.0));
        assert!(pose.beats.has(Beats::ATTACK) && pose.action == Action::Attack);
        s.windup_left = Some(0.28);
        let at_end = p.step(1.3, &s);
        let strike = Clip::Slash.info().strike;
        assert!((at_end.main.time - strike).abs() < 1e-3, "{} at the windup's end", at_end.main.time);
        assert!(at_end.beats.has(Beats::STRIKE));
        // Released: it follows through, standing, then goes back to its guard.
        let after = p.step(1.5, &seen());
        assert_eq!(after.main.clip, Clip::Slash);
        assert!(after.main.time > strike);
        let poses = run(&mut p, 1.5, 2.0, |_| seen());
        assert_eq!(poses.last().unwrap().1.main.clip, Clip::Guard);
        assert!(poses.iter().all(|(_, p)| !p.beats.has(Beats::STRIKE)), "one strike a swing");
    }

    #[test]
    fn a_strike_eases_in_holds_and_follows_through() {
        let mut p = Poser::new(def("Vire"));
        p.step(0.0, &seen());
        let s = Seen { flags: flags::ATTACKING, windup_left: Some(0.4), ..seen() };
        p.step(1.0, &s);
        let strike = Clip::Slash.info().strike;
        let f = Feel::GAME;
        // Halfway through the wind-up (real time), well short of halfway through its clip time.
        let half = p.step(1.2, &s);
        assert!(half.main.time < strike * 0.4, "{}", half.main.time);
        assert!(half.charge.is_some_and(|c| (c - 0.5).abs() < 1e-3) && !half.swing);
        // Just before the strike, swinging; then held on it.
        assert!(p.step(1.35, &s).swing);
        let held = p.step(1.4 + f64::from(f.hold_melee) * 0.9, &s);
        assert_eq!(held.main.time, strike);
        assert!(held.swing && held.charge.is_none());
        // Then on through, at the follow's pace.
        let t = 1.4 + f64::from(f.hold_melee) + 0.1;
        let on = p.step(t, &seen());
        let pace = strike / 0.4 * f.follow;
        assert!((on.main.time - (strike + 0.1 * pace)).abs() < 1e-3, "{}", on.main.time);
        assert!(!p.step(t + 0.1, &seen()).swing);
    }

    #[test]
    fn attacks_alternate() {
        let mut p = Poser::new(def("Vire"));
        p.step(0.0, &seen());
        let mut swung = Vec::new();
        for k in 0..4 {
            let t = 1.0 + 2.0 * f64::from(k);
            swung.push(p.step(t, &Seen { flags: flags::ATTACKING, ..seen() }).main.clip);
            p.step(t + 1.0, &seen());
        }
        assert_eq!(swung, [Clip::Slash, Clip::Sweep, Clip::Slash, Clip::Sweep]);
    }

    #[test]
    fn anyone_elses_attack_is_timed_from_its_champion_and_then_from_what_was_seen() {
        let d = def("Hollis");
        let mut p = Poser::new(d);
        let at = |p: &mut Poser, t: f64, on: bool| {
            p.step(t, &Seen { flags: if on { flags::ATTACKING } else { 0 }, ..seen() })
        };
        at(&mut p, 0.0, false);
        at(&mut p, 1.0, true);
        // Hollis at level 1: 0.62 attacks a second, a period of 48 ticks, 0.26 of it the windup.
        let first = p.attack_clip.info().strike / p.attack_rate;
        assert!((first - 12.0 / 30.0).abs() < 1e-3, "{first}");
        at(&mut p, 1.2, false);
        // That windup was seen to be 0.2 s (bought attack speed): the next is timed to it.
        at(&mut p, 3.0, true);
        assert!((p.attack_clip.info().strike / p.attack_rate - 0.2).abs() < 1e-3);
    }

    #[test]
    fn a_cast_releases_with_its_ability_and_its_legs_run_when_it_moves() {
        let d = def("Seren");
        let mut p = Poser::new(d);
        p.step(0.0, &seen());
        // Skyfall: 0.5 s of windup.
        p.cast(3, 1.0);
        let pose = p.step(1.0, &Seen { cast: 4, ..seen() });
        assert_eq!(pose.main.clip, Clip::Channel);
        assert!(pose.beats.has(Beats::CAST) && pose.action == Action::Cast(3));
        let released = p.step(1.5, &seen());
        assert!((released.main.time - Clip::Channel.info().strike).abs() < 1e-3);
        assert!(released.beats.has(Beats::STRIKE));
        assert_eq!(p.step(1.6, &seen()).main.clip, Clip::Channel);
        // Walking: the arms go on with the cast, the legs fade into the run.
        let walking = |_| Seen { speed: 300.0, ..seen() };
        let poses = run(&mut p, 1.62, 0.2, walking);
        assert!(poses.iter().all(|(_, p)| p.main.clip == Clip::Channel));
        let legs: Vec<f32> = poses.iter().map(|(_, p)| p.legs.map_or(0.0, |l| l.weight)).collect();
        assert!(legs.windows(2).all(|w| w[1] >= w[0]) && legs.last() == Some(&1.0), "{legs:?}");
        // Its follow-through over, still walking: the run takes all of it, the arms fading alone.
        let poses = run(&mut p, 1.82, 3.0, walking);
        let (_, ended) = poses.iter().find(|(_, p)| p.main.clip == Clip::Run).unwrap();
        assert!(ended.fading.is_some_and(|f| f.clip == Clip::Channel) && ended.fading_upper);
        let (_, before) = poses.iter().rev().find(|(_, p)| p.main.clip == Clip::Channel).unwrap();
        // The legs' cycle goes on where it was.
        let gap = (ended.main.time - before.legs.unwrap().time).rem_euclid(Clip::Run.info().length);
        assert!(gap < 0.05, "{gap}");
    }

    #[test]
    fn a_cast_begun_on_the_run_keeps_its_legs_running() {
        let mut p = Poser::new(def("Nimra"));
        let walking = Seen { speed: 300.0, ..seen() };
        run(&mut p, 0.0, 0.5, |_| walking);
        p.cast(0, 0.5);
        let pose = p.step(0.5, &Seen { cast: 1, ..walking });
        assert_eq!(pose.main.clip, kit(def("Nimra")).casts[0]);
        assert_eq!(pose.legs.map(|l| l.weight), Some(1.0));
        assert!(pose.fading.is_some_and(|f| f.clip == Clip::Run) && pose.fading_upper);
    }

    #[test]
    fn a_cast_seen_only_in_the_record_still_plays() {
        let mut p = Poser::new(def("Nimra"));
        p.step(0.0, &seen());
        assert_eq!(p.step(0.5, &Seen { cast: 1, ..seen() }).main.clip, kit(def("Nimra")).casts[0]);
        // And an event then its record is one cast, not two.
        p.step(2.0, &seen());
        p.cast(1, 3.0);
        p.step(3.0, &Seen { cast: 2, ..seen() });
        let started = p.now.start;
        p.step(3.05, &Seen { cast: 2, ..seen() });
        assert_eq!(p.now.start, started);
    }

    #[test]
    fn a_hit_flinches_its_upper_body_and_a_heavy_one_reels_it() {
        let mut p = Poser::new(def("Vire"));
        p.step(0.0, &seen());
        p.hit(1.0, false);
        let first = p.step(1.0, &seen());
        assert!(first.beats.has(Beats::HIT));
        let poses = run(&mut p, 1.0, 0.5, |_| seen());
        let flinch: Vec<f32> = poses.iter().map(|(_, p)| p.flinch.map_or(0.0, |f| f.weight)).collect();
        let most = flinch.iter().cloned().fold(0.0, f32::max);
        assert!((most - Feel::GAME.flinch).abs() < 1e-3, "{most}");
        assert_eq!(flinch.last(), Some(&0.0), "over in its length");
        assert!(poses.iter().all(|(_, p)| p.flinch.is_none_or(|f| f.clip == Clip::Flinch)));
        // Another at once doesn't start it over; a heavy one does.
        p.hit(2.0, false);
        p.step(2.0, &seen());
        p.hit(2.05, false);
        assert!((p.step(2.05, &seen()).flinch.unwrap().time - 0.05).abs() < 1e-4);
        p.hit(2.06, true);
        let reel = p.step(2.06, &seen()).flinch.unwrap();
        assert_eq!((reel.clip, reel.time), (Clip::Reel, 0.0));
        // Not while it's stunned, thrown up or dead.
        p.hit(3.0, false);
        assert!(p.step(3.0, &Seen { flags: flags::STUNNED, ..seen() }).flinch.is_none());
        p.hit(4.0, true);
        assert!(p.step(4.0, &Seen { alive: false, ..seen() }).flinch.is_none());
    }

    #[test]
    fn beats_mark_a_landing_a_fall_and_a_dash_of_its_own() {
        let mut p = Poser::new(def("Vessa"));
        p.step(0.0, &seen());
        p.step(0.5, &Seen { flags: flags::AIRBORNE, ..seen() });
        assert!(p.step(1.0, &seen()).beats.has(Beats::LAND));
        assert!(!p.step(1.1, &seen()).beats.has(Beats::LAND));
        p.cast(2, 2.0);
        assert!(p.step(2.0, &Seen { flags: flags::DASHING, ..seen() }).beats.has(Beats::DASH));
        // Carried by someone else's knock is no dash of its own.
        p.step(3.0, &seen());
        assert!(!p.step(5.0, &Seen { flags: flags::DASHING, ..seen() }).beats.has(Beats::DASH));
        assert!(p.step(6.0, &Seen { alive: false, ..seen() }).beats.has(Beats::FALL));
    }

    #[test]
    fn states_outrank_actions_and_fade_in() {
        let mut p = Poser::new(def("Gorum"));
        p.step(0.0, &Seen { speed: 340.0, ..seen() });
        let stunned = p.step(1.0, &Seen { flags: flags::STUNNED | flags::ATTACKING, ..seen() });
        assert_eq!(stunned.main.clip, Clip::Hit);
        assert_eq!(stunned.fading.map(|f| (f.clip, f.weight)), Some((Clip::Run, 1.0)));
        let half = p.step(1.0 + FADE / 2.0, &Seen { flags: flags::STUNNED, ..seen() });
        assert!((half.main.weight - 0.5).abs() < 1e-3);
        assert!(p.step(1.0 + FADE, &Seen { flags: flags::STUNNED, ..seen() }).fading.is_none());
        assert_eq!(
            p.step(2.0, &Seen { flags: flags::AIRBORNE | flags::STUNNED, ..seen() }).main.clip,
            Clip::Airborne
        );
        // Death holds its last frame.
        let dead = run(&mut p, 3.0, 4.0, |_| Seen { alive: false, ..seen() });
        let last = dead.last().unwrap().1.main;
        assert_eq!((last.clip, last.time), (Clip::Death, Clip::Death.info().length));
    }

    #[test]
    fn a_dash_of_its_own_is_its_dash_and_being_carried_is_a_flinch() {
        let d = def("Vessa");
        let mut p = Poser::new(d);
        p.step(0.0, &seen());
        p.cast(2, 1.0);
        assert_eq!(p.step(1.0, &Seen { flags: flags::DASHING, ..seen() }).main.clip, Clip::Roll);
        assert_eq!(p.step(5.0, &Seen { flags: flags::DASHING, ..seen() }).main.clip, Clip::Hit);
    }

    #[test]
    fn a_run_keeps_its_stride_as_its_pace_changes() {
        let d = def("Talmar");
        let mut p = Poser::new(d);
        let s = stride(d);
        let a = p.step(0.0, &Seen { speed: s, ..seen() });
        assert_eq!((a.main.clip, p.now.rate), (Clip::Run, 1.0));
        let b = p.step(0.1, &Seen { speed: s, ..seen() });
        let c = p.step(0.1, &Seen { speed: s * 1.5, ..seen() });
        // No jump in the cycle where the pace changed.
        assert!((b.main.time - c.main.time).abs() < 1e-4);
        assert!((p.now.rate - 1.5).abs() < 1e-4);
        // And it goes on at the new pace.
        let d2 = p.step(0.2, &Seen { speed: s * 1.5, ..seen() });
        assert!((d2.main.time - (c.main.time + 0.15)).abs() < 1e-4);
    }

    #[test]
    fn the_feel_reshapes_the_timing_but_never_the_strike() {
        for knobs in ["snap=0", "snap=1;hold_melee=0", "follow=2;hold_melee=0.2"] {
            let mut p = Poser::new(def("Pell"));
            p.set_feel(Feel::GAME.with(knobs).unwrap());
            p.step(0.0, &seen());
            let s = Seen { flags: flags::ATTACKING, windup_left: Some(0.25), ..seen() };
            p.step(1.0, &s);
            let at = p.step(1.25, &s);
            assert!((at.main.time - Clip::Cut.info().strike).abs() < 1e-3, "{knobs}: {}", at.main.time);
            assert!(at.beats.has(Beats::STRIKE), "{knobs}");
        }
    }
}
