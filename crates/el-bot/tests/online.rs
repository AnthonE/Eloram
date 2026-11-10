//! Agents over real WebTransport against a real server: the handshake (one signing in with a
//! wallet), the queue, a match with the server's bots in the empty seats, snapshots in and orders
//! out, and the store's counts and roster.

use std::time::Duration;

use el_bot::{BotClient, BotConfig};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agents_play_a_match_over_webtransport() {
    let cfg = el_server::Config {
        wt_port: 0,
        http_addr: "127.0.0.1:0".parse().unwrap(),
        web_dir: None,
        matches_per_shard: 4,
        fill_after: Duration::from_millis(1_500),
        ..el_server::Config::default()
    };
    let h = el_server::start(cfg).await.expect("the server starts");
    let server = format!("http://{}", h.http_addr);
    let mut tasks = Vec::new();
    // The first signs in with a wallet; the second plays as a guest.
    let key = |k: u8| {
        let mut secret = [0u8; 32];
        secret[31] = k;
        secret
    };
    for k in 0..2u8 {
        let server = server.clone();
        let key = (k == 0).then(|| key(1));
        tasks.push(tokio::spawn(async move {
            let mut bot = BotClient::connect(&BotConfig { server, name: format!("Agent-{k}"), key }).await?;
            anyhow::ensure!(bot.address.is_some() == key.is_some(), "signed in as it asked");
            bot.queue().await?;
            bot.wait_match(Duration::from_secs(10)).await?;
            bot.play(Duration::from_secs(12), &mut el_bot::scripted()).await?;
            let c = bot.client.take().expect("seated");
            let roster = bot.roster.clone();
            bot.close().await;
            anyhow::Ok((c.stats, c.orders.given, c.orders.outstanding(), c.seat, roster))
        }));
    }
    tokio::time::sleep(Duration::from_secs(6)).await;
    let launcher = el_bot::get_json(&server, "/status.json").await.unwrap();
    assert_eq!((launcher["players"].as_u64(), launcher["agents"].as_u64()), (Some(2), Some(2)), "{launcher}");
    let status = el_bot::get_json(&server, "/status").await.unwrap();
    assert_eq!(status["game"]["matches"], 1, "{status}");
    // The store's roster: the signed-in agent's wallet (test key 1's), and not the guest.
    let online = el_bot::get_json(&server, "/online.json").await.unwrap();
    let on = online["online"].as_array().unwrap();
    assert_eq!(on.len(), 1, "{online}");
    assert_eq!(on[0]["wallet"], "0x7e5f4552091a69125d5dfcb7b8c2659029395bdf");
    assert_eq!((on[0]["name"].as_str(), on[0]["agent"].as_bool()), (Some("Agent-0"), Some(true)));
    let mut seats = Vec::new();
    for t in tasks {
        let (stats, given, unacked, seat, roster) = t.await.unwrap().expect("an agent plays");
        println!("seat {seat}: {stats:?}, {given} orders given, {unacked} unacked");
        assert!(stats.datagrams > 250, "{} datagrams in 12 s", stats.datagrams);
        assert_eq!((stats.errors, stats.no_baseline), (0, 0));
        assert!(given > 5, "{given} orders");
        assert!(unacked <= 8, "{unacked} orders unacked at the end");
        assert_eq!(roster.iter().filter(|p| p.agent).count(), 2, "{roster:?}");
        assert_eq!(roster.iter().filter(|p| p.bot).count(), 8);
        seats.push(seat);
    }
    seats.sort_unstable();
    assert_eq!(seats, [0, 5], "a side each");
    let status = el_bot::get_json(&server, "/status").await.unwrap();
    let shard = &status["game"]["shards"][0];
    assert_eq!(shard["out_drops"], 0, "{shard}");
    assert!(shard["sent"].as_u64().unwrap() > 500, "{shard}");
    h.shutdown();
}

