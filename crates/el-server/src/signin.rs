//! Sign-in on the server: the domains a challenge may name, the nonces an Elo Pros pass must carry,
//! the resume tokens that bring a signed-in player back without asking again, and who is on
//! (`/online.json`, for the Elo Pros store). The checks themselves are `el_auth`'s. Nothing here is
//! near a shard thread.
//!
//! Never logged: signatures and resume tokens. An address is logged only shortened ([`short`]).

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use el_proto::auth::{NONCE_BYTES, PlayerId, TOKEN_BYTES};

/// Seconds since the epoch.
pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// `0x1234…cdef`: an id as a log may name it.
pub fn short(a: &PlayerId) -> String {
    let s = a.as_str();
    format!("{}…{}", &s[..6], &s[s.len() - 4..])
}

/// A domain as a pass names it: the host, with no port.
pub fn host_of(domain: &str) -> &str {
    domain.rsplit_once(':').map_or(domain, |(h, _)| h)
}

/// Elo Pros' account key from its standard base64 (`GET /api/account/pubkey`).
pub fn parse_pubkey(text: &str) -> Option<[u8; 32]> {
    base64::engine::general_purpose::STANDARD.decode(text.trim()).ok()?.try_into().ok()
}

/// The host (and port, unless it's the scheme's default) of an `Origin`, as SIWE names a domain:
/// `https://elopros.com` is `elopros.com`, `http://127.0.0.1:8080` stays `127.0.0.1:8080`.
pub fn origin_host(origin: &str) -> &str {
    let rest = origin.split_once("://").map_or(origin, |(_, r)| r);
    let rest = rest.split('/').next().unwrap_or(rest);
    rest.strip_suffix(":443").filter(|_| origin.starts_with("https://")).unwrap_or(rest)
}

/// Which of `domains` a session's challenge names: the one its page came from (`origin`), or the
/// first (agents, and pages from anywhere else, whose wallets will warn).
pub fn pick_domain<'a>(domains: &'a [String], origin: Option<&str>) -> &'a str {
    let host = origin.map(origin_host);
    host.and_then(|h| domains.iter().find(|d| d.eq_ignore_ascii_case(h)))
        .or_else(|| domains.first())
        .map_or("", String::as_str)
}

/// Fresh random bytes.
pub fn random<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).expect("the system's randomness");
    out
}

/// Resume tokens: each names a wallet, for a while. A token is kept only as its keccak-256, and
/// with a file (`--records`' `tokens.json`) the hashes outlive a restart, so a signed-in player's
/// page resumes on the next server as on this one.
#[derive(Default)]
pub struct Tokens {
    /// Each token's hash: its wallet, and until when (seconds since the epoch).
    held: Mutex<HashMap<[u8; 32], (PlayerId, u64)>>,
    file: Option<PathBuf>,
}

impl Tokens {
    /// Tokens kept in `file`, read back from it if it's there.
    pub fn kept(file: PathBuf) -> Self {
        let mut held = HashMap::new();
        if let Ok(b) = std::fs::read(&file)
            && let Ok(rows) = serde_json::from_slice::<Vec<(String, String, u64)>>(&b)
        {
            let now = unix_now();
            for (hash, wallet, until) in rows {
                let (Ok(h), Some(a)) = (hex::decode(&hash), PlayerId::parse(&wallet)) else {
                    continue;
                };
                if let Ok(h) = <[u8; 32]>::try_from(h.as_slice())
                    && until > now
                {
                    held.insert(h, (a, until));
                }
            }
        }
        Self { held: Mutex::new(held), file: Some(file) }
    }

    /// A new token for `address`, good for `ttl`.
    pub fn issue(&self, address: PlayerId, ttl: Duration) -> [u8; TOKEN_BYTES] {
        let token = random::<TOKEN_BYTES>();
        if let Ok(mut t) = self.held.lock() {
            let now = unix_now();
            t.retain(|_, (_, until)| *until > now);
            t.insert(el_auth::keccak256(&token), (address, now + ttl.as_secs()));
            if let Some(file) = &self.file {
                let rows: Vec<(String, String, u64)> = t
                    .iter()
                    .map(|(h, (a, until))| (hex::encode(h), a.as_str().to_string(), *until))
                    .collect();
                if let Ok(b) = serde_json::to_vec(&rows) {
                    crate::records::write_atomically(file, &b);
                }
            }
        }
        token
    }

