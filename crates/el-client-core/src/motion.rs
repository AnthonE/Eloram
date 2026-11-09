//! What a figure does on top of its clips (`docs/CHAMPIONS.md`, "The motion"): the part of a
//! champion's movement no clip has, worked out each frame from what the client knows and the beats
//! its pose struck (`pose::Beats`). Nothing here moves the unit: where it stands and which way it
//! faces are the server's, and the figure is only drawn turning, leaning, recoiling and squashing
//! about its own feet.
//!
//! - **It turns** toward its facing in a few hundredths of a second instead of snapping to it at
//!   the sim's 30 Hz (a path round a minion turns it every tick).
//! - **It leans** into a turn by how fast it turns and moves, forward as it speeds up and a little
//!   while it runs, back as it stops.
//! - **It recoils** from a hit, away from whoever struck it, by how much of its health the hit took,
//!   and springs back.
//! - **It squashes and stretches**, keeping its volume: down as it lands, strikes or falls, up
//!   as it dashes.
//!
//! The numbers are a [`Feel`]: [`Feel::GAME`] in a match, retuned without a rebuild on the anim
//! stage (`?animstage=`, `knobs=`), as the effects' recipes are on the fx stage.

use crate::pose::Beats;

/// How the figures move: the timing's shape (`pose`) and the motion's sizes (here).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feel {
    /// How much a strike's wind-up eases in: 0 plays it straight; at 1 it starts still and ends
    /// three times as fast (the clip's time to its strike is `u` blended with `u³`).
    pub snap: f32,
    /// How long a strike holds on its contact (s): a melee swing's, a shot's, a cast's release.
    pub hold_melee: f32,
    pub hold_ranged: f32,
    pub hold_cast: f32,
    /// The follow-through's pace, of the wind-up's.
    pub follow: f32,
    /// The turn's time constant (s): it is two thirds of the way round after this long.
    pub turn: f32,
    /// The lean while it runs at its stride (rad), and its most (rad).
    pub lean_run: f32,
    pub lean_max: f32,
    /// Forward lean (rad) a unit a second a second of speeding up.
    pub lean_accel: f32,
    /// Bank (rad) a radian a second of turning a unit a second of speed.
    pub bank: f32,
    /// A hit's recoil (rad): at least this, and this much more for the whole of its health.
    pub recoil: f32,
    pub recoil_share: f32,
    /// Squash and stretch, a share of its height: landing, a strike, a dash, a fall.
    pub squash_land: f32,
    pub squash_strike: f32,
    pub stretch_dash: f32,
    pub squash_fall: f32,
    /// How much a flinch takes the upper body (0 to 1).
    pub flinch: f32,
    /// A hit is heavy (a reel, not a flinch) past this share of its health.
    pub heavy: f32,
}

impl Feel {
    /// The game's.
    pub const GAME: Feel = Feel {
        snap: 0.6,
        hold_melee: 0.07,
        hold_ranged: 0.035,
        hold_cast: 0.06,
        follow: 0.85,
        turn: 0.035,
        lean_run: 0.07,
        lean_max: 0.2,
        lean_accel: 0.000_035,
        bank: 0.000_03,
        recoil: 0.07,
        recoil_share: 0.5,
        squash_land: 0.16,
        squash_strike: 0.05,
        stretch_dash: 0.12,
        squash_fall: 0.1,
        flinch: 0.85,
        heavy: 0.08,
    };

    /// The knobs the anim stage takes (`knobs=name=value;...`), each with its place.
    pub const KNOBS: [&'static str; 18] = [
        "snap",
        "hold_melee",
        "hold_ranged",
        "hold_cast",
        "follow",
        "turn",
        "lean_run",
        "lean_max",
        "lean_accel",
        "bank",
        "recoil",
        "recoil_share",
        "squash_land",
        "squash_strike",
        "stretch_dash",
        "squash_fall",
        "flinch",
        "heavy",
    ];

    fn slot(&mut self, name: &str) -> Option<&mut f32> {
        Some(match name {
            "snap" => &mut self.snap,
            "hold_melee" => &mut self.hold_melee,
            "hold_ranged" => &mut self.hold_ranged,
            "hold_cast" => &mut self.hold_cast,
            "follow" => &mut self.follow,
            "turn" => &mut self.turn,
            "lean_run" => &mut self.lean_run,
            "lean_max" => &mut self.lean_max,
            "lean_accel" => &mut self.lean_accel,
            "bank" => &mut self.bank,
            "recoil" => &mut self.recoil,
            "recoil_share" => &mut self.recoil_share,
            "squash_land" => &mut self.squash_land,
            "squash_strike" => &mut self.squash_strike,
            "stretch_dash" => &mut self.stretch_dash,
            "squash_fall" => &mut self.squash_fall,
            "flinch" => &mut self.flinch,
            "heavy" => &mut self.heavy,
            _ => return None,
        })
    }

