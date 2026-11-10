//! The desktop's mixing desk: what the browser's Web Audio graph does for the page (`el-client`'s
//! `audio/web.rs`), sample by sample, for the desktop's sound card (`audio/native.rs` feeds it
//! from the audio thread). Pure, so it is tested here, and it neither allocates nor frees once
//! built: every voice, bus and buffer is made in [`Renderer::new`], and a sound's samples are
//! shared ([`Arc`]) with the game thread, which keeps its own handle, so the last one is never
//! dropped on the audio thread.
//!
//! The graph, as the browser's: each voice is a buffer read at a pitch → a low pass (what's off the
//! screen is dulled; RBJ's, as Web Audio's `BiquadFilterNode`, its Q 1 dB) → its gain, eased as
//! `setTargetAtTime` eases → an equal-power stereo panner (Web Audio's `StereoPannerNode`'s
//! formula) → the front of the effects bus, its back (which ducks under a big moment), or a bus of
//! its own; then the six buses → the master → a limiter (−6 dB, a knee of 4, 14:1, 2 ms in,
//! 120 ms out) → the speakers.

use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

/// Frames rendered at a time.
pub const BLOCK: usize = 128;
/// Buses, in `sound::Bus::ALL`'s order (the effects first: its front and back mix into it).
pub const BUSES: usize = super::sound::Bus::ALL.len();

/// A sound's samples, decoded: interleaved, one or two channels.
#[derive(Debug)]
pub struct Pcm {
    pub rate: u32,
    pub channels: u8,
    pub frames: Box<[f32]>,
}

impl Pcm {
    pub fn len(&self) -> usize {
        self.frames.len() / usize::from(self.channels.max(1))
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn secs(&self) -> f64 {
        self.len() as f64 / f64::from(self.rate.max(1))
    }
}

/// Where a voice goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// The effects bus's front: the ladder's top rungs.
    Front,
    /// Its back, which ducks.
    Back,
    /// A bus straight (`k` in [`BUSES`]).
    Bus(u8),
}

/// What the game thread tells the desk.
#[derive(Clone, Debug)]
pub enum Cmd {
    /// A sound's samples, in slot `slot` (a slot is filled once and never replaced).
    Install {
        slot: u16,
        pcm: Arc<Pcm>,
    },
    Start {
        voice: u32,
        slot: u16,
        route: Route,
        gain: f32,
        pan: Option<f32>,
        lowpass: f32,
        rate: f32,
        looped: bool,
    },
    /// A voice's gain eased toward `target` with time constant `tau` (s).
    Gain {
        voice: u32,
        target: f32,
        tau: f32,
    },
    Pan {
        voice: u32,
        target: f32,
        tau: f32,
    },
    /// Faded out over `fade` (s), then let go.
    Stop {
        voice: u32,
        fade: f32,
    },
    Bus {
        k: u8,
        target: f32,
        tau: f32,
    },
    Master {
        target: f32,
        tau: f32,
    },
    /// The effects bus's back (the duck).
    Back {
        target: f32,
        tau: f32,
    },
}

/// A value eased toward its target, one sample at a time.
#[derive(Clone, Copy, Debug, Default)]
struct Ease {
    now: f32,
    target: f32,
    /// The share of the gap closed each sample.
    k: f32,
}

impl Ease {
    fn set(v: f32) -> Self {
        Self { now: v, target: v, k: 1.0 }
    }

    /// `setTargetAtTime`'s curve: 63% of the way in `tau`.
    fn toward(&mut self, target: f32, tau: f32, rate: f32) {
        self.target = target;
        self.k = if tau <= 0.0 { 1.0 } else { 1.0 - (-1.0 / (tau * rate)).exp() };
    }

    #[inline]
    fn step(&mut self) -> f32 {
        self.now += (self.target - self.now) * self.k;
        self.now
    }
}

/// RBJ's low pass, as Web Audio's (`Q` in dB).
#[derive(Clone, Copy, Debug, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    /// Each channel's two samples of state.
    z: [[f32; 2]; 2],
    on: bool,
}