    /// The wallet a token names, if it's one of ours and still good.
    pub fn redeem(&self, token: &[u8; TOKEN_BYTES]) -> Option<PlayerId> {
        let t = self.held.lock().ok()?;
        t.get(&el_auth::keccak256(token)).filter(|(_, until)| *until > unix_now()).map(|(a, _)| *a)
    }
}

struct Entry {
    name: String,
    agent: bool,
    since: u64,
    session: u32,
    /// What they're doing, in words (at most 64 printable characters), for the store's friends
    /// list and presence: "At the menu", "Finding a match", "Champion select", "In a match"...
    status: &'static str,
}

/// What a session is doing, as `/online.json` says it.
pub mod status {
    pub const MENU: &str = "At the menu";
    pub const QUEUED: &str = "Finding a match";
    pub const RANKED: &str = "In the ranked queue";
    pub const SELECT: &str = "Champion select";
    pub const PLAYING: &str = "In a match";
    pub const PRACTICE: &str = "Practising";
}

/// The wallets on now, by id (so the document reads in a stable order). The store's playtime polls
/// it once a minute and counts hours per wallet (`meter/playtime.py` in AnthonE/scry-forge), in the
/// shape Gates' shard and Before Colony serve. `wallet` is the player's id as Elo Pros writes it:
/// `0x…` lowercase, base58 exactly as written, or an account's `acct_…`. Guests have none and
/// aren't in it.
#[derive(Default)]
pub struct Online(Mutex<BTreeMap<PlayerId, Entry>>);

impl Online {
    /// `address` is on in `session` until the guard drops. A newer session for the same wallet
    /// takes the entry over, and the older one's guard then leaves it be.
    pub fn enter(self: &Arc<Self>, address: PlayerId, session: u32, name: &str, agent: bool) -> Here {
        if let Ok(mut on) = self.0.lock() {
            on.insert(
                address,
                Entry { name: name.to_string(), agent, since: unix_now(), session, status: status::MENU },
            );
        }
        Here { online: self.clone(), address, session }
    }

    /// The session `address` is on now, if any.
    pub fn session_of(&self, address: &PlayerId) -> Option<u32> {
        self.0.lock().ok()?.get(address).map(|e| e.session)
    }

    /// `{"at": unix, "online": [{"wallet", "name", "agent", "since", "status"}]}`.
    pub fn json(&self) -> serde_json::Value {
        let online: Vec<serde_json::Value> = self.0.lock().map_or_else(
            |_| Vec::new(),
            |on| {
                on.iter()
                    .map(|(a, e)| {
                        serde_json::json!({
                            "wallet": a.as_str(),
                            "name": e.name,
                            "agent": e.agent,
                            "since": e.since,
                            "status": e.status,
                        })
                    })
                    .collect()
            },
        );
        serde_json::json!({ "at": unix_now(), "online": online })
    }
}

/// A session's place in [`Online`], given back when it ends.
pub struct Here {
    online: Arc<Online>,
    address: PlayerId,
    session: u32,
}

impl Here {
    /// What this session is doing now ([`status`]).
    pub fn set_status(&self, now: &'static str) {
        if let Ok(mut on) = self.online.0.lock()
            && let Some(e) = on.get_mut(&self.address).filter(|e| e.session == self.session)
        {
            e.status = now;
        }
    }
}

impl Drop for Here {
    fn drop(&mut self) {
        if let Ok(mut on) = self.online.0.lock()
            && on.get(&self.address).is_some_and(|e| e.session == self.session)
        {
            on.remove(&self.address);
        }
    }
}

/// How long a nonce is good for: long enough to sit on the page until a pass (ten minutes of its
/// own) is used.
pub const NONCE_TTL: Duration = Duration::from_secs(15 * 60);
const NONCE_CAP: usize = 100_000;

