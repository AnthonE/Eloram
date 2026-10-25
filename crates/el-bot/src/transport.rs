//! The native WebTransport client (wtransport) agents and the tests use: Before Colony's
//! `transport.rs`, finding a dev server's certificate over plain HTTP.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wtransport::config::IpBindConfig;
use wtransport::tls::Sha256Digest;
use wtransport::{ClientConfig, Connection, Endpoint};

/// Installs the rustls ring provider. Safe to call repeatedly.
pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Where to connect, as the server's `/cert-hash` says.
#[derive(Clone, Debug)]
pub struct EndpointInfo {
    pub url: String,
    pub cert_hash: Option<[u8; 32]>,
}

/// GETs `path` from a plain-HTTP server (`http://host:port`) and parses its body as JSON.
pub async fn get_json(http_base: &str, path: &str) -> anyhow::Result<serde_json::Value> {
    let host = http_base.trim_start_matches("http://").trim_end_matches('/');
    let mut s = tokio::time::timeout(Duration::from_secs(5), tokio::net::TcpStream::connect(host)).await??;
    s.write_all(format!("GET {path} HTTP/1.0\r\nHost: {host}\r\n\r\n").as_bytes()).await?;
    let mut text = String::new();
    tokio::time::timeout(Duration::from_secs(5), s.read_to_string(&mut text)).await??;
    let (head, body) = text.split_once("\r\n\r\n").ok_or_else(|| anyhow::anyhow!("{path}: no body"))?;
    anyhow::ensure!(
        head.starts_with("HTTP/1.") && head.contains(" 200 "),
        "{path}: {}",
        head.lines().next().unwrap_or("")
    );
    Ok(serde_json::from_str(body)?)
}

/// Asks a dev server (`http://host:port`) for its WebTransport port and certificate hash.
pub async fn discover(http_base: &str) -> anyhow::Result<EndpointInfo> {
    let doc = get_json(http_base, "/cert-hash").await?;
    let port = doc["port"].as_u64().ok_or_else(|| anyhow::anyhow!("/cert-hash: missing port"))?;
    let path = doc["path"].as_str().unwrap_or("/el");
    let cert_hash = match doc["hash"].as_str() {
        Some(hex_hash) => {
            let mut hash = [0u8; 32];
            hex::decode_to_slice(hex_hash, &mut hash)?;
            Some(hash)
        }
        None => None,
    };
    let host = http_base
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .rsplit_once(':')
        .map_or("127.0.0.1", |(h, _)| h)
        .to_string();
    Ok(EndpointInfo { url: format!("https://{host}:{port}{path}"), cert_hash })
}

/// Opens a WebTransport session. With `cert_hash` the server's self-signed certificate is pinned by
/// hash (exactly what browsers do); without it the system roots are used.
///
/// Binds dual-stack when possible and falls back to IPv4 on hosts without IPv6.
pub async fn connect(url: &str, cert_hash: Option<[u8; 32]>) -> anyhow::Result<Connection> {
    install_crypto_provider();
    let build = |bind: IpBindConfig| -> anyhow::Result<ClientConfig> {
        let builder = ClientConfig::builder().with_bind_config(bind);
        Ok(match cert_hash {
            Some(hash) => builder.with_server_certificate_hashes([Sha256Digest::new(hash)]),
            None => builder.with_native_certs(),
        }
        .keep_alive_interval(Some(Duration::from_secs(3)))
        .max_idle_timeout(Some(Duration::from_secs(10)))?
        .build())
    };
    let endpoint = match Endpoint::client(build(IpBindConfig::InAddrAnyDual)?) {
        Ok(endpoint) => endpoint,
        Err(_) => Endpoint::client(build(IpBindConfig::InAddrAnyV4)?)?,
    };
    Ok(endpoint.connect(url).await?)
}
