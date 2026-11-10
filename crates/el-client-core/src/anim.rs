//! The anim stage (`docs/CHAMPIONS.md`, "The stage"; `el-client/src/animstage.rs`): one champion
//! alone, doing one thing, twice at once (in profile over a dark card, three-quarters over a light
//! one), on the fx stage's clock (a step a sixtieth of a second, held where a shooter asks), so a
//! strip of it can be shot (`e2e/fxstrip.mjs`, `scripts/anim.sh`), read and compared as an
//! effect's is. Its knobs are `motion::Feel`'s, retuned without a rebuild (`&knobs=`).
//!
//! This is the part with no engine in it: the catalogue (every champion, every act) and each act's
//! script, what the figure is told at each moment as a match's client would tell it: how fast it
//! moves and which way it faces, its flags, the casts it begins, the hits it takes.

use el_proto::unit::flags;
use el_sim::content::Effect;
use el_sim::content::champions::CHAMPIONS;

use crate::pose::{Seen, stride};

/// What a staged figure does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Stand,
    Run,
    Attack,
    /// Cast the ability in this slot, standing.
    Cast(u8),
    /// Cast its first ability on the run.
    Walkcast,
    Hit,
    Turn,
    Land,
    Fall,
    Win,
}

impl Act {
    pub const ALL: [Act; 13] = [
        Act::Stand,
        Act::Run,
        Act::Attack,
        Act::Cast(0),
        Act::Cast(1),
        Act::Cast(2),
        Act::Cast(3),
        Act::Walkcast,
        Act::Hit,
        Act::Turn,
        Act::Land,
        Act::Fall,
        Act::Win,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Act::Stand => "stand",
            Act::Run => "run",
            Act::Attack => "attack",
            Act::Cast(0) => "q",
            Act::Cast(1) => "w",
            Act::Cast(2) => "e",
            Act::Cast(_) => "r",
            Act::Walkcast => "walkcast",
            Act::Hit => "hit",
            Act::Turn => "turn",
            Act::Land => "land",
            Act::Fall => "fall",
            Act::Win => "win",
        }
    }

    pub fn what(self) -> &'static str {
        match self {
            Act::Stand => "standing: its stance",
            Act::Run => "off from standing, a run at its own pace, and a stop",
            Act::Attack => "three attacks at level 1, each striking as its windup ends, alternating",
            Act::Cast(_) => "the ability cast standing, released as its windup ends",
            Act::Walkcast => "its first ability cast on the run: the arms cast, the legs run on",
            Act::Hit => "a light hit from in front, then a heavy one from its left",
            Act::Turn => "an about-face, back, then a run weaving left and right",
            Act::Land => "thrown up, and down",
            Act::Fall => "it falls",
            Act::Win => "its side has won",
        }
    }
}

/// One thing staged: a champion (its index in [`CHAMPIONS`]) and an act.
#[derive(Clone, Debug, PartialEq)]
pub struct Staged {
    pub champ: usize,
    pub act: Act,
    /// `<champion>-<act>`: `vire-attack`, `seren-r`.
    pub name: String,
    /// How long the act lasts (s), its follow-through included.
    pub span: f32,
}

/// What the figure is told at a moment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Moment {
    pub seen: Seen,
    /// Its turn from where it faces at rest (rad, to its left).
    pub turn: f32,
    /// A cast begun since the last moment (its slot).
    pub cast: Option<u8>,
    /// A hit taken since the last moment: where from (rad from its front at rest, to its left) and
    /// the share of its health it took.
    pub hit: Option<(f32, f32)>,
}

/// When things happen in a script (s).
const START: f32 = 0.25;

/// A champion's attack at level 1 with nothing bought: its period and its windup (s), as the sim
/// sums them (`Poser::windup`).
pub fn attack_timing(champ: usize) -> (f32, f32) {
    let d = &CHAMPIONS[champ % CHAMPIONS.len()];
    let period = (30.0 / d.base.attack_speed.clamp(0.2, 2.5) + 0.5).floor().max(1.0);
    let windup = (period * d.windup + 0.5).floor().max(1.0);
    (period / 30.0, windup / 30.0)
}