impl Biquad {
    fn lowpass(cutoff: f32, rate: f32) -> Self {
        if !(cutoff > 0.0 && cutoff < 17_000.0 && cutoff < rate * 0.45) {
            return Self::default();
        }
        let w0 = 2.0 * std::f32::consts::PI * cutoff / rate;
        let (sin, cos) = w0.sin_cos();
        // Web Audio's lowpass reads Q in dB: 1 dB.
        let q = 10f32.powf(1.0 / 20.0);
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - cos) / 2.0 / a0,
            b1: (1.0 - cos) / a0,
            b2: (1.0 - cos) / 2.0 / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            z: [[0.0; 2]; 2],
            on: true,
        }
    }

    #[inline]
    fn run(&mut self, ch: usize, x: f32) -> f32 {
        if !self.on {
            return x;
        }
        // Transposed direct form II.
        let z = &mut self.z[ch];
        let y = self.b0 * x + z[0];
        z[0] = self.b1 * x - self.a1 * y + z[1];
        z[1] = self.b2 * x - self.a2 * y;
        y
    }
}

/// Web Audio's equal-power panner, for a mono or a stereo input.
#[inline]
pub fn pan(p: f32, l: f32, r: f32, stereo: bool) -> (f32, f32) {
    let p = p.clamp(-1.0, 1.0);
    if !stereo {
        let x = (p + 1.0) * 0.5 * FRAC_PI_2;
        return (l * x.cos(), l * x.sin());
    }
    if p <= 0.0 {
        let x = (p + 1.0) * FRAC_PI_2;
        (l + r * x.cos(), r * x.sin())
    } else {
        let x = p * FRAC_PI_2;
        (l * x.cos(), r + l * x.sin())
    }
}

#[derive(Clone, Debug, Default)]
struct Voice {
    live: bool,
    id: u32,
    pcm: Option<Arc<Pcm>>,
    pos: f64,
    step: f64,
    looped: bool,
    route: Option<Route>,
    gain: Ease,
    pan: Option<Ease>,
    filter: Biquad,
    stopping: bool,
}

/// The limiter, as Web Audio's compressor set hard and fast (without its make-up gain).
#[derive(Clone, Copy, Debug)]
struct Limiter {
    threshold: f32,
    knee: f32,
    ratio: f32,
    attack: f32,
    release: f32,
    /// The gain reduction now, dB (≤ 0).
    reduce: f32,
}

impl Limiter {
    fn new(rate: f32) -> Self {
        let k = |secs: f32| 1.0 - (-1.0 / (secs * rate)).exp();
        Self { threshold: -6.0, knee: 4.0, ratio: 14.0, attack: k(0.002), release: k(0.12), reduce: 0.0 }
    }

    /// The reduction a level (dB) calls for: none below the knee, the ratio above it, a curve between.
    fn wanted(&self, db: f32) -> f32 {
        let over = db - self.threshold;
        let half = self.knee / 2.0;
        let slope = 1.0 / self.ratio - 1.0;
        if over <= -half {
            0.0
        } else if over >= half {
            over * slope
        } else {
            slope * (over + half) * (over + half) / (2.0 * self.knee)
        }
    }

    #[inline]
    fn run(&mut self, l: f32, r: f32) -> (f32, f32) {
        let peak = l.abs().max(r.abs()).max(1e-9);
        let want = self.wanted(20.0 * peak.log10());
        let k = if want < self.reduce { self.attack } else { self.release };
        self.reduce += (want - self.reduce) * k;
        let g = 10f32.powf(self.reduce / 20.0);
        (l * g, r * g)
    }
}

/// The desk.
pub struct Renderer {
    rate: f32,
    slots: Box<[Option<Arc<Pcm>>]>,
    voices: Box<[Voice]>,
    buses: [Ease; BUSES],
    master: Ease,
    back: Ease,
    /// Scratch: the front and back of the effects bus, and each bus, one block of stereo.
    front_buf: Box<[f32]>,
    back_buf: Box<[f32]>,
    bus_buf: Box<[f32]>,
    limiter: Limiter,
    frames: u64,
    /// Starts refused for want of a free voice, and commands naming no voice.
    pub refused: u32,
}