    /// One knob's value.
    pub fn get(&self, name: &str) -> Option<f32> {
        let mut f = *self;
        f.slot(name).map(|v| *v)
    }

    /// The feel with `knobs` (`name=value;...`) set; a name it doesn't have is returned.
    pub fn with(mut self, knobs: &str) -> Result<Feel, String> {
        for kv in knobs.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let (k, v) = kv.split_once('=').ok_or_else(|| format!("{kv}: not name=value"))?;
            let v: f32 = v.trim().parse().map_err(|_| format!("{kv}: not a number"))?;
            *self.slot(k.trim()).ok_or_else(|| format!("{k}: no such knob"))? = v;
        }
        Ok(self)
    }
}

impl Default for Feel {
    fn default() -> Self {
        Feel::GAME
    }
}

/// A spring toward a target: critically damped at `damping` 1, a wobble below it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Spring {
    x: f32,
    v: f32,
}

impl Spring {
    fn step(&mut self, target: f32, omega: f32, damping: f32, dt: f32) {
        // Small steps, so a long frame doesn't blow it up.
        let n = (dt / (1.0 / 240.0)).ceil().clamp(1.0, 32.0);
        let h = dt / n;
        for _ in 0..n as u32 {
            let a = -omega * omega * (self.x - target) - 2.0 * damping * omega * self.v;
            self.v += a * h;
            self.x += self.v * h;
        }
    }
}

/// The shortest way round from `a` to `b` (rad).
pub fn wrap(d: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (d + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

/// How a figure is drawn this frame, about its feet.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Drawn {
    /// Its turn about the up axis (rad), the scene's: the model's +Z turned to face it.
    pub yaw: f32,
    /// Leaning forward (rad, about its own X: its head toward its front).
    pub pitch: f32,
    /// Leaning to its right (rad, about its own Z: its head toward its −X).
    pub roll: f32,
    /// Its height's scale; its width's is `1 / √height`, so its volume is kept.
    pub height: f32,
}

impl Drawn {
    pub fn width(&self) -> f32 {
        1.0 / self.height.max(0.2).sqrt()
    }
}

/// A figure's motion, frame to frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Motion {
    yaw: f32,
    yaw_rate: f32,
    started: bool,
    last: f64,
    speed: f32,
    accel: f32,
    pitch: Spring,
    roll: Spring,
    /// A hit's recoil, in its own frame (forward, right).
    kick: [Spring; 2],
    squash: Spring,
}

/// How stiff the lean, the recoil and the squash are (rad/s), and how damped.
const LEAN: (f32, f32) = (18.0, 1.0);
const KICK: (f32, f32) = (26.0, 0.45);
const SQUASH: (f32, f32) = (24.0, 0.35);

impl Motion {
    /// A hit that came from `away` (the direction from whoever struck it to it, in the scene's
    /// ground plane: `[x, z]`, any length), taking `share` of its health.
    pub fn hit(&mut self, away: [f32; 2], share: f32, feel: &Feel) {
        let l = away[0].hypot(away[1]);
        if l < 1e-4 {
            return;
        }
        let size = (feel.recoil + feel.recoil_share * share.clamp(0.0, 1.0)).min(feel.lean_max * 1.5);
        // Into its own frame: its front is +Z turned by its yaw, its right −X.
        let (s, c) = self.yaw.sin_cos();
        let (ax, az) = (away[0] / l, away[1] / l);
        let front = ax * s + az * c;
        let right = -(ax * c - az * s);
        // A kick to its lean's speed: its head goes the way it was pushed.
        self.kick[0].v += front * size * KICK.0;
        self.kick[1].v += right * size * KICK.0;
    }

