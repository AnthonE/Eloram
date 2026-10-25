//! Server-time estimation and input lead (Before Colony's `clock.rs`, steered on Gates' buffer
//! depth).
//!
//! - `server_now(now)` is the server's simulation time in ticks (continuous): snapshot `T` is
//!   produced as tick `T` completes, so on arrival the server is about half an RTT past `T`.
//! - Commands for tick `k` must reach the server before it simulates `k`. The client runs its
//!   input clock `lead` ticks ahead of `server_now`, steering `lead` so the server never reports
//!   less than ~2 ticks of buffered input (`input_health`). It is the *minimum* that matters: a
//!   client that sends in bursts (a slow frame rate, an agent that thinks twice a second) has a
//!   full buffer just after each burst and an empty one just before the next.
//! - Everyone else is drawn `interp_delay` ticks in the past, between two received snapshots.
//!   Missiles are drawn at the input clock, where the own champion is, so a dodge on screen is a
//!   dodge on the server (`docs/NETCODE.md`, "Closed forms go once").
//! - What is drawn runs on eased clocks: the times the own champion and everyone else are drawn at
//!   ([`Clock::own_tick`], [`Clock::view_tick`]) follow their estimates at a few percent off real
//!   time rather than jumping with each correction, so nothing on screen lurches when the clock is
//!   put right. Commands are scheduled on the raw estimate ([`Clock::input_tick`]).

use el_proto::TICK_HZ;

const HZ: f64 = TICK_HZ as f64;
/// Target input-buffer depth at the server, ticks.
pub const TARGET_HEALTH: f64 = 2.0;
/// Snapshots over which the lowest input-buffer depth is tracked (1 s).
const HEALTH_WINDOW: usize = 30;
/// A snapshot this far from the estimate, ticks: a first sync, or the server's clock moved.
const FAR: f64 = 6.0;
/// Far-late snapshots in a row before the estimate is moved back to them: one burst stamped late
/// by a stalled page mustn't drag the clock back.
const LATE_RUN: u32 = 15;
/// The most one late snapshot moves the estimate back, ticks.
const LATE_STEP: f64 = 0.05;
/// How the drawn clocks close on their estimates (rad/s): a critically damped follow, so they
/// never run more than a few percent off real time and never change pace abruptly.
const EASE: f64 = 2.5;
/// A drawn clock this far off its estimate (ticks) jumps to it.
const EASE_SNAP: f64 = 4.0;

/// An offset (ticks) that follows its target on a critically damped spring instead of jumping,
/// as a function of time: its pace changes smoothly too.
#[derive(Clone, Copy, Debug, Default)]
struct Ease {
    /// Where it's heading.
    to: f64,
    /// How far off it was (`gap`, ticks) and how fast that was closing (`pace`, ticks/s) at `since`.
    gap: f64,
    pace: f64,
    since: f64,
    set: bool,
}

impl Ease {
    /// The gap and its rate of change `secs` after `since`.
    fn state(&self, now: f64) -> (f64, f64) {
        let t = (now - self.since).max(0.0);
        let e = (-EASE * t).exp();
        let j = self.pace + self.gap * EASE;
        ((self.gap + j * t) * e, (self.pace - j * EASE * t) * e)
    }

    fn at(&self, now: f64) -> f64 {
        self.to + self.state(now).0
    }

    /// Its rate against real time at `now` (1 = real time).
    fn rate(&self, now: f64) -> f64 {
        1.0 + self.state(now).1 / HZ
    }

    fn aim(&mut self, now: f64, to: f64) {
        let (gap, pace) = self.state(now);
        let here = self.to + gap;
        (self.gap, self.pace) =
            if !self.set || (to - here).abs() > EASE_SNAP { (0.0, 0.0) } else { (here - to, pace) };
        self.to = to;
        self.since = now;
        self.set = true;
    }
}

