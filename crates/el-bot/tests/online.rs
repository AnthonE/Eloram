//! Agents over real WebTransport against a real server: the handshake, the queue, a match with the
//! server's bots in the empty seats, snapshots in and orders out, and the store's counts.

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
    for k in 0..2 {
        let server = server.clone();
        tasks.push(tokio::spawn(async move {
            let mut bot = BotClient::connect(&BotConfig { server, name: format!("Agent-{k}") }).await?;
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
