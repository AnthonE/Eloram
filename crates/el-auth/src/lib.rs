//! Wallet sign-in (Sign-In with Ethereum, EIP-4361): the message a player's wallet signs, and the
//! check the server makes. Before Colony's `bc-auth` (itself from Gates), naming this game: the
//! text is the Elo Pros launcher's `prove` text, so one sign-in serves whichever door signs it.
//!
//! **One implementation of the message, used by both sides.** The client builds the text to hand
//! to the wallet and the server rebuilds it from its own nonce, time and domain to check the
//! signature; the client never sends the text, so a signature over anything else can't pass.
//!
//! `no_std`, no allocation: the message is written into a fixed buffer. `verify` (the server's)
//! needs the `verify` feature (k256); `sign` (tests and agents) needs the `sign` feature.

#![no_std]

use el_proto::auth::{Address, NONCE_BYTES, Signature};
use tiny_keccak::{Hasher, Keccak};

/// The longest message [`siwe_message`] writes.
pub const SIWE_MAX: usize = 512;
/// The chain named in the message: Robinhood Chain, where Elo Pros (the store that lists the game)
/// lives. Signing moves nothing on it.
pub const CHAIN_ID: u64 = 4663;
/// The game's name on Elo Pros, as the message names it.
pub const GAME: &str = "eloram";

/// keccak-256.
pub fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut k = Keccak::v256();
    let mut out = [0u8; 32];
    k.update(data);
    k.finalize(&mut out);
    out
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// `0x` and 40 lowercase hex digits: the address as the server keys players by.
pub fn lower_hex(a: &Address) -> [u8; 42] {
    let mut out = [0u8; 42];
    out[0] = b'0';
    out[1] = b'x';
    for (i, b) in a.0.iter().enumerate() {
        out[2 + i * 2] = HEX[(b >> 4) as usize];
        out[3 + i * 2] = HEX[(b & 0xf) as usize];
    }
    out
}

/// EIP-55: the same text re-cased by a hash of itself (what wallets show, and what the message
/// must carry).
pub fn checksum_hex(a: &Address) -> [u8; 42] {
    let mut out = lower_hex(a);
    let hash = keccak256(&out[2..]);
    for i in 0..40 {
        let nibble = (hash[i / 2] >> if i % 2 == 0 { 4 } else { 0 }) & 0xf;
        if nibble >= 8 {
            out[2 + i] = out[2 + i].to_ascii_uppercase();
        }
    }
    out
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// `0x`-prefixed hex of exactly `N` bytes; anything else is refused (never padded: a padded
/// signature recovers a different address).
fn parse_hex<const N: usize>(s: &str) -> Option<[u8; N]> {
    let b = s.trim().as_bytes();
    if b.len() != 2 + N * 2 || b[0] != b'0' || (b[1] != b'x' && b[1] != b'X') {
        return None;
    }
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = (nibble(b[2 + i * 2])? << 4) | nibble(b[3 + i * 2])?;
    }
    Some(out)
}

/// An address from a wallet (any case).
pub fn parse_address(s: &str) -> Option<Address> {
    parse_hex::<20>(s).map(Address)
}

/// A `personal_sign` signature from a wallet: `0x` and 130 hex digits (r, s, v).
pub fn parse_signature(s: &str) -> Option<Signature> {
    parse_hex::<65>(s).map(Signature)
}

/// What `personal_sign` actually signs: keccak of the EIP-191 envelope around the message.
pub fn personal_sign_digest(message: &[u8]) -> [u8; 32] {
    let mut k = Keccak::v256();
    k.update(b"\x19Ethereum Signed Message:\n");
    let mut digits = [0u8; 20];
    let mut n = 0;
    let mut v = message.len();
    loop {
        digits[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
        if v == 0 {
            break;
        }
    }
    digits[..n].reverse();
    k.update(&digits[..n]);
    k.update(message);
    let mut out = [0u8; 32];
    k.finalize(&mut out);
    out
}

struct Text<'a> {
    out: &'a mut [u8; SIWE_MAX],
    at: usize,
}

