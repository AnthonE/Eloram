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

/// How long a self-signed certificate is used before a new one is made: well inside its 14 days,
/// so a page that pinned it a while ago still connects. A running server remakes its own on the
/// same clock ([`rotate`]): browsers refuse a pinned certificate past its 14 days, and a server
/// up for a fortnight took no new players before.
pub const KEEP_CERT: Duration = Duration::from_secs(10 * 24 * 3_600);
/// How often a running server looks at its certificate ([`rotate`]).
pub const CHECK_CERT: Duration = Duration::from_secs(3_600);

/// How old the kept self-signed certificate in `dir` is, if there is one.
fn kept_age(dir: &std::path::Path) -> Option<Duration> {
    std::fs::metadata(dir.join("self-signed.pem")).and_then(|m| m.modified()).ok()?.elapsed().ok()
}

/// A self-signed identity kept in `dir` (`self-signed.pem`, `self-signed.key`) while it's younger
/// than `keep`, so a restarted server has the hash its players' pages pinned; a new one otherwise
/// (and without a directory).
pub async fn kept_self_signed_identity(
    dir: Option<&std::path::Path>,
    keep: Duration,
) -> anyhow::Result<(Identity, [u8; 32])> {
    let Some(dir) = dir else { return self_signed_identity() };
    let (cert, key) = (dir.join("self-signed.pem"), dir.join("self-signed.key"));
    let young = kept_age(dir).is_some_and(|age| age < keep);
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

/// The endpoint's configuration: its bind, its identity, its keep-alive and idle timeout.
fn server_config(bind: IpBindConfig, port: u16, identity: Identity) -> anyhow::Result<ServerConfig> {
    Ok(ServerConfig::builder()
        .with_bind_config(bind, port)
        .with_identity(identity)
        .keep_alive_interval(Some(Duration::from_secs(3)))
        .max_idle_timeout(Some(Duration::from_secs(10)))?
        .build())
}

/// Binds dual-stack when the host has IPv6, IPv4-only otherwise (e.g. containers without IPv6).
/// Returns the endpoint and the bind it took, for a certificate reloaded later.
pub fn make_endpoint(port: u16, identity: Identity) -> anyhow::Result<(Endpoint<Server>, IpBindConfig)> {
    match Endpoint::server(server_config(IpBindConfig::InAddrAnyDual, port, identity.clone_identity())?) {
        Ok(endpoint) => Ok((endpoint, IpBindConfig::InAddrAnyDual)),
        Err(e) => {
            tracing::info!("dual-stack bind failed ({e}); using IPv4 only");
            Ok((
                Endpoint::server(server_config(IpBindConfig::InAddrAnyV4, port, identity)?)?,
                IpBindConfig::InAddrAnyV4,
            ))
        }
    }
}

/// Where a server's identity comes from, for a certificate remade or renewed while it runs.
#[derive(Clone, Debug)]
pub enum TlsSource {
    /// Self-signed, kept in `dir` when there is one; remade once it's `keep` old.
    SelfSigned { dir: Option<std::path::PathBuf>, keep: Duration },
    /// A CA-issued certificate and key (PEM), read again every check: a renewed file (Let's
    /// Encrypt's, every two months) is taken up without a restart.
    Pem { cert: std::path::PathBuf, key: std::path::PathBuf },
}

/// The certificate's hash as `/cert-hash` serves it (hex; none when it's CA-issued, nothing to pin),
/// shared with the HTTP side, which reads it on every request.
pub type CertHash = Arc<std::sync::RwLock<Option<String>>>;

/// Keeps the endpoint's certificate good while the server runs: every `check`, a self-signed
/// certificate older than its `keep` is remade and kept, and a CA-issued one is read again and
/// taken up if its file changed. Connections already open keep going (the new configuration is
/// for the handshakes after it); a page dials with the hash it fetches fresh each time.
#[allow(clippy::too_many_arguments)]
pub async fn rotate(
    endpoint: Arc<Endpoint<Server>>,
    source: TlsSource,
    bind: IpBindConfig,
    port: u16,
    check: Duration,
    mut current: [u8; 32],
    hash: CertHash,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    // Without a directory a self-signed certificate has no file to date: its age is counted here.
    let mut made = std::time::Instant::now();
    loop {
        tokio::select! {
            _ = tokio::time::sleep(check) => {}
            _ = shutdown.changed() => return,
        }
        let fresh = match &source {
            TlsSource::SelfSigned { dir, keep } => {
                let age = dir.as_deref().and_then(kept_age).unwrap_or_else(|| made.elapsed());
                if age < *keep {
                    continue;
                }
                match kept_self_signed_identity(dir.as_deref(), *keep).await {
                    Ok(id) => id,
                    Err(e) => {
                        tracing::warn!("remaking the self-signed certificate: {e}");
                        continue;
                    }
                }
            }
            TlsSource::Pem { cert, key } => match pem_identity(cert, key).await {
                Ok((id, h)) if h != current => (id, h),
                Ok(_) => continue,
                Err(e) => {
                    tracing::warn!("reading the certificate again: {e}");
                    continue;
                }
            },
        };
        let (identity, new_hash) = fresh;
        let config = match server_config(bind, port, identity) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("the new certificate's configuration: {e}");
                continue;
            }
        };
        if let Err(e) = endpoint.reload_config(config, false) {
            tracing::warn!("taking up the new certificate: {e}");
            continue;
        }
        current = new_hash;
        made = std::time::Instant::now();
        if let Ok(mut h) = hash.write()
            && h.is_some()
        {
            *h = Some(hex::encode(new_hash));
        }
        tracing::info!(
            "the certificate was {}",
            match source {
                TlsSource::SelfSigned { .. } => "remade (self-signed)",
                TlsSource::Pem { .. } => "read again and taken up",
            }
        );
    }
}

/// Takes connections as `admission` allows (`admit`), each to its session, until shutdown.
pub async fn accept_loop(
    endpoint: Arc<Endpoint<Server>>,
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
