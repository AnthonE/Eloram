use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;
use el_server::{Config, Mode};

/// Counts heap operations; the shard threads mark their ticks as hot regions, so any allocation
/// inside a tick shows up as `hot_path_allocations` in /status.
#[global_allocator]
static ALLOC: el_alloc::CountingAlloc = el_alloc::CountingAlloc;

/// Eloram game server.
#[derive(Parser, Debug)]
#[command(name = "el-server", version, about)]
struct Args {
    /// `game` runs the lobby and the matches; `echo` bounces datagrams back (transport smoke test).
    #[arg(long, value_enum, default_value_t = Mode::Game)]
    mode: Mode,
    /// UDP port for WebTransport.
    #[arg(long, default_value_t = 4433)]
    wt_port: u16,
    /// HTTP address (static client, /cert-hash, /status, /status.json).
    #[arg(long, default_value = "127.0.0.1:8080")]
    http: SocketAddr,
    /// Built web client directory to serve at `/`.
    #[arg(long, default_value = "web/dist")]
    web_dir: PathBuf,
    /// PEM certificate chain for WebTransport (e.g. Let's Encrypt's fullchain.pem). Without it the
    /// server self-signs and browsers pin the certificate's hash.
    #[arg(long, requires = "key")]
    cert: Option<PathBuf>,
    /// PEM private key for `--cert`.
    #[arg(long, requires = "cert")]
    key: Option<PathBuf>,
    /// Origins whose pages may read /status, /status.json and /cert-hash from another site (CORS).
    /// Repeat it or separate with commas.
    #[arg(long, default_value = el_server::STORE_ORIGIN)]
    cors_origin: Vec<String>,
    /// Connections taken in all, handshaking or open.
    #[arg(long, default_value_t = el_server::net::admit::Limits::default().connections)]
    max_connections: u32,
    /// Connections taken from one address (loopback excepted).
    #[arg(long, default_value_t = el_server::net::admit::Limits::default().per_address)]
    per_address: u32,
}

fn main() -> anyhow::Result<()> {
    el_server::telemetry::init();
    let args = Args::parse();
    let web_dir = args.web_dir.is_dir().then_some(args.web_dir.clone());
    if web_dir.is_none() {
        tracing::warn!(
            "{} not found: serving no web client (run scripts/build-web.sh)",
            args.web_dir.display()
        );
    }
    let cfg = Config {
        mode: args.mode,
        wt_port: args.wt_port,
        http_addr: args.http,
        web_dir,
        tls: args.cert.zip(args.key),
        cors_origins: el_server::split_list(&args.cors_origin),
        limits: el_server::net::admit::Limits {
            connections: args.max_connections.max(1),
            per_address: args.per_address.max(1),
            ..el_server::net::admit::Limits::default()
        },
        ..Config::default()
    };
    // Two workers are plenty: the matches run on their own shard threads.
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
    rt.block_on(async move {
        let handle = el_server::start(cfg).await?;
        tracing::info!("open http://{} (WebTransport on UDP {})", handle.http_addr, handle.wt_port);
        tokio::signal::ctrl_c().await?;
        tracing::info!("shutting down");
        handle.shutdown();
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_certificate_comes_with_its_key() {
        assert!(Args::try_parse_from(["el-server", "--cert", "c.pem"]).is_err());
        assert!(Args::try_parse_from(["el-server", "--key", "k.pem"]).is_err());
        let a = Args::parse_from(["el-server", "--cert", "c.pem", "--key", "k.pem"]);
        assert_eq!(a.cert.zip(a.key), Some((PathBuf::from("c.pem"), PathBuf::from("k.pem"))));
    }

    #[test]
    fn cors_origins_take_commas_or_repeats() {
        let a = Args::parse_from(["el-server", "--cors-origin", "https://a.example,https://b.example"]);
        assert_eq!(el_server::split_list(&a.cors_origin), ["https://a.example", "https://b.example"]);
        let a = Args::parse_from(["el-server"]);
        assert_eq!(a.cors_origin, [el_server::STORE_ORIGIN]);
    }
}
