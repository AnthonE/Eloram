//! Wallet sign-in on the wire: what the handshake carries. The message a wallet signs, and the
//! server's check of it, are `el-auth`'s.

/// An Ethereum address.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Address(pub [u8; 20]);

/// A `personal_sign` signature: r, s, v.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Signature(pub [u8; 65]);

impl Default for Signature {
    fn default() -> Self {
        Self([0; 65])
    }
}

/// Not printed: a signature in a log is noise that looks like a secret.
impl core::fmt::Debug for Signature {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Signature(65 bytes)")
    }
}

/// The server's per-connection challenge nonce.
pub const NONCE_BYTES: usize = 32;
/// A resume token: lets a signed-in pilot reconnect without signing again.
pub const TOKEN_BYTES: usize = 32;
/// Longest server domain, bytes.
pub const MAX_DOMAIN: usize = 64;

/// The server's name as pages see it (`play.example.com`, `127.0.0.1:8080`): printable ASCII,
/// stored inline.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Domain {
    pub(crate) len: u8,
    pub(crate) bytes: [u8; MAX_DOMAIN],
}

impl Domain {
    /// Keeps printable ASCII, up to [`MAX_DOMAIN`] bytes.
    pub fn new(s: &str) -> Self {
        let mut bytes = [0u8; MAX_DOMAIN];
        let mut len = 0;
        for b in s.bytes().filter(|b| b.is_ascii_graphic()).take(MAX_DOMAIN) {
            bytes[len] = b;
            len += 1;
        }
        Self { len: len as u8, bytes }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

impl Default for Domain {
    fn default() -> Self {
        Self::new("")
    }
}

impl core::fmt::Debug for Domain {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Domain({:?})", self.as_str())
    }
}
/// A Solana wallet's public key.
pub const SOLANA_KEY_BYTES: usize = 32;

/// An Ed25519 signature by a Solana wallet.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SolanaSignature(pub [u8; 64]);

impl Default for SolanaSignature {
    fn default() -> Self {
        Self([0; 64])
    }
}

/// Not printed, like [`Signature`].
impl core::fmt::Debug for SolanaSignature {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SolanaSignature(64 bytes)")
    }
}

/// The longest [`PlayerId`]: a Solana address is up to 44 base58 characters.
pub const MAX_PLAYER_ID: usize = 44;

/// Who a signed-in pilot is, as Elo Pros names them, in its one spelling: a Robinhood Chain wallet
/// (`0x` and 40 lowercase hex), a Solana wallet (base58, **case-sensitive, exactly as written**) or
/// an account with no wallet (`acct_` and 20 lowercase hex). Stored inline, so it's `Copy`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlayerId {
    len: u8,
    bytes: [u8; MAX_PLAYER_ID],
}

/// What a [`PlayerId`] names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerKind {
    Evm,
    Solana,
    Account,
}

fn lower_hex_digits(s: &[u8], n: usize) -> bool {
    s.len() == n && s.iter().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
}

impl PlayerId {
    fn inline(s: &[u8]) -> Self {
        let mut bytes = [0u8; MAX_PLAYER_ID];
        bytes[..s.len()].copy_from_slice(s);
        Self { len: s.len() as u8, bytes }
    }

    /// The id `s` spells, or `None` if it's none of the three. An `0x` address is lowercased (its
    /// one spelling); nothing else is changed.
    pub fn parse(s: &str) -> Option<Self> {
        let b = s.as_bytes();
        if b.len() == 42 && b[0] == b'0' && (b[1] == b'x' || b[1] == b'X') {
            if !b[2..].iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            let mut id = Self::inline(b);
            id.bytes[..42].make_ascii_lowercase();
            return Some(id);
        }
        if b.starts_with(b"acct_") {
            return lower_hex_digits(&b[5..], 20).then(|| Self::inline(b));
        }
        base58_key(s).map(|_| Self::inline(b))
    }

    /// A Solana wallet from its public key.
    pub fn solana(key: &[u8; SOLANA_KEY_BYTES]) -> Self {
        let mut bytes = [0u8; MAX_PLAYER_ID];
        let len = base58_write(key, &mut bytes);
        Self { len: len as u8, bytes }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }

    pub fn kind(&self) -> PlayerKind {
        match self.bytes[0] {
            b'0' => PlayerKind::Evm,
            b'a' if self.len == 25 && self.bytes[..5] == *b"acct_" => PlayerKind::Account,
            _ => PlayerKind::Solana,
        }
    }

    /// The wallet, if this is an `0x` one.
    pub fn address(&self) -> Option<Address> {
        if self.kind() != PlayerKind::Evm {
            return None;
        }
        let mut out = [0u8; 20];
        for (i, o) in out.iter_mut().enumerate() {
            let digit = |c: u8| if c.is_ascii_digit() { c - b'0' } else { c - b'a' + 10 };
            *o = digit(self.bytes[2 + i * 2]) << 4 | digit(self.bytes[3 + i * 2]);
        }
        Some(Address(out))
    }

    /// The public key, if this is a Solana wallet.
    pub fn solana_key(&self) -> Option<[u8; SOLANA_KEY_BYTES]> {
        if self.kind() == PlayerKind::Solana { base58_key(self.as_str()) } else { None }
    }

