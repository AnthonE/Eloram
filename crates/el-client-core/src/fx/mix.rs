//! How the sounds share the speakers (`docs/FX.md`, "Sound"): which may start when many fire at
//! once (a ladder of rungs, Riot's "priority ladder"), what is cut to make room, how far down the
//! lower rungs duck under a big moment, and where each sits in the stereo picture.
//!
//! The ladder, top first: 0, the viewer's own (its casts, what lands on it, a turret turning on it)
//! and the interface; 1, enemies' warnings and ultimates; 2, enemies' casts and hits; 3, allies';
//! 4, the world (minions, turrets on minions). When a fight crowds the mix the lower rungs duck,
//! and the top doesn't get louder.

use glam::Vec2;

use super::look::Side;
use super::sound::{Bus, Sfx};

/// Effects sounding at once, at most.
pub const VOICES: usize = 32;
/// The rungs.
pub const RUNGS: u8 = 5;
/// The rungs from which a sound ducks the others (and the rungs it ducks).
pub const DUCKS_FROM: u8 = 1;
pub const DUCKED: u8 = 3;
/// How far (dB) and how long (s) the lower rungs duck under a big moment, and under a crowd.
pub const DUCK_DB: f32 = -5.0;
pub const DUCK_SECS: f64 = 0.45;
pub const CROWD: usize = 20;
pub const CROWD_DB: f32 = -3.0;

/// The rung a sound starts on, for a viewer on `side` of what made it, rated `rating` (1 to 10)
/// for that viewer.
pub fn rung(side: Side, rating: u8) -> u8 {
    match side {
        Side::Own => {
            if rating >= 3 {
                0
            } else {
                2
            }
        }
        _ if rating >= 8 => 1,
        Side::Enemy if rating >= 4 => 2,
        _ if rating >= 3 => 3,
        _ => 4,
    }
}

/// A sound playing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Voice {
    pub id: u32,
    pub sfx: Sfx,
    pub rung: u8,
    pub started: f64,
    pub ends: f64,
}

/// What became of a sound asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Admit {
    /// Start it (as `id`), cutting `cut` first if one is named.
    Play { id: u32, cut: Option<u32>, take: u8, cents: f32 },
    /// No room: everything sounding is above it.
    Drop,
}

/// The effects' voices: who is sounding, and who gives way.
pub struct Mixer {
    voices: Vec<Voice>,
    next_id: u32,
    rng: u32,
    /// The last take each sound played (to draw another).
    last_take: Vec<u8>,
    /// When the last big moment (a sound on rung [`DUCKS_FROM`] or above) started.
    big_at: f64,
}

impl Default for Mixer {
    fn default() -> Self {
        Self {
            voices: Vec::with_capacity(VOICES + 1),
            next_id: 1,
            rng: 0x9E37_79B9,
            last_take: vec![u8::MAX; Sfx::ALL.len()],
            big_at: f64::NEG_INFINITY,
        }
    }
}

