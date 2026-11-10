//! The desktop's launch: what the page's address and `localStorage` say in the browser, said on the
//! command line and read from the settings file. The Elo launcher starts the game with
//! `--server {server} --identity {wallet} --servers {servers}` (an empty value is no value), and a
//! join link (`elo://join/eloram/host:port`) arrives as `--server` or by itself.

use super::LaunchConfig;
use crate::platform::kv_get;

/// What the arguments came to.
#[derive(Debug)]
pub enum Parsed {
    Run(Box<LaunchConfig>),
    Help,
    Bad(String),
}

pub const USAGE: &str = "\
eloram [address | elo://join/eloram/host:port] [options]

  --server host:port   the game server to play on (the launcher's {server})
  --servers url        the server list to find one in (the launcher's {servers})
  --identity 0x...     the wallet the launcher plays as (the launcher's {wallet})
  --http url           the server's HTTP side (a dev server: http://127.0.0.1:8080)
  --cert-hash hex      pin a server's self-signed certificate by its SHA-256
  --name NAME          the name a guest plays under
  --queue              straight into the queue, past the menu
  --practice [champ]   a practice match (a server started with --practice)
  --watch id|live      watch a match a minute behind (live: the newest)
  --replay url         replay a record
  --guest              don't sign in through the launcher
  --no-launcher        don't look for the Elo launcher at all
  --no-warm            don't draw a match's models and effects out of sight before it
  --keys league|wasd   the keys' layout
  --indicators 0|1     cast with an indicator
  --camera locked|free the camera
  --quality low|high   the picture's tier
  --fullscreen | --windowed
  --mode echo          the transport's smoke test
  --status file        write the game's status (JSON) to file, four times a second
  --inbox file         read commands (JSON lines) from file, as the page's __elInbox
  --screenshot png     save a screenshot once --screenshot-at's moment comes, then quit
  --screenshot-at P    a phase (lobby, playing...) or menu
  --help";

/// `elo://join/eloram/host:port` (or the old `scry://`) to its address; another title's is refused.
pub fn join_link(link: &str) -> Result<String, String> {
    let rest = link
        .strip_prefix("elo://join/")
        .or_else(|| link.strip_prefix("scry://join/"))
        .ok_or_else(|| format!("{link}: not a join link"))?;
    let (slug, addr) = rest.split_once('/').ok_or_else(|| format!("{link}: no address"))?;
    if slug != el_launcher::SLUG {
        return Err(format!("{link}: a link to {slug}, not Eloram"));
    }
    let addr = addr.trim_end_matches('/');
    check_addr(addr)?;
    Ok(addr.to_string())
}

/// A `host:port`, unresolved: a name or an address, a port, nothing after.
pub fn check_addr(addr: &str) -> Result<(), String> {
    let (host, port) = addr.rsplit_once(':').ok_or_else(|| format!("{addr}: no port"))?;
    let ok_host = !host.is_empty()
        && host.len() <= 253
        && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '[' | ']' | ':'));
    if !ok_host || port.parse::<u16>().map_or(true, |p| p == 0) {
        return Err(format!("{addr}: not a host:port"));
    }
    Ok(())
}

/// Parses the arguments (the program's name left off), over the settings already kept.
pub fn parse(args: impl IntoIterator<Item = String>) -> Parsed {
    let mut cfg = LaunchConfig {
        // What a match draws, drawn before it (`warm.rs`; `--no-warm` leaves it to the match).
        warm: true,
        // The settings this machine kept, as the page's loader reads them from localStorage.
        wasd: kv_get("el.keys").is_some_and(|v| v == "wasd"),
        indicators: kv_get("el.indicators").is_some_and(|v| v == "1"),
        camera_locked: kv_get("el.camera").is_some_and(|v| v == "locked"),
        low: kv_get("el.quality").is_some_and(|v| v == "low"),
        resume: kv_get("el.resume").and_then(|h| super::decode_hex(&h)).and_then(|b| b.try_into().ok()),
        sign_in: true,
        ..LaunchConfig::default()
    };
    let mut args = args.into_iter().peekable();
    // A flag's value: `""` from the launcher is no value.
    fn value(
        args: &mut std::iter::Peekable<impl Iterator<Item = String>>,
        flag: &str,
    ) -> Result<Option<String>, String> {
        match args.next() {
            Some(v) if v.is_empty() => Ok(None),
            Some(v) => Ok(Some(v)),
            None => Err(format!("{flag} needs a value")),
        }
    }
    let mut join: Option<String> = None;
    while let Some(a) = args.next() {
        let r: Result<(), String> = (|| {
            match a.as_str() {
                "--help" | "-h" => return Err("help".into()),
                "--server" => cfg.server = value(&mut args, &a)?,
                "--servers" => cfg.servers_url = value(&mut args, &a)?,
                "--identity" => cfg.identity = value(&mut args, &a)?,
                "--http" => cfg.http_base = value(&mut args, &a)?.unwrap_or_default(),
                "--cert-hash" => {
                    let h = value(&mut args, &a)?.unwrap_or_default().replace(':', "");
                    cfg.cert_hash = Some(
                        super::decode_hex(&h)
                            .filter(|b| b.len() == 32)
                            .ok_or("--cert-hash: 64 hex digits")?,
                    );
                }
                "--name" => cfg.name = value(&mut args, &a)?.unwrap_or_default(),
                "--queue" => cfg.queue = true,
                "--practice" => {
                    let champ = match args.peek() {
                        Some(n) if !n.starts_with('-') => {
                            args.next().and_then(|n| n.parse::<u8>().ok()).unwrap_or(0)
                        }
                        _ => 0,
                    };
                    cfg.practice = Some(champ);
                }
                "--watch" => {
                    let w = value(&mut args, &a)?.ok_or("--watch needs a record id or live")?;
                    if w == "live" {
                        cfg.watch_newest = true;
                    } else {
                        cfg.record_id = Some(w);
                    }
                    cfg.watch_live = true;
                }
                "--replay" => cfg.record_url = value(&mut args, &a)?,
                "--guest" => cfg.sign_in = false,
                "--signin" => cfg.sign_in = true,
                "--no-launcher" => cfg.no_launcher = true,
                "--no-warm" => cfg.warm = false,
                "--keys" => cfg.wasd = value(&mut args, &a)?.is_some_and(|v| v == "wasd"),
                "--indicators" => cfg.indicators = value(&mut args, &a)?.is_some_and(|v| v == "1"),
                "--camera" => cfg.camera_locked = value(&mut args, &a)?.is_some_and(|v| v == "locked"),
                "--quality" => cfg.low = value(&mut args, &a)?.is_some_and(|v| v == "low"),
                "--fullscreen" => cfg.fullscreen = Some(true),
                "--windowed" => cfg.fullscreen = Some(false),
                "--mode" => cfg.echo = value(&mut args, &a)?.is_some_and(|v| v == "echo"),
                "--bench" => {
                    cfg.bench = true;
                    cfg.practice.get_or_insert(0);
                }
                "--status" => cfg.status_file = value(&mut args, &a)?,
                "--inbox" => cfg.inbox_file = value(&mut args, &a)?,
                "--screenshot" => cfg.screenshot = value(&mut args, &a)?,
                "--screenshot-at" => cfg.screenshot_at = value(&mut args, &a)?,
                "--join" => join = value(&mut args, &a)?,
                s if s.starts_with('-') => return Err(format!("{s}: not an option (--help lists them)")),
                s if s.contains("://") => join = Some(s.to_string()),
                s => {
                    check_addr(s)?;
                    if cfg.server.is_none() {
                        cfg.server = Some(s.to_string());
                    }
                }
            }
            Ok(())
        })();
        match r {
            Err(e) if e == "help" => return Parsed::Help,
            Err(e) => return Parsed::Bad(e),
            Ok(()) => {}
        }
    }
    // `--server` wins over a join link, as the launcher's own `{server}` is the one it chose.
    if let Some(link) = join
        && cfg.server.is_none()
    {
        match join_link(&link) {
            Ok(addr) => cfg.server = Some(addr),
            Err(e) => return Parsed::Bad(e),
        }
    }
    if let Some(s) = &cfg.server
        && let Err(e) = check_addr(s)
    {
        return Parsed::Bad(e);
    }
    if cfg.cert_hash.is_some() && cfg.server.is_none() && cfg.http_base.is_empty() {
        return Parsed::Bad("--cert-hash pins the server --server names".into());
    }
    Parsed::Run(Box::new(cfg))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> LaunchConfig {
        match parse(args.iter().map(|s| s.to_string())) {
            Parsed::Run(c) => *c,
            other => panic!("{args:?}: {other:?}"),
        }
    }

    #[test]
    fn the_launchers_line_with_empty_values_is_no_values() {
        let c = run(&[
            "--server",
            "",
            "--identity",
            "",
            "--servers",
            "https://elopros.com/api/launcher/servers/eloram",
        ]);
        assert_eq!((c.server, c.identity), (None, None));
        assert_eq!(c.servers_url.as_deref(), Some("https://elopros.com/api/launcher/servers/eloram"));
        let c = run(&["--server", "game.elopros.com:61260", "--identity", "0xabc"]);
        assert_eq!(c.server.as_deref(), Some("game.elopros.com:61260"));
    }

    #[test]
    fn join_links_land_on_their_address_and_only_eloram_s() {
        assert_eq!(
            run(&["elo://join/eloram/game.elopros.com:61260"]).server.as_deref(),
            Some("game.elopros.com:61260")
        );
        assert!(matches!(parse(["elo://join/gates/game.elopros.com:61234".to_string()]), Parsed::Bad(_)));
        assert!(matches!(parse(["elo://join/eloram/host:port/x".to_string()]), Parsed::Bad(_)));
        // The launcher's own --server wins over a link.
        let c = run(&["--server", "a.example:1", "elo://join/eloram/b.example:2"]);
        assert_eq!(c.server.as_deref(), Some("a.example:1"));
    }

    #[test]
    fn what_is_refused() {
        for bad in [
            &["--frobnicate"][..],
            &["--server"],
            &["host-without-port"],
            &["--cert-hash", "abcd", "--server", "h:1"],
        ] {
            assert!(matches!(parse(bad.iter().map(|s| s.to_string())), Parsed::Bad(_)), "{bad:?}");
        }
        assert!(matches!(parse(["--help".to_string()]), Parsed::Help));
    }

    #[test]
    fn deep_links_say_what_they_skip_to() {
        let c = run(&["--practice", "7", "--queue"]);
        assert_eq!((c.practice, c.queue), (Some(7), true));
        let c = run(&["--watch", "live"]);
        assert!(c.watch_newest && c.watch_live);
        let c = run(&["--watch", "00ff00ff00ff00ff"]);
        assert_eq!(c.record_id.as_deref(), Some("00ff00ff00ff00ff"));
        assert!(!run(&["--guest"]).sign_in);
    }
}
