//! The Elo Pros module against a stand-in for the store's meter (an axum server here that checks
//! every call's signature as `meter/serverkeys.py` does): the key checked at startup, ten wallets
//! queued RANKED and seated together by the store's match with no bots (a guest refused), and a
//! finished match's reports kept until they're taken, sent with the record's hash, and a refusal
//! put aside.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::IntoResponse;
use el_bot::{BotClient, BotConfig};
use el_server::elopros::report::{MatchDone, SeatDone};
use el_server::elopros::{EloConfig, EloPros, key, outbox};
use serde_json::{Value, json};

const MATCH: &str = "q-test";

#[derive(Default)]
struct Meter {
    /// The public key the calls must be signed by.
    public: Vec<u8>,
    /// What this server is listed at.
    addr: String,
    /// Every call: its route and body.
    calls: Mutex<Vec<(String, Value)>>,
    bad_signatures: Mutex<u32>,
    queued: Mutex<Vec<String>>,
    matched: Mutex<Option<Value>>,
    /// The status the reports get.
    report_status: Mutex<u16>,
}

fn header<'a>(h: &'a HeaderMap, k: &str) -> &'a str {
    h.get(k).and_then(|v| v.to_str().ok()).unwrap_or("")
}

async fn meter(
    State(m): State<Arc<Meter>>,
    method: Method,
    uri: Uri,
    h: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let path = uri.path().strip_prefix("/api").unwrap_or(uri.path()).to_string();
    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if method == Method::POST {
        let nonce = header(&h, "x-elo-nonce");
        let text = key::server_text(
            "POST",
            &path,
            header(&h, "x-elo-server"),
            header(&h, "x-elo-time"),
            &body,
            (!nonce.is_empty()).then_some(nonce),
        );
        use base64::Engine as _;
        let sig = base64::engine::general_purpose::STANDARD
            .decode(header(&h, "x-elo-signature"))
            .unwrap_or_default();
        let ok = ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &m.public)
            .verify(text.as_bytes(), &sig)
            .is_ok();
        if !ok || header(&h, "x-elo-server") != "eloram/game-1" {
            *m.bad_signatures.lock().unwrap() += 1;
            return (StatusCode::UNAUTHORIZED, axum::Json(json!({"error": "bad signature"})));
        }
    }
    m.calls.lock().unwrap().push((path.clone(), v.clone()));
    let state = |p: &str| -> Value {
        let matched = m.matched.lock().unwrap();
        let in_match = matched.as_ref().is_some_and(|mm| {
            mm["sides"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["players"].as_array().unwrap().iter().any(|x| x == p))
        });
        if in_match {
            json!({"player": p, "state": "launched", "match": MATCH, "server": m.addr})
        } else if m.queued.lock().unwrap().iter().any(|q| q == p) {
            json!({"player": p, "state": "queued", "waited_s": 2})
        } else {
            json!({"player": p, "state": null, "unqueued_why": "not queued"})
        }
    };
    let ok = |b: Value| (StatusCode::OK, axum::Json(b));
    match path.as_str() {
        "/serverkeys/check" => ok(json!({"ok": true})),
        "/lobbies/serve/eloram" => {
            let players: Vec<String> =
                v["players"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect();
            match v["op"].as_str() {
                Some("queue") => {
                    assert_eq!(
                        (v["ladder"].as_str(), v["teams"].as_u64(), v["size"].as_u64()),
                        (Some("ranked"), Some(2), Some(5))
                    );
                    // Queued again: out of the match they were in (a select called off).
                    let mut matched = m.matched.lock().unwrap();
                    if matched
                        .as_ref()
                        .is_some_and(|mm| players.iter().any(|p| mm.to_string().contains(p.as_str())))
                    {
                        *matched = None;
                    }
                    drop(matched);
                    let mut q = m.queued.lock().unwrap();
                    q.extend(players.iter().cloned());
                    if q.len() == 10 {
                        let sides = json!([{"players": q[..5]}, {"players": q[5..]}]);
                        *m.matched.lock().unwrap() = Some(
                            json!({"id": MATCH, "game": "eloram", "ladder": "ranked", "sides": sides, "server": {"addr": m.addr}}),
                        );
                        q.clear();
                    }
                }
                Some("unqueue") => m.queued.lock().unwrap().retain(|q| !players.contains(q)),
                _ => {}
            }
            ok(json!({"ok": true, "players": players.iter().map(|p| state(p)).collect::<Vec<_>>()}))
        }
        "/lobbies/match/q-test" => match m.matched.lock().unwrap().clone() {
            Some(mm) => ok(mm),
            None => (StatusCode::NOT_FOUND, axum::Json(json!({"error": "no match"}))),
        },
        "/ratings/eloram/report" | "/gamestats/eloram/report" => {
            let s = *m.report_status.lock().unwrap();
            (
                StatusCode::from_u16(s).unwrap(),
                axum::Json(json!({"ok": s == 200, "error": "refused by the test"})),
            )
        }
        _ => (StatusCode::NOT_FOUND, axum::Json(json!({"error": "no route"}))),
    }
}

