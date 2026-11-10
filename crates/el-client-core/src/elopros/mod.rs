//! The Elo Pros platform as the menu reads it, with no I/O: the friends list and the signed text
//! that changes it (`friends`), a player's face (`face`), their hours, rating and stats in Eloram
//! (`profile`), and a bug report (`bugs`). Each is tested against the platform's own vectors
//! (`tests/testdata/`, copied from `AnthonE/scry-forge`'s `sdk/testdata/`; `scripts/elo-drift.sh`
//! checks the copies). After Gates' (`AnthonE/Gates`, MIT, `crates/client/src/friends.rs`,
//! `profile.rs`, `report.rs`), for wallets of either chain.

pub mod bugs;
pub mod face;
pub mod friends;
pub mod profile;

/// The platform.
pub const ORIGIN: &str = "https://elopros.com";
/// The title's slug there.
pub const GAME: &str = "eloram";

/// `https://elopros.com/api<path>`.
pub fn api(path: &str) -> String {
    format!("{ORIGIN}/api{path}")
}

/// A player id as the platform writes it (`0x...` lowercase, base58 as written), if it's a wallet
/// (an `acct_` login has no key to sign with).
pub fn wallet(id: &str) -> Option<String> {
    let p = el_proto::auth::PlayerId::parse(id)?;
    let s = p.to_string();
    (!s.starts_with("acct_")).then_some(s)
}

/// A field the platform may send as `null` reads as its default.
pub(crate) fn or_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de> + Default,
{
    use serde::Deserialize as _;
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// `3h ago`, floored.
pub fn ago(secs: u64) -> String {
    match secs {
        s if s < 60 => "just now".into(),
        s if s < 3_600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3_600),
        s => format!("{}d ago", s / 86_400),
    }
}

/// A refusal in words: the body's `error`/`why` if it says one.
pub fn error_of(status: u16, body: &[u8]) -> String {
    let said = serde_json::from_slice::<serde_json::Value>(body).ok().and_then(|v| {
        ["error", "why", "detail"].iter().find_map(|k| v.get(*k).and_then(|x| x.as_str()).map(str::to_owned))
    });
    match said {
        Some(w) => format!("Elo Pros said no: {w}"),
        None => format!("Elo Pros answered {status}"),
    }
}
