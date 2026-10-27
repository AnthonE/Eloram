//! What a champion's figure does (`docs/CHAMPIONS.md`, "The runtime"): which clip it plays, how far
//! into it, and how fast, from what the client knows of it each frame: its flags, the ability it's
//! casting, how fast it moves, whether it lives; and the casts it began, from the events.
//!
//! The clips are timed to the game, never the other way round (Riot: an animation is a tell of
//! what the game does and never "exceeds" its time): an attack's clip is played so its strike lands
//! as the server's windup ends, a cast's so its release lands as the ability's windup does. The own
//! champion's windup is known to the tick; anyone else's is taken from its champion, its level and
//! the last one seen. A champion that moves cancels into its run, as an order cancels a windup.
//!
//! Nothing here touches the sim: the figures are drawn from what the client was told.

use el_proto::quant::dequantize_centered;
use el_proto::unit::{QUnit, VEL_BITS, VEL_MAX, flags};
use el_sim::content::champions::CHAMPIONS;
use el_sim::content::{Effect, Role, Target};

/// A clip every champion's model carries (`web/assets/models/champions/<name>.glb`, by name).
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
    pub const ALL: [Clip; 20] = [
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
    ];

    /// Its name in the models and its timing, as `art/tools/champions.mjs` cut it
    /// (`art/champions.json`, `clips`).
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
            Clip::Slam => info("slam", 2.0, 1.05, false),
            Clip::Judgment => info("judgment", 2.1, 0.7, false),
            Clip::Slash => info("slash", 1.3, 0.55, false),
            Clip::Thrust => info("thrust", 1.2, 0.73, false),
            Clip::Smash => info("smash", 1.2, 0.93, false),
            Clip::Punch => info("punch", 1.4, 0.53, false),
            Clip::Shoot => info("shoot", 2.0, 0.85, false),
            Clip::Bolt => info("bolt", 1.6, 0.7, false),
            Clip::Flick => info("flick", 1.5, 0.5, false),
            Clip::Point => info("point", 2.2, 1.45, false),
            Clip::Channel => info("channel", 2.667, 1.8, false),
        }
    }

    pub const fn name(self) -> &'static str {
        self.info().name
    }
}

/// The clips a champion plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kit {
    pub attack: Clip,
    /// Q, W, E and R.
    pub casts: [Clip; 4],
    /// While its own dash carries it.
    pub dash: Clip,
}

impl Kit {
    /// Every clip it plays, each once: what its model needs.
    pub fn clips(&self) -> Vec<Clip> {
        let mut out = vec![Clip::Idle, Clip::Run, Clip::Hit, Clip::Death, Clip::Airborne, Clip::Victory];
        for c in [self.attack, self.dash].into_iter().chain(self.casts) {
            if !out.contains(&c) {
                out.push(c);
            }
        }
        out
    }
}

/// The attack each champion swings with, by what it holds (`art/champions.json`, `held`), in
/// [`CHAMPIONS`]' order.
const ATTACKS: [Clip; 20] = [
    Clip::Shoot,  // Vessa: a crossbow
    Clip::Bolt,   // Orrin
    Clip::Punch,  // Brakka: a hook on a chain, and his fists
    Clip::Thrust, // Kesh: two knives
    Clip::Flick,  // Liora
    Clip::Thrust, // Talmar: a trident
    Clip::Bolt,   // Seren
    Clip::Smash,  // Hollis: a mace
    Clip::Slash,  // Vire: a greatsword
    Clip::Flick,  // Oda
    Clip::Shoot,  // Marret: a blunderbuss
    Clip::Bolt,   // Nimra
    Clip::Smash,  // Gorum: his fists, overhead
    Clip::Flick,  // Sable
    Clip::Slash,  // Pell: a sword
    Clip::Flick,  // Ulla
    Clip::Bolt,   // Torek
    Clip::Slash,  // Sefa: a crescent blade
    Clip::Smash,  // Brun: a forge hammer
    Clip::Flick,  // Mira
];

