//! What a finished match tells Elo Pros, worked out with no I/O: its result for the ratings (a
//! ranked match's alone: `POST /ratings/eloram/report`) and each signed-in player's stats,
//! unlocks and board scores (`POST /gamestats/eloram/report`), against the definitions in the
//! repository's `elo.json`.

use el_sim::units::SEATS;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The ladder ranked matches are rated on.
pub const LADDER: &str = "ranked";

/// A seat at a match's end.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SeatDone {
    /// The player's id (`0x...`, base58, or `acct_...`), signed in; none for a guest or a bot.
    pub player: Option<String>,
    pub agent: bool,
    pub kills: u16,
    pub deaths: u16,
    pub assists: u16,
    pub last_hits: u16,
    pub level: u8,
}

/// A match at its end, as the outbox keeps it until it's sent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MatchDone {
    /// The record's id (`{id}.elr`).
    pub record: String,
    /// The store's match id, for a ranked match.
    pub ranked: Option<String>,
    /// 0 or 1; anything else is no contest.
    pub winner: u8,
    pub practice: bool,
    /// The tick it ended on.
    pub tick: u32,
    pub seats: Vec<SeatDone>,
}

impl MatchDone {
    fn decided(&self) -> bool {
        self.winner < 2 && !self.practice && self.seats.len() == SEATS
    }

    fn won(&self, seat: usize) -> bool {
        usize::from(self.winner) == el_sim::units::team_of_seat(seat) as usize
    }
}

/// The ratings report for a ranked match (none for any other): its two sides, who won, the
/// agents among them, and the record it can be checked against.
pub fn ratings(done: &MatchDone, replay_sha256: Option<&str>, replay_url: Option<&str>) -> Option<Value> {
    let id = done.ranked.as_deref()?;
    if !done.decided() {
        return None;
    }
    let side = |team: usize| -> Vec<String> {
        (0..SEATS)
            .filter(|&s| el_sim::units::team_of_seat(s) as usize == team)
            .filter_map(|s| done.seats[s].player.clone())
            .collect()
    };
    let teams: Vec<Value> = (0..2)
        .map(|t| json!({"players": side(t), "place": if usize::from(done.winner) == t { 1 } else { 2 }}))
        .collect();
    if teams.iter().any(|t| t["players"].as_array().is_none_or(Vec::is_empty)) {
        return None;
    }
    let agents: Vec<&str> =
        done.seats.iter().filter(|s| s.agent).filter_map(|s| s.player.as_deref()).collect();
    let mut evidence = serde_json::Map::new();
    if let Some(h) = replay_sha256 {
        evidence.insert("replay_sha256".into(), h.into());
    }
    if let Some(u) = replay_url.filter(|u| u.starts_with("https://")) {
        evidence.insert("replay_url".into(), u.into());
    }
    let mut m = json!({"id": id, "ladder": LADDER, "teams": teams, "agents": agents});
    if !evidence.is_empty() {
        m["evidence"] = Value::Object(evidence);
    }
    Some(json!({"matches": [m]}))
}

