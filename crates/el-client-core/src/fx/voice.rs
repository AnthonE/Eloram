//! The announcer and the music (`docs/FX.md`, "Sound" and "The voices"): which line the match calls
//! for, from what the team was told, and one line at a time. Each line's words are in [`says`]
//! (the script a recording is made from, and the banner the HUD shows as it's said); the files are
//! placeholders (`art/tools/voices.py`) until a recording of the same name replaces one, and a
//! line without its file is shown and not heard.
//!
//! The lines are aimed, as Riot aims theirs: a kill is said from the viewer's side ("you slew",
//! "an ally has fallen", "an enemy has fallen"), and so are a run of kills ("on a run" for one of
//! ours, "an enemy is on a run" for one of theirs), a multikill, an ace, a structure, an inhibitor
//! coming back and a surrender. League's structure, in the game's own words: first blood; two to
//! five kills close together; three to eight and more without falling, and the run broken; a
//! champion killed by no champion; the whole team down.

use el_sim::units::{Kind, SEATS, TEAM_SIZE, team_of_seat};

use super::sound::Sfx;

/// Seconds between a champion's kills that still count as one streak (a double, a triple...).
pub const MULTI_SECS: f64 = 10.0;
/// A line that has waited this long (s) for the ones before it to finish is dropped: news that late
/// is noise. Big news keeps a little longer.
pub const STALE_SECS: f64 = 3.0;
pub const STALE_BIG_SECS: f64 = 5.0;
/// Kills without falling that make a run (and that the run's breaking is called for).
pub const SPREE: u8 = 3;

/// What the announcer says for each of its lines: what's recorded, and what the banner shows.
pub fn says(s: Sfx) -> Option<&'static str> {
    Some(match s {
        Sfx::VoiceWelcome => "Welcome to the Causeway.",
        Sfx::VoiceMinionsSoon => "Thirty seconds until the minions march.",
        Sfx::VoiceMinions => "The minions march.",
        Sfx::VoiceFirstBlood => "First blood!",
        Sfx::VoiceYouSlew => "You slew an enemy.",
        Sfx::VoiceYouFell => "You have fallen.",
        Sfx::VoiceAllyFell => "An ally has fallen.",
        Sfx::VoiceEnemyFell => "An enemy has fallen.",
        Sfx::VoiceMulti2 => "Double kill!",
        Sfx::VoiceMulti3 => "Triple kill!",
        Sfx::VoiceMulti4 => "Fourfold!",
        Sfx::VoiceMulti5 => "Clean sweep!",
        Sfx::VoiceEnemyMulti2 => "Enemy double kill.",
        Sfx::VoiceEnemyMulti3 => "Enemy triple kill.",
        Sfx::VoiceEnemyMulti4 => "Enemy fourfold.",
        Sfx::VoiceEnemyMulti5 => "An enemy swept your team.",
        Sfx::VoiceAce => "That's an ace!",
        Sfx::VoiceAced => "Your team is down.",
        Sfx::VoiceSpree3 => "On a run!",
        Sfx::VoiceSpree4 => "Rampant!",
        Sfx::VoiceSpree5 => "Untouchable!",
        Sfx::VoiceSpree6 => "Merciless!",
        Sfx::VoiceSpree7 => "Fearsome!",
        Sfx::VoiceSpree8 => "Beyond legend!",
        Sfx::VoiceEnemySpree3 => "An enemy is on a run.",
        Sfx::VoiceEnemySpree4 => "An enemy is rampant.",
        Sfx::VoiceEnemySpree5 => "An enemy is untouchable.",
        Sfx::VoiceEnemySpree6 => "An enemy is merciless.",
        Sfx::VoiceEnemySpree7 => "An enemy is fearsome.",
        Sfx::VoiceEnemySpree8 => "An enemy is beyond legend!",
        Sfx::VoiceShutdown => "The run is broken!",
        Sfx::VoiceExecuted => "Felled by the Causeway.",
        Sfx::VoiceTurretDown => "Your team has destroyed a turret.",
        Sfx::VoiceTurretLost => "Your turret has fallen.",
        Sfx::VoiceInhibitorDown => "Your team has destroyed an inhibitor!",
        Sfx::VoiceInhibitorLost => "Your inhibitor has fallen.",
        Sfx::VoiceInhibitorSoon => "Your inhibitor rises soon.",
        Sfx::VoiceEnemyInhibitorSoon => "The enemy inhibitor rises soon.",
        Sfx::VoiceInhibitorBack => "Your inhibitor stands again.",
        Sfx::VoiceEnemyInhibitorBack => "The enemy inhibitor stands again.",
        Sfx::VoiceSurrender => "Your team has surrendered.",
        Sfx::VoiceEnemySurrender => "The enemy has surrendered.",
        Sfx::VoiceVictory => "Victory!",
        Sfx::VoiceDefeat => "Defeat.",
        _ => return None,
    })
}

