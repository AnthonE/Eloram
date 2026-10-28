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
    /// In champion select: this champion back to the team's bench, a new one dealt.
    Reroll,
    /// In champion select: `champ` off the team's bench, mine onto it.
    Swap { champ: u8 },
    /// In champion select: done choosing (the match starts once everyone seated is).
    Ready,
    /// Back into the seat a `Match` gave this `token` for, after the connection dropped (or the
    /// page reloaded). Answered with that `Match` again, or [`ToClient::NoSeat`].
    Rejoin { token: String },
    /// A practice match as champion `champ` (`el_sim::practice`), on a server that holds them
    /// (`el-server --practice`): at once, no champion select; asked again, a new one as another.
    /// Answered with its `Match`, or [`ToClient::NoSeat`].
    Practice { champ: u8 },
}

/// A seat in champion select with no champion shown: the other team's, unseen until the match.
pub const UNSEEN: u8 = u8::MAX;

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
    /// Champion select (ARAM's): your seat; every seat's champion as your team may see it (the
    /// other team's [`UNSEEN`]); your team's bench; your rerolls left; the seconds left; and who
    /// sits where. Sent again whenever any of it changes.
    Select { seat: u8, picks: Vec<u8>, bench: Vec<u8>, rerolls: u8, ends_in: u32, roster: Vec<Player> },
    /// Seated: every seat's champion, the augment screens' tiers and the headliner (all a client
    /// needs of the match's seed, which stays on the server: every seat's augment offers follow
    /// from it), and who plays where. Snapshots follow as datagrams. `rejoin` takes the seat back
    /// on a new connection while the match lasts ([`ToServer::Rejoin`]); it's the player's alone.
    Match {
        seat: u8,
        picks: Vec<u8>,
        tiers: [u8; 4],
        headliner: u8,
        roster: Vec<Player>,
        #[serde(default)]
        rejoin: String,
        /// A practice match (`el_sim::practice`): the client makes its copy one too.
        #[serde(default)]
        practice: bool,
    },
    /// A rejoin found no seat: the match is over, or the token isn't one.
    NoSeat,
    /// The match is over; `winner` is the team whose nexus stands.
    Over { winner: u8 },
}

impl ToClient {
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
            picks: (0..10).collect(),
            tiers: [0, 1, 2, 1],
            headliner: 4,
            roster: vec![Player { name: "Ash".into(), agent: false, bot: false }],
            rejoin: "00ff".into(),
            practice: false,
        };
        let text = serde_json::to_string(&m).unwrap();
        assert!(text.starts_with(r#"{"t":"match","seat":3,"picks":[0,1,2"#), "{text}");
        assert!(!text.contains("seed"), "the seed stays on the server: {text}");
        let back: ToClient = serde_json::from_str(&text).unwrap();
        assert_eq!(back, m);
        assert_eq!(back.picks().map(|p| p[9]), Some(9));
        for (json, msg) in [
            (r#"{"t":"reroll"}"#, ToServer::Reroll),
            (r#"{"t":"swap","champ":7}"#, ToServer::Swap { champ: 7 }),
            (r#"{"t":"ready"}"#, ToServer::Ready),
            (r#"{"t":"rejoin","token":"ab"}"#, ToServer::Rejoin { token: "ab".into() }),
            (r#"{"t":"practice","champ":12}"#, ToServer::Practice { champ: 12 }),
        ] {
            assert_eq!(serde_json::from_str::<ToServer>(json).unwrap(), msg);
        }
    }
}
