//! Control stream: one reliable, ordered, bidirectional WebTransport stream per session.
//!
//! Frame: `[u16 LE payload length][u8 tag][payload]`. Frames are small and rare (handshake, sign-in,
//! being seated in a match), so they're byte-aligned for simplicity. A [`LOBBY`] frame carries a
//! lobby message instead (JSON: the queue, the draw, the shop's catalogue, the scoreboard; up to
//! [`MAX_LOBBY_FRAME`]): read the stream with [`Frame::decode`].

use crate::auth::{Address, Domain, MAX_DOMAIN, NONCE_BYTES, Signature, TOKEN_BYTES};
use crate::{DecodeError, PROTOCOL_VERSION};

/// Longest player name, in UTF-8 bytes.
pub const MAX_NAME: usize = 16;
/// Largest control frame (prefix included).
pub const MAX_FRAME: usize = 256;
/// The tag of a lobby frame, whose payload is a lobby message (JSON).
pub const LOBBY: u8 = 11;
/// Largest lobby frame (prefix and tag included).
pub const MAX_LOBBY_FRAME: usize = 2 + u16::MAX as usize;

/// Who is at the controls. Agents say so: the store is for people and agents equally, and both
/// are shown for what they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PlayerKind {
    Human = 0,
    Agent = 1,
}

impl PlayerKind {
    pub fn from_u8(v: u8) -> Self {
        if v == 1 { PlayerKind::Agent } else { PlayerKind::Human }
    }
}

/// [`ControlMsg::Hello`] flags.
pub mod hello_flags {
    /// The player will sign in with a wallet: answer with a Challenge.
    pub const SIGN_IN: u8 = 1 << 0;
    /// The player signed in earlier and carries a resume token (a reconnect: no signature asked).
    pub const RESUME: u8 = 1 << 1;
}

/// [`ControlMsg::Welcome`] flags.
pub mod welcome_flags {
    /// Signed in with a wallet.
    pub const SIGNED_IN: u8 = 1 << 0;
}

/// [`ControlMsg::Bye`] reasons.
pub mod bye {
    pub const LEAVE: u8 = 0;
    /// The same player signed in somewhere else.
    pub const TAKEN_OVER: u8 = 1;
    /// Nothing heard from the client for too long.
    pub const IDLE: u8 = 2;
    pub const SHUTDOWN: u8 = 3;
}

/// A short player name stored inline (no allocation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    len: u8,
    bytes: [u8; MAX_NAME],
}

impl Name {
    /// Truncates to [`MAX_NAME`] bytes on a character boundary and drops control characters.
    pub fn new(s: &str) -> Self {
        let mut bytes = [0u8; MAX_NAME];
        let mut len = 0usize;
        for ch in s.chars().filter(|c| !c.is_control()) {
            let mut tmp = [0u8; 4];
            let enc = ch.encode_utf8(&mut tmp).as_bytes();
            if len + enc.len() > MAX_NAME {
                break;
            }
            bytes[len..len + enc.len()].copy_from_slice(enc);
            len += enc.len();
        }
        Self { len: len as u8, bytes }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("?")
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for Name {
    fn default() -> Self {
        Self::new("")
    }
}

/// Why the server refused a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RejectReason {
    VersionMismatch = 1,
    ServerFull = 2,
    BadHello = 3,
    /// The signature didn't prove the address.
    AuthFailed = 5,
    /// This server admits signed-in players only.
    AuthRequired = 6,
    /// The resume token is unknown or expired: sign in again.
    ResumeExpired = 7,
    /// No signature came in time.
    AuthTimeout = 8,
}

impl RejectReason {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::VersionMismatch,
            2 => Self::ServerFull,
            5 => Self::AuthFailed,
            6 => Self::AuthRequired,
            7 => Self::ResumeExpired,
            8 => Self::AuthTimeout,
            _ => Self::BadHello,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlMsg {
    /// Client → server, first frame. `player` must be honest: agents identify as
    /// [`PlayerKind::Agent`]. `flags` are [`hello_flags`]; `resume` is the resume token when
    /// [`hello_flags::RESUME`] is set.
    Hello { version: u16, player: PlayerKind, name: Name, flags: u8, resume: [u8; TOKEN_BYTES] },
    /// Server → client: you're in (the lobby, until a match seats you).
    Welcome {
        version: u16,
        /// The session's number on this server, for the roster.
        session: u32,
        tick_hz: u8,
        max_datagram: u16,
        /// [`welcome_flags`].
        flags: u8,
    },
    /// Server → client, then the stream closes.
    Reject { reason: RejectReason },
    /// Either direction: goodbye ([`bye`]).
    Bye { reason: u8 },
    /// Server → client, after a Hello with [`hello_flags::SIGN_IN`]: sign in to `domain` with this
    /// nonce, at this time (Unix seconds).
    Challenge { nonce: [u8; NONCE_BYTES], issued_at: u64, domain: Domain },
    /// Client → server: the wallet's address and its signature over the challenge's message.
    Auth { address: Address, signature: Signature },
    /// Server → client, after the Welcome of a signed-in player: reconnect with this instead of
    /// signing again.
    Token { token: [u8; TOKEN_BYTES] },
    /// Server → client: something the player should know; `name` is the code's detail.
    Notice { code: u8, name: Name },
}

impl ControlMsg {
    pub fn hello(player: PlayerKind, name: &str) -> Self {
        ControlMsg::Hello {
            version: PROTOCOL_VERSION,
            player,
            name: Name::new(name),
            flags: 0,
            resume: [0; TOKEN_BYTES],
        }
    }

