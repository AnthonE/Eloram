//! The lobby's messages: JSON on control-stream frames tagged [`el_proto::control::LOBBY`]. The
//! server, the browser and the agents all read and write them through these types.

use serde::{Deserialize, Serialize};

/// Client → server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ToServer {
    /// Find me a match.
    Queue,
    /// Not any more.
    Leave,
}

/// Who sits in a seat, as the scoreboard shows them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    /// An agent playing through the protocol, as people do.
    pub agent: bool,
    /// One of the server's own bots, filling a seat no one took.
    pub bot: bool,
}

/// Server → client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ToClient {
    /// In the queue, with this many others waiting; the match starts within `fill_in` seconds
    /// (bots take the seats no one did).
    Queued { waiting: usize, fill_in: u32 },
    /// Seated: the match's seed (a decimal string: a page's numbers stop at 2^53), every seat's
    /// champion and who plays it. Snapshots follow as datagrams.
    Match { seat: u8, seed: String, picks: Vec<u8>, roster: Vec<Player> },
    /// The match is over; `winner` is the team whose nexus stands.
    Over { winner: u8 },
}

impl ToClient {
    /// The match's seed, if this seats the client.
    pub fn seed(&self) -> Option<u64> {
        match self {
            ToClient::Match { seed, .. } => seed.parse().ok(),
            _ => None,
        }
    }

    /// Every seat's champion, if this seats the client (and there are ten of them).
    pub fn picks(&self) -> Option<[u8; el_sim::units::SEATS]> {
        match self {
            ToClient::Match { picks, .. } => picks.as_slice().try_into().ok(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_read_as_the_page_writes_them() {
        assert_eq!(serde_json::from_str::<ToServer>(r#"{"t":"queue"}"#).unwrap(), ToServer::Queue);
        let m = ToClient::Match {
            seat: 3,
            seed: u64::MAX.to_string(),
            picks: (0..10).collect(),
            roster: vec![Player { name: "Ash".into(), agent: false, bot: false }],
        };
        let text = serde_json::to_string(&m).unwrap();
        assert!(text.starts_with(r#"{"t":"match","seat":3,"seed":"18446744073709551615""#), "{text}");
        let back: ToClient = serde_json::from_str(&text).unwrap();
        assert_eq!(back.seed(), Some(u64::MAX));
        assert_eq!(back.picks().map(|p| p[9]), Some(9));
    }
}
