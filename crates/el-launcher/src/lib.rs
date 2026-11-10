//! The Elo Pros launcher, as Eloram's desktop client reaches it (after Gates' `elo.rs`,
//! `AnthonE/Gates`, MIT, MoreRight DAO): who is playing, a sign-in proof with no key in the game,
//! where the title's manifest and server list are, and pages opened in the player's browser.
//!
//! No launcher is a normal state: the game plays as a guest, and never asks for a key.

#[allow(clippy::all, clippy::pedantic)]
#[path = "elo_overlay.rs"]
pub mod overlay;

use overlay::{Overlay, SignError};

/// The title's slug on Elo Pros.
pub const SLUG: &str = "eloram";

/// The vendored SDK file's sha256 (`scry-forge/sdk/SHA256SUMS`, its first line).
pub const VENDORED_SHA256: &str =
    "a39278c38b557d7d65eb328101f67d4da44011f8c35de8365cc0b118a56a69d3";

/// A connection to the running launcher.
pub struct Elo {
    overlay: Overlay,
}

/// A proof the launcher signed: the wallet, and its 65-byte signature over the server's text.
#[derive(Clone, Debug)]
pub struct Proved {
    pub address: String,
    pub signature: [u8; 65],
}

fn hex65(s: &str) -> Option<[u8; 65]> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.len() != 130 {
        return None;
    }
    let mut out = [0u8; 65];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

impl Elo {
    /// The launcher running for this player, if there is one (its socket from
    /// `ELO_LAUNCHER_SOCKET`, else the platform's default place).
    pub fn connect(version: &str) -> Result<Elo, String> {
        Overlay::connect(SLUG, version).map(|overlay| Elo { overlay })
    }

    /// How the launcher signs (`local`, `browser`, `none`...).
    pub fn signer(&self) -> String {
        self.overlay.signer()
    }

    /// The wallet it signs for, if it is signed in.
    pub fn address(&mut self) -> Option<String> {
        self.overlay.address()
    }

    /// Signs the server's sign-in text for `domain` with `nonce` (hex), dated by the server's own
    /// `issued_at` (the server rebuilds the text with it, so the launcher's clock must not be).
    /// It can wait minutes for the player: never call it from a frame.
    pub fn prove_at(
        &mut self,
        domain: &str,
        nonce_hex: &str,
        issued_at: u64,
    ) -> Result<Proved, String> {
        let at = i64::try_from(issued_at).map_err(|_| "a date past 2262".to_string())?;
        let p = self
            .overlay
            .prove_at(domain, nonce_hex, Some(at))
            .map_err(|e| match e {
                SignError::NoLauncher(why) => format!("the launcher isn't answering: {why}"),
                SignError::Refused(why) if why.is_empty() => "the launcher said no".to_string(),
                SignError::Refused(why) => format!("the launcher said no: {why}"),
                other => other.to_string(),
            })?;
        let address = p.address.ok_or("the launcher signed for no wallet")?;
        // Exactly 65 bytes, never padded (a short signature is a refusal, not a guess).
        let signature = hex65(&p.signature).ok_or("the launcher's signature isn't 65 bytes")?;
        Ok(Proved { address, signature })
    }

    /// Signs `text` (an Elo Pros action's, which the launcher shows the player), as `0x` hex.
    pub fn sign(&mut self, text: &str, why: &str) -> Result<String, String> {
        self.overlay
            .sign(text, why)
            .map(|s| s.signature)
            .map_err(|e| e.to_string())
    }

    /// Where the title's manifest is (`/api/launcher/manifest/eloram` on the launcher's origin).
    pub fn title_url(&mut self) -> Option<String> {
        self.overlay.title(SLUG)
    }

    /// Where the title's server list is.
    pub fn servers_url(&mut self) -> Option<String> {
        self.overlay.servers_url(SLUG)
    }

    /// Opens `url` in the player's browser; false if the launcher said no.
    pub fn open(&mut self, url: &str) -> bool {
        self.overlay.open_url(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vendored file is the platform's, byte for byte: a fix goes upstream and is copied back
    /// (`scripts/elo-drift.sh` compares the copy with the platform's `sdk/SHA256SUMS`).
    #[test]
    fn vendored_sdk_has_not_been_edited_here() {
        let bytes = include_bytes!("elo_overlay.rs");
        let d = ring::digest::digest(&ring::digest::SHA256, bytes);
        let hex: String = d.as_ref().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, VENDORED_SHA256);
    }

    /// No launcher is a normal state: connecting to a socket nobody serves is an error, not a hang.
    #[test]
    fn no_launcher_is_a_normal_state() {
        let path = std::env::temp_dir().join("el-launcher-test-nobody-here.sock");
        let _ = std::fs::remove_file(&path);
        assert!(overlay::Overlay::connect_at(
            &path,
            SLUG,
            "test",
            std::time::Duration::from_secs(1)
        )
        .is_err());
    }

    #[test]
    fn a_signature_is_exactly_65_bytes() {
        assert!(hex65(&format!("0x{}", "ab".repeat(65))).is_some());
        assert!(hex65(&"ab".repeat(64)).is_none());
        assert!(hex65(&format!("{}zz", "ab".repeat(64))).is_none());
    }
}
