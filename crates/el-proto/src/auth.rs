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