/// What a line is worth when two are waiting: the bigger news goes first.
fn weight(s: Sfx) -> u8 {
    match s {
        Sfx::VoiceVictory | Sfx::VoiceDefeat => 9,
        Sfx::VoiceSurrender | Sfx::VoiceEnemySurrender => 8,
        Sfx::VoiceAce | Sfx::VoiceAced | Sfx::VoiceMulti5 | Sfx::VoiceEnemyMulti5 => 7,
        Sfx::VoiceMulti4 | Sfx::VoiceMulti3 | Sfx::VoiceEnemyMulti4 | Sfx::VoiceEnemyMulti3 => 6,
        Sfx::VoiceFirstBlood | Sfx::VoiceMulti2 | Sfx::VoiceEnemyMulti2 | Sfx::VoiceShutdown => 5,
        Sfx::VoiceInhibitorDown | Sfx::VoiceInhibitorLost => 4,
        Sfx::VoiceSpree3
        | Sfx::VoiceSpree4
        | Sfx::VoiceSpree5
        | Sfx::VoiceSpree6
        | Sfx::VoiceSpree7
        | Sfx::VoiceSpree8
        | Sfx::VoiceEnemySpree3
        | Sfx::VoiceEnemySpree4
        | Sfx::VoiceEnemySpree5
        | Sfx::VoiceEnemySpree6
        | Sfx::VoiceEnemySpree7
        | Sfx::VoiceEnemySpree8 => 4,
        Sfx::VoiceYouSlew | Sfx::VoiceYouFell | Sfx::VoiceTurretDown | Sfx::VoiceTurretLost => 3,
        Sfx::VoiceExecuted => 3,
        _ => 2,
    }
}

/// A line said: what, about whom (seats: for the banner's names), and whether it's good news for
/// the viewer's side (none: neither).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Said {
    pub line: Sfx,
    pub killer: Option<u8>,
    pub victim: Option<u8>,
    pub good: Option<bool>,
}

impl Said {
    pub fn says(&self) -> &'static str {
        says(self.line).unwrap_or("")
    }
}

/// A line waiting: what, when it was called, its group's weight and its place in the queue (lines
/// called together are said together, in the order they were called).
#[derive(Clone, Copy, Debug)]
struct Waiting {
    said: Said,
    at: f64,
    weight: u8,
    seq: u32,
}

/// The announcer's sense of the match, and its queue.
#[derive(Clone, Debug)]
pub struct Announcer {
    team: u8,
    seat: usize,
    first_blood: bool,
    /// Each champion's kills close together: how many, and the time of the last.
    streak: [(u8, f64); SEATS],
    /// Each champion's kills since it last fell.
    spree: [u8; SEATS],
    /// Each team's champions dead, by seat.
    dead: [bool; SEATS],
    queue: Vec<Waiting>,
    /// The lines of the moment being called (one kill can call three), before they're queued.
    group: Vec<Said>,
    seq: u32,
}

impl Announcer {
    pub fn new(seat: usize) -> Self {
        Self {
            team: team_of_seat(seat.min(SEATS - 1)),
            seat,
            first_blood: false,
            streak: [(0, f64::NEG_INFINITY); SEATS],
            spree: [0; SEATS],
            dead: [false; SEATS],
            queue: Vec::with_capacity(16),
            group: Vec::with_capacity(4),
            seq: 0,
        }
    }

    /// The announcer of a match watched: Dawn's side the near one, no champion its own.
    pub fn watching() -> Self {
        Self { seat: super::director::NO_SEAT, ..Self::new(0) }
    }

    /// The team the announcer speaks to.
    pub fn team(&self) -> u8 {
        self.team
    }