    /// A wallet: it can hold things. An account with no wallet can't.
    pub fn is_wallet(&self) -> bool {
        self.kind() != PlayerKind::Account
    }
}

impl From<Address> for PlayerId {
    fn from(a: Address) -> Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut bytes = [0u8; MAX_PLAYER_ID];
        bytes[..2].copy_from_slice(b"0x");
        for (i, b) in a.0.iter().enumerate() {
            bytes[2 + i * 2] = HEX[usize::from(b >> 4)];
            bytes[3 + i * 2] = HEX[usize::from(b & 0xf)];
        }
        Self { len: 42, bytes }
    }
}

impl core::fmt::Debug for PlayerId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "PlayerId({})", self.as_str())
    }
}

impl core::fmt::Display for PlayerId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

const BASE58: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// The base58 text of `key` into `out`; its length.
fn base58_write(key: &[u8; SOLANA_KEY_BYTES], out: &mut [u8; MAX_PLAYER_ID]) -> usize {
    // Digits, least significant first.
    let mut digits = [0u8; MAX_PLAYER_ID];
    let mut n = 0;
    for &byte in key {
        let mut carry = u32::from(byte);
        for d in &mut digits[..n] {
            carry += u32::from(*d) << 8;
            *d = (carry % 58) as u8;
            carry /= 58;
        }
        while carry > 0 && n < MAX_PLAYER_ID {
            digits[n] = (carry % 58) as u8;
            carry /= 58;
            n += 1;
        }
    }
    let zeros = key.iter().take_while(|b| **b == 0).count();
    let len = zeros + n;
    out[..zeros].fill(b'1');
    for (o, d) in out[zeros..len].iter_mut().zip(digits[..n].iter().rev()) {
        *o = BASE58[usize::from(*d)];
    }
    len
}

/// The public key a Solana address spells: base58, 32 to 44 characters, exactly the text of 32
/// bytes. Case matters: another casing is another key, or none.
pub fn base58_key(s: &str) -> Option<[u8; SOLANA_KEY_BYTES]> {
    if !(32..=MAX_PLAYER_ID).contains(&s.len()) {
        return None;
    }
    let mut key = [0u8; SOLANA_KEY_BYTES];
    for c in s.bytes() {
        let mut carry = BASE58.iter().position(|d| *d == c)? as u32;
        for b in key.iter_mut().rev() {
            carry += u32::from(*b) * 58;
            *b = carry as u8;
            carry >>= 8;
        }
        if carry != 0 {
            return None;
        }
    }
    // Only one text spells 32 bytes (a leading `1` is a zero byte).
    let mut again = [0u8; MAX_PLAYER_ID];
    let len = base58_write(&key, &mut again);
    (&again[..len] == s.as_bytes()).then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `signin_check.json`'s throwaway wallet (Ed25519 seed 07…07), and its public key.
    const SOL: &str = "GmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB";

    #[test]
    fn a_solana_address_is_kept_exactly_as_written() {
        let id = PlayerId::parse(SOL).unwrap();
        assert_eq!(id.as_str(), SOL);
        assert_eq!(id.kind(), PlayerKind::Solana);
        assert!(id.is_wallet());
        let key = id.solana_key().unwrap();
        assert_eq!(PlayerId::solana(&key), id);
        // Lowercased, it names nobody.
        let mut lower = [0u8; 44];
        lower.copy_from_slice(SOL.as_bytes());
        lower.make_ascii_lowercase();
        assert_eq!(PlayerId::parse(core::str::from_utf8(&lower).unwrap()), None);
    }

    #[test]
    fn solanas_all_ones_and_an_all_ff_key_round_trip() {
        // The system program's address: 32 zero bytes.
        let zero = PlayerId::solana(&[0; 32]);
        assert_eq!(zero.as_str(), "11111111111111111111111111111111");
        assert_eq!(PlayerId::parse(zero.as_str()), Some(zero));
        let ff = PlayerId::solana(&[0xff; 32]);
        assert_eq!(ff.as_str().len(), 44);
        assert_eq!(ff.solana_key(), Some([0xff; 32]));
    }

    #[test]
    fn the_other_two_shapes() {
        let upper = "0xABCDEF0123456789ABCDEF0123456789ABCDEF01";
        let id = PlayerId::parse(upper).unwrap();
        assert_eq!(id.as_str(), "0xabcdef0123456789abcdef0123456789abcdef01");
        assert_eq!(id.kind(), PlayerKind::Evm);
        assert_eq!(PlayerId::from(id.address().unwrap()), id);
        let acct = PlayerId::parse("acct_0123456789abcdef0123").unwrap();
        assert_eq!(acct.kind(), PlayerKind::Account);
        assert!(!acct.is_wallet());
        assert_eq!(acct.address(), None);
        assert_eq!(acct.solana_key(), None);
        for bad in ["", "0x12", "acct_01234", "acct_0123456789ABCDEF0123", "../../etc/passwd", "O0Il"] {
            assert_eq!(PlayerId::parse(bad), None, "{bad}");
        }
    }
}