/// A seat survives its connection: an agent whose link drops takes its seat back on a new one with
/// its match's rejoin token, a second connection with the token takes it over from a first still
/// holding it, and a token that names no seat is told so.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dropped_agent_rejoins_its_seat() {
    let cfg = el_server::Config {
        wt_port: 0,
        http_addr: "127.0.0.1:0".parse().unwrap(),
        web_dir: None,
        matches_per_shard: 2,
        fill_after: Duration::from_millis(300),
        select_for: Duration::ZERO,
        ..el_server::Config::default()
    };
    let h = el_server::start(cfg).await.expect("the server starts");
    let server = format!("http://{}", h.http_addr);
    let join = |name: &str| BotConfig { server: server.clone(), name: name.into(), key: None };
    let mut brain = el_bot::scripted();

    let mut first = BotClient::connect(&join("Agent")).await.unwrap();
    first.queue().await.unwrap();
    first.wait_match(Duration::from_secs(10)).await.unwrap();
    first.play(Duration::from_secs(3), &mut brain).await.unwrap();
    let seat = first.client.as_ref().unwrap().seat;
    let token = first.rejoin.clone().expect("a rejoin token with the match");
    assert_eq!(token.len(), 32);
    first.drop_link();

    // The match waits for its player (the server's bot playing the seat meanwhile).
    tokio::time::sleep(Duration::from_secs(1)).await;
    let status = el_bot::get_json(&server, "/status").await.unwrap();
    assert_eq!(status["game"]["matches"], 1, "{status}");

    let mut back = BotClient::connect(&join("Agent")).await.unwrap();
    back.rejoin(&token).await.unwrap();
    back.wait_match(Duration::from_secs(10)).await.unwrap();
    back.play(Duration::from_secs(4), &mut brain).await.unwrap();
    let c = back.client.as_ref().unwrap();
    assert_eq!(c.seat, seat, "the same seat");
    assert!(c.started(), "playing again");
    assert!(c.stats.datagrams > 80, "{:?}", c.stats);
    assert_eq!((c.stats.errors, c.stats.no_baseline), (0, 0), "{:?}", c.stats);
    assert_eq!(back.rejoin.as_deref(), Some(token.as_str()), "the same token");

    // A third connection with the token while `back` still holds the seat: it takes it over.
    let mut third = BotClient::connect(&join("Agent")).await.unwrap();
    third.rejoin(&token).await.unwrap();
    third.wait_match(Duration::from_secs(10)).await.unwrap();
    third.play(Duration::from_secs(3), &mut brain).await.unwrap();
    let c = third.client.as_ref().unwrap();
    assert_eq!(c.seat, seat);
    assert!(c.stats.datagrams > 60, "{:?}", c.stats);
    assert_eq!(c.stats.errors, 0, "{:?}", c.stats);
    // The one replaced is let go.
    assert!(back.play(Duration::from_secs(3), &mut brain).await.is_err(), "the replaced session ends");

    // A token for no seat.
    let mut stray = BotClient::connect(&join("Stray")).await.unwrap();
    stray.rejoin(&"7".repeat(32)).await.unwrap();
    assert!(stray.wait_match(Duration::from_secs(5)).await.is_err());
    assert!(stray.no_seat);
    h.shutdown();
}

