//! Watching: a match's record (`el_sim::record`) stepped here by the same sim, with full vision.
//! `?watch=<id>` follows a live match a minute behind its players (the server serves its record
//! no closer), fetching what's new every few seconds; `?replay=<url>` plays any record from the
//! start. The record is the truth (`docs/NETCODE.md`, "The record"): every hash in it is checked
//! on the way, so what's watched is what was played.

use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use el_sim::record::{Divergence, Frames, Referee};
use el_sim::units::MAX_UNITS;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::net::{LaunchConfigRes, MapRes, now_s};
use crate::session::{Phase, Session};

/// What a fetch brought: the record's bytes so far, or why not.
type Fetched = Arc<Mutex<Option<Result<Vec<u8>, String>>>>;

/// Seconds between fetches of a live record.
const REFETCH: f64 = 3.0;
/// Ticks a frame may step to catch up (a twenty-minute match is caught up in under a minute).
const CATCH_UP: u32 = 900;

pub struct Watch {
    pub referee: Referee,
    pub live: bool,
    bytes: Vec<u8>,
    /// How far the frames have been taken.
    read: usize,
    /// The newest tick the bytes go to.
    pub available: u32,
    /// The tick being shown (fractional), and how fast it runs.
    pub at: f64,
    pub speed: f64,
    /// Units' positions before the last step, to draw between ticks.
    pub prev: Vec<Vec2>,
    last: f64,
    fetched: Fetched,
    next_fetch: f64,
    pub url: String,
    pub error: Option<String>,
    started: bool,
}

impl Watch {
    pub fn new(map: &'static el_sim::map::Map, url: String, live: bool) -> Self {
        Self {
            referee: Referee::new(map),
            live,
            bytes: Vec::new(),
            read: 0,
            available: 0,
            at: 0.0,
            speed: 1.0,
            prev: vec![Vec2::ZERO; MAX_UNITS],
            last: now_s(),
            fetched: Arc::new(Mutex::new(None)),
            next_fetch: 0.0,
            url,
            error: None,
            started: false,
        }
    }

    /// The tick the match is at, as drawn (between the last two stepped).
    pub fn blend(&self) -> f32 {
        let base = f64::from(self.referee.m.tick.saturating_sub(1));
        (self.at - base).clamp(0.0, 1.0) as f32
    }

    /// Where unit `i` is drawn.
    pub fn pos(&self, i: usize) -> Vec2 {
        self.prev[i].lerp(self.referee.m.units[i].pos, self.blend())
    }

    fn fetch(&mut self) {
        let slot = self.fetched.clone();
        let url = self.url.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let got = async {
                let window = web_sys::window().ok_or("no window")?;
                let res: web_sys::Response = JsFuture::from(window.fetch_with_str(&url))
                    .await
                    .map_err(|e| format!("{e:?}"))?
                    .dyn_into()
                    .map_err(|_| "not a response")?;
                if !res.ok() {
                    return Err(format!("{url}: the server answered {}", res.status()));
                }
                let buf = JsFuture::from(res.array_buffer().map_err(|e| format!("{e:?}"))?)
                    .await
                    .map_err(|e| format!("{e:?}"))?;
                Ok(js_sys::Uint8Array::new(&buf).to_vec())
            }
            .await;
            if let Ok(mut s) = slot.lock() {
                *s = Some(got);
            }
        });
    }

    /// Takes what was fetched, steps the match on to the clock, and asks for more when it's time.
    fn advance(&mut self) {
        let now = now_s();
        let dt = (now - self.last).clamp(0.0, 0.5);
        self.last = now;
        let got = self.fetched.lock().ok().and_then(|mut s| s.take());
        if let Some(got) = got {
            match got {
                Ok(bytes) if bytes.len() >= self.bytes.len() => {
                    self.bytes = bytes;
                    self.available = Frames::new(&self.bytes)
                        .filter_map(|f| match f {
                            el_sim::record::Frame::Tick { tick, .. } => Some(tick + 1),
                            _ => None,
                        })
                        .last()
                        .unwrap_or(self.available);
                }
                Ok(_) => {}
                Err(e) => self.error = Some(e),
            }
            if !self.started && self.available > 0 {
                self.started = true;
                // A live match: straight to the newest the server gives, less a moment to buffer.
                if self.live {
                    self.at = f64::from(self.available.saturating_sub(60));
                }
            }
        }
        if now >= self.next_fetch && (self.live || self.bytes.is_empty()) {
            self.next_fetch = now + REFETCH;
            self.fetch();
        }
        if !self.started {
            return;
        }
        self.at = (self.at + dt * f64::from(el_sim::TICK_HZ) * self.speed).min(f64::from(self.available));
        let want = self.at.floor() as u32;
        let mut budget = CATCH_UP;
        while self.referee.m.tick < want.max(1) && budget > 0 && self.error.is_none() {
            let Some((f, used)) = el_sim::record::read_frame(&self.bytes[self.read..]).ok().flatten() else {
                break;
            };
            self.read += used;
            if matches!(f, el_sim::record::Frame::Tick { .. }) {
                for (p, u) in self.prev.iter_mut().zip(self.referee.m.units.iter()) {
                    *p = u.pos;
                }
                budget -= 1;
            }
            if let Err(d) = self.referee.take(&f) {
                self.error = Some(match d {
                    Divergence::Hash { tick, .. } => {
                        format!("the replay went astray of its record by tick {tick}")
                    }
                    Divergence::Version { .. } => "the record is from another version of the game".into(),
                    other => format!("the record doesn't read: {other:?}"),
                });
            }
        }
        // Behind by more than a frame's catching up: what's drawn is where the match is.
        if f64::from(self.referee.m.tick) + 1.0 < self.at {
            self.at = f64::from(self.referee.m.tick);
        }
    }
}

pub struct WatchPlugin;

impl Plugin for WatchPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, start).add_systems(Update, (step, speed).before(crate::net::NetSet::Send));
    }
}

fn start(cfg: Res<LaunchConfigRes>, map: Res<MapRes>, mut session: ResMut<Session>) {
    if let Some(url) = cfg.0.record_url.clone() {
        session.watch = Some(Box::new(Watch::new(map.0, url, cfg.0.watch_live)));
        session.phase = Phase::Watching;
    }
}

fn step(mut session: ResMut<Session>) {
    if let Some(w) = session.watch.as_mut() {
        w.advance();
    }
}

/// `]` doubles the speed, `[` halves it (a replay; a live match can't run ahead of its delay).
fn speed(keys: Res<ButtonInput<KeyCode>>, mut session: ResMut<Session>) {
    let Some(w) = session.watch.as_mut() else { return };
    if keys.just_pressed(KeyCode::BracketRight) {
        w.speed = (w.speed * 2.0).min(16.0);
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        w.speed = (w.speed * 0.5).max(0.25);
    }
}
