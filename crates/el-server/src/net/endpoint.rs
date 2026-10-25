//! WebTransport endpoint setup and the accept loop.

use std::sync::Arc;
use std::time::Duration;

use wtransport::config::IpBindConfig;
use wtransport::endpoint::endpoint_side::Server;
use wtransport::{Endpoint, Identity, ServerConfig};

use super::NetStats;
use super::admit::{Admission, HANDSHAKE, Refusal, Verdict};
use crate::Mode;

/// Path clients must request; anything else gets a 404.
pub const WT_PATH: &str = "/el";

/// A CA-issued identity (e.g. Let's Encrypt's `fullchain.pem` and `privkey.pem`): browsers
/// validate it themselves, so nothing is pinned. Returns the identity and its leaf's SHA-256.
pub async fn pem_identity(
    cert: &std::path::Path,
    key: &std::path::Path,
) -> anyhow::Result<(Identity, [u8; 32])> {
    let identity = Identity::load_pemfiles(cert, key)
        .await
        .map_err(|e| anyhow::anyhow!("loading {} / {}: {e}", cert.display(), key.display()))?;
    let leaf = identity
        .certificate_chain()
        .as_slice()
        .first()
        .ok_or_else(|| anyhow::anyhow!("{}: no certificate", cert.display()))?;
    let hash = *leaf.hash().as_ref();
    Ok((identity, hash))
}

/// Self-signed ECDSA P-256 identity valid for 14 days: the constraints browsers impose on
/// `serverCertificateHashes`. Returns the identity and the certificate's SHA-256.
pub fn self_signed_identity() -> anyhow::Result<(Identity, [u8; 32])> {
    let identity = Identity::self_signed(["localhost", "127.0.0.1", "::1"])?;
    let hash = *identity.certificate_chain().as_slice()[0].hash().as_ref();
    Ok((identity, hash))
}

/// Binds dual-stack when the host has IPv6, IPv4-only otherwise (e.g. containers without IPv6).
pub fn make_endpoint(port: u16, identity: Identity) -> anyhow::Result<Endpoint<Server>> {
    let build = |bind: IpBindConfig, identity: Identity| -> anyhow::Result<ServerConfig> {
        Ok(ServerConfig::builder()
            .with_bind_config(bind, port)
            .with_identity(identity)
            .keep_alive_interval(Some(Duration::from_secs(3)))
            .max_idle_timeout(Some(Duration::from_secs(10)))?
            .build())
    };
    match Endpoint::server(build(IpBindConfig::InAddrAnyDual, identity.clone_identity())?) {
        Ok(endpoint) => Ok(endpoint),
        Err(e) => {
            tracing::info!("dual-stack bind failed ({e}); using IPv4 only");
            Ok(Endpoint::server(build(IpBindConfig::InAddrAnyV4, identity)?)?)
        }
    }
}

/// Takes connections as `admission` allows (`admit`), each to its session, until shutdown.
pub async fn accept_loop(
    endpoint: Endpoint<Server>,
    mode: Mode,
    stats: Arc<NetStats>,
    admission: Arc<Admission>,
    lobby: Option<Arc<crate::lobby::Lobby>>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        let incoming = tokio::select! {
            incoming = endpoint.accept() => incoming,
            _ = shutdown.changed() => break,
        };
        let from = incoming.remote_address().ip();
        let validated = incoming.remote_address_validated();
        let mut pass = match admission.admit(from, validated) {
            Verdict::Admit(pass) => pass,
            Verdict::Retry => {
                NetStats::add(&stats.retried, 1);
                incoming.retry();
                continue;
            }
            Verdict::Refuse(why) => {
                NetStats::add(
                    match why {
                        Refusal::Full => &stats.refused_full,
                        Refusal::Address => &stats.refused_address,
                    },
                    1,
                );
                incoming.refuse();
                continue;
            }
        };
        let stats = stats.clone();
        let lobby = lobby.clone();
        tokio::spawn(async move {
            // Both handshakes within their deadline, or the place goes to someone else.
            let request = match tokio::time::timeout(HANDSHAKE, incoming).await {
                Ok(Ok(request)) => request,
                Ok(Err(e)) => {
                    tracing::debug!("handshake failed: {e}");
                    return;
                }
                Err(_) => {
                    NetStats::add(&stats.handshake_timeouts, 1);
                    return;
                }
            };
            if request.path() != WT_PATH {
                request.not_found().await;
                return;
            }
            let conn = match tokio::time::timeout(HANDSHAKE, request.accept()).await {
                Ok(Ok(conn)) => conn,
                Ok(Err(e)) => {
                    tracing::debug!("session accept failed: {e}");
                    return;
                }
                Err(_) => {
                    NetStats::add(&stats.handshake_timeouts, 1);
                    return;
                }
            };
            pass.established();
            NetStats::add(&stats.sessions_total, 1);
            NetStats::add(&stats.sessions_active, 1);
            match (mode, lobby) {
                (Mode::Game, Some(lobby)) => super::session::run(conn, stats.clone(), lobby).await,
                _ => super::echo::run(conn, stats.clone()).await,
            }
            stats.sessions_active.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            drop(pass);
        });
    }
    endpoint.close(0u32.into(), b"server shutting down");
}
