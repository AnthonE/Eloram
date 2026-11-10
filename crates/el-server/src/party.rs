//! Parties: up to five players on this server who queue together (NORMAL or RANKED) and are
//! seated on one side of one match. A player asks another in by their id (a signed-in player's:
//! a guest can ask, but can't be found to be asked); the asked one accepts within [`ASK_FOR`]; the
//! party's leader queues for everyone. Leaving, or the connection going, takes a player out; the
//! lead passes to the next member, and a party of one is no party.
//!
//! On tokio, beside the lobby: it may lock, and it never touches a shard.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use el_client_core::lobby::{PARTY_MAX, PartyAsk, PartyMember, ToClient};

use crate::lobby::{LobbyEvent, Waiting};

/// How long an ask into a party stands.
pub const ASK_FOR: Duration = Duration::from_secs(120);

#[derive(Default)]
struct Inner {
    /// Every session on the server, as the lobby would seat it.
    sessions: BTreeMap<u32, Waiting>,
    /// Each party's members, its leader first, by its leader's session.
    parties: Vec<Vec<u32>>,
    /// Asks: who's asked, by which party's leader, when.
    asks: Vec<(u32, u32, Instant)>,
}

#[derive(Default)]
pub struct Parties(Mutex<Inner>);

impl Inner {
    fn party_of(&self, session: u32) -> Option<usize> {
        self.parties.iter().position(|p| p.contains(&session))
    }

    fn name(&self, session: u32) -> String {
        self.sessions.get(&session).map_or_else(|| "someone".into(), |w| w.name.clone())
    }

    fn view(&self, session: u32) -> ToClient {
        let now = Instant::now();
        let (leader, members) = match self.party_of(session) {
            Some(k) => {
                let p = &self.parties[k];
                let members = p
                    .iter()
                    .map(|&s| PartyMember {
                        session: s,
                        name: self.name(s),
                        player: self.sessions.get(&s).and_then(|w| w.wallet).map(|w| w.to_string()),
                    })
                    .collect();
                (p[0], members)
            }
            None => (0, Vec::new()),
        };
        let invites_in = self
            .asks
            .iter()
            .filter(|(to, _, at)| *to == session && now.duration_since(*at) < ASK_FOR)
            .map(|&(_, leader, _)| PartyAsk { leader, name: self.name(leader) })
            .collect();
        ToClient::Party { leader, members, invites_in }
    }

    fn tell(&self, sessions: impl IntoIterator<Item = u32>) {
        for s in sessions {
            if let Some(w) = self.sessions.get(&s) {
                let _ = w.tx.send(LobbyEvent::Tell(self.view(s)));
            }
        }
    }

    /// Takes `session` out of its party: the lead passes on, and a party left with one is none.
    /// The sessions that hear of it.
    fn out(&mut self, session: u32) -> Vec<u32> {
        let Some(k) = self.party_of(session) else { return Vec::new() };
        let mut told = self.parties[k].clone();
        self.parties[k].retain(|&s| s != session);
        if self.parties[k].len() <= 1 {
            let p = self.parties.swap_remove(k);
            if let Some(&last) = p.first() {
                self.asks.retain(|(_, leader, _)| *leader != last);
            }
        }
        // Asks by a party whose leader left are void.
        let alive: Vec<u32> = self.parties.iter().map(|p| p[0]).collect();
        let void: Vec<u32> =
            self.asks.iter().filter(|(_, l, _)| !alive.contains(l)).map(|(to, _, _)| *to).collect();
        self.asks.retain(|(_, l, _)| alive.contains(l));
        told.extend(void);
        told
    }
}

impl Parties {
    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A session is on the server (as the lobby would seat it).
    pub(crate) fn register(&self, w: Waiting) {
        self.inner().sessions.insert(w.session, w);
    }

    /// A session is gone: out of its party, its asks void.
    pub fn unregister(&self, session: u32) {
        let mut inner = self.inner();
        let told = inner.out(session);
        inner.asks.retain(|(to, l, _)| *to != session && *l != session);
        inner.sessions.remove(&session);
        inner.tell(told.into_iter().filter(|&s| s != session));
    }

    /// `from` asks `to` into its party (a new one, led by `from`, if it's in none).
    pub fn invite(&self, from: u32, to: u32) -> Result<(), String> {
        let mut inner = self.inner();
        if from == to {
            return Err("That's you.".into());
        }
        if !inner.sessions.contains_key(&to) {
            return Err("They aren't on this server.".into());
        }
        let k = match inner.party_of(from) {
            Some(k) => k,
            None => {
                inner.parties.push(vec![from]);
                inner.parties.len() - 1
            }
        };
        if inner.parties[k][0] != from {
            return Err("Only the party's leader asks people in.".into());
        }
        if inner.parties[k].contains(&to) {
            return Err("They're in your party already.".into());
        }
        if inner.parties[k].len() >= PARTY_MAX {
            return Err(format!("A party is {PARTY_MAX} at most."));
        }
        inner.asks.retain(|(t, l, _)| !(*t == to && *l == from));
        inner.asks.push((to, from, Instant::now()));
        let mut told = inner.parties[k].clone();
        told.push(to);
        inner.tell(told);
        Ok(())
    }

