//! A player's Eloram on Elo Pros, for PROFILE: hours played (`GET /api/playtime/{w}`), the
//! ranked rating (`GET /api/ratings/eloram/of/{w}`), and stats, achievements and boards
//! (`GET /api/gamestats/eloram/of/{w}`). Each read says what it found, or that it found nothing,
//! or that it couldn't look: never a zero it didn't measure.

use serde_json::Value;

/// One read's state.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Read<T> {
    #[default]
    Asking,
    /// Nothing on record.
    None,
    /// Couldn't look, and why.
    Unreachable(String),
    Got(T),
}

pub fn playtime_url(w: &str) -> String {
    super::api(&format!("/playtime/{w}"))
}
pub fn rating_url(w: &str) -> String {
    super::api(&format!("/ratings/{}/of/{w}", super::GAME))
}
pub fn stats_url(w: &str) -> String {
    super::api(&format!("/gamestats/{}/of/{w}", super::GAME))
}

/// Seconds played in Eloram, and when last.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hours {
    pub seconds: u64,
    pub last_played: Option<u64>,
}

pub fn hours(bytes: &[u8]) -> Read<Hours> {
    let Ok(v) = serde_json::from_slice::<Value>(bytes) else {
        return Read::Unreachable("the hours didn't read".into());
    };
    let row = v["games"].as_array().into_iter().flatten().find(|g| g["game"] == super::GAME);
    match row {
        Some(g) => Read::Got(Hours {
            seconds: g["seconds"].as_u64().unwrap_or(0),
            last_played: g["last_played"].as_u64(),
        }),
        None => Read::None,
    }
}

impl Hours {
    pub fn line(&self) -> String {
        let h = self.seconds as f64 / 3600.0;
        if h < 1.0 {
            format!("{} minutes in Eloram", self.seconds / 60)
        } else {
            format!("{h:.1} hours in Eloram")
        }
    }
}

/// The ranked ladder's rating.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rating {
    pub rating: f64,
    pub rd: f64,
    pub games: u64,
    pub wins: u64,
    pub placed: bool,
    pub rank: Option<u64>,
}

pub fn rating(bytes: &[u8]) -> Read<Rating> {
    let Ok(v) = serde_json::from_slice::<Value>(bytes) else {
        return Read::Unreachable("the rating didn't read".into());
    };
    let l = &v["ladders"][crate::elopros::profile::LADDER];
    if !l["rated"].as_bool().unwrap_or(false) {
        return Read::None;
    }
    Read::Got(Rating {
        rating: l["rating"].as_f64().unwrap_or(1500.0),
        rd: l["rd"].as_f64().unwrap_or(0.0),
        games: l["games"].as_u64().unwrap_or(0),
        wins: l["wins"].as_u64().unwrap_or(0),
        placed: l["placed"].as_bool().unwrap_or(false),
        rank: l["rank"].as_u64(),
    })
}

/// The ladder ranked matches are rated on (the server's `report::LADDER`).
pub const LADDER: &str = "ranked";

impl Rating {
    pub fn line(&self) -> String {
        let r = format!("{:.0} \u{b1} {:.0}", self.rating, self.rd).replace('\u{b1}', "+/-");
        match (self.placed, self.rank) {
            (true, Some(k)) => format!("Ranked {r}, #{k} on the ladder"),
            (true, None) => format!("Ranked {r}"),
            (false, _) => format!("Ranked {r}, placing ({} played)", self.games),
        }
    }
}

/// What gamestats knows of the player.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    /// `(name, value)` of each stat on record.
    pub stats: Vec<(String, u64)>,
    pub earned: u64,
    pub total: u64,
    /// The achievements earned, newest first: their names.
    pub recent: Vec<String>,
    /// `(board name, score, rank)` of each board with a score.
    pub boards: Vec<(String, u64, Option<u64>, String)>,
}

pub fn stats(bytes: &[u8]) -> Read<Stats> {
    let Ok(v) = serde_json::from_slice::<Value>(bytes) else {
        return Read::Unreachable("the stats didn't read".into());
    };
    if !v["on_record"].as_bool().unwrap_or(false) {
        return Read::None;
    }
    let stats = v["stats"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| Some((s["name"].as_str()?.to_string(), s["value"].as_u64()?)))
        .collect();
    let mut earned: Vec<(u64, String)> = v["achievements"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["earned"].as_bool().unwrap_or(false))
        .filter_map(|a| Some((a["unlocked_at"].as_u64().unwrap_or(0), a["name"].as_str()?.to_string())))
        .collect();
    earned.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    let boards = v["boards"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| {
            Some((
                b["name"].as_str()?.to_string(),
                b["score"].as_u64()?,
                b["rank"].as_u64(),
                b["format"].as_str().unwrap_or("number").to_string(),
            ))
        })
        .collect();
    Read::Got(Stats {
        stats,
        earned: v["totals"]["earned"].as_u64().unwrap_or(0),
        total: v["totals"]["total"].as_u64().unwrap_or(0),
        recent: earned.into_iter().map(|(_, n)| n).take(4).collect(),
        boards,
    })
}

/// A board's score as its format says (`time_ms` → `m:ss`).
pub fn score_text(score: u64, format: &str) -> String {
    if format == "time_ms" {
        let s = score / 1000;
        format!("{}:{:02}", s / 60, s % 60)
    } else {
        score.to_string()
    }
}

/// A read's line when it isn't `Got`.
pub fn state_line<T>(r: &Read<T>, what: &str) -> Option<String> {
    match r {
        Read::Asking => Some(format!("Asking Elo Pros for {what}...")),
        Read::None => Some(format!("No {what} on record yet.")),
        Read::Unreachable(why) => Some(format!("Couldn't read {what}: {why}")),
        Read::Got(_) => None,
    }
}