impl Text<'_> {
    fn bytes(&mut self, b: &[u8]) {
        let end = (self.at + b.len()).min(SIWE_MAX);
        let n = end - self.at;
        self.out[self.at..end].copy_from_slice(&b[..n]);
        self.at = end;
    }
    fn s(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    fn hex(&mut self, b: &[u8]) {
        for x in b {
            self.bytes(&[HEX[(x >> 4) as usize], HEX[(x & 0xf) as usize]]);
        }
    }
    fn num(&mut self, mut v: u64) {
        let mut buf = [0u8; 20];
        let mut n = 0;
        loop {
            buf[n] = b'0' + (v % 10) as u8;
            v /= 10;
            n += 1;
            if v == 0 {
                break;
            }
        }
        buf[..n].reverse();
        self.bytes(&buf[..n]);
    }
    fn pad2(&mut self, v: u64) {
        self.bytes(&[b'0' + (v / 10 % 10) as u8, b'0' + (v % 10) as u8]);
    }
    /// `YYYY-MM-DDTHH:MM:SSZ` (Howard Hinnant's `civil_from_days`).
    fn iso8601(&mut self, unix_secs: u64) {
        let days = (unix_secs / 86_400) as i64;
        let rem = unix_secs % 86_400;
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u64;
        let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u64;
        let y = (yoe + era * 400 + i64::from(m <= 2)) as u64;
        self.pad2(y / 100);
        self.pad2(y % 100);
        self.s("-");
        self.pad2(m);
        self.s("-");
        self.pad2(d);
        self.s("T");
        self.pad2(rem / 3_600);
        self.s(":");
        self.pad2(rem % 3_600 / 60);
        self.s(":");
        self.pad2(rem % 60);
        self.s("Z");
    }
}

/// Whether `domain` is this machine (the dev server), which is served over plain http.
fn loopback(domain: &str) -> bool {
    let host = domain.rsplit_once(':').map_or(domain, |(h, _)| h);
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
}

/// The launcher's one human sentence (`elo-broker`'s `statement`), for [`GAME`].
const STATEMENT: &str =
    "Prove your identity to play eloram. This signature authorises nothing and moves no funds.";

/// The exact text a player's wallet signs to sign in to `domain` with `address`, for the server's
/// `nonce` at `issued_at` (Unix seconds). Written into `out`; returns its length.
///
/// `domain` is the host (and port) the page was served from: wallets compare it with the page's
/// and warn about a mismatch, and it's what stops a signature collected by one server being used
/// at another.
pub fn siwe_message(
    domain: &str,
    address: &Address,
    nonce: &[u8; NONCE_BYTES],
    issued_at: u64,
    out: &mut [u8; SIWE_MAX],
) -> usize {
    let mut w = Text { out, at: 0 };
    let checksummed = checksum_hex(address);
    w.s(domain);
    w.s(" wants you to sign in with your Ethereum account:\n");
    w.bytes(&checksummed);
    w.s("\n\n");
    w.s(STATEMENT);
    w.s("\n\nURI: ");
    w.s(if loopback(domain) { "http://" } else { "https://" });
    w.s(domain);
    w.s("\nVersion: 1\nChain ID: ");
    w.num(CHAIN_ID);
    w.s("\nNonce: ");
    w.hex(nonce);
    w.s("\nIssued At: ");
    w.iso8601(issued_at);
    w.at
}

/// Why a sign-in failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthError {
    /// `v` isn't 27, 28, 0 or 1.
    BadRecoveryId,
    /// Not a valid secp256k1 signature.
    Malformed,
    /// Valid, but by a different address than the one claimed.
    WrongSigner,
}

/// Checks that `signature` is `address` signing in to `domain` with this nonce and time. The
/// message is rebuilt here, never taken from the client.
#[cfg(feature = "verify")]
pub fn verify(
    domain: &str,
    nonce: &[u8; NONCE_BYTES],
    issued_at: u64,
    address: &Address,
    signature: &Signature,
) -> Result<(), AuthError> {
    use k256::ecdsa::{RecoveryId, Signature as EcdsaSig, VerifyingKey};

    let mut text = [0u8; SIWE_MAX];
    let len = siwe_message(domain, address, nonce, issued_at, &mut text);
    let digest = personal_sign_digest(&text[..len]);
    let raw = &signature.0;
    let rec = match raw[64] {
        27 | 0 => 0u8,
        28 | 1 => 1u8,
        _ => return Err(AuthError::BadRecoveryId),
    };
    let sig = EcdsaSig::from_slice(&raw[..64]).map_err(|_| AuthError::Malformed)?;
    let rec = RecoveryId::from_byte(rec).ok_or(AuthError::BadRecoveryId)?;
    let key = VerifyingKey::recover_from_prehash(&digest, &sig, rec).map_err(|_| AuthError::Malformed)?;
    if address_of(&key) != *address {
        return Err(AuthError::WrongSigner);
    }
    Ok(())
}

