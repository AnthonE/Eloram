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
    assert_eq!((launcher["players"].as_u64(), launcher["agents"].as_u64()), (Some(0), Some(2)), "{launcher}");
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