#[derive(Clone, Debug)]
pub struct Clock {
    offset: Option<f64>,
    /// Smoothed round-trip time, s.
    pub rtt: f64,
    /// Input clock lead over server time, ticks.
    pub lead: f64,
    /// Remote entities are rendered this many ticks behind server time.
    pub interp_delay: f64,
    /// Latest input-buffer depth reported by the server.
    pub health: i8,
    /// The last [`HEALTH_WINDOW`] depths (a ring).
    recent: [i8; HEALTH_WINDOW],
    recent_n: usize,
    /// Far-late snapshots in a row.
    late_run: u32,
    /// The drawn clocks' offsets from local time: the own champion's (the input clock's) and
    /// everyone else's.
    own: Ease,
    view: Ease,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            offset: None,
            rtt: 0.1,
            lead: 4.0,
            interp_delay: 3.0,
            health: 0,
            recent: [0; HEALTH_WINDOW],
            recent_n: 0,
            late_run: 0,
            own: Ease::default(),
            view: Ease::default(),
        }
    }
}

impl Clock {
    pub fn synced(&self) -> bool {
        self.offset.is_some()
    }

    /// Server time (ticks) at local time `now` (s).
    pub fn server_now(&self, now: f64) -> f64 {
        now * HZ + self.offset.unwrap_or(0.0)
    }

    /// Tick the next input should target.
    pub fn input_tick(&self, now: f64) -> u32 {
        (self.server_now(now) + self.lead).floor().max(0.0) as u32
    }

    /// The input clock as drawn (ticks): eased onto `server_now + lead`. The own champion is drawn a
    /// tick behind it, between the last two ticks predicted.
    pub fn own_tick(&self, now: f64) -> f64 {
        if !self.own.set {
            return self.server_now(now) + self.lead;
        }
        now * HZ + self.own.at(now)
    }

    /// How fast [`Clock::own_tick`] runs at `now`, against real time (1 = real time).
    pub fn own_rate(&self, now: f64) -> f64 {
        if self.own.set { self.own.rate(now) } else { 1.0 }
    }

    /// Time (ticks) at which everyone else is drawn: eased onto `server_now − interp_delay`.
    /// (There is no lag compensation: nothing is judged at this time, only drawn.)
    pub fn view_tick(&self, now: f64) -> f64 {
        if !self.view.set {
            return self.server_now(now) - self.interp_delay;
        }
        now * HZ + self.view.at(now)
    }