    /// Encodes one frame (length prefix included) into `buf`; returns its length.
    pub fn encode(&self, buf: &mut [u8]) -> Option<usize> {
        let mut p = Cursor { buf, pos: 2, ok: true };
        match *self {
            ControlMsg::Hello { version, player, name, flags, resume } => {
                p.u8(1);
                p.u16(version);
                p.u8(player as u8);
                p.name(&name);
                p.u8(flags);
                if flags & hello_flags::RESUME != 0 {
                    p.bytes(&resume);
                }
            }
            ControlMsg::Welcome { version, session, tick_hz, max_datagram, flags } => {
                p.u8(2);
                p.u16(version);
                p.u32(session);
                p.u8(tick_hz);
                p.u16(max_datagram);
                p.u8(flags);
            }
            ControlMsg::Reject { reason } => {
                p.u8(3);
                p.u8(reason as u8);
            }
            ControlMsg::Bye { reason } => {
                p.u8(6);
                p.u8(reason);
            }
            ControlMsg::Challenge { nonce, issued_at, domain } => {
                p.u8(7);
                p.bytes(&nonce);
                p.u64(issued_at);
                p.u8(domain.len);
                p.bytes(&domain.bytes[..domain.len as usize]);
            }
            ControlMsg::Auth { address, signature } => {
                p.u8(8);
                p.bytes(&address.0);
                p.bytes(&signature.0);
            }
            ControlMsg::Token { token } => {
                p.u8(9);
                p.bytes(&token);
            }
            ControlMsg::Notice { code, name } => {
                p.u8(10);
                p.u8(code);
                p.name(&name);
            }
        }
        if !p.ok {
            return None;
        }
        let len = p.pos;
        let payload = (len - 2) as u16;
        p.buf[..2].copy_from_slice(&payload.to_le_bytes());
        Some(len)
    }

