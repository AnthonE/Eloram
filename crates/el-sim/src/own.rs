//! The owner's own champion, at full precision: everything [`Match::step_seat`] reads or writes of
//! it, so the client's prediction starts from exactly the server's state and steps it exactly as
//! the server will (`docs/NETCODE.md`, "Your champion is predicted"). It is one fixed list of
//! fields ([`OwnState`], floats as their bits), walked by one function both ways, and it travels
//! against the newest the client acked (`el_proto::fields`): what changed in a tick, to the bit.

use el_proto::fields;
use el_proto::{BitReader, BitWriter, DecodeError};
use glam::Vec2;

use crate::game::{Champ, Dash, Intent, Match};
use crate::handle::Handle;
use crate::missile::Source;
use crate::path::MAX_WAYPOINTS;
use crate::units::{Buff, Kind, Unit};

/// Fields of the own champion's state.
pub const OWN_FIELDS: usize = 159;
/// Bits of the own state's baseline's age: 1 to 15 ticks before the snapshot's, 0 for none (it
/// goes whole). The server takes one only that young, so the client keeps the last 16 it took.
pub const OWN_BACK_BITS: u32 = 4;
pub const OWN_BASELINES: usize = 1 << OWN_BACK_BITS;

/// The own champion's state as its fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnState(pub [u32; OWN_FIELDS]);

impl Default for OwnState {
    fn default() -> Self {
        OwnState([0; OWN_FIELDS])
    }
}

/// Each field's width in bits.
pub type OwnWidths = [u8; OWN_FIELDS];

/// The fields' widths (worked out once, by whoever codes them).
pub fn own_widths() -> OwnWidths {
    let mut f = Widths { out: [0; OWN_FIELDS], n: 0 };
    visit(&mut Unit::default(), &mut Champ::new(0), &mut f);
    f.out
}

impl OwnState {
    /// Writes it against `base`, `back` ticks older (`None`: whole).
    pub fn write(&self, w: &mut BitWriter<'_>, base: Option<(u32, &OwnState)>, widths: &OwnWidths) {
        let zero = OwnState::default();
        let (back, base) = base.filter(|(b, _)| (1..OWN_BASELINES as u32).contains(b)).unwrap_or((0, &zero));
        w.write_bits(back, OWN_BACK_BITS);
        fields::write(w, &self.0, &base.0, widths);
    }

    /// Reads one, given the kept state `back` ticks before the snapshot's.
    pub fn read(
        r: &mut BitReader<'_>,
        widths: &OwnWidths,
        base: impl FnOnce(u32) -> Option<OwnState>,
    ) -> Result<OwnState, DecodeError> {
        let back = r.read_bits(OWN_BACK_BITS);
        let base = if back == 0 { OwnState::default() } else { base(back).ok_or(DecodeError::NoBaseline)? };
        let mut out = OwnState::default();
        fields::read(r, &base.0, widths, &mut out.0)?;
        Ok(out)
    }
}

/// Walks the fields, reading or writing each in place.
trait Fields {
    fn field(&mut self, v: &mut u32, bits: u32);
}

struct Put<'a> {
    out: &'a mut [u32; OWN_FIELDS],
    n: usize,
}

impl Fields for Put<'_> {
    fn field(&mut self, v: &mut u32, _: u32) {
        self.out[self.n] = *v;
        self.n += 1;
    }
}

struct Take<'a> {
    from: &'a [u32; OWN_FIELDS],
    n: usize,
}

impl Fields for Take<'_> {
    fn field(&mut self, v: &mut u32, bits: u32) {
        *v = if bits == 32 { self.from[self.n] } else { self.from[self.n] & ((1 << bits) - 1) };
        self.n += 1;
    }
}

struct Widths {
    out: OwnWidths,
    n: usize,
}

impl Fields for Widths {
    fn field(&mut self, _: &mut u32, bits: u32) {
        self.out[self.n] = bits as u8;
        self.n += 1;
    }
}

fn word(f: &mut impl Fields, v: &mut u32) {
    f.field(v, 32);
}

fn narrow<T: Copy + Into<u32> + TryFrom<u32>>(f: &mut impl Fields, v: &mut T, bits: u32) {
    let mut x: u32 = (*v).into();
    f.field(&mut x, bits);
    if let Ok(x) = T::try_from(x) {
        *v = x;
    }
}