/// A meter, its base URL, and a key it takes (written to `dir`).
async fn start_meter(dir: &std::path::Path, addr: &str) -> (Arc<Meter>, String, std::path::PathBuf) {
    let (pem, public) = key::generate().unwrap();
    let key_file = dir.join("server.pem");
    std::fs::write(&key_file, pem).unwrap();
    use base64::Engine as _;
    let m = Arc::new(Meter {
        public: base64::engine::general_purpose::STANDARD.decode(public).unwrap(),
        addr: addr.to_string(),
        report_status: Mutex::new(200),
        ..Meter::default()
    });
    let app = axum::Router::new().fallback(meter).with_state(m.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (m, base, key_file)
}

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("el-elopros-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn secret(k: u8) -> [u8; 32] {
    let mut s = [0u8; 32];
    s[31] = k;
    s
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ten_wallets_queue_ranked_and_are_seated_together() {
    let dir = scratch("ranked");
    let public_addr = "game.example:61260";
    let (m, base, key_file) = start_meter(&dir, public_addr).await;
    let cfg = el_server::Config {
        wt_port: 0,
        http_addr: "127.0.0.1:0".parse().unwrap(),
        web_dir: None,
        matches_per_shard: 2,
        fill_after: Duration::from_secs(600),
        select_for: Duration::from_secs(60),
        elopros: Some(EloConfig {
            api: base,
            key_file,
            key_id: "game-1".into(),
            public_addr: Some(public_addr.into()),
            records_url: None,
            ranked: true,
        }),
        ..el_server::Config::default()
    };
    let h = el_server::start(cfg).await.expect("the server starts");
    let server = format!("http://{}", h.http_addr);

    // A guest is told ranked is for wallets.
    let mut guest =
        BotClient::connect(&BotConfig { server: server.clone(), name: "Guest".into(), key: None })
            .await
            .unwrap();
    guest.queue_ranked().await.unwrap();
    guest.listen(Duration::from_millis(800)).await.unwrap();
    assert!(guest.refused.as_deref().is_some_and(|w| w.contains("wallets")), "{:?}", guest.refused);
    assert!(matches!(
        guest.about,
        Some(el_client_core::lobby::ToClient::About { ranked: true, player: None, .. })
    ));

    let mut bots = Vec::new();
    for k in 1..=10u8 {
        let cfg = BotConfig { server: server.clone(), name: format!("Wallet-{k}"), key: Some(secret(k)) };
        let mut b = BotClient::connect(&cfg).await.unwrap();
        b.queue_ranked().await.unwrap();
        bots.push(b);
    }
    // Waiting, then matched by the store and seated here: everyone in one champion select.
    let mut seen_waiting = 0;
    for _ in 0..40 {
        for b in &mut bots {
            b.listen(Duration::from_millis(50)).await.unwrap();
        }
        seen_waiting = seen_waiting.max(bots.iter().filter(|b| b.ranked.is_some()).count());
        if bots.iter().all(|b| b.selecting) {
            break;
        }
    }
    assert!(seen_waiting > 0, "the queue's state was told");
    for b in &bots {
        assert!(b.selecting, "{} is in champion select", b.session);
        assert_eq!(b.roster.iter().filter(|p| p.bot).count(), 0, "no bots in a ranked match: {:?}", b.roster);
        assert_eq!(b.roster.len(), 10);
    }
    // The store's sides are the seats: its first five on side 0.
    let sides = m.matched.lock().unwrap().clone().unwrap()["sides"].clone();
    let first = sides[0]["players"].as_array().unwrap().clone();
    for b in &bots {
        let me = b.address.unwrap().to_string();
        let names: Vec<&str> = b.roster.iter().map(|p| p.name.as_str()).collect();
        let my_seat =
            names.iter().position(|n| *n == format!("Wallet-{}", bots_index(&bots, b) + 1)).unwrap();
        assert_eq!(my_seat < 5, first.iter().any(|p| p == &me), "{me} in seat {my_seat}");
    }
    let calls = m.calls.lock().unwrap().clone();
    assert!(calls.iter().any(|(p, _)| p == "/serverkeys/check"), "the key was checked");
    assert_eq!(*m.bad_signatures.lock().unwrap(), 0, "every call signed");
    let status = el_bot::get_json(&server, "/status").await.unwrap();
    assert_eq!(status["game"]["elopros"]["key"], "ok", "{status}");
    assert_eq!(status["game"]["elopros"]["ranked"], true);
    for b in bots {
        b.close().await;
    }
    guest.close().await;
    h.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

fn bots_index(bots: &[BotClient], b: &BotClient) -> usize {
    bots.iter().position(|x| x.session == b.session).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reports_wait_on_disk_until_taken() {
    let dir = scratch("outbox");
    let (m, base, key_file) = start_meter(&dir, "game.example:61260").await;
    let records = dir.join("records");
    std::fs::create_dir_all(&records).unwrap();
    let cfg = EloConfig {
        api: base,
        key_file,
        key_id: "game-1".into(),
        public_addr: None,
        records_url: Some("https://game.example/eloram/records".into()),
        ranked: false,
    };
    let elo = EloPros::new(&cfg, Some(records.clone())).unwrap();
    let done = MatchDone {
        record: "00000000000000ab".into(),
        ranked: Some(MATCH.into()),
        winner: 1,
        practice: false,
        tick: 9_000,
        seats: (0..10)
            .map(|s| SeatDone { player: Some(format!("0x{:040x}", s + 1)), kills: s, ..SeatDone::default() })
            .collect(),
    };
    let record = b"a record's bytes".to_vec();
    std::fs::write(records.join("00000000000000ab.elr"), &record).unwrap();
    let pass = |elo: Arc<EloPros>| async move {
        tokio::task::spawn_blocking(move || outbox::pass(&elo)).await.unwrap()
    };

    // The store is down: both kept, tried later.
    *m.report_status.lock().unwrap() = 503;
    elo.outbox.owe(&done);
    assert_eq!(elo.outbox.waiting().len(), 2);
    pass(elo.clone()).await;
    let left = elo.outbox.waiting();
    assert_eq!(left.len(), 2, "kept through a 503");
    assert!(left.iter().all(|(_, j)| j.tries == 1 && j.next > j.made));

    // A server started again on the same records finds them, and sends them once due.
    let again = EloPros::new(&cfg, Some(records.clone())).unwrap();
    assert_eq!(again.outbox.waiting().len(), 2);
    for (path, mut j) in again.outbox.waiting() {
        j.next = 0;
        std::fs::write(path, serde_json::to_vec(&j).unwrap()).unwrap();
    }
    *m.report_status.lock().unwrap() = 200;
    m.calls.lock().unwrap().clear();
    pass(again.clone()).await;
    assert!(again.outbox.waiting().is_empty(), "taken");
    let calls = m.calls.lock().unwrap().clone();
    let rating = &calls.iter().find(|(p, _)| p == "/ratings/eloram/report").unwrap().1["matches"][0];
    assert_eq!(rating["id"], MATCH);
    assert_eq!(rating["teams"][1]["place"], 1, "side 1 won");
    assert_eq!(rating["evidence"]["replay_sha256"], key::sha256_hex(&record));
    assert_eq!(rating["evidence"]["replay_url"], "https://game.example/eloram/records/00000000000000ab");
    let stats = &calls.iter().find(|(p, _)| p == "/gamestats/eloram/report").unwrap().1;
    assert_eq!(stats["reports"].as_array().unwrap().len(), 10);

    // A refusal is put aside with its reason, not tried for ever.
    *m.report_status.lock().unwrap() = 422;
    again.outbox.owe(&MatchDone { ranked: None, ..done.clone() });
    pass(again.clone()).await;
    assert!(again.outbox.waiting().is_empty());
    let failed: Vec<_> =
        std::fs::read_dir(records.join("elopros/outbox/failed")).unwrap().flatten().collect();
    assert_eq!(failed.len(), 2, "the job and its reason");
    assert_eq!(*m.bad_signatures.lock().unwrap(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A ranked party of two queues as one call and sits on one side; then someone leaving champion
/// select calls the match off, and the other nine are back in the queue (nothing was started).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_ranked_party_and_a_leaver_in_select() {
    let dir = scratch("party");
    let public_addr = "game.example:61260";
    let (m, base, key_file) = start_meter(&dir, public_addr).await;
    let cfg = el_server::Config {
        wt_port: 0,
        http_addr: "127.0.0.1:0".parse().unwrap(),
        web_dir: None,
        matches_per_shard: 2,
        fill_after: Duration::from_secs(600),
        select_for: Duration::from_secs(60),
        elopros: Some(EloConfig {
            api: base,
            key_file,
            key_id: "game-1".into(),
            public_addr: Some(public_addr.into()),
            records_url: None,
            ranked: true,
        }),
        ..el_server::Config::default()
    };
    let h = el_server::start(cfg).await.expect("the server starts");
    let server = format!("http://{}", h.http_addr);
    let mut bots = Vec::new();
    for k in 1..=10u8 {
        let cfg = BotConfig { server: server.clone(), name: format!("Wallet-{k}"), key: Some(secret(k)) };
        bots.push(BotClient::connect(&cfg).await.unwrap());
    }
    // The first asks the second into a party; the second takes it.
    let second = bots[1].address.unwrap().to_string();
    bots[0].party_invite(&second).await.unwrap();
    bots[1].listen(Duration::from_millis(500)).await.unwrap();
    assert_eq!(bots[1].party.2, vec![bots[0].session], "asked");
    let leader = bots[0].session;
    bots[1].party_accept(leader).await.unwrap();
    bots[0].listen(Duration::from_millis(500)).await.unwrap();
    assert_eq!(bots[0].party.0, leader);
    assert_eq!(bots[0].party.1.len(), 2);
    // Only the leader queues for the party.
    bots[1].queue_ranked().await.unwrap();
    bots[1].listen(Duration::from_millis(500)).await.unwrap();
    assert!(bots[1].refused.as_deref().is_some_and(|w| w.contains("leader")), "{:?}", bots[1].refused);
    for b in &mut bots {
        if b.session != bots_session_of(&second, b) {
            b.queue_ranked().await.unwrap();
        }
    }
    for _ in 0..40 {
        for b in &mut bots {
            b.listen(Duration::from_millis(50)).await.unwrap();
        }
        if bots.iter().all(|b| b.selecting) {
            break;
        }
    }
    assert!(bots.iter().all(|b| b.selecting && b.ranked_match), "everyone in a ranked select");
    let calls = m.calls.lock().unwrap().clone();
    let party_call = calls
        .iter()
        .find(|(p, v)| {
            p == "/lobbies/serve/eloram" && v["op"] == "queue" && v["players"].as_array().unwrap().len() == 2
        })
        .expect("the party queued in one call");
    assert_eq!(party_call.1["players"][1], second.as_str());
    let seat_of = |b: &BotClient, name: &str| b.roster.iter().position(|p| p.name == name).unwrap();
    assert_eq!(seat_of(&bots[0], "Wallet-1") / 5, seat_of(&bots[0], "Wallet-2") / 5, "the party on one side");

    // The last one goes: the select is called off, the other nine queue again.
    for b in &mut bots {
        b.ranked = None;
    }
    let gone = bots.pop().unwrap();
    gone.close().await;
    for _ in 0..40 {
        for b in &mut bots {
            b.listen(Duration::from_millis(50)).await.unwrap();
        }
        if bots.iter().all(|b| b.ranked.as_deref() == Some("waiting")) {
            break;
        }
    }
    assert!(
        bots.iter().all(|b| b.ranked.as_deref() == Some("waiting")),
        "the nine are told they're queued again"
    );
    assert_eq!(m.queued.lock().unwrap().len(), 9, "and the store has them");
    let status = el_bot::get_json(&server, "/status").await.unwrap();
    assert_eq!(status["game"]["matches"], 0, "nothing was started: {status}");
    for b in bots {
        b.close().await;
    }
    h.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// The session whose wallet is `addr`, or 0.
fn bots_session_of(addr: &str, b: &BotClient) -> u32 {
    if b.address.map(|a| a.to_string()).as_deref() == Some(addr) { b.session } else { 0 }
}

/// A NORMAL party of two sits on one side, the server's bots in the other seats.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_party_plays_on_one_side() {
    let cfg = el_server::Config {
        wt_port: 0,
        http_addr: "127.0.0.1:0".parse().unwrap(),
        web_dir: None,
        matches_per_shard: 2,
        fill_after: Duration::from_millis(800),
        select_for: Duration::ZERO,
        ..el_server::Config::default()
    };
    let h = el_server::start(cfg).await.expect("the server starts");
    let server = format!("http://{}", h.http_addr);
    let mut a =
        BotClient::connect(&BotConfig { server: server.clone(), name: "A".into(), key: Some(secret(21)) })
            .await
            .unwrap();
    let mut b =
        BotClient::connect(&BotConfig { server: server.clone(), name: "B".into(), key: Some(secret(22)) })
            .await
            .unwrap();
    // A guest can't be asked: nobody can find one.
    let guest =
        BotClient::connect(&BotConfig { server: server.clone(), name: "G".into(), key: None }).await.unwrap();
    a.party_invite("0x0000000000000000000000000000000000000001").await.unwrap();
    a.listen(Duration::from_millis(300)).await.unwrap();
    assert!(a.refused.is_some(), "nobody there");
    a.party_invite(&b.address.unwrap().to_string()).await.unwrap();
    b.listen(Duration::from_millis(400)).await.unwrap();
    b.party_accept(a.session).await.unwrap();
    a.listen(Duration::from_millis(400)).await.unwrap();
    assert_eq!(a.party.1.len(), 2);
    a.queue().await.unwrap();
    a.wait_match(Duration::from_secs(10)).await.unwrap();
    b.wait_match(Duration::from_secs(10)).await.unwrap();
    let (sa, sb) = (a.client.as_ref().unwrap().seat, b.client.as_ref().unwrap().seat);
    assert_eq!(sa / 5, sb / 5, "seats {sa} and {sb}: one side");
    assert_eq!(a.roster.iter().filter(|p| p.bot).count(), 8);
    a.close().await;
    b.close().await;
    guest.close().await;
    h.shutdown();
}
