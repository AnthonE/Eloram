//! Friends from Elo Pros: the list a wallet keeps there (`GET /api/friends/{id}`), who on it is
//! playing and where, and the signed text that asks, accepts, declines, cancels or removes one.
//! Every change is prepare → check → sign → act, and the text is rebuilt here and compared with
//! what the platform offered, never signed on trust.

use serde::Deserialize;

use super::{ago, or_default, wallet};

/// Largest list body read.
pub const MAX_VIEW_BYTES: usize = 128 * 1024;
/// How far the platform's `at` may sit from this clock and still be signed.
pub const AT_SKEW_S: u64 = 600;
/// A name drawn in a row is cut here.
pub const MAX_NAME_CHARS: usize = 32;

/// One change to the list, and the key its one argument goes under (`meter/friends.py` `OPS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Ask,
    Accept,
    Decline,
    Cancel,
    Remove,
    Block,
    Unblock,
    Invite,
}

impl Op {
    pub const ALL: [Op; 8] =
        [Op::Ask, Op::Accept, Op::Decline, Op::Cancel, Op::Remove, Op::Block, Op::Unblock, Op::Invite];

    pub fn name(self) -> &'static str {
        match self {
            Op::Ask => "ask",
            Op::Accept => "accept",
            Op::Decline => "decline",
            Op::Cancel => "cancel",
            Op::Remove => "remove",
            Op::Block => "block",
            Op::Unblock => "unblock",
            Op::Invite => "invite",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Op::Ask | Op::Cancel | Op::Invite => "to",
            Op::Accept | Op::Decline => "from",
            Op::Remove | Op::Block | Op::Unblock => "wallet",
        }
    }

    pub fn parse(s: &str) -> Option<Op> {
        Op::ALL.into_iter().find(|o| o.name() == s)
    }

    /// The status line once the act landed.
    pub fn done(self) -> &'static str {
        match self {
            Op::Ask => "Asked. They'll see it on their profile.",
            Op::Accept => "You're friends now.",
            Op::Decline => "Declined.",
            Op::Cancel => "Ask withdrawn.",
            Op::Remove => "Removed.",
            Op::Block => "Blocked.",
            Op::Unblock => "Unblocked.",
            Op::Invite => "Invited.",
        }
    }
}