    fn line(&mut self, line: Sfx, killer: Option<usize>, victim: Option<usize>, good: Option<bool>) {
        let seat = |s: Option<usize>| s.filter(|&s| s < SEATS).map(|s| s as u8);
        self.group.push(Said { line, killer: seat(killer), victim: seat(victim), good });
    }

    /// The moment's lines, queued together under the weight of the biggest.
    fn call(&mut self, now: f64) {
        let weight = self.group.iter().map(|s| self::weight(s.line)).max().unwrap_or(0);
        for k in 0..self.group.len() {
            if self.queue.len() >= 16 {
                break;
            }
            self.seq = self.seq.wrapping_add(1);
            self.queue.push(Waiting { said: self.group[k], at: now, weight, seq: self.seq });
        }
        self.group.clear();
    }

    fn say(&mut self, line: Sfx, good: Option<bool>, now: f64) {
        self.line(line, None, None, good);
        self.call(now);
    }

    /// The match has begun.
    pub fn welcome(&mut self, now: f64) {
        self.say(Sfx::VoiceWelcome, None, now);
    }

    /// Thirty seconds before the first wave, and at it.
    pub fn minions(&mut self, soon: bool, now: f64) {
        self.say(if soon { Sfx::VoiceMinionsSoon } else { Sfx::VoiceMinions }, None, now);
    }

    /// Unit `unit` (of kind `kind`, team `team`) died by `killer`'s hand (a slot: a champion's seat,
    /// or a turret's, a minion's, the platform's).
    pub fn death(&mut self, unit: usize, kind: Kind, team: u8, killer: usize, now: f64) {
        let ours = team == self.team;
        match kind {
            Kind::Champion if unit < SEATS => {
                self.dead[unit] = true;
                let run = core::mem::take(&mut self.spree[unit]);
                self.streak[unit] = (0, f64::NEG_INFINITY);
                let by_champion = killer < SEATS && team_of_seat(killer) != team;
                // Good news if it's theirs that fell.
                let good = Some(!ours);
                let mut main = if !by_champion {
                    Sfx::VoiceExecuted
                } else if unit == self.seat {
                    Sfx::VoiceYouFell
                } else if killer == self.seat {
                    Sfx::VoiceYouSlew
                } else if ours {
                    Sfx::VoiceAllyFell
                } else {
                    Sfx::VoiceEnemyFell
                };
                let mut spree = None;
                if by_champion {
                    if !self.first_blood {
                        main = Sfx::VoiceFirstBlood;
                        self.first_blood = true;
                    }
                    let s = &mut self.streak[killer];
                    s.0 = if now - s.1 <= MULTI_SECS { s.0.saturating_add(1) } else { 1 };
                    s.1 = now;
                    let theirs = team_of_seat(killer) != self.team;
                    main = match (s.0, theirs) {
                        (2, false) => Sfx::VoiceMulti2,
                        (3, false) => Sfx::VoiceMulti3,
                        (4, false) => Sfx::VoiceMulti4,
                        (n, false) if n >= 5 => Sfx::VoiceMulti5,
                        (2, true) => Sfx::VoiceEnemyMulti2,
                        (3, true) => Sfx::VoiceEnemyMulti3,
                        (4, true) => Sfx::VoiceEnemyMulti4,
                        (n, true) if n >= 5 => Sfx::VoiceEnemyMulti5,
                        _ => main,
                    };
                    self.spree[killer] = self.spree[killer].saturating_add(1);
                    let n = self.spree[killer];
                    if n >= SPREE {
                        let k = usize::from(n.min(8) - SPREE);
                        let ally = [
                            Sfx::VoiceSpree3,
                            Sfx::VoiceSpree4,
                            Sfx::VoiceSpree5,
                            Sfx::VoiceSpree6,
                            Sfx::VoiceSpree7,
                            Sfx::VoiceSpree8,
                        ];
                        let enemy = [
                            Sfx::VoiceEnemySpree3,
                            Sfx::VoiceEnemySpree4,
                            Sfx::VoiceEnemySpree5,
                            Sfx::VoiceEnemySpree6,
                            Sfx::VoiceEnemySpree7,
                            Sfx::VoiceEnemySpree8,
                        ];
                        spree = Some(if theirs { enemy[k] } else { ally[k] });
                    }
                }
                let killer = (killer < SEATS).then_some(killer);
                self.line(main, killer, Some(unit), good);
                if by_champion && run >= SPREE {
                    self.line(Sfx::VoiceShutdown, killer, Some(unit), good);
                }
                let side = usize::from(team) * TEAM_SIZE;
                if self.dead[side..side + TEAM_SIZE].iter().all(|&d| d) {
                    self.line(if ours { Sfx::VoiceAced } else { Sfx::VoiceAce }, killer, Some(unit), good);
                }
                if let Some(s) = spree {
                    self.line(s, killer, None, Some(!ours));
                }
                self.call(now);
            }
            Kind::Turret => {
                let line = if ours { Sfx::VoiceTurretLost } else { Sfx::VoiceTurretDown };
                self.say(line, Some(!ours), now);
            }
            Kind::Inhibitor => {
                let line = if ours { Sfx::VoiceInhibitorLost } else { Sfx::VoiceInhibitorDown };
                self.say(line, Some(!ours), now);
            }
            _ => {}
        }
    }