/// The nonces this server hands the page, each good once.
///
/// A nonce carries its own proof: `hex(issued unix seconds, 8 bytes)`, `hex(8 random bytes)`, then
/// the first 16 bytes of their HMAC-SHA256 under this process's key, in hex: 64 characters, inside
/// Elo Pros' `[0-9A-Za-z]{8,64}`. So it needs no list of what was issued (oneshot's, whose page signs
/// in the same way).
pub struct Nonces {
    key: ring::hmac::Key,
    used: Mutex<BTreeMap<String, u64>>,
}

impl Default for Nonces {
    fn default() -> Self {
        Self {
            key: ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &random::<32>()),
            used: Mutex::new(BTreeMap::new()),
        }
    }
}

impl Nonces {
    fn mac(&self, head: &str) -> String {
        hex::encode(&ring::hmac::sign(&self.key, head.as_bytes()).as_ref()[..16])
    }

    pub fn issue(&self) -> String {
        self.issue_at(unix_now())
    }

    pub fn issue_at(&self, now: u64) -> String {
        let head = format!("{}{}", hex::encode(now.to_be_bytes()), hex::encode(random::<8>()));
        let mac = self.mac(&head);
        head + &mac
    }

    /// Whether `n` was issued here in the last [`NONCE_TTL`] and hasn't been taken. Takes it.
    pub fn take(&self, n: &str) -> bool {
        self.take_at(n, unix_now())
    }

    pub fn take_at(&self, n: &str, now: u64) -> bool {
        if n.len() != 64 || !n.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return false;
        }
        let (head, mac) = n.split_at(32);
        // Compared whole, in constant time.
        let want = self.mac(head);
        if want.bytes().zip(mac.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) != 0 {
            return false;
        }
        let Ok(at) = u64::from_str_radix(&head[..16], 16) else { return false };
        if at > now + 5 || now.saturating_sub(at) >= NONCE_TTL.as_secs() {
            return false;
        }
        let Ok(mut used) = self.used.lock() else { return false };
        used.retain(|_, t| now.saturating_sub(*t) < NONCE_TTL.as_secs());
        if used.contains_key(n) || used.len() >= NONCE_CAP {
            return false;
        }
        used.insert(n.to_string(), at);
        true
    }
}

/// How the server signs players in.
pub struct SignIn {
    /// The domains a challenge may name (`--siwe-domain`): the first unless the page came from
    /// another of them.
    pub domains: Vec<String>,
    /// People must sign in (agents too, unless they bring a wallet they may play as guests).
    pub required: bool,
    /// How long a player's wallet has to sign.
    pub wait: Duration,
    /// How long a resume token is good for.
    pub token_ttl: Duration,
    pub tokens: Tokens,
    pub online: Arc<Online>,
    /// Sign-ins waiting on a wallet at once.
    pub waiting: Arc<tokio::sync::Semaphore>,
    /// Elo Pros' account key (`--elo-pubkey`). Without it no pass is taken.
    pub pass_key: Option<[u8; 32]>,
    /// The nonces a pass must carry (`/nonce`).
    pub nonces: Nonces,
}

impl SignIn {
    pub fn new(domains: Vec<String>, required: bool) -> Self {
        Self {
            domains,
            required,
            wait: Duration::from_secs(90),
            token_ttl: Duration::from_secs(12 * 3_600),
            tokens: Tokens::default(),
            online: Arc::new(Online::default()),
            waiting: Arc::new(tokio::sync::Semaphore::new(256)),
            pass_key: None,
            nonces: Nonces::default(),
        }
    }

    /// A challenge's nonce and time.
    pub fn challenge() -> ([u8; NONCE_BYTES], u64) {
        (random::<NONCE_BYTES>(), unix_now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use el_proto::auth::Address;

    #[test]
    fn nonces_are_good_once_and_for_a_while() {
        let n = Nonces::default();
        let a = n.issue_at(1000);
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|b| b.is_ascii_alphanumeric()), "Elo Pros' nonce shape");
        assert!(n.take_at(&a, 1001));
        assert!(!n.take_at(&a, 1002), "once");
        assert!(!n.take_at(&"0".repeat(64), 1001));
        assert!(!Nonces::default().take_at(&n.issue_at(1000), 1001), "another key");
        let ttl = NONCE_TTL.as_secs();
        assert!(!n.take_at(&n.issue_at(1000), 1000 + ttl));
        assert!(n.take_at(&n.issue_at(1000), 1000 + ttl - 1));
        assert!(!n.take_at(&n.issue_at(2000), 1000), "not from the future");
    }