impl Renderer {
    /// A desk for a sound card at `rate` Hz, with `voices` voices and `slots` sounds.
    pub fn new(rate: u32, voices: usize, slots: usize) -> Self {
        let rate = rate.max(8_000) as f32;
        Self {
            rate,
            slots: (0..slots).map(|_| None).collect(),
            voices: (0..voices).map(|_| Voice::default()).collect(),
            buses: [Ease::set(1.0); BUSES],
            master: Ease::set(1.0),
            back: Ease::set(1.0),
            front_buf: vec![0.0; 2 * BLOCK].into_boxed_slice(),
            back_buf: vec![0.0; 2 * BLOCK].into_boxed_slice(),
            bus_buf: vec![0.0; 2 * BLOCK * BUSES].into_boxed_slice(),
            limiter: Limiter::new(rate),
            frames: 0,
            refused: 0,
        }
    }

    /// Frames rendered so far (the desk's clock).
    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn rate(&self) -> u32 {
        self.rate as u32
    }

    /// Voices sounding now.
    pub fn live(&self) -> usize {
        self.voices.iter().filter(|v| v.live).count()
    }

    fn voice(&mut self, id: u32) -> Option<&mut Voice> {
        self.voices.iter_mut().find(|v| v.live && v.id == id)
    }

    pub fn apply(&mut self, cmd: Cmd) {
        let rate = self.rate;
        match cmd {
            Cmd::Install { slot, pcm } => {
                if let Some(s) = self.slots.get_mut(usize::from(slot))
                    && s.is_none()
                {
                    *s = Some(pcm);
                }
            }
            Cmd::Start { voice, slot, route, gain, pan, lowpass, rate: pitch, looped } => {
                let Some(pcm) = self.slots.get(usize::from(slot)).and_then(|p| p.clone()) else {
                    self.refused += 1;
                    return;
                };
                let Some(v) = self.voices.iter_mut().find(|v| !v.live) else {
                    self.refused += 1;
                    return;
                };
                *v = Voice {
                    live: true,
                    id: voice,
                    step: f64::from(pcm.rate) / f64::from(rate) * f64::from(pitch.max(0.01)),
                    pcm: Some(pcm),
                    pos: 0.0,
                    looped,
                    route: Some(route),
                    gain: Ease::set(gain),
                    pan: pan.map(Ease::set),
                    filter: Biquad::lowpass(lowpass, rate),
                    stopping: false,
                };
            }
            Cmd::Gain { voice, target, tau } => match self.voice(voice) {
                Some(v) => v.gain.toward(target, tau, rate),
                None => self.refused += 1,
            },
            Cmd::Pan { voice, target, tau } => {
                if let Some(Some(p)) = self.voice(voice).map(|v| v.pan.as_mut()) {
                    p.toward(target, tau, rate);
                }
            }
            Cmd::Stop { voice, fade } => {
                if let Some(v) = self.voice(voice) {
                    v.gain.toward(0.0, fade, rate);
                    v.stopping = true;
                }
            }
            Cmd::Bus { k, target, tau } => {
                if let Some(b) = self.buses.get_mut(usize::from(k)) {
                    b.toward(target, tau, rate);
                }
            }
            Cmd::Master { target, tau } => self.master.toward(target, tau, rate),
            Cmd::Back { target, tau } => self.back.toward(target, tau, rate),
        }
    }