    pub fn respawn(&mut self, unit: usize) {
        if unit < SEATS {
            self.dead[unit] = false;
        }
    }

    /// Team `team`'s inhibitor stands again soon (`soon`), or now.
    pub fn inhibitor(&mut self, team: u8, soon: bool, now: f64) {
        let ours = team == self.team;
        let line = match (ours, soon) {
            (true, true) => Sfx::VoiceInhibitorSoon,
            (false, true) => Sfx::VoiceEnemyInhibitorSoon,
            (true, false) => Sfx::VoiceInhibitorBack,
            (false, false) => Sfx::VoiceEnemyInhibitorBack,
        };
        self.say(line, Some(ours), now);
    }

    /// The match is over: `winner` won; `surrendered` names the team that gave up, if one did.
    pub fn outcome(&mut self, winner: u8, surrendered: Option<u8>, now: f64) {
        self.queue.clear();
        if let Some(t) = surrendered {
            let ours = t == self.team;
            self.line(
                if ours { Sfx::VoiceSurrender } else { Sfx::VoiceEnemySurrender },
                None,
                None,
                Some(!ours),
            );
        }
        // A match watched is no one's victory.
        if self.seat != super::director::NO_SEAT {
            let won = winner == self.team;
            self.line(if won { Sfx::VoiceVictory } else { Sfx::VoiceDefeat }, None, None, Some(won));
        }
        self.call(now);
    }