fn flag(f: &mut impl Fields, v: &mut bool) {
    let mut x = u32::from(*v);
    f.field(&mut x, 1);
    *v = x != 0;
}

fn float(f: &mut impl Fields, v: &mut f32) {
    let mut x = v.to_bits();
    f.field(&mut x, 32);
    *v = f32::from_bits(x);
}

fn vec2(f: &mut impl Fields, v: &mut Vec2) {
    float(f, &mut v.x);
    float(f, &mut v.y);
}

fn handle(f: &mut impl Fields, h: &mut Handle) {
    narrow(f, &mut h.idx, 16);
    narrow(f, &mut h.generation, 16);
}

/// Every field, in order. What isn't in use is zero (the writer's copies are made so first), so
/// the state is a function of what the sim reads and a field unused costs a bit at most.
fn visit(u: &mut Unit, c: &mut Champ, f: &mut impl Fields) {
    flag(f, &mut u.alive);
    narrow(f, &mut u.generation, 16);
    vec2(f, &mut u.pos);
    vec2(f, &mut u.moved);
    for v in [&mut u.facing, &mut u.hp, &mut u.mana, &mut u.shield] {
        float(f, v);
    }
    word(f, &mut u.shield_until);
    for t in [&mut u.cc.stun, &mut u.cc.root, &mut u.cc.silence, &mut u.cc.airborne, &mut u.cc.slow_until] {
        word(f, t);
    }
    word(f, &mut u.cc.immune);
    float(f, &mut u.cc.slow);
    for b in &mut u.buffs {
        narrow(f, &mut b.kind, 8);
        word(f, &mut b.until);
        float(f, &mut b.value);
    }
    handle(f, &mut u.target);
    word(f, &mut u.attack_ready);
    narrow(f, &mut u.windup, 16);
    word(f, &mut u.repath_at);
    narrow(f, &mut u.path.len, 5);
    narrow(f, &mut u.path.next, 5);
    for p in &mut u.path.points {
        vec2(f, p);
    }
    // The champion.
    narrow(f, &mut c.def, 8);
    narrow(f, &mut c.level, 8);
    float(f, &mut c.xp);
    float(f, &mut c.gold);
    for r in &mut c.ranks {
        narrow(f, r, 3);
    }
    narrow(f, &mut c.points, 8);
    for t in &mut c.ready {
        word(f, t);
    }
    for it in &mut c.items {
        narrow(f, it, 8);
    }
    narrow(f, &mut c.n_aug, 3);
    for a in &mut c.augments {
        narrow(f, a, 8);
    }
    flag(f, &mut c.screen.open);
    narrow(f, &mut c.screen.tier, 2);
    for o in &mut c.screen.offers {
        narrow(f, o, 8);
    }
    narrow(f, &mut c.screen.rerolled, 3);
    narrow(f, &mut c.screens_earned, 3);
    narrow(f, &mut c.screens_done, 3);
    for t in &mut c.aug_ready {
        word(f, t);
    }
    narrow(f, &mut c.auto_hits, 16);
    // What it's doing, as a tag and what each kind holds.
    let (mut tag, mut at, mut target, mut slot) = match c.intent {
        Intent::Idle => (0u32, Vec2::ZERO, Handle::NONE, 0u8),
        Intent::Move(p) => (1, p, Handle::NONE, 0),
        Intent::AttackMove(p) => (2, p, Handle::NONE, 0),
        Intent::Attack(h) => (3, Vec2::ZERO, h, 0),
        Intent::Hold => (4, Vec2::ZERO, Handle::NONE, 0),
        Intent::CastAt { slot, target } => (5, Vec2::ZERO, target, slot),
    };
    f.field(&mut tag, 3);
    vec2(f, &mut at);
    handle(f, &mut target);
    narrow(f, &mut slot, 8);
    c.intent = match tag {
        1 => Intent::Move(at),
        2 => Intent::AttackMove(at),
        3 => Intent::Attack(target),
        4 => Intent::Hold,
        5 => Intent::CastAt { slot, target },
        _ => Intent::Idle,
    };
    narrow(f, &mut c.cast.slot, 8);
    narrow(f, &mut c.cast.left, 16);
    vec2(f, &mut c.cast.at);
    handle(f, &mut c.cast.target);
    narrow(f, &mut c.cast.rank, 8);
    flag(f, &mut c.dash.active);
    vec2(f, &mut c.dash.to);
    handle(f, &mut c.dash.target);
    float(f, &mut c.dash.speed);
    float(f, &mut c.dash.radius);
    let mut source = c.dash.source.code();
    f.field(&mut source, 12);
    c.dash.source = Source::from_code(source);
    narrow(f, &mut c.dash.rank, 8);
    word(f, &mut c.dash.until);
    word(f, &mut c.dead_until);
    for n in [&mut c.kills, &mut c.deaths, &mut c.assists, &mut c.last_hits, &mut c.immobile_n] {
        narrow(f, n, 16);
    }
    for w in &mut c.immobile {
        let (mut lo, mut hi) = (*w as u32, (*w >> 32) as u32);
        word(f, &mut lo);
        word(f, &mut hi);
        *w = u64::from(lo) | u64::from(hi) << 32;
    }
}

