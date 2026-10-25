//! Wallet sign-in on the server: the domains a challenge may name, the resume tokens that bring a
//! signed-in player back without asking their wallet again, and who is on (`/online.json`, for the
//! Elo Pros store). The check itself is `el_auth::verify`. Nothing here is near a shard thread.
//!
//! Never logged: signatures and resume tokens. An address is logged only shortened ([`short`]).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use el_proto::auth::{Address, NONCE_BYTES, TOKEN_BYTES};

/// Seconds since the epoch.
pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// `0x1234…cdef`: an address as a log may name it.
pub fn short(a: &Address) -> String {
    let hex = el_auth::lower_hex(a);
    let s = std::str::from_utf8(&hex).unwrap_or("0x?");
    format!("{}…{}", &s[..6], &s[38..])
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

/// Resume tokens: each names a wallet, for a while.
#[derive(Default)]
pub struct Tokens(Mutex<HashMap<[u8; TOKEN_BYTES], (Address, Instant)>>);

impl Tokens {
    /// A new token for `address`, good for `ttl`.
    pub fn issue(&self, address: Address, ttl: Duration) -> [u8; TOKEN_BYTES] {
        let token = random::<TOKEN_BYTES>();
        if let Ok(mut t) = self.0.lock() {
            let now = Instant::now();
            t.retain(|_, (_, until)| *until > now);
            t.insert(token, (address, now + ttl));
        }
        token
    }

    /// The wallet a token names, if it's one of ours and still good.
    pub fn redeem(&self, token: &[u8; TOKEN_BYTES]) -> Option<Address> {
        let t = self.0.lock().ok()?;
        t.get(token).filter(|(_, until)| *until > Instant::now()).map(|(a, _)| *a)
    }
}

struct Entry {
    name: String,
    agent: bool,
    since: u64,
    session: u32,
}

/// The wallets on now, by address (so the document reads in a stable order). The store's
/// playtime polls it once a minute and counts hours per wallet (`meter/playtime.py` in
/// AnthonE/scry-forge), in the shape Gates' shard and Before Colony serve. Guests have no wallet
/// and aren't in it.
#[derive(Default)]
pub struct Online(Mutex<BTreeMap<[u8; 20], Entry>>);

impl Online {
    /// `address` is on in `session` until the guard drops. A newer session for the same wallet
    /// takes the entry over, and the older one's guard then leaves it be.
    pub fn enter(self: &Arc<Self>, address: Address, session: u32, name: &str, agent: bool) -> Here {
        if let Ok(mut on) = self.0.lock() {
            on.insert(address.0, Entry { name: name.to_string(), agent, since: unix_now(), session });
        }
        Here { online: self.clone(), address, session }
    }

    /// `{"at": unix, "online": [{"wallet", "name", "agent", "since"}]}`, wallets lowercase.
    pub fn json(&self) -> serde_json::Value {
        let online: Vec<serde_json::Value> = self.0.lock().map_or_else(
            |_| Vec::new(),
            |on| {
                on.iter()
                    .map(|(a, e)| {
                        let wallet = el_auth::lower_hex(&Address(*a));
                        serde_json::json!({
                            "wallet": std::str::from_utf8(&wallet).unwrap_or_default(),
                            "name": e.name,
                            "agent": e.agent,
                            "since": e.since,
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
    address: Address,
    session: u32,
}

impl Drop for Here {
    fn drop(&mut self) {
        if let Ok(mut on) = self.online.0.lock()
            && on.get(&self.address.0).is_some_and(|e| e.session == self.session)
        {
            on.remove(&self.address.0);
        }
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
        let a = Address([7; 20]);
        let token = t.issue(a, Duration::from_secs(60));
        assert_eq!(t.redeem(&token), Some(a));
        assert_eq!(t.redeem(&[0; TOKEN_BYTES]), None);
        let gone = t.issue(a, Duration::ZERO);
        assert_eq!(t.redeem(&gone), None);
    }

    #[test]
    fn a_takeover_keeps_the_newer_session() {
        let online = Arc::new(Online::default());
        let a = Address([0xAB; 20]);
        let first = online.enter(a, 1, "Ace", false);
        let second = online.enter(a, 2, "Ace", false);
        drop(first);
        let doc = online.json();
        assert_eq!(doc["online"][0]["wallet"], format!("0x{}", "ab".repeat(20)));
        assert_eq!(doc["online"][0]["name"], "Ace");
        drop(second);
        assert_eq!(online.json()["online"], serde_json::json!([]));
    }

    #[test]
    fn a_short_address_keeps_its_ends() {
        let a = el_auth::parse_address("0x7e5f4552091a69125d5dfcb7b8c2659029395bdf").unwrap();
        assert_eq!(short(&a), "0x7e5f…5bdf");
    }
}
