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

/// How long a kept self-signed certificate is used before a new one is made: well inside its 14
/// days, so a page that pinned it a while ago still connects.
const KEEP_CERT: Duration = Duration::from_secs(10 * 24 * 3_600);

/// A self-signed identity kept in `dir` (`self-signed.pem`, `self-signed.key`) while it's young,
/// so a restarted server has the hash its players' pages pinned; a new one otherwise (and without
/// a directory).
pub async fn kept_self_signed_identity(
    dir: Option<&std::path::Path>,
) -> anyhow::Result<(Identity, [u8; 32])> {
    let Some(dir) = dir else { return self_signed_identity() };
    let (cert, key) = (dir.join("self-signed.pem"), dir.join("self-signed.key"));
    let young = std::fs::metadata(&cert)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < KEEP_CERT);
    if young && let Ok(kept) = pem_identity(&cert, &key).await {
        return Ok(kept);
    }
    let (identity, hash) = self_signed_identity()?;
    let stored = async {
        // The key's file is the owner's alone before anything is written to it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&key)?;
            std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600))?;
        }
        identity.private_key().store_secret_pemfile(&key).await?;
        identity.certificate_chain().store_pemfile(&cert).await
    };
    if let Err(e) = stored.await {
        tracing::warn!("keeping the self-signed certificate in {}: {e}", dir.display());
    }
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
    signin: Arc<crate::signin::SignIn>,
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
        let signin = signin.clone();
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
            // The page's origin: a sign-in's challenge names its domain.
            let origin = request.origin().map(str::to_string);
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
                (Mode::Game, Some(lobby)) => {
                    super::session::run(conn, stats.clone(), lobby, signin, origin).await
                }
                _ => super::echo::run(conn, stats.clone()).await,
            }
            stats.sessions_active.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            drop(pass);
        });
    }
    endpoint.close(0u32.into(), b"server shutting down");
}
