//! Eloram's agents. They play through the same WebTransport protocol and client core
//! (`el-client-core`) as the browser, so they get the same rules, the same input rate and the same
//! team view as people. They say they're agents in the handshake, and the roster shows them so.
//!
//! ```no_run
//! # async fn demo() -> anyhow::Result<()> {
//! use el_bot::{BotClient, BotConfig};
//!
//! let cfg = BotConfig { server: "http://127.0.0.1:8080".into(), name: "Agent-01".into(), key: None };
//! let mut bot = BotClient::connect(&cfg).await?;
//! bot.queue().await?;
//! bot.wait_match(std::time::Duration::from_secs(60)).await?;
//! bot.play(std::time::Duration::from_secs(600), &mut el_bot::scripted()).await?;
//! bot.close().await;
//! # Ok(()) }
//! ```

pub mod client;
pub mod transport;

pub use client::{BotClient, BotConfig, Brain};
pub use transport::{EndpointInfo, connect, discover, get_json, install_crypto_provider};

/// The scripted bot (`el_sim::bot`), playing from what its client is sent.
pub fn scripted() -> impl FnMut(&el_sim::Match, usize, u32, &mut el_sim::SeatInput) + Send {
    |m, seat, _, out| el_sim::bot::think(m, seat, out)
}