    /// Renders one block of stereo (`out` holds `2 * BLOCK` samples, left and right in turn).
    pub fn render(&mut self, out: &mut [f32]) {
        let n = (out.len() / 2).min(BLOCK);
        self.front_buf.fill(0.0);
        self.back_buf.fill(0.0);
        self.bus_buf.fill(0.0);
        for v in self.voices.iter_mut().filter(|v| v.live) {
            let Some(pcm) = v.pcm.clone() else {
                v.live = false;
                continue;
            };
            let stereo = pcm.channels >= 2;
            let ch = usize::from(pcm.channels.max(1));
            let len = pcm.len();
            let target: &mut [f32] = match v.route {
                Some(Route::Front) => &mut self.front_buf,
                Some(Route::Back) => &mut self.back_buf,
                Some(Route::Bus(k)) => {
                    let k = usize::from(k).min(BUSES - 1);
                    &mut self.bus_buf[k * 2 * BLOCK..(k + 1) * 2 * BLOCK]
                }
                None => &mut self.front_buf,
            };
            for i in 0..n {
                if v.pos >= len as f64 {
                    if v.looped && len > 0 {
                        v.pos -= len as f64;
                    } else {
                        v.live = false;
                        break;
                    }
                }
                let at = v.pos as usize;
                let frac = (v.pos - at as f64) as f32;
                let next = if at + 1 < len {
                    at + 1
                } else if v.looped {
                    0
                } else {
                    at
                };
                let read = |c: usize| {
                    let a = pcm.frames[at * ch + c.min(ch - 1)];
                    let b = pcm.frames[next * ch + c.min(ch - 1)];
                    a + (b - a) * frac
                };
                let (mut l, mut r) = (read(0), read(1));
                l = v.filter.run(0, l);
                r = if stereo { v.filter.run(1, r) } else { l };
                let g = v.gain.step();
                let (pl, pr) = match v.pan.as_mut() {
                    Some(p) => pan(p.step(), l, r, stereo),
                    None => (l, r),
                };
                target[2 * i] += pl * g;
                target[2 * i + 1] += pr * g;
                v.pos += v.step;
            }
            if v.stopping && v.gain.now.abs() < 1e-4 {
                v.live = false;
            }
            if !v.live {
                // The samples stay in their slot (the game thread holds them too): nothing is freed here.
                v.pcm = None;
            }
        }
        for i in 0..n {
            let duck = self.back.step();
            let mut l = 0.0;
            let mut r = 0.0;
            for k in 0..BUSES {
                let g = self.buses[k].step();
                let at = k * 2 * BLOCK + 2 * i;
                let (mut bl, mut br) = (self.bus_buf[at], self.bus_buf[at + 1]);
                if k == 0 {
                    bl += self.front_buf[2 * i] + self.back_buf[2 * i] * duck;
                    br += self.front_buf[2 * i + 1] + self.back_buf[2 * i + 1] * duck;
                }
                l += bl * g;
                r += br * g;
            }
            let m = self.master.step();
            let (l, r) = self.limiter.run(l * m, r * m);
            out[2 * i] = l;
            out[2 * i + 1] = r;
        }
        self.frames += n as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, hz: f32, secs: f32) -> Arc<Pcm> {
        let n = (rate as f32 * secs) as usize;
        let frames: Box<[f32]> =
            (0..n).map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin() * 0.25).collect();
        Arc::new(Pcm { rate, channels: 1, frames })
    }

    fn rms(desk: &mut Renderer, blocks: usize) -> (f32, f32) {
        let mut out = [0f32; 2 * BLOCK];
        let (mut l, mut r) = (0.0, 0.0);
        for _ in 0..blocks {
            desk.render(&mut out);
            for f in out.chunks(2) {
                l += f[0] * f[0];
                r += f[1] * f[1];
            }
        }
        let n = (blocks * BLOCK) as f32;
        ((l / n).sqrt(), (r / n).sqrt())
    }

    fn start(desk: &mut Renderer, voice: u32, pan: Option<f32>, lowpass: f32) {
        desk.apply(Cmd::Start {
            voice,
            slot: 0,
            route: Route::Front,
            gain: 1.0,
            pan,
            lowpass,
            rate: 1.0,
            looped: true,
        });
    }

    #[test]
    fn a_low_pass_dulls_what_is_above_it() {
        let mut open = Renderer::new(48_000, 4, 1);
        open.apply(Cmd::Install { slot: 0, pcm: tone(48_000, 4_000.0, 1.0) });
        start(&mut open, 1, None, 0.0);
        let mut dull = Renderer::new(48_000, 4, 1);
        dull.apply(Cmd::Install { slot: 0, pcm: tone(48_000, 4_000.0, 1.0) });
        start(&mut dull, 1, None, 500.0);
        let (a, _) = rms(&mut open, 40);
        let (b, _) = rms(&mut dull, 40);
        assert!(b < a * 0.1, "{b} against {a}");
    }

    #[test]
    fn panned_hard_left_is_heard_on_the_left_alone() {
        let mut desk = Renderer::new(48_000, 4, 1);
        desk.apply(Cmd::Install { slot: 0, pcm: tone(48_000, 440.0, 1.0) });
        start(&mut desk, 1, Some(-1.0), 0.0);
        let (l, r) = rms(&mut desk, 20);
        assert!(l > 0.1 && r < 1e-4, "{l} {r}");
        // The middle: equal power, each side at cos(π/4).
        let (a, b) = pan(0.0, 1.0, 1.0, false);
        assert!((a - b).abs() < 1e-6 && (a - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    }

    #[test]
    fn a_gain_eases_as_set_target_at_time_does() {
        let mut desk = Renderer::new(48_000, 2, 1);
        desk.apply(Cmd::Install {
            slot: 0,
            pcm: Arc::new(Pcm { rate: 48_000, channels: 1, frames: vec![0.1; 48_000].into() }),
        });
        desk.apply(Cmd::Start {
            voice: 1,
            slot: 0,
            route: Route::Bus(2),
            gain: 0.0,
            pan: None,
            lowpass: 0.0,
            rate: 1.0,
            looped: true,
        });
        desk.apply(Cmd::Gain { voice: 1, target: 1.0, tau: 0.1 });
        // 0.1 s is 37.5 blocks of 128 at 48 kHz: after it, about 63% of the way.
        let mut out = [0f32; 2 * BLOCK];
        for _ in 0..37 {
            desk.render(&mut out);
        }
        let now = out[2 * BLOCK - 2] / 0.1;
        assert!((0.58..0.68).contains(&now), "{now}");
    }

    #[test]
    fn the_back_ducks_and_the_limiter_holds_the_top() {
        let mut desk = Renderer::new(48_000, 8, 1);
        desk.apply(Cmd::Install {
            slot: 0,
            pcm: Arc::new(Pcm { rate: 48_000, channels: 1, frames: vec![0.9; 48_000].into() }),
        });
        for v in 0..6 {
            desk.apply(Cmd::Start {
                voice: v,
                slot: 0,
                route: Route::Back,
                gain: 1.0,
                pan: None,
                lowpass: 0.0,
                rate: 1.0,
                looped: true,
            });
        }
        let (loud, _) = rms(&mut desk, 200);
        // Six voices at 0.9 would be 5.4: the limiter brings it to about its threshold's level.
        assert!(loud < 1.1, "{loud}");
        desk.apply(Cmd::Back { target: 0.1, tau: 0.01 });
        let (ducked, _) = rms(&mut desk, 100);
        assert!(ducked < loud * 0.5, "{ducked} against {loud}");
    }

    #[test]
    fn a_stopped_voice_lets_go_and_a_full_desk_refuses() {
        let mut desk = Renderer::new(48_000, 2, 1);
        desk.apply(Cmd::Install { slot: 0, pcm: tone(48_000, 440.0, 1.0) });
        start(&mut desk, 1, None, 0.0);
        start(&mut desk, 2, None, 0.0);
        start(&mut desk, 3, None, 0.0);
        assert_eq!((desk.live(), desk.refused), (2, 1));
        desk.apply(Cmd::Stop { voice: 1, fade: 0.005 });
        let _ = rms(&mut desk, 20);
        assert_eq!(desk.live(), 1);
        // A sound that isn't looped ends by itself.
        desk.apply(Cmd::Start {
            voice: 9,
            slot: 0,
            route: Route::Front,
            gain: 1.0,
            pan: None,
            lowpass: 0.0,
            rate: 2.0,
            looped: false,
        });
        let _ = rms(&mut desk, 200);
        assert_eq!(desk.live(), 1);
    }
}
