//! A crash, recovered (`el_server::recover`): a real server with a match on is killed outright
//! (SIGKILL: nothing flushed, nothing said), and a new one on the same records directory takes the
//! match up from its last verified checkpoint and its record. The agent comes back to its seat with
//! its rejoin token, as after any dropped link, and the record that results is one record,
//! re-stepped from its first tick to its last without a gap or a wrong hash.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use el_bot::{BotClient, BotConfig};
use el_sim::record::{Frame, Frames, Referee};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// The server binary on `dir`, its log in `log`.
fn server(dir: &Path, port: u16, log: &Path) -> Child {
    let out = std::fs::OpenOptions::new().create(true).append(true).open(log).unwrap();
    Command::new(env!("CARGO_BIN_EXE_el-server"))
        .args(["--wt-port", "0", "--http", &format!("127.0.0.1:{port}"), "--web-dir", "no-web-here"])
        .args([
            "--records",
            dir.to_str().unwrap(),
            "--matches",
            "2",
            "--fill-after",
            "0.3",
            "--select-secs",
            "0",
        ])
        .args(["--checkpoint-secs", "3"])
        .stdout(Stdio::from(out.try_clone().unwrap()))
        .stderr(Stdio::from(out))
        .spawn()
        .expect("the server starts")
}

async fn up(base: &str) -> serde_json::Value {
    for _ in 0..100 {
        if let Ok(s) = el_bot::get_json(base, "/status").await {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{base} never answered");
}

fn dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("el-recover-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_killed_server_s_match_is_taken_up_by_the_next() {
    let dir = dir();
    let log = dir.join("server.log");
    let port = free_port();
    let base = format!("http://127.0.0.1:{port}");
    let mut first = server(&dir, port, &log);
    up(&base).await;
    let join = |base: &str| BotConfig { server: base.to_string(), name: "Agent".into(), key: None };
    let mut brain = el_bot::scripted();
    let mut bot = BotClient::connect(&join(&base)).await.unwrap();
    bot.queue().await.unwrap();
    bot.wait_match(Duration::from_secs(10)).await.unwrap();
    bot.play(Duration::from_secs(11), &mut brain).await.unwrap();
    let seat = bot.client.as_ref().unwrap().seat;
    let token = bot.rejoin.clone().expect("a rejoin token");
    let status = el_bot::get_json(&base, "/status").await.unwrap();
    let referee = &status["game"]["referee"];
    assert!(referee["checkpoints"].as_u64().unwrap() >= 2, "{status}");
    assert_eq!(referee["divergences"], 0, "{status}");
    let tick_before = status["game"]["shards"][0]["ticks"].as_u64().unwrap();

    // Killed outright.
    first.kill().unwrap();
    first.wait().unwrap();
    drop(bot);
    let parts: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".elr.part"))
        .collect();
    assert_eq!(parts.len(), 1, "the live match's record on disk: {parts:?}");
    let id = parts[0].trim_end_matches(".elr.part").to_string();
    assert!(dir.join(format!("{id}.ck")).exists(), "and its checkpoint");
    assert!(dir.join(format!("{id}.live")).exists(), "and its lobby file");
    let live = std::fs::read_to_string(dir.join(format!("{id}.live"))).unwrap();
    assert!(!live.contains(&token), "the rejoin token itself is never on disk");

    // The next server takes it up.
    let port = free_port();
    let base = format!("http://127.0.0.1:{port}");
    let mut second = server(&dir, port, &log);
    let status = up(&base).await;
    assert_eq!(status["game"]["matches"], 1, "the match is back: {status}");
    let records = el_bot::get_json(&base, "/records.json").await.unwrap();
    assert_eq!(records[0]["id"], id.as_str(), "{records}");
    assert_eq!(records[0]["live"], true);
    let mut back = BotClient::connect(&join(&base)).await.unwrap();
    back.rejoin(&token).await.unwrap();
    back.wait_match(Duration::from_secs(10)).await.unwrap();
    back.play(Duration::from_secs(6), &mut brain).await.unwrap();
    let c = back.client.as_ref().unwrap();
    assert_eq!(c.seat, seat, "the same seat");
    assert!(c.started() && c.stats.datagrams > 100, "playing again: {:?}", c.stats);
    assert_eq!((c.stats.errors, c.stats.no_baseline), (0, 0), "{:?}", c.stats);
    let status = el_bot::get_json(&base, "/status").await.unwrap();
    assert_eq!(status["game"]["referee"]["divergences"], 0, "{status}");
    assert!(status["game"]["referee"]["checkpoints"].as_u64().unwrap() >= 1, "{status}");
    back.close().await;

    // Stopped as an operator would (Ctrl-C): the match stays a part, for the next one again.
    let _ = Command::new("kill").args(["-INT", &second.id().to_string()]).status();
    let stopped = (0..50).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        second.try_wait().ok().flatten().is_some()
    });
    if !stopped {
        second.kill().unwrap();
    }
    second.wait().unwrap();
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("a match taken up where the last server left it"), "{text}");

    // One record: every tick from the first, every hash right, the seat left and taken back.
    let bytes = std::fs::read(dir.join(format!("{id}.elr.part"))).unwrap();
    let mut r = Referee::new(el_sim::map::Map::leaked());
    let mut frames = Frames::new(&bytes);
    let mut seats = Vec::new();
    for f in frames.by_ref() {
        if let Frame::Seat { tick, seat: s, joined } = f
            && usize::from(s) == seat
        {
            seats.push((tick, joined));
        }
        r.take(&f).expect("the record re-steps across the crash");
    }
    r.flush();
    assert_eq!(frames.error, None);
    assert!(u64::from(r.m.tick) > tick_before, "it went on after the crash: {} ticks", r.m.tick);
    let joins: Vec<bool> = seats.iter().map(|(_, j)| *j).collect();
    assert_eq!(joins, [true, false, true], "seated, gone with the server, back: {seats:?}");
    println!("{} ticks, {} hashes checked, seat {seat}: {seats:?}", r.m.tick, r.checked);
    // And its checkpoint takes a referee to the same end.
    let ck = std::fs::read(dir.join(format!("{id}.ck"))).unwrap();
    let mut from = Referee::new(el_sim::map::Map::leaked());
    from.resume(&ck).unwrap();
    for f in Frames::new(&bytes) {
        from.take(&f).unwrap();
    }
    from.flush();
    assert_eq!((from.m.tick, from.m.hash()), (r.m.tick, r.m.hash()));
    let _ = std::fs::remove_dir_all(&dir);
}