impl Staged {
    pub fn new(champ: usize, act: Act) -> Staged {
        let c = &CHAMPIONS[champ % CHAMPIONS.len()];
        let span = match act {
            Act::Stand => 2.5,
            Act::Run => 2.2,
            Act::Attack => START + 3.0 * attack_timing(champ).0 + 0.9,
            Act::Cast(s) => START + c.abilities[usize::from(s.min(3))].windup + 1.8,
            Act::Walkcast => 2.4,
            Act::Hit => 2.0,
            Act::Turn => 2.8,
            Act::Land => 1.8,
            Act::Fall => 2.6,
            Act::Win => 3.4,
        };
        Staged { champ, act, name: format!("{}-{}", c.name.to_lowercase(), act.name()), span }
    }

    /// What the figure is told now (`t`, s since step 1), with what began since `prev` (an event
    /// at `e` is told once, when `prev < e <= t`; the first call's `prev` is below zero).
    pub fn at(&self, prev: f32, t: f32) -> Moment {
        let c = &CHAMPIONS[self.champ % CHAMPIONS.len()];
        let fresh = |e: f32| prev < e && e <= t;
        let during = |from: f32, to: f32| from <= t && t < to;
        let mut m = Moment { seen: Seen { alive: true, level: 1, ..Seen::default() }, ..Moment::default() };
        let pace = stride(self.champ);
        match self.act {
            Act::Stand => {}
            Act::Run => {
                if during(0.3, 1.7) {
                    m.seen.speed = pace;
                }
            }
            Act::Attack => {
                let (period, windup) = attack_timing(self.champ);
                for k in 0..3 {
                    let from = START + k as f32 * period;
                    if during(from, from + windup) {
                        m.seen.flags |= flags::ATTACKING;
                        m.seen.windup_left = Some(from + windup - t);
                    }
                }
            }
            Act::Cast(slot) => {
                let a = &c.abilities[usize::from(slot.min(3))];
                let w = a.windup.max(1.0 / 30.0);
                if fresh(START) {
                    m.cast = Some(slot);
                }
                if during(START, START + w) {
                    m.seen.cast = slot + 1;
                }
                if matches!(a.effect, Effect::Dash { .. }) && during(START + w, START + w + 0.35) {
                    m.seen.flags |= flags::DASHING;
                }
            }
            Act::Walkcast => {
                if during(0.15, 2.0) {
                    m.seen.speed = pace;
                }
                let w = c.abilities[0].windup.max(1.0 / 30.0);
                if fresh(0.6) {
                    m.cast = Some(0);
                }
                if during(0.6, 0.6 + w) {
                    m.seen.cast = 1;
                }
            }
            Act::Hit => {
                if fresh(0.3) {
                    m.hit = Some((0.0, 0.04));
                }
                if fresh(1.1) {
                    m.hit = Some((std::f32::consts::FRAC_PI_2, 0.15));
                }
            }
            Act::Turn => {
                if during(0.3, 1.0) {
                    m.turn = std::f32::consts::PI;
                }
                if during(1.4, 2.6) {
                    m.seen.speed = pace;
                    m.turn = 0.7 * ((t - 1.4) * 5.0).sin();
                }
            }
            Act::Land => {
                if during(START, 0.85) {
                    m.seen.flags |= flags::AIRBORNE;
                }
            }
            Act::Fall => m.seen.alive = t < 0.3,
            Act::Win => m.seen.won = t >= 0.2,
        }
        m
    }
}

/// Everything the stage can stage: each champion, each act.
pub fn list() -> Vec<Staged> {
    (0..CHAMPIONS.len()).flat_map(|c| Act::ALL.map(|a| Staged::new(c, a))).collect()
}

