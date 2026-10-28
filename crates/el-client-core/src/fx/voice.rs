//! The announcer and the music (`docs/FX.md`, "Sound"): which line the match calls for, from what
//! the team was told, and one line at a time. The lines' recordings are the owner's to supply
//! (`voice.*` in the sound table, optional); a line without its file is simply not heard.
//!
//! The lines are aimed, as Riot aims theirs: a kill is said from the viewer's side ("you slew",
//! "an ally fell", "an enemy fell"), a structure by whose it was.

use el_sim::units::{Kind, SEATS, team_of_seat};

use super::sound::Sfx;

/// Seconds between a champion's kills that still count as one streak (a double, a triple...).
pub const MULTI_SECS: f64 = 10.0;
/// A line that has waited this long (s) for the one before it to finish is dropped: news that late
/// is noise.
pub const STALE_SECS: f64 = 3.0;

/// What a line is worth when two are waiting: the bigger news goes first.
fn weight(s: Sfx) -> u8 {
    match s {
        Sfx::VoiceVictory | Sfx::VoiceDefeat => 9,
        Sfx::VoiceSurrender => 8,
        Sfx::VoiceAce | Sfx::VoiceMulti5 => 7,
        Sfx::VoiceMulti4 | Sfx::VoiceMulti3 => 6,
        Sfx::VoiceFirstBlood | Sfx::VoiceMulti2 => 5,
        Sfx::VoiceInhibitorDown | Sfx::VoiceInhibitorLost => 4,
        Sfx::VoiceYouSlew | Sfx::VoiceYouFell | Sfx::VoiceTurretDown | Sfx::VoiceTurretLost => 3,
        _ => 2,
    }
}

/// The announcer's sense of the match, and its queue.
#[derive(Clone, Debug)]
pub struct Announcer {
    team: u8,
    seat: usize,
    first_blood: bool,
    /// Each champion's streak: kills in it, and the time of the last.
    streak: [(u8, f64); SEATS],
    /// Each team's champions dead, by seat.
    dead: [bool; SEATS],
    /// Waiting lines: the line, and when it was called.
    queue: Vec<(Sfx, f64)>,
}

impl Announcer {
    pub fn new(seat: usize) -> Self {
        Self {
            team: team_of_seat(seat),
            seat,
            first_blood: false,
            streak: [(0, f64::NEG_INFINITY); SEATS],
            dead: [false; SEATS],
            queue: Vec::with_capacity(8),
        }
    }

    /// The announcer of a match watched: Dawn's side the near one, no champion its own.
    pub fn watching() -> Self {
        Self { seat: super::director::NO_SEAT, ..Self::new(0) }
    }

    fn say(&mut self, line: Sfx, now: f64) {
        if self.queue.len() < 8 {
            self.queue.push((line, now));
        }
    }

    /// The match has begun.
    pub fn welcome(&mut self, now: f64) {
        self.say(Sfx::VoiceWelcome, now);
    }

    /// Thirty seconds before the first wave, and at it.
    pub fn minions(&mut self, soon: bool, now: f64) {
        self.say(if soon { Sfx::VoiceMinionsSoon } else { Sfx::VoiceMinions }, now);
    }

    /// Unit `unit` (of kind `kind`, team `team`) died by `killer`'s hand.
    pub fn death(&mut self, unit: usize, kind: Kind, team: u8, killer: usize, now: f64) {
        let ours = team == self.team;
        match kind {
            Kind::Champion if unit < SEATS => {
                self.dead[unit] = true;
                let mut line = if unit == self.seat {
                    Sfx::VoiceYouFell
                } else if killer == self.seat {
                    Sfx::VoiceYouSlew
                } else if ours {
                    Sfx::VoiceAllyFell
                } else {
                    Sfx::VoiceEnemyFell
                };
                if killer < SEATS && team_of_seat(killer) != team {
                    if !self.first_blood {
                        line = Sfx::VoiceFirstBlood;
                    }
                    let s = &mut self.streak[killer];
                    s.0 = if now - s.1 <= MULTI_SECS { s.0.saturating_add(1) } else { 1 };
                    s.1 = now;
                    line = match s.0 {
                        2 => Sfx::VoiceMulti2,
                        3 => Sfx::VoiceMulti3,
                        4 => Sfx::VoiceMulti4,
                        n if n >= 5 => Sfx::VoiceMulti5,
                        _ => line,
                    };
                }
                self.first_blood = true;
                let side = usize::from(team) * 5;
                if self.dead[side..side + 5].iter().all(|&d| d) {
                    self.say(line, now);
                    line = Sfx::VoiceAce;
                }
                self.say(line, now);
            }
            Kind::Turret => self.say(if ours { Sfx::VoiceTurretLost } else { Sfx::VoiceTurretDown }, now),
            Kind::Inhibitor => {
                self.say(if ours { Sfx::VoiceInhibitorLost } else { Sfx::VoiceInhibitorDown }, now)
            }
            _ => {}
        }
    }

