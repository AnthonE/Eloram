//! Load test: many agents at once, a match's seats each.
//!
//!   cargo run -p el-bot --release --bin el-swarm -- --bots 40 --secs 60
//!
//! Every agent queues; the server seats them ten to a match (bots fill the rest after its
//! `--fill-after`). Each plays the scripted brain from its own client, then the swarm prints what
//! it got and what the server says it cost.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use clap::Parser;
use el_bot::{BotClient, BotConfig};

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    server: String,
    #[arg(long, default_value_t = 10)]
    bots: usize,
    #[arg(long, default_value_t = 30)]
    secs: u64,
    /// Each agent signs in with a throwaway test key (the secret is its number, from 1): the
    /// server's `/online.json` then lists their wallets.
    #[arg(long)]
    sign_in: bool,
}

#[derive(Default)]
struct Totals {
    seated: AtomicU64,
    datagrams: AtomicU64,
    bytes: AtomicU64,
    errors: AtomicU64,
    stale: AtomicU64,
    orders: AtomicU64,
    unacked: AtomicU64,
    walking: AtomicU64,
    walking_off: AtomicU64,
    failed: AtomicU64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let a = Args::parse();
    let totals = Arc::new(Totals::default());
    let mut tasks = Vec::new();
    for k in 0..a.bots {
        let totals = totals.clone();
        let server = a.server.clone();
        let secs = a.secs;
        let key = a.sign_in.then(|| {
            let mut secret = [0u8; 32];
            secret[30..].copy_from_slice(&(k as u16 + 1).to_be_bytes());
            secret
        });
        tasks.push(tokio::spawn(async move {
            if let Err(e) =
                one(k, BotConfig { server, name: format!("Swarm-{k:03}"), key }, secs, &totals).await
            {
                eprintln!("agent {k}: {e:#}");
                totals.failed.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }
    for t in tasks {
        let _ = t.await;
    }
    let t = &totals;
    let r = |c: &AtomicU64| c.load(Ordering::Relaxed);
    let seated = r(&t.seated).max(1) as f64;
    let secs = a.secs.max(1) as f64;
    println!("agents seated       {}/{}", r(&t.seated), a.bots);
    println!("datagrams/agent/s   {:.1}", r(&t.datagrams) as f64 / seated / secs);
    println!("downlink/agent      {:.1} KB/s", r(&t.bytes) as f64 / seated / secs / 1_000.0);
    println!("stale / errors      {} / {}", r(&t.stale), r(&t.errors));
    println!("orders / unacked    {} / {}", r(&t.orders), r(&t.unacked));
    println!("walking corrections {} ({} off by a unit or more)", r(&t.walking), r(&t.walking_off));
    println!("failed              {}", r(&t.failed));
    if let Ok(status) = el_bot::get_json(&a.server, "/status").await {
        for s in status["game"]["shards"].as_array().into_iter().flatten() {
            println!(
                "shard: {} matches, tick p50 ≤{} µs p99 ≤{} µs, overruns {}, out drops {}, refused {}",
                s["matches"],
                s["tick_us"]["p50"],
                s["tick_us"]["p99"],
                s["overruns"],
                s["out_drops"],
                s["refused"]
            );
        }
        println!("hot-path allocations {}", status["hot_path_allocations"]);
    }
    if let Ok(s) = el_bot::get_json(&a.server, "/status.json").await {
        println!("/status.json        {s}");
    }
    Ok(())
}

async fn one(k: usize, cfg: BotConfig, secs: u64, t: &Totals) -> anyhow::Result<()> {
    let mut bot = BotClient::connect(&cfg).await?;
    let _ = k;
    bot.queue().await?;
    bot.wait_match(Duration::from_secs(120)).await?;
    t.seated.fetch_add(1, Ordering::Relaxed);
    let mut brain = el_bot::scripted();
    let mut walking = 0;
    let mut off = 0;
    let end = std::time::Instant::now() + Duration::from_secs(secs);
    while std::time::Instant::now() < end && bot.over.is_none() {
        bot.step(&mut brain).await?;
        if let Some(c) = bot.client.as_mut().and_then(|c| c.correction.take())
            && c.cause == el_client_core::Cause::Walking
        {
            walking += 1;
            off += u64::from(c.error >= 1.0);
        }
    }
    let c = bot.client.as_ref().expect("seated");
    let s = c.stats;
    t.datagrams.fetch_add(s.datagrams, Ordering::Relaxed);
    t.bytes.fetch_add(s.bytes, Ordering::Relaxed);
    t.errors.fetch_add(s.errors, Ordering::Relaxed);
    t.stale.fetch_add(s.stale, Ordering::Relaxed);
    t.orders.fetch_add(c.orders.given, Ordering::Relaxed);
    t.unacked.fetch_add(c.orders.outstanding() as u64, Ordering::Relaxed);
    t.walking.fetch_add(walking, Ordering::Relaxed);
    t.walking_off.fetch_add(off, Ordering::Relaxed);
    bot.close().await;
    Ok(())
}