/// Practice champions switched one after another, as the fx suite switches them in the page: each
/// switch seats the agent in a new match while the last one's snapshots are still in flight. None
/// of those is taken for the new match (it once set the new client's clock to the old match's
/// tick, or its own champion to the old one, and the page sat there seeing nothing new): the new
/// match's client is at the new match's tick, with the champion asked for.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn practice_switches_take_nothing_of_the_last_match() {
    let cfg = el_server::Config {
        wt_port: 0,
        http_addr: "127.0.0.1:0".parse().unwrap(),
        web_dir: None,
        matches_per_shard: 4,
        practice: true,
        ..el_server::Config::default()
    };
    let h = el_server::start(cfg).await.expect("the server starts");
    let server = format!("http://{}", h.http_addr);
    let mut bot =
        BotClient::connect(&BotConfig { server: server.clone(), name: "Practice".into(), key: None })
            .await
            .unwrap();
    let mut idle = |_: &el_sim::Match, _: usize, _: u32, _: &mut el_sim::game::SeatInput| {};
    bot.practice(3).await.unwrap();
    bot.wait_match(Duration::from_secs(10)).await.unwrap();
    bot.play(Duration::from_secs(8), &mut idle).await.unwrap();
    let mut streams = vec![bot.client.as_ref().unwrap().stream()];
    let mut foreign = 0;
    for champ in [7u8, 11, 2, 16] {
        // A slow frame (the page in SwiftShader draws one every two seconds): the last match's
        // snapshots wait unread, and the switch is asked for straight after.
        tokio::time::sleep(Duration::from_secs(2)).await;
        bot.practice(champ).await.unwrap();
        bot.wait_match(Duration::from_secs(10)).await.unwrap();
        bot.play(Duration::from_secs(3), &mut idle).await.unwrap();
        let c = bot.client.as_ref().unwrap();
        assert!(c.started(), "champion {champ}: never started, {:?}", c.stats);
        assert_eq!(c.world.champs[c.seat].def, champ, "champion {champ}: {:?}", c.stats);
        // Three seconds in (the last match was eight or more): the new match's clock.
        assert!(c.world.tick < 5 * 30, "champion {champ}: at tick {}, {:?}", c.world.tick, c.stats);
        assert_eq!((c.stats.errors, c.stats.no_baseline), (0, 0), "champion {champ}: {:?}", c.stats);
        assert!(c.stats.datagrams > 60, "champion {champ}: {:?}", c.stats);
        assert!(!streams.contains(&c.stream()), "champion {champ}: stream {} again", c.stream());
        streams.push(c.stream());
        foreign += c.stats.foreign;
        println!("champion {champ}: stream {}, {:?}", c.stream(), c.stats);
    }
    println!("{foreign} snapshots of an earlier match dropped");
    assert!(foreign > 0, "no switch had the last match's snapshots waiting: the test tests nothing");
    bot.close().await;
    h.shutdown();
}

/// A Solana wallet signs in by its own signature, and an Elo Pros pass by Elo Pros': each on the
/// store's roster as Elo Pros writes it, base58 exactly as it is.
mod solana_and_pass {
    use base64::Engine;
    use el_bot::Way;
    use el_proto::auth::{PlayerId, SolanaSignature};
    use ring::signature::{Ed25519KeyPair, KeyPair};

    use super::*;

    /// Elo Pros' test key (`sdk/testdata/account_token.json`, its `seed_hex`), and `signin_check.json`'s
    /// throwaway wallet (Ed25519 seed 07…07).
    const SEED: &str = "2f3e65db2d7c08d3d45cb88040eca8badb2b6287066b2d30a1861756938b6536";
    const ACCT: &str = "acct_0123456789abcdef0123";
    const SOL: &str = "GmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB";