/// The address a public key signs as.
#[cfg(any(feature = "verify", feature = "sign"))]
pub fn address_of(key: &k256::ecdsa::VerifyingKey) -> Address {
    let point = key.to_encoded_point(false);
    let h = keccak256(&point.as_bytes()[1..]);
    let mut a = [0u8; 20];
    a.copy_from_slice(&h[12..]);
    Address(a)
}

/// A wallet in memory, for tests and agents: signs the way a browser wallet's `personal_sign`
/// does.
#[cfg(feature = "sign")]
pub struct LocalWallet {
    key: k256::ecdsa::SigningKey,
}

#[cfg(feature = "sign")]
impl LocalWallet {
    /// From a 32-byte secret; `None` if it isn't a valid key.
    pub fn from_secret(secret: &[u8; 32]) -> Option<Self> {
        k256::ecdsa::SigningKey::from_slice(secret).ok().map(|key| Self { key })
    }

    /// Throwaway test key `k` (the secret is the number): what agents in tests sign with.
    pub fn test_key(k: u8) -> Self {
        let mut secret = [0u8; 32];
        secret[31] = k.max(1);
        Self::from_secret(&secret).expect("a small number is a valid key")
    }

    pub fn address(&self) -> Address {
        address_of(self.key.verifying_key())
    }

    /// `personal_sign` over `message`: r, s and v (27 or 28).
    pub fn sign(&self, message: &[u8]) -> Signature {
        let digest = personal_sign_digest(message);
        let mut out = [0u8; 65];
        if let Ok((sig, rec)) = self.key.sign_prehash_recoverable(&digest) {
            out[..64].copy_from_slice(&sig.to_bytes());
            out[64] = 27 + rec.to_byte();
        }
        Signature(out)
    }

