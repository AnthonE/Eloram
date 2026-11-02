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
