//! Who gets a connection (Before Colony's `docs/ARCHITECTURE.md`, "Under abuse").
//!
//! Titanfall's servers were DDoSed out of the game for years. A flood of packets is for the
//! network upstream of the server to stop (a host's scrubbing, a firewall's rate limits); what the
//! server does is keep a flood of *connections* from crowding the players out:
//! - **Retry under load.** With more than [`Limits::retry_above`] handshakes under way, an address
//!   that hasn't proved it hears back (QUIC's address validation) is sent a Retry first: a flood
//!   from spoofed addresses never gets as far as a handshake.
//! - **A share per address.** One address holds at most [`Limits::per_address`] connections,
//!   handshaking or open. Loopback is excepted: the server's own agents connect from it.
//! - **A ceiling.** At most [`Limits::connections`] in all.
//! - **A deadline.** A handshake, QUIC's and WebTransport's, finishes within [`HANDSHAKE`].
//!
//! What a session may do once it's in is limited where it's read (`session.rs`: inputs and the
//! lobby's requests).

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long a connection's handshakes may take, QUIC's and then WebTransport's.
pub const HANDSHAKE: Duration = Duration::from_secs(5);

/// How many connections the server takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Connections in all, handshaking or open.
    pub connections: u32,
    /// Connections from one address...
    pub per_address: u32,
    /// ...but loopback's, where the server's own agents connect from.
    pub loopback_excepted: bool,
    /// Handshakes under way past which an unvalidated address is sent a Retry first.
    pub retry_above: u32,
}

impl Default for Limits {
    fn default() -> Self {
        // A household or a campus behind one address brings a few.
        Self { connections: 512, per_address: 8, loopback_excepted: true, retry_above: 32 }
    }
}

/// What to do with a connection attempt.
#[derive(Debug)]
pub enum Verdict {
    /// In, for as long as the [`Pass`] is kept.
    Admit(Pass),
    /// Ask it to prove its address first (a QUIC Retry).
    Retry,
    Refuse(Refusal),
}

/// Why a connection attempt was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The server has all the connections it takes.
    Full,
    /// Its address has its share.
    Address,
}

/// The connections the server holds, in all and by address.
#[derive(Debug)]
pub struct Admission {
    limits: Limits,
    by_address: Mutex<HashMap<IpAddr, u32>>,
    open: AtomicU32,
    handshaking: AtomicU32,
}

impl Admission {
    pub fn new(limits: Limits) -> Arc<Self> {
        Arc::new(Self {
            limits,
            by_address: Mutex::new(HashMap::new()),
            open: AtomicU32::new(0),
            handshaking: AtomicU32::new(0),
        })
    }

    /// A connection attempt from `ip`; `validated`: it has proved it hears back at that address.
    pub fn admit(self: &Arc<Self>, ip: IpAddr, validated: bool) -> Verdict {
        let ip = ip.to_canonical();
        let local = ip.is_loopback() && self.limits.loopback_excepted;
        let ip = whose(ip);
        let mut by_address = self.by_address.lock().unwrap_or_else(|e| e.into_inner());
        if self.open.load(Ordering::Relaxed) >= self.limits.connections {
            return Verdict::Refuse(Refusal::Full);
        }
        let held = by_address.get(&ip).copied().unwrap_or(0);
        if !local && held >= self.limits.per_address {
            return Verdict::Refuse(Refusal::Address);
        }
        if !validated && self.handshaking.load(Ordering::Relaxed) >= self.limits.retry_above {
            return Verdict::Retry;
        }
        by_address.insert(ip, held + 1);
        self.open.fetch_add(1, Ordering::Relaxed);
        self.handshaking.fetch_add(1, Ordering::Relaxed);
        Verdict::Admit(Pass { admission: self.clone(), ip, handshaking: true })
    }

    /// Connections held, handshaking or open.
    pub fn open(&self) -> u32 {
        self.open.load(Ordering::Relaxed)
    }

    /// Handshakes under way.
    pub fn handshaking(&self) -> u32 {
        self.handshaking.load(Ordering::Relaxed)
    }

    /// Distinct addresses holding connections.
    pub fn addresses(&self) -> usize {
        self.by_address.lock().map_or(0, |m| m.len())
    }
}

/// Whom a connection from `ip` (canonical) counts against: an IPv4 address, or an IPv6 address's
/// /64, which is what one subscriber is given (a host can take any address in it).
fn whose(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => IpAddr::V6(Ipv6Addr::from(u128::from(v6) & !(u128::from(u64::MAX)))),
        v4 => v4,
    }
}

/// A connection's place, given back when it's dropped.
#[derive(Debug)]
pub struct Pass {
    admission: Arc<Admission>,
    ip: IpAddr,
    handshaking: bool,
}