    fn account_key() -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed_unchecked(&hex::decode(SEED).unwrap()).unwrap()
    }

    fn mint(wallet: &str, game: &str, shard: &str, nonce: &str) -> String {
        let exp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 600;
        let text = format!(
            "elo account\nid: {ACCT}\nfor: play:{game}\nshard: {shard}\nnonce: {nonce}\nwallet: {wallet}\nexp: {exp}"
        );
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        format!("{}.{}", b64.encode(&text), b64.encode(account_key().sign(text.as_bytes())))
    }

    fn config(server: &str, name: &str) -> BotConfig {
        BotConfig { server: server.to_string(), name: name.to_string(), key: None }
    }

    async fn nonce(server: &str) -> String {
        let doc = el_bot::get_json(server, "/nonce").await.unwrap();
        assert_eq!(doc["game"], "eloram");
        doc["nonce"].as_str().expect("this server takes passes").to_string()
    }

    async fn online(server: &str) -> Vec<String> {
        let doc = el_bot::get_json(server, "/online.json").await.unwrap();
        doc["online"].as_array().unwrap().iter().map(|e| e["wallet"].as_str().unwrap().to_string()).collect()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_solana_wallet_and_a_pass_sign_in() {
        let cfg = el_server::Config {
            wt_port: 0,
            http_addr: "127.0.0.1:0".parse().unwrap(),
            web_dir: None,
            matches_per_shard: 2,
            siwe_domains: vec!["elopros.com".into()],
            elo_pubkey: Some(account_key().public_key().as_ref().try_into().unwrap()),
            ..el_server::Config::default()
        };
        let h = el_server::start(cfg).await.expect("the server starts");
        let server = format!("http://{}", h.http_addr);

        // A Solana wallet, its address exact.
        let pair = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
        let key: [u8; 32] = pair.public_key().as_ref().try_into().unwrap();
        let sign = |text: &[u8]| {
            let mut s = [0u8; 64];
            s.copy_from_slice(pair.sign(text).as_ref());
            SolanaSignature(s)
        };
        let sol =
            BotClient::connect_with(&config(&server, "Sol"), Way::Solana { key, sign: &sign }).await.unwrap();
        assert_eq!(sol.address.map(|a| a.to_string()).as_deref(), Some(SOL));
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(online(&server).await, [SOL]);

        // Someone else's signature for it.
        let other = Ed25519KeyPair::from_seed_unchecked(&[8; 32]).unwrap();
        let forged = |text: &[u8]| {
            let mut s = [0u8; 64];
            s.copy_from_slice(other.sign(text).as_ref());
            SolanaSignature(s)
        };
        assert!(
            BotClient::connect_with(&config(&server, "Forger"), Way::Solana { key, sign: &forged })
                .await
                .is_err()
        );

        // A pass with no wallet plays as its account; used, its nonce is gone.
        let n = nonce(&server).await;
        let pass = mint("none", "eloram", "elopros.com", &n);
        let acct = BotClient::connect_with(&config(&server, "Acct"), Way::Pass(&pass)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(online(&server).await.contains(&ACCT.to_string()));
        assert!(BotClient::connect_with(&config(&server, "Again"), Way::Pass(&pass)).await.is_err());

        // A pass that names a Solana wallet plays as that wallet, as written.
        let n = nonce(&server).await;
        assert_eq!(PlayerId::parse(SOL).map(|a| a.to_string()).as_deref(), Some(SOL));
        let pass = mint(SOL, "eloram", "elopros.com", &n);
        let named = BotClient::connect_with(&config(&server, "Named"), Way::Pass(&pass)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(online(&server).await.contains(&SOL.to_string()));

        // Passes never for here, and no pass at all.
        for (game, shard) in [("gates", "elopros.com"), ("eloram", "evil.example")] {
            let n = nonce(&server).await;
            let pass = mint("none", game, shard, &n);
            assert!(BotClient::connect_with(&config(&server, "Elsewhere"), Way::Pass(&pass)).await.is_err());
        }
        let made_up = mint("none", "eloram", "elopros.com", &"0123456789abcdef".repeat(4));
        assert!(BotClient::connect_with(&config(&server, "Made"), Way::Pass(&made_up)).await.is_err());
        assert!(BotClient::connect_with(&config(&server, "Junk"), Way::Pass("not a pass")).await.is_err());

        drop((sol, acct, named));
        h.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn with_no_account_key_no_pass_is_taken() {
        let cfg = el_server::Config {
            wt_port: 0,
            http_addr: "127.0.0.1:0".parse().unwrap(),
            web_dir: None,
            matches_per_shard: 2,
            ..el_server::Config::default()
        };
        let h = el_server::start(cfg).await.expect("the server starts");
        let server = format!("http://{}", h.http_addr);
        assert!(el_bot::get_json(&server, "/nonce").await.unwrap()["nonce"].is_null());
        let pass = mint("none", "eloram", "127.0.0.1", &"0".repeat(64));
        assert!(BotClient::connect_with(&config(&server, "Acct"), Way::Pass(&pass)).await.is_err());
        h.shutdown();
    }
}