    /// Feeds one snapshot: its tick, arrival time, and optional RTT sample (s).
    pub fn on_snapshot(&mut self, tick: u32, recv_now: f64, rtt_sample: Option<f64>, health: i8) {
        if let Some(r) = rtt_sample.filter(|r| r.is_finite() && *r >= 0.0 && *r < 5.0) {
            self.rtt = if self.offset.is_none() { r } else { self.rtt * 0.9 + r * 0.1 };
        }
        let sample = f64::from(tick) + self.rtt * 0.5 * HZ - recv_now * HZ;
        let far_late = self.offset.is_some_and(|o| o - sample > FAR);
        self.late_run = if far_late { self.late_run + 1 } else { 0 };
        self.offset = Some(match self.offset {
            None => sample,
            // Far early (first sync, a server hitch): snap. Far late: only once it has kept up for
            // a while, since a page that stalled stamps a whole burst of snapshots late at once.
            Some(o) if sample - o > FAR => sample,
            Some(_) if self.late_run >= LATE_RUN => {
                self.late_run = 0;
                sample
            }
            // Otherwise track gently; late snapshots pull the estimate back less than early ones
            // push it forward.
            Some(o) if sample > o => o + 0.1 * (sample - o),
            Some(o) => o + (0.02 * (sample - o)).max(-LATE_STEP),
        });
        self.health = health;
        self.recent[self.recent_n % HEALTH_WINDOW] = health;
        self.recent_n += 1;
        let lowest = self.recent[..self.recent_n.min(HEALTH_WINDOW)].iter().copied().min().unwrap_or(health);
        // Running dry: lengthen the lead at once. Spare input for a whole second: shorten it
        // slowly. In between, hold (the dead band keeps it from chattering).
        let h = f64::from(health);
        let low = f64::from(lowest);
        if h < TARGET_HEALTH {
            self.lead += 0.1 * (TARGET_HEALTH - h);
        } else if low > TARGET_HEALTH {
            self.lead -= 0.02 * (low - TARGET_HEALTH);
        }
        // Bursty senders can need a long lead; the minimum keeps it short on steady ones.
        let base = self.rtt * 0.5 * HZ;
        self.lead = self.lead.clamp(base + 0.5, base + 45.0);
        // More jitter → more interpolation delay (2–6 ticks).
        self.interp_delay = (2.0 + self.rtt * HZ * 0.25).clamp(2.0, 6.0);
        let offset = self.offset.unwrap_or(0.0);
        self.own.aim(recv_now, offset + self.lead);
        self.view.aim(recv_now, offset - self.interp_delay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clock fed a snapshot every tick for `secs` from `t0`, arriving `late` seconds late, with
    /// the server `shift` ticks along; returns the time it got to.
    fn feed(c: &mut Clock, t0: f64, secs: f64, shift: f64, late: f64) -> f64 {
        let mut t = t0;
        while t < t0 + secs {
            t += 1.0 / HZ;
            let tick = ((t - 0.05) * HZ + shift) as u32;
            c.on_snapshot(tick, t + late, Some(0.1), 2);
        }
        t
    }

    #[test]
    fn the_drawn_clocks_ease_rather_than_jump() {
        let mut c = Clock::default();
        let t = feed(&mut c, 0.0, 3.0, 0.0, 0.0);
        // The estimate jumps by half a tick (a lead correction, say): the drawn clocks run a few
        // percent fast until they have caught up, never jumping.
        c.lead += 0.5;
        c.on_snapshot((t * HZ) as u32, t, Some(0.1), 2);
        let (mut prev, mut prev_view) = (c.own_tick(t), c.view_tick(t));
        let mut now = t;
        for _ in 0..600 {
            now += 0.001;
            let (own, view) = (c.own_tick(now), c.view_tick(now));
            let rate = (own - prev) / 0.001 / HZ;
            assert!((0.95..=1.05).contains(&rate), "own clock at {rate:.3}× real time");
            let rate = (view - prev_view) / 0.001 / HZ;
            assert!((0.95..=1.05).contains(&rate), "view clock at {rate:.3}× real time");
            (prev, prev_view) = (own, view);
        }
        let target = c.server_now(now) + c.lead;
        assert!((c.own_tick(now + 20.0) - (target + 20.0 * HZ)).abs() < 1e-6, "it gets there");
    }

    #[test]
    fn a_stall_stamped_burst_doesnt_drag_the_clock_back() {
        let mut c = Clock::default();
        let t = feed(&mut c, 0.0, 3.0, 0.0, 0.0);
        let before = c.server_now(t);
        // Ten snapshots that queued up behind a stalled page, all stamped 300 ms late.
        for k in 0..10 {
            let tick = ((t - 0.05) * HZ) as u32 - 10 + k;
            c.on_snapshot(tick, t + 0.3, Some(0.1), 2);
        }
        let back = before - c.server_now(t);
        assert!(back <= 0.5 + 1e-9, "dragged back {back:.2} ticks");
    }

    #[test]
    fn a_server_clock_that_really_moved_back_is_followed() {
        let mut c = Clock::default();
        let t = feed(&mut c, 0.0, 3.0, 0.0, 0.0);
        // The server resynchronised 12 ticks back (after a long overrun).
        let t = feed(&mut c, t, 0.6, -12.0, 0.0);
        let want = (t - 0.05) * HZ - 12.0 + 0.05 * HZ;
        assert!((c.server_now(t) - want).abs() < 1.0, "estimate {:.1}, want {want:.1}", c.server_now(t));
        // Too far to ease: the drawn clocks jump with it (then ease the last little way).
        assert!((c.view_tick(t) - (c.server_now(t) - c.interp_delay)).abs() < 0.1, "drawn clocks jump too");
    }

    #[test]
    fn eases_smoothly_at_the_rate_it_reports() {
        let mut e = Ease::default();
        e.aim(0.0, 10.0);
        e.aim(0.0, 12.5);
        let mut prev_rate = e.rate(0.0);
        for k in 1..800 {
            let now = f64::from(k) * 0.005;
            // Retargeted part-way: the pace carries on without a jump.
            if k == 100 {
                e.aim(now, 11.0);
            }
            let d = (e.at(now + 1e-5) - e.at(now)) / 1e-5 / HZ;
            let rate = e.rate(now);
            assert!((d - (rate - 1.0)).abs() < 1e-3, "at {now}: moved {d}, said {}", rate - 1.0);
            assert!((rate - prev_rate).abs() < 0.01, "pace jumped from {prev_rate} to {rate} at {now}");
            assert!((rate - 1.0).abs() < 0.1, "{rate}× real time at {now}");
            prev_rate = rate;
        }
        assert!((e.at(100.0) - 11.0).abs() < 1e-9);
    }
}