/// The staged thing named `<champion>-<act>`.
pub fn find(name: &str) -> Option<Staged> {
    list().into_iter().find(|s| s.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::Feel;
    use crate::pose::{Beats, Clip, Poser, kit};

    fn def(name: &str) -> usize {
        CHAMPIONS.iter().position(|c| c.name == name).unwrap()
    }

    /// Plays a staged act through a poser at 60 frames a second, as the stage does.
    fn play(s: &Staged) -> Vec<(f32, crate::pose::Pose)> {
        let mut p = Poser::new(s.champ);
        let mut out = Vec::new();
        let mut prev = -1.0;
        for k in 0..(s.span * 60.0) as u32 {
            let t = k as f32 / 60.0;
            let m = s.at(prev, t);
            let now = 100.0 + f64::from(t);
            if let Some(slot) = m.cast {
                p.cast(slot, now);
            }
            if let Some((_, share)) = m.hit {
                p.hit(now, share > Feel::GAME.heavy);
            }
            out.push((t, p.step(now, &m.seen)));
            prev = t;
        }
        out
    }

    #[test]
    fn every_name_is_its_own_and_found() {
        let all = list();
        assert_eq!(all.len(), CHAMPIONS.len() * Act::ALL.len());
        for (k, s) in all.iter().enumerate() {
            assert!(all[..k].iter().all(|o| o.name != s.name), "{} twice", s.name);
            assert_eq!(find(&s.name).as_ref(), Some(s));
            assert!(s.span > 1.0 && s.span < 8.0, "{}: {}", s.name, s.span);
        }
        assert!(find("vire-attack").is_some() && find("nobody-attack").is_none());
    }

    #[test]
    fn an_attack_act_swings_three_times_alternating_each_striking_on_time() {
        let s = find("vire-attack").unwrap();
        let (period, windup) = attack_timing(s.champ);
        let poses = play(&s);
        let strikes: Vec<f32> =
            poses.iter().filter(|(_, p)| p.beats.has(Beats::STRIKE)).map(|(t, _)| *t).collect();
        assert_eq!(strikes.len(), 3, "{strikes:?}");
        for (k, t) in strikes.iter().enumerate() {
            let want = START + k as f32 * period + windup;
            assert!((t - want).abs() < 1.5 / 60.0, "strike {k} at {t}, not {want}");
        }
        let swung: Vec<Clip> =
            poses.iter().filter(|(_, p)| p.beats.has(Beats::ATTACK)).map(|(_, p)| p.main.clip).collect();
        assert_eq!(swung, [Clip::Slash, Clip::Sweep, Clip::Slash]);
    }

    #[test]
    fn a_cast_act_casts_once_and_a_walkcast_runs_under_it() {
        for (name, slot) in [("seren-r", 3u8), ("brakka-q", 0)] {
            let s = find(name).unwrap();
            let poses = play(&s);
            let casts = poses.iter().filter(|(_, p)| p.beats.has(Beats::CAST)).count();
            assert_eq!(casts, 1, "{name}");
            assert!(
                poses.iter().any(|(_, p)| p.main.clip == kit(s.champ).casts[usize::from(slot)]),
                "{name}"
            );
        }
        let s = find("nimra-walkcast").unwrap();
        let poses = play(&s);
        assert!(poses.iter().any(|(_, p)| p.legs.is_some_and(|l| l.weight == 1.0)));
    }

    #[test]
    fn the_other_acts_do_what_they_say() {
        let beats =
            |name: &str, b: u8| play(&find(name).unwrap()).iter().filter(|(_, p)| p.beats.has(b)).count();
        assert_eq!(beats("vessa-land", Beats::LAND), 1);
        assert_eq!(beats("vessa-fall", Beats::FALL), 1);
        assert_eq!(beats("vessa-hit", Beats::HIT), 2);
        assert_eq!(beats("vessa-e", Beats::DASH), 1, "Vessa's E is a dash");
        let s = find("gorum-turn").unwrap();
        let turns: Vec<f32> = (0..(s.span * 60.0) as u32).map(|k| s.at(-1.0, k as f32 / 60.0).turn).collect();
        assert!(turns.iter().any(|t| (t - std::f32::consts::PI).abs() < 1e-3));
        assert!(turns.iter().any(|t| *t < -0.5) && turns.iter().any(|t| *t > 0.5));
        assert!(play(&find("gorum-win").unwrap()).last().unwrap().1.main.clip == Clip::Victory);
        assert_eq!(
            play(&find(&format!("{}-stand", "pell")).unwrap()).last().unwrap().1.main.clip,
            kit(def("Pell")).idle
        );
    }
}