/// The stats report: a line for each signed-in player of a decided match that wasn't practice.
pub fn gamestats(done: &MatchDone) -> Option<Value> {
    if !done.decided() {
        return None;
    }
    let ranked = done.ranked.is_some();
    let reports: Vec<Value> = (0..SEATS)
        .filter_map(|s| {
            let seat = &done.seats[s];
            let wallet = seat.player.as_deref()?;
            let won = done.won(s);
            let mut unlock = vec!["FIRST_MATCH"];
            if won {
                unlock.push("FIRST_WIN");
                if seat.deaths == 0 {
                    unlock.push("UNTOUCHED");
                }
                if ranked {
                    unlock.push("RANKED_WIN");
                }
            }
            if seat.kills >= 10 {
                unlock.push("TEN_TAKEDOWNS");
            }
            let mut scores = json!({"most_kills": seat.kills});
            if won {
                scores["fastest_win"] = json!(u64::from(done.tick) * 1000 / u64::from(el_proto::TICK_HZ));
            }
            Some(json!({
                "wallet": wallet,
                "agent": seat.agent,
                "stats": {
                    "matches": 1,
                    "wins": u8::from(won),
                    "kills": seat.kills,
                    "deaths": seat.deaths,
                    "assists": seat.assists,
                    "last_hits": seat.last_hits,
                    "ranked_matches": u8::from(ranked),
                    "ranked_wins": u8::from(ranked && won),
                },
                "unlock": unlock,
                "scores": scores,
            }))
        })
        .collect();
    (!reports.is_empty()).then(|| json!({"reports": reports}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done(ranked: bool) -> MatchDone {
        let seats = (0..SEATS)
            .map(|s| SeatDone {
                player: Some(format!("0x{:040x}", s + 1)),
                agent: s == 9,
                kills: s as u16,
                deaths: u16::from(s != 0),
                ..SeatDone::default()
            })
            .collect();
        MatchDone {
            record: "00000000000000ab".into(),
            ranked: ranked.then(|| "q-abc".into()),
            winner: 0,
            practice: false,
            tick: 30 * 60 * 10,
            seats,
        }
    }

    /// The elo.json this repository lands: every id a report names is declared there, or the
    /// store refuses the whole call.
    fn declared() -> Value {
        serde_json::from_str(include_str!("../../../../elo.json")).unwrap()
    }

    #[test]
    fn a_ranked_match_is_rated_with_its_record() {
        let d = done(true);
        let r = ratings(
            &d,
            Some(&"a".repeat(64)),
            Some("https://game.elopros.com/eloram/records/00000000000000ab"),
        )
        .unwrap();
        let m = &r["matches"][0];
        assert_eq!(m["id"], "q-abc");
        assert_eq!(m["ladder"], LADDER);
        assert_eq!(m["teams"][0]["place"], 1);
        assert_eq!(m["teams"][1]["place"], 2);
        assert_eq!(m["teams"][0]["players"].as_array().unwrap().len(), 5);
        assert_eq!(m["agents"], json!([format!("0x{:040x}", 10)]));
        assert_eq!(m["evidence"]["replay_sha256"].as_str().unwrap().len(), 64);
        assert!(ratings(&done(false), None, None).is_none(), "an unrated match isn't rated");
        let mut nc = done(true);
        nc.winner = 2;
        assert!(ratings(&nc, None, None).is_none(), "no contest");
    }

    #[test]
    fn stats_name_only_what_elo_json_declares() {
        let defs = declared();
        let ids = |k: &str| -> Vec<String> {
            defs["stats"][k]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["id"].as_str().unwrap().to_string())
                .collect()
        };
        let (stats, achs, boards) = (ids("stats"), ids("achievements"), ids("boards"));
        let mut guest = done(true);
        guest.seats[3].player = None;
        let r = gamestats(&guest).unwrap();
        let reps = r["reports"].as_array().unwrap();
        assert_eq!(reps.len(), 9, "a guest has no line");
        for rep in reps {
            for k in rep["stats"].as_object().unwrap().keys() {
                assert!(stats.contains(k), "stat {k}");
            }
            for a in rep["unlock"].as_array().unwrap() {
                assert!(achs.contains(&a.as_str().unwrap().to_string()), "achievement {a}");
            }
            for k in rep["scores"].as_object().unwrap().keys() {
                assert!(boards.contains(k), "board {k}");
            }
        }
        assert_eq!(reps[0]["unlock"], json!(["FIRST_MATCH", "FIRST_WIN", "UNTOUCHED", "RANKED_WIN"]));
        assert_eq!(reps[0]["scores"]["fastest_win"], 600_000);
        let mut practice = done(false);
        practice.practice = true;
        assert!(gamestats(&practice).is_none());
    }
}