impl Mixer {
    fn rand(&mut self) -> u32 {
        // xorshift32: the takes and pitches drawn need no more than that.
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    /// The voices still sounding at `now`.
    pub fn sounding(&mut self, now: f64) -> &[Voice] {
        self.voices.retain(|v| v.ends > now);
        &self.voices
    }

    /// Asks to start `sfx` on `rung` at `now`, lasting `secs`. A take is drawn (not the last one
    /// played, when it has more), and a pitch a few cents either way, for the effects.
    pub fn admit(&mut self, sfx: Sfx, rung: u8, now: f64, secs: f64) -> Admit {
        let d = sfx.def();
        self.voices.retain(|v| v.ends > now);
        let mut cut = None;
        // Its own cap: the oldest of it gives way.
        let same: Vec<usize> = (0..self.voices.len()).filter(|&k| self.voices[k].sfx == sfx).collect();
        if same.len() >= usize::from(d.cap) {
            let k = *same
                .iter()
                .min_by(|&&a, &&b| self.voices[a].started.total_cmp(&self.voices[b].started))
                .unwrap_or(&0);
            cut = Some(self.voices.remove(k).id);
        } else if self.voices.len() >= VOICES {
            // The whole mix full: the lowest rung's oldest gives way, if it's no higher than this.
            let k = (0..self.voices.len())
                .max_by(|&a, &b| {
                    let (va, vb) = (&self.voices[a], &self.voices[b]);
                    va.rung.cmp(&vb.rung).then(vb.started.total_cmp(&va.started))
                })
                .unwrap_or(0);
            if self.voices[k].rung < rung {
                return Admit::Drop;
            }
            cut = Some(self.voices.remove(k).id);
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.voices.push(Voice { id, sfx, rung, started: now, ends: now + secs });
        if rung <= DUCKS_FROM {
            self.big_at = now;
        }
        let takes = d.takes.max(1);
        let last = self.last_take[sfx as usize];
        let mut take = (self.rand() % u32::from(takes)) as u8;
        if takes > 1 && take == last {
            take = (take + 1 + (self.rand() % u32::from(takes - 1)) as u8) % takes;
        }
        self.last_take[sfx as usize] = take;
        let cents = if d.bus == Bus::Effects { (self.rand() % 81) as f32 - 40.0 } else { 0.0 };
        Admit::Play { id, cut, take, cents }
    }

    /// How far (dB) the lower rungs ([`DUCKED`] and below) sit under the rest at `now`.
    pub fn duck(&self, now: f64) -> f32 {
        let big = if now - self.big_at < DUCK_SECS { DUCK_DB } else { 0.0 };
        let crowd = if self.voices.iter().filter(|v| v.ends > now).count() > CROWD { CROWD_DB } else { 0.0 };
        big + crowd
    }
}

/// Where a sound sits: its pan (-1 left to 1 right), its gain (0 to 1), and the cutoff of the low
/// pass that dulls what's off the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub pan: f32,
    pub gain: f32,
    pub lowpass: f32,
}

/// The place of a sound at `ndc` on the screen (the camera's normalised device coordinates: -1 to
/// 1 across and up the view, past them off it). Equal-power panning is the browser's (a stereo
/// panner); here, how far across it pans (never all the way: a sound hard in one ear reads as
/// wrong on headphones), how it falls away past the edge, and how dull it gets.
pub fn place(ndc: Vec2) -> Place {
    let pan = (ndc.x * 0.8).clamp(-0.85, 0.85);
    // On the screen: nearly full, a little less toward its rim. Off it: fading out over half a
    // screen more, dulled.
    let edge = ndc.x.abs().max(ndc.y.abs());
    let gain =
        if edge <= 1.0 { 1.0 - 0.2 * edge * edge } else { (0.8 * (1.0 - (edge - 1.0) / 0.5)).max(0.0) };
    let lowpass = if edge <= 1.0 { 18_000.0 } else { (18_000.0 * (-3.0 * (edge - 1.0)).exp()).max(900.0) };
    Place { pan, gain, lowpass }
}

/// Decibels as a gain.
pub fn db(v: f32) -> f32 {
    10f32.powf(v / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sound_past_its_cap_cuts_its_oldest_and_a_full_mix_keeps_the_top() {
        let mut m = Mixer::default();
        let cap = Sfx::AutoSwing.def().cap;
        for k in 0..cap {
            assert!(matches!(
                m.admit(Sfx::AutoSwing, 3, f64::from(k) * 0.01, 1.0),
                Admit::Play { cut: None, .. }
            ));
        }
        let Admit::Play { cut: Some(_), .. } = m.admit(Sfx::AutoSwing, 3, 0.2, 1.0) else {
            panic!("its cap")
        };
        // Fill the rest with high rungs: then a low one finds no room.
        let mut n = m.sounding(0.2).len();
        let mut k = 0;
        while n < VOICES {
            let s = Sfx::ALL[k % 40];
            if s != Sfx::AutoSwing && matches!(m.admit(s, 1, 0.3, 5.0), Admit::Play { cut: None, .. }) {
                n += 1;
            }
            k += 1;
            n = n.max(m.sounding(0.3).len());
        }
        assert_eq!(m.admit(Sfx::MinionHit, 4, 0.4, 0.2), Admit::Drop, "a minion doesn't cut an ult");
        assert!(matches!(m.admit(Sfx::StatusStun, 0, 0.4, 0.5), Admit::Play { cut: Some(_), .. }));
        assert!(m.duck(0.5) <= DUCK_DB, "a big moment ducks the rest");
        assert_eq!(m.duck(10.0), 0.0, "and it passes");
    }

    #[test]
    fn takes_vary_and_pitches_stay_close() {
        let mut m = Mixer::default();
        let mut last = u8::MAX;
        for k in 0..40 {
            let Admit::Play { take, cents, .. } = m.admit(Sfx::AutoHit, 3, f64::from(k), 0.1) else {
                panic!()
            };
            assert_ne!(take, last, "the same take twice running");
            assert!(take < Sfx::AutoHit.def().takes && cents.abs() <= 40.0);
            last = take;
        }
    }

    #[test]
    fn the_stereo_picture_follows_the_screen() {
        let mid = place(Vec2::ZERO);
        assert_eq!((mid.pan, mid.gain), (0.0, 1.0));
        let right = place(Vec2::new(0.9, 0.0));
        assert!(right.pan > 0.6 && right.gain > 0.8 && right.lowpass > 10_000.0);
        let off = place(Vec2::new(-1.3, 0.0));
        assert!(off.pan < -0.8 && off.gain < 0.5 && off.lowpass < 8_000.0);
        assert_eq!(place(Vec2::new(0.0, 3.0)).gain, 0.0, "far off the screen, nothing");
        // The ladder: the viewer's own first, then what threatens it.
        assert_eq!(rung(Side::Own, 5), 0);
        assert_eq!(rung(Side::Enemy, 9), 1);
        assert_eq!(rung(Side::Enemy, 5), 2);
        assert_eq!(rung(Side::Ally, 5), 3);
        assert_eq!(rung(Side::Ally, 1), 4);
    }
}