    #[test]
    fn the_account_key_is_base64_of_32_bytes() {
        assert_eq!(parse_pubkey("H0SHD2PwtN2Lb2tZhyrcrQCHGFIs+qy9VvR4aISFwlE=").map(|k| k[0]), Some(0x1f));
        assert_eq!(parse_pubkey("AAAA"), None);
    }

    #[test]
    fn a_solana_wallet_and_an_account_are_on_the_roster_and_in_the_tokens_file_as_written() {
        let sol = "GmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB";
        let acct = "acct_0123456789abcdef0123";
        let online = Arc::new(Online::default());
        let _a = online.enter(PlayerId::parse(sol).unwrap(), 1, "Sol", false);
        let _b = online.enter(PlayerId::parse(acct).unwrap(), 2, "Acct", false);
        let doc = online.json();
        let wallets: Vec<&str> =
            doc["online"].as_array().unwrap().iter().map(|e| e["wallet"].as_str().unwrap()).collect();
        assert!(wallets.contains(&sol) && wallets.contains(&acct), "{wallets:?}");

        let file = std::env::temp_dir().join(format!("el-tokens-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&file);
        let t = Tokens::kept(file.clone()).issue(PlayerId::parse(sol).unwrap(), Duration::from_secs(60));
        assert_eq!(
            Tokens::kept(file.clone()).redeem(&t).map(|a| a.as_str().to_string()).as_deref(),
            Some(sol)
        );
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn a_challenge_names_the_pages_own_domain() {
        let d = vec!["elopros.com".to_string(), "game.elopros.com".to_string(), "127.0.0.1:8080".to_string()];
        assert_eq!(pick_domain(&d, Some("https://game.elopros.com")), "game.elopros.com");
        assert_eq!(pick_domain(&d, Some("https://elopros.com:443")), "elopros.com");
        assert_eq!(pick_domain(&d, Some("http://127.0.0.1:8080")), "127.0.0.1:8080");
        assert_eq!(pick_domain(&d, Some("https://evil.example")), "elopros.com");
        assert_eq!(pick_domain(&d, None), "elopros.com");
    }

    #[test]
    fn a_token_names_its_wallet_until_it_runs_out() {
        let t = Tokens::default();
        let a = PlayerId::from(Address([7; 20]));
        let token = t.issue(a, Duration::from_secs(60));
        assert_eq!(t.redeem(&token), Some(a));
        assert_eq!(t.redeem(&[0; TOKEN_BYTES]), None);
        let gone = t.issue(a, Duration::ZERO);
        assert_eq!(t.redeem(&gone), None);
    }

    #[test]
    fn a_takeover_keeps_the_newer_session() {
        let online = Arc::new(Online::default());
        let a = PlayerId::from(Address([0xAB; 20]));
        let first = online.enter(a, 1, "Ace", false);
        let second = online.enter(a, 2, "Ace", false);
        drop(first);
        let doc = online.json();
        assert_eq!(doc["online"][0]["wallet"], format!("0x{}", "ab".repeat(20)));
        assert_eq!(doc["online"][0]["name"], "Ace");
        assert_eq!(doc["online"][0]["status"], status::MENU);
        second.set_status(status::QUEUED);
        assert_eq!(online.json()["online"][0]["status"], status::QUEUED);
        drop(second);
        assert_eq!(online.json()["online"], serde_json::json!([]));
    }

    #[test]
    fn a_short_address_keeps_its_ends() {
        let a = PlayerId::parse("0x7e5f4552091a69125d5dfcb7b8c2659029395bdf").unwrap();
        assert_eq!(short(&a), "0x7e5f…5bdf");
    }
}