/// `GET /api/friends/{id}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct View {
    #[serde(default, deserialize_with = "or_default")]
    pub wallet: String,
    #[serde(default, deserialize_with = "or_default")]
    pub friends: Vec<Friend>,
    #[serde(default, deserialize_with = "or_default")]
    pub buddies: Vec<Friend>,
    #[serde(default, deserialize_with = "or_default")]
    pub asks_in: Vec<Ask>,
    #[serde(default, deserialize_with = "or_default")]
    pub asks_out: Vec<Ask>,
    #[serde(default, deserialize_with = "or_default")]
    pub invites_in: Vec<Friend>,
    #[serde(default)]
    pub presence: Option<Presence>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Friend {
    #[serde(default, deserialize_with = "or_default")]
    pub wallet: String,
    #[serde(default)]
    pub since: Option<u64>,
    #[serde(default)]
    pub name: Option<String>,
    /// `/api/face/{wallet}/pic.png?v=<rev>`, or none.
    #[serde(default)]
    pub pic: Option<String>,
    #[serde(default, deserialize_with = "or_default")]
    pub agent: bool,
    #[serde(default, deserialize_with = "or_default")]
    pub online: bool,
    #[serde(default)]
    pub playing: Option<Playing>,
    #[serde(default)]
    pub last_online: Option<u64>,
    #[serde(default)]
    pub buddy: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Playing {
    #[serde(default)]
    pub game: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub server_name: Option<String>,
    #[serde(default)]
    pub since: Option<u64>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub join: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Ask {
    #[serde(default, deserialize_with = "or_default")]
    pub wallet: String,
    #[serde(default)]
    pub at: Option<u64>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Presence {
    #[serde(default)]
    pub reachable: Option<bool>,
    #[serde(default)]
    pub why: Option<String>,
}

fn tidy_name(n: &mut Option<String>) {
    *n = n
        .take()
        .map(|s| s.trim().chars().take(MAX_NAME_CHARS).collect::<String>())
        .filter(|s| !s.is_empty() && !s.chars().any(char::is_control));
}

/// A list read: rows with no readable wallet dropped, every id as the platform writes it.
pub fn parse(bytes: &[u8]) -> Result<View, String> {
    if bytes.len() > MAX_VIEW_BYTES {
        return Err("the friends list was too long".into());
    }
    let v: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "the friends list didn't read".to_string())?;
    view_of(v)
}

/// [`parse`] for a body already read (an act's `view`).
pub fn view_of(v: serde_json::Value) -> Result<View, String> {
    if !v.is_object() {
        return Err("the friends list didn't read".into());
    }
    let mut view: View = serde_json::from_value(v).map_err(|_| "the friends list didn't read".to_string())?;
    let fix = |w: &mut String| match wallet(w) {
        Some(n) => {
            *w = n;
            true
        }
        None => false,
    };
    for rows in [&mut view.friends, &mut view.buddies, &mut view.invites_in] {
        rows.retain_mut(|f| fix(&mut f.wallet));
        for f in rows.iter_mut() {
            tidy_name(&mut f.name);
        }
    }
    for rows in [&mut view.asks_in, &mut view.asks_out] {
        rows.retain_mut(|a| fix(&mut a.wallet));
        for a in rows.iter_mut() {
            tidy_name(&mut a.name);
        }
    }
    Ok(view)
}

impl View {
    /// Every friend, then every buddy that isn't also one.
    pub fn everyone(&self) -> impl Iterator<Item = &Friend> + '_ {
        self.friends
            .iter()
            .chain(self.buddies.iter().filter(|b| !self.friends.iter().any(|f| f.wallet == b.wallet)))
    }

    /// The platform couldn't see who's playing, so nobody reads online.
    pub fn blind(&self) -> bool {
        self.presence.as_ref().is_some_and(|p| p.reachable == Some(false))
    }

    /// The rows in the order the page draws them: online first (in Eloram first of those), then
    /// the rest by when they were last seen.
    pub fn ordered(&self) -> Vec<&Friend> {
        let mut rows: Vec<&Friend> = self.everyone().collect();
        rows.sort_by_key(|f| {
            (!f.online, f.on_eloram().is_none(), std::cmp::Reverse(f.last_online.unwrap_or(0)))
        });
        rows
    }
}

/// `0x1234...abcd`.
pub fn short(w: &str) -> String {
    crate::front::who::short(w)
}

impl Friend {
    /// The name, else the short wallet; an agent marked.
    pub fn label(&self) -> String {
        let name = self.name.clone().unwrap_or_else(|| short(&self.wallet));
        if self.agent { format!("{name} (agent)") } else { name }
    }

    /// Where they're playing Eloram, if they are.
    pub fn on_eloram(&self) -> Option<&Playing> {
        self.playing.as_ref().filter(|p| self.online && p.game.as_deref() == Some(super::GAME))
    }
}

/// The line under a friend's name: `In Eloram: In a match`, `Playing Gates`, `online`, `last
/// seen 3h ago`, `offline`.
pub fn presence_line(f: &Friend, now: u64) -> String {
    if f.online {
        if let Some(p) = &f.playing {
            let title = p.title.as_deref().or(p.game.as_deref()).unwrap_or("a game");
            return match (f.on_eloram().is_some(), p.status.as_deref()) {
                (true, Some(s)) => format!("In {title}: {s}"),
                _ => format!("Playing {title}"),
            };
        }
        return "online".into();
    }
    match f.last_online {
        Some(t) => format!("last seen {}", ago(now.saturating_sub(t))),
        None => "offline".into(),
    }
}

/// UTC `YYYY-MM-DD` of unix second `at` (civil-from-days).
pub fn utc_day(at: u64) -> String {
    let z = (at / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// `{"args":{"<key>":"<other>"},"at":<at>,"op":"<op>"}`, keys sorted.
pub fn detail(op: Op, other: &str, at: u64) -> String {
    format!("{{\"args\":{{\"{}\":\"{other}\"}},\"at\":{at},\"op\":\"{}\"}}", op.key(), op.name())
}

/// The exact text a wallet signs for one change.
pub fn sign_text(me: &str, op: Op, other: &str, at: u64) -> Result<String, String> {
    let me = wallet(me).ok_or("Sign in with a wallet first.")?;
    let other = wallet(other).ok_or("That isn't a wallet.")?;
    Ok(format!(
        "elo play\naction: friends\nwallet: {me}\nday: {}\ndetail: {}",
        utc_day(at),
        detail(op, &other, at)
    ))
}

/// `POST /api/friends/prepare`'s body.
pub fn prepare_body(me: &str, op: Op, other: &str) -> Vec<u8> {
    serde_json::json!({"wallet": me, "op": op.name(), "args": { op.key(): other }}).to_string().into_bytes()
}

/// `POST /api/friends/act`'s body.
pub fn act_body(me: &str, op: Op, other: &str, at: u64, signature: &str) -> Vec<u8> {
    serde_json::json!({"wallet": me, "op": op.name(), "args": { op.key(): other }, "at": at, "signature": signature})
        .to_string()
        .into_bytes()
}

/// What prepare offered to have signed.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Offered {
    #[serde(default, deserialize_with = "or_default")]
    pub sign_this: String,
    #[serde(default, deserialize_with = "or_default")]
    pub at: u64,
    #[serde(default, deserialize_with = "or_default")]
    pub op: String,
    #[serde(default, deserialize_with = "or_default")]
    pub args: serde_json::Map<String, serde_json::Value>,
}

/// The text to sign, if prepare offered exactly the change asked for.
pub fn check_offered(
    me: &str,
    op: Op,
    other: &str,
    now: u64,
    offered_body: &[u8],
) -> Result<(String, u64), String> {
    let nothing = "nothing was signed";
    let offered: Offered = serde_json::from_slice(offered_body)
        .map_err(|_| format!("Elo Pros' answer didn't read; {nothing}."))?;
    if offered.op != op.name() {
        return Err(format!("Elo Pros offered another change; {nothing}."));
    }
    let arg = offered.args.get(op.key()).and_then(|v| v.as_str()).and_then(wallet);
    if offered.args.len() != 1 || arg != wallet(other) {
        return Err(format!("Elo Pros offered a change to someone else; {nothing}."));
    }
    if offered.at.abs_diff(now) > AT_SKEW_S {
        return Err(format!("This clock and Elo Pros' are more than ten minutes apart; {nothing}."));
    }
    let text = sign_text(me, op, other, offered.at)?;
    if offered.sign_this != text {
        return Err(format!("Elo Pros asked to sign other words than this change; {nothing}."));
    }
    Ok((text, offered.at))
}

/// The list an act answered with.
pub fn parse_act(bytes: &[u8]) -> Result<View, String> {
    let v: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "Elo Pros' answer didn't read".to_string())?;
    match v.get("view") {
        Some(view) => view_of(view.clone()),
        None => Err("Elo Pros' answer had no list".into()),
    }
}
