//! This server's key with Elo Pros, and the text it signs: a port of the platform SDK's
//! `sdk/rust/src/server.rs` (AnthonE/scry-forge), its vectors copied into `testdata/` and checked
//! here (`scripts/elo-drift.sh` holds the copy to the platform's).
//!
//! The text, as every SDK and the meter build it: `elo server`, the method, the path without
//! `/api` (with `?query`), `<slug>/<key id>`, unix seconds, the nonce, and the body's sha256 hex,
//! one a line, no newline at the end. The four headers carry the signer, the time, the nonce and
//! the signature.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ring::rand::SecureRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};

/// PKCS#8 v1 for an Ed25519 key: this prefix, then the 32-byte seed (what
/// `openssl genpkey -algorithm ed25519` writes, and what the SDK reads).
const PKCS8_ED25519: [u8; 16] =
    [0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20];
const NONCE_ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
/// The title's slug on Elo Pros.
pub const GAME: &str = "eloram";

pub fn sha256_hex(body: &[u8]) -> String {
    hex::encode(ring::digest::digest(&ring::digest::SHA256, body))
}

/// The exact text a game server signs for one request.
pub fn server_text(
    method: &str,
    path: &str,
    who: &str,
    at: &str,
    body: &[u8],
    nonce: Option<&str>,
) -> String {
    let tail = nonce.map(|n| format!("{n}\n")).unwrap_or_default();
    format!("elo server\n{}\n{path}\n{who}\n{at}\n{tail}{}", method.to_ascii_uppercase(), sha256_hex(body))
}

/// A fresh request nonce: 22 of letters, digits, `-` and `_`.
fn new_nonce() -> String {
    let mut buf = [0u8; 22];
    if ring::rand::SystemRandom::new().fill(&mut buf).is_err() {
        return String::new();
    }
    buf.iter().map(|b| NONCE_ABC[usize::from(*b & 63)] as char).collect()
}

/// A key id: lowercase letters, digits and `-`, up to 31.
fn id_ok(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 31
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// One of the game's server keys.
pub struct ServerKey {
    game: String,
    key_id: String,
    pair: Ed25519KeyPair,
}

impl ServerKey {
    pub fn from_seed(game: &str, key_id: &str, seed: &[u8]) -> Result<ServerKey, String> {
        if !id_ok(key_id) {
            return Err("the key id is lowercase letters, digits and '-', up to 31 characters".into());
        }
        let pair = Ed25519KeyPair::from_seed_unchecked(seed)
            .map_err(|_| "an Ed25519 private key is a 32-byte seed".to_string())?;
        Ok(ServerKey { game: game.into(), key_id: key_id.into(), pair })
    }

    /// From a PKCS#8 PEM (`el-server --keygen` writes one; so does `openssl genpkey -algorithm ed25519`).
    pub fn from_pem(game: &str, key_id: &str, pem: &str) -> Result<ServerKey, String> {
        let body: String = pem.lines().filter(|l| !l.starts_with("-----")).map(str::trim).collect();
        let der = STANDARD.decode(body).map_err(|_| "that PEM is not base64")?;
        if der.len() != 48 || der[..16] != PKCS8_ED25519 {
            return Err("that PEM is not an Ed25519 private key".into());
        }
        ServerKey::from_seed(game, key_id, &der[16..])
    }

    /// `<slug>/<key id>`, as a request names its signer.
    pub fn who(&self) -> String {
        format!("{}/{}", self.game, self.key_id)
    }

    /// The public half, standard base64: what `elo.json`'s `server_keys` names.
    pub fn public_b64(&self) -> String {
        STANDARD.encode(self.pair.public_key().as_ref())
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn sign_text(&self, text: &str) -> String {
        STANDARD.encode(self.pair.sign(text.as_bytes()).as_ref())
    }

    /// The four headers for one request; `path` is the route without `/api`.
    pub fn headers(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
        at: u64,
        nonce: Option<&str>,
    ) -> [(&'static str, String); 4] {
        let nonce = nonce.map_or_else(new_nonce, str::to_string);
        let at = at.to_string();
        let text =
            server_text(method, path, &self.who(), &at, body, (!nonce.is_empty()).then_some(nonce.as_str()));
        [
            ("X-Elo-Server", self.who()),
            ("X-Elo-Time", at),
            ("X-Elo-Nonce", nonce),
            ("X-Elo-Signature", self.sign_text(&text)),
        ]
    }
}

/// A new key: the PEM to keep on the server (mode 0600) and its public half.
pub fn generate() -> Result<(String, String), String> {
    let mut seed = [0u8; 32];
    ring::rand::SystemRandom::new().fill(&mut seed).map_err(|_| "no randomness")?;
    let mut der = PKCS8_ED25519.to_vec();
    der.extend_from_slice(&seed);
    let b64 = STANDARD.encode(&der);
    let pem = format!("-----BEGIN PRIVATE KEY-----\n{b64}\n-----END PRIVATE KEY-----\n");
    let public = ServerKey::from_seed(GAME, "k", &seed)?.public_b64();
    Ok((pem, public))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTORS: &str = include_str!("testdata/server_texts.json");

    /// The platform's vectors: each text, and the seed's signature over it, byte for byte.
    #[test]
    fn the_platforms_texts_and_signatures() {
        let v: serde_json::Value = serde_json::from_str(VECTORS).unwrap();
        let seed = hex::decode(v["seed_hex"].as_str().unwrap()).unwrap();
        let pem = format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            v["private_pkcs8_b64"].as_str().unwrap()
        );
        let from_pem = ServerKey::from_pem("my-game", "eu-1", &pem).unwrap();
        assert_eq!(from_pem.public_b64(), v["public_b64"].as_str().unwrap());
        let key = ServerKey::from_seed("my-game", "eu-1", &seed).unwrap();
        for c in v["cases"].as_array().unwrap() {
            let s = |k: &str| c[k].as_str().unwrap();
            let text = server_text(
                s("method"),
                s("path"),
                s("who"),
                s("at"),
                s("body_utf8").as_bytes(),
                c["nonce"].as_str(),
            );
            assert_eq!(text, s("text"));
            assert_eq!(key.sign_text(&text), s("signature_b64"));
            if let Some(n) = c["nonce"].as_str() {
                let h = key.headers(
                    s("method"),
                    s("path"),
                    s("body_utf8").as_bytes(),
                    s("at").parse().unwrap(),
                    Some(n),
                );
                assert_eq!(h[3].1, s("signature_b64"));
                assert_eq!(h[0].1, "my-game/eu-1");
            }
        }
    }

    #[test]
    fn a_generated_key_reads_back() {
        let (pem, public) = generate().unwrap();
        let k = ServerKey::from_pem(GAME, "eu-1", &pem).unwrap();
        assert_eq!(k.public_b64(), public);
        assert!(ServerKey::from_pem(GAME, "Bad Id", &pem).is_err());
        assert!(ServerKey::from_pem(GAME, "eu-1", "-----BEGIN-----\nAAAA\n-----END-----").is_err());
        let h = k.headers("POST", "/serverkeys/check", b"", 1, None);
        assert_eq!(h[2].1.len(), 22);
    }
}