    /// The next line to say, if the last one is done (`busy`: one is still being said). Stale ones
    /// go unsaid; of those waiting, the weightiest moment, then the oldest, and a moment's lines in
    /// the order they were called.
    pub fn next(&mut self, now: f64, busy: bool) -> Option<Said> {
        // The end of the match is never stale.
        let ends = |s: Sfx| {
            matches!(s, Sfx::VoiceVictory | Sfx::VoiceDefeat | Sfx::VoiceSurrender | Sfx::VoiceEnemySurrender)
        };
        self.queue.retain(|w| {
            let stale = if w.weight >= 5 { STALE_BIG_SECS } else { STALE_SECS };
            now - w.at <= stale || ends(w.said.line)
        });
        if busy {
            return None;
        }
        let best =
            (0..self.queue.len()).max_by_key(|&k| (self.queue[k].weight, u32::MAX - self.queue[k].seq))?;
        Some(self.queue.remove(best).said)
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
    use crate::fx::sound::Bus;

    fn drain(a: &mut Announcer, now: f64) -> Vec<Sfx> {
        let mut out = Vec::new();
        while let Some(s) = a.next(now, false) {
            out.push(s.line);
        }
        out
    }

    #[test]
    fn every_line_of_the_announcers_has_its_words() {
        for &s in Sfx::ALL {
            let d = s.def();
            if d.bus == Bus::Voice {
                assert!(says(s).is_some_and(|t| !t.is_empty()), "{} says nothing", d.name);
            } else {
                assert!(says(s).is_none(), "{} isn't the announcer's", d.name);
            }
        }
    }

    #[test]
    fn kills_are_said_from_where_the_viewer_stands() {
        // Seat 2, of team 0 (seats 0 to 4).
        let mut a = Announcer::new(2);
        a.death(7, Kind::Champion, 1, 2, 10.0);
        let said = a.next(10.0, false).unwrap();
        assert_eq!(
            said,
            Said { line: Sfx::VoiceFirstBlood, killer: Some(2), victim: Some(7), good: Some(true) }
        );
        a.death(8, Kind::Champion, 1, 0, 30.0);
        assert_eq!(drain(&mut a, 30.0), [Sfx::VoiceEnemyFell]);
        a.death(2, Kind::Champion, 0, 6, 31.0);
        assert_eq!(drain(&mut a, 31.0), [Sfx::VoiceYouFell]);
        a.death(1, Kind::Champion, 0, 6, 33.0);
        assert_eq!(drain(&mut a, 33.0), [Sfx::VoiceEnemyMulti2], "seat 6's second kill in ten seconds");
        a.death(3, Kind::Champion, 0, 0, 50.0);
        assert_eq!(drain(&mut a, 50.0), [Sfx::VoiceExecuted], "a seat of our own team is no champion's kill");
        a.death(SEATS + 1, Kind::Turret, 1, 0, 60.0);
        assert_eq!(drain(&mut a, 60.0), [Sfx::VoiceTurretDown]);
        a.inhibitor(1, true, 70.0);
        a.inhibitor(0, false, 70.0);
        assert_eq!(drain(&mut a, 70.0), [Sfx::VoiceEnemyInhibitorSoon, Sfx::VoiceInhibitorBack]);
    }

    #[test]
    fn a_run_is_called_and_its_breaking_and_both_sides_hear_it_their_way() {
        let mut ours = Announcer::new(0);
        let mut theirs = Announcer::new(5);
        // Seat 1 kills three of Dusk's, a minute apart: no multikill, a run on the third.
        for (k, v) in [5, 6, 7].into_iter().enumerate() {
            let t = 100.0 + 60.0 * k as f64;
            for a in [&mut ours, &mut theirs] {
                a.death(v, Kind::Champion, 1, 1, t);
                a.respawn(v);
            }
        }
        let t = 220.0;
        assert_eq!(drain(&mut ours, t), [Sfx::VoiceEnemyFell, Sfx::VoiceSpree3]);
        assert_eq!(drain(&mut theirs, t), [Sfx::VoiceAllyFell, Sfx::VoiceEnemySpree3]);
        // Seat 8 ends it.
        ours.death(1, Kind::Champion, 0, 8, 300.0);
        theirs.death(1, Kind::Champion, 0, 8, 300.0);
        assert_eq!(drain(&mut ours, 300.0), [Sfx::VoiceAllyFell, Sfx::VoiceShutdown]);
        let t = theirs.next(300.0, false).unwrap();
        assert_eq!((t.line, t.good), (Sfx::VoiceEnemyFell, Some(true)));
        assert_eq!(theirs.next(300.0, false).map(|s| s.line), Some(Sfx::VoiceShutdown));
    }

    #[test]
    fn a_whole_team_down_is_an_ace_and_stale_news_goes_unsaid() {
        let mut a = Announcer::new(0);
        for (k, s) in (5..10).enumerate() {
            a.death(s, Kind::Champion, 1, 0, 100.0 + k as f64 * 20.0);
        }
        let said = drain(&mut a, 180.0);
        assert_eq!(said, [Sfx::VoiceYouSlew, Sfx::VoiceAce, Sfx::VoiceSpree5], "{said:?}");
        assert!(!said.contains(&Sfx::VoiceFirstBlood), "first blood was a minute ago: {said:?}");
        // The other side hears its team down.
        let mut b = Announcer::new(9);
        for s in 5..10 {
            b.death(s, Kind::Champion, 1, 0, 100.0);
        }
        assert!(drain(&mut b, 100.0).contains(&Sfx::VoiceAced));
        a.outcome(0, Some(1), 200.0);
        assert_eq!(a.next(200.0, true), None, "one line at a time");
        assert_eq!(
            drain(&mut a, 260.0),
            [Sfx::VoiceEnemySurrender, Sfx::VoiceVictory],
            "the result is never stale"
        );
    }

    #[test]
    fn a_bigger_moment_waiting_goes_first() {
        let mut a = Announcer::new(0);
        a.death(SEATS + 2, Kind::Turret, 1, 0, 10.0);
        a.death(SEATS + 3, Kind::Inhibitor, 1, 0, 10.5);
        assert_eq!(drain(&mut a, 11.0), [Sfx::VoiceInhibitorDown, Sfx::VoiceTurretDown]);
    }
}