    pub fn respawn(&mut self, unit: usize) {
        if unit < SEATS {
            self.dead[unit] = false;
        }
    }

    pub fn inhibitor_back(&mut self, now: f64) {
        self.say(Sfx::VoiceInhibitorBack, now);
    }

    /// The match is over: `winner` won (`surrendered`: by a vote).
    pub fn outcome(&mut self, winner: u8, surrendered: bool, now: f64) {
        self.queue.clear();
        if surrendered {
            self.say(Sfx::VoiceSurrender, now);
        }
        // A match watched is no one's victory.
        if self.seat != super::director::NO_SEAT {
            self.say(if winner == self.team { Sfx::VoiceVictory } else { Sfx::VoiceDefeat }, now);
        }
    }

    /// The next line to say, if the last one is done (`busy`: one is still being said). Stale ones
    /// go unsaid; of those waiting, the weightiest, then the oldest.
    pub fn next(&mut self, now: f64, busy: bool) -> Option<Sfx> {
        let ends = matches!(self.queue.last(), Some((Sfx::VoiceVictory | Sfx::VoiceDefeat, _)));
        self.queue.retain(|(s, at)| {
            now - at <= STALE_SECS || matches!(s, Sfx::VoiceVictory | Sfx::VoiceDefeat) && ends
        });
        if busy {
            return None;
        }
        let best = (0..self.queue.len()).max_by_key(|&k| (weight(self.queue[k].0), usize::MAX - k))?;
        Some(self.queue.remove(best).0)
    }
}

/// What the music plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Music {
    Quiet,
    Select,
    Match,
    Victory,
    Defeat,
}

impl Music {
    pub fn sound(self) -> Option<Sfx> {
        match self {
            Music::Quiet => None,
            Music::Select => Some(Sfx::MusicSelect),
            Music::Match => Some(Sfx::MusicMatch),
            Music::Victory => Some(Sfx::MusicVictory),
            Music::Defeat => Some(Sfx::MusicDefeat),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(a: &mut Announcer, now: f64) -> Vec<Sfx> {
        let mut out = Vec::new();
        while let Some(s) = a.next(now, false) {
            out.push(s);
        }
        out
    }

    #[test]
    fn kills_are_said_from_where_the_viewer_stands() {
        // Seat 2, of team 0 (seats 0 to 4).
        let mut a = Announcer::new(2);
        a.death(7, Kind::Champion, 1, 2, 10.0);
        assert_eq!(drain(&mut a, 10.0), [Sfx::VoiceFirstBlood], "the first kill is first blood");
        a.death(8, Kind::Champion, 1, 0, 30.0);
        assert_eq!(drain(&mut a, 30.0), [Sfx::VoiceEnemyFell]);
        a.death(2, Kind::Champion, 0, 6, 31.0);
        assert_eq!(drain(&mut a, 31.0), [Sfx::VoiceYouFell]);
        a.death(1, Kind::Champion, 0, 6, 33.0);
        assert_eq!(drain(&mut a, 33.0), [Sfx::VoiceMulti2], "seat 6's second kill in ten seconds");
        a.death(SEATS + 1, Kind::Turret, 1, 0, 40.0);
        assert_eq!(drain(&mut a, 40.0), [Sfx::VoiceTurretDown]);
    }

    #[test]
    fn a_whole_team_down_is_an_ace_and_stale_news_goes_unsaid() {
        let mut a = Announcer::new(0);
        for (k, s) in (5..10).enumerate() {
            a.death(s, Kind::Champion, 1, 0, 100.0 + k as f64 * 20.0);
        }
        let said = drain(&mut a, 180.0);
        assert_eq!(said.first(), Some(&Sfx::VoiceAce), "the ace goes first: {said:?}");
        assert!(!said.contains(&Sfx::VoiceFirstBlood), "first blood was a minute ago: {said:?}");
        a.outcome(0, false, 200.0);
        assert_eq!(a.next(200.0, true), None, "one line at a time");
        assert_eq!(a.next(260.0, false), Some(Sfx::VoiceVictory), "the result is never stale");
    }
}
