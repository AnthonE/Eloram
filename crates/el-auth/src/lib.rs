//! Wallet sign-in (Sign-In with Ethereum, EIP-4361): the message a player's wallet signs, and the
//! check the server makes. Before Colony's `bc-auth` (itself from Gates), naming this game: the
//! text is the Elo Pros launcher's `prove` text, so one sign-in serves whichever door signs it.
//!
//! **One implementation of the message, used by both sides.** The client builds the text to hand
//! to the wallet and the server rebuilds it from its own nonce, time and domain to check the
//! signature; the client never sends the text, so a signature over anything else can't pass.
//!
//! A Solana wallet signs the same text in its own form ([`siws_message`]), and a player signed in on
//! Elo Pros brings a pass instead of a signature ([`verify_pass`]): any login, a wallet or none.
//!
//! `no_std`, no allocation: the message is written into a fixed buffer. `verify` (the server's)
//! needs the `verify` feature (k256, ring); `sign` (tests and agents) needs the `sign` feature.

#![no_std]

use el_proto::auth::{Address, NONCE_BYTES, Signature, SolanaSignature};
#[cfg(feature = "verify")]
use el_proto::auth::{PlayerId, SOLANA_KEY_BYTES};
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

/// A Solana wallet's signature, as the page hands it over: `0x` and 128 hex digits.
pub fn parse_solana_signature(s: &str) -> Option<SolanaSignature> {
    parse_hex::<64>(s).map(SolanaSignature)
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

/// The launcher's one human sentence (`elo-broker`'s `statement`), around the game it names.
const STATEMENT_HEAD: &str = "Prove your identity to play ";
const STATEMENT_TAIL: &str = ". This signature authorises nothing and moves no funds.";

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
    let checksummed = checksum_hex(address);
    write_message(GAME, domain, b"Ethereum", &checksummed, Chain::Id(CHAIN_ID), nonce, issued_at, out)
}

/// The text a Solana wallet signs to sign in, its address as written: [`siwe_message`]'s in its
/// Solana form (Elo Pros' `/api/signin/check` reads the same shape, `Chain ID: mainnet`).
pub fn siws_message(
    domain: &str,
    address: &str,
    nonce: &[u8; NONCE_BYTES],
    issued_at: u64,
    out: &mut [u8; SIWE_MAX],
) -> usize {
    write_message(GAME, domain, b"Solana", address.as_bytes(), Chain::Named("mainnet"), nonce, issued_at, out)
}

/// A message's `Chain ID` line.
enum Chain {
    Id(u64),
    Named(&'static str),
}

/// EIP-4361's shape, as the Elo Pros launcher's `prove` writes it (`elo-broker`'s
/// `prove_message`): its URI is always `https://` and the domain, the dev server's on this machine
/// too, so one signature text serves the page and the launcher alike.
#[allow(clippy::too_many_arguments)]
fn write_message(
    game: &str,
    domain: &str,
    family: &[u8],
    address: &[u8],
    chain: Chain,
    nonce: &[u8],
    issued_at: u64,
    out: &mut [u8; SIWE_MAX],
) -> usize {
    let mut w = Text { out, at: 0 };
    w.s(domain);
    w.s(" wants you to sign in with your ");
    w.bytes(family);
    w.s(" account:\n");
    w.bytes(address);
    w.s("\n\n");
    w.s(STATEMENT_HEAD);
    w.s(game);
    w.s(STATEMENT_TAIL);
    w.s("\n\nURI: https://");
    w.s(domain);
    w.s("\nVersion: 1\nChain ID: ");
    match chain {
        Chain::Id(id) => w.num(id),
        Chain::Named(name) => w.s(name),
    }
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

/// Checks that `signature` is the Solana wallet `key` signing in to `domain` with this nonce and
/// time. The message is rebuilt here, as [`verify`]'s is; a Solana wallet signs its bytes as they
/// are, with no envelope.
#[cfg(feature = "verify")]
pub fn verify_solana(
    domain: &str,
    nonce: &[u8; NONCE_BYTES],
    issued_at: u64,
    key: &[u8; SOLANA_KEY_BYTES],
    signature: &SolanaSignature,
) -> Result<(), AuthError> {
    let mut text = [0u8; SIWE_MAX];
    let len = siws_message(domain, PlayerId::solana(key).as_str(), nonce, issued_at, &mut text);
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
        .verify(&text[..len], &signature.0)
        .map_err(|_| AuthError::WrongSigner)
}

/// Who an Elo Pros pass says signed in.
#[cfg(feature = "verify")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Passed {
    /// The account: `acct_` and 20 lowercase hex.
    pub account: PlayerId,
    /// The wallet it signs in with, if the pass names one.
    pub wallet: Option<PlayerId>,
}

#[cfg(feature = "verify")]
impl Passed {
    /// The id the player plays as: the wallet if there is one, else the account (Elo Pros' `player`).
    pub fn player(&self) -> PlayerId {
        self.wallet.unwrap_or(self.account)
    }
}

/// Why a pass was refused.
#[cfg(feature = "verify")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassError {
    Malformed,
    WrongSigner,
    /// For another game, or handed to another host.
    Elsewhere,
    /// A nonce we didn't issue, or already used.
    Replayed,
    Expired,
}

