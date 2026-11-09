//! The desktop's line to the Elo launcher (`el-launcher`): asked once, on a thread, at the start;
//! kept for the program's life. Without one (none running, or `--no-launcher`) the game plays as
//! a guest and opens nothing: that is a normal state, never a reason to ask for a key.

use std::sync::{Mutex, OnceLock};

use el_launcher::{Elo, Proved};

use crate::platform::Opened;

/// The launcher, once asked: `None` inside while there is none.
fn line() -> &'static Mutex<Option<Elo>> {
    static LINE: OnceLock<Mutex<Option<Elo>>> = OnceLock::new();
    LINE.get_or_init(|| Mutex::new(None))
}

/// What the launcher said when it was asked.
#[derive(Clone, Debug, Default)]
pub struct Found {
    pub running: bool,
    pub signer: String,
    pub address: Option<String>,
    pub title_url: Option<String>,
    pub servers_url: Option<String>,
    pub why: Option<String>,
}

/// Whether the launcher answered.
pub fn running() -> bool {
    line().lock().is_ok_and(|l| l.is_some())
}

/// Asks for the launcher (blocking: a local socket, a few milliseconds when it's there).
pub fn connect() -> Found {
    match Elo::connect(env!("CARGO_PKG_VERSION")) {
        Ok(mut elo) => {
            let found = Found {
                running: true,
                signer: elo.signer(),
                address: elo.address(),
                title_url: elo.title_url(),
                servers_url: elo.servers_url(),
                why: None,
            };
            if let Ok(mut l) = line().lock() {
                *l = Some(elo);
            }
            found
        }
        Err(why) => Found { why: Some(why), ..Found::default() },
    }
}

/// Signs the server's sign-in text through the launcher: it can wait minutes for the player, so
/// only from a thread that may wait.
pub fn prove_at(domain: &str, nonce_hex: &str, issued_at: u64) -> Result<Proved, String> {
    let mut l = line().lock().map_err(|_| "the launcher's line broke")?;
    let elo = l.as_mut().ok_or("the Elo launcher isn't running: start Eloram from it to sign in")?;
    elo.prove_at(domain, nonce_hex, issued_at)
}

/// Opens `url` in the player's browser. Never waits on a sign-in under way: then it says no.
pub fn open(url: &str) -> Opened {
    let Ok(mut l) = line().try_lock() else { return Opened::Refused };
    match l.as_mut() {
        None => Opened::NoLauncher,
        Some(elo) => {
            if elo.open(url) {
                Opened::Opened
            } else {
                Opened::Refused
            }
        }
    }
}