/// A champion's clips: its attack by what it holds; each ability's from how it reaches and
/// whether its champion fights up close (as `fx::look` derives an ability's look).
pub fn kit(def: usize) -> Kit {
    let d = &CHAMPIONS[def % CHAMPIONS.len()];
    let attack = ATTACKS[def % ATTACKS.len()];
    let melee = d.missile_speed == 0.0;
    let blade = matches!(attack, Clip::Slash | Clip::Thrust);
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
                    attack
                }
            }
            Effect::Strike { .. } | Effect::Homing { .. } if a.target == Target::Ally => Clip::Flick,
            Effect::Strike { .. } | Effect::Homing { .. } | Effect::Line { .. } if melee => Clip::Thrust,
            Effect::Strike { .. } | Effect::Homing { .. } | Effect::Line { .. } => Clip::Point,
            Effect::Area { .. } | Effect::Zone { .. } if melee => Clip::Slam,
            // Brought down from the sky, or raised around the caster: gathered and released.
            Effect::Area { .. } | Effect::Zone { .. } if ult || a.target == Target::Caster => Clip::Channel,
            Effect::Area { .. } | Effect::Zone { .. } => Clip::Point,
        }
    });
    Kit { attack, casts, dash }
}

/// What each champion's model carries, for `art/tools/champions.mjs`: every clip's name and
/// timing, and each champion's clips (its kit's). `art/clips.json`, written by
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
    let mut out = serde_json::to_string_pretty(&serde_json::json!({
        "about": "The clips each champion's model carries (el_client_core::pose). Written by `cargo test -p \
                  el-client-core pose -- --ignored write_clips`; art/tools/champions.mjs cuts them to these \
                  lengths from art/champions.json's windows.",
        "clips": clips,
        "champions": champions,
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

/// What the figure shows: its clip, and the one it's leaving, fading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub main: Layer,
    pub fading: Option<Layer>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Why {
    State,
    Attack,
    Cast,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Playing {
    clip: Clip,
    start: f64,
    rate: f32,
    why: Why,
}

impl Playing {
    fn at(&self, now: f64) -> f32 {
        let i = self.clip.info();
        let t = ((now - self.start).max(0.0) as f32) * self.rate;
        if i.looped { t % i.length } else { t.min(i.length) }
    }

    fn done(&self, now: f64) -> bool {
        let i = self.clip.info();
        !i.looped && ((now - self.start).max(0.0) as f32) * self.rate >= i.length
    }
}

/// A champion's figure, frame to frame.
#[derive(Clone, Copy, Debug)]
pub struct Poser {
    def: usize,
    kit: Kit,
    now: Playing,
    prev: Option<(Playing, f64)>,
    was_attacking: bool,
    attack_rose: f64,
    /// The last windup seen to end (s), or 0.
    measured: f32,
    attack_rate: f32,
    was_cast: u8,
    /// The ability whose cast began, and when.
    casting: Option<(u8, f64)>,
    cast_rate: f32,
    /// When it last began a dash of its own.
    dashed: f64,
}

impl Poser {
    pub fn new(def: usize) -> Self {
        Self {
            def,
            kit: kit(def),
            now: Playing { clip: Clip::Idle, start: 0.0, rate: 1.0, why: Why::State },
            prev: None,
            was_attacking: false,
            attack_rose: f64::NEG_INFINITY,
            measured: 0.0,
            attack_rate: 1.0,
            was_cast: 0,
            casting: None,
            cast_rate: 1.0,
            dashed: f64::NEG_INFINITY,
        }
    }

    pub fn kit(&self) -> Kit {
        self.kit
    }

    /// It began casting ability `slot` (an event, or its record's cast rising).
    pub fn cast(&mut self, slot: u8, now: f64) {
        let s = usize::from(slot.min(3));
        let a = &CHAMPIONS[self.def % CHAMPIONS.len()].abilities[s];
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

    /// The windup its next attack is timed to (s): its own, known; else the last one seen; else
    /// its champion's at its level with no attack speed bought.
    fn windup(&self, seen: &Seen) -> f32 {
        if let Some(w) = seen.windup_left {
            return w.max(1.0 / 30.0);
        }
        if self.measured > 0.0 {
            return self.measured;
        }
        let d = &CHAMPIONS[self.def % CHAMPIONS.len()];
        let lvl = f32::from(seen.level.max(1) - 1);
        let aps = (d.base.attack_speed + d.growth.attack_speed * lvl).clamp(0.2, 2.5);
        let period = (30.0 / aps + 0.5).floor().max(1.0);
        ((period * d.windup + 0.5).floor().max(1.0)) / 30.0
    }

    pub fn step(&mut self, now: f64, seen: &Seen) -> Pose {
        let attacking = seen.flags & flags::ATTACKING != 0;
        let moving = seen.speed > MOVING;
        let mut restart = false;
        if attacking && !self.was_attacking {
            self.attack_rose = now;
            self.attack_rate = (self.kit.attack.info().strike / self.windup(seen)).clamp(0.4, 6.0);
            restart = true;
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
        }

        let following = |why: Why| self.now.why == why && !self.now.done(now) && !moving;
        let (clip, rate, why) = if !seen.alive {
            (Clip::Death, 1.0, Why::State)
        } else if seen.flags & flags::AIRBORNE != 0 {
            (Clip::Airborne, 1.0, Why::State)
        } else if seen.flags & flags::STUNNED != 0 {
            (Clip::Hit, 0.8, Why::State)
        } else if seen.flags & flags::DASHING != 0 {
            if now - self.dashed < OWN_DASH {
                (self.kit.dash, 1.4, Why::State)
            } else {
                (Clip::Hit, 1.0, Why::State)
            }
        } else if let Some((slot, _)) = self.casting.filter(|_| seen.cast != 0 || following(Why::Cast)) {
            (self.kit.casts[usize::from(slot.min(3))], self.cast_rate, Why::Cast)
        } else if attacking || following(Why::Attack) {
            (self.kit.attack, self.attack_rate, Why::Attack)
        } else if moving {
            (Clip::Run, (seen.speed / stride(self.def)).clamp(0.5, 2.0), Why::State)
        } else if seen.won {
            (Clip::Victory, 1.0, Why::State)
        } else {
            (Clip::Idle, 1.0, Why::State)
        };
        if why != Why::Cast && self.casting.is_some_and(|(_, t)| t < now) && seen.cast == 0 {
            self.casting = None;
        }
        let switch = clip != self.now.clip || why != self.now.why || (restart && why != Why::State);
        if switch {
            self.prev = Some((self.now, now));
            self.now = Playing { clip, start: now, rate, why };
        } else {
            // The same clip at a new pace (a run speeding up) keeps its place.
            if (rate - self.now.rate).abs() > 1e-3 && clip.info().looped {
                let t = self.now.at(now);
                self.now.start = now - f64::from(t / rate);
            }
            self.now.rate = rate;
        }
        let fading = self.prev.and_then(|(p, at)| {
            let w = 1.0 - ((now - at) / FADE) as f32;
            (w > 0.0).then(|| Layer { clip: p.clip, time: p.at(now), weight: w })
        });
        if fading.is_none() {
            self.prev = None;
        }
        let main = Layer {
            clip: self.now.clip,
            time: self.now.at(now),
            weight: 1.0 - fading.map_or(0.0, |f| f.weight),
        };
        Pose { main, fading }
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
    fn model_clips(name: &str) -> Vec<(String, f32)> {
        let path = root().join(format!("web/assets/models/champions/{name}.glb"));
        let glb = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(&glb[..4], b"glTF");
        let len = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
        let json: serde_json::Value = serde_json::from_slice(&glb[20..20 + len]).unwrap();
        let accessors = json["accessors"].as_array().unwrap();
        json["animations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| {
                let input = a["samplers"][0]["input"].as_u64().unwrap() as usize;
                let end = accessors[input]["max"][0].as_f64().unwrap() as f32;
                (a["name"].as_str().unwrap().to_string(), end)
            })
            .collect()
    }

    #[test]
    fn every_model_carries_its_clips_at_their_lengths() {
        for (d, c) in CHAMPIONS.iter().enumerate() {
            let have = model_clips(&c.name.to_lowercase());
            for clip in kit(d).clips() {
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
            // What's swung or cast has a strike; what loops doesn't.
            assert_eq!(i.strike > 0.0, !i.looped && !matches!(c, Clip::Death | Clip::Airborne | Clip::Roll));
        }
    }

    #[test]
    fn kits_follow_what_each_ability_does() {
        assert_eq!(kit(def("Vessa")).casts, [Clip::Point, Clip::Point, Clip::Roll, Clip::Point]);
        assert_eq!(kit(def("Brakka")).casts, [Clip::Thrust, Clip::Shout, Clip::Slam, Clip::Charge]);
        assert_eq!(kit(def("Vire")).casts, [Clip::Slam, Clip::Shout, Clip::Charge, Clip::Judgment]);
        assert_eq!(kit(def("Seren")).casts[3], Clip::Channel);
        assert_eq!(kit(def("Liora")).casts[2], Clip::Flick, "a ward on an ally");
        assert_eq!(kit(def("Marret")).dash, Clip::Roll);
        assert_eq!(kit(def("Gorum")).dash, Clip::Charge);
        for (d, c) in CHAMPIONS.iter().enumerate() {
            let k = kit(d);
            // The melee swing with what they hold; the rest cast.
            let melee = c.missile_speed == 0.0;
            assert_eq!(melee, matches!(k.attack, Clip::Slash | Clip::Thrust | Clip::Smash | Clip::Punch));
            assert!(k.clips().len() <= 12, "{}: {} clips", c.name, k.clips().len());
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
        s.windup_left = Some(0.28);
        let at_end = p.step(1.3, &s);
        let strike = Clip::Slash.info().strike;
        assert!((at_end.main.time - strike).abs() < 1e-3, "{} at the windup's end", at_end.main.time);
        // Released: it follows through, standing, then goes back to idle.
        let after = p.step(1.5, &seen());
        assert_eq!(after.main.clip, Clip::Slash);
        assert!(after.main.time > strike);
        let poses = run(&mut p, 1.5, 2.0, |_| seen());
        assert_eq!(poses.last().unwrap().1.main.clip, Clip::Idle);
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
        let first = Clip::Smash.info().strike / p.attack_rate;
        assert!((first - 12.0 / 30.0).abs() < 1e-3, "{first}");
        at(&mut p, 1.2, false);
        // That windup was seen to be 0.2 s (bought attack speed): the next is timed to it.
        at(&mut p, 3.0, true);
        assert!((Clip::Smash.info().strike / p.attack_rate - 0.2).abs() < 1e-3);
    }

    #[test]
    fn a_cast_releases_with_its_ability_and_moving_cuts_it_short() {
        let d = def("Seren");
        let mut p = Poser::new(d);
        p.step(0.0, &seen());
        // Skyfall: 0.5 s of windup.
        p.cast(3, 1.0);
        let pose = p.step(1.0, &Seen { cast: 4, ..seen() });
        assert_eq!(pose.main.clip, Clip::Channel);
        let released = p.step(1.5, &seen());
        assert!((released.main.time - Clip::Channel.info().strike).abs() < 1e-3);
        // The follow-through holds while she stands; an order to walk ends it.
        assert_eq!(p.step(1.6, &seen()).main.clip, Clip::Channel);
        let walking = p.step(1.7, &Seen { speed: 300.0, ..seen() });
        assert_eq!(walking.main.clip, Clip::Run);
        assert_eq!(walking.fading.map(|f| f.clip), Some(Clip::Channel));
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
    }
}