/// base64url with no padding into `out`; the bytes written.
#[cfg(feature = "verify")]
fn base64url<'a>(s: &[u8], out: &'a mut [u8]) -> Option<&'a [u8]> {
    let mut bits = 0u32;
    let mut have = 0;
    let mut n = 0;
    for &c in s {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        bits = bits << 6 | u32::from(v);
        have += 6;
        if have >= 8 {
            have -= 8;
            *out.get_mut(n)? = (bits >> have) as u8;
            n += 1;
        }
    }
    Some(&out[..n])
}

/// The longest pass text and signature decoded, bytes.
#[cfg(feature = "verify")]
const PASS_TEXT_MAX: usize = 768;

/// Checks an Elo Pros pass offline against the account key (`GET /api/account/pubkey`).
///
/// A pass is `b64url(text) "." b64url(ed25519 signature over text)`; the text is exactly seven
/// lines: `elo account`, `id: acct_…`, `for: play:<slug>`, `shard: <the page's host>`, `nonce: <the
/// one this server handed out>`, `wallet: 0x…|<base58>|none`, `exp: <unix>`. `hosts` are the names
/// the page is served from, with no port; `nonce_ok` says whether the pass's nonce is one this
/// server issued and hasn't seen used, and marks it used, last, so a refused pass spends nothing.
#[cfg(feature = "verify")]
pub fn verify_pass<'h>(
    pass: &str,
    key: &[u8; 32],
    slug: &str,
    hosts: impl IntoIterator<Item = &'h str>,
    nonce_ok: impl FnOnce(&str) -> bool,
    now: u64,
) -> Result<Passed, PassError> {
    let (body, sig) = pass.split_once('.').ok_or(PassError::Malformed)?;
    let mut text = [0u8; PASS_TEXT_MAX];
    let text = base64url(body.as_bytes(), &mut text).ok_or(PassError::Malformed)?;
    let mut raw_sig = [0u8; 64];
    let sig = base64url(sig.as_bytes(), &mut raw_sig).ok_or(PassError::Malformed)?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
        .verify(text, sig)
        .map_err(|_| PassError::WrongSigner)?;
    let text = core::str::from_utf8(text).map_err(|_| PassError::Malformed)?;
    let mut lines = text.split('\n');
    if lines.next() != Some("elo account") {
        return Err(PassError::Malformed);
    }
    let mut take = |name: &str| {
        lines
            .next()
            .and_then(|l| l.strip_prefix(name))
            .and_then(|l| l.strip_prefix(": "))
            .ok_or(PassError::Malformed)
    };
    let id = take("id")?;
    let game = take("for")?;
    let shard = take("shard")?;
    let nonce = take("nonce")?;
    let wallet = take("wallet")?;
    let exp = take("exp")?;
    if lines.next().is_some() {
        return Err(PassError::Malformed);
    }
    let account = PlayerId::parse(id).filter(|a| !a.is_wallet()).ok_or(PassError::Malformed)?;
    let wallet = match wallet {
        "none" => None,
        w => Some(PlayerId::parse(w).filter(PlayerId::is_wallet).ok_or(PassError::Malformed)?),
    };
    let exp: u64 = exp.parse().map_err(|_| PassError::Malformed)?;
    if game.strip_prefix("play:") != Some(slug) || !hosts.into_iter().any(|h| h == shard) {
        return Err(PassError::Elsewhere);
    }
    if exp <= now {
        return Err(PassError::Expired);
    }
    if !nonce_ok(nonce) {
        return Err(PassError::Replayed);
    }
    Ok(Passed { account, wallet })
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
        assert_eq!(GAME, "eloram");
        let a = parse_address(KEY1_ADDRESS).unwrap();
        let nonce = core::array::from_fn(|i| i as u8);
        let mut out = [0u8; SIWE_MAX];
        let n = siwe_message("127.0.0.1:8080", &a, &nonce, 1_790_000_000, &mut out);
        let expected = "127.0.0.1:8080 wants you to sign in with your Ethereum account:\n\
0x7E5F4552091A69125d5DfCb7b8C2659029395Bdf\n\n\
Prove your identity to play eloram. This signature authorises nothing and moves no funds.\n\n\
URI: https://127.0.0.1:8080\n\
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

/// Against Elo Pros' own vectors (`sdk/testdata/` there, copied to `tests/testdata/`).
#[cfg(all(test, feature = "verify"))]
mod pass_tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    const VECTOR: &str = include_str!("../tests/testdata/account_token.json");
    const SIGNIN: &str = include_str!("../tests/testdata/signin_check.json");
    const NONCE: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    /// The string after `"key": "`, from where `after` first appears.
    fn field<'a>(json: &'a str, after: &str, key: &str) -> &'a str {
        let tail = &json[json.find(after).unwrap()..];
        let mut quoted = [0u8; 64];
        let (q, k) = quoted.split_at_mut(1);
        q[0] = b'"';
        k[..key.len()].copy_from_slice(key.as_bytes());
        let name = core::str::from_utf8(&quoted[..key.len() + 1]).unwrap();
        let at = tail.find(name).unwrap() + name.len();
        let rest = tail[at..].trim_start_matches("\": \"");
        &rest[..rest.find('"').unwrap()]
    }

    fn seed() -> [u8; 32] {
        let hex = field(VECTOR, "\"seed_hex", "seed_hex");
        let mut out = [0u8; 32];
        for (i, o) in out.iter_mut().enumerate() {
            *o = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        out
    }

    fn pair() -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed_unchecked(&seed()).unwrap()
    }

    fn pubkey() -> [u8; 32] {
        pair().public_key().as_ref().try_into().unwrap()
    }

    fn check(pass: &str, slug: &str, host: &str, now: u64) -> Result<Passed, PassError> {
        verify_pass(pass, &pubkey(), slug, [host], |n| n == NONCE, now)
    }

    fn b64(bytes: &[u8], out: &mut [u8]) -> usize {
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let (mut bits, mut have, mut n) = (0u32, 0, 0);
        for &b in bytes {
            bits = bits << 8 | u32::from(b);
            have += 8;
            while have >= 6 {
                have -= 6;
                out[n] = A[(bits >> have & 63) as usize];
                n += 1;
            }
        }
        if have > 0 {
            out[n] = A[(bits << (6 - have) & 63) as usize];
            n += 1;
        }
        n
    }

    /// A pass over `text`, signed with the vector's key, as Elo Pros would mint it.
    fn with_pass<R>(text: &str, then: impl FnOnce(&str) -> R) -> R {
        let mut out = [0u8; 1024];
        let mut n = b64(text.as_bytes(), &mut out);
        out[n] = b'.';
        n += 1;
        n += b64(pair().sign(text.as_bytes()).as_ref(), &mut out[n..]);
        then(core::str::from_utf8(&out[..n]).unwrap())
    }

    #[test]
    fn elo_pros_vector_checks_out() {
        let with = field(VECTOR, "with_wallet", "pass");
        let p = check(with, "gates", "elopros.com", 1_700_000_000).unwrap();
        assert_eq!(p.account.as_str(), "acct_0123456789abcdef0123");
        assert_eq!(p.player().as_str(), "0x24445efddb08d4938e3e3627042b2cf4063d9092");
        let without = field(VECTOR, "no_wallet", "pass");
        let p = check(without, "gates", "elopros.com", 1_700_000_000).unwrap();
        assert_eq!(p.wallet, None);
        assert_eq!(p.player().as_str(), "acct_0123456789abcdef0123");
    }

    /// A Solana wallet riding a pass, from Elo Pros' own vector: the player is the key as written.
    #[test]
    fn a_solana_wallet_on_elo_pros_pass_vector() {
        let pass = field(VECTOR, "solana_wallet", "pass");
        let p = check(pass, "gates", "elopros.com", 1_700_000_000).unwrap();
        assert_eq!(p.player().as_str(), field(VECTOR, "solana_wallet", "wallet"));
    }

    #[test]
    fn a_base58_wallet_on_a_pass_keeps_its_case() {
        let sol = "GmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB";
        let text = "elo account\nid: acct_0123456789abcdef0123\nfor: play:eloram\nshard: elopros.com\nnonce: 000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f\nwallet: GmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB\nexp: 1800000000";
        with_pass(text, |pass| {
            let p = check(pass, GAME, "elopros.com", 1_700_000_000).unwrap();
            assert_eq!(p.player().as_str(), sol);
        });
        // A wallet line that spells nothing (here, the same address lowercased).
        let bad = "elo account\nid: acct_0123456789abcdef0123\nfor: play:eloram\nshard: elopros.com\nnonce: 000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f\nwallet: gmadrppbc7p5arkv8g3djiwp89vz1jlk23v2gbjuaegb\nexp: 1800000000";
        with_pass(bad, |pass| {
            assert_eq!(check(pass, GAME, "elopros.com", 1_700_000_000), Err(PassError::Malformed));
        });
    }

    #[test]
    fn what_is_refused() {
        let pass = field(VECTOR, "with_wallet", "pass");
        assert_eq!(check(pass, "eloram", "elopros.com", 0), Err(PassError::Elsewhere));
        assert_eq!(check(pass, "gates", "evil.example", 0), Err(PassError::Elsewhere));
        assert_eq!(check(pass, "gates", "elopros.com", 1_800_000_000), Err(PassError::Expired));
        assert_eq!(
            verify_pass(pass, &pubkey(), "gates", ["elopros.com"], |_| false, 0),
            Err(PassError::Replayed)
        );
        assert_eq!(
            verify_pass(pass, &[9; 32], "gates", ["elopros.com"], |_| true, 0),
            Err(PassError::WrongSigner)
        );
        assert_eq!(check("not a pass", "gates", "elopros.com", 0), Err(PassError::Malformed));
        // Good bytes in the wrong shape: a site token, an extra line.
        with_pass("elo account\nid: acct_0123456789abcdef0123\nfor: site\nexp: 1800000000", |p| {
            assert_eq!(check(p, "gates", "elopros.com", 0), Err(PassError::Malformed));
        });
    }

    /// Elo Pros' Solana case: Ed25519 over the text as it stands, with the address's own key.
    #[test]
    fn elo_pros_solana_vector_checks_out() {
        let at = SIGNIN.find("the web SDK's Solana sign-in").unwrap();
        let tail = &SIGNIN[at..];
        let msg = tail.find("\"message\": \"").unwrap() + 12;
        let end = tail[msg..].find('"').unwrap();
        let mut text = [0u8; 512];
        let mut n = 0;
        let raw = &tail.as_bytes()[msg..msg + end];
        let mut i = 0;
        while i < raw.len() {
            if raw[i] == b'\\' && raw[i + 1] == b'n' {
                text[n] = b'\n';
                i += 2;
            } else {
                text[n] = raw[i];
                i += 1;
            }
            n += 1;
        }
        let key =
            PlayerId::parse("GmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB").unwrap().solana_key().unwrap();
        // The signature's bytes, from its base58: a big number, like a key but 64 bytes long.
        let sig_at = tail.find("\"signature\": \"").unwrap() + 14;
        let sig_end = tail[sig_at..].find('"').unwrap();
        let mut sig = [0u8; 64];
        for c in tail[sig_at..sig_at + sig_end].bytes() {
            let alphabet = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
            let mut carry = alphabet.iter().position(|d| *d == c).unwrap() as u32;
            for b in sig.iter_mut().rev() {
                carry += u32::from(*b) * 58;
                *b = carry as u8;
                carry >>= 8;
            }
        }
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &key)
            .verify(&text[..n], &sig)
            .expect("Elo Pros' Solana vector");
    }

    #[test]
    fn a_solana_wallet_signs_in_and_nothing_else_passes() {
        let seed = [7u8; 32];
        let pair = Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        let key: [u8; 32] = pair.public_key().as_ref().try_into().unwrap();
        assert_eq!(PlayerId::solana(&key).as_str(), "GmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB");
        let nonce = [5u8; NONCE_BYTES];
        let mut text = [0u8; SIWE_MAX];
        let n =
            siws_message("elopros.com", PlayerId::solana(&key).as_str(), &nonce, 1_793_016_000, &mut text);
        let message = core::str::from_utf8(&text[..n]).unwrap();
        assert!(message.starts_with(
            "elopros.com wants you to sign in with your Solana account:\nGmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB\n\n"
        ));
        assert!(message.contains("\nChain ID: mainnet\n"));
        let mut sig = [0u8; 64];
        sig.copy_from_slice(pair.sign(&text[..n]).as_ref());
        let sig = SolanaSignature(sig);
        assert_eq!(verify_solana("elopros.com", &nonce, 1_793_016_000, &key, &sig), Ok(()));
        assert_eq!(
            verify_solana("evil.example", &nonce, 1_793_016_000, &key, &sig),
            Err(AuthError::WrongSigner)
        );
        assert_eq!(
            verify_solana("elopros.com", &[6; 32], 1_793_016_000, &key, &sig),
            Err(AuthError::WrongSigner)
        );
        assert_eq!(
            verify_solana("elopros.com", &nonce, 1_793_016_000, &[1; 32], &sig),
            Err(AuthError::WrongSigner)
        );
    }

    /// Byte for byte the text the Elo Pros launcher composes for `prove` (its own fixture,
    /// `sdk/testdata/prove_fixture.json`, copied; `scripts/elo-drift.sh` keeps it current), so a
    /// desktop sign-in the launcher signs checks out here.
    #[test]
    fn the_launchers_prove_fixture_is_this_text() {
        const FIXTURE: &str = include_str!("../tests/testdata/prove_fixture.json");
        let top = |key: &str| {
            let mut quoted = [0u8; 64];
            quoted[0] = b'"';
            quoted[1..=key.len()].copy_from_slice(key.as_bytes());
            field(FIXTURE, core::str::from_utf8(&quoted[..=key.len()]).unwrap(), key)
        };
        let mut lower = [0u8; 42];
        lower.copy_from_slice(top("address").as_bytes());
        lower.make_ascii_lowercase();
        let a = parse_address(core::str::from_utf8(&lower).unwrap()).unwrap();
        let hex = top("nonce");
        let nonce: [u8; 8] =
            core::array::from_fn(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap());
        // 2026-08-07T12:00:00Z, the fixture's `issued_at`.
        let mut out = [0u8; SIWE_MAX];
        let n = write_message(
            top("game"),
            top("domain"),
            b"Ethereum",
            &checksum_hex(&a),
            Chain::Id(CHAIN_ID),
            &nonce,
            1_786_104_000,
            &mut out,
        );
        // The fixture's JSON writes each line break as `\n`.
        let text = core::str::from_utf8(&out[..n]).unwrap();
        let mut want = top("message").split("\\n");
        let mut got = text.split('\n');
        loop {
            match (want.next(), got.next()) {
                (None, None) => break,
                (w, g) => assert_eq!(w, g),
            }
        }
    }
}
