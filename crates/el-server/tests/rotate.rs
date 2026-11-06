//! A certificate remade while the server runs (`el_server::net::endpoint::rotate`): browsers refuse
//! a pinned self-signed certificate past its 14 days, and a server up for a fortnight took no new
//! players before. Here a server keeps its certificate for a moment, `/cert-hash` says a new one
//! after it, the kept files are the new one's, an agent dials with the new hash and is let in, and
//! one dialling with the old hash isn't.

use std::path::PathBuf;
use std::time::Duration;

use el_server::{Config, Mode};

fn dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("el-rotate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

async fn hash(base: &str) -> String {
    el_bot::get_json(base, "/cert-hash").await.unwrap()["hash"].as_str().unwrap().to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_self_signed_certificate_is_remade_while_the_server_runs() {
    let dir = dir();
    let cfg = Config {
        mode: Mode::Echo,
        wt_port: 0,
        http_addr: "127.0.0.1:0".parse().unwrap(),
        records_dir: Some(dir.clone()),
        cert_keep: Duration::from_millis(600),
        cert_check: Duration::from_millis(100),
        ..Config::default()
    };
    let server = el_server::start(cfg).await.unwrap();
    let base = format!("http://{}", server.http_addr);
    let first = hash(&base).await;
    let kept_first = std::fs::read(dir.join("self-signed.pem")).unwrap();
    let first_info = el_bot::discover(&base).await.unwrap();
    assert_eq!(first_info.cert_hash.map(hex::encode), Some(first.clone()));
    // Within its keep, the same certificate.
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(hash(&base).await, first, "remade before its keep was up");
    // Past it, a new one: said by `/cert-hash`, kept in the files, taken by the endpoint.
    let mut second = first.clone();
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        second = hash(&base).await;
        if second != first {
            break;
        }
    }
    assert_ne!(second, first, "the certificate wasn't remade");
    assert_ne!(
        std::fs::read(dir.join("self-signed.pem")).unwrap(),
        kept_first,
        "the kept file is the old one"
    );
    // An agent with the new hash is let in; one with the old hash is refused.
    let info = el_bot::discover(&base).await.unwrap();
    assert_eq!(info.cert_hash.map(hex::encode), Some(second.clone()));
    let conn = el_bot::connect(&info.url, info.cert_hash).await.expect("the new hash connects");
    drop(conn);
    let old = el_bot::connect(&info.url, first_info.cert_hash).await;
    assert!(old.is_err(), "the old hash still connects");
    server.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_certificate_within_its_keep_is_left_alone() {
    let cfg = Config {
        mode: Mode::Echo,
        wt_port: 0,
        http_addr: "127.0.0.1:0".parse().unwrap(),
        cert_keep: Duration::from_secs(3_600),
        cert_check: Duration::from_millis(50),
        ..Config::default()
    };
    let server = el_server::start(cfg).await.unwrap();
    let base = format!("http://{}", server.http_addr);
    let first = hash(&base).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(hash(&base).await, first);
    server.shutdown();
}
