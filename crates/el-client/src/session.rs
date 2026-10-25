//! Where the player is (connecting, in the queue, in champion select, in a match, done), and the
//! match's client.

use bevy::prelude::*;
use el_client_core::Client;
use el_client_core::lobby::Player;
use el_proto::event::WireEvent;
use el_sim::units::SEATS;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Connecting,
    /// The server asked for a signature; the wallet has it.
    SigningIn,
    /// Welcomed, not queued.
    Lobby,
    Queued,
    /// Champion select: a reroll, the bench, ready.
    Selecting,
    Playing,
    Over,
    Failed,
    /// Watching a record.
    Watching,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::Connecting => "connecting",
            Phase::SigningIn => "signing_in",
            Phase::Lobby => "lobby",
            Phase::Queued => "queued",
            Phase::Selecting => "selecting",
            Phase::Playing => "playing",
            Phase::Over => "over",
            Phase::Failed => "failed",
            Phase::Watching => "watching",
        }
    }
}

/// Champion select, as the server last told it (`ToClient::Select`).
#[derive(Clone, Debug, Default)]
pub struct SelectView {
    pub seat: usize,
    /// Every seat's champion as our team sees them (the other team's `lobby::UNSEEN`).
    pub picks: Vec<u8>,
    pub bench: Vec<u8>,
    pub rerolls: u8,
    /// When it ends (s, the page's clock).
    pub ends_at: f64,
    /// We said we're ready.
    pub ready: bool,
}

impl SelectView {
    /// Our champion.
    pub fn mine(&self) -> Option<u8> {
        self.picks
            .get(self.seat)
            .copied()
            .filter(|&c| usize::from(c) < el_sim::content::champions::CHAMPIONS.len())
    }
}

/// A line of the kill feed.
#[derive(Clone, Debug)]
pub struct FeedLine {
    pub text: String,
    /// When it came (s, the page's clock).
    pub at: f64,
}

#[derive(Resource, Default)]
pub struct Session {
    pub phase: Phase,
    pub session: Option<u32>,
    /// Welcomed as a wallet (not a guest).
    pub signed_in: bool,
    pub client: Option<Box<Client>>,
    /// Champion select, while it lasts.
    pub select: Option<SelectView>,
    /// The record being watched, instead of a match being played.
    pub watch: Option<Box<crate::watch::Watch>>,
    pub roster: Vec<Player>,
    pub winner: Option<u8>,
    pub waiting: usize,
    pub fill_in: u32,
    pub error: Option<String>,
    /// Each seat's kills and deaths, as the kill feed told them.
    pub kd: [(u16, u16); SEATS],
    pub feed: Vec<FeedLine>,
    /// Teammates' pings heard, all told.
    pub pings: u32,
    /// The client's events read so far.
    feed_read: u64,
}

/// What a ping says, in words.
pub fn ping_words(kind: u8) -> &'static str {
    use el_proto::order::ping;
    match kind {
        ping::DANGER => "careful!",
        ping::COMING => "on my way",
        ping::GONE => "one's missing",
        ping::ATTACK => "go in!",
        ping::BACK => "fall back",
        _ => "look here",
    }
}

impl Session {
    /// The name in seat `s`.
    pub fn name(&self, s: usize) -> &str {
        self.roster.get(s).map_or("?", |p| p.name.as_str())
    }

    /// Takes the client's new events into the kill feed (and anything else the page tallies).
    /// Returns the area flashes among them, for the scene.
    pub fn read_events(&mut self, now: f64, mut area: impl FnMut(&WireEvent)) {
        let Some(c) = &self.client else { return };
        let from = self.feed_read.max(c.feed_n.saturating_sub(el_client_core::client::FEED as u64));
        let mut lines = Vec::new();
        for n in from..c.feed_n {
            let (_, e) = c.feed[(n % el_client_core::client::FEED as u64) as usize];
            match e {
                WireEvent::Death { unit, killer } if usize::from(unit) < SEATS => {
                    let (u, k) = (usize::from(unit), usize::from(killer));
                    self.kd[u].1 += 1;
                    let text = if k < SEATS {
                        self.kd[k].0 += 1;
                        format!("{} slew {}", self.name(k), self.name(u))
                    } else {
                        format!("{} fell", self.name(u))
                    };
                    lines.push(FeedLine { text, at: now });
                }
                WireEvent::Area { .. } | WireEvent::MissileEnd { .. } => area(&e),
                WireEvent::Ping { seat, kind, .. } => {
                    self.pings += 1;
                    let text = format!("{}: {}", self.name(usize::from(seat)), ping_words(kind));
                    lines.push(FeedLine { text, at: now });
                    area(&e);
                }
                WireEvent::Vote { yes, no, open, passed, .. } => {
                    let text = match (open, passed) {
                        (true, _) => format!("Surrender? {} yes, {} no", yes.count_ones(), no.count_ones()),
                        (false, true) => "Your team surrendered".into(),
                        (false, false) => "The vote to surrender failed".into(),
                    };
                    lines.push(FeedLine { text, at: now });
                }
                _ => {}
            }
        }
        self.feed_read = c.feed_n;
        self.feed.extend(lines);
        let keep = self.feed.len().saturating_sub(6);
        self.feed.drain(..keep);
    }
}
