//! The lobby's messages: JSON on control-stream frames tagged [`el_proto::control::LOBBY`]. The
//! server, the browser and the agents all read and write them through these types.

use serde::{Deserialize, Serialize};

/// Client → server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ToServer {
    /// Find me a match (NORMAL: bots fill the seats no one takes, nothing rated).
    Queue,
    /// Find me a ranked match: ten wallets, no bots, every match rated on Elo Pros. Answered with
    /// [`ToClient::Ranked`] while it waits, or [`ToClient::Refused`].
    QueueRanked,
    /// Not any more: out of the queue or champion select, or (seated) out of the match for good:
    /// its rejoin token forgotten, a bot in the seat.
    Leave,
    /// In champion select: this champion back to the team's bench, a new one dealt.
    Reroll,
    /// In champion select: `champ` off the team's bench, mine onto it.
    Swap {
        champ: u8,
    },
    /// In champion select: done choosing (the match starts once everyone seated is).
    Ready,
    /// Back into the seat a `Match` gave this `token` for, after the connection dropped (or the
    /// page reloaded). Answered with that `Match` again, or [`ToClient::NoSeat`].
    Rejoin {
        token: String,
    },
    /// Asks `player` (a signed-in player's id, on this server) into my party (a new one, led by
    /// me, if I'm in none).
    PartyInvite {
        player: String,
    },
    /// Into the party `leader` (a session) asked me into.
    PartyAccept {
        leader: u32,
    },
    PartyDecline {
        leader: u32,
    },
    /// Out of my party (its lead passes to the next member).
    PartyLeave,
    /// A practice match as champion `champ` (`el_sim::practice`), on a server that holds them
    /// (`el-server --practice`): at once, no champion select; asked again, a new one as another.
    /// Answered with its `Match`, or [`ToClient::NoSeat`].
    Practice {
        champ: u8,
    },
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

/// A party's member, as the party card shows them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyMember {
    pub session: u32,
    pub name: String,
    /// Their id, signed in (a guest's: none, and a guest can't queue ranked).
    #[serde(default)]
    pub player: Option<String>,
}

/// An ask into a party.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyAsk {
    pub leader: u32,
    pub name: String,
}

/// The most a party holds: a side.
pub const PARTY_MAX: usize = 5;

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
    Select {
        seat: u8,
        picks: Vec<u8>,
        bench: Vec<u8>,
        rerolls: u8,
        ends_in: u32,
        roster: Vec<Player>,
        /// A ranked match's select (someone leaving it sends everyone else back to the queue).
        #[serde(default)]
        ranked: bool,
    },
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
        /// The stream this seating's snapshots come on (`Client::set_stream`): the client takes
        /// no other, so nothing of a match it was in before is taken for this one. 0: not told.
        #[serde(default)]
        stream: u8,
        /// Rated on Elo Pros.
        #[serde(default)]
        ranked: bool,
    },
    /// A rejoin found no seat: the match is over, or the token isn't one.
    NoSeat,
    /// The match is over; `winner` is the team whose nexus stands.
    Over { winner: u8 },
    /// Who the server knows this session as (its wallet or account id, none for a guest), and
    /// what it holds. Sent once, after the Welcome (and the Token).
    About {
        #[serde(default)]
        player: Option<String>,
        #[serde(default)]
        practice: bool,
        #[serde(default)]
        ranked: bool,
    },
    /// The ranked queue, as the store has it: `state` one of `waiting`, `matched`, `gathering`,
    /// `expired`, `failed`; the seconds waited; why, when it ended.
    Ranked {
        state: String,
        #[serde(default)]
        waited_s: u32,
        #[serde(default)]
        why: Option<String>,
    },
    /// Something asked for that this server won't do (`what`: `ranked`, `practice`...), and why,
    /// in words to show.
    Refused { what: String, why: String },
    /// My party (`leader` 0 and no members: none) and the asks into others', whenever either
    /// changes. Only the leader queues; everyone in it goes into the same match, on one side.
    Party { leader: u32, members: Vec<PartyMember>, invites_in: Vec<PartyAsk> },
}

/// The ranked queue's state in words, for the menu.
pub fn ranked_line(state: &str, waited_s: u32, why: Option<&str>) -> String {
    let waited = if waited_s >= 60 {
        format!("{}:{:02}", waited_s / 60, waited_s % 60)
    } else {
        format!("{waited_s}s")
    };
    match (state, why) {
        ("waiting", _) => format!("In the ranked queue, {waited}"),
        ("matched" | "gathering", _) => "Matched: gathering the ten players".into(),
        ("expired", _) => "The ranked queue gave up after its wait. Queue again?".into(),
        (_, Some(w)) => format!("Ranked: {w}"),
        _ => format!("Ranked: {state}"),
    }
}

/// The ranked wait is over without a match.
pub fn ranked_ended(state: &str) -> bool {
    matches!(state, "expired" | "failed" | "off")
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
            stream: 12,
            ranked: false,
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
            (r#"{"t":"queue_ranked"}"#, ToServer::QueueRanked),
            (r#"{"t":"party_invite","player":"0xab"}"#, ToServer::PartyInvite { player: "0xab".into() }),
            (r#"{"t":"party_accept","leader":4}"#, ToServer::PartyAccept { leader: 4 }),
            (r#"{"t":"party_leave"}"#, ToServer::PartyLeave),
        ] {
            assert_eq!(serde_json::from_str::<ToServer>(json).unwrap(), msg);
        }
        for (json, msg) in [
            (
                r#"{"t":"about","player":"0xab","practice":true,"ranked":false}"#,
                ToClient::About { player: Some("0xab".into()), practice: true, ranked: false },
            ),
            (r#"{"t":"about"}"#, ToClient::About { player: None, practice: false, ranked: false }),
            (
                r#"{"t":"ranked","state":"waiting","waited_s":75}"#,
                ToClient::Ranked { state: "waiting".into(), waited_s: 75, why: None },
            ),
            (
                r#"{"t":"refused","what":"ranked","why":"no"}"#,
                ToClient::Refused { what: "ranked".into(), why: "no".into() },
            ),
            (
                r#"{"t":"party","leader":3,"members":[{"session":3,"name":"A","player":"0xab"},{"session":5,"name":"B"}],"invites_in":[]}"#,
                ToClient::Party {
                    leader: 3,
                    members: vec![
                        PartyMember { session: 3, name: "A".into(), player: Some("0xab".into()) },
                        PartyMember { session: 5, name: "B".into(), player: None },
                    ],
                    invites_in: vec![],
                },
            ),
        ] {
            assert_eq!(serde_json::from_str::<ToClient>(json).unwrap(), msg);
        }
    }

    #[test]
    fn the_ranked_queue_in_words() {
        assert_eq!(ranked_line("waiting", 75, None), "In the ranked queue, 1:15");
        assert_eq!(ranked_line("waiting", 9, None), "In the ranked queue, 9s");
        assert_eq!(ranked_line("failed", 0, Some("no server with room")), "Ranked: no server with room");
        assert!(ranked_ended("expired") && !ranked_ended("waiting"));
        for s in ["waiting", "matched", "expired", "failed", "odd"] {
            assert!(ranked_line(s, 3, None).is_ascii());
        }
    }
}