    /// Signs in: the SIWE message for this challenge, signed.
    pub fn sign_in(&self, domain: &str, nonce: &[u8; NONCE_BYTES], issued_at: u64) -> Signature {
        let mut text = [0u8; SIWE_MAX];
        let n = siwe_message(domain, &self.address(), nonce, issued_at, &mut text);
        self.sign(&text[..n])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The address of private key 1 (a well-known test vector).
    const KEY1_ADDRESS: &str = "0x7e5f4552091a69125d5dfcb7b8c2659029395bdf";

    #[test]
    fn eip55_vectors() {
        // From the EIP-55 specification.
        for s in [
            "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
            "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
            "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
            "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
        ] {
            let a = parse_address(s).unwrap();
            assert_eq!(core::str::from_utf8(&checksum_hex(&a)).unwrap(), s);
        }
    }

    #[test]
    fn hex_parsing_refuses_rather_than_pads() {
        let a = parse_address(KEY1_ADDRESS).unwrap();
        assert_eq!(&lower_hex(&a), KEY1_ADDRESS.as_bytes());
        assert_eq!(parse_address("0x7E5F4552091A69125d5DfCb7b8C2659029395Bdf"), Some(a));
        assert_eq!(parse_address(&KEY1_ADDRESS[..41]), None);
        assert_eq!(parse_address(&KEY1_ADDRESS[2..]), None);
        assert_eq!(parse_address("0x7e5f4552091a69125d5dfcb7b8c2659029395bdg"), None);
        assert_eq!(parse_signature("0x1234"), None);
    }

    /// The launcher's `prove` text (`elo-broker`'s `prove_message`), for this game.
    #[test]
    fn the_message_is_the_launchers_prove_text() {
        assert_eq!(
            STATEMENT
                .strip_prefix("Prove your identity to play ")
                .and_then(|r| r.strip_suffix(". This signature authorises nothing and moves no funds.")),
            Some(GAME)
        );
        let a = parse_address(KEY1_ADDRESS).unwrap();
        let nonce = core::array::from_fn(|i| i as u8);
        let mut out = [0u8; SIWE_MAX];
        let n = siwe_message("127.0.0.1:8080", &a, &nonce, 1_790_000_000, &mut out);
        let expected = "127.0.0.1:8080 wants you to sign in with your Ethereum account:\n\
0x7E5F4552091A69125d5DfCb7b8C2659029395Bdf\n\n\
Prove your identity to play eloram. This signature authorises nothing and moves no funds.\n\n\
URI: http://127.0.0.1:8080\n\
Version: 1\n\
Chain ID: 4663\n\
Nonce: 000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f\n\
Issued At: 2026-09-21T14:13:20Z";
        assert_eq!(core::str::from_utf8(&out[..n]).unwrap(), expected);
        let n = siwe_message("play.example.com", &a, &nonce, 0, &mut out);
        let text = core::str::from_utf8(&out[..n]).unwrap();
        assert!(text.contains("URI: https://play.example.com\n"));
        assert!(text.ends_with("Issued At: 1970-01-01T00:00:00Z"));
    }

    #[test]
    fn dates_across_leap_days_and_years() {
        let date = |secs: u64| {
            let mut out = [0u8; SIWE_MAX];
            let mut w = Text { out: &mut out, at: 0 };
            w.iso8601(secs);
            let n = w.at;
            let mut s = [0u8; 20];
            s.copy_from_slice(&out[..n.min(20)]);
            s
        };
        assert_eq!(&date(951_782_400), b"2000-02-29T00:00:00Z");
        assert_eq!(&date(1_704_067_199), b"2023-12-31T23:59:59Z");
        assert_eq!(&date(1_704_067_200), b"2024-01-01T00:00:00Z");
    }

    #[cfg(all(feature = "sign", feature = "verify"))]
    mod signed {
        use super::super::*;
        use super::KEY1_ADDRESS;

        #[test]
        fn key_one_has_its_known_address() {
            assert_eq!(&lower_hex(&LocalWallet::test_key(1).address()), KEY1_ADDRESS.as_bytes());
        }

        #[test]
        fn a_signed_in_wallet_verifies_and_nothing_else_does() {
            let w = LocalWallet::test_key(1);
            let nonce = [7u8; NONCE_BYTES];
            let sig = w.sign_in("127.0.0.1:8080", &nonce, 1_790_000_000);
            assert_eq!(verify("127.0.0.1:8080", &nonce, 1_790_000_000, &w.address(), &sig), Ok(()));
            // v as 0/1 works too.
            let mut low = sig;
            low.0[64] -= 27;
            assert_eq!(verify("127.0.0.1:8080", &nonce, 1_790_000_000, &w.address(), &low), Ok(()));
            // Another nonce, time, domain or claimed address: refused.
            assert!(
                verify("127.0.0.1:8080", &[8u8; NONCE_BYTES], 1_790_000_000, &w.address(), &sig).is_err()
            );
            assert!(verify("127.0.0.1:8080", &nonce, 1_790_000_001, &w.address(), &sig).is_err());
            assert!(verify("evil.example", &nonce, 1_790_000_000, &w.address(), &sig).is_err());
            assert_eq!(
                verify("127.0.0.1:8080", &nonce, 1_790_000_000, &LocalWallet::test_key(2).address(), &sig),
                Err(AuthError::WrongSigner)
            );
            let mut bad = sig;
            bad.0[64] = 5;
            assert_eq!(
                verify("127.0.0.1:8080", &nonce, 1_790_000_000, &w.address(), &bad),
                Err(AuthError::BadRecoveryId)
            );
        }

        #[test]
        fn garbage_never_panics() {
            let w = LocalWallet::test_key(3);
            let mut x = 0x1234_5678u32;
            for _ in 0..300 {
                let mut sig = [0u8; 65];
                for b in sig.iter_mut() {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    *b = x as u8;
                }
                let _ = verify("127.0.0.1:8080", &[1; NONCE_BYTES], 5, &w.address(), &Signature(sig));
            }
        }
    }
}
