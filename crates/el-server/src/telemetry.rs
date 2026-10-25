/// Installs a `tracing` subscriber that honours `RUST_LOG` (default `info`).
pub fn init() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).with_target(false).try_init();
}