    /// `session` takes `leader`'s ask: out of any party it's in, into that one.
    pub fn accept(&self, session: u32, leader: u32) -> Result<(), String> {
        let mut inner = self.inner();
        let now = Instant::now();
        let Some(i) = inner
            .asks
            .iter()
            .position(|&(to, l, at)| to == session && l == leader && now.duration_since(at) < ASK_FOR)
        else {
            return Err("That ask has lapsed.".into());
        };
        inner.asks.swap_remove(i);
        let Some(k) = inner.parties.iter().position(|p| p[0] == leader) else {
            return Err("That party is gone.".into());
        };
        if inner.parties[k].len() >= PARTY_MAX {
            return Err("That party is full.".into());
        }
        let mut told = inner.out(session);
        let Some(k) = inner.parties.iter().position(|p| p[0] == leader) else {
            return Err("That party is gone.".into());
        };
        inner.parties[k].push(session);
        told.extend(inner.parties[k].iter().copied());
        told.sort_unstable();
        told.dedup();
        inner.tell(told);
        Ok(())
    }

    pub fn decline(&self, session: u32, leader: u32) {
        let mut inner = self.inner();
        inner.asks.retain(|(to, l, _)| !(*to == session && *l == leader));
        let mut told = vec![session];
        if let Some(k) = inner.parties.iter().position(|p| p[0] == leader) {
            told.extend(inner.parties[k].iter().copied());
        }
        inner.tell(told);
    }

    pub fn leave(&self, session: u32) {
        let mut inner = self.inner();
        let mut told = inner.out(session);
        told.push(session);
        told.sort_unstable();
        told.dedup();
        inner.tell(told);
    }

    /// `session`'s party, its leader first, if it's in one of two or more.
    pub fn party_of(&self, session: u32) -> Option<Vec<u32>> {
        let inner = self.inner();
        inner.party_of(session).map(|k| inner.parties[k].clone()).filter(|p| p.len() > 1)
    }

    /// The members as the lobby seats them, in the party's order.
    pub(crate) fn waiting(&self, members: &[u32]) -> Vec<Waiting> {
        let inner = self.inner();
        members.iter().filter_map(|s| inner.sessions.get(s).cloned()).collect()
    }

    /// The party members taken out of a queue when `who` left it are told so.
    pub fn tell_left(&self, sessions: &[u32], who: &str) {
        let inner = self.inner();
        for s in sessions {
            if let Some(w) = inner.sessions.get(s) {
                let why = format!("{who} left the queue: your party is out of it.");
                let _ = w.tx.send(LobbyEvent::Tell(ToClient::Refused { what: "queue".into(), why }));
            }
        }
    }

    /// A member's name.
    pub fn name(&self, session: u32) -> String {
        self.inner().name(session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    fn join(p: &Parties, session: u32) -> UnboundedReceiver<LobbyEvent> {
        let (tx, rx) = unbounded_channel();
        p.register(Waiting {
            session,
            name: format!("P{session}"),
            agent: false,
            max_datagram: 1200,
            tx,
            token: String::new(),
            wallet: None,
            party: 0,
        });
        rx
    }

    fn last_view(rx: &mut UnboundedReceiver<LobbyEvent>) -> Option<(u32, Vec<u32>, Vec<u32>)> {
        let mut last = None;
        while let Ok(ev) = rx.try_recv() {
            if let LobbyEvent::Tell(ToClient::Party { leader, members, invites_in }) = ev {
                last = Some((
                    leader,
                    members.iter().map(|m| m.session).collect(),
                    invites_in.iter().map(|a| a.leader).collect(),
                ));
            }
        }
        last
    }

    #[test]
    fn ask_accept_lead_passes_and_one_is_no_party() {
        let p = Parties::default();
        let (mut a, mut b, mut c) = (join(&p, 1), join(&p, 2), join(&p, 3));
        p.invite(1, 2).unwrap();
        assert_eq!(last_view(&mut b), Some((0, vec![], vec![1])), "asked, in no party yet");
        assert!(p.party_of(1).is_none(), "a party of one is none");
        p.accept(2, 1).unwrap();
        assert_eq!(last_view(&mut a), Some((1, vec![1, 2], vec![])));
        assert_eq!(p.party_of(2), Some(vec![1, 2]));
        assert!(p.invite(2, 3).is_err(), "only the leader asks");
        p.invite(1, 3).unwrap();
        p.accept(3, 1).unwrap();
        assert_eq!(last_view(&mut c), Some((1, vec![1, 2, 3], vec![])));
        // The leader goes: the next member leads.
        p.unregister(1);
        assert_eq!(last_view(&mut b), Some((2, vec![2, 3], vec![])));
        p.leave(3);
        assert_eq!(last_view(&mut b), Some((0, vec![], vec![])), "one left: no party");
        assert!(p.accept(3, 9).is_err(), "no such ask");
    }

    #[test]
    fn a_party_is_five_at_most() {
        let p = Parties::default();
        let _rx: Vec<_> = (1..=7).map(|s| join(&p, s)).collect();
        for s in 2..=5 {
            p.invite(1, s).unwrap();
            p.accept(s, 1).unwrap();
        }
        assert!(p.invite(1, 6).is_err());
        assert_eq!(p.party_of(1).map(|m| m.len()), Some(5));
    }
}