    /// The figure this frame: `facing` the scene's yaw the sim gives it, `speed` (units a second)
    /// as it moves, `stride` its run's own pace, `beats` its pose's.
    pub fn step(
        &mut self,
        now: f64,
        facing: f32,
        speed: f32,
        stride: f32,
        beats: Beats,
        feel: &Feel,
    ) -> Drawn {
        let dt = if self.started { (now - self.last).clamp(0.0, 0.25) as f32 } else { 0.0 };
        self.last = now;
        if !self.started {
            self.started = true;
            self.yaw = facing;
            self.speed = speed;
        }
        // The turn: a good way round in a few hundredths of a second, the short way.
        let before = self.yaw;
        if dt > 0.0 {
            let k = 1.0 - (-dt / feel.turn.max(1e-3)).exp();
            self.yaw = wrap(self.yaw + wrap(facing - self.yaw) * k);
            let rate = wrap(self.yaw - before) / dt;
            self.yaw_rate += (rate - self.yaw_rate) * (dt / 0.05).min(1.0);
            let a = (speed - self.speed) / dt;
            self.accel += (a - self.accel) * (dt / 0.08).min(1.0);
        }
        self.speed = speed;
        // The lean: a little while it runs, more as it speeds up (back as it slows), into turns.
        let running = (speed / stride.max(1.0)).min(1.5);
        let pitch =
            (feel.lean_run * running + feel.lean_accel * self.accel).clamp(-feel.lean_max, feel.lean_max);
        let roll = (-feel.bank * self.yaw_rate * speed).clamp(-feel.lean_max, feel.lean_max);
        self.pitch.step(pitch, LEAN.0, LEAN.1, dt);
        self.roll.step(roll, LEAN.0, LEAN.1, dt);
        for k in &mut self.kick {
            k.step(0.0, KICK.0, KICK.1, dt);
        }
        // Squash and stretch, kicked by what it did this frame.
        let mut kick = 0.0;
        if beats.has(Beats::LAND) {
            kick -= feel.squash_land;
        }
        if beats.has(Beats::STRIKE) {
            kick -= feel.squash_strike;
        }
        if beats.has(Beats::DASH) {
            kick += feel.stretch_dash;
        }
        if beats.has(Beats::FALL) {
            kick -= feel.squash_fall;
        }
        self.squash.v += kick * SQUASH.0;
        self.squash.step(0.0, SQUASH.0, SQUASH.1, dt);
        Drawn {
            yaw: self.yaw,
            pitch: self.pitch.x + self.kick[0].x,
            roll: self.roll.x + self.kick[1].x,
            height: (1.0 + self.squash.x).clamp(0.6, 1.4),
        }
    }