    /// Decodes the first complete frame in `buf`. `Ok(None)` means more bytes are needed; on success
    /// returns the message and how many bytes it consumed.
    pub fn decode(buf: &[u8]) -> Result<Option<(Self, usize)>, DecodeError> {
        if buf.len() < 2 {
            return Ok(None);
        }
        let payload = u16::from_le_bytes([buf[0], buf[1]]) as usize;
        if payload == 0 || payload + 2 > MAX_FRAME {
            return Err(DecodeError::Invalid);
        }
        if buf.len() < payload + 2 {
            return Ok(None);
        }
        let mut r = Reader { buf: &buf[2..2 + payload], pos: 0 };
        let msg = match r.u8()? {
            1 => {
                let version = r.u16()?;
                if version == PROTOCOL_VERSION {
                    let player = PlayerKind::from_u8(r.u8()?);
                    let name = r.name()?;
                    let flags = r.u8()?;
                    let resume = if flags & hello_flags::RESUME != 0 {
                        r.array::<TOKEN_BYTES>()?
                    } else {
                        [0; TOKEN_BYTES]
                    };
                    ControlMsg::Hello { version, player, name, flags, resume }
                } else {
                    // Another protocol's Hello may not parse as ours: keep only its version, so the
                    // server can answer VersionMismatch.
                    ControlMsg::Hello {
                        version,
                        player: PlayerKind::Human,
                        name: Name::default(),
                        flags: 0,
                        resume: [0; TOKEN_BYTES],
                    }
                }
            }
            2 => ControlMsg::Welcome {
                version: r.u16()?,
                session: r.u32()?,
                tick_hz: r.u8()?,
                max_datagram: r.u16()?,
                flags: r.u8()?,
            },
            3 => ControlMsg::Reject { reason: RejectReason::from_u8(r.u8()?) },
            6 => ControlMsg::Bye { reason: r.u8()? },
            7 => {
                let nonce = r.array::<NONCE_BYTES>()?;
                let issued_at = r.u64()?;
                let len = r.u8()? as usize;
                if len > MAX_DOMAIN {
                    return Err(DecodeError::Invalid);
                }
                let raw = r.take(len)?;
                let text = core::str::from_utf8(raw).map_err(|_| DecodeError::Invalid)?;
                ControlMsg::Challenge { nonce, issued_at, domain: Domain::new(text) }
            }
            8 => ControlMsg::Auth { address: Address(r.array()?), signature: Signature(r.array()?) },
            9 => ControlMsg::Token { token: r.array()? },
            10 => ControlMsg::Notice { code: r.u8()?, name: r.name()? },
            _ => return Err(DecodeError::WrongKind),
        };
        Ok(Some((msg, payload + 2)))
    }
}

/// One frame off the control stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame<'a> {
    Msg(ControlMsg),
    /// A lobby message's payload (JSON).
    Lobby(&'a [u8]),
}

impl<'a> Frame<'a> {
    /// Decodes the first complete frame in `buf`. `Ok(None)` means more bytes are needed; on
    /// success returns the frame and how many bytes it took.
    pub fn decode(buf: &'a [u8]) -> Result<Option<(Frame<'a>, usize)>, DecodeError> {
        if buf.len() < 3 {
            return Ok(None);
        }
        if buf[2] != LOBBY {
            return ControlMsg::decode(buf).map(|m| m.map(|(msg, used)| (Frame::Msg(msg), used)));
        }
        let payload = u16::from_le_bytes([buf[0], buf[1]]) as usize;
        if payload == 0 {
            return Err(DecodeError::Invalid);
        }
        if buf.len() < payload + 2 {
            return Ok(None);
        }
        Ok(Some((Frame::Lobby(&buf[3..payload + 2]), payload + 2)))
    }
}

/// Writes a lobby frame carrying `payload` into `out`. Its length, or `None` if it's too long for
/// a frame or for `out`.
pub fn encode_lobby(payload: &[u8], out: &mut [u8]) -> Option<usize> {
    let len = payload.len() + 1;
    if len > u16::MAX as usize || out.len() < len + 2 {
        return None;
    }
    out[..2].copy_from_slice(&(len as u16).to_le_bytes());
    out[2] = LOBBY;
    out[3..len + 2].copy_from_slice(payload);
    Some(len + 2)
}

struct Cursor<'a> {
    buf: &'a mut [u8],
    pos: usize,
    ok: bool,
}

impl Cursor<'_> {
    fn bytes(&mut self, b: &[u8]) {
        if !self.ok || self.pos + b.len() > self.buf.len().min(MAX_FRAME) {
            self.ok = false;
            return;
        }
        self.buf[self.pos..self.pos + b.len()].copy_from_slice(b);
        self.pos += b.len();
    }
    fn u8(&mut self, v: u8) {
        self.bytes(&[v]);
    }
    fn u16(&mut self, v: u16) {
        self.bytes(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
    fn name(&mut self, n: &Name) {
        self.u8(n.len);
        self.bytes(&n.bytes[..n.len as usize]);
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.pos.checked_add(n).ok_or(DecodeError::Truncated)?;
        let out = self.buf.get(self.pos..end).ok_or(DecodeError::Truncated)?;
        self.pos = end;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, DecodeError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }
    fn name(&mut self) -> Result<Name, DecodeError> {
        let len = self.u8()? as usize;
        if len > MAX_NAME {
            return Err(DecodeError::Invalid);
        }
        let raw = self.take(len)?;
        let text = core::str::from_utf8(raw).map_err(|_| DecodeError::Invalid)?;
        Ok(Name::new(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(m: ControlMsg) -> usize {
        let mut buf = [0u8; MAX_FRAME];
        let n = m.encode(&mut buf).expect("fits");
        let (back, used) = ControlMsg::decode(&buf[..n]).expect("decodes").expect("whole");
        assert_eq!((back, used), (m, n));
        // A frame cut short asks for more, never fails.
        for cut in 0..n {
            assert_eq!(ControlMsg::decode(&buf[..cut]), Ok(None));
        }
        n
    }

    #[test]
    fn every_message_round_trips() {
        round_trip(ControlMsg::hello(PlayerKind::Human, "Aster"));
        round_trip(ControlMsg::hello(PlayerKind::Agent, "Agent-07"));
        round_trip(ControlMsg::Hello {
            version: PROTOCOL_VERSION,
            player: PlayerKind::Human,
            name: Name::new("back"),
            flags: hello_flags::SIGN_IN | hello_flags::RESUME,
            resume: [7; TOKEN_BYTES],
        });
        round_trip(ControlMsg::Welcome {
            version: PROTOCOL_VERSION,
            session: 0xDEAD_BEEF,
            tick_hz: 30,
            max_datagram: 1100,
            flags: welcome_flags::SIGNED_IN,
        });
        for reason in [
            RejectReason::VersionMismatch,
            RejectReason::ServerFull,
            RejectReason::BadHello,
            RejectReason::AuthFailed,
            RejectReason::AuthRequired,
            RejectReason::ResumeExpired,
            RejectReason::AuthTimeout,
        ] {
            round_trip(ControlMsg::Reject { reason });
        }
        round_trip(ControlMsg::Bye { reason: bye::IDLE });
        round_trip(ControlMsg::Challenge {
            nonce: [3; NONCE_BYTES],
            issued_at: 1_790_000_000,
            domain: Domain::new("elopros.com"),
        });
        round_trip(ControlMsg::Auth { address: Address([9; 20]), signature: Signature([4; 65]) });
        round_trip(ControlMsg::Token { token: [1; TOKEN_BYTES] });
        round_trip(ControlMsg::Notice { code: 2, name: Name::new("Dusk") });
    }

    #[test]
    fn another_versions_hello_still_gives_its_version() {
        let mut buf = [0u8; 16];
        buf[..2].copy_from_slice(&3u16.to_le_bytes());
        buf[2] = 1;
        buf[3..5].copy_from_slice(&(PROTOCOL_VERSION + 1).to_le_bytes());
        match ControlMsg::decode(&buf[..5]) {
            Ok(Some((ControlMsg::Hello { version, .. }, 5))) => assert_eq!(version, PROTOCOL_VERSION + 1),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn lobby_frames_ride_between_control_frames() {
        let mut stream = [0u8; 512];
        let mut n = ControlMsg::Bye { reason: bye::LEAVE }.encode(&mut stream).unwrap();
        n += encode_lobby(br#"{"queue":true}"#, &mut stream[n..]).unwrap();
        n += ControlMsg::Token { token: [5; TOKEN_BYTES] }.encode(&mut stream[n..]).unwrap();
        let mut at = 0;
        let mut seen = 0;
        while let Ok(Some((frame, used))) = Frame::decode(&stream[at..n]) {
            match (seen, frame) {
                (0, Frame::Msg(ControlMsg::Bye { reason: bye::LEAVE })) => {}
                (1, Frame::Lobby(json)) => assert_eq!(json, br#"{"queue":true}"#),
                (2, Frame::Msg(ControlMsg::Token { .. })) => {}
                other => panic!("{other:?}"),
            }
            at += used;
            seen += 1;
        }
        assert_eq!((seen, at), (3, n));
    }

    #[test]
    fn frames_past_the_limit_are_refused_and_names_truncate_on_a_char_boundary() {
        let mut buf = [0u8; 4];
        buf[..2].copy_from_slice(&(MAX_FRAME as u16).to_le_bytes());
        assert_eq!(ControlMsg::decode(&buf), Err(DecodeError::Invalid));
        let n = Name::new("ééééééééé");
        assert_eq!(n.as_str(), "éééééééé");
        assert_eq!(Name::new("a\u{7}b").as_str(), "ab");
    }
}