impl Pass {
    /// Its handshakes are done.
    pub fn established(&mut self) {
        if std::mem::take(&mut self.handshaking) {
            self.admission.handshaking.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

impl Drop for Pass {
    fn drop(&mut self) {
        self.established();
        let a = &self.admission;
        a.open.fetch_sub(1, Ordering::Relaxed);
        let mut by_address = a.by_address.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = by_address.get_mut(&self.ip) {
            *n -= 1;
            if *n == 0 {
                by_address.remove(&self.ip);
            }
        }
    }
}

/// How fast a session may ask things of the lobby on the control stream: a bucket of
/// [`Requests::BURST`] that refills at [`Requests::PER_SEC`]. A session that keeps on past an
/// empty bucket is ended ([`Requests::ENDS_AFTER`] refused in a row).
#[derive(Debug)]
pub struct Requests {
    tokens: f64,
    last: std::time::Instant,
    refused: u32,
}

/// What a session's request comes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    Take,
    /// Too fast: refused (say so).
    Refuse,
    /// Far too fast, for too long: end the session.
    End,
}

impl Requests {
    pub const PER_SEC: f64 = 10.0;
    pub const BURST: f64 = 40.0;
    pub const ENDS_AFTER: u32 = 200;

    pub fn new(now: std::time::Instant) -> Self {
        Self { tokens: Self::BURST, last: now, refused: 0 }
    }

    pub fn ask(&mut self, now: std::time::Instant) -> Asked {
        let gap = now.saturating_duration_since(self.last).as_secs_f64();
        self.tokens = (self.tokens + gap * Self::PER_SEC).min(Self::BURST);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            self.refused = 0;
            Asked::Take
        } else {
            self.refused += 1;
            if self.refused >= Self::ENDS_AFTER { Asked::End } else { Asked::Refuse }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([203, 0, 113, last])
    }

    fn admit(a: &Arc<Admission>, from: IpAddr) -> Option<Pass> {
        match a.admit(from, true) {
            Verdict::Admit(p) => Some(p),
            _ => None,
        }
    }

    #[test]
    fn an_address_gets_its_share_and_no_more() {
        let a = Admission::new(Limits {
            connections: 100,
            per_address: 3,
            retry_above: 100,
            ..Limits::default()
        });
        let mut held: Vec<Pass> = (0..3).map(|_| admit(&a, ip(1)).unwrap()).collect();
        assert!(matches!(a.admit(ip(1), true), Verdict::Refuse(Refusal::Address)));
        // Another address is let in.
        let other = admit(&a, ip(2)).unwrap();
        assert_eq!((a.open(), a.addresses()), (4, 2));
        // A connection closing makes room for its address again.
        held.pop();
        assert!(admit(&a, ip(1)).is_some());
        drop(other);
        assert_eq!(a.addresses(), 1);
    }

    #[test]
    fn an_address_is_counted_as_its_subscriber() {
        let a = Admission::new(Limits { per_address: 2, ..Limits::default() });
        // IPv4 as itself, mapped into IPv6 or not.
        let v4 = admit(&a, ip(1)).unwrap();
        let mapped = admit(&a, IpAddr::V6(Ipv6Addr::from([0, 0, 0, 0, 0, 0xffff, 0xcb00, 0x7101]))).unwrap();
        assert!(matches!(a.admit(ip(1), true), Verdict::Refuse(Refusal::Address)));
        // IPv6 by its /64: a host hopping addresses within it is one subscriber.
        let net = |host: u16| IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 1, 2, 0, 0, 0, host));
        let held: Vec<Pass> = (1..=2).map(|h| admit(&a, net(h)).unwrap()).collect();
        assert!(matches!(a.admit(net(3), true), Verdict::Refuse(Refusal::Address)));
        assert!(admit(&a, IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 1, 3, 0, 0, 0, 1))).is_some());
        // Loopback mapped into IPv6 is loopback.
        let local: Vec<Pass> = (0..4)
            .map(|_| admit(&a, IpAddr::V6(Ipv6Addr::from([0, 0, 0, 0, 0, 0xffff, 0x7f00, 1]))).unwrap())
            .collect();
        drop((v4, mapped, held, local));
    }

    #[test]
    fn loopback_is_excepted_but_not_from_the_ceiling() {
        let a =
            Admission::new(Limits { connections: 5, per_address: 2, retry_above: 100, ..Limits::default() });
        let local: Vec<Pass> = (0..5).map(|_| admit(&a, IpAddr::from([127, 0, 0, 1])).unwrap()).collect();
        assert!(matches!(a.admit(IpAddr::from([127, 0, 0, 1]), true), Verdict::Refuse(Refusal::Full)));
        assert!(matches!(a.admit(ip(9), true), Verdict::Refuse(Refusal::Full)));
        drop(local);
        assert_eq!((a.open(), a.handshaking(), a.addresses()), (0, 0, 0));
    }

    #[test]
    fn under_load_an_unproved_address_is_asked_to_retry() {
        let a = Admission::new(Limits {
            connections: 100,
            per_address: 100,
            retry_above: 2,
            ..Limits::default()
        });
        let mut first: Vec<Pass> = (0..2).map(|k| admit(&a, ip(k)).unwrap()).collect();
        assert_eq!(a.handshaking(), 2);
        // Two handshakes under way: a new address must prove itself; one that has comes in.
        assert!(matches!(a.admit(ip(7), false), Verdict::Retry));
        let proved = admit(&a, ip(7)).unwrap();
        // Handshakes done, the load is off: no Retry.
        for p in &mut first {
            p.established();
        }
        drop(proved);
        assert_eq!(a.handshaking(), 0);
        assert!(matches!(a.admit(ip(8), false), Verdict::Admit(_)));
    }

    #[test]
    fn a_request_flood_is_refused_then_ended() {
        let t0 = Instant::now();
        let mut r = Requests::new(t0);
        // A burst is fine...
        for _ in 0..Requests::BURST as usize {
            assert_eq!(r.ask(t0), Asked::Take);
        }
        // ...then it's refused until the bucket refills...
        assert_eq!(r.ask(t0), Asked::Refuse);
        assert_eq!(r.ask(t0 + Duration::from_millis(150)), Asked::Take);
        // ...and a session that never stops is ended.
        let mut last = Asked::Take;
        for _ in 0..Requests::ENDS_AFTER {
            last = r.ask(t0 + Duration::from_millis(150));
        }
        assert_eq!(last, Asked::End);
    }
}