impl Match {
    /// Becomes a copy of `from`    /// Becomes a copy of `from`, in place and without allocating: how the client's prediction
    /// starts over from its model of the server's match.
    pub fn assume(&mut self, from: &Match) {
        self.seed = from.seed;
        self.tick = from.tick;
        self.rng = from.rng;
        self.units.copy_from_slice(&from.units);
        self.champs = from.champs;
        self.missiles.copy_from_slice(&from.missiles);
        self.zones.copy_from_slice(&from.zones);
        self.minion_slots.copy_from(&from.minion_slots);
        self.next_wave = from.next_wave;
        self.waves = from.waves;
        self.relics = from.relics;
        self.inhib_back = from.inhib_back;
        self.vision.grid = from.vision.grid;
        for t in 0..2 {
            self.vision.units[t].copy_from(&from.vision.units[t]);
            self.vision.missiles[t].copy_from(&from.vision.missiles[t]);
        }
        self.aug_tiers = from.aug_tiers;
        self.headliner = from.headliner;
        self.events.clear();
        self.events_dropped = 0;
        self.outcome = from.outcome;
    }

    /// The champion of `seat` as its fields.
    pub fn own_state(&self, seat: usize) -> OwnState {
        let (mut u, mut c) = (self.units[seat], self.champs[seat]);
        // What isn't in use goes as zero: the buffs that are live, first; the path's corners; a
        // dash under way; the window Combo Breaker counts.
        let mut live = [Buff::default(); crate::units::MAX_BUFFS];
        for (to, b) in live.iter_mut().zip(u.buffs.iter().filter(|b| b.kind != 0 && b.until > self.tick)) {
            *to = *b;
        }
        u.buffs = live;
        let len = usize::from(u.path.len).min(MAX_WAYPOINTS);
        u.path.points[len..].fill(Vec2::ZERO);
        if !c.dash.active {
            c.dash = Dash::default();
        }
        if c.immobile_n == 0 {
            c.immobile = [0; 4];
        }
        let mut out = OwnState::default();
        visit(&mut u, &mut c, &mut Put { out: &mut out.0, n: 0 });
        out
    }

    /// Takes the champion of `seat` from its fields, into this (the client's) model of the match.
    pub fn apply_own(&mut self, seat: usize, s: &OwnState) -> Result<(), DecodeError> {
        let (mut u, mut c) = (self.units[seat], self.champs[seat]);
        visit(&mut u, &mut c, &mut Take { from: &s.0, n: 0 });
        if usize::from(u.path.len) > MAX_WAYPOINTS
            || usize::from(c.def) >= crate::content::champions::CHAMPIONS.len()
            || usize::from(c.n_aug) > c.augments.len()
        {
            return Err(DecodeError::Invalid);
        }
        u.kind = Kind::Champion;
        u.team = crate::units::team_of_seat(seat);
        u.radius = c.def().radius;
        self.units[seat] = u;
        self.champs[seat] = c;
        // Stats are a function of what was read: worked out, not carried, and nothing read moved.
        self.recompute_mods(seat);
        self.units[seat].stats = self.stats_of(seat);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Count(usize);

    impl Fields for Count {
        fn field(&mut self, _: &mut u32, _: u32) {
            self.0 += 1;
        }
    }

    #[test]
    fn the_fields_are_as_many_as_said_and_fit_their_widths() {
        let mut n = Count(0);
        visit(&mut Unit::default(), &mut Champ::new(0), &mut n);
        assert_eq!(n.0, OWN_FIELDS);
        assert!(own_widths().iter().all(|&w| (1..=32).contains(&w)));
    }
}