    /// Back to standing as `facing`, as a figure seen again is.
    pub fn reset(&mut self) {
        *self = Motion::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const F: Feel = Feel::GAME;

    fn run(m: &mut Motion, from: f64, secs: f64, mut f: impl FnMut(f64) -> (f32, f32, Beats)) -> Vec<Drawn> {
        let mut out = Vec::new();
        let mut t = from;
        while t < from + secs {
            let (facing, speed, beats) = f(t);
            out.push(m.step(t, facing, speed, 350.0, beats, &F));
            t += 1.0 / 60.0;
        }
        out
    }

    #[test]
    fn it_turns_quickly_the_short_way_and_never_past() {
        let mut m = Motion::default();
        assert_eq!(
            m.step(0.0, 3.0, 0.0, 350.0, Beats::default(), &F).yaw,
            3.0,
            "the first frame is where it faces"
        );
        // Round past π: from 3.0 to −3.0 is 0.28 rad the short way.
        let d = run(&mut m, 1.0 / 60.0, 0.3, |_| (-3.0, 0.0, Beats::default()));
        let gap = |y: f32| wrap(-3.0 - y).abs();
        assert!(d.iter().all(|x| gap(x.yaw) <= 0.29), "never the long way");
        assert!(gap(d[6].yaw) < 0.05, "most of the way in a tenth of a second: {}", gap(d[6].yaw));
        // A half turn: done in under a fifth of a second, never overshooting.
        let mut m = Motion::default();
        m.step(0.0, 0.0, 0.0, 350.0, Beats::default(), &F);
        let d = run(&mut m, 1.0 / 60.0, 0.3, |_| (PI, 0.0, Beats::default()));
        assert!(d.windows(2).all(|w| w[1].yaw.abs() >= w[0].yaw.abs() - 1e-4));
        assert!(wrap(PI - d[11].yaw).abs() < 0.05, "{}", d[11].yaw);
    }

    #[test]
    fn it_leans_into_a_turn_and_forward_as_it_speeds_up() {
        let mut m = Motion::default();
        m.step(0.0, 0.0, 0.0, 350.0, Beats::default(), &F);
        // Off from standing: forward, more than while it runs on.
        let d = run(&mut m, 1.0 / 60.0, 1.0, |_| (0.0, 350.0, Beats::default()));
        let most = d.iter().map(|x| x.pitch).fold(0.0, f32::max);
        let steady = d.last().unwrap().pitch;
        assert!(most > steady + 0.02 && (steady - F.lean_run).abs() < 0.01, "{most} {steady}");
        // A left turn at speed (its yaw rising): leaning left, its roll below zero.
        let mut yaw = 0.0;
        let d = run(&mut m, 1.0, 0.2, |_| {
            yaw += 0.12;
            (yaw, 350.0, Beats::default())
        });
        assert!(d.last().unwrap().roll < -0.05, "{}", d.last().unwrap().roll);
        // Stopped: upright again, having leant back a moment.
        let d = run(&mut m, 1.2, 1.5, |_| (yaw, 0.0, Beats::default()));
        assert!(d.iter().any(|x| x.pitch < -0.02));
        let last = d.last().unwrap();
        assert!(last.pitch.abs() < 0.01 && last.roll.abs() < 0.01, "{last:?}");
        assert!(d.iter().all(|x| x.pitch.abs() <= F.lean_max + 1e-4 && x.roll.abs() <= F.lean_max + 1e-4));
    }

    #[test]
    fn a_hit_pushes_it_away_from_its_source_and_it_comes_back() {
        // Facing +Z (yaw 0); hit from in front, so pushed toward −Z: leaning back.
        let mut m = Motion::default();
        m.step(0.0, 0.0, 0.0, 350.0, Beats::default(), &F);
        m.hit([0.0, -1.0], 0.2, &F);
        let d = run(&mut m, 1.0 / 60.0, 0.6, |_| (0.0, 0.0, Beats::default()));
        let back = d.iter().map(|x| x.pitch).fold(0.0, f32::min);
        assert!(back < -0.08, "{back}");
        assert!(d.last().unwrap().pitch.abs() < 0.01);
        // From its left (pushed toward −X, its right): leaning right.
        let mut m = Motion::default();
        m.step(0.0, 0.0, 0.0, 350.0, Beats::default(), &F);
        m.hit([-1.0, 0.0], 0.0, &F);
        let d = run(&mut m, 1.0 / 60.0, 0.2, |_| (0.0, 0.0, Beats::default()));
        assert!(d.iter().map(|x| x.roll).fold(0.0, f32::max) > 0.03);
        // Turned to face +X, the same push from the left is from behind: leaning forward.
        let mut m = Motion::default();
        m.step(0.0, PI / 2.0, 0.0, 350.0, Beats::default(), &F);
        m.hit([1.0, 0.0], 0.0, &F);
        let d = run(&mut m, 1.0 / 60.0, 0.2, |_| (PI / 2.0, 0.0, Beats::default()));
        assert!(d.iter().map(|x| x.pitch).fold(0.0, f32::max) > 0.03);
    }

    #[test]
    fn landing_squashes_a_dash_stretches_and_volume_is_kept() {
        let mut m = Motion::default();
        m.step(0.0, 0.0, 0.0, 350.0, Beats::default(), &F);
        let land = Beats(Beats::LAND);
        let d = run(&mut m, 1.0 / 60.0, 0.8, |t| (0.0, 0.0, if t < 0.02 { land } else { Beats::default() }));
        let low = d.iter().map(|x| x.height).fold(2.0, f32::min);
        assert!(low < 0.93, "{low}");
        assert!((d.last().unwrap().height - 1.0).abs() < 0.01);
        for x in &d {
            assert!((x.width() * x.width() * x.height - 1.0).abs() < 1e-4);
        }
        let mut m = Motion::default();
        m.step(0.0, 0.0, 0.0, 350.0, Beats::default(), &F);
        let dash = Beats(Beats::DASH);
        let d = run(&mut m, 1.0 / 60.0, 0.3, |t| (0.0, 0.0, if t < 0.02 { dash } else { Beats::default() }));
        assert!(d.iter().map(|x| x.height).fold(0.0, f32::max) > 1.05);
    }

    #[test]
    fn a_long_frame_does_not_blow_up() {
        let mut m = Motion::default();
        m.step(0.0, 0.0, 0.0, 350.0, Beats::default(), &F);
        m.hit([1.0, 1.0], 1.0, &F);
        let d = m.step(2.0, 1.0, 300.0, 350.0, Beats(Beats::LAND), &F);
        assert!(d.pitch.is_finite() && d.roll.is_finite() && d.height.is_finite());
        assert!(d.pitch.abs() < 1.0 && d.roll.abs() < 1.0);
    }

    #[test]
    fn knobs_set_the_feel_and_say_what_they_dont_know() {
        let f = Feel::GAME.with("snap=0.2; hold_melee=0.1").unwrap();
        assert_eq!((f.snap, f.hold_melee, f.turn), (0.2, 0.1, Feel::GAME.turn));
        assert!(Feel::GAME.with("nope=1").is_err());
        assert!(Feel::GAME.with("snap").is_err());
        for k in Feel::KNOBS {
            assert!(Feel::GAME.get(k).is_some(), "{k}");
        }
    }
}
